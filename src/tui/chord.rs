//! Vim/which-key-style chord resolver.
//!
//! Consumers register a list of [`ChordSpec`]s — each is a sequence of one or
//! more [`KeyEvent`]s mapped to a user-defined `Action` token, scoped by a
//! user-defined `Scope` token. The [`ChordResolver`] then turns each incoming
//! key into a [`ChordOutcome`]:
//!
//! - [`ChordOutcome::Resolved`] — the consumer dispatches the action.
//! - [`ChordOutcome::Pending`] — a prefix has been entered; the consumer
//!   should redraw (typically showing a which-key popup with continuations).
//! - [`ChordOutcome::Cancelled`] — a pending chord was broken (mismatched
//!   key, `Esc`, or timeout). State has been cleared.
//! - [`ChordOutcome::Passthrough`] — the key starts no chord in the current
//!   scope; the consumer is free to handle it normally.
//!
//! Scope is decided by the consumer via the [`ChordScope`] trait. Typically
//! you define an enum like `enum Pane { Any, Vaults, Items, Detail }` whose
//! `Any` variant matches every other variant.
//!
//! Length-1 chords resolve immediately on first key press *unless* a longer
//! chord shares that prefix in the current scope; in that case the resolver
//! waits for further input or the timeout (default 800 ms). On timeout, an
//! exact match (if any) commits; otherwise the chord is cancelled.
//!
//! The timeout can be disabled entirely with [`ChordResolver::persistent`], so
//! a pending prefix waits indefinitely (yazi-style) until it is completed or
//! cancelled by `Esc` / a non-matching key. That is only safe when no length-1
//! chord shares a prefix with a longer chord in the same scope.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Trait describing how a chord scope answers "is this spec active right now?".
///
/// Implementors typically have an `Any` variant that returns `true` for every
/// `current` value.
pub trait ChordScope {
    /// Returns `true` if a spec carrying `self` should be considered when the
    /// consumer is currently in the `current` scope.
    fn matches(&self, current: &Self) -> bool;
}

/// One chord: a sequence of key events plus an action and scope.
pub struct ChordSpec<A, S> {
    /// The key sequence. Length 1 = single-key shortcut.
    pub keys: Vec<KeyEvent>,
    /// Human-readable action name, shown in the which-key popup.
    pub label: &'static str,
    /// Action token returned to the consumer when the chord resolves.
    pub action: A,
    /// Scope token, matched against the current scope at dispatch time.
    pub scope: S,
}

impl<A, S> ChordSpec<A, S> {
    pub fn new(keys: Vec<KeyEvent>, label: &'static str, action: A, scope: S) -> Self {
        Self {
            keys,
            label,
            action,
            scope,
        }
    }
}

/// Outcome returned from [`ChordResolver::on_key`] / [`ChordResolver::on_tick`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChordOutcome<A> {
    /// A chord matched exactly. Consumer should dispatch `A`.
    Resolved(A),
    /// A prefix of one or more chords has been entered. Consumer should
    /// redraw to show the which-key popup; resolver state is preserved.
    Pending,
    /// A pending chord was broken (mismatched key, `Esc`, or timeout). State
    /// has been cleared.
    Cancelled,
    /// The key starts no chord in the current scope. Consumer is free to
    /// handle it any other way (e.g. ignore, or fall through to a fallback
    /// keymap).
    Passthrough(KeyEvent),
}

/// State machine for chord resolution.
pub struct ChordResolver<A: Clone, S: ChordScope> {
    specs: Vec<ChordSpec<A, S>>,
    pending: Vec<KeyEvent>,
    started_at: Option<Instant>,
    /// `None` disables the timeout: a pending prefix waits indefinitely.
    timeout: Option<Duration>,
}

impl<A: Clone, S: ChordScope> ChordResolver<A, S> {
    /// Build a resolver from a static list of specs.
    pub fn new(specs: Vec<ChordSpec<A, S>>) -> Self {
        Self {
            specs,
            pending: Vec::new(),
            started_at: None,
            timeout: Some(Duration::from_millis(800)),
        }
    }

    /// Override the chord timeout (default: 800 ms).
    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.timeout = Some(t);
        self
    }

    /// Disable the chord timeout — a pending prefix waits indefinitely until it
    /// is completed by a valid continuation or cancelled (`Esc` / a non-matching
    /// key). Only safe when no length-1 chord shares a prefix with a longer
    /// chord in the same scope, since such a shorter match would then never
    /// commit (nothing left to fire it).
    pub fn persistent(mut self) -> Self {
        self.timeout = None;
        self
    }

    /// The currently entered chord prefix (empty unless a chord is pending).
    pub fn pending(&self) -> &[KeyEvent] {
        &self.pending
    }

    /// True if a chord is currently pending (prefix has been entered).
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Reset all pending state.
    pub fn reset(&mut self) {
        self.pending.clear();
        self.started_at = None;
    }

    /// Enumerate `(next_key, label)` pairs that would extend the current
    /// pending prefix into a still-valid chord in `scope`. Useful for
    /// rendering a which-key popup.
    pub fn continuations(&self, scope: &S) -> Vec<(KeyEvent, &'static str)> {
        let mut out: Vec<(KeyEvent, &'static str)> = Vec::new();
        let prefix_len = self.pending.len();
        for spec in &self.specs {
            if !spec.scope.matches(scope) {
                continue;
            }
            if spec.keys.len() <= prefix_len {
                continue;
            }
            if !key_slice_eq(&spec.keys[..prefix_len], &self.pending) {
                continue;
            }
            let next = spec.keys[prefix_len];
            if !out.iter().any(|(k, _)| key_eq(k, &next)) {
                out.push((next, spec.label));
            }
        }
        out
    }

    /// Feed a key into the resolver.
    pub fn on_key(&mut self, key: KeyEvent, scope: &S) -> ChordOutcome<A> {
        // Special-case Esc: always cancels a pending chord. If nothing is
        // pending, fall through to normal resolution (so apps can bind Esc
        // as a chord of length 1).
        if key.code == KeyCode::Esc && !self.pending.is_empty() {
            self.reset();
            return ChordOutcome::Cancelled;
        }

        let mut tentative = self.pending.clone();
        tentative.push(key);

        let mut exact: Option<A> = None;
        let mut has_longer_prefix = false;

        for spec in &self.specs {
            if !spec.scope.matches(scope) {
                continue;
            }
            if spec.keys.len() < tentative.len() {
                continue;
            }
            if !key_slice_eq(&spec.keys[..tentative.len()], &tentative) {
                continue;
            }
            if spec.keys.len() == tentative.len() {
                if exact.is_none() {
                    exact = Some(spec.action.clone());
                }
            } else {
                has_longer_prefix = true;
            }
        }

        match (exact, has_longer_prefix) {
            (Some(action), false) => {
                // Unambiguous exact match — fire immediately.
                self.reset();
                ChordOutcome::Resolved(action)
            }
            (_, true) => {
                // A longer chord exists with this prefix — wait for more keys
                // (or for the timeout to commit any exact match we may have).
                self.pending = tentative;
                self.started_at = Some(Instant::now());
                ChordOutcome::Pending
            }
            (None, false) => {
                // No spec involves this key sequence. If we had a pending
                // prefix, it's now broken; otherwise the key was never bound.
                if self.pending.is_empty() {
                    ChordOutcome::Passthrough(key)
                } else {
                    self.reset();
                    ChordOutcome::Cancelled
                }
            }
        }
    }

    /// Tick the resolver. If a pending chord has exceeded the timeout, this
    /// returns either `Resolved` (for the longest exact match of the current
    /// pending prefix) or `Cancelled`. Returns `None` otherwise.
    pub fn on_tick(&mut self) -> Option<ChordOutcome<A>> {
        let timeout = self.timeout?;
        let started = self.started_at?;
        if started.elapsed() < timeout {
            return None;
        }
        // Timeout fired. Look for an exact match of the current pending
        // prefix (any scope — at this point the consumer's scope is the same
        // it was on the last on_key call, but we don't have it; specs we
        // matched earlier all already passed the scope filter, so an exact
        // match here is still valid by construction).
        let exact: Option<A> = self
            .specs
            .iter()
            .find(|s| s.keys.len() == self.pending.len() && key_slice_eq(&s.keys, &self.pending))
            .map(|s| s.action.clone());
        self.reset();
        Some(match exact {
            Some(a) => ChordOutcome::Resolved(a),
            None => ChordOutcome::Cancelled,
        })
    }
}

/// Build a `KeyEvent` for a plain unmodified character.
pub fn key_char(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

/// Build a `KeyEvent` for a `Ctrl`-modified character.
pub fn key_ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// Build a `KeyEvent` for a `Shift`-modified character.
pub fn key_shift_char(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT)
}

/// Build a `KeyEvent` for a non-character code (Esc, Enter, Tab, …).
pub fn key_code(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Render a single key as a short label suitable for which-key popups
/// (`"j"`, `"^N"`, `"<Esc>"`, `"↑"`, …).
pub fn key_label(key: &KeyEvent) -> String {
    let base = match key.code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Esc => "<Esc>".into(),
        KeyCode::Enter => "<Enter>".into(),
        KeyCode::Tab => "<Tab>".into(),
        KeyCode::BackTab => "<S-Tab>".into(),
        KeyCode::Backspace => "<BS>".into(),
        KeyCode::Up => "↑".into(),
        KeyCode::Down => "↓".into(),
        KeyCode::Left => "←".into(),
        KeyCode::Right => "→".into(),
        KeyCode::Home => "<Home>".into(),
        KeyCode::End => "<End>".into(),
        KeyCode::PageUp => "<PgUp>".into(),
        KeyCode::PageDown => "<PgDn>".into(),
        KeyCode::Delete => "<Del>".into(),
        KeyCode::Insert => "<Ins>".into(),
        KeyCode::F(n) => format!("<F{n}>"),
        other => format!("{other:?}"),
    };
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        format!("^{base}")
    } else {
        base
    }
}

/// Render a chord (slice of keys) as a compact label, e.g. `"cu"` or `"^N"`.
pub fn chord_label(keys: &[KeyEvent]) -> String {
    keys.iter().map(key_label).collect::<Vec<_>>().join("")
}

#[inline]
fn key_eq(a: &KeyEvent, b: &KeyEvent) -> bool {
    a.code == b.code && a.modifiers == b.modifiers
}

#[inline]
fn key_slice_eq(a: &[KeyEvent], b: &[KeyEvent]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| key_eq(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Act {
        Quit,
        Down,
        CopyUrl,
        CopyPassword,
        Reveal,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Scope {
        Any,
        Detail,
        Vaults,
    }

    impl ChordScope for Scope {
        fn matches(&self, current: &Self) -> bool {
            matches!(self, Scope::Any) || self == current
        }
    }

    fn build() -> ChordResolver<Act, Scope> {
        ChordResolver::new(vec![
            ChordSpec::new(vec![key_char('q')], "quit", Act::Quit, Scope::Any),
            ChordSpec::new(vec![key_char('j')], "down", Act::Down, Scope::Any),
            ChordSpec::new(vec![key_code(KeyCode::Down)], "down", Act::Down, Scope::Any),
            ChordSpec::new(vec![key_ctrl('n')], "down", Act::Down, Scope::Any),
            ChordSpec::new(
                vec![key_char('c'), key_char('u')],
                "copy url",
                Act::CopyUrl,
                Scope::Detail,
            ),
            ChordSpec::new(
                vec![key_char('c'), key_char('p')],
                "copy password",
                Act::CopyPassword,
                Scope::Detail,
            ),
            ChordSpec::new(vec![key_char('r')], "reveal", Act::Reveal, Scope::Detail),
        ])
    }

    #[test]
    fn single_key_resolves_immediately() {
        let mut r = build();
        assert_eq!(
            r.on_key(key_char('q'), &Scope::Any),
            ChordOutcome::Resolved(Act::Quit)
        );
        assert!(!r.is_pending());
    }

    #[test]
    fn two_key_chord_pending_then_resolves() {
        let mut r = build();
        assert_eq!(
            r.on_key(key_char('c'), &Scope::Detail),
            ChordOutcome::Pending
        );
        assert!(r.is_pending());
        assert_eq!(
            r.on_key(key_char('u'), &Scope::Detail),
            ChordOutcome::Resolved(Act::CopyUrl)
        );
        assert!(!r.is_pending());
    }

    #[test]
    fn passthrough_for_unbound_key() {
        let mut r = build();
        match r.on_key(key_char('z'), &Scope::Any) {
            ChordOutcome::Passthrough(k) => assert_eq!(k.code, KeyCode::Char('z')),
            other => panic!("expected Passthrough, got {other:?}"),
        }
    }

    #[test]
    fn esc_cancels_pending() {
        let mut r = build();
        assert_eq!(
            r.on_key(key_char('c'), &Scope::Detail),
            ChordOutcome::Pending
        );
        assert_eq!(
            r.on_key(key_code(KeyCode::Esc), &Scope::Detail),
            ChordOutcome::Cancelled
        );
        assert!(!r.is_pending());
    }

    #[test]
    fn mismatched_key_after_prefix_cancels() {
        let mut r = build();
        assert_eq!(
            r.on_key(key_char('c'), &Scope::Detail),
            ChordOutcome::Pending
        );
        assert_eq!(
            r.on_key(key_char('z'), &Scope::Detail),
            ChordOutcome::Cancelled
        );
        assert!(!r.is_pending());
    }

    #[test]
    fn scope_filters_chord() {
        let mut r = build();
        // 'r' is Detail-only — outside Detail it passes through.
        match r.on_key(key_char('r'), &Scope::Vaults) {
            ChordOutcome::Passthrough(k) => assert_eq!(k.code, KeyCode::Char('r')),
            other => panic!("expected Passthrough, got {other:?}"),
        }
        // In Detail it resolves to Reveal.
        assert_eq!(
            r.on_key(key_char('r'), &Scope::Detail),
            ChordOutcome::Resolved(Act::Reveal)
        );
    }

    #[test]
    fn aliases_resolve_same_action() {
        let mut r = build();
        assert_eq!(
            r.on_key(key_char('j'), &Scope::Any),
            ChordOutcome::Resolved(Act::Down)
        );
        assert_eq!(
            r.on_key(key_code(KeyCode::Down), &Scope::Any),
            ChordOutcome::Resolved(Act::Down)
        );
        assert_eq!(
            r.on_key(key_ctrl('n'), &Scope::Any),
            ChordOutcome::Resolved(Act::Down)
        );
    }

    #[test]
    fn continuations_lists_next_keys() {
        let mut r = build();
        assert_eq!(
            r.on_key(key_char('c'), &Scope::Detail),
            ChordOutcome::Pending
        );
        let conts = r.continuations(&Scope::Detail);
        let codes: Vec<KeyCode> = conts.iter().map(|(k, _)| k.code).collect();
        assert!(codes.contains(&KeyCode::Char('u')));
        assert!(codes.contains(&KeyCode::Char('p')));
        assert_eq!(codes.len(), 2);
    }

    #[test]
    fn timeout_cancels_pending_with_no_exact_match() {
        let mut r = ChordResolver::new(vec![ChordSpec::new(
            vec![key_char('c'), key_char('u')],
            "cu",
            Act::CopyUrl,
            Scope::Any,
        )])
        .with_timeout(Duration::from_millis(10));
        assert_eq!(r.on_key(key_char('c'), &Scope::Any), ChordOutcome::Pending);
        std::thread::sleep(Duration::from_millis(25));
        assert_eq!(r.on_tick(), Some(ChordOutcome::Cancelled));
        assert!(!r.is_pending());
    }

    #[test]
    fn timeout_commits_exact_match_when_longer_chord_is_pending() {
        // Bind both `g` (jump-top) and `gg` (something else) so `g` alone is
        // a length-1 exact match while `gg` is also a valid prefix. After the
        // timeout, the shorter exact match should fire.
        let mut r = ChordResolver::new(vec![
            ChordSpec::new(vec![key_char('g')], "top", Act::Down, Scope::Any),
            ChordSpec::new(
                vec![key_char('g'), key_char('g')],
                "gg",
                Act::Quit,
                Scope::Any,
            ),
        ])
        .with_timeout(Duration::from_millis(10));
        assert_eq!(r.on_key(key_char('g'), &Scope::Any), ChordOutcome::Pending);
        std::thread::sleep(Duration::from_millis(25));
        assert_eq!(r.on_tick(), Some(ChordOutcome::Resolved(Act::Down)));
    }

    #[test]
    fn persistent_prefix_never_times_out() {
        let mut r = ChordResolver::new(vec![ChordSpec::new(
            vec![key_char('c'), key_char('u')],
            "cu",
            Act::CopyUrl,
            Scope::Any,
        )])
        .persistent();
        assert_eq!(r.on_key(key_char('c'), &Scope::Any), ChordOutcome::Pending);
        // No timeout: ticking (even after a delay) never cancels the prefix.
        std::thread::sleep(Duration::from_millis(25));
        assert_eq!(r.on_tick(), None);
        assert!(r.is_pending());
        assert_eq!(r.on_tick(), None);
        assert!(r.is_pending());
        // The continuation still completes the chord.
        assert_eq!(
            r.on_key(key_char('u'), &Scope::Any),
            ChordOutcome::Resolved(Act::CopyUrl)
        );
        assert!(!r.is_pending());
    }
}
