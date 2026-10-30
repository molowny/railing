//! Pure strings, byte for byte: escapes are written `\e` so a fixture shows
//! exactly what was emitted, not what a terminal made of it.

use cli_kit::glyph::{self, State};
use cli_kit::style::Caps;
use cli_kit::{CliError, Grid, Tone, style, ui};

pub fn all() -> Vec<(String, Result<String, String>)> {
    let mut out = Vec::new();
    for color in [true, false] {
        let variant = if color { "color" } else { "plain" };
        colors(color);
        out.push((format!("strings/glyph-{variant}.txt"), Ok(glyphs(color))));
        out.push((
            format!("strings/grid_plain-{variant}.txt"),
            Ok(grid_plain()),
        ));
        out.push((format!("strings/ui_styles-{variant}.txt"), Ok(ui_styles())));
        out.push((format!("strings/step-{variant}.txt"), Ok(steps(color))));
    }
    colors(false);
    out.push(("strings/errors.txt".into(), Ok(errors())));
    out
}

fn colors(on: bool) {
    cli_kit::color::set_stream(cli_kit::color::Stream::Stdout, on);
    cli_kit::color::set_stream(cli_kit::color::Stream::Stderr, on);
}

fn visible(s: &str) -> String {
    s.replace('\x1b', "\\e")
}

fn glyphs(color: bool) -> String {
    // The caps a terminal that takes colour (or not) would have.
    let caps = Caps {
        color,
        redraw: true,
    };
    let mut out = String::new();
    for state in [State::Active, State::Submit, State::Error, State::Cancel] {
        out += &format!("[{state:?}]\n");
        out += &format!("step    {}\n", visible(&glyph::step(caps, state)));
        out += &format!("bar     {}\n", visible(&glyph::bar(caps, state)));
        out += &format!("corner  {}\n", visible(&glyph::corner(caps, state)));
        out += &format!("tinted  {}\n", visible(&glyph::tinted(caps, state, "text")));
        out += &format!(
            "header  {}",
            visible(&glyph::header(caps, state, "first\nsecond"))
        );
    }
    out += &format!("[dim]\n{}\n", visible(&glyph::dim(caps, "muted")));
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
            CliError::internal(anyhow::anyhow!("plain message")),
        ),
        (
            "internal with context",
            CliError::internal(anyhow::Error::new(io()).context("saving the vault")),
        ),
        (
            "internal, two contexts",
            CliError::internal(
                anyhow::Error::new(io())
                    .context("writing vault.db")
                    .context("saving the vault"),
            ),
        ),
        (
            "internal() from anyhow",
            CliError::internal(anyhow::anyhow!("converted")),
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

/// The core's step blocks in every state, with no `Flow` in scope (askpass
/// bar A8): the open secret step askpass draws, its error, submitted and
/// cancelled forms, an input row with a value, and the plain form for a
/// terminal that cannot redraw.
fn steps(color: bool) -> String {
    use cli_kit::render::Park;
    use cli_kit::step::{self, Body, Foot, Status, Step};
    let caps = Caps {
        color,
        redraw: true,
    };
    let secret = |status| Step {
        status,
        label: "Master password",
        desc: "Unlock your sealward vault",
        body: Body::silent_input(),
        foot: Foot::Hint("hidden as you type · enter to submit · esc to cancel"),
    };
    let value = |status| Step {
        status,
        label: "Project name\nsecond line of the question",
        desc: "",
        body: Body::Rows {
            rows: vec!["alice".into()],
            cursor: Some((0, 5)),
        },
        foot: Foot::Message("[Esc](Preview)"),
    };
    let mut out = String::new();
    let mut show = |name: &str, b: cli_kit::render::Block| {
        let park = match b.park {
            Some(Park::At { up, col }) => format!("park up {up} col {col}"),
            Some(Park::Here) => "park here".into(),
            None => "no park".into(),
        };
        out += &format!("[{name}] {} lines, {park}\n{}\n", b.lines, visible(&b.text));
    };
    show(
        "secret active",
        step::render(caps, &secret(Status::Active), 80),
    );
    show(
        "secret error",
        step::render(
            caps,
            &secret(Status::Error("Wrong password (4 attempts remaining)")),
            80,
        ),
    );
    let done = Step {
        desc: "",
        body: Body::None,
        ..secret(Status::Submit)
    };
    show("secret submitted", step::render(caps, &done, 80));
    show(
        "secret cancelled",
        step::render(caps, &secret(Status::Cancel), 80),
    );
    show(
        "value active",
        step::render(caps, &value(Status::Active), 80),
    );
    show(
        "value submitted",
        step::render(caps, &value(Status::Submit), 80),
    );
    show(
        "secret active, 40 columns",
        step::render(caps, &secret(Status::Active), 40),
    );
    show(
        "secret error, plain terminal",
        step::render(Caps::PLAIN, &secret(Status::Error("Wrong password")), 80),
    );
    out
}
