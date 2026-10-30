//! Prompts that show what is typed: [`Input`] and [`Confirm`], on any
//! [`Tty`], drawn as a [`crate::step`] and read through [`Raw`].
//!
//! Each runs the same loop: render the step for the current state, draw it
//! only if it differs from what is on screen, read a key, update. The
//! terminal's width is checked between keys (a timed read), so a resize
//! repaints the step at the new width; no SIGWINCH handler is installed for
//! it. On a terminal that cannot redraw, each prompt asks in the terminal's
//! own line mode instead ([`Tty::read_line`]).

use std::io::{self, Write};
use std::time::Duration;

use crate::glyph::{self, State};
use crate::prompt::line::{Edit, LineEditor};
use crate::render::{Block, Region};
use crate::step::{self, Body, Foot, GUTTER, Status, Step};
use crate::style::{self, Caps, Style};
use crate::term::{Event, Key, Line, Tty};

/// How often the width is checked while waiting for a key.
const RESIZE_POLL: Duration = Duration::from_millis(200);

const CURSOR_HIDE: &str = "\x1b[?25l";
const CURSOR_SHOW: &str = "\x1b[?25h";

/// How a prompt ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer<T> {
    Submitted(T),
    /// Esc or Ctrl-C. The step is left drawn as cancelled (`■`, `└
    /// Operation cancelled.`), which ends the flow's gutter.
    Cancelled,
    /// The terminal went away.
    Hangup,
    /// A signal (installed with `term::signals`): the terminal is restored
    /// and the step erased. Re-raise it.
    Interrupted(i32),
}

impl<T> Answer<T> {
    /// The submitted value through `f`; every other ending as it is.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Answer<U> {
        match self {
            Answer::Submitted(v) => Answer::Submitted(f(v)),
            Answer::Cancelled => Answer::Cancelled,
            Answer::Hangup => Answer::Hangup,
            Answer::Interrupted(sig) => Answer::Interrupted(sig),
        }
    }
}

/// What a key did to a prompt.
pub(crate) enum Next<T> {
    Stay,
    Error(String),
    Submit(T),
    Cancel,
}

/// A prompt the runner can drive.
pub(crate) trait Ask {
    type Output;
    fn render(&self, caps: Caps, cols: usize, status: Status<'_>) -> Block;
    fn key(&mut self, key: Key) -> Next<Self::Output>;
    /// Ask on a terminal that cannot be redrawn.
    fn plain(&mut self, tty: &mut Tty) -> io::Result<Answer<Self::Output>>;
}

enum Phase<T> {
    Open,
    Error(String),
    Done(T),
    Cancelled,
}

pub(crate) fn run<A: Ask>(tty: &mut Tty, ask: &mut A) -> io::Result<Answer<A::Output>> {
    let caps = tty.caps();
    if !caps.redraw {
        return ask.plain(tty);
    }
    let mut region = Region::new(caps);
    let mut raw = tty.raw()?;
    let mut size = tty.size();
    let mut shown: Option<Block> = None;
    let mut phase = Phase::Open;
    let mut hidden = false;
    loop {
        let cols = size.map_or(80, |s| usize::from(s.cols));
        let status = match &phase {
            Phase::Open => Status::Active,
            Phase::Error(e) => Status::Error(e),
            Phase::Done(_) => Status::Submit,
            Phase::Cancelled => Status::Cancel,
        };
        let block = ask.render(caps, cols, status);
        if shown.as_ref() != Some(&block) {
            // A step with no input row keeps the cursor out of sight, as
            // clack does; one with an input row shows it there.
            if block.park.is_none() && !hidden && matches!(phase, Phase::Open | Phase::Error(_)) {
                tty.write_all(CURSOR_HIDE.as_bytes())?;
                hidden = true;
            }
            region.draw(tty, &block)?;
            shown = Some(block);
        }
        match phase {
            Phase::Done(value) => {
                return finish(tty, &mut region, raw, hidden, Answer::Submitted(value));
            }
            Phase::Cancelled => return finish(tty, &mut region, raw, hidden, Answer::Cancelled),
            _ => {}
        }
        phase = match raw.read(Some(RESIZE_POLL))? {
            Event::Timeout => {
                let now = tty.size();
                if now != size {
                    size = now;
                    region.resized(now.map_or(80, |s| usize::from(s.cols)));
                    shown = None;
                }
                continue;
            }
            Event::Key(Key::Ctrl('c')) => Phase::Cancelled,
            Event::Key(key) => match ask.key(key) {
                Next::Stay => Phase::Open,
                Next::Error(e) => Phase::Error(e),
                Next::Submit(v) => Phase::Done(v),
                Next::Cancel => Phase::Cancelled,
            },
            Event::Hangup => {
                let _ = raw.restore();
                let _ = region.leave_input(tty);
                return Ok(Answer::Hangup);
            }
            Event::Interrupted(sig) => {
                let _ = region.erase(tty);
                return Ok(Answer::Interrupted(sig));
            }
        };
    }
}

fn finish<T>(
    tty: &mut Tty,
    region: &mut Region,
    mut raw: crate::term::Raw,
    hidden: bool,
    answer: Answer<T>,
) -> io::Result<Answer<T>> {
    raw.restore()?;
    region.keep(tty)?;
    if hidden {
        tty.write_all(CURSOR_SHOW.as_bytes())?;
        tty.flush()?;
    }
    Ok(answer)
}

/// A line of `text` that keeps column `col` in view within `room`: as it is
/// if it fits, else a window ending at the cursor, `…` where it was cut.
/// Returns the text and the cursor's column in it.
fn window(text: &str, col: usize, room: usize) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let width = |cs: &[char]| style::measure(&cs.iter().collect::<String>());
    let at = width(&chars[..col]);
    if at < room {
        return (text.to_string(), at);
    }
    let mut start = col;
    while start > 0 && width(&chars[start - 1..col]) + 1 < room {
        start -= 1;
    }
    let shown: String = std::iter::once('…')
        .chain(chars[start..].iter().copied())
        .collect();
    (shown, 1 + width(&chars[start..col]))
}

// ── Input ──────────────────────────────────────────────────────────────

/// A line (or lines) of text.
///
/// Empty on Enter: the default if there is one, else "Input required" when
/// required, else the empty string. Multi-line: Enter breaks the line, Esc
/// previews, and Enter in the preview submits (a key typed there goes back
/// to editing).
pub struct Input {
    label: String,
    placeholder: String,
    default: Option<String>,
    required: bool,
    multiline: bool,
    preview: bool,
    editor: LineEditor,
}

impl Input {
    pub fn new(label: &str) -> Input {
        Input {
            label: label.to_string(),
            placeholder: String::new(),
            default: None,
            required: true,
            multiline: false,
            preview: false,
            editor: LineEditor::new(),
        }
    }

    /// Dim text shown while nothing is typed.
    pub fn placeholder(mut self, text: &str) -> Input {
        self.placeholder = text.to_string();
        self
    }

    /// What an empty answer means; shown as `<default> (default)` when there
    /// is no placeholder.
    pub fn default(mut self, value: &str) -> Input {
        self.default = Some(value.to_string());
        self
    }

    pub fn required(mut self, yes: bool) -> Input {
        self.required = yes;
        self
    }

    pub fn multiline(mut self) -> Input {
        self.multiline = true;
        self
    }

    pub fn ask(mut self, tty: &mut Tty) -> io::Result<Answer<String>> {
        if self.placeholder.is_empty()
            && let Some(d) = &self.default
        {
            self.placeholder = format!("{d} (default)");
            // A default is submitted from the preview at once.
            self.preview = self.multiline;
        }
        run(tty, &mut self)
    }

    fn submit(&self) -> Next<String> {
        if !self.editor.is_empty() {
            return Next::Submit(self.editor.text());
        }
        match &self.default {
            Some(d) => Next::Submit(d.clone()),
            None if self.required => Next::Error("Input required".into()),
            None => Next::Submit(String::new()),
        }
    }
}

impl Ask for Input {
    type Output = String;

    fn render(&self, caps: Caps, cols: usize, status: Status<'_>) -> Block {
        let open = matches!(status, Status::Active | Status::Error(_));
        let room = cols.saturating_sub(GUTTER + 1).max(2);
        let (lines, (row, col)) = self.editor.lines();
        let dim = Style::new().dim();
        let (rows, cursor) = if self.editor.is_empty() {
            let shown = match status {
                // Hidden once cancelled; dim otherwise, a default included.
                Status::Cancel => String::new(),
                Status::Submit => match &self.default {
                    Some(d) => dim.paint(caps, d),
                    None => dim.paint(caps, &self.placeholder),
                },
                _ => dim.paint(caps, &self.placeholder),
            };
            (vec![shown], open.then_some((0, 0)))
        } else {
            let style = match status {
                Status::Submit => dim,
                Status::Cancel => dim.strikethrough(),
                _ => Style::new(),
            };
            let mut rows = Vec::new();
            let mut cursor = None;
            for (i, line) in lines.iter().enumerate() {
                if open && i == row {
                    let (text, at) = window(line, col, room);
                    rows.push(style.paint(caps, text));
                    cursor = Some((i, at));
                } else {
                    rows.push(style.paint(caps, line));
                }
            }
            (rows, cursor)
        };
        let foot = match (self.multiline, self.preview) {
            (true, false) => Foot::Message("[Esc](Preview)"),
            (true, true) => Foot::Message("[Enter](Submit)"),
            _ => Foot::None,
        };
        step::render(
            caps,
            &Step {
                status,
                label: &self.label,
                desc: "",
                body: Body::Rows { rows, cursor },
                foot,
            },
            cols,
        )
    }

    fn key(&mut self, key: Key) -> Next<String> {
        if self.preview {
            return match key {
                Key::Enter => self.submit(),
                Key::Esc => Next::Cancel,
                Key::Char(_) | Key::Backspace => {
                    self.preview = false;
                    self.editor.apply(key, false);
                    Next::Stay
                }
                _ => {
                    self.preview = false;
                    Next::Stay
                }
            };
        }
        match key {
            Key::Esc if self.multiline => {
                self.preview = true;
                Next::Stay
            }
            Key::Esc => Next::Cancel,
            Key::Enter if !self.multiline => self.submit(),
            key => {
                if self.editor.apply(key, self.multiline) == Edit::Ignored {
                    // Not an edit: nothing changes.
                }
                Next::Stay
            }
        }
    }

    fn plain(&mut self, tty: &mut Tty) -> io::Result<Answer<String>> {
        let mut error: Option<String> = None;
        loop {
            write_plain_step(tty, &self.label, error.as_deref())?;
            match tty.read_line(true)? {
                Line::Entered(line) => {
                    self.editor.clear();
                    line.chars().for_each(|c| self.editor.insert(c));
                    match self.submit() {
                        Next::Submit(v) => return Ok(Answer::Submitted(v)),
                        Next::Error(e) => error = Some(e),
                        _ => {}
                    }
                }
                Line::Cancelled => return Ok(Answer::Cancelled),
                Line::Hangup => return Ok(Answer::Hangup),
            }
        }
    }
}

/// `◆  label` and `│  ` for the answer, the error (if any) on a line above.
fn write_plain_step(tty: &mut Tty, label: &str, error: Option<&str>) -> io::Result<()> {
    let caps = Caps::PLAIN;
    if let Some(e) = error {
        writeln!(tty, "{}  {e}", glyph::step(caps, State::Error))?;
    }
    write!(
        tty,
        "{}  {label}\n{}  ",
        glyph::step(caps, State::Active),
        glyph::bar(caps, State::Active)
    )?;
    tty.flush()
}

// ── Confirm ────────────────────────────────────────────────────────────

/// Yes or no. The arrows (and h/j/k/l) switch, `y`/`n` answer at once,
/// Enter takes what is selected.
pub struct Confirm {
    label: String,
    value: bool,
}

impl Confirm {
    pub fn new(label: &str, default: bool) -> Confirm {
        Confirm {
            label: label.to_string(),
            value: default,
        }
    }

    pub fn ask(mut self, tty: &mut Tty) -> io::Result<Answer<bool>> {
        run(tty, &mut self)
    }

    /// One choice: clack's radio item.
    fn item(caps: Caps, status: Status<'_>, label: &str, selected: bool) -> String {
        let dim = Style::new().dim();
        match status {
            Status::Active | Status::Error(_) if selected => {
                format!("{} {label} ", Style::new().green().paint(caps, "●"))
            }
            Status::Active | Status::Error(_) => {
                format!("{} {} ", dim.paint(caps, "○"), dim.paint(caps, label))
            }
            _ if !selected => String::new(),
            Status::Submit => format!("{} ", dim.paint(caps, label)),
            _ => format!("{} ", dim.strikethrough().paint(caps, label)),
        }
    }
}

impl Ask for Confirm {
    type Output = bool;

    fn render(&self, caps: Caps, cols: usize, status: Status<'_>) -> Block {
        let yes = Self::item(caps, status, "Yes", self.value);
        let no = Self::item(caps, status, "No", !self.value);
        let divider = match status {
            Status::Active => Style::new().dim().paint(caps, " / "),
            _ => String::new(),
        };
        let row = format!("{yes}{divider}{no}");
        step::render(
            caps,
            &Step {
                status,
                label: &self.label,
                desc: "",
                body: Body::Rows {
                    rows: vec![row],
                    cursor: None,
                },
                foot: Foot::None,
            },
            cols,
        )
    }

    fn key(&mut self, key: Key) -> Next<bool> {
        match key {
            Key::Up | Key::Down | Key::Left | Key::Right | Key::Char('h' | 'j' | 'k' | 'l') => {
                self.value = !self.value;
                Next::Stay
            }
            Key::Char('y' | 'Y') => {
                self.value = true;
                Next::Submit(true)
            }
            Key::Char('n' | 'N') => {
                self.value = false;
                Next::Submit(false)
            }
            Key::Enter => Next::Submit(self.value),
            Key::Esc => Next::Cancel,
            _ => Next::Stay,
        }
    }

    fn plain(&mut self, tty: &mut Tty) -> io::Result<Answer<bool>> {
        let hint = if self.value { "[Y/n]" } else { "[y/N]" };
        loop {
            write!(
                tty,
                "{}  {} {hint}\n{}  ",
                glyph::step(Caps::PLAIN, State::Active),
                self.label,
                glyph::bar(Caps::PLAIN, State::Active)
            )?;
            tty.flush()?;
            match tty.read_line(true)? {
                Line::Entered(line) => match line.trim().to_lowercase().as_str() {
                    "" => return Ok(Answer::Submitted(self.value)),
                    "y" | "yes" => return Ok(Answer::Submitted(true)),
                    "n" | "no" => return Ok(Answer::Submitted(false)),
                    _ => writeln!(
                        tty,
                        "{}  answer y or n",
                        glyph::step(Caps::PLAIN, State::Error)
                    )?,
                },
                Line::Cancelled => return Ok(Answer::Cancelled),
                Line::Hangup => return Ok(Answer::Hangup),
            }
        }
    }
}

// ── Password ───────────────────────────────────────────────────────────

/// A password or passphrase on the primary screen: the step is drawn, then
/// nothing while it is typed (not a mask, not a counter, not a redraw), and
/// it collapses to `◇  label` with no value row once submitted. An empty
/// Enter redraws the step as `Input required`, which says nothing about what
/// was typed. The answer is wiped when dropped.
///
/// Read by [`crate::prompt::secret`], which is given the read half of the
/// terminal only.
pub struct Password {
    label: String,
}

/// The hint under an open secret prompt.
const SECRET_HINT: &str = "hidden as you type";

impl Password {
    pub fn new(label: &str) -> Password {
        Password {
            label: label.to_string(),
        }
    }

    fn step<'a>(&'a self, status: Status<'a>) -> Step<'a> {
        let open = matches!(status, Status::Active | Status::Error(_));
        Step {
            status,
            label: &self.label,
            desc: "",
            body: if open {
                Body::silent_input()
            } else {
                Body::None
            },
            foot: Foot::Hint(SECRET_HINT),
        }
    }

    pub fn ask(self, tty: &mut Tty) -> io::Result<Answer<zeroize::Zeroizing<String>>> {
        use crate::prompt::{Echo, Secret, secret};
        let caps = tty.caps();
        if !caps.redraw {
            return self.plain(tty);
        }
        let cols = tty.size().map_or(80, |s| usize::from(s.cols));
        let mut region = Region::new(caps);
        let mut raw = tty.raw()?;
        region.draw(tty, &step::render(caps, &self.step(Status::Active), cols))?;
        loop {
            let (status, answer) = match secret(&mut raw, Echo::Silent)? {
                Secret::Entered(s) => (Status::Submit, Answer::Submitted(s)),
                Secret::Empty => {
                    let required = self.step(Status::Error("Input required"));
                    region.draw(tty, &step::render(caps, &required, cols))?;
                    continue;
                }
                Secret::Cancelled => (Status::Cancel, Answer::Cancelled),
                Secret::Hangup => {
                    let _ = raw.restore();
                    let _ = region.leave_input(tty);
                    return Ok(Answer::Hangup);
                }
                Secret::Interrupted(sig) => {
                    let _ = region.erase(tty);
                    return Ok(Answer::Interrupted(sig));
                }
            };
            raw.restore()?;
            region.draw(tty, &step::render(caps, &self.step(status), cols))?;
            region.keep(tty)?;
            return Ok(answer);
        }
    }

    fn plain(self, tty: &mut Tty) -> io::Result<Answer<zeroize::Zeroizing<String>>> {
        let mut error = None;
        loop {
            write_plain_step(tty, &self.label, error)?;
            let line = tty.read_line(false)?;
            // Echo was off, the newline with it: end the input line.
            writeln!(tty)?;
            match line {
                Line::Entered(s) if s.is_empty() => error = Some("Input required"),
                Line::Entered(s) => return Ok(Answer::Submitted(s)),
                Line::Cancelled => return Ok(Answer::Cancelled),
                Line::Hangup => return Ok(Answer::Hangup),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_line_keeps_the_cursor_in_view() {
        assert_eq!(window("short", 5, 20), ("short".into(), 5));
        let (shown, at) = window(&"x".repeat(50), 50, 20);
        assert!(shown.starts_with('…'));
        assert!(style::measure(&shown) <= 20 && at < 20, "{shown} {at}");
        // The cursor in the middle of a long line: what is left of it shows.
        let (shown, at) = window("abcdefghijklmnopqrstuvwxyz", 3, 10);
        assert_eq!((shown.as_str(), at), ("abcdefghijklmnopqrstuvwxyz", 3));
    }

    #[test]
    fn an_empty_answer_takes_the_default_or_is_required() {
        let i = Input::new("Name");
        assert!(matches!(i.submit(), Next::Error(e) if e == "Input required"));
        let i = Input::new("Name").default("main");
        assert!(matches!(i.submit(), Next::Submit(v) if v == "main"));
        let i = Input::new("Name").required(false);
        assert!(matches!(i.submit(), Next::Submit(v) if v.is_empty()));
    }

    #[test]
    fn y_and_n_answer_at_once() {
        let mut c = Confirm::new("Sure?", false);
        assert!(matches!(c.key(Key::Right), Next::Stay));
        assert!(matches!(c.key(Key::Enter), Next::Submit(true)));
        assert!(matches!(c.key(Key::Char('n')), Next::Submit(false)));
    }
}
