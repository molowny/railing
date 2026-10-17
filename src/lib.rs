//! Reusable CLI helpers. The `full` feature (default) is the whole kit; off,
//! only the slim core is built: [`glyph`] (the flow's marks and tones) and
//! [`spin`] (the spinner and its glow), as plain strings over `console`, for
//! a process that draws on a terminal of its own choosing.

pub mod glyph;
pub mod spin;

#[cfg(feature = "full")]
pub mod args;
#[cfg(feature = "full")]
mod browse;
#[cfg(feature = "full")]
pub mod completion;
#[cfg(feature = "full")]
pub mod error;
#[cfg(feature = "full")]
pub mod flow;
#[cfg(feature = "full")]
mod frame;
#[cfg(feature = "full")]
pub mod fuzzy;
#[cfg(feature = "full")]
pub mod grid;
#[cfg(feature = "full")]
pub mod logging;
#[cfg(feature = "full")]
pub mod output;
#[cfg(feature = "full")]
pub mod paths;
#[cfg(feature = "full")]
mod picker;
#[cfg(feature = "full")]
pub mod prompts;
#[cfg(feature = "full")]
pub mod ui;
#[cfg(feature = "full")]
pub mod wait;
#[cfg(feature = "full")]
pub mod workspace;

#[cfg(feature = "tui")]
pub mod tui;

#[cfg(feature = "full")]
pub use comfy_table::Table;
pub use console::style;
#[cfg(feature = "full")]
pub use error::CliError;
#[cfg(feature = "full")]
pub use flow::{Browse, Browsed, Flow, Gutter, Plain, Task};
#[cfg(feature = "full")]
pub use grid::{Grid, GridCell, SortKey, Tone};
#[cfg(feature = "full")]
pub use output::{
    CheckRow, CheckStatus, ChecksReport, DiagnosticsOutput, GlobalOpts, OutputFormat, Renderable,
    make_table,
};
#[cfg(feature = "full")]
pub use wait::{Mark, Tick, Wait, WaitView};
