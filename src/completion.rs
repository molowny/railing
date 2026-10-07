//! Static shell completions generated from the app's clap command tree.
//!
//! Handle `completions <shell>` immediately after parsing, before logging,
//! configuration, or other application startup. Output is always shell code,
//! regardless of the app's JSON or quiet flags.

use std::io::{self, Write};

use clap::CommandFactory;
pub use clap_complete::Shell;

use crate::CliError;

/// Print a completion script using the binary name declared by clap.
pub fn print<C: CommandFactory>(shell: Shell) -> Result<(), CliError> {
    let mut command = C::command();
    let name = command
        .get_bin_name()
        .unwrap_or(command.get_name())
        .to_owned();
    // Generate into memory so a failed stdout write is returned as an error
    // instead of panicking inside clap_complete's generator.
    let mut script = Vec::new();
    clap_complete::generate(shell, &mut command, name, &mut script);
    io::stdout()
        .lock()
        .write_all(&script)
        .map_err(|error| CliError::Internal(error.into()))
}
