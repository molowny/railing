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

use tracing_subscriber::{EnvFilter, fmt};

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
///
/// **Side effect**: also calls [`crate::ui::init_colors`] to configure
/// `console` color output based on our own `IsTerminal` check, fixing a
/// class of "no colors in shell hook" bugs caused by `console`'s
/// auto-detection misfiring under unusual stderr inheritance.
pub fn init(verbosity: u8) {
    crate::ui::init_colors();

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
