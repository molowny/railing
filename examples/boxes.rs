//! Every box a live flow draws, one after another, so they can be compared:
//! a list table, a key/value details box, a text box, a note, health checks
//! and — last — the browsed grid frozen on its view (`q` to finish).
//!
//! Run with:
//!   cargo run -p cli-kit --example boxes              # all boxes, then the grid
//!   cargo run -p cli-kit --example boxes -- no-grid   # the static boxes only

use cli_kit::flow::text_box;
use cli_kit::{Browse, CheckRow, CheckStatus, ChecksReport, CliError, Flow, Grid, Tone};

fn items() -> Grid {
    let mut g = Grid::new(&["Id", "Type", "Title", "Size"])
        .tones([Tone::Muted, Tone::Accent, Tone::Strong])
        .right(&[3]);
    for (id, kind, title, size) in [
        ("a1b2c3", "login", "github", "12 KB"),
        ("d4e5f6", "note", "recovery codes", "1 KB"),
        ("0789ab", "card", "visa", "—"),
    ] {
        g.row([id, kind, title, size]);
    }
    g
}

fn run(flow: &Flow, grid: bool) -> Result<(), CliError> {
    flow.table("Items (flow.table)", &items());

    let mut kv = Grid::kv();
    kv.row(["Vault", "personal"])
        .row(["Items", "3"])
        .row(["Remote", "origin"]);
    flow.table("Details (Grid::kv)", &kv);

    eprint!(
        "{}",
        text_box("Text (text_box)", "A pre-styled panel.\nTwo lines.")
    );

    flow.note("Note (flow.note)", "A block of prose.\nIts body is dimmed.");

    flow.checks(
        "Checks (flow.checks)",
        &ChecksReport(vec![
            CheckRow::new(CheckStatus::Pass, "agent", "running"),
            CheckRow::new(CheckStatus::Warn, "sync", "3 days ago"),
            CheckRow::new(CheckStatus::Fail, "remote", "unreachable"),
        ]),
    );

    if grid {
        let view = flow.browse_table("Items (browse_table, frozen)", &items())?;
        flow.info(&format!("browsed: {}", view.browsed));
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    cli_kit::ui::init_colors();
    let grid = !std::env::args().any(|a| a == "no-grid");
    let live = console::user_attended_stderr() && console::user_attended();
    let flow = Flow::begin("boxes example", live).browse(Browse::Always);
    match run(&flow, grid) {
        Ok(()) => {
            flow.done("every box shown");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => cli_kit::flow::render_error(&e),
    }
}
