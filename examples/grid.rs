//! Every `Tone` in a live flow and as the plain table.
//!
//! Run with:
//!   cargo run -p railing --example grid             # live box on stderr
//!   cargo run -p railing --example grid | cat       # plain: no colour on stdout
//!   NO_COLOR=1 cargo run -p railing --example grid  # live, no colour
//!   cargo run -p railing --example grid -- plain    # plain, coloured on a TTY

use railing::{Flow, Grid, Tone};

fn main() {
    railing::ui::init_colors();
    let plain = std::env::args().any(|a| a == "plain");
    let live = !plain
        && std::io::IsTerminal::is_terminal(&std::io::stderr())
        && std::io::IsTerminal::is_terminal(&std::io::stdout());

    let flow = Flow::begin("grid example", live);

    let mut g = Grid::new(&["Id", "Type", "Title", "Amount", "State"])
        .tones([Tone::Muted, Tone::Accent, Tone::Strong])
        .right(&[3]);
    g.row([
        "a1b2c3".into(),
        "login".into(),
        "github".into(),
        "12.00".into(),
        Tone::Success.cell("synced"),
    ]);
    g.row([
        "d4e5f6".into(),
        "note".into(),
        "recovery codes".into(),
        "1 250.50".into(),
        Tone::Warn.cell("pending"),
    ]);
    g.row([
        "0789ab".into(),
        "card".into(),
        "visa".into(),
        "—".into(),
        Tone::Danger.cell("refused"),
    ]);
    flow.table("Items", &g);

    let mut kv = Grid::kv();
    kv.row(["Vault", "personal"]).row(["Items", "3"]);
    kv.row(["Remote".into(), Tone::Accent.cell("origin")]);
    flow.table("Status", &kv);

    let mut quiet = Grid::new(&["Tone", "Sample"]).header_tone(Tone::Muted);
    for (name, tone) in [
        ("Default", Tone::Default),
        ("Strong", Tone::Strong),
        ("Muted", Tone::Muted),
        ("Accent", Tone::Accent),
        ("Success", Tone::Success),
        ("Warn", Tone::Warn),
        ("Danger", Tone::Danger),
    ] {
        quiet.row([name.into(), tone.cell("The quick brown fox 0123456789")]);
    }
    flow.table("Palette (muted header)", &quiet);

    flow.done("3 items");
}
