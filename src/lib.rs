//! Reusable CLI helpers. The `full` feature (default) is the whole kit; off,
//! only the core is built, over libc, unicode-width and zeroize: [`style`]
//! (colour per terminal, widths), [`term`] (a named terminal, raw mode,
//! keys, signals), [`glyph`] (the flow's marks and tones) and [`spin`] (the
//! spinner and its glow), for a process that draws on a terminal of its own
//! choosing.

pub mod glyph;
pub mod group;
pub mod prompt;
pub mod render;
pub mod spin;
pub mod step;
pub mod style;
pub mod term;

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
#[cfg(all(feature = "full", unix))]
mod surface;
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
#[cfg(feature = "full")]
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
