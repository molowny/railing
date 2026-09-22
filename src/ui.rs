//! High-level UI primitives: alerts, spinners, intro/outro.
//!
//! Wraps `cliclack` so downstream crates never import it directly.

use console::style;

use crate::error::CliError;

// ---------------------------------------------------------------------------
// Colored alerts
// ---------------------------------------------------------------------------

/// Print a success message in green.
pub fn success(msg: &str) {
    println!("{} {}", style("→").green(), style(msg).green());
}

/// Print a warning message in yellow.
pub fn warning(msg: &str) {
    println!("{} {}", style("⚠").yellow(), style(msg).yellow());
}

/// Print an info message in blue.
pub fn info(msg: &str) {
    println!("{} {}", style("ℹ").blue(), style(msg).blue());
}

/// Print an error message in red.
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

/// A spinner that wraps `cliclack::spinner`.
pub struct Spinner {
    inner: cliclack::ProgressBar,
}

impl Spinner {
    /// Create and start a spinner with the given message.
    pub fn start(msg: &str) -> Self {
        let s = cliclack::spinner();
        s.start(msg);
        Self { inner: s }
    }

    /// Update the spinner message.
    pub fn update(&self, msg: &str) {
        self.inner.start(msg);
    }

    /// Stop the spinner and show a success message.
    pub fn stop(self, msg: &str) {
        self.inner.stop(msg);
    }

    /// Clear the spinner without leaving any output.
    pub fn clear(self) {
        self.inner.clear();
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

/// Emit a tagged status line to stderr: `{tag}: {body}`.
///
/// The tag is dimmed (so the eye lands on the body). Use this for
/// direnv-style activity messages — env-var changes on `cd`, "vault locked"
/// reminders, "loading config" hints. Designed to be safe to call from
/// shell hooks because output is stderr-only.
pub fn notice(tag: &str, body: &str) {
    eprintln!("{} {}", style(format!("{tag}:")).for_stderr().dim(), body);
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
    style(format!("({detail})")).for_stderr().dim().to_string()
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
        style(format!("{tag}:")).for_stderr().dim(),
        warn_body(body)
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests in this module mutate `console`'s global color flags and
    /// `NO_COLOR`. Serialize them through this lock so parallel `cargo test`
    /// can't observe inconsistent state. Poisoning is ignored — once one
    /// test panics, the others might double-fail but that's strictly
    /// better than masking the original failure.
    fn lock() -> std::sync::MutexGuard<'static, ()> {
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
    fn alert_helpers_do_not_panic_in_either_color_state() {
        let _g = lock();
        for &on in &[true, false] {
            console::set_colors_enabled(on);
            console::set_colors_enabled_stderr(on);
            success("ok");
            warning("careful");
            info("note");
            error("fail");
        }
    }
}
