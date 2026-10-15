//! `prompts::multiselect` — zero or more choices, all selected to start.
//!
//! Run with: `cargo run -p cli-kit --example multiselect`

use cli_kit::{CliError, prompts, ui};

fn run() -> Result<(), CliError> {
    ui::intro(" multiselect example ")?;
    let toppings = prompts::multiselect(
        "Pizza toppings",
        &[
            ("cheese", "Cheese", ""),
            ("mushrooms", "Mushrooms", "vegetarian"),
            ("olives", "Olives", "vegetarian"),
            ("ham", "Ham", ""),
            ("pineapple", "Pineapple", "controversial"),
        ],
    )?;
    ui::outro_success(&format!(
        "{} chosen: {}",
        toppings.len(),
        toppings.join(", ")
    ))?;
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => e.render(),
    }
}
