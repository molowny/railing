//! Demonstrates logging bootstrap with `-v` / `-vv` verbosity.
//!
//! Run with:
//!   cargo run --example logging           # silent
//!   cargo run --example logging -- -v     # info
//!   cargo run --example logging -- -vv    # debug

use tracing::{debug, error, info, warn};

fn main() {
    let verbosity = std::env::args()
        .skip(1)
        .find(|a| a.starts_with("-v"))
        .map(|a| a.chars().filter(|c| *c == 'v').count() as u8)
        .unwrap_or(0);

    railing::logging::init(verbosity);

    info!("starting up");
    debug!(user = "alice", "loaded profile");
    warn!("cache miss, falling back to API");
    error!("this always shows via CliError::render");

    println!("(stdout stays clean — logs go to stderr)");
}
