//! Reusable CLI helpers, in features an app picks from.
//!
//! The core is always built, over rustix and signal-hook (Unix),
//! unicode-width and zeroize:
//! [`style`] (colour per terminal, widths), [`term`] (a named terminal, raw
//! mode, keys, signals), [`render`] and [`step`] (blocks and the flow's
//! steps as strings), [`glyph`] and [`group`] (the flow's marks and lines),
//! [`spin`] (the spinner and its glow), [`progress`] (bars as strings) and
//! [`prompt`] (prompts on any
//! terminal), for a process that draws on a terminal of its own choosing.
//!
//! | Feature | Adds | Dependencies |
//! |---|---|---|
//! | `flow` | the live flow, `prompts`, `CliError`, `color`, `wait`, `live` | — |
//! | `fuzzy` | ranked matching in the picker and the grid's filter | nucleo-matcher |
//! | `table` | `Grid`, `Flow::table`, the browsed grid | — |
//! | `output` | JSON: `Renderable`, reports, `CliError::to_json` | serde, serde_json |
//! | `args` | `args::parse`, completion | clap |
//! | `logging` | the tracing subscriber | tracing |
//! | `paths` | XDG paths, private files, `workspace::find` | dirs, toml, serde |
//! | `tui` | ratatui components and the app runner | ratatui, crossterm |
//! | `full` | all but `tui` | |

// No `unsafe` anywhere in the kit: the OS is reached through rustix and
// signal-hook, whose safe APIs carry the FFI.
#![forbid(unsafe_code)]

pub mod glyph;
pub mod group;
pub mod progress;
pub mod prompt;
pub mod render;
pub mod spin;
pub mod step;
pub mod style;
pub mod term;

#[cfg(feature = "args")]
pub mod args;
#[cfg(feature = "table")]
mod browse;
#[cfg(feature = "flow")]
pub mod color;
#[cfg(feature = "args")]
pub mod completion;
#[cfg(feature = "flow")]
pub mod error;
#[cfg(feature = "flow")]
pub mod flow;
#[cfg(feature = "flow")]
mod frame;
#[cfg(any(feature = "flow", feature = "fuzzy"))]
pub mod fuzzy;
#[cfg(feature = "table")]
pub mod grid;
#[cfg(feature = "flow")]
pub mod live;
#[cfg(feature = "logging")]
pub mod logging;
#[cfg(feature = "flow")]
pub mod output;
#[cfg(feature = "paths")]
pub mod paths;
#[cfg(feature = "flow")]
mod picker;
#[cfg(feature = "flow")]
pub mod prompts;
#[cfg(all(feature = "flow", unix))]
mod surface;
#[cfg(feature = "flow")]
pub mod ui;
#[cfg(feature = "flow")]
pub mod wait;
#[cfg(feature = "paths")]
pub mod workspace;

#[cfg(feature = "tui")]
pub mod tui;

#[cfg(feature = "flow")]
pub use color::style;
#[cfg(feature = "flow")]
pub use error::CliError;
#[cfg(feature = "table")]
pub use flow::{Browse, Browsed};
#[cfg(feature = "flow")]
pub use flow::{Flow, Gutter, Plain, Task};
#[cfg(feature = "table")]
pub use grid::{Grid, GridCell, SortKey, Tone};
#[cfg(feature = "flow")]
pub use live::{Live, Progress};
#[cfg(feature = "output")]
pub use output::{CheckRow, CheckStatus, ChecksReport, DiagnosticsOutput, Renderable};
#[cfg(feature = "flow")]
pub use output::{GlobalOpts, OutputFormat};
#[cfg(feature = "flow")]
pub use wait::{Mark, Tick, Wait, WaitView};
