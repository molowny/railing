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
//!
//! ## JSON mode
//!
//! An app that offers machine output calls [`crate::output::set_format`] once
//! at startup. From then on an error that reaches `main` is printed as one
//! JSON document on **stdout** — the stream a client is already parsing —
//! instead of styled text on stderr:
//!
//! ```json
//! {"error": {"kind": "usage", "message": "...", "exit_code": 2}}
//! ```
//!
//! `kind` is [`CliError::kind`]; `details` is present when the error carries
//! structured data ([`CliError::with_details`], e.g. the candidates of an
//! ambiguous lookup). A success prints the command's own document, so in JSON
//! mode stdout holds exactly one document either way and the exit code says
//! which it is.

use std::process::ExitCode;

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

    /// Another error plus structured data for JSON mode
    /// ([`CliError::with_details`]). Displays, exits and renders as text
    /// exactly like the error it wraps.
    #[error("{error}")]
    Detailed {
        error: Box<CliError>,
        details: serde_json::Value,
    },
}

impl CliError {
    /// Wrap any `anyhow::Error` as a `CliError::Internal`.
    pub fn from_anyhow(e: anyhow::Error) -> Self {
        CliError::Internal(e)
    }

    /// The usage error for a left-out argument where no prompt can be shown
    /// ([`crate::Flow::can_ask`]): `` `app cmd` needs an item — `app cmd <item>` ``.
    /// `how` says how to supply it: the form to type, or where to list them.
    pub fn needs(cmd: &str, noun: &str, how: &str) -> Self {
        let a = match noun.chars().next() {
            Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
            _ => "a",
        };
        CliError::Usage(format!("`{cmd}` needs {a} {noun} — {how}"))
    }

    /// Attach structured data that JSON mode reports as `error.details`.
    pub fn with_details(self, details: serde_json::Value) -> Self {
        CliError::Detailed {
            error: Box::new(self),
            details,
        }
    }

    /// The error without any [`CliError::Detailed`] wrapper.
    pub(crate) fn inner(&self) -> &CliError {
        match self {
            CliError::Detailed { error, .. } => error.inner(),
            other => other,
        }
    }

    /// The stable, machine-readable bucket: `usage`, `cancelled`, `auth`,
    /// `config`, `external`, `internal`, or `failed` for an error a flow has
    /// already shown ([`CliError::Rendered`]).
    pub fn kind(&self) -> &'static str {
        match self.inner() {
            CliError::Usage(_) => "usage",
            CliError::Cancelled => "cancelled",
            CliError::Auth(_) => "auth",
            CliError::Config(_) => "config",
            CliError::External(_) => "external",
            CliError::Internal(_) => "internal",
            CliError::Rendered(_) => "failed",
            CliError::Detailed { .. } => unreachable!("inner() unwraps Detailed"),
        }
    }

    /// The numeric exit code, as [`CliError::exit_code`] returns it.
    pub fn code(&self) -> u8 {
        match self.inner() {
            CliError::Cancelled => 130,
            CliError::Usage(_) => 2,
            _ => 1,
        }
    }

    /// The process exit code for this error: 130 for a cancelled prompt,
    /// 2 for a usage error, 1 otherwise.
    pub fn exit_code(&self) -> ExitCode {
        ExitCode::from(self.code())
    }

    /// The JSON-mode document for this error (see the module docs).
    pub fn to_json(&self) -> serde_json::Value {
        // The bare message: `kind` already says what the text prefix
        // ("API error: ") would.
        let message = match self.inner() {
            CliError::Usage(m)
            | CliError::Auth(m)
            | CliError::Config(m)
            | CliError::External(m)
            | CliError::Rendered(m) => m.clone(),
            // The whole context chain: a client has no `-v` to ask for it.
            CliError::Internal(e) => format!("{e:#}"),
            other => other.to_string(),
        };
        let mut error = serde_json::json!({
            "kind": self.kind(),
            "message": message,
            "exit_code": self.code(),
        });
        if let CliError::Detailed { details, .. } = self {
            error["details"] = details.clone();
        }
        serde_json::json!({ "error": error })
    }

    /// Render this error and return the appropriate exit code: as JSON on
    /// stdout in JSON mode ([`crate::output::set_format`]), otherwise to
    /// stderr in the shared style.
    ///
    /// Internal details are also emitted at `tracing::error` level so they
    /// appear when verbosity is enabled.
    #[allow(clippy::print_stdout)]
    pub fn render(&self) -> ExitCode {
        if crate::output::is_json() {
            if !matches!(self.inner(), CliError::Cancelled) {
                error!(error = %self, "command failed");
            }
            println!("{}", serde_json::to_string_pretty(&self.to_json()).unwrap());
            return self.exit_code();
        }
        match self {
            CliError::Detailed { error, .. } => error.render(),
            CliError::Cancelled => {
                ExitCode::from(130) // conventional SIGINT exit code
            }
            CliError::Rendered(_) => ExitCode::FAILURE,
            CliError::Usage(msg) => {
                crate::ui::error(msg);
                ExitCode::from(2) // conventional usage-error code
            }
            CliError::Auth(msg) => {
                error!(error = %self, "command failed");
                crate::ui::error(&format!("Authentication error: {msg}"));
                ExitCode::FAILURE
            }
            other => {
                error!(error = %other, "command failed");
                crate::ui::error(&other.to_string());
                ExitCode::FAILURE
            }
        }
    }

    /// Return a multi-line string with the full error chain, suitable for
    /// verbose / debug output.
    pub fn chain(&self) -> String {
        match self.inner() {
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {

    #[test]
    fn needs_names_the_argument_and_how_to_give_it() {
        let e = CliError::needs("app rm", "item", "`app rm <item>`");
        assert_eq!(e.to_string(), "`app rm` needs an item — `app rm <item>`");
        assert_eq!(e.code(), 2);
        let e = CliError::needs("app use", "profile", "`app ls` lists them");
        assert_eq!(
            e.to_string(),
            "`app use` needs a profile — `app ls` lists them"
        );
    }

    /// A plain flow never asks, whatever stdin is.
    #[test]
    fn a_plain_flow_cannot_ask() {
        assert!(!crate::Flow::plain().can_ask());
    }
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
    fn json_document_shape() {
        let v = CliError::Usage("bad input".into()).to_json();
        assert_eq!(v["error"]["kind"], "usage");
        assert_eq!(v["error"]["message"], "bad input");
        assert_eq!(v["error"]["exit_code"], 2);
        assert!(v["error"].get("details").is_none());

        assert_eq!(CliError::Cancelled.to_json()["error"]["exit_code"], 130);
        let ext = CliError::External("item not found".into()).to_json();
        assert_eq!(ext["error"]["kind"], "external");
        assert_eq!(ext["error"]["message"], "item not found");
        assert_eq!(
            CliError::Rendered("x".into()).to_json()["error"]["kind"],
            "failed"
        );
    }

    #[test]
    fn json_internal_message_has_the_chain() {
        let e = CliError::from_anyhow(anyhow::anyhow!("root cause").context("outer"));
        let msg = e.to_json()["error"]["message"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(msg.contains("outer") && msg.contains("root cause"), "{msg}");
    }

    #[test]
    fn details_wrap_without_changing_the_error() {
        let e = CliError::Usage("ambiguous".into())
            .with_details(serde_json::json!({ "matches": [1, 2] }));
        assert_eq!(e.to_string(), "ambiguous");
        assert_eq!(e.kind(), "usage");
        assert_eq!(e.exit_code(), ExitCode::from(2));
        assert_eq!(e.to_json()["error"]["details"]["matches"][1], 2);
    }

    #[test]
    fn from_anyhow_wraps_as_internal() {
        let e = CliError::from_anyhow(anyhow::anyhow!("something"));
        assert!(matches!(e, CliError::Internal(_)));
    }
}
