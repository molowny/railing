//! High-level UI primitives: the failure line, spinners, intro/outro.
//!
//! Wraps `cliclack` so downstream crates never import it directly.

use console::style;

use crate::error::CliError;

// ---------------------------------------------------------------------------
// The failure line
// ---------------------------------------------------------------------------

/// `✖ msg` in red on stderr: how a command fails when no frame is open
/// (plain output, or before the flow began). Inside a frame the failure is
/// the flow's closing line instead ([`crate::flow::render_error`]). Everything
/// else a command says goes through [`crate::Flow`], in the gutter.
pub fn error(msg: &str) {
    eprintln!(
        "{} {}",
        style("✖").for_stderr().red(),
        style(msg).for_stderr().red()
    );
}

// ---------------------------------------------------------------------------
// Spinner
// ---------------------------------------------------------------------------

struct SpinnerState {
    msg: std::sync::Mutex<String>,
    done: std::sync::atomic::AtomicBool,
}

/// A spinner on its own line while something slow runs: [`crate::spin`]'s
/// turning star and glow across the message. [`Spinner::stop`] leaves `◇ msg` in
/// its place, [`Spinner::clear`] (or dropping it) leaves nothing. Off a
/// terminal nothing animates.
pub struct Spinner {
    state: std::sync::Arc<SpinnerState>,
    animator: Option<std::thread::JoinHandle<()>>,
}

impl Spinner {
    /// Create and start a spinner with the given message.
    pub fn start(msg: &str) -> Self {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let state = Arc::new(SpinnerState {
            msg: std::sync::Mutex::new(msg.to_owned()),
            done: AtomicBool::new(false),
        });
        let term = console::Term::stderr();
        let animator = term.is_term().then(|| {
            let state = Arc::clone(&state);
            std::thread::spawn(move || {
                let _ = term.hide_cursor();
                let mut spin = crate::spin::Spin::new();
                while !state.done.load(Ordering::SeqCst) {
                    let msg = state.msg.lock().map(|m| m.clone()).unwrap_or_default();
                    let cols = usize::from(term.size().1);
                    let _ = term.write_str(&format!("\r\x1b[2K{}", spin.line(&msg, cols)));
                    spin.advance();
                    std::thread::sleep(crate::spin::PACE);
                }
                let _ = term.write_str("\r\x1b[2K");
                let _ = term.show_cursor();
            })
        });
        Self { state, animator }
    }

    /// Update the spinner message.
    pub fn update(&self, msg: &str) {
        if let Ok(mut m) = self.state.msg.lock() {
            *m = msg.to_owned();
        }
    }

    /// Stop the spinner and leave `◇ msg` (and the gutter's bar) in its place.
    pub fn stop(mut self, msg: &str) {
        use cliclack::{Theme, ThemeState};
        self.halt();
        let theme = crate::grid::Clack;
        let bar = theme
            .bar_color(&ThemeState::Submit)
            .for_stderr()
            .apply_to("│");
        let _ = console::Term::stderr().write_str(&format!(
            "{}  {msg}\n{bar}\n",
            theme.state_symbol(&ThemeState::Submit)
        ));
    }

    /// Clear the spinner without leaving any output.
    pub fn clear(mut self) {
        self.halt();
    }

    /// Stop animating and erase the line; waits for the last frame so
    /// nothing is drawn after it.
    fn halt(&mut self) {
        self.state
            .done
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(h) = self.animator.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.halt();
    }
}

// ---------------------------------------------------------------------------
// Intro / Outro
// ---------------------------------------------------------------------------

/// Show a styled intro banner.
pub fn intro(title: &str) -> Result<(), CliError> {
    cliclack::intro(title.to_string()).map_err(|e| CliError::Internal(anyhow::anyhow!(e)))
}

/// Show a styled outro message (neutral).
pub fn outro(msg: &str) -> Result<(), CliError> {
    cliclack::outro(msg.to_string()).map_err(|e| CliError::Internal(anyhow::anyhow!(e)))
}

/// Show a green success outro that closes the prompt group.
pub fn outro_success(msg: &str) -> Result<(), CliError> {
    cliclack::outro(format!("{} {}", style("✔").green(), style(msg).green()))
        .map_err(|e| CliError::Internal(anyhow::anyhow!(e)))
}

/// Show a red error outro that closes the prompt group.
pub fn outro_error(msg: &str) -> Result<(), CliError> {
    cliclack::outro_cancel(format!("{} {}", style("✖").red(), style(msg).red()))
        .map_err(|e| CliError::Internal(anyhow::anyhow!(e)))
}

/// Show a yellow warning outro that closes the prompt group.
pub fn outro_warning(msg: &str) -> Result<(), CliError> {
    cliclack::outro(format!("{} {}", style("⚠").yellow(), style(msg).yellow()))
        .map_err(|e| CliError::Internal(anyhow::anyhow!(e)))
}

// ---------------------------------------------------------------------------
// Inside a prompt group: notes and gutter log lines
// ---------------------------------------------------------------------------
//
// Between `intro` and `outro`, plain `println!` breaks the `│` gutter that
// cliclack draws. These render inside it: a boxed note for a block of text
// and one-line log entries with the group's own symbols.

fn io_err(e: std::io::Error) -> CliError {
    CliError::Internal(anyhow::anyhow!(e))
}

/// A titled box of prose inside the prompt group (`◇ title ─╮ … ├─╯`), its
/// body dimmed — the same frame as every table and list. Lines longer than
/// the terminal are word-wrapped so the box always fits.
pub fn note(title: &str, body: &str) -> Result<(), CliError> {
    let cols = console::Term::stderr().size().1 as usize;
    let width = cols.saturating_sub(8).clamp(40, 120);
    let body = wrap_to(body, width)
        .lines()
        .map(|l| dim().for_stderr().apply_to(l).to_string())
        .collect::<Vec<_>>()
        .join("\n");
    console::Term::stderr()
        .write_str(&crate::frame::Frame::boxed(title, &body))
        .map_err(io_err)
}

/// Word-wrap every line of `text` to `width` visible columns (ANSI styling is
/// not counted). Existing line breaks are kept; leading indentation of a
/// wrapped line is carried onto its continuations. A word longer than the
/// line (one-line XML, a long path) is broken mid-word, so nothing drawn in a
/// box runs past it; a styled word is left whole rather than cut through its
/// escape codes.
pub fn wrap_to(text: &str, width: usize) -> String {
    let mut out = Vec::new();
    for line in text.lines() {
        if console::measure_text_width(line) <= width {
            out.push(line.to_string());
            continue;
        }
        let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        let mut cur = indent.clone();
        let mut cur_w = console::measure_text_width(&cur);
        let room = width.saturating_sub(indent.len()).max(1);
        let words = line.split_whitespace().flat_map(|word| {
            let plain = !word.contains('\x1b');
            if plain && console::measure_text_width(word) > room {
                let chars: Vec<char> = word.chars().collect();
                chars
                    .chunks(room)
                    .map(|c| c.iter().collect::<String>())
                    .collect::<Vec<_>>()
            } else {
                vec![word.to_string()]
            }
        });
        for word in words {
            let word = word.as_str();
            let w = console::measure_text_width(word);
            if cur_w > indent.len() && cur_w + 1 + w > width {
                out.push(std::mem::replace(&mut cur, indent.clone()));
                cur_w = indent.len();
            }
            if cur_w > indent.len() {
                cur.push(' ');
                cur_w += 1;
            }
            cur.push_str(word);
            cur_w += w;
        }
        out.push(cur);
    }
    out.join("\n")
}

/// One-line log entries drawn with the prompt group's gutter.
pub mod log {
    use super::{CliError, io_err};

    /// A completed step (`◇`).
    pub fn step(msg: &str) -> Result<(), CliError> {
        cliclack::log::step(msg).map_err(io_err)
    }

    /// An informational line (`●`).
    pub fn info(msg: &str) -> Result<(), CliError> {
        cliclack::log::info(msg).map_err(io_err)
    }

    /// A dim remark (`│`).
    pub fn remark(msg: &str) -> Result<(), CliError> {
        cliclack::log::remark(msg).map_err(io_err)
    }

    pub fn success(msg: &str) -> Result<(), CliError> {
        cliclack::log::success(msg).map_err(io_err)
    }

    pub fn warning(msg: &str) -> Result<(), CliError> {
        cliclack::log::warning(msg).map_err(io_err)
    }

    pub fn error(msg: &str) -> Result<(), CliError> {
        cliclack::log::error(msg).map_err(io_err)
    }
}

/// Streams text inside the prompt group: every line starts with the gutter
/// bar, however the text arrives (a token at a time is fine). Call
/// [`Gutter::finish`] before printing anything else so the last line is closed.
#[derive(Default)]
pub struct Gutter {
    at_line_start: bool,
    open: bool,
}

impl Gutter {
    pub fn new() -> Self {
        Self {
            at_line_start: true,
            open: false,
        }
    }

    /// Whether a line is currently open (text written without a trailing newline).
    pub fn mid_line(&self) -> bool {
        self.open && !self.at_line_start
    }

    #[allow(clippy::print_stdout)]
    pub fn write(&mut self, delta: &str) {
        use std::io::Write;
        let bar = style("│").color256(8);
        let mut out = std::io::stdout().lock();
        for part in delta.split_inclusive('\n') {
            if self.at_line_start {
                let _ = write!(out, "{bar}  ");
            }
            let _ = write!(out, "{part}");
            self.at_line_start = part.ends_with('\n');
            self.open = true;
        }
        let _ = out.flush();
    }

    /// Close an open line and leave the group's usual empty `│` line.
    #[allow(clippy::print_stdout)]
    pub fn finish(&mut self) {
        if !self.open {
            return;
        }
        if !self.at_line_start {
            println!();
        }
        println!("{}", style("│").color256(8));
        self.at_line_start = true;
        self.open = false;
    }
}

// ---------------------------------------------------------------------------
// Color detection
// ---------------------------------------------------------------------------

/// Configure `console`'s color globals based on our own `IsTerminal` check.
///
/// The upstream `console` crate's auto-detection has been observed to
/// misfire inside shell hook contexts (notably fish `--on-event` /
/// `--on-variable` handlers, where stderr inheritance is non-obvious),
/// silently dropping color from error renders, ambient notices, and
/// table output. Calling this once early in `main()` overrides
/// console's inference with a direct `IsTerminal` check on stdout/stderr,
/// honoring `NO_COLOR` when set.
///
/// `cli_kit::logging::init` calls this automatically — most binaries
/// don't need to invoke it explicitly.
pub fn init_colors() {
    use std::io::IsTerminal;
    let no_color = std::env::var_os("NO_COLOR").is_some();
    console::set_colors_enabled(std::io::stdout().is_terminal() && !no_color);
    console::set_colors_enabled_stderr(std::io::stderr().is_terminal() && !no_color);
}

// ---------------------------------------------------------------------------
// Ambient notices (direnv-style)
// ---------------------------------------------------------------------------
//
// For tools that emit "what just happened" status lines around shell hooks
// or background activity. Output goes to **stderr** so it never interferes
// with `eval`/`source` of stdout. Color codes are gated by `console`'s
// stderr-color global, which `init_colors` configures correctly at startup.

/// 256-colour index of dim text, everywhere cli-kit draws it: CLI output,
/// flows, grids, and the TUI's `theme::FG_DIM`.
///
/// A grayscale-ramp entry (`#949494`) rather than SGR 2 "faint", which
/// terminals render anywhere from invisible to identical to normal text, or
/// palette `DarkGray`, whose shade is up to the terminal theme. Readable on
/// dark backgrounds and still clearly below normal text on light ones.
pub const DIM_256: u8 = 246;

/// Style for dim text: [`DIM_256`]. Add `.for_stderr()` for stderr-bound
/// output, so colour follows that stream's flag.
pub fn dim() -> console::Style {
    console::Style::new().color256(DIM_256)
}

/// Emit a tagged status line to stderr: `{tag}: {body}`.
///
/// The tag is dimmed (so the eye lands on the body). Use this for
/// direnv-style activity messages — env-var changes on `cd`, "vault locked"
/// reminders, "loading config" hints. Designed to be safe to call from
/// shell hooks because output is stderr-only.
pub fn notice(tag: &str, body: &str) {
    eprintln!(
        "{} {}",
        dim().for_stderr().apply_to(format!("{tag}:")),
        body
    );
}

/// Render a `+name` token in green. Use when listing items that were
/// added in a transition (e.g. env vars set by direnv-style activate).
///
/// Designed to be embedded in stderr-bound output (e.g. via [`notice`]),
/// so the color decision uses the **stderr** color flag — important
/// because shell hooks are typically invoked with stdout piped.
pub fn diff_added(name: &str) -> String {
    format!(
        "{}{}",
        style("+").for_stderr().green(),
        style(name).for_stderr().green()
    )
}

/// Render a `-name` token in red. Use when listing items that were removed.
///
/// Stderr-bound; see [`diff_added`] for color-flag rationale.
pub fn diff_removed(name: &str) -> String {
    format!(
        "{}{}",
        style("-").for_stderr().red(),
        style(name).for_stderr().red()
    )
}

/// Render `(detail)` in dim style. Use as a trailing hint on notice lines —
/// e.g. the path of the config file that drove the transition.
///
/// Stderr-bound; see [`diff_added`] for color-flag rationale.
pub fn dim_hint(detail: &str) -> String {
    dim()
        .for_stderr()
        .apply_to(format!("({detail})"))
        .to_string()
}

/// Render a warning body in yellow. Use as the body of a [`notice_warn`]
/// or compose into a longer stderr message that includes a non-fatal
/// warning fragment.
///
/// Stderr-bound; see [`diff_added`] for color-flag rationale.
pub fn warn_body(body: &str) -> String {
    style(body).for_stderr().yellow().to_string()
}

/// Like [`notice`] but the body is rendered yellow — for non-fatal
/// warnings: failed-but-skipped lookups, deprecation hints, partial
/// success messages. Output is stderr-only.
///
/// ```text
/// sealward: FOO: ipc: field not found — skipping
/// └─ dim     └─ yellow
/// ```
pub fn notice_warn(tag: &str, body: &str) {
    eprintln!(
        "{} {}",
        dim().for_stderr().apply_to(format!("{tag}:")),
        warn_body(body)
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests here and in `grid` mutate `console`'s global color flags and
    /// `NO_COLOR`. Serialize them through this lock so parallel `cargo test`
    /// can't observe inconsistent state. Poisoning is ignored — once one
    /// test panics, the others might double-fail but that's strictly
    /// better than masking the original failure.
    pub(crate) fn lock() -> std::sync::MutexGuard<'static, ()> {
        static M: Mutex<()> = Mutex::new(());
        M.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// ANSI Control Sequence Introducer (ESC `[`). Reliable marker that
    /// some color escape was emitted.
    const CSI: &str = "\x1b[";

    // ---- warn_body / notice_warn (yellow-bodied notices) -----------------

    #[test]
    fn warn_body_emits_yellow_when_stderr_color_on() {
        let _g = lock();
        console::set_colors_enabled(false); // hook-env case: stdout piped
        console::set_colors_enabled_stderr(true);
        let out = warn_body("ONLY_HERE: field not found — skipping");
        assert!(out.contains(CSI), "got: {out:?}");
        assert!(out.contains("ONLY_HERE"), "body content preserved: {out:?}");
    }

    #[test]
    fn warn_body_is_plain_when_stderr_color_off() {
        let _g = lock();
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
        assert_eq!(warn_body("hello"), "hello");
    }

    #[test]
    fn notice_warn_does_not_panic_in_either_color_state() {
        let _g = lock();
        for &on in &[true, false] {
            console::set_colors_enabled_stderr(on);
            notice_warn("sealward", "FOO: skipping");
        }
    }

    // ---- diff helpers: color-on vs color-off state -----------------------

    #[test]
    fn diff_added_emits_ansi_when_colors_on() {
        let _g = lock();
        console::set_colors_enabled(true);
        console::set_colors_enabled_stderr(true);
        let out = diff_added("FOO");
        assert!(out.contains(CSI), "should contain CSI: {out:?}");
        assert!(out.contains('+'));
        assert!(out.contains("FOO"));
    }

    /// Regression: `sealward hook-env -s fish | source` has stdout piped
    /// (not a TTY) but stderr still attached to the user's terminal. The
    /// stderr-bound helpers (notice, diff_added, diff_removed, dim_hint)
    /// must check the **stderr** color flag, not stdout's.
    ///
    /// Reproduces the bug the user observed: error notices and `+VAR`
    /// transition messages displaying without color in fish.
    #[test]
    fn diff_added_respects_stderr_flag_when_stdout_piped() {
        let _g = lock();
        console::set_colors_enabled(false); // stdout: piped, no color
        console::set_colors_enabled_stderr(true); // stderr: TTY, should color
        let out = diff_added("FOO");
        assert!(
            out.contains(CSI),
            "diff_added is meant for stderr; must respect stderr color flag. got: {out:?}"
        );
    }

    #[test]
    fn diff_removed_respects_stderr_flag_when_stdout_piped() {
        let _g = lock();
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(true);
        let out = diff_removed("FOO");
        assert!(out.contains(CSI), "got: {out:?}");
    }

    #[test]
    fn dim_hint_respects_stderr_flag_when_stdout_piped() {
        let _g = lock();
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(true);
        let out = dim_hint("~/p");
        assert!(out.contains(CSI), "got: {out:?}");
    }

    #[test]
    fn diff_added_is_plain_when_colors_off() {
        let _g = lock();
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
        let out = diff_added("FOO");
        assert_eq!(out, "+FOO", "no ANSI when disabled: {out:?}");
    }

    #[test]
    fn diff_removed_emits_ansi_when_colors_on() {
        let _g = lock();
        console::set_colors_enabled(true);
        console::set_colors_enabled_stderr(true);
        let out = diff_removed("BAR");
        assert!(out.contains(CSI), "{out:?}");
        assert!(out.contains('-'));
        assert!(out.contains("BAR"));
    }

    #[test]
    fn diff_removed_is_plain_when_colors_off() {
        let _g = lock();
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
        assert_eq!(diff_removed("BAR"), "-BAR");
    }

    #[test]
    fn dim_hint_emits_ansi_when_colors_on() {
        let _g = lock();
        console::set_colors_enabled(true);
        console::set_colors_enabled_stderr(true);
        let out = dim_hint("~/p");
        assert!(out.contains(CSI), "{out:?}");
        assert!(out.contains("(~/p)"));
    }

    #[test]
    fn dim_hint_is_plain_when_colors_off() {
        let _g = lock();
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
        assert_eq!(dim_hint("~/p"), "(~/p)");
    }

    // ---- init_colors honors NO_COLOR -------------------------------------

    #[test]
    fn init_colors_disables_when_no_color_set() {
        let _g = lock();
        // SAFETY: NO_COLOR mutation is serialized via `lock()`.
        unsafe {
            std::env::set_var("NO_COLOR", "1");
        }
        init_colors();
        // Whatever stdout/stderr is in the test harness, NO_COLOR forces off.
        assert!(!console::colors_enabled());
        assert!(!console::colors_enabled_stderr());
        unsafe {
            std::env::remove_var("NO_COLOR");
        }
    }

    #[test]
    fn init_colors_no_color_takes_priority_over_tty() {
        let _g = lock();
        // Even if we pre-force colors on, NO_COLOR must turn them off.
        console::set_colors_enabled(true);
        console::set_colors_enabled_stderr(true);
        unsafe {
            std::env::set_var("NO_COLOR", "anything");
        }
        init_colors();
        assert!(!console::colors_enabled());
        assert!(!console::colors_enabled_stderr());
        unsafe {
            std::env::remove_var("NO_COLOR");
        }
    }

    #[test]
    fn init_colors_without_no_color_matches_isterminal_check() {
        use std::io::IsTerminal;
        let _g = lock();
        unsafe {
            std::env::remove_var("NO_COLOR");
        }
        init_colors();
        // After init_colors, the global flags equal what we'd compute from
        // IsTerminal — which is what `style()` callers depend on.
        assert_eq!(console::colors_enabled(), std::io::stdout().is_terminal());
        assert_eq!(
            console::colors_enabled_stderr(),
            std::io::stderr().is_terminal()
        );
    }

    // ---- notice / alerts: smoke tests (just verify no panic) -------------
    //
    // notice() / success() / warning() / info() / error() write to stdio
    // streams; we can't capture cleanly in unit tests without restructuring.
    // These smoke tests at least verify the code paths don't panic in
    // either color state.

    #[test]
    fn notice_does_not_panic_in_either_color_state() {
        let _g = lock();
        console::set_colors_enabled_stderr(true);
        notice("test", "with color");
        console::set_colors_enabled_stderr(false);
        notice("test", "without color");
    }

    #[test]
    fn the_failure_line_does_not_panic_in_either_color_state() {
        let _g = lock();
        for &on in &[true, false] {
            console::set_colors_enabled_stderr(on);
            error("fail");
        }
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::wrap_to;

    #[test]
    fn breaks_a_word_longer_than_the_line() {
        let w = wrap_to("<a><b>0123456789012345678901234</b></a> tail", 10);
        assert!(w.lines().all(|l| l.chars().count() <= 10), "{w}");
        assert_eq!(
            w.replace('\n', ""),
            "<a><b>0123456789012345678901234</b></a>tail"
        );
    }

    #[test]
    fn wraps_long_lines_and_keeps_short_ones() {
        let t = "short\n  a long indented line that must wrap somewhere sensible";
        let w = wrap_to(t, 30);
        assert_eq!(
            w,
            "short\n  a long indented line that\n  must wrap somewhere sensible"
        );
        assert!(w.lines().all(|l| l.chars().count() <= 30));
    }
}
