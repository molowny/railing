//! One command, one prompt group: the `wright commit` look as a reusable shape.
//!
//! ```text
//! ┌  app cmd
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
        ui::intro(&title)
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
const FRAME_LINES: usize = 4;

/// Whether a table box drawn as `rendered` fits a terminal `rows` high along
/// with the frame around it.
fn fits(rendered: &str, rows: usize) -> bool {
    rendered.lines().count() + FRAME_LINES <= rows
}

/// The [`Browse`] decision, given the gate and the fit.
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

/// Where a plain (unframed) flow writes. The app picks it per command with
/// [`Flow::plain_to`]; a live flow draws everything on stderr regardless.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Plain {
    /// Data (tables, notes, blocks) on stdout; narration and the closing
    /// line on stderr. `app ls | cut …` gets the table and nothing else.
    #[default]
    Split,
    /// The command's output *is* its report: tables, blocks and the closing
    /// line all on stdout, so `app plan | less` reads like the terminal does.
    /// One-line narration ([`Flow::step`], [`Flow::warn`], …) stays on stderr.
    Stdout,
    /// Everything on stderr: stdout is kept for the one value the command
    /// prints itself (`id=$(app submit …)`).
    Stderr,
}

/// The gutter mark a [`Flow::block`] gets when live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gutter {
    /// `◇` something settled.
    Step,
    /// `●` context worth reading.
    Info,
    /// `▲` does not stop anything, but should be read.
    Warn,
    /// `■` what blocks the command.
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum To {
    Out,
    Err,
}

/// What a line of plain output is, for [`shows`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Data, narration, warnings, the closing line.
    Text,
    /// What went wrong: a failed close, a non-fatal error.
    Failure,
}

/// Whether plain output of `kind` bound for `to` is written. `quiet` (`-q`,
/// `--json`) drops text on stderr, but never a failure: `-q` is "data and
/// errors only". Under `--json` a failure is not written here either — `main`
/// prints it as the one JSON document (`docs/cli.md` §6.1).
fn shows(kind: Kind, to: To, quiet: bool, json: bool) -> bool {
    match kind {
        Kind::Failure => !json,
        Kind::Text => !(quiet && to == To::Err),
    }
}

/// One command's rendering. Live: a prompt group. Plain: no frame; where the
/// text goes is [`Plain`].
#[derive(Debug)]
#[must_use = "close the flow with done/warn_done/fail"]
pub struct Flow {
    live: bool,
    browse: Browse,
    plain: Plain,
    /// Plain output bound for stderr is dropped (`-q`, `--json`).
    quiet: bool,
    closed: std::cell::Cell<bool>,
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
            plain: Plain::default(),
            quiet: false,
            closed: Default::default(),
        }
    }

    /// A flow that never draws a frame.
    pub fn plain() -> Self {
        Self::begin("", false)
    }

    /// Where this flow writes when it is not live — the app's choice for the
    /// command ([`Plain`]).
    pub fn plain_to(mut self, plain: Plain) -> Self {
        self.plain = plain;
        self
    }

    /// Drop the plain output that would go to stderr: `-q` and `--json`
    /// promise no narration (`docs/cli.md` §2). Errors still reach `main`.
    pub fn quiet(mut self, yes: bool) -> Self {
        self.quiet = yes;
        self
    }

    /// Where plain data (tables, notes, blocks) goes.
    fn data_to(&self) -> To {
        match self.plain {
            Plain::Split | Plain::Stdout => To::Out,
            Plain::Stderr => To::Err,
        }
    }

    fn write(&self, to: To, text: &str) {
        self.write_as(Kind::Text, to, text);
    }

    #[allow(clippy::print_stdout, clippy::print_stderr)]
    fn write_as(&self, kind: Kind, to: To, text: &str) {
        if !shows(kind, to, self.quiet, crate::output::is_json()) {
            return;
        }
        match to {
            To::Out => print!("{text}"),
            To::Err => eprint!("{text}"),
        }
    }

    /// `✖ msg` on stderr, the way `CliError::render` says it.
    fn write_failure(&self, msg: &str) {
        if shows(Kind::Failure, To::Err, self.quiet, crate::output::is_json()) {
            ui::error(msg);
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
    ///
    /// Plain, a [`Plain::Split`] table is the bare data; in a report
    /// ([`Plain::Stdout`], [`Plain::Stderr`]) it is a titled block like the
    /// rest of the report.
    pub fn table(&self, title: &str, grid: &Grid) {
        if self.live {
            let _ = console::Term::stderr().write_str(&table_box(title, grid));
        } else if self.plain == Plain::Split {
            self.write(To::Out, &format!("{}\n", grid.plain()));
        } else {
            self.block(Gutter::Step, title, &grid.plain());
        }
    }

    /// A titled block: `body` may span lines (a list, a log). Live it is one
    /// gutter entry with `mark`, every line kept inside the bar; plain it is
    /// the title, the body indented under it, and a blank line.
    pub fn block(&self, mark: Gutter, title: &str, body: &str) {
        if self.live {
            let title = match mark {
                Gutter::Warn => style(title).for_stderr().yellow().bold(),
                Gutter::Error => style(title).for_stderr().red().bold(),
                _ => style(title).for_stderr().bold(),
            };
            let text = if body.is_empty() {
                title.to_string()
            } else {
                let body = ui::wrap_to(body.trim_end_matches('\n'), gutter_width());
                format!("{title}\n{body}")
            };
            let _ = match mark {
                Gutter::Step => ui::log::step(&text),
                Gutter::Info => ui::log::info(&text),
                Gutter::Warn => ui::log::warning(&text),
                Gutter::Error => ui::log::error(&text),
            };
            return;
        }
        self.write(self.data_to(), &plain_block(title, body));
    }

    /// A list's table, or — at a terminal, when the [`Browse`] policy says so
    /// — the interactive grid over it, inline in the flow (`docs/cli.md`
    /// §8.1.1). When the person finishes, the grid stays where it is, frozen
    /// on the view they ended on, its filter and sort in its title; nothing
    /// else is printed. Ctrl-C clears it and is [`CliError::Cancelled`]. If
    /// the grid cannot start, the table is printed as if it had never been
    /// asked for.
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
        let view = match crate::browse::run(crate::browse::Browser::new(title, grid)) {
            Ok(Some(view)) => view,
            Ok(None) => return Err(CliError::Cancelled),
            Err(error) => {
                tracing::warn!(%error, "cannot open the data grid; printing the table");
                self.table(title, grid);
                return Ok(Browsed::all(grid.len()));
            }
        };
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
            let _ = ui::note(title, body);
        } else {
            self.write(self.data_to(), &format!("{body}\n"));
        }
    }

    /// A completed step (`◇`).
    pub fn step(&self, msg: &str) {
        if self.live {
            let _ = ui::log::step(&wrapped(msg));
        } else {
            self.write(To::Err, &format!("{msg}\n"));
        }
    }

    /// An informational line (`●`).
    pub fn info(&self, msg: &str) {
        if self.live {
            let _ = ui::log::info(&wrapped(msg));
        } else {
            self.write(To::Err, &format!("{msg}\n"));
        }
    }

    /// A dim remark under the previous line (`│`).
    pub fn remark(&self, msg: &str) {
        if self.live {
            let _ = ui::log::remark(&ui::dim().apply_to(wrapped(msg)).to_string());
        } else {
            self.write(To::Err, &format!("{msg}\n"));
        }
    }

    /// A non-fatal warning (`▲`).
    pub fn warn(&self, msg: &str) {
        if self.live {
            let _ = ui::log::warning(&wrapped(msg));
        } else {
            let tag = style("warning:").for_stderr().yellow();
            self.write(To::Err, &format!("{tag} {msg}\n"));
        }
    }

    /// A non-fatal error inside the flow (`■`); the flow goes on.
    pub fn error(&self, msg: &str) {
        if self.live {
            let _ = ui::log::error(&wrapped(msg));
        } else {
            self.write_failure(msg);
        }
    }

    /// Slow work with a spinner (live) that ends as a `◇` line
    /// ([`Task::done`]) or leaves nothing ([`Task::clear`], or dropping it on
    /// an error). Plain, only the done line is written, as narration.
    pub fn task(&self, running: &str) -> Task<'_> {
        Task {
            flow: self,
            spinner: self.live.then(|| ui::Spinner::start(running)),
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
        self.close(Outcome::Ok, msg);
    }

    /// Close with a warning outcome.
    pub fn warn_done(self, msg: &str) {
        self.close(Outcome::Warn, msg);
    }

    /// Close with an explicit outcome. Unlike [`Flow::fail`] this is not an
    /// error: a report that found problems still ran successfully.
    pub fn end(self, outcome: Outcome, msg: &str) {
        self.close(outcome, msg);
    }

    /// Close from code that only borrows the flow — a helper that owns the
    /// closing line, a watch that ends on its summary. The first close wins;
    /// later ones (the command's own `done`) do nothing.
    ///
    /// Plain, a [`Plain::Split`] flow says it on stderr (`warning:` for a
    /// warning, `✖` for a failure); a report writes the line as is where the
    /// report goes.
    pub fn close(&self, outcome: Outcome, msg: &str) {
        if self.closed.replace(true) {
            return;
        }
        if self.live {
            end(outcome, msg);
            return;
        }
        if self.plain != Plain::Split {
            if !msg.is_empty() {
                let to = if self.plain == Plain::Stdout {
                    To::Out
                } else {
                    To::Err
                };
                let kind = if outcome == Outcome::Fail {
                    Kind::Failure
                } else {
                    Kind::Text
                };
                self.write_as(kind, to, &format!("{msg}\n"));
            }
            return;
        }
        match outcome {
            Outcome::Ok if !msg.is_empty() => self.write(To::Err, &format!("{msg}\n")),
            Outcome::Ok => {}
            Outcome::Warn => {
                let tag = style("warning:").for_stderr().yellow();
                self.write(To::Err, &format!("{tag} {msg}\n"));
            }
            Outcome::Fail => self.write_failure(msg),
        }
    }

    /// The person declined a confirmation: close saying what did not happen
    /// (`aborted; nothing was submitted`) and hand back
    /// [`CliError::Cancelled`] (exit 130), which `main` prints nothing more for.
    pub fn abort(&self, what: &str) -> CliError {
        self.close(Outcome::Warn, &format!("aborted; {what}"));
        CliError::Cancelled
    }

    /// Close as failed and hand back an error that `main` will not print again
    /// — except in JSON mode, where `main` prints it as the JSON document and
    /// nothing is said here.
    ///
    /// The message is said even under `-q` ([`Flow::quiet`]): a `Rendered`
    /// error is never printed again, so dropping it here would fail silently.
    pub fn fail(self, msg: &str) -> CliError {
        self.close(Outcome::Fail, msg);
        CliError::Rendered(msg.to_string())
    }
}

/// A [`Flow::task`]: the spinner while it runs.
#[must_use = "finish the task with done or clear"]
pub struct Task<'a> {
    flow: &'a Flow,
    spinner: Option<ui::Spinner>,
}

impl Task<'_> {
    /// Change what the spinner says.
    pub fn update(&self, msg: &str) {
        if let Some(s) = &self.spinner {
            s.update(msg);
        }
    }

    /// It finished: a `◇ msg` line (plain: `msg` as narration).
    pub fn done(mut self, msg: &str) {
        match self.spinner.take() {
            Some(s) => s.stop(msg),
            None => self.flow.write(To::Err, &format!("{msg}\n")),
        }
    }

    /// It finished and needs no line of its own.
    pub fn clear(mut self) {
        if let Some(s) = self.spinner.take() {
            s.clear();
        }
    }
}

impl Drop for Task<'_> {
    fn drop(&mut self) {
        if let Some(s) = self.spinner.take() {
            s.clear();
        }
    }
}

/// A [`Flow::block`] without a frame: the title, the body indented two
/// columns under it (blank lines kept blank), and a blank line after.
fn plain_block(title: &str, body: &str) -> String {
    let mut s = format!("{title}\n");
    for line in body.trim_end_matches('\n').lines() {
        if line.is_empty() {
            s.push('\n');
        } else {
            s.push_str(&format!("  {line}\n"));
        }
    }
    s.push('\n');
    s
}

/// The columns beside the 3-column gutter on stderr, where a live block
/// draws. A terminal that reports no width gets 100.
fn gutter_width() -> usize {
    let cols = console::Term::stderr()
        .size_checked()
        .map_or(0, |(_, c)| usize::from(c));
    let cols = if cols == 0 { 100 } else { cols };
    cols.saturating_sub(3).max(40)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_block_is_title_indented_body_and_a_blank_line() {
        assert_eq!(plain_block("Log", "a\n\nb\n"), "Log\n  a\n\n  b\n\n");
        assert_eq!(
            plain_block("Filenames  all clean", ""),
            "Filenames  all clean\n\n"
        );
    }

    #[test]
    fn quiet_drops_stderr_text_but_never_a_failure() {
        // -q: narration on stderr goes, data on stdout stays.
        assert!(!shows(Kind::Text, To::Err, true, false));
        assert!(shows(Kind::Text, To::Out, true, false));
        // A failure is said under -q, wherever it is bound…
        assert!(shows(Kind::Failure, To::Err, true, false));
        assert!(shows(Kind::Failure, To::Out, true, false));
        // …and not under --json, where main's error document says it.
        assert!(!shows(Kind::Failure, To::Err, true, true));
        // Without quiet nothing is dropped.
        assert!(shows(Kind::Text, To::Err, false, false));
    }

    #[test]
    fn plain_data_follows_the_mode() {
        let to = |p| Flow::plain().plain_to(p).data_to();
        assert!(to(Plain::Split) == To::Out);
        assert!(to(Plain::Stdout) == To::Out);
        assert!(to(Plain::Stderr) == To::Err);
    }

    #[test]
    fn the_first_close_wins_and_abort_is_cancelled() {
        let f = Flow::plain().plain_to(Plain::Stderr).quiet(true);
        assert!(matches!(
            f.abort("nothing was submitted"),
            CliError::Cancelled
        ));
        assert!(f.closed.get());
        f.close(Outcome::Ok, "never printed");
        f.done("nor this");
    }

    #[test]
    fn the_grid_never_opens_where_a_prompt_could_not() {
        for policy in [Browse::Auto, Browse::Always, Browse::Never] {
            assert!(!opens_grid(policy, false, || false), "{policy:?}");
        }
    }

    #[test]
    fn auto_opens_only_when_the_table_does_not_fit() {
        assert!(opens_grid(Browse::Auto, true, || false));
        assert!(!opens_grid(Browse::Auto, true, || true));
        assert!(opens_grid(Browse::Always, true, || true));
        assert!(!opens_grid(Browse::Never, true, || false));
    }

    #[test]
    fn fit_counts_the_rendered_box_and_the_frame() {
        let _c = crate::ui::tests::lock();
        let mut g = Grid::new(&["Id", "Title"]);
        for i in 0..10 {
            g.row([format!("{i}"), "x".into()]);
        }
        let boxed = boxed_note("Items", &g.boxed(80));
        let h = boxed.lines().count();
        // title, blank, header, 10 rows, bottom rule, trailing bar.
        assert_eq!(h, 15, "{boxed}");
        assert!(fits(&boxed, h + FRAME_LINES));
        assert!(!fits(&boxed, h + FRAME_LINES - 1));
        // A cell that wraps takes the lines it wraps to.
        let mut wide = Grid::new(&["Id", "Title"]);
        wide.row(["1".into(), "word ".repeat(40)]);
        assert!(boxed_note("Items", &wide.boxed(40)).lines().count() > 6);
    }

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
