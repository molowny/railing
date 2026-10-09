//! One command, one prompt group: the `wright commit` look as a reusable shape.
//!
//! ```text
//! ┌   app cmd
//! │
//! ◇  Items ────────────────────────╮
//! │                                │
//! │  ID   TITLE    UPDATED         │
//! │  a1   github   2026-10-09      │
//! ├────────────────────────────────╯
//! │
//! └  ✔ 1 item
//! ```
//!
//! A [`Flow`] is either *live* (the frame above, drawn by cliclack on stderr)
//! or *plain* (no frame: data on stdout, chatter on stderr) — the app decides
//! which once, from its own policy (`--json`, `-q`, whether the streams are
//! terminals). Code that renders a command never branches on it.
//!
//! Groups nest: a flow begun inside an open one is a `◇` step, not a second
//! `┌`. The open depth is process-global so `main` can close a group that an
//! error propagated out of with [`render_error`].

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};

use console::style;

use crate::error::CliError;
pub use crate::grid::Grid;
use crate::grid::boxed_note;
use crate::output::{CheckStatus, ChecksReport, Renderable};
use crate::ui;

/// Nesting depth of open live groups.
static DEPTH: AtomicUsize = AtomicUsize::new(0);

/// Whether a live group is open.
pub fn is_open() -> bool {
    DEPTH.load(Ordering::SeqCst) > 0
}

/// Open a live group: `┌ title` at top level, a `◇ title` step when nested.
pub fn begin(title: &str) {
    let title = style(title).bold().to_string();
    let _ = if DEPTH.fetch_add(1, Ordering::SeqCst) == 0 {
        ui::intro(&format!(" {title} "))
    } else {
        ui::log::step(&title)
    };
}

/// How a group ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Warn,
    Fail,
}

/// Close the innermost live group with its outcome.
pub fn end(outcome: Outcome, message: &str) {
    let closing = outro_text(message);
    let prev = DEPTH
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |d| {
            Some(d.saturating_sub(1))
        })
        .unwrap_or(0);
    let _ = match (prev <= 1, outcome) {
        (true, Outcome::Ok) => ui::outro_success(&closing),
        (true, Outcome::Warn) => ui::outro_warning(&closing),
        (true, Outcome::Fail) => ui::outro_error(&closing),
        (false, Outcome::Ok) => ui::log::success(message),
        (false, Outcome::Warn) => ui::log::warning(message),
        (false, Outcome::Fail) => ui::log::error(message),
    };
}

/// Render an error that reached `main`. With a live group open it closes
/// every level with `└ ✖ message`; otherwise it is [`CliError::render`].
/// The exit code is the error's own either way.
pub fn render_error(err: &CliError) -> ExitCode {
    let was_open = DEPTH.swap(0, Ordering::SeqCst) > 0;
    if !was_open || crate::output::is_json() {
        return err.render();
    }
    match err.inner() {
        CliError::Rendered(_) => {}
        CliError::Cancelled => {
            let _ = cliclack::outro_cancel("Aborted.");
        }
        other => {
            tracing::error!(error = %other, "command failed");
            let _ = ui::outro_error(&outro_text(&other.to_string()));
        }
    }
    err.exit_code()
}

/// The closing `└` line is the last of the gutter, so a multi-line message
/// keeps its continuation lines aligned under the first instead of letting
/// them fall back to column 0.
fn outro_text(message: &str) -> String {
    let mut lines = message.lines();
    let mut out = lines.next().unwrap_or_default().to_string();
    for l in lines {
        out.push_str("\n   ");
        out.push_str(l);
    }
    out
}

/// Gutter log lines are not wrapped by cliclack; wrap them to the box width.
fn wrapped(msg: &str) -> String {
    ui::wrap_to(msg, note_width())
}

/// Width available inside a note box on this terminal.
fn note_width() -> usize {
    let cols = console::Term::stderr().size().1 as usize;
    cols.saturating_sub(8).clamp(40, 160)
}

/// One command's rendering. Live: a prompt group. Plain: data on stdout,
/// chatter on stderr, no frame.
#[derive(Debug)]
#[must_use = "close the flow with done/warn_done/fail"]
pub struct Flow {
    live: bool,
}

#[allow(clippy::print_stdout, clippy::print_stderr)]
impl Flow {
    /// Start a command's flow. `live` is the app's policy decision.
    pub fn begin(title: &str, live: bool) -> Self {
        if live {
            begin(title);
        }
        Self { live }
    }

    /// A flow that never draws a frame.
    pub fn plain() -> Self {
        Self { live: false }
    }

    pub fn is_live(&self) -> bool {
        self.live
    }

    /// Whether the command may ask (a picker, an input, a confirm): the frame
    /// is drawn, so the prompt has a gutter and nobody is capturing it, and
    /// stdin is a terminal, so the prompt cannot hang a script. A command that
    /// cannot ask fails instead, naming what it needed ([`CliError::needs`]).
    pub fn can_ask(&self) -> bool {
        self.live && std::io::IsTerminal::is_terminal(&std::io::stdin())
    }

    /// A table in its own titled box (live) or the bordered table on stdout
    /// (plain). Not a [`Flow::note`]: a note dims its body, a table keeps its
    /// [`crate::Tone`]s.
    pub fn table(&self, title: &str, grid: &Grid) {
        if self.live {
            let _ =
                console::Term::stderr().write_str(&boxed_note(title, &grid.boxed(note_width())));
        } else {
            println!("{}", grid.plain());
        }
    }

    /// Health checks as `✔ name  detail` rows in a titled box (live), or the
    /// report's own pretty rendering (plain). Returns the outcome the
    /// command should close with: the worst status seen.
    pub fn checks(&self, title: &str, report: &ChecksReport) -> Outcome {
        if self.live {
            let mut g = Grid::new(&["", "Check", "Detail"]).no_header(true);
            for row in &report.0 {
                let icon = match row.status {
                    CheckStatus::Pass => style("✔").green(),
                    CheckStatus::Warn => style("▲").yellow(),
                    CheckStatus::Fail => style("✖").red(),
                };
                g.row([icon.to_string(), row.name.clone(), row.detail.clone()]);
            }
            let _ = console::Term::stderr().write_str(&boxed_note(title, &g.boxed(note_width())));
        } else {
            report.render_pretty();
        }
        report.outcome()
    }

    /// A block of text in a titled box (live) or on stdout (plain).
    pub fn note(&self, title: &str, body: &str) {
        if self.live {
            let _ = ui::note(&style(title).bold().to_string(), body);
        } else {
            println!("{body}");
        }
    }

    /// A completed step (`◇`).
    pub fn step(&self, msg: &str) {
        if self.live {
            let _ = ui::log::step(&wrapped(msg));
        } else {
            eprintln!("{msg}");
        }
    }

    /// An informational line (`●`).
    pub fn info(&self, msg: &str) {
        if self.live {
            let _ = ui::log::info(&wrapped(msg));
        } else {
            eprintln!("{msg}");
        }
    }

    /// A dim remark under the previous line (`│`).
    pub fn remark(&self, msg: &str) {
        if self.live {
            let _ = ui::log::remark(&ui::dim().apply_to(wrapped(msg)).to_string());
        } else {
            eprintln!("{msg}");
        }
    }

    /// A non-fatal warning (`▲`).
    pub fn warn(&self, msg: &str) {
        if self.live {
            let _ = ui::log::warning(&wrapped(msg));
        } else {
            eprintln!("{} {msg}", style("warning:").for_stderr().yellow());
        }
    }

    /// A non-fatal error inside the flow (`■`); the flow goes on.
    pub fn error(&self, msg: &str) {
        if self.live {
            let _ = ui::log::error(&wrapped(msg));
        } else {
            ui::error(msg);
        }
    }

    /// A wait the person watches and can act on ([`crate::wait`]): drawn in
    /// place with keys in an interactive live flow, plain lines otherwise.
    pub fn wait(&self) -> crate::Wait {
        crate::Wait::new(self.live)
    }

    /// A spinner while something slow runs (live only).
    pub fn spinner(&self, msg: &str) -> Option<ui::Spinner> {
        self.live.then(|| ui::Spinner::start(msg))
    }

    /// Close with success: `└ ✔ msg` (live) or `msg` on stderr (plain).
    pub fn done(self, msg: &str) {
        if self.live {
            end(Outcome::Ok, msg);
        } else if !msg.is_empty() {
            eprintln!("{msg}");
        }
    }

    /// Close with a warning outcome.
    pub fn warn_done(self, msg: &str) {
        if self.live {
            end(Outcome::Warn, msg);
        } else {
            eprintln!("{} {msg}", style("warning:").for_stderr().yellow());
        }
    }

    /// Close with an explicit outcome. Unlike [`Flow::fail`] this is not an
    /// error: a report that found problems still ran successfully.
    pub fn end(self, outcome: Outcome, msg: &str) {
        match outcome {
            Outcome::Ok => self.done(msg),
            Outcome::Warn => self.warn_done(msg),
            Outcome::Fail if self.live => end(Outcome::Fail, msg),
            Outcome::Fail => ui::error(msg),
        }
    }

    /// Close as failed and hand back an error that `main` will not print again
    /// — except in JSON mode, where `main` prints it as the JSON document and
    /// nothing is said here.
    pub fn fail(self, msg: &str) -> CliError {
        if self.live {
            end(Outcome::Fail, msg);
        } else if !crate::output::is_json() {
            ui::error(msg);
        }
        CliError::Rendered(msg.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_lines_stay_aligned_under_the_first() {
        assert_eq!(outro_text("one"), "one");
        assert_eq!(
            outro_text("bad input\nrun --help"),
            "bad input\n   run --help"
        );
    }
}
