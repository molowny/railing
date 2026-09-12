//! Shared CLI error type.
//!
//! `CliError` is the single error type that bubbles up to `main` and is
//! rendered there — once, with a consistent style.
//!
//! ## Error buckets
//!
//! | Variant | When to use |
//! |---------|-------------|
//! | `Usage` | Bad CLI input, validation failures |
//! | `Cancelled` | User aborted an interactive prompt |
//! | `Auth` | Authentication / token failures |
//! | `Config` | Config or state file I/O failures |
//! | `External` | Remote API / network errors |
//! | `Internal` | Unexpected I/O, serialisation, or invariant violations |

use std::process::ExitCode;

use console::style;
use thiserror::Error;
use tracing::error;

/// The shared application-level CLI error type.
#[derive(Debug, Error)]
pub enum CliError {
    /// Invalid user input or failed validation.
    #[error("{0}")]
    Usage(String),

    /// The user cancelled an interactive prompt or confirmation.
    #[error("Aborted.")]
    Cancelled,

    /// Authentication or token failure.
    #[error("Authentication error: {0}")]
    Auth(String),

    /// Config or local state I/O failure.
    #[error("Config error: {0}")]
    Config(String),

    /// Remote API or network failure.
    #[error("API error: {0}")]
    External(String),

    /// Unexpected internal failure (bug / invariant violation / I/O).
    #[error("{0}")]
    Internal(#[from] anyhow::Error),

    /// Error already displayed (e.g. via outro_error). Just set exit code.
    #[error("{0}")]
    Rendered(String),
}

impl CliError {
    /// Wrap any `anyhow::Error` as a `CliError::Internal`.
    pub fn from_anyhow(e: anyhow::Error) -> Self {
        CliError::Internal(e)
    }

    /// Render this error to stderr in the shared style and return the
    /// appropriate exit code.
    ///
    /// Internal details are also emitted at `tracing::error` level so they
    /// appear when verbosity is enabled.
    pub fn render(&self) -> ExitCode {
        match self {
            CliError::Cancelled => {
                ExitCode::from(130) // conventional SIGINT exit code
            }
            CliError::Rendered(_) => ExitCode::FAILURE,
            CliError::Usage(msg) => {
                eprintln!("{} {}", style("✖").red(), style(msg).red());
                ExitCode::from(2) // conventional usage-error code
            }
            CliError::Auth(msg) => {
                error!(error = %self, "command failed");
                eprintln!(
                    "{} {}",
                    style("✖").red(),
                    style(format!("Authentication error: {msg}")).red()
                );
                ExitCode::FAILURE
            }
            other => {
                error!(error = %other, "command failed");
                eprintln!("{} {}", style("✖").red(), style(other).red());
                ExitCode::FAILURE
            }
        }
    }

    /// Return a multi-line string with the full error chain, suitable for
    /// verbose / debug output.
    pub fn chain(&self) -> String {
        match self {
            CliError::Internal(e) => {
                let mut out = format!("{}", e);
                let mut source = e.source();
                while let Some(s) = source {
                    out.push_str(&format!("\n  caused by: {}", s));
                    source = std::error::Error::source(s);
                }
                out
            }
            other => format!("{}", other),
        }
    }
}

// ---------------------------------------------------------------------------
// Prompt error mapping
// ---------------------------------------------------------------------------

/// Convert a `cliclack`/`std::io` error into [`CliError`].
///
/// cliclack signals cancellation (Esc / Ctrl+C) as an `io::Error` with kind
/// `Interrupted`. Everything else becomes [`CliError::Internal`].
pub(crate) fn map_prompt_err(e: std::io::Error) -> CliError {
    if e.kind() == std::io::ErrorKind::Interrupted {
        CliError::Cancelled
    } else {
        CliError::Internal(anyhow::anyhow!(e))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::ExitCode;

    #[test]
    fn cancelled_renders_exit_130() {
        let code = CliError::Cancelled.render();
        assert_eq!(code, ExitCode::from(130));
    }

    #[test]
    fn usage_renders_exit_2() {
        let code = CliError::Usage("bad input".into()).render();
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn auth_renders_exit_failure() {
        let code = CliError::Auth("token expired".into()).render();
        assert_eq!(code, ExitCode::FAILURE);
    }

    #[test]
    fn config_renders_exit_failure() {
        let code = CliError::Config("file not found".into()).render();
        assert_eq!(code, ExitCode::FAILURE);
    }

    #[test]
    fn external_renders_exit_failure() {
        let code = CliError::External("API returned 500".into()).render();
        assert_eq!(code, ExitCode::FAILURE);
    }

    #[test]
    fn internal_renders_exit_failure() {
        let e = CliError::from_anyhow(anyhow::anyhow!("unexpected bug"));
        let code = e.render();
        assert_eq!(code, ExitCode::FAILURE);
    }

    #[test]
    fn usage_display_contains_message() {
        let e = CliError::Usage("NIP must be 10 digits".into());
        assert!(format!("{e}").contains("NIP must be 10 digits"));
    }

    #[test]
    fn cancelled_display_is_aborted() {
        assert_eq!(format!("{}", CliError::Cancelled), "Aborted.");
    }

    #[test]
    fn auth_display_contains_prefix_and_message() {
        let e = CliError::Auth("cert load failed".into());
        let s = format!("{e}");
        assert!(s.contains("Authentication error"));
        assert!(s.contains("cert load failed"));
    }

    #[test]
    fn config_display_contains_prefix() {
        let e = CliError::Config("bad path".into());
        assert!(format!("{e}").contains("Config error"));
    }

    #[test]
    fn external_display_contains_prefix() {
        let e = CliError::External("timeout".into());
        assert!(format!("{e}").contains("API error"));
    }

    #[test]
    fn chain_non_internal_returns_display_string() {
        let e = CliError::Usage("bad value".into());
        assert_eq!(e.chain(), format!("{e}"));
    }

    #[test]
    fn chain_internal_shows_root_cause() {
        let inner = anyhow::anyhow!("root cause");
        let outer = anyhow::anyhow!(inner).context("outer context");
        let e = CliError::from_anyhow(outer);
        let chain = e.chain();
        assert!(chain.contains("outer context"), "chain: {chain}");
        assert!(chain.contains("root cause"), "chain: {chain}");
        assert!(chain.contains("caused by"), "chain: {chain}");
    }

    #[test]
    fn chain_internal_single_error_no_caused_by() {
        let e = CliError::from_anyhow(anyhow::anyhow!("solo error"));
        let chain = e.chain();
        assert!(chain.contains("solo error"));
        assert!(!chain.contains("caused by"));
    }

    #[test]
    fn from_anyhow_wraps_as_internal() {
        let e = CliError::from_anyhow(anyhow::anyhow!("something"));
        assert!(matches!(e, CliError::Internal(_)));
    }
}
