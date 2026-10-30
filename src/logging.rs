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
/// **Side effect**: also calls [`crate::ui::init_colors`], which decides
/// colour for stdout and stderr ([`crate::color`]) from a direct
/// `IsTerminal` check on each (see there for the shell-hook bugs this
/// fixed).
pub fn init(verbosity: u8) {
    init_with_floor(verbosity, "off");
}

/// Like [`init`], but with a level that is always on at verbosity 0 — e.g.
/// `"warn"` for a tool whose failures must not be silent by default.
pub fn init_with_floor(verbosity: u8, floor: &str) {
    crate::ui::init_colors();

    let filter = match verbosity {
        0 => EnvFilter::new(floor),
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
