//! A wait the person can watch and act on: a spinner, a countdown, a short
//! checklist, and the keys that act on it.
//!
//! ```text
//! ◓  Waiting for approval…                             9:12 left
//! │   ✓ code accepted
//! │   ◓ approval — on desktop run: app approve 3f9a
//! │   · make the store and pull it
//! │  [k] enter recovery kit here   [esc] finish later
//! ```
//!
//! **Interactive** (a live [`Flow`](crate::Flow), stdin and stderr terminals,
//! the `tui` feature): drawn in place on stderr and redrawn each
//! [`Wait::tick`]. Raw mode is on only inside a tick, so a prompt the app
//! shows between ticks works as usual; [`Wait::clear`] (and dropping the
//! [`Wait`]) erases the block and restores the terminal. In raw mode Ctrl‑C
//! arrives as a key: it is [`Tick::Detach`], like Esc, when a detach label is
//! set.
//!
//! **Plain**: one line whenever the status or the minutes left change. Keys
//! cannot be read, so an app asks its questions before it starts waiting.

// Without `tui` nothing is drawn in place; the drawing code stays compiled
// and tested.
#![cfg_attr(not(feature = "tui"), allow(dead_code))]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use console::style;

use crate::error::CliError;
use crate::ui;

/// How a checklist row is marked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Done,
    Active,
    Pending,
    Failed,
}

/// What one tick shows.
#[derive(Clone, Debug, Default)]
pub struct WaitView {
    /// The headline, beside the spinner; alone in plain output.
    pub status: String,
    pub steps: Vec<(Mark, String)>,
    /// When the wait gives up, in unix seconds: a countdown.
    pub deadline: Option<u64>,
}

/// What happened during a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// Nothing: the time passed.
    Idle,
    /// One of the [`Wait::key`]s, as declared.
    Key(char),
    /// Esc or Ctrl‑C, with a [`Wait::detach`] label set.
    Detach,
}

#[must_use = "tick it until the wait ends"]
pub struct Wait {
    live: bool,
    interactive: bool,
    keys: Vec<(char, String)>,
    detach: Option<String>,
    /// Plain: the last line printed.
    said: String,
    frame: usize,
    /// Interactive: lines drawn by the last tick, to redraw over.
    drawn: usize,
}

impl Wait {
    pub(crate) fn new(live: bool) -> Self {
        Self {
            live,
            interactive: live && can_read_keys(),
            keys: Vec::new(),
            detach: None,
            said: String::new(),
            frame: 0,
            drawn: 0,
        }
    }

    /// A key that ends a tick with [`Tick::Key`]; matched case-insensitively.
    pub fn key(mut self, key: char, label: &str) -> Self {
        self.keys.push((key.to_ascii_lowercase(), label.to_owned()));
        self
    }

    /// Esc and Ctrl‑C end a tick with [`Tick::Detach`]; `label` says what that
    /// does. Without it Ctrl‑C interrupts as usual.
    pub fn detach(mut self, label: &str) -> Self {
        self.detach = Some(label.to_owned());
        self
    }

    /// Never draw in place or read keys.
    pub fn plain(mut self) -> Self {
        self.interactive = false;
        self
    }

    /// Whether keys are read: an app offers its choices as prompts otherwise.
    pub fn is_interactive(&self) -> bool {
        self.interactive
    }

    /// Shows `view` and waits up to `timeout` for a key.
    pub fn tick(&mut self, view: &WaitView, timeout: Duration) -> Result<Tick, CliError> {
        if self.interactive {
            return self.tick_interactive(view, timeout);
        }
        let line = plain_line(view, unix_now());
        if line != self.said {
            if self.live {
                let _ = ui::log::step(&line);
            } else {
                #[allow(clippy::print_stderr)]
                {
                    eprintln!("{line}");
                }
            }
            self.said = line;
        }
        std::thread::sleep(timeout);
        Ok(Tick::Idle)
    }

    /// Erases what the last tick drew and gives the terminal back, for a
    /// prompt or for what comes after the wait. The next tick draws again.
    pub fn clear(&mut self) {
        #[cfg(feature = "tui")]
        if self.interactive {
            interactive::erase(self.drawn);
        }
        self.drawn = 0;
    }

    fn rows(&mut self, view: &WaitView, cols: usize) -> Vec<String> {
        const SPIN: [&str; 4] = ["◒", "◐", "◓", "◑"];
        let spin = SPIN[self.frame % SPIN.len()];
        self.frame += 1;
        let bar = style("│").dim().to_string();
        let mut rows = Vec::with_capacity(view.steps.len() + 2);

        let left = view
            .deadline
            .map(|d| time_left(d, unix_now()))
            .unwrap_or_default();
        let room = cols.saturating_sub(3 + width(&left) + 2);
        let status = fit(&view.status, room);
        let gap = " ".repeat(room.saturating_sub(width(&status)) + 2);
        rows.push(format!(
            "{}  {}{gap}{}",
            style(spin).magenta(),
            status,
            style(left).dim()
        ));
        for (mark, text) in &view.steps {
            let icon = match mark {
                Mark::Done => style("✓").green().to_string(),
                Mark::Active => style(spin).cyan().to_string(),
                Mark::Pending => style("·").dim().to_string(),
                Mark::Failed => style("✖").red().to_string(),
            };
            let text = fit(text, cols.saturating_sub(6));
            let text = match mark {
                Mark::Pending => style(text).dim().to_string(),
                _ => text,
            };
            rows.push(format!("{bar}   {icon} {text}"));
        }
        let mut hint: Vec<String> = self
            .keys
            .iter()
            .map(|(k, l)| format!("[{k}] {l}"))
            .collect();
        if let Some(l) = &self.detach {
            hint.push(format!("[esc] {l}"));
        }
        if !hint.is_empty() {
            let hint = fit(&hint.join("   "), cols.saturating_sub(3));
            rows.push(format!("{bar}  {}", style(hint).dim()));
        }
        rows
    }

    #[cfg(feature = "tui")]
    fn tick_interactive(&mut self, view: &WaitView, timeout: Duration) -> Result<Tick, CliError> {
        let cols = interactive::columns();
        let rows = self.rows(view, cols);
        let io = |e: std::io::Error| CliError::Internal(anyhow::anyhow!(e));
        interactive::draw(self.drawn, &rows).map_err(io)?;
        self.drawn = rows.len();
        let key = interactive::read_key(timeout).map_err(io)?;
        Ok(match key {
            Some(interactive::Key::Detach | interactive::Key::Interrupt)
                if self.detach.is_some() =>
            {
                Tick::Detach
            }
            Some(interactive::Key::Interrupt) => {
                // No detach offered: Ctrl‑C interrupts, as it would outside raw mode.
                self.clear();
                return Err(CliError::Cancelled);
            }
            Some(interactive::Key::Char(c)) => self
                .keys
                .iter()
                .find(|(k, _)| *k == c.to_ascii_lowercase())
                .map_or(Tick::Idle, |(k, _)| Tick::Key(*k)),
            _ => Tick::Idle,
        })
    }

    #[cfg(not(feature = "tui"))]
    fn tick_interactive(&mut self, _: &WaitView, _: Duration) -> Result<Tick, CliError> {
        unreachable!("interactive only with the tui feature")
    }
}

impl Drop for Wait {
    fn drop(&mut self) {
        self.clear();
    }
}

fn can_read_keys() -> bool {
    #[cfg(feature = "tui")]
    {
        use std::io::IsTerminal;
        std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
    }
    #[cfg(not(feature = "tui"))]
    false
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `9:12 left`, `1:03:00 left`, or `time is up`.
fn time_left(deadline: u64, now: u64) -> String {
    let Some(s) = deadline.checked_sub(now).filter(|s| *s > 0) else {
        return "time is up".into();
    };
    if s >= 3600 {
        format!("{}:{:02}:{:02} left", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02} left", s / 60, s % 60)
    }
}

/// The plain line: the status, and whole minutes left (so it changes at most
/// once a minute).
fn plain_line(view: &WaitView, now: u64) -> String {
    match view.deadline {
        Some(d) if d > now => {
            let min = (d - now).div_ceil(60);
            format!("{} ({min} min left)", view.status)
        }
        Some(_) => format!("{} (time is up)", view.status),
        None => view.status.clone(),
    }
}

fn width(s: &str) -> usize {
    #[cfg(feature = "tui")]
    {
        unicode_width::UnicodeWidthStr::width(s)
    }
    #[cfg(not(feature = "tui"))]
    s.chars().count()
}

/// `s` cut to `cols` columns, ending in `…` when cut: a row that wrapped
/// would throw off the redraw.
fn fit(s: &str, cols: usize) -> String {
    if width(s) <= cols {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = width(c.encode_utf8(&mut [0; 4]));
        if used + w + 1 > cols {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

#[cfg(feature = "tui")]
mod interactive {
    use std::io::Write;
    use std::time::{Duration, Instant};

    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use crossterm::{cursor, execute, queue, terminal};

    pub(super) enum Key {
        Char(char),
        Detach,
        Interrupt,
    }

    pub(super) fn columns() -> usize {
        // A pty nobody sized reports 0.
        match terminal::size() {
            Ok((c, _)) if c > 0 => c as usize,
            _ => 80,
        }
    }

    /// Draws `rows` over the `drawn` rows the last tick left.
    pub(super) fn draw(drawn: usize, rows: &[String]) -> std::io::Result<()> {
        let mut err = std::io::stderr().lock();
        if drawn > 0 {
            queue!(err, cursor::MoveToPreviousLine(drawn as u16))?;
        }
        queue!(
            err,
            cursor::Hide,
            cursor::MoveToColumn(0),
            terminal::Clear(terminal::ClearType::FromCursorDown)
        )?;
        for r in rows {
            write!(err, "{r}\r\n")?;
        }
        err.flush()
    }

    pub(super) fn erase(drawn: usize) {
        let mut err = std::io::stderr().lock();
        if drawn > 0 {
            let _ = queue!(
                err,
                cursor::MoveToPreviousLine(drawn as u16),
                terminal::Clear(terminal::ClearType::FromCursorDown)
            );
        }
        let _ = execute!(err, cursor::Show);
    }

    /// The next key within `timeout`, in raw mode for that long only.
    pub(super) fn read_key(timeout: Duration) -> std::io::Result<Option<Key>> {
        terminal::enable_raw_mode()?;
        let got = next_key(timeout);
        let _ = terminal::disable_raw_mode();
        got
    }

    fn next_key(timeout: Duration) -> std::io::Result<Option<Key>> {
        let end = Instant::now() + timeout;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() || !event::poll(left)? {
                return Ok(None);
            }
            let Event::Key(k) = event::read()? else {
                continue;
            };
            if k.kind != KeyEventKind::Press {
                continue;
            }
            return Ok(Some(match k.code {
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => Key::Interrupt,
                KeyCode::Esc => Key::Detach,
                KeyCode::Char(c) => Key::Char(c),
                _ => continue,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_left_counts_down_and_stops() {
        assert_eq!(time_left(1_000 + 552, 1_000), "9:12 left");
        assert_eq!(time_left(1_000 + 3_780, 1_000), "1:03:00 left");
        assert_eq!(time_left(1_000, 1_000), "time is up");
        assert_eq!(time_left(900, 1_000), "time is up");
    }

    #[test]
    fn the_plain_line_changes_once_a_minute() {
        let v = |deadline| WaitView {
            status: "waiting".into(),
            steps: vec![],
            deadline,
        };
        assert_eq!(plain_line(&v(None), 0), "waiting");
        assert_eq!(plain_line(&v(Some(600)), 0), "waiting (10 min left)");
        assert_eq!(plain_line(&v(Some(600)), 1), "waiting (10 min left)");
        assert_eq!(plain_line(&v(Some(600)), 61), "waiting (9 min left)");
        assert_eq!(plain_line(&v(Some(600)), 600), "waiting (time is up)");
    }

    #[test]
    fn rows_fit_the_terminal() {
        assert_eq!(fit("short", 10), "short");
        assert_eq!(fit("a longer line", 8), "a longe…");
        let mut w = Wait::new(false).key('k', "kit").detach("leave");
        let rows = w.rows(
            &WaitView {
                status: "x".repeat(200),
                steps: vec![(Mark::Done, "y".repeat(200)), (Mark::Pending, "z".into())],
                deadline: None,
            },
            40,
        );
        assert_eq!(rows.len(), 4, "status, two steps, the keys");
        for r in &rows {
            let plain = console::strip_ansi_codes(r);
            assert!(width(&plain) <= 40, "{plain:?} is wider than 40");
        }
        assert!(console::strip_ansi_codes(&rows[3]).contains("[k] kit   [esc] leave"));
    }

    #[test]
    fn a_wait_without_a_live_flow_never_reads_keys() {
        assert!(!Wait::new(false).is_interactive());
    }
}
