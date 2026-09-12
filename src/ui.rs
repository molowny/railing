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
