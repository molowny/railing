//! A wait the person can watch and act on: a spinner, a countdown, a short
//! checklist, and the keys that act on it.
//!
//! ```text
//! ◆  Joining ──────────────────────────────────── 9:12 left ─╮
//! │                                                          │
//! │   ✓ code accepted                                        │
//! │   ◓ approval — on desktop run: app approve 3f9a          │
//! │   · make the store and pull it                           │
//! ├──────────────────────────────────────────────────────────╯
//! │
//! ◇  k enter recovery kit here · esc finish later
//! ```
//!
//! The box is the grid's and the picker's ([`crate::frame`]): the wait's
//! title, the countdown where they put their counts, the controls as a step
//! below. One thing spins — the active step, or the status line of a wait
//! without steps. [`Wait::freeze`] leaves the last state in place, grey and
//! without controls, as a finished grid does.
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
    /// What is being waited for as a whole (`Pairing`): the box's title.
    /// Empty, the box is titled `Waiting`.
    pub title: String,
    /// Where it stands now, in a line: alone in plain output, and the one
    /// spinning row of a wait without steps.
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
    /// The active step's spinner.
    spin: crate::spin::Spin,
    /// Interactive: lines drawn by the last tick, to redraw over.
    drawn: usize,
    /// The box's content width so far: it only grows, so the box does not
    /// jitter as a step's text changes.
    inner: usize,
}

impl Wait {
    pub(crate) fn new(live: bool) -> Self {
        Self {
            live,
            interactive: live && can_read_keys(),
            keys: Vec::new(),
            detach: None,
            said: String::new(),
            // `·` marks a pending step here, so the diamond skips its dot.
            spin: crate::spin::Spin::new().without_dot(),
            drawn: 0,
            inner: 0,
        }
    }

    /// A key that ends a tick with [`Tick::Key`]; matched case-insensitively.
    pub fn key(mut self, key: char, label: &str) -> Self {
        self.keys.push((key.to_ascii_lowercase(), label.to_owned()));
        self
    }

    /// Replaces the keys on offer, for a wait whose choices change as it goes.
    pub fn offer(&mut self, keys: &[(char, &str)]) {
        self.keys = keys
            .iter()
            .map(|(k, l)| (k.to_ascii_lowercase(), (*l).to_owned()))
            .collect();
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

    /// Leaves `view` in place as the wait's last word — grey, without the
    /// controls — instead of erasing it; nothing to do off a terminal, where
    /// the plain lines already said it. The wait is done drawing after this.
    pub fn freeze(&mut self, view: &WaitView) {
        #[cfg(feature = "tui")]
        if self.interactive {
            let rows = self.rows(view, interactive::columns(), true);
            let _ = interactive::draw(self.drawn, &rows);
            interactive::erase(0);
        }
        #[cfg(not(feature = "tui"))]
        let _ = view;
        self.drawn = 0;
    }

    /// The box for `view` on a terminal `cols` wide; `frozen` is the grey,
    /// control-less last frame.
    fn rows(&mut self, view: &WaitView, cols: usize, frozen: bool) -> Vec<String> {
        use crate::frame::{Filter, Frame, dim, max_inner};
        use crate::glyph::State;

        let title = if view.title.is_empty() {
            "Waiting"
        } else {
            view.title.as_str()
        };
        // A frozen wait is over: no time is left to count.
        let left = view
            .deadline
            .filter(|_| !frozen)
            .map(|d| time_left(d, unix_now()))
            .unwrap_or_default();
        // Each row's icon and text; the one spinner is the active step's, or
        // the status line's when there are no steps.
        let body: Vec<(String, String)> = if view.steps.is_empty() {
            let icon = if frozen {
                dim("·")
            } else {
                self.spin.symbol()
            };
            vec![(icon, view.status.clone())]
        } else {
            view.steps
                .iter()
                .map(|(mark, text)| {
                    let icon = match mark {
                        Mark::Done => style("✓").green().to_string(),
                        Mark::Active if frozen => dim("·"),
                        Mark::Active => self.spin.symbol(),
                        Mark::Pending => dim("·"),
                        Mark::Failed => style("✖").red().to_string(),
                    };
                    (icon, text.clone())
                })
                .collect()
        };
        let want = body
            .iter()
            .map(|(_, t)| width(t) + 2)
            .chain(std::iter::once(width(title) + width(&left) + 3))
            .max()
            .unwrap_or(0);
        self.inner = self.inner.max(want).min(max_inner(cols));
        let inner = self.inner;

        let state = if frozen { State::Submit } else { State::Active };
        let frame = Frame::new(inner, state);
        let mut out = frame.head(title, "", Filter::Applied, &left);
        out += &frame.line("");
        for ((icon, text), (mark, _)) in body.iter().zip(
            view.steps
                .iter()
                .map(|(m, t)| (*m, t.as_str()))
                .chain(std::iter::repeat((Mark::Active, ""))),
        ) {
            let text = fit(text, inner.saturating_sub(2));
            // What is in progress glows, as every spinner's message does.
            let text = match mark {
                Mark::Pending => dim(&text),
                Mark::Active if !frozen => self.spin.glow(&text),
                _ => text,
            };
            out += &frame.line(&format!("{icon} {text}"));
        }
        out += &frame.close();

        // The keys are the controls line the grid and the picker end with.
        let mut hint: Vec<String> = self.keys.iter().map(|(k, l)| format!("{k} {l}")).collect();
        if let Some(l) = &self.detach {
            hint.push(format!("esc {l}"));
        }
        if !frozen && !hint.is_empty() {
            out += &frame.controls(&hint.join(" · "), cols);
        }
        out.lines().map(str::to_owned).collect()
    }

    #[cfg(feature = "tui")]
    fn tick_interactive(&mut self, view: &WaitView, timeout: Duration) -> Result<Tick, CliError> {
        let io = |e: std::io::Error| CliError::Internal(anyhow::anyhow!(e));
        // Redrawn at the spinner's pace whatever the caller's tick, so the
        // diamond pulses and the glow moves as fast as every other spinner's.
        let end = std::time::Instant::now() + timeout;
        loop {
            let rows = self.rows(view, interactive::columns(), false);
            interactive::draw(self.drawn, &rows).map_err(io)?;
            self.drawn = rows.len();
            let left = end.saturating_duration_since(std::time::Instant::now());
            let slice = left.min(crate::spin::PACE);
            match interactive::read_key(slice).map_err(io)? {
                Some(interactive::Key::Detach | interactive::Key::Interrupt)
                    if self.detach.is_some() =>
                {
                    return Ok(Tick::Detach);
                }
                Some(interactive::Key::Interrupt) => {
                    // No detach offered: Ctrl‑C interrupts, as it would outside raw mode.
                    self.clear();
                    return Err(CliError::Cancelled);
                }
                Some(interactive::Key::Char(c)) => {
                    let c = c.to_ascii_lowercase();
                    if let Some((k, _)) = self.keys.iter().find(|(k, _)| *k == c) {
                        return Ok(Tick::Key(*k));
                    }
                }
                Some(interactive::Key::Detach) | None => {}
            }
            if left <= slice {
                return Ok(Tick::Idle);
            }
        }
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
            deadline,
            ..WaitView::default()
        };
        assert_eq!(plain_line(&v(None), 0), "waiting");
        assert_eq!(plain_line(&v(Some(600)), 0), "waiting (10 min left)");
        assert_eq!(plain_line(&v(Some(600)), 1), "waiting (10 min left)");
        assert_eq!(plain_line(&v(Some(600)), 61), "waiting (9 min left)");
        assert_eq!(plain_line(&v(Some(600)), 600), "waiting (time is up)");
    }

    /// Whether `row` shows a spinner frame, in any style but its dot.
    fn spinning(row: &str) -> bool {
        crate::ui::SpinnerStyle::ALL
            .iter()
            .flat_map(|s| s.frames()[1..].iter())
            .any(|f| row.contains(f))
    }

    fn plain_rows(rows: &[String]) -> Vec<String> {
        rows.iter()
            .map(|r| console::strip_ansi_codes(r).into_owned())
            .collect()
    }

    #[test]
    fn rows_fit_the_terminal() {
        assert_eq!(fit("short", 10), "short");
        assert_eq!(fit("a longer line", 8), "a longe…");
        let mut w = Wait::new(false).key('k', "kit").detach("leave");
        let rows = plain_rows(&w.rows(
            &WaitView {
                title: "Pairing".into(),
                status: "x".repeat(200),
                steps: vec![(Mark::Done, "y".repeat(200)), (Mark::Pending, "z".into())],
                deadline: None,
            },
            40,
            false,
        ));
        // head, blank, two steps, bottom rule, bar, controls
        assert_eq!(rows.len(), 7, "{rows:#?}");
        for r in &rows {
            assert!(width(r) < 40, "{r:?} is wider than 40");
        }
        assert!(rows[0].starts_with("◆  Pairing "), "{rows:#?}");
        assert_eq!(rows[6], "◇  k kit · esc leave", "{rows:#?}");
    }

    #[test]
    fn one_thing_spins_and_a_frozen_wait_has_no_controls() {
        let view = WaitView {
            title: "Pairing".into(),
            status: "waiting for approval".into(),
            steps: vec![
                (Mark::Done, "code accepted".into()),
                (Mark::Active, "approval".into()),
                (Mark::Pending, "store pulled".into()),
            ],
            deadline: Some(unix_now() + 600),
        };
        let mut w = Wait::new(false).key('k', "kit");
        let rows = plain_rows(&w.rows(&view, 80, false));
        // Only the box's body rows: the title and the controls line carry
        // the flow's own diamonds (`◆`, `◇`).
        let body = |r: &&String| r.starts_with("│   ");
        let spinners = rows.iter().filter(body).filter(|r| spinning(r)).count();
        assert_eq!(spinners, 1, "{rows:#?}");
        assert!(
            rows[0].ends_with(" left ─╮"),
            "the countdown is on the title: {rows:#?}"
        );
        assert!(
            !rows.iter().any(|r| r.contains("waiting for approval")),
            "{rows:#?}"
        );

        let frozen = plain_rows(&w.rows(&view, 80, true));
        assert!(frozen[0].starts_with("◇  Pairing "), "{frozen:#?}");
        assert!(
            !frozen[0].contains("left"),
            "no countdown once over: {frozen:#?}"
        );
        assert!(!frozen.iter().any(|r| r.contains("k kit")), "{frozen:#?}");
        assert!(
            !frozen.iter().filter(body).any(|r| spinning(r)),
            "{frozen:#?}"
        );

        // Without steps, the status line is the one that spins.
        let bare = WaitView {
            status: "waiting for the server".into(),
            ..WaitView::default()
        };
        let rows = plain_rows(&Wait::new(false).rows(&bare, 80, false));
        assert!(rows[0].starts_with("◆  Waiting "), "{rows:#?}");
        assert!(rows[2].contains("waiting for the server"), "{rows:#?}");
        assert!(spinning(&rows[2]), "{rows:#?}");
    }

    #[test]
    fn the_active_step_glows_and_nothing_else_does() {
        let _g = crate::ui::tests::lock();
        console::set_colors_enabled(true);
        console::set_colors_enabled_stderr(true);
        let view = WaitView {
            steps: vec![
                (Mark::Done, "code accepted".into()),
                (Mark::Active, "approval".into()),
                (Mark::Pending, "store pulled".into()),
            ],
            ..WaitView::default()
        };
        let mut w = Wait::new(false);
        let mut lit = false;
        for _ in 0..4 {
            for r in w.rows(&view, 80, false) {
                let bright = r.contains("38;5;255");
                if console::strip_ansi_codes(&r).contains("approval") {
                    lit |= bright;
                } else {
                    assert!(!bright, "only the active step glows: {r:?}");
                }
            }
        }
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
        assert!(lit, "the active step glows");
    }

    #[test]
    fn the_box_only_grows() {
        let mut w = Wait::new(false);
        let view = |t: &str| WaitView {
            title: "T".into(),
            steps: vec![(Mark::Active, t.into())],
            ..WaitView::default()
        };
        let wide = plain_rows(&w.rows(&view("a long step text here"), 80, false));
        let narrow = plain_rows(&w.rows(&view("short"), 80, false));
        assert_eq!(width(&wide[0]), width(&narrow[0]));
    }

    #[test]
    fn a_wait_without_a_live_flow_never_reads_keys() {
        assert!(!Wait::new(false).is_interactive());
    }
}
