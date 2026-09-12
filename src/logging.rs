//! Logging bootstrap for CLI applications.
//!
//! Logging is silent by default. Pass `-v` to enable `info`-level output and
//! `-vv` (or higher) to enable `debug`-level output. All log output goes to
//! **stderr** so that `--format json` stdout remains uncontaminated.
//!
//! # Usage
//!
//! Call [`init`] once at the start of `main`, before any other work, passing
//! the `verbose` count from the parsed CLI args.

use tracing_subscriber::{fmt, EnvFilter};

/// Initialise the global `tracing` subscriber.
///
/// | `verbosity` | active log level |
/// |-------------|--------------------|
/// | 0 (default) | logs suppressed    |
/// | 1 (`-v`)    | `INFO`             |
/// | 2+ (`-vv`)  | `DEBUG`            |
///
/// The subscriber writes to **stderr** only, keeping stdout clean for
/// structured (JSON) command output.
pub fn init(verbosity: u8) {
    let filter = match verbosity {
        0 => {
            // Suppress all log output by default.
            EnvFilter::new("off")
        }
        1 => EnvFilter::new("info"),
        _ => EnvFilter::new("debug"),
    };

    // Allow RUST_LOG to override the verbosity flag for power users.
    let filter = EnvFilter::try_from_default_env().unwrap_or(filter);

    fmt::Subscriber::builder()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .compact()
        .init();
}
