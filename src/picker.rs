//! The list behind [`crate::prompts::Picker`]. It keeps cliclack's look —
//! its theme draws the bar, symbols and colours — and shows what cliclack's
//! select cannot: how many rows there are and which of them are on screen,
//! and every row's hint, not only the highlighted one's. Typing filters with
//! [`Fuzzy`], the matcher the TUIs use, so a space separates words instead of
//! being dropped.

use std::io;

use cliclack::{Theme, ThemeState};
use std::borrow::Cow;

use console::{Key, Term, measure_text_width, style};

use crate::fuzzy::Fuzzy;
use crate::grid::Clack;

/// What a key did to the list.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Active,
    /// The index of the chosen row in the rows the list was given.
    Submit(usize),
    Cancel,
}

/// A row on the filtered list: where it came from and which label chars matched.
struct Shown {
    index: usize,
    hits: Vec<u32>,
}

pub(crate) struct List<'a> {
    prompt: &'a str,
    /// `(label, hint)`.
    rows: Vec<(&'a str, &'a str)>,
    height: usize,
    query: String,
    shown: Vec<Shown>,
    /// Index into `shown`.
    cursor: usize,
    /// First row of `shown` on screen.
    start: usize,
    error: Option<String>,
    fuzzy: Fuzzy,
}

impl<'a> List<'a> {
    pub(crate) fn new(prompt: &'a str, rows: Vec<(&'a str, &'a str)>, height: usize) -> Self {
        let mut list = Self {
            prompt,
            rows,
            height: height.max(1),
            query: String::new(),
            shown: Vec::new(),
            cursor: 0,
            start: 0,
            error: None,
            fuzzy: Fuzzy::new(),
        };
        list.filter();
        list
    }

    fn filter(&mut self) {
        self.shown = if self.query.trim().is_empty() {
            (0..self.rows.len())
                .map(|index| Shown {
                    index,
                    hits: Vec::new(),
                })
                .collect()
        } else {
            self.fuzzy
                .score(&self.query, self.rows.iter().map(|(label, _)| label))
                .into_iter()
                .map(|m| Shown {
                    index: m.index,
                    hits: m.indices,
                })
                .collect()
        };
        self.cursor = 0;
        self.start = 0;
    }

    fn move_to(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.shown.len().saturating_sub(1));
        if self.cursor < self.start {
            self.start = self.cursor;
        } else if self.cursor >= self.start + self.height {
            self.start = self.cursor + 1 - self.height;
        }
    }

    pub(crate) fn on(&mut self, key: Key) -> Step {
        self.error = None;
        match key {
            Key::Enter => match self.shown.get(self.cursor) {
                Some(row) => return Step::Submit(row.index),
                None => self.error = Some(format!("nothing matches '{}'", self.query)),
            },
            // Esc first clears what was typed, then leaves.
            Key::Escape if !self.query.is_empty() => {
                self.query.clear();
                self.filter();
            }
            Key::Escape | Key::CtrlC => return Step::Cancel,
            Key::ArrowUp | Key::BackTab | Key::Char('\u{10}') => {
                self.move_to(self.cursor.saturating_sub(1))
            }
            Key::ArrowDown | Key::Tab | Key::Char('\u{e}') => self.move_to(self.cursor + 1),
            Key::PageUp => self.move_to(self.cursor.saturating_sub(self.height)),
            Key::PageDown => self.move_to(self.cursor + self.height),
            Key::Home => self.move_to(0),
            Key::End => self.move_to(usize::MAX),
            Key::Backspace => {
                if self.query.pop().is_some() {
                    self.filter();
                }
            }
            // Ctrl-U clears the query, Ctrl-W drops its last word.
            Key::Char('\u{15}') => {
                self.query.clear();
                self.filter();
            }
            Key::Char('\u{17}') => {
                let kept = self.query.trim_end().rfind(' ').map_or(0, |i| i + 1);
                self.query.truncate(kept);
                self.filter();
            }
            Key::Char(c) if !c.is_control() => {
                self.query.push(c);
                self.filter();
            }
            _ => {}
        }
        Step::Active
    }

    /// `12 of 120 match · 1–8`, `1–25 of 120`, `7 items`.
    pub(crate) fn status(&self) -> String {
        let total = self.rows.len();
        let n = self.shown.len();
        let window = (n > self.height).then(|| {
            let end = (self.start + self.height).min(n);
            format!("{}–{end}", self.start + 1)
        });
        match (self.query.is_empty(), window) {
            (true, Some(w)) => format!("{w} of {total}"),
            (true, None) if total == 1 => "1 item".into(),
            (true, None) => format!("{total} items"),
            (false, _) if n == 0 => format!("no match of {total}"),
            (false, Some(w)) => format!("{n} of {total} match · {w}"),
            (false, None) => format!("{n} of {total} match"),
        }
    }

    /// The frame for `step` on a terminal `width` columns wide; every line is
    /// cut to fit, so the frame is exactly as many lines as it has `\n`s.
    pub(crate) fn render(&self, step: &Step, width: usize) -> String {
        let fit = width.saturating_sub(1).max(10);
        let state = match (step, &self.error) {
            (Step::Submit(_), _) => ThemeState::Submit,
            (Step::Cancel, _) => ThemeState::Cancel,
            (Step::Active, Some(e)) => ThemeState::Error(e.clone()),
            (Step::Active, None) => ThemeState::Active,
        };
        let bar = Clack.bar_color(&state).for_stderr().apply_to("│");
        let mut out = Clack.format_header(&state, self.prompt);
        match step {
            Step::Submit(i) => {
                let label = cut(self.rows[*i].0, fit.saturating_sub(3));
                out += &format!("{bar}  {}\n", style(label).dim().for_stderr());
                out += &format!("{bar}\n");
                return out;
            }
            Step::Cancel => return out + &Clack.format_footer(&state),
            Step::Active => {}
        }

        let query = if self.query.is_empty() {
            style("type to filter").dim().for_stderr().to_string()
        } else {
            cut(&self.query, fit.saturating_sub(4)).into_owned()
        };
        out += &format!("{bar}  {query}{}\n", style(" ").reverse().for_stderr());

        // `│  ○ ` before the label; labels share a column so hints line up.
        let room = fit.saturating_sub(5);
        let visible = &self.shown[self.start..(self.start + self.height).min(self.shown.len())];
        let label_col = visible
            .iter()
            .map(|s| measure_text_width(self.rows[s.index].0))
            .max()
            .unwrap_or(0)
            .min(room);
        for (offset, row) in visible.iter().enumerate() {
            let selected = self.start + offset == self.cursor;
            let (label, hint) = self.rows[row.index];
            let label = cut(label, label_col);
            let pad = " ".repeat(label_col - measure_text_width(&label));
            let hint_room = room.saturating_sub(label_col + 2);
            let hint = if hint.is_empty() || hint_room < 4 {
                String::new()
            } else {
                let hint = cut(hint, hint_room);
                format!("  {}", style(hint).dim().for_stderr())
            };
            out += &format!(
                "{bar}  {} {}{pad}{hint}\n",
                Clack.radio_symbol(&state, selected),
                paint_label(&label, &row.hits, selected),
            );
        }

        out + &match &state {
            ThemeState::Error(_) => Clack.format_footer(&state),
            _ => format!(
                "{}  {}\n",
                Clack.bar_color(&state).for_stderr().apply_to("└"),
                style(cut(&self.status(), fit.saturating_sub(3)))
                    .dim()
                    .for_stderr()
            ),
        }
    }
}

/// The label, dim unless it is the highlighted row, with matched chars in
/// cyan — cliclack's active colour.
fn paint_label(label: &str, hits: &[u32], selected: bool) -> String {
    label
        .chars()
        .enumerate()
        .map(|(i, c)| {
            let s = style(c).for_stderr();
            let s = if hits.contains(&(i as u32)) {
                s.cyan()
            } else if selected {
                s
            } else {
                s.dim()
            };
            s.to_string()
        })
        .collect()
}

/// `s` if it fits in `width` columns, else as much as fits followed by `…`.
fn cut(s: &str, width: usize) -> Cow<'_, str> {
    if measure_text_width(s) <= width {
        return Cow::Borrowed(s);
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = console::measure_text_width(c.encode_utf8(&mut [0; 4]));
        if used + w + 1 > width {
            break;
        }
        used += w;
        out.push(c);
    }
    if width > 0 {
        out.push('…');
    }
    Cow::Owned(out)
}

/// Draw `list` on stderr until a row is chosen (its index) or the person
/// cancels (`Interrupted`, which [`crate::error`] maps to `Cancelled`).
pub(crate) fn run(mut list: List<'_>) -> io::Result<usize> {
    if list.rows.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nothing to pick from",
        ));
    }
    let term = Term::stderr();
    if !term.is_term() {
        return Err(io::ErrorKind::NotConnected.into());
    }
    term.hide_cursor()?;
    let result = (|| {
        let mut step = Step::Active;
        let mut drawn = 0;
        loop {
            let frame = list.render(&step, usize::from(term.size().1));
            term.clear_last_lines(drawn)?;
            term.write_str(&frame)?;
            drawn = frame.lines().count();
            match step {
                Step::Submit(i) => return Ok(i),
                Step::Cancel => return Err(io::ErrorKind::Interrupted.into()),
                Step::Active => {}
            }
            step = match term.read_key() {
                Ok(key) => list.on(key),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => Step::Cancel,
                Err(e) => return Err(e),
            };
        }
    })();
    term.show_cursor()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(n: usize, height: usize) -> List<'static> {
        let rows: Vec<(&str, &str)> = (0..n)
            .map(|i| {
                let label: &'static str = Box::leak(format!("item-{i:02}").into_boxed_str());
                (label, if i % 2 == 0 { "login" } else { "note" })
            })
            .collect();
        List::new("Item", rows, height)
    }

    fn typed(l: &mut List, text: &str) {
        for c in text.chars() {
            l.on(Key::Char(c));
        }
    }

    fn plain(l: &List, step: &Step) -> String {
        console::strip_ansi_codes(&l.render(step, 80)).into_owned()
    }

    #[test]
    fn status_counts_all_rows_and_the_window() {
        let mut l = list(40, 10);
        assert_eq!(l.status(), "1–10 of 40");
        l.on(Key::End);
        assert_eq!(l.status(), "31–40 of 40");
        assert_eq!(list(3, 10).status(), "3 items");
        assert_eq!(list(1, 10).status(), "1 item");
    }

    #[test]
    fn typing_filters_and_counts_matches() {
        let mut l = list(40, 5);
        typed(&mut l, "item-1");
        assert_eq!(l.status(), "13 of 40 match · 1–5");
        typed(&mut l, "7");
        assert_eq!(l.status(), "1 of 40 match");
        assert_eq!(l.on(Key::Enter), Step::Submit(17));
        typed(&mut l, "zz");
        assert_eq!(l.status(), "no match of 40");
        assert_eq!(l.on(Key::Enter), Step::Active, "no match is not a choice");
        assert!(plain(&l, &Step::Active).contains("nothing matches"));
    }

    #[test]
    fn a_space_separates_words() {
        let mut l = List::new(
            "Item",
            vec![
                ("Private/Aws", ""),
                ("Homelab/aws-backup", ""),
                ("Private/OLX", ""),
            ],
            10,
        );
        typed(&mut l, "priv aws");
        assert_eq!(l.status(), "1 of 3 match");
        assert_eq!(l.on(Key::Enter), Step::Submit(0));
    }

    #[test]
    fn esc_clears_the_query_before_cancelling() {
        let mut l = list(5, 10);
        typed(&mut l, "item-3");
        assert_eq!(l.on(Key::Escape), Step::Active);
        assert_eq!(l.status(), "5 items");
        assert_eq!(l.on(Key::Escape), Step::Cancel);
    }

    #[test]
    fn the_cursor_scrolls_the_window() {
        let mut l = list(20, 5);
        for _ in 0..6 {
            l.on(Key::ArrowDown);
        }
        assert_eq!((l.cursor, l.start), (6, 2));
        l.on(Key::PageUp);
        assert_eq!((l.cursor, l.start), (1, 1));
        l.on(Key::ArrowUp);
        l.on(Key::ArrowUp);
        assert_eq!((l.cursor, l.start), (0, 0), "stops at the top");
        assert_eq!(l.on(Key::Enter), Step::Submit(0));
    }

    #[test]
    fn every_row_shows_its_hint_and_the_frame_fits() {
        let l = list(40, 10);
        let frame = plain(&l, &Step::Active);
        let lines: Vec<&str> = frame.lines().collect();
        // header, query, 10 rows, footer
        assert_eq!(lines.len(), 13, "{frame}");
        assert!(lines[2].contains("item-00  login"), "{frame}");
        assert!(lines[3].contains("item-01  note"), "{frame}");
        assert!(lines[12].contains("1–10 of 40"), "{frame}");

        let narrow = console::strip_ansi_codes(&l.render(&Step::Active, 20)).into_owned();
        assert!(
            narrow.lines().all(|line| measure_text_width(line) < 20),
            "{narrow}"
        );
    }

    #[test]
    fn a_choice_leaves_only_its_label() {
        let l = list(3, 10);
        let frame = plain(&l, &Step::Submit(1));
        assert!(frame.contains("item-01"), "{frame}");
        assert!(!frame.contains("item-00"), "{frame}");
    }

    #[test]
    fn cut_keeps_what_fits_whole() {
        assert_eq!(cut("abc", 3), "abc");
        assert_eq!(cut("abcd", 3), "ab…");
        assert_eq!(cut("abcd", 1), "…");
        assert_eq!(cut("abcd", 0), "");
    }
}
