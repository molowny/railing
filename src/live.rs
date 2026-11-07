//! Things the live flow redraws while work goes on: a [`Progress`] line that
//! animates itself, and a [`Live`] block the caller redraws at its own pace.
//!
//! ```text
//! │
//! ◒  Copying files  ━━━━━━━━━━──────────  42/100    ← Flow::progress
//!
//! │
//! │  run r-x  ·  GEOGRAFIA_3  ·  1308 files          ← Flow::live: any lines,
//! │  transcode ━━━━━━━━━━━━━──────  412/1308  31%      replaced in place
//! ```
//!
//! Both draw on stderr through the core: the line is a
//! [`crate::progress::line`] on a [`crate::spin::Animation`], the block a
//! [`crate::render::Region`]. Neither draws off a terminal: a plain flow's
//! progress says only how it ended, and a block is the caller's to say some
//! other way (one line per change).

use std::io::Write as _;
use std::sync::{Arc, Mutex};

use crate::Flow;
use crate::glyph::{self, State};
use crate::progress::{self, DONE, Part};
use crate::render::{Block, Region};
use crate::spin::Animation;
use crate::style;
use crate::ui::{stderr_caps, stderr_size};

const HIDE: &str = "\x1b[?25l";
const SHOW: &str = "\x1b[?25h";

// ------------------------------------------------------------------ Progress

/// A [`Flow::progress`]: a bar for work whose size is known, on one line
/// that animates on its own. It ends as a `◇` step ([`Progress::done`]), a
/// `■` error ([`Progress::fail`]) or nothing ([`Progress::clear`], or
/// dropping it on an error); plain, only that end is said, as narration.
///
/// Like a spinner, it is cleared before any prompt and never runs while one
/// is shown (`docs/cli.md` §4); nothing else is written while it runs.
#[must_use = "finish the progress with done, fail or clear"]
pub struct Progress<'a> {
    flow: &'a Flow,
    meter: Arc<Mutex<Meter>>,
    animation: Option<Animation>,
}

#[derive(Debug, Default)]
struct Meter {
    msg: String,
    pos: u64,
    len: u64,
}

impl<'a> Progress<'a> {
    pub(crate) fn start(flow: &'a Flow, live: bool, msg: &str, len: u64) -> Self {
        let meter = Arc::new(Mutex::new(Meter {
            msg: msg.to_owned(),
            pos: 0,
            len,
        }));
        let on_terminal = live && std::io::IsTerminal::is_terminal(&std::io::stderr());
        let animation = on_terminal.then(|| {
            let meter = Arc::clone(&meter);
            let mut first = true;
            Animation::start(
                std::io::stderr(),
                move |w, spin| {
                    let (msg, pos, len) = match meter.lock() {
                        Ok(m) => (m.msg.clone(), m.pos, m.len),
                        Err(_) => return,
                    };
                    let caps = stderr_caps();
                    let line = progress::line(
                        caps,
                        spin,
                        &msg,
                        &[Part::new(pos, DONE)],
                        len,
                        &progress::count(pos, len),
                        stderr_size().1,
                    );
                    let hide = if std::mem::take(&mut first) { HIDE } else { "" };
                    let _ = write!(w, "{hide}\r\x1b[2K{line}");
                    let _ = w.flush();
                },
                |w| {
                    let _ = write!(w, "\r\x1b[2K{SHOW}");
                    let _ = w.flush();
                },
            )
        });
        Progress {
            flow,
            meter,
            animation,
        }
    }

    fn with(&self, f: impl FnOnce(&mut Meter)) {
        if let Ok(mut m) = self.meter.lock() {
            f(&mut m);
        }
    }

    /// Move on by `n`.
    pub fn inc(&self, n: u64) {
        self.with(|m| m.pos = m.pos.saturating_add(n));
    }

    /// Be at `pos`.
    pub fn set(&self, pos: u64) {
        self.with(|m| m.pos = pos);
    }

    /// The whole is `len` after all (a listing that grew while it ran).
    pub fn set_len(&self, len: u64) {
        self.with(|m| m.len = len);
    }

    /// Say something else while it runs.
    pub fn update(&self, msg: &str) {
        self.with(|m| msg.clone_into(&mut m.msg));
    }

    /// Where it is now.
    pub fn position(&self) -> u64 {
        self.meter.lock().map_or(0, |m| m.pos)
    }

    /// It finished: a `◇ msg` step (plain: `msg` as narration).
    pub fn done(mut self, msg: &str) {
        self.halt();
        self.flow.step(msg);
    }

    /// It failed, and the flow goes on: a `■ msg` error (plain: on stderr,
    /// even under `-q`). For a failure that ends the command, `clear` it and
    /// return the error.
    pub fn fail(mut self, msg: &str) {
        self.halt();
        self.flow.error(msg);
    }

    /// It finished and needs no line of its own.
    pub fn clear(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        if let Some(a) = self.animation.take() {
            a.stop();
        }
    }
}

impl Drop for Progress<'_> {
    fn drop(&mut self) {
        self.halt();
    }
}

// ---------------------------------------------------------------------- Live

/// A block of lines on stderr that the caller redraws in place: a status
/// panel under `--watch`, a table of jobs ticking over. Each
/// [`Live::draw`] replaces the last; the caller decides when to draw (a
/// timer, a poll) and what (any styled text, a [`progress::bar`] in it).
///
/// In a flow ([`Flow::live`]) each line sits behind the gutter's `│`; bare
/// ([`Live::stderr`]) the lines are drawn as given. Either way a line is cut
/// to the terminal's width, never wrapped, and a resize between draws is
/// repainted at the new width. The cursor is hidden while it shows.
///
/// [`Live::keep`] leaves the last frame on screen (a watch's final state),
/// [`Live::clear`] takes it away; dropping it keeps it.
#[derive(Debug)]
pub struct Live {
    #[cfg(unix)]
    tty: crate::term::Tty,
    region: Region,
    gutter: bool,
    cols: usize,
    hidden: bool,
}

impl Live {
    /// A block on stderr, its lines as given: `None` unless stderr is a
    /// terminal that can be redrawn (not a pipe, not `TERM=dumb`), where the
    /// caller says it some other way.
    pub fn stderr() -> Option<Live> {
        Self::open(false)
    }

    pub(crate) fn open(gutter: bool) -> Option<Live> {
        #[cfg(unix)]
        {
            let tty = crate::ui::stderr_tty().ok()?;
            if !tty.is_terminal() || !tty.caps().redraw {
                return None;
            }
            let cols = cols_of(&tty);
            Some(Live {
                region: Region::new(tty.caps()),
                tty,
                gutter,
                cols,
                hidden: false,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = gutter;
            None
        }
    }

    /// The columns a line can take now: the terminal's width, less the
    /// gutter's `│  ` and the last column, which is never written.
    pub fn width(&self) -> usize {
        #[cfg(unix)]
        let cols = cols_of(&self.tty);
        #[cfg(not(unix))]
        let cols = self.cols;
        cols.saturating_sub(1 + self.indent())
    }

    fn indent(&self) -> usize {
        if self.gutter { 3 } else { 0 }
    }

    /// The block `text` would be at `cols`: each line behind the gutter and
    /// cut to fit.
    fn block(&self, text: &str, cols: usize) -> Block {
        let caps = self.region.caps();
        let width = cols.saturating_sub(1 + self.indent());
        let bar = glyph::bar(caps, State::Submit);
        Block::of_lines(text.lines().map(|l| {
            let l = style::truncate(l, width, "…");
            match (self.gutter, l.is_empty()) {
                (false, _) => l.into_owned(),
                (true, true) => bar.clone(),
                (true, false) => format!("{bar}  {l}"),
            }
        }))
    }

    /// A step still in progress: `head` as given, its own mark where the
    /// gutter would be (a [`progress::line`], the spinner in it), `body`
    /// behind the bar, and the bar below it, as a finished log entry leaves
    /// one. Bare, `head` and `body` are drawn as given.
    fn step_block(&self, head: &str, body: &str, cols: usize) -> Block {
        let head = style::truncate(head, cols.saturating_sub(1), "…").into_owned();
        let body = self.block(body, cols);
        let tail = self
            .gutter
            .then(|| glyph::bar(self.region.caps(), State::Submit));
        Block::of_lines(
            std::iter::once(head)
                .chain(body.text.lines().map(str::to_owned))
                .chain(tail),
        )
    }

    /// Replace what is shown with `text`.
    pub fn draw(&mut self, text: &str) {
        #[cfg(unix)]
        {
            let cols = self.repaint_cols();
            let block = self.block(text, cols);
            self.paint(&block);
        }
        #[cfg(not(unix))]
        let _ = text;
    }

    /// Replace what is shown with a step in progress: `head` is its first
    /// line with its own mark — `progress::line(caps, &spin, …)`, redrawn
    /// every [`crate::spin::PACE`] so the spinner turns — and `body` the
    /// lines under it, behind the gutter. When the step is over, [`Live::clear`]
    /// it and say how it ended as a flow step (`◇`, `■`).
    ///
    /// ```text
    /// ◒  transcode  ━━━━━━━━━━━━━━━━──────────  112/240  46%   ← head
    /// │  ✓ 110  ✗ 2  ▶ 24 running  ·  104 queued              ← body
    /// │
    /// ```
    pub fn draw_step(&mut self, head: &str, body: &str) {
        #[cfg(unix)]
        {
            let cols = self.repaint_cols();
            let block = self.step_block(head, body, cols);
            self.paint(&block);
        }
        #[cfg(not(unix))]
        let _ = (head, body);
    }

    /// The terminal's width now; a change since the last draw is counted
    /// before this one.
    #[cfg(unix)]
    fn repaint_cols(&mut self) -> usize {
        let cols = cols_of(&self.tty);
        if cols != self.cols {
            self.region.resized(cols);
            self.cols = cols;
        }
        cols
    }

    #[cfg(unix)]
    fn paint(&mut self, block: &Block) {
        if !self.hidden {
            let _ = std::io::Write::write_all(&mut self.tty, HIDE.as_bytes());
            self.hidden = true;
        }
        let _ = self.region.draw(&mut self.tty, block);
    }

    /// Leave the last frame on screen for good; what is written next goes
    /// below it.
    pub fn keep(mut self) {
        self.finish(false);
    }

    /// Take the block off the screen.
    pub fn clear(mut self) {
        self.finish(true);
    }

    fn finish(&mut self, erase: bool) {
        #[cfg(unix)]
        {
            let _ = if erase {
                self.region.erase(&mut self.tty)
            } else {
                self.region.keep(&mut self.tty)
            };
            if std::mem::take(&mut self.hidden) {
                let _ = std::io::Write::write_all(&mut self.tty, SHOW.as_bytes());
                let _ = std::io::Write::flush(&mut self.tty);
            }
        }
        #[cfg(not(unix))]
        let _ = erase;
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.finish(false);
    }
}

#[cfg(unix)]
fn cols_of(tty: &crate::term::Tty) -> usize {
    tty.size().map_or(80, |s| usize::from(s.cols))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Caps;

    fn live(gutter: bool) -> Live {
        Live {
            #[cfg(unix)]
            tty: crate::term::Tty::stderr().expect("stderr"),
            region: Region::new(Caps::PLAIN),
            gutter,
            cols: 20,
            hidden: false,
        }
    }

    #[test]
    fn lines_sit_behind_the_gutter_and_never_wrap() {
        let l = live(true);
        let b = l.block("run r-x\n\ntranscode ━━━━━━━━━━━━━━━━━━━━", 20);
        assert_eq!(b.lines, 3);
        assert_eq!(b.text, "│  run r-x\n│\n│  transcode ━━━━━…\n");
        for line in b.text.lines() {
            assert!(style::measure(line) < 20, "{line:?}");
        }
    }

    #[test]
    fn a_step_heads_its_block_with_its_own_mark() {
        let l = live(true);
        let b = l.step_block("◒  transcode ━━━━━━━━━━━━━━━━━━━━", "✓ 3\n\nrunning a", 20);
        assert_eq!(b.text, "◒  transcode ━━━━━…\n│  ✓ 3\n│\n│  running a\n│\n");
        assert_eq!(b.lines, 5);

        let bare = live(false);
        let b = bare.step_block("◒  t", "  one", 20);
        assert_eq!(b.text, "◒  t\n  one\n");
    }

    #[test]
    fn bare_lines_are_drawn_as_given() {
        let l = live(false);
        let b = l.block("  one\n  two", 20);
        assert_eq!(b.text, "  one\n  two\n");
    }
}
