//! Demonstrates alerts, spinner, and intro/outro banners.
//!
//! Run with: `cargo run --example ui`

use std::thread::sleep;
use std::time::Duration;

use cli_kit::{CliError, ui};

fn run() -> Result<(), CliError> {
    ui::intro(" cli-kit ui demo ")?;

    ui::success("operation complete");
    ui::info("3 items loaded");
    ui::warning("cache is stale");
    ui::error("(this one goes to stderr)");

    let sp = ui::Spinner::start("fetching data…");
    sleep(Duration::from_millis(400));
    sp.update("parsing response…");
    sleep(Duration::from_millis(400));
    sp.stop("done");

    ui::outro_success("all good")?;
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => e.render(),
    }
}
