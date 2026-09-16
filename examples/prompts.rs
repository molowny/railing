//! Demonstrates every prompt helper in `cli_kit::prompts`.
//!
//! Run with: `cargo run --example prompts`
//!
//! Press Esc or Ctrl+C at any prompt to see `CliError::Cancelled` propagate.

use cli_kit::{prompts, ui, CliError};

fn run() -> Result<(), CliError> {
    ui::intro(" cli-kit prompts demo ")?;

    let name = prompts::input("What is your name?", "e.g. Ada")?;
    let nickname = prompts::input_with_default("Nickname", &name)?;
    let note = prompts::input_optional("Any notes?", "(optional)")?;
    let _bio = prompts::input_optional_with_default("Bio", "no bio yet")?;
    let _pw = prompts::password("Password")?;

    let color = prompts::select(
        "Favourite colour?",
        &[
            ("red", "Red", "warm"),
            ("green", "Green", "calm"),
            ("blue", "Blue", "cool"),
        ],
    )?;

    let toppings = prompts::multiselect(
        "Pizza toppings",
        &[
            ("cheese", "Cheese", ""),
            ("mush", "Mushrooms", ""),
            ("olives", "Olives", ""),
        ],
    )?;

    if !prompts::confirm("Send telemetry?", false)? {
        ui::info("telemetry disabled");
    }

    if !prompts::confirm("Save profile?", true)? {
        return Err(CliError::Cancelled);
    }

    ui::outro_success(&format!(
        "saved {nickname} ({color}) with {} toppings, note={note:?}",
        toppings.len()
    ))?;
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => e.render(),
    }
}
