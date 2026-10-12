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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use console::style;

use crate::error::CliError;
pub use crate::grid::Grid;
use crate::grid::boxed_note;
use crate::output::{CheckStatus, ChecksReport, Renderable};
use crate::ui;

/// Nesting depth of open live groups.
static DEPTH: AtomicUsize = AtomicUsize::new(0);

/// A cancelled prompt drew the gutter's last line (`└ Operation cancelled.`)
/// itself, so whatever was open is closed already.
static CLOSED_BY_PROMPT: AtomicBool = AtomicBool::new(false);

/// Whether a cancelled prompt has closed the gutter since the last top-level
/// group began. A closing line drawn after it would be a second `└`, so an
/// app's own group (one that does not go through [`begin`]) checks this
/// before closing on [`CliError::Cancelled`].
pub fn closed_by_prompt() -> bool {
    CLOSED_BY_PROMPT.load(Ordering::SeqCst)
}

/// A prompt was cancelled and drew its own closing line: every open group is
/// over.
pub(crate) fn prompt_cancelled() {
    DEPTH.store(0, Ordering::SeqCst);
    CLOSED_BY_PROMPT.store(true, Ordering::SeqCst);
}

/// Whether a live group is open.
pub fn is_open() -> bool {
    DEPTH.load(Ordering::SeqCst) > 0
}

/// Open a live group: `┌ title` at top level, a `◇ title` step when nested.
pub fn begin(title: &str) {
    let title = style(title).bold().to_string();
    let _ = if DEPTH.fetch_add(1, Ordering::SeqCst) == 0 {
        CLOSED_BY_PROMPT.store(false, Ordering::SeqCst);
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

/// Close the innermost live group with its outcome. Nothing is drawn once a
/// cancelled prompt has closed the gutter itself.
pub fn end(outcome: Outcome, message: &str) {
    let closing = outro_text(message);
    let prev = DEPTH
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |d| {
            Some(d.saturating_sub(1))
        })
        .unwrap_or(0);
    if closed_by_prompt() {
        return;
    }
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

/// A grid in its own titled box (`◇ Title ───╮ … ├──╯`), fitted to this
/// terminal, ready to write to stderr. What [`Flow::table`] draws live; for
/// an app that keeps its own gutter but wants the same table box.
pub fn table_box(title: &str, grid: &Grid) -> String {
    boxed_note(title, &grid.boxed(note_width()))
}

/// Pre-styled text (a status panel) in its own titled box, its styling kept
/// -- a [`Flow::note`] dims its body.
pub fn text_box(title: &str, body: &str) -> String {
    boxed_note(title, body)
}

/// The gutter's `│` in the group's bar colour, for an app that draws its own
/// lines inside the group (a live panel).
pub fn bar() -> String {
    crate::grid::gutter_bar()
}

/// Columns available inside a box on this terminal.
pub fn box_width() -> usize {
    note_width()
}

/// Width available inside a note box on this terminal.
fn note_width() -> usize {
    let cols = console::Term::stderr().size().1 as usize;
    cols.saturating_sub(8).clamp(40, 160)
}

/// When [`Flow::browse_table`] opens the interactive grid instead of printing
/// the table: the app's policy, set once with [`Flow::browse`]. Whatever the
/// policy, the grid opens only where a prompt could ([`Flow::can_ask`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Browse {
    /// When the table would not fit the terminal.
    #[default]
    Auto,
    /// Whenever the flow can ask.
    Always,
    /// Never (`--no-pager`).
    Never,
}

/// Lines of frame around a table box in a one-table command: `┌ title`, the
/// blank bar under it, and the closing `└` with the bar above it.
#[cfg(feature = "tui")]
const FRAME_LINES: usize = 4;

/// Whether a table box drawn as `rendered` fits a terminal `rows` high along
/// with the frame around it.
#[cfg(feature = "tui")]
fn fits(rendered: &str, rows: usize) -> bool {
    rendered.lines().count() + FRAME_LINES <= rows
}

/// The [`Browse`] decision, given the gate and the fit.
#[cfg(feature = "tui")]
fn opens_grid(policy: Browse, can_ask: bool, fits: impl FnOnce() -> bool) -> bool {
    can_ask
        && match policy {
            Browse::Never => false,
            Browse::Always => true,
            Browse::Auto => !fits(),
        }
}

/// What [`Flow::browse_table`] showed: the rows the person ended on, or every
/// row when the table was printed without the grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Browsed {
    /// Indices into the grid's rows, in the order shown.
    pub rows: Vec<usize>,
    /// The grid's row count.
    pub total: usize,
    /// The filter, when one was applied.
    pub query: Option<String>,
    /// `(column, descending)`, when the person sorted.
    pub sort: Option<(usize, bool)>,
    /// Whether the grid was opened.
    pub browsed: bool,
}

impl Browsed {
    #[cfg(feature = "tui")]
    fn all(total: usize) -> Self {
        Self {
            rows: (0..total).collect(),
            total,
            query: None,
            sort: None,
            browsed: false,
        }
    }

    /// Whether every row is shown.
    pub fn is_all(&self) -> bool {
        self.rows.len() == self.total
    }
}

/// One command's rendering. Live: a prompt group. Plain: data on stdout,
/// chatter on stderr, no frame.
#[derive(Debug)]
#[must_use = "close the flow with done/warn_done/fail"]
pub struct Flow {
    live: bool,
    browse: Browse,
}

#[allow(clippy::print_stdout, clippy::print_stderr)]
impl Flow {
    /// Start a command's flow. `live` is the app's policy decision.
    pub fn begin(title: &str, live: bool) -> Self {
        if live {
            begin(title);
        }
        Self {
            live,
            browse: Browse::default(),
        }
    }

    /// A flow that never draws a frame.
    pub fn plain() -> Self {
        Self {
            live: false,
            browse: Browse::default(),
        }
    }

    /// Set when [`Flow::browse_table`] opens the grid — the app's policy.
    pub fn browse(mut self, policy: Browse) -> Self {
        self.browse = policy;
        self
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
            let _ = console::Term::stderr().write_str(&table_box(title, grid));
        } else {
            println!("{}", grid.plain());
        }
    }

    /// A list's table, or — at a terminal, when the [`Browse`] policy says so
    /// — the interactive grid over it (`docs/cli.md` §8.1.1). When the person
    /// finishes, the view they ended on is printed as the table (after a step
    /// line naming the filter and sort, if any); Ctrl-C is
    /// [`CliError::Cancelled`] and prints nothing. If the grid cannot start,
    /// the table is printed as if it had never been asked for.
    #[cfg(feature = "tui")]
    pub fn browse_table(&self, title: &str, grid: &Grid) -> Result<Browsed, CliError> {
        let rows = || console::Term::stderr().size().0 as usize;
        if grid.is_empty()
            || !opens_grid(self.browse, self.can_ask(), || {
                fits(&table_box(title, grid), rows())
            })
        {
            self.table(title, grid);
            return Ok(Browsed::all(grid.len()));
        }
        use crate::tui::widgets::data_grid::DataGrid;
        let view = match crate::tui::run(DataGrid::new(title, grid)) {
            Ok(Some(view)) => view,
            Ok(None) => return Err(CliError::Cancelled),
            Err(error) => {
                tracing::warn!(%error, "cannot open the data grid; printing the table");
                self.table(title, grid);
                return Ok(Browsed::all(grid.len()));
            }
        };
        let mut said = Vec::new();
        if let Some(q) = &view.query {
            said.push(format!("filtered by '{q}'"));
        }
        if let Some((col, desc)) = view.sort {
            let name = grid.headers().get(col).map(|h| h.to_lowercase());
            let dir = if desc { "descending" } else { "ascending" };
            said.push(format!("sorted by {} {dir}", name.unwrap_or_default()));
        }
        if !said.is_empty() {
            let line = said.join(", ");
            let mut chars = line.chars();
            let first = chars.next().map(|c| c.to_uppercase().collect::<String>());
            self.step(&format!("{}{}", first.unwrap_or_default(), chars.as_str()));
        }
        if !view.rows.is_empty() {
            self.table(title, &grid.pick(&view.rows));
        }
        Ok(Browsed {
            rows: view.rows,
            total: grid.len(),
            query: view.query,
            sort: view.sort,
            browsed: true,
        })
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

    #[cfg(feature = "tui")]
    #[test]
    fn the_grid_never_opens_where_a_prompt_could_not() {
        for policy in [Browse::Auto, Browse::Always, Browse::Never] {
            assert!(!opens_grid(policy, false, || false), "{policy:?}");
        }
    }

    #[cfg(feature = "tui")]
    #[test]
    fn auto_opens_only_when_the_table_does_not_fit() {
        assert!(opens_grid(Browse::Auto, true, || false));
        assert!(!opens_grid(Browse::Auto, true, || true));
        assert!(opens_grid(Browse::Always, true, || true));
        assert!(!opens_grid(Browse::Never, true, || false));
    }

    #[cfg(feature = "tui")]
    #[test]
    fn fit_counts_the_rendered_box_and_the_frame() {
        let _c = crate::ui::tests::lock();
        let mut g = Grid::new(&["Id", "Title"]);
        for i in 0..10 {
            g.row([format!("{i}"), "x".into()]);
        }
        let boxed = boxed_note("Items", &g.boxed(80));
        let h = boxed.lines().count();
        // title, blank, header, 10 rows, blank, bottom rule, trailing bar.
        assert_eq!(h, 16, "{boxed}");
        assert!(fits(&boxed, h + FRAME_LINES));
        assert!(!fits(&boxed, h + FRAME_LINES - 1));
        // A cell that wraps takes the lines it wraps to.
        let mut wide = Grid::new(&["Id", "Title"]);
        wide.row(["1".into(), "word ".repeat(40)]);
        assert!(boxed_note("Items", &wide.boxed(40)).lines().count() > 6);
    }

    #[cfg(feature = "tui")]
    #[test]
    fn browsed_all_is_every_row_in_order() {
        let b = Browsed::all(3);
        assert_eq!(b.rows, [0, 1, 2]);
        assert!(b.is_all() && !b.browsed);
    }

    #[test]
    fn closing_lines_stay_aligned_under_the_first() {
        assert_eq!(outro_text("one"), "one");
        assert_eq!(
            outro_text("bad input\nrun --help"),
            "bad input\n   run --help"
        );
    }
}
