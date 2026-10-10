//! The filtered picker (`prompts::pick`) over a list longer than the screen:
//! type to narrow, ↑↓ / PgUp / PgDn to move, Esc clears then cancels.
//!
//! Run with: `cargo run --example pick`

use cli_kit::{CliError, prompts, ui};

fn run() -> Result<(), CliError> {
    ui::intro(" cli-kit pick demo ")?;
    let vaults = ["Private", "Homelab", "Work"];
    let kinds = ["login", "note", "ssh key", "card"];
    let rows: Vec<(usize, String, String)> = (0..60)
        .map(|i| {
            let label = format!("{}/service-{i:02}", vaults[i % vaults.len()]);
            (i, label, kinds[i % kinds.len()].to_string())
        })
        .collect();
    let i = prompts::pick("Item", &rows)?;
    ui::outro_success(&format!("picked {}", rows[i].1))?;
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => e.render(),
    }
}
