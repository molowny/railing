//! Demonstrates the `CliError` enum and how it maps to exit codes.
//!
//! Run with: `cargo run --example errors`

use std::process::ExitCode;

use railing::CliError;

fn do_work(kind: &str) -> Result<(), CliError> {
    match kind {
        "usage" => Err(CliError::Usage("expected --name argument".into())),
        "auth" => Err(CliError::Auth("token expired".into())),
        "config" => Err(CliError::Config("~/.config/app.toml not found".into())),
        "external" => Err(CliError::External("API returned 503".into())),
        "cancelled" => Err(CliError::Cancelled),
        "internal" => Err(CliError::internal(
            anyhow::anyhow!("disk offline").context("while writing state"),
        )),
        _ => Ok(()),
    }
}

fn main() -> ExitCode {
    for kind in [
        "usage",
        "auth",
        "config",
        "external",
        "cancelled",
        "internal",
    ] {
        println!("\n--- {kind} ---");
        if let Err(e) = do_work(kind) {
            println!("chain: {}", e.chain());
            let _ = e.render();
        }
    }
    ExitCode::SUCCESS
}
