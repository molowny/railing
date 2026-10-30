//! Structured output types and renderers for CLI commands.
//!
//! # Architecture
//!
//! - [`Renderable`] is the trait every output type implements for pretty and
//!   JSON rendering.
//! - [`make_table`] builds [`comfy_table`] tables with a consistent style.
//! - [`DiagnosticsOutput`] is used by `config show`.

use std::sync::atomic::{AtomicBool, Ordering};

use comfy_table::{Attribute, Cell, ContentArrangement, Table, presets::UTF8_BORDERS_ONLY};

// ---------------------------------------------------------------------------
// OutputFormat
// ---------------------------------------------------------------------------

/// How to format CLI output.
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum OutputFormat {
    /// Pretty human-readable output (default)
    #[default]
    Pretty,
    /// Output as JSON
    Json,
}

/// The process's output format, for code that renders without a
/// [`GlobalOpts`] at hand: an error reaching `main` ([`crate::CliError::render`])
/// and a flow closing with [`crate::Flow::fail`].
static JSON: AtomicBool = AtomicBool::new(false);

/// Declare the process's output format. An app with a JSON mode calls this
/// once, right after parsing its arguments, so errors come out as JSON too
/// (see [`crate::error`]). Not calling it keeps every error styled text.
pub fn set_format(format: OutputFormat) {
    JSON.store(matches!(format, OutputFormat::Json), Ordering::SeqCst);
}

/// Whether [`set_format`] declared JSON.
pub fn is_json() -> bool {
    JSON.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// GlobalOpts
// ---------------------------------------------------------------------------

/// Global CLI options threaded through all command handlers.
#[derive(Clone, Debug)]
pub struct GlobalOpts {
    pub format: OutputFormat,
    pub quiet: bool,
    pub yes: bool,
    pub no_header: bool,
    /// Print long lists as their table instead of opening the grid
    /// (`--no-pager`, `docs/cli.md` §8.1.1).
    pub no_pager: bool,
}

// ---------------------------------------------------------------------------
// Renderable trait
// ---------------------------------------------------------------------------

/// A value that can be rendered as pretty human output or as JSON.
pub trait Renderable {
    fn render_pretty(&self);
    fn render_json(&self);

    fn render(&self, format: OutputFormat) {
        match format {
            OutputFormat::Pretty => self.render_pretty(),
            OutputFormat::Json => self.render_json(),
        }
    }
}

// ---------------------------------------------------------------------------
// Table helper
// ---------------------------------------------------------------------------

/// Build a [`comfy_table::Table`] with the shared CLI preset: bold header,
/// styled only when stdout has colour. Prefer [`crate::Grid`], which also
/// draws inside a live flow and takes [`crate::Tone`]s.
pub fn make_table(headers: &[&str], no_header: bool) -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_BORDERS_ONLY)
        .set_content_arrangement(ContentArrangement::Dynamic);
    if crate::color::enabled(crate::color::Stream::Stdout) {
        table.enforce_styling();
    } else {
        table.force_no_tty();
    }
    if !no_header {
        table.set_header(
            headers
                .iter()
                .map(|h| Cell::new(h).add_attribute(Attribute::Bold))
                .collect::<Vec<_>>(),
        );
    }
    table
}

// ---------------------------------------------------------------------------
// Config diagnostics output
// ---------------------------------------------------------------------------

/// A list of (label, value) pairs for `config show`.
pub struct DiagnosticsOutput(pub Vec<(String, String)>);

impl Renderable for DiagnosticsOutput {
    fn render_pretty(&self) {
        let mut kv = make_table(&["Setting", "Value"], false);
        for (k, v) in &self.0 {
            kv.add_row([k.as_str(), v.as_str()]);
        }
        println!("{kv}");
    }

    fn render_json(&self) {
        let map: serde_json::Map<String, serde_json::Value> = self
            .0
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        println!("{}", serde_json::to_string_pretty(&map).unwrap());
    }
}

// ---------------------------------------------------------------------------
// Health-check report
// ---------------------------------------------------------------------------

/// Status of a single health check.
#[derive(Clone, Copy, Debug)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl CheckStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
        }
    }
}

/// One row in a [`ChecksReport`].
pub struct CheckRow {
    pub status: CheckStatus,
    pub name: String,
    pub detail: String,
}

impl CheckRow {
    pub fn new(status: CheckStatus, name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status,
            name: name.into(),
            detail: detail.into(),
        }
    }
}

/// A list of diagnostic checks rendered as colored status rows or JSON.
pub struct ChecksReport(pub Vec<CheckRow>);

impl ChecksReport {
    /// Number of (passed, warned, failed) checks.
    pub fn counts(&self) -> (usize, usize, usize) {
        self.0
            .iter()
            .fold((0, 0, 0), |(p, w, f), r| match r.status {
                CheckStatus::Pass => (p + 1, w, f),
                CheckStatus::Warn => (p, w + 1, f),
                CheckStatus::Fail => (p, w, f + 1),
            })
    }

    /// The worst status seen, as a flow outcome.
    pub fn outcome(&self) -> crate::flow::Outcome {
        match self.counts() {
            (_, _, f) if f > 0 => crate::flow::Outcome::Fail,
            (_, w, _) if w > 0 => crate::flow::Outcome::Warn,
            _ => crate::flow::Outcome::Ok,
        }
    }

    /// `5 passed`, `4 passed · 1 warning · 2 failed`.
    pub fn summary(&self) -> String {
        let (p, w, f) = self.counts();
        let mut parts = vec![format!("{p} passed")];
        if w > 0 {
            parts.push(format!("{w} warning{}", if w == 1 { "" } else { "s" }));
        }
        if f > 0 {
            parts.push(format!("{f} failed"));
        }
        parts.join(" · ")
    }
}

impl Renderable for ChecksReport {
    fn render_pretty(&self) {
        use crate::color::style;
        for row in &self.0 {
            let icon = match row.status {
                CheckStatus::Pass => style("→").green(),
                CheckStatus::Warn => style("⚠").yellow(),
                CheckStatus::Fail => style("✖").red(),
            };
            println!("{icon} {:<22} {}", row.name, row.detail);
        }
    }

    fn render_json(&self) {
        let items: Vec<_> = self
            .0
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "status": r.status.label(),
                    "detail": r.detail,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items).unwrap());
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_json_is_valid_object() {
        let data = DiagnosticsOutput(vec![
            (
                "config_path".into(),
                "/home/user/.config/ksef/config.toml".into(),
            ),
            ("profile".into(), "default".into()),
        ]);
        let map: serde_json::Map<String, serde_json::Value> = data
            .0
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        let rendered = serde_json::to_string_pretty(&map).unwrap();
        let v: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert!(v.is_object());
        assert_eq!(v["config_path"], "/home/user/.config/ksef/config.toml");
        assert_eq!(v["profile"], "default");
    }

    fn assert_clean_json(json: &str) {
        let v: serde_json::Value = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("not valid JSON: {e}\ncontent:\n{json}"));
        assert!(
            !json.contains('\x1b'),
            "JSON output contains ANSI escape codes"
        );
        assert!(
            !json.contains("◆") && !json.contains("◇"),
            "JSON output contains prompt characters"
        );
        assert!(
            !json.contains("INFO") && !json.contains("DEBUG") && !json.contains("WARN"),
            "JSON output contains log level markers"
        );
        let _ = v;
    }

    #[test]
    fn diagnostics_json_is_clean() {
        let data = DiagnosticsOutput(vec![("key".into(), "value".into())]);
        let map: serde_json::Map<String, serde_json::Value> = data
            .0
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        let json = serde_json::to_string_pretty(&map).unwrap();
        assert_clean_json(&json);
    }

    #[test]
    fn output_format_default_is_pretty() {
        assert!(matches!(OutputFormat::default(), OutputFormat::Pretty));
    }

    fn checks_report_json(report: &ChecksReport) -> String {
        let items: Vec<_> = report
            .0
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "status": r.status.label(),
                    "detail": r.detail,
                })
            })
            .collect();
        serde_json::to_string_pretty(&items).unwrap()
    }

    #[test]
    fn check_status_labels() {
        assert_eq!(CheckStatus::Pass.label(), "PASS");
        assert_eq!(CheckStatus::Warn.label(), "WARN");
        assert_eq!(CheckStatus::Fail.label(), "FAIL");
    }

    #[test]
    fn checks_report_json_shape() {
        let report = ChecksReport(vec![
            CheckRow::new(CheckStatus::Pass, "Store exists", "/data"),
            CheckRow::new(CheckStatus::Warn, "KDF strength", "Argon2id 64 MB"),
            CheckRow::new(CheckStatus::Fail, "Socket", "missing"),
        ]);
        let json = checks_report_json(&report);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let arr = v.as_array().expect("expected JSON array");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["name"], "Store exists");
        assert_eq!(arr[0]["status"], "PASS");
        assert_eq!(arr[0]["detail"], "/data");
        assert_eq!(arr[1]["status"], "WARN");
        assert_eq!(arr[2]["status"], "FAIL");
    }

    #[test]
    fn checks_report_json_clean() {
        let report = ChecksReport(vec![CheckRow::new(
            CheckStatus::Pass,
            "Store exists",
            "/data",
        )]);
        assert_clean_json(&checks_report_json(&report));
    }
}
