//! A long list through `Flow::browse_table` (`docs/cli.md` §8.1.1): the grid
//! opens inline in the flow and freezes on the view the person ends on.
//!
//! Run with:
//!   cargo run -p cli-kit --example data_grid               # grid when 200 rows do not fit
//!   cargo run -p cli-kit --example data_grid -- 5          # fits: the table, no grid
//!   cargo run -p cli-kit --example data_grid -- 200 never  # `--no-pager`
//!   cargo run -p cli-kit --example data_grid | cat         # plain: the table on stdout

use cli_kit::{Browse, CliError, Flow, Grid, GridCell, SortKey, Tone};

fn main() -> std::process::ExitCode {
    cli_kit::ui::init_colors();
    let mut args = std::env::args().skip(1);
    let n: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(200);
    let policy = match args.next().as_deref() {
        Some("never") => Browse::Never,
        Some("always") => Browse::Always,
        _ => Browse::Auto,
    };
    let live = std::io::IsTerminal::is_terminal(&std::io::stderr())
        && std::io::IsTerminal::is_terminal(&std::io::stdout());
    match run(Flow::begin("data_grid example", live).browse(policy), n) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => cli_kit::flow::render_error(&e),
    }
}

fn run(flow: Flow, n: usize) -> Result<(), CliError> {
    let kinds = ["login", "note", "card", "ssh"];
    let mut g = Grid::new(&["Id", "Type", "Title", "Size", "State"])
        .tones([Tone::Muted, Tone::Accent, Tone::Strong])
        .right(&[3]);
    for i in 0..n {
        let size = (i * 7919) % 100_000;
        let size_cell = if i % 11 == 0 {
            GridCell::from("—")
        } else {
            GridCell::from(format!("{size} B")).key(SortKey::Num(size as i64))
        };
        let state = if i % 5 == 0 {
            Tone::Warn.cell("pending")
        } else {
            Tone::Success.cell("synced")
        };
        g.row([
            format!("{:06x}", i * 2654435761 % 0xffffff).into(),
            kinds[i % kinds.len()].into(),
            format!("item {i:03}").into(),
            size_cell,
            state,
        ]);
    }
    let view = flow.browse_table("Items", &g)?;
    let msg = if view.is_all() {
        format!("{} items", view.total)
    } else {
        format!("{} of {} items", view.rows.len(), view.total)
    };
    flow.done(&msg);
    Ok(())
}
