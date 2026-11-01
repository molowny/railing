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

use std::fmt;
use std::process::ExitCode;

/// `tracing::error!` with the `logging` feature, nothing without it.
macro_rules! log_error {
    ($($t:tt)*) => {{
        #[cfg(feature = "logging")]
        tracing::error!($($t)*);
    }};
}
pub(crate) use log_error;

/// `tracing::warn!` with the `logging` feature, nothing without it.
#[cfg(feature = "table")]
macro_rules! log_warn {
    ($($t:tt)*) => {{
        #[cfg(feature = "logging")]
        tracing::warn!($($t)*);
    }};
}
#[cfg(feature = "table")]
pub(crate) use log_warn;

/// Any error, boxed: what [`CliError::Internal`] holds. An app's own error
/// types, `std::io::Error`, a `String`, and `anyhow::Error` (which converts
/// into it, keeping its chain) all go in through [`CliError::internal`].
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// The shared application-level CLI error type.
#[derive(Debug)]
pub enum CliError {
    /// Invalid user input or failed validation.
    Usage(String),

    /// The user cancelled an interactive prompt or confirmation.
    Cancelled,

    /// Authentication or token failure.
    Auth(String),

    /// Config or local state I/O failure.
    Config(String),

    /// Remote API or network failure.
    External(String),

    /// Unexpected internal failure (bug / invariant violation / I/O). Its
    /// causes are the error's [`source`](std::error::Error::source) chain.
    Internal(BoxError),

    /// Error already displayed (e.g. via outro_error). Just set exit code.
    Rendered(String),

    /// Another error plus structured data for JSON mode
    /// ([`CliError::with_details`]). Displays, exits and renders as text
    /// exactly like the error it wraps.
    Detailed {
        error: Box<CliError>,
        details: Details,
    },
}

/// Structured data an error carries for JSON mode: what
/// [`CliError::with_details`] attached (feature `output`).
#[derive(Clone, Debug)]
pub struct Details(#[cfg(feature = "output")] serde_json::Value);

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Usage(m) | CliError::Rendered(m) => f.write_str(m),
            CliError::Cancelled => f.write_str("Aborted."),
            CliError::Auth(m) => write!(f, "Authentication error: {m}"),
            CliError::Config(m) => write!(f, "Config error: {m}"),
            CliError::External(m) => write!(f, "API error: {m}"),
            CliError::Internal(e) => write!(f, "{e}"),
            CliError::Detailed { error, .. } => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CliError {
    /// An internal error is the source of its `CliError`, so a caller
    /// walking the chain finds the original error and its causes.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CliError::Internal(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

/// A message and the error that caused it: what the kit used
/// `anyhow::Context` for. Displays as the message; the cause is its
/// [`source`](std::error::Error::source), so a chain renders as
/// `message: cause`.
#[derive(Debug)]
pub struct WithContext {
    message: String,
    source: BoxError,
}

impl WithContext {
    pub fn new(message: impl Into<String>, source: impl Into<BoxError>) -> Self {
        WithContext {
            message: message.into(),
            source: source.into(),
        }
    }
}

impl fmt::Display for WithContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WithContext {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// `.context(|| ...)` for the kit's own results, giving a [`WithContext`].
#[cfg(feature = "paths")]
pub(crate) trait Context<T> {
    fn context(self, message: impl FnOnce() -> String) -> Result<T, WithContext>;
}

#[cfg(feature = "paths")]
impl<T, E: Into<BoxError>> Context<T> for Result<T, E> {
    fn context(self, message: impl FnOnce() -> String) -> Result<T, WithContext> {
        self.map_err(|e| WithContext::new(message(), e))
    }
}

/// `error` and each of its causes, outermost first.
fn causes<'a>(
    error: &'a (dyn std::error::Error + 'static),
) -> impl Iterator<Item = &'a (dyn std::error::Error + 'static)> {
    std::iter::successors(Some(error), |e| e.source())
}

impl CliError {
    /// Any error as a [`CliError::Internal`]: an app's error type, an
    /// `std::io::Error`, a message (`&str`, `String`), or an `anyhow::Error`,
    /// whose context chain comes along as the boxed error's sources.
    pub fn internal(error: impl Into<BoxError>) -> Self {
        CliError::Internal(error.into())
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
    #[cfg(feature = "output")]
    pub fn with_details(self, details: serde_json::Value) -> Self {
        CliError::Detailed {
            error: Box::new(self),
            details: Details(details),
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
    #[cfg(feature = "output")]
    pub fn to_json(&self) -> serde_json::Value {
        // The bare message: `kind` already says what the text prefix
        // ("API error: ") would.
        let message = match self.inner() {
            CliError::Usage(m)
            | CliError::Auth(m)
            | CliError::Config(m)
            | CliError::External(m)
            | CliError::Rendered(m) => m.clone(),
            // The whole chain: a client has no `-v` to ask for it.
            CliError::Internal(_) => self.chain_line(),
            other => other.to_string(),
        };
        let mut error = serde_json::json!({
            "kind": self.kind(),
            "message": message,
            "exit_code": self.code(),
        });
        if let CliError::Detailed { details, .. } = self {
            error["details"] = details.0.clone();
        }
        serde_json::json!({ "error": error })
    }

    /// Render this error and return the appropriate exit code: as JSON on
    /// stdout in JSON mode ([`crate::output::set_format`], with the `output`
    /// feature, which writes JSON), otherwise to stderr in the shared style.
    ///
    /// With the `logging` feature, internal details are also emitted at
    /// `tracing::error` level so they appear when verbosity is enabled.
    #[allow(clippy::print_stdout)]
    pub fn render(&self) -> ExitCode {
        #[cfg(feature = "output")]
        if crate::output::is_json() {
            if !matches!(self.inner(), CliError::Cancelled) {
                log_error!(error = %self, "command failed");
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
                log_error!(error = %self, "command failed");
                crate::ui::error(&format!("Authentication error: {msg}"));
                ExitCode::FAILURE
            }
            other => {
                log_error!(error = %other, "command failed");
                crate::ui::error(&other.to_string());
                ExitCode::FAILURE
            }
        }
    }

    /// The message and every cause on one line, `outer: cause: root`, as JSON
    /// mode reports it: for an app that shows the whole chain where the
    /// default text shows only the outermost message. Read through
    /// [`source`](std::error::Error::source), never alternate formatting.
    pub fn chain_line(&self) -> String {
        match self.inner() {
            CliError::Internal(e) => causes(e.as_ref())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(": "),
            other => other.to_string(),
        }
    }

    /// Return a multi-line string with the full error chain, suitable for
    /// verbose / debug output.
    pub fn chain(&self) -> String {
        match self.inner() {
            CliError::Internal(e) => {
                let mut out = String::new();
                for (i, cause) in causes(e.as_ref()).enumerate() {
                    if i > 0 {
                        out += "\n  caused by: ";
                    }
                    out += &cause.to_string();
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
        let e = CliError::internal(anyhow::anyhow!("unexpected bug"));
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
        let e = CliError::internal(outer);
        let chain = e.chain();
        assert!(chain.contains("outer context"), "chain: {chain}");
        assert!(chain.contains("root cause"), "chain: {chain}");
        assert!(chain.contains("caused by"), "chain: {chain}");
    }

    #[test]
    fn chain_internal_single_error_no_caused_by() {
        let e = CliError::internal(anyhow::anyhow!("solo error"));
        let chain = e.chain();
        assert!(chain.contains("solo error"));
        assert!(!chain.contains("caused by"));
    }

    #[cfg(feature = "output")]
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

    #[cfg(feature = "output")]
    #[test]
    fn json_internal_message_has_the_chain() {
        let e = CliError::internal(anyhow::anyhow!("root cause").context("outer"));
        let msg = e.to_json()["error"]["message"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(msg.contains("outer") && msg.contains("root cause"), "{msg}");
    }

    #[cfg(feature = "output")]
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
    fn internal_takes_any_error_and_a_message() {
        let e = CliError::internal(anyhow::anyhow!("something"));
        assert!(matches!(e, CliError::Internal(_)));
        assert_eq!(CliError::internal("a message").to_string(), "a message");
        let io = std::io::Error::other("disk full");
        assert_eq!(CliError::internal(io).to_string(), "disk full");
    }

    /// A chain from the kit's own context type reads like anyhow's did.
    #[cfg(feature = "output")]
    #[test]
    fn a_context_chain_renders_through_its_sources() {
        let io = std::io::Error::other("disk full");
        let e = CliError::internal(WithContext::new(
            "saving the vault",
            WithContext::new("writing vault.db", io),
        ));
        assert_eq!(e.to_string(), "saving the vault");
        assert_eq!(
            e.chain(),
            "saving the vault\n  caused by: writing vault.db\n  caused by: disk full"
        );
        assert_eq!(
            e.to_json()["error"]["message"],
            "saving the vault: writing vault.db: disk full"
        );
    }
}
