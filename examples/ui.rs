//! Demonstrates the spinner and intro/outro banners. Messages inside a
//! command go through `cli_kit::Flow` (see the `grid` example).
//!
//! Run with: `cargo run --example ui`

use std::thread::sleep;
use std::time::Duration;

use cli_kit::{CliError, ui};

fn run() -> Result<(), CliError> {
    ui::intro("cli-kit ui demo")?;

    ui::log::info("3 items loaded")?;
    ui::log::warning("cache is stale")?;

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
