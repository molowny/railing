//! Structured output types and renderers for CLI commands.
//!
//! # Architecture
//!
//! - [`Renderable`] is the trait every output type implements for pretty and
//!   JSON rendering.
//! - [`make_table`] builds [`comfy_table`] tables with a consistent style.
//! - [`DiagnosticsOutput`] is used by `config show`.

use comfy_table::{Cell, ContentArrangement, Table, presets::UTF8_BORDERS_ONLY};

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

/// Build a [`comfy_table::Table`] with the shared CLI preset.
pub fn make_table(headers: &[&str], no_header: bool) -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_BORDERS_ONLY)
        .set_content_arrangement(ContentArrangement::Dynamic);
    if !no_header {
        table.set_header(headers.iter().map(Cell::new).collect::<Vec<_>>());
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
// Convenience free functions
// ---------------------------------------------------------------------------

/// Print a short success notice.
pub fn print_success(message: &str) {
    println!("OK: {}", message);
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
}
