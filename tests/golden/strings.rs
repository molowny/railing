//! Pure strings, byte for byte: escapes are written `\e` so a fixture shows
//! exactly what was emitted, not what a terminal made of it.

use cli_kit::glyph::{self, State};
use cli_kit::{CliError, Grid, Tone, style, ui};

pub fn all() -> Vec<(String, Result<String, String>)> {
    let mut out = Vec::new();
    for color in [true, false] {
        let variant = if color { "color" } else { "plain" };
        colors(color);
        out.push((format!("strings/glyph-{variant}.txt"), Ok(glyphs())));
        out.push((
            format!("strings/grid_plain-{variant}.txt"),
            Ok(grid_plain()),
        ));
        out.push((format!("strings/ui_styles-{variant}.txt"), Ok(ui_styles())));
    }
    colors(false);
    out.push(("strings/errors.txt".into(), Ok(errors())));
    out
}

fn colors(on: bool) {
    console::set_colors_enabled(on);
    console::set_colors_enabled_stderr(on);
}

fn visible(s: &str) -> String {
    s.replace('\x1b', "\\e")
}

fn glyphs() -> String {
    let mut out = String::new();
    for state in [State::Active, State::Submit, State::Error, State::Cancel] {
        out += &format!("[{state:?}]\n");
        out += &format!("step    {}\n", visible(&glyph::step(state)));
        out += &format!("bar     {}\n", visible(&glyph::bar(state)));
        out += &format!("corner  {}\n", visible(&glyph::corner(state)));
        out += &format!("tinted  {}\n", visible(&glyph::tinted(state, "text")));
        out += &format!(
            "header  {}",
            visible(&glyph::header(state, "first\nsecond"))
        );
    }
    out += &format!("[dim]\n{}\n", visible(&glyph::dim("muted")));
    out
}

/// The bordered layout (plain output). The boxed one depends on the
/// terminal's width, so it is a screen (`flow_table`).
fn grid_plain() -> String {
    let mut g = Grid::new(&["Name", "Kind", "Size"])
        .tones([Tone::Strong, Tone::Accent, Tone::Muted])
        .right(&[2]);
    g.lead("3 items");
    g.row(["alpha", "file", "12 KB"]);
    g.row(["bravo (two\nlines)", "dir", "—"]);
    g.row(["日本語のファイル", "file", "1.2 MB"]);
    g.row([
        style("styled").red().to_string(),
        "link".into(),
        "0 B".into(),
    ]);
    let mut kv = Grid::kv();
    kv.row(["url", "https://example.test"]);
    kv.row(["user", "alice"]);
    format!(
        "[table]\n{}\n[kv]\n{}\n[no header, tones]\n{}\n",
        visible(&g.plain()),
        visible(&kv.plain()),
        visible(
            &Grid::new(&["A", "B"])
                .no_header(true)
                .tones([Tone::Success, Tone::Danger])
                .row(["ok", "bad"])
                .plain()
        )
    )
}

fn ui_styles() -> String {
    [
        ("diff_added", ui::diff_added("new-item")),
        ("diff_removed", ui::diff_removed("old-item")),
        ("dim_hint", ui::dim_hint("press q to quit")),
        ("warn_body", ui::warn_body("check the logs")),
        ("flow::bar", cli_kit::flow::bar()),
    ]
    .iter()
    .map(|(name, s)| format!("{name:<13}{}\n", visible(s)))
    .collect()
}

/// Display, chain() and JSON for every kind: what a removal of anyhow (M6)
/// must keep saying.
fn errors() -> String {
    let io = || std::io::Error::other("disk full");
    let cases = [
        ("usage", CliError::Usage("`app add` needs a name".into())),
        (
            "needs",
            CliError::needs("app add", "item", "pass it as an argument"),
        ),
        ("cancelled", CliError::Cancelled),
        ("auth", CliError::Auth("token expired".into())),
        ("config", CliError::Config("missing url".into())),
        ("external", CliError::External("503".into())),
        (
            "internal",
            CliError::Internal(anyhow::anyhow!("plain message")),
        ),
        (
            "internal with context",
            CliError::Internal(anyhow::Error::new(io()).context("saving the vault")),
        ),
        (
            "internal, two contexts",
            CliError::Internal(
                anyhow::Error::new(io())
                    .context("writing vault.db")
                    .context("saving the vault"),
            ),
        ),
        (
            "from_anyhow",
            CliError::from_anyhow(anyhow::anyhow!("converted")),
        ),
        ("rendered", CliError::Rendered("already said".into())),
        (
            "detailed",
            CliError::Config("bad value".into()).with_details(serde_json::json!({"key": "url"})),
        ),
    ];
    let mut out = String::new();
    for (name, e) in cases {
        out += &format!("[{name}]\n");
        out += &format!("kind     {} (exit {})\n", e.kind(), e.code());
        out += &format!("display  {e}\n");
        out += &format!("chain    {}\n", e.chain().replace('\n', "\n         "));
        out += &format!(
            "json     {}\n",
            serde_json::to_string_pretty(&e.to_json())
                .expect("json")
                .replace('\n', "\n         ")
        );
    }
    out
}
