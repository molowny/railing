//! High-level UI primitives: the failure line, spinners, intro/outro.
//!
//! The group's lines are [`crate::group`]'s, written on stderr in its colours.

use crate::color::style;

use crate::error::CliError;
use crate::group;

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

pub use crate::spin::{SpinnerStyle, set_spinner_style, spinner_style};

struct SpinnerState {
    msg: std::sync::Mutex<String>,
    done: std::sync::atomic::AtomicBool,
}

/// A spinner on its own line while something slow runs: the pulsing shape
/// ([`SpinnerStyle`]) and a glow across the message. [`Spinner::stop`] leaves `◇ msg` in
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
        let on_terminal = std::io::IsTerminal::is_terminal(&std::io::stderr());
        let animator = on_terminal.then(|| {
            let state = Arc::clone(&state);
            std::thread::spawn(move || {
                let _ = stderr_write("\x1b[?25l");
                let spin = crate::spin::Spin::new();
                while !state.done.load(Ordering::SeqCst) {
                    let msg = state.msg.lock().map(|m| m.clone()).unwrap_or_default();
                    let cols = stderr_size().1;
                    let _ = stderr_write(&format!(
                        "\r\x1b[2K{}",
                        spin.line(stderr_caps(), &msg, cols)
                    ));
                    std::thread::sleep(crate::spin::PACE);
                }
                let _ = stderr_write("\r\x1b[2K\x1b[?25h");
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
        use crate::glyph::{self, State};
        self.halt();
        let _ = stderr_write(&format!(
            "{}  {msg}\n{}\n",
            glyph::step(stderr_caps(), State::Submit),
            glyph::bar(stderr_caps(), State::Submit)
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

fn write_stderr(text: &str) -> Result<(), CliError> {
    stderr_write(text).map_err(io_err)
}

/// Write `text` on stderr, whole, and flush it.
pub(crate) fn stderr_write(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut err = std::io::stderr().lock();
    err.write_all(text.as_bytes())?;
    err.flush()
}

/// stdout's width, when it is a terminal that says: what a plain table is
/// fitted to.
#[cfg(feature = "table")]
pub(crate) fn stdout_cols() -> Option<usize> {
    #[cfg(unix)]
    {
        use std::os::fd::AsFd;
        let fd = std::io::stdout().as_fd().try_clone_to_owned().ok()?;
        let tty = crate::term::Tty::from_fd(fd, &crate::style::TermEnv::default());
        tty.is_terminal()
            .then(|| tty.size())
            .flatten()
            .map(|s| usize::from(s.cols))
    }
    #[cfg(not(unix))]
    None
}

/// stderr's size as `(rows, cols)`, when it is a terminal that says.
pub(crate) fn stderr_size_checked() -> Option<(usize, usize)> {
    #[cfg(unix)]
    {
        crate::term::Tty::stderr()
            .ok()
            .and_then(|t| t.size())
            .map(|s| (usize::from(s.rows), usize::from(s.cols)))
    }
    #[cfg(not(unix))]
    None
}

/// stderr's size as `(rows, cols)`; 24×79 when it cannot say, the fallback
/// console had, kept so that nothing drawn off a terminal changes.
pub fn stderr_size() -> (usize, usize) {
    stderr_size_checked().unwrap_or((24, 79))
}

/// `mark msg`, both in `style`: how a closing line says how it went.
fn verdict(mark: &str, msg: &str, style: crate::style::Style) -> String {
    let caps = stderr_caps();
    format!("{} {}", style.paint(caps, mark), style.paint(caps, msg))
}

/// Show a styled intro banner.
pub fn intro(title: &str) -> Result<(), CliError> {
    write_stderr(&group::open(stderr_caps(), title))
}

/// Show a styled outro message (neutral).
pub fn outro(msg: &str) -> Result<(), CliError> {
    write_stderr(&group::close(stderr_caps(), msg))
}

/// Show a green success outro that closes the prompt group.
pub fn outro_success(msg: &str) -> Result<(), CliError> {
    let line = verdict("✔", msg, crate::style::Style::new().green());
    write_stderr(&group::close(stderr_caps(), &line))
}

/// Show a red error outro that closes the prompt group.
pub fn outro_error(msg: &str) -> Result<(), CliError> {
    let line = verdict("✖", msg, crate::style::Style::new().red());
    write_stderr(&group::close(stderr_caps(), &line))
}

/// Show a yellow warning outro that closes the prompt group.
pub fn outro_warning(msg: &str) -> Result<(), CliError> {
    let line = verdict("⚠", msg, crate::style::Style::new().yellow());
    write_stderr(&group::close(stderr_caps(), &line))
}

/// Close the group as given up: `└  msg`, the message red.
pub fn outro_cancel(msg: &str) -> Result<(), CliError> {
    write_stderr(&group::close_cancelled(stderr_caps(), msg))
}

// ---------------------------------------------------------------------------
// Inside a prompt group: notes and gutter log lines
// ---------------------------------------------------------------------------
//
// Between `intro` and `outro`, plain `println!` breaks the group's `│`
// gutter. These render inside it: a boxed note for a block of text
// and one-line log entries with the group's own symbols.

fn io_err(e: std::io::Error) -> CliError {
    CliError::internal(e)
}

/// A titled box of prose inside the prompt group (`◇ title ─╮ … ├─╯`), its
/// body dimmed — the same frame as every table and list. Lines longer than
/// the terminal are word-wrapped so the box always fits.
pub fn note(title: &str, body: &str) -> Result<(), CliError> {
    let cols = stderr_size().1;
    let width = cols.saturating_sub(8).clamp(40, 120);
    let body = wrap_to(body, width)
        .lines()
        .map(|l| dim().for_stderr().apply_to(l).to_string())
        .collect::<Vec<_>>()
        .join("\n");
    write_stderr(&crate::frame::Frame::boxed(title, &body))
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
        if crate::style::measure(line) <= width {
            out.push(line.to_string());
            continue;
        }
        let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        let indent_w = crate::style::measure(&indent);
        let mut cur = indent.clone();
        let mut cur_w = indent_w;
        let room = width.saturating_sub(indent_w).max(1);
        let words = line.split_whitespace().flat_map(|word| {
            let plain = !word.contains('\x1b');
            if plain && crate::style::measure(word) > room {
                break_word(word, room)
            } else {
                vec![word.to_string()]
            }
        });
        for word in words {
            let word = word.as_str();
            let w = crate::style::measure(word);
            if cur_w > indent_w && cur_w + 1 + w > width {
                out.push(std::mem::replace(&mut cur, indent.clone()));
                cur_w = indent_w;
            }
            if cur_w > indent_w {
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

/// A plain `word` in pieces at most `room` columns wide (a character wider
/// than `room` a piece of its own): nothing dropped, wide characters
/// counted as the columns they take.
fn break_word(word: &str, room: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0;
    for c in word.chars() {
        let w = crate::style::measure(c.encode_utf8(&mut [0; 4]));
        if cur_w > 0 && cur_w + w > room {
            pieces.push(std::mem::take(&mut cur));
            cur_w = 0;
        }
        cur.push(c);
        cur_w += w;
    }
    if !cur.is_empty() {
        pieces.push(cur);
    }
    pieces
}

/// One-line log entries drawn with the prompt group's gutter
/// ([`group::log`]).
pub mod log {
    use super::{CliError, stderr_caps, write_stderr};
    use crate::group::{self, Mark};

    fn entry(mark: Mark, msg: &str) -> Result<(), CliError> {
        write_stderr(&group::log(stderr_caps(), mark, msg))
    }

    /// A completed step (`◇`).
    pub fn step(msg: &str) -> Result<(), CliError> {
        entry(Mark::Step, msg)
    }

    /// An informational line (`●`).
    pub fn info(msg: &str) -> Result<(), CliError> {
        entry(Mark::Info, msg)
    }

    /// A remark (`├`).
    pub fn remark(msg: &str) -> Result<(), CliError> {
        entry(Mark::Remark, msg)
    }

    pub fn success(msg: &str) -> Result<(), CliError> {
        entry(Mark::Success, msg)
    }

    pub fn warning(msg: &str) -> Result<(), CliError> {
        entry(Mark::Warning, msg)
    }

    pub fn error(msg: &str) -> Result<(), CliError> {
        entry(Mark::Error, msg)
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

/// Decide colour for stdout and stderr ([`crate::color`]) from a direct
/// `IsTerminal` check on each, off when `NO_COLOR` is set at all.
///
/// console's auto-detection, which the kit used until the core refactor's
/// M5, was observed to misfire inside shell hook contexts (fish
/// `--on-event` / `--on-variable` handlers, where stderr inheritance is
/// non-obvious), silently dropping colour from error renders, ambient
/// notices and tables. Calling this once early in `main()` makes the
/// decision explicit; without it each stream detects on first use.
///
/// `cli_kit::logging::init` calls this automatically — most binaries
/// don't need to invoke it explicitly.
pub fn init_colors() {
    use crate::color::{Stream, set_stream};
    use std::io::IsTerminal;
    let no_color = std::env::var_os("NO_COLOR").is_some();
    set_stream(Stream::Stdout, std::io::stdout().is_terminal() && !no_color);
    set_stream(Stream::Stderr, std::io::stderr().is_terminal() && !no_color);
}

// ---------------------------------------------------------------------------
// Ambient notices (direnv-style)
// ---------------------------------------------------------------------------
//
// For tools that emit "what just happened" status lines around shell hooks
// or background activity. Output goes to **stderr** so it never interferes
// with `eval`/`source` of stdout. Colour follows stderr's decision
// (`crate::color`), which `init_colors` makes at startup.

pub use crate::glyph::DIM_256;

/// The caps the full kit draws on stderr with: colour as stderr's decision
/// says ([`crate::color`], [`init_colors`]), cursor movement on.
pub(crate) fn stderr_caps() -> crate::style::Caps {
    crate::color::caps(crate::color::Stream::Stderr)
}

/// The terminal this command inherited ([`crate::term::Tty::stderr`]), its
/// colour as stderr's decision says ([`stderr_caps`]) rather than its own
/// detection, so a prompt agrees with the flow around it: what every prompt
/// and list the full kit draws asks on.
#[cfg(unix)]
pub(crate) fn stderr_tty() -> std::io::Result<crate::term::Tty> {
    let tty = crate::term::Tty::stderr()?;
    let caps = crate::style::Caps {
        color: stderr_caps().color,
        ..tty.caps()
    };
    Ok(tty.with_caps(caps))
}

/// Style for dim text: [`DIM_256`]. Add `.for_stderr()` for stderr-bound
/// output, so colour follows that stream's decision.
pub fn dim() -> crate::color::Style {
    crate::color::Style::new().color256(DIM_256)
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

    /// Tests here and elsewhere set the process's colour decision and
    /// `NO_COLOR`: one lock for all of them ([`crate::color`]'s).
    pub(crate) fn lock() -> std::sync::MutexGuard<'static, ()> {
        crate::color::tests::lock()
    }

    /// ANSI Control Sequence Introducer (ESC `[`). Reliable marker that
    /// some color escape was emitted.
    const CSI: &str = "\x1b[";

    // ---- warn_body / notice_warn (yellow-bodied notices) -----------------

    #[test]
    fn warn_body_emits_yellow_when_stderr_color_on() {
        let _g = lock();
        crate::color::set_stream(crate::color::Stream::Stdout, false); // hook-env case: stdout piped
        crate::color::set_stream(crate::color::Stream::Stderr, true);
        let out = warn_body("ONLY_HERE: field not found — skipping");
        assert!(out.contains(CSI), "got: {out:?}");
        assert!(out.contains("ONLY_HERE"), "body content preserved: {out:?}");
    }

    #[test]
    fn warn_body_is_plain_when_stderr_color_off() {
        let _g = lock();
        crate::color::set_stream(crate::color::Stream::Stdout, false);
        crate::color::set_stream(crate::color::Stream::Stderr, false);
        assert_eq!(warn_body("hello"), "hello");
    }

    #[test]
    fn notice_warn_does_not_panic_in_either_color_state() {
        let _g = lock();
        for &on in &[true, false] {
            crate::color::set_stream(crate::color::Stream::Stderr, on);
            notice_warn("sealward", "FOO: skipping");
        }
    }

    // ---- diff helpers: color-on vs color-off state -----------------------

    #[test]
    fn diff_added_emits_ansi_when_colors_on() {
        let _g = lock();
        crate::color::set_stream(crate::color::Stream::Stdout, true);
        crate::color::set_stream(crate::color::Stream::Stderr, true);
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
        crate::color::set_stream(crate::color::Stream::Stdout, false); // stdout: piped, no color
        crate::color::set_stream(crate::color::Stream::Stderr, true); // stderr: TTY, should color
        let out = diff_added("FOO");
        assert!(
            out.contains(CSI),
            "diff_added is meant for stderr; must respect stderr color flag. got: {out:?}"
        );
    }

    #[test]
    fn diff_removed_respects_stderr_flag_when_stdout_piped() {
        let _g = lock();
        crate::color::set_stream(crate::color::Stream::Stdout, false);
        crate::color::set_stream(crate::color::Stream::Stderr, true);
        let out = diff_removed("FOO");
        assert!(out.contains(CSI), "got: {out:?}");
    }

    #[test]
    fn dim_hint_respects_stderr_flag_when_stdout_piped() {
        let _g = lock();
        crate::color::set_stream(crate::color::Stream::Stdout, false);
        crate::color::set_stream(crate::color::Stream::Stderr, true);
        let out = dim_hint("~/p");
        assert!(out.contains(CSI), "got: {out:?}");
    }

    #[test]
    fn diff_added_is_plain_when_colors_off() {
        let _g = lock();
        crate::color::set_stream(crate::color::Stream::Stdout, false);
        crate::color::set_stream(crate::color::Stream::Stderr, false);
        let out = diff_added("FOO");
        assert_eq!(out, "+FOO", "no ANSI when disabled: {out:?}");
    }

    #[test]
    fn diff_removed_emits_ansi_when_colors_on() {
        let _g = lock();
        crate::color::set_stream(crate::color::Stream::Stdout, true);
        crate::color::set_stream(crate::color::Stream::Stderr, true);
        let out = diff_removed("BAR");
        assert!(out.contains(CSI), "{out:?}");
        assert!(out.contains('-'));
        assert!(out.contains("BAR"));
    }

    #[test]
    fn diff_removed_is_plain_when_colors_off() {
        let _g = lock();
        crate::color::set_stream(crate::color::Stream::Stdout, false);
        crate::color::set_stream(crate::color::Stream::Stderr, false);
        assert_eq!(diff_removed("BAR"), "-BAR");
    }

    #[test]
    fn dim_hint_emits_ansi_when_colors_on() {
        let _g = lock();
        crate::color::set_stream(crate::color::Stream::Stdout, true);
        crate::color::set_stream(crate::color::Stream::Stderr, true);
        let out = dim_hint("~/p");
        assert!(out.contains(CSI), "{out:?}");
        assert!(out.contains("(~/p)"));
    }

    #[test]
    fn dim_hint_is_plain_when_colors_off() {
        let _g = lock();
        crate::color::set_stream(crate::color::Stream::Stdout, false);
        crate::color::set_stream(crate::color::Stream::Stderr, false);
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
        assert!(!crate::color::enabled(crate::color::Stream::Stdout));
        assert!(!crate::color::enabled(crate::color::Stream::Stderr));
        unsafe {
            std::env::remove_var("NO_COLOR");
        }
    }

    #[test]
    fn init_colors_no_color_takes_priority_over_tty() {
        let _g = lock();
        // Even if we pre-force colors on, NO_COLOR must turn them off.
        crate::color::set_stream(crate::color::Stream::Stdout, true);
        crate::color::set_stream(crate::color::Stream::Stderr, true);
        unsafe {
            std::env::set_var("NO_COLOR", "anything");
        }
        init_colors();
        assert!(!crate::color::enabled(crate::color::Stream::Stdout));
        assert!(!crate::color::enabled(crate::color::Stream::Stderr));
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
        assert_eq!(
            crate::color::enabled(crate::color::Stream::Stdout),
            std::io::stdout().is_terminal()
        );
        assert_eq!(
            crate::color::enabled(crate::color::Stream::Stderr),
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
        crate::color::set_stream(crate::color::Stream::Stderr, true);
        notice("test", "with color");
        crate::color::set_stream(crate::color::Stream::Stderr, false);
        notice("test", "without color");
    }

    #[test]
    fn the_failure_line_does_not_panic_in_either_color_state() {
        let _g = lock();
        for &on in &[true, false] {
            crate::color::set_stream(crate::color::Stream::Stderr, on);
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

    #[test]
    fn breaks_a_wide_word_by_columns_and_keeps_all_of_it() {
        let word = "界".repeat(20);
        let w = wrap_to(&word, 20);
        assert!(w.lines().all(|l| crate::style::measure(l) <= 20), "{w}");
        assert_eq!(w.replace('\n', ""), word);
        // An odd room: a wide character never straddles the edge.
        let w = wrap_to(&format!("  {word}"), 9);
        assert!(w.lines().all(|l| crate::style::measure(l) <= 9), "{w}");
        assert_eq!(w.replace(['\n', ' '], ""), word);
    }
}
