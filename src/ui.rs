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
    eprintln!("{} {}", style("✖").red(), style(msg).red());
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
// Ambient notices (direnv-style)
// ---------------------------------------------------------------------------
//
// For tools that emit "what just happened" status lines around shell hooks
// or background activity. Output goes to **stderr** so it never interferes
// with `eval`/`source` of stdout, and color codes are auto-suppressed when
// stderr isn't a TTY.
//
// We do our own `IsTerminal` + `NO_COLOR` check rather than rely on the
// upstream `console` crate's auto-detection, which has been observed to
// misfire inside shell hook contexts (notably fish `--on-event` and
// `--on-variable` handlers, where stderr inheritance is non-obvious).

fn stderr_color_enabled() -> bool {
    use std::io::IsTerminal;
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    std::io::stderr().is_terminal()
}

/// Emit a tagged status line to stderr: `{tag}: {body}`.
///
/// The tag is dimmed (so the eye lands on the body). Use this for
/// direnv-style activity messages — env-var changes on `cd`, "vault locked"
/// reminders, "loading config" hints. Designed to be safe to call from
/// shell hooks because output is stderr-only.
pub fn notice(tag: &str, body: &str) {
    let head = format!("{tag}:");
    if stderr_color_enabled() {
        eprintln!("{} {}", style(head).dim().force_styling(true), body);
    } else {
        eprintln!("{head} {body}");
    }
}

/// Render a `+name` token in green. Use when listing items that were
/// added in a transition (e.g. env vars set by direnv-style activate).
pub fn diff_added(name: &str) -> String {
    if stderr_color_enabled() {
        format!(
            "{}{}",
            style("+").green().force_styling(true),
            style(name).green().force_styling(true)
        )
    } else {
        format!("+{name}")
    }
}

/// Render a `-name` token in red. Use when listing items that were removed.
pub fn diff_removed(name: &str) -> String {
    if stderr_color_enabled() {
        format!(
            "{}{}",
            style("-").red().force_styling(true),
            style(name).red().force_styling(true)
        )
    } else {
        format!("-{name}")
    }
}

/// Render `(detail)` in dim style. Use as a trailing hint on notice lines —
/// e.g. the path of the config file that drove the transition.
pub fn dim_hint(detail: &str) -> String {
    if stderr_color_enabled() {
        style(format!("({detail})"))
            .dim()
            .force_styling(true)
            .to_string()
    } else {
        format!("({detail})")
    }
}
