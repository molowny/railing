//! `prompts::select` — a short choice — and, for comparison, `prompts::pick`
//! over the same rows.
//!
//! Run with:
//!   cargo run -p railing --example select           # select
//!   cargo run -p railing --example select -- pick   # the picker, same rows

use railing::{CliError, prompts, ui};

fn run(pick: bool) -> Result<(), CliError> {
    ui::intro("select example")?;
    let rows = [
        ("red", "Red", "warm"),
        ("green", "Green", ""),
        ("blue", "Blue", "cool"),
        ("none", "None of these", "skip"),
    ];
    let chosen = if pick {
        let owned: Vec<(&str, String, String)> = rows
            .iter()
            .map(|(v, l, h)| (*v, l.to_string(), h.to_string()))
            .collect();
        prompts::pick("Favourite colour?", &owned)?
    } else {
        prompts::select("Favourite colour?", &rows.map(|(v, l, h)| (v, l, h)))?
    };
    ui::outro_success(&format!("chose {chosen}"))?;
    Ok(())
}

fn main() -> std::process::ExitCode {
    let pick = std::env::args().any(|a| a == "pick");
    match run(pick) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => e.render(),
    }
}
