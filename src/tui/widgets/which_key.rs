//! Which-key.nvim-style hint line that shows the current chord prefix and the
//! available continuations after it.
//!
//! This is a thin adapter over [`super::footer_hints`]: it renders the same
//! flat hint row (accent keys, dim labels, ` · ` separators) with a bold accent
//! group title. The title is the consumer-supplied name (e.g. `"Copy"`); when
//! absent the raw pending prefix is shown as `c…`.
//!
//! Layout (one line, **right-aligned** so it sits in the same place as
//! [`super::footer_hints::render_right`]):
//!
//! ```text
//!                    Copy   u  url · p  password · y  username · t  totp
//! ```
//!
//! Construction is wired through [`super::super::chord`]: the consumer feeds
//! `resolver.pending()` and `resolver.continuations(scope)` directly, plus an
//! optional human-readable group title.

use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;

use super::footer_hints;
use crate::tui::chord::{chord_label, key_label};

/// Render the which-key line at row `y` right-aligned within `[x, x + width)`.
///
/// `title` is an optional group name (e.g. `"Copy"`); when `None` the pending
/// prefix chord is shown as `c…`. `pending` is the chord prefix already entered
/// (typically `resolver.pending()`); `continuations` is the list of valid
/// next-key labels (typically `resolver.continuations(&scope)`).
pub fn render(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    title: Option<&str>,
    pending: &[KeyEvent],
    continuations: &[(KeyEvent, &str)],
) {
    if width == 0 || pending.is_empty() {
        return;
    }

    let header = match title {
        Some(t) => t.to_string(),
        None => format!("{}…", chord_label(pending)),
    };

    // Adapt the KeyEvent continuations into the hints the shared renderer
    // expects. Continuations are leaf actions, not chord leaders.
    let key_strs: Vec<String> = continuations.iter().map(|(k, _)| key_label(k)).collect();
    let hints: Vec<footer_hints::Hint> = key_strs
        .iter()
        .zip(continuations)
        .map(|(k, (_, lab))| footer_hints::Hint::action(k.as_str(), lab))
        .collect();

    footer_hints::render_hint_row(buf, x, y, width, Some(&header), &hints);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::chord::key_char;
    use ratatui::layout::{Position, Rect};

    fn render_to_string(
        width: u16,
        title: Option<&str>,
        pending: &[KeyEvent],
        conts: &[(KeyEvent, &str)],
    ) -> String {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        render(&mut buf, 0, 0, width, title, pending, conts);
        let mut out = String::new();
        for cx in 0..width {
            let cell = buf.cell(Position::new(cx, 0)).unwrap();
            out.push_str(cell.symbol());
        }
        out.trim_end().to_string()
    }

    #[test]
    fn empty_pending_renders_nothing() {
        let s = render_to_string(40, Some("Copy"), &[], &[(key_char('u'), "url")]);
        assert_eq!(s, "");
    }

    #[test]
    fn renders_title_and_continuations_right_aligned() {
        let s = render_to_string(
            60,
            Some("Copy"),
            &[key_char('c')],
            &[(key_char('u'), "url"), (key_char('p'), "password")],
        );
        assert!(s.contains("Copy"), "got {s:?}");
        assert!(s.contains("u url"), "got {s:?}");
        assert!(s.contains("p password"), "got {s:?}");
        assert!(s.contains(" · "), "expected dot separator, got {s:?}");
        assert!(s.starts_with(' '), "expected left padding, got {s:?}");
    }

    #[test]
    fn falls_back_to_prefix_when_no_title() {
        let s = render_to_string(60, None, &[key_char('c')], &[(key_char('u'), "url")]);
        assert!(s.contains("c\u{2026}"), "got {s:?}");
    }

    #[test]
    fn drops_pairs_from_the_left_when_tight() {
        // Tight width: not enough for both pairs. The rightmost pair
        // (`p password`) stays anchored; `u url` is dropped.
        let s = render_to_string(
            22,
            Some("Copy"),
            &[key_char('c')],
            &[(key_char('u'), "url"), (key_char('p'), "password")],
        );
        assert!(s.contains("p password"), "got {s:?}");
        assert!(!s.contains("u url"), "got {s:?}");
    }
}
