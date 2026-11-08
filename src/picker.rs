//! The list behind [`crate::prompts::Picker`], drawn in the same box as the
//! browsed grid ([`crate::frame`]):
//!
//! ```text
//! ◆  Item ▸ /serv▌ ─────────────────── 13 of 60 match · 1–10 ─╮
//! │                                                            │
//! │ › Private/service-00                                login   │
//! │   Homelab/service-01                                note    │
//! ├────────────────────────────────────────────────────────────╯
//! │
//! ◇  type to filter · ↑↓ move · enter pick · esc cancel
//! ```
//!
//! A picker chooses, so — unlike the grid — it has a cursor, and typing
//! filters straight away (there are no letter shortcuts to protect). The
//! filter is [`Fuzzy`], the matcher the TUIs use, so a space separates words;
//! matches are ranked by relevance, best first. Once chosen, the box collapses
//! to the prompt and its answer, like any clack prompt.
//!
//! [`List::select`] is the same list as `prompts::select`: a short, fixed set
//! of choices with no filter, in a box sized to its rows.

// Off Unix nothing is drawn; the list and its rendering stay compiled and
// tested.
#![cfg_attr(not(unix), allow(dead_code))]

#[cfg(unix)]
use std::io;

use crate::color::style;
use crate::glyph::{self, State};
#[cfg(unix)]
use crate::prompt::Answer;
use crate::style as core_style;
#[cfg(unix)]
use crate::surface::{Input, Surface};
use crate::term::Key;
#[cfg(unix)]
use crate::term::Tty;

use crate::frame::{self, CHIP_ROOM, Filter, Frame, cut, dim, paint_hits};
use crate::fuzzy::Fuzzy;

/// Columns between the label and the hint.
const GAP: usize = 2;
const HINTS: &str = "type to filter · ↑↓ move · enter pick · esc cancel";
const QUERY_HINTS: &str = "↑↓ move · enter pick · esc clear";
const SELECT_HINTS: &str = "↑↓ move · enter pick · esc cancel";
const MULTI_HINTS: &str = "↑↓ move · space toggle · a all · enter done · esc cancel";

/// What a key did to the list.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Active,
    /// The index of the chosen row in the rows the list was given.
    Submit(usize),
    /// A multiselect's checked rows, in the order given.
    Chosen(Vec<usize>),
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
    /// Typing filters (the picker). Off for a select: a short, fixed set of
    /// choices read whole, where letters do nothing and Esc leaves at once.
    filterable: bool,
    /// A multiselect: which rows (by index in `rows`) are checked.
    checked: Option<Vec<bool>>,
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
            filterable: true,
            checked: None,
        };
        list.filter();
        list
    }

    /// The same list as a select: no filter, no chip, `jk` move, the box
    /// sized to its rows, a count only when it has to scroll.
    pub(crate) fn select(mut self) -> Self {
        self.filterable = false;
        self
    }

    /// A select where any number of rows can be checked (`space`, `a` for
    /// all) and Enter returns them; `checked` is where it starts.
    pub(crate) fn multi(mut self, checked: Vec<bool>) -> Self {
        self.filterable = false;
        self.checked = Some(checked);
        self
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
        if let Some(checked) = &mut self.checked {
            match key {
                Key::Char(' ') => {
                    let i = self.shown[self.cursor].index;
                    checked[i] = !checked[i];
                    return Step::Active;
                }
                Key::Char('a') => {
                    let all = checked.iter().all(|&c| c);
                    checked.iter_mut().for_each(|c| *c = !all);
                    return Step::Active;
                }
                Key::Enter => {
                    let rows = (0..checked.len()).filter(|&i| checked[i]).collect();
                    return Step::Chosen(rows);
                }
                _ => {}
            }
        }
        if !self.filterable {
            match key {
                Key::Enter => return Step::Submit(self.shown[self.cursor].index),
                Key::Esc | Key::Ctrl('c') => return Step::Cancel,
                Key::Up | Key::BackTab | Key::Char('k') => {
                    self.move_to(self.cursor.saturating_sub(1))
                }
                Key::Down | Key::Tab | Key::Char('j') => self.move_to(self.cursor + 1),
                Key::PageUp => self.move_to(self.cursor.saturating_sub(self.height)),
                Key::PageDown => self.move_to(self.cursor + self.height),
                Key::Home => self.move_to(0),
                Key::End => self.move_to(usize::MAX),
                _ => {}
            }
            return Step::Active;
        }
        match key {
            Key::Enter => match self.shown.get(self.cursor) {
                Some(row) => return Step::Submit(row.index),
                None => self.error = Some(format!("nothing matches '{}'", self.query)),
            },
            // Esc first clears what was typed, then leaves.
            Key::Esc if !self.query.is_empty() => {
                self.query.clear();
                self.filter();
            }
            Key::Esc | Key::Ctrl('c') => return Step::Cancel,
            Key::Up | Key::BackTab | Key::Ctrl('p') => self.move_to(self.cursor.saturating_sub(1)),
            Key::Down | Key::Tab | Key::Ctrl('n') => self.move_to(self.cursor + 1),
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
            Key::Ctrl('u') => {
                self.query.clear();
                self.filter();
            }
            Key::Ctrl('w') => {
                let kept = self.query.trim_end().rfind(' ').map_or(0, |i| i + 1);
                self.query.truncate(kept);
                self.filter();
            }
            // Typing already filters; a leading `/` — the grid's filter key,
            // pressed from habit — is not part of the query. Later ones are
            // (`Private/aws`).
            Key::Char('/') if self.query.is_empty() => {}
            Key::Char(c) if !c.is_control() => {
                self.query.push(c);
                self.filter();
            }
            _ => {}
        }
        Step::Active
    }

    /// Rows in the window: the list's height, or fewer when the list is
    /// shorter. It stays put while the person filters, so the box does not
    /// jump.
    fn window(&self) -> usize {
        self.height.min(self.rows.len()).max(1)
    }

    /// `1–10 of 120`, `13 of 120 match · 1–10`, `no match of 120` — the
    /// same words as the browsed grid's.
    pub(crate) fn status(&self) -> String {
        let total = self.rows.len();
        let n = self.shown.len();
        let end = (self.start + self.window()).min(n);
        let window = format!("{}–{end}", self.start + 1);
        if let Some(checked) = &self.checked {
            // What was checked is the news; the window only when it scrolls.
            let k = checked.iter().filter(|&&c| c).count();
            return if total > self.window() {
                format!("{k} of {total} selected · {window}")
            } else {
                format!("{k} of {total} selected")
            };
        }
        if self.query.trim().is_empty() {
            format!("{window} of {total}")
        } else if n == 0 {
            format!("no match of {total}")
        } else {
            format!("{n} of {total} match · {window}")
        }
    }

    /// The frame for `step` on a terminal `width` columns wide; every line is
    /// cut to fit, so the frame is exactly as many lines as it has `\n`s.
    pub(crate) fn render(&self, step: &Step, width: usize) -> String {
        match step {
            // Chosen: the prompt and its answer, as a submitted clack prompt.
            Step::Submit(i) => {
                let state = State::Submit;
                let bar = glyph::bar(crate::ui::stderr_caps(), state);
                let label = cut(self.rows[*i].0, width.saturating_sub(4));
                return glyph::header(crate::ui::stderr_caps(), state, self.prompt)
                    + &format!("{bar}  {}\n", style(label).dim().for_stderr())
                    + &format!("{bar}\n");
            }
            // Chosen: the prompt and what was checked, on one line.
            Step::Chosen(rows) => {
                let state = State::Submit;
                let bar = glyph::bar(crate::ui::stderr_caps(), state);
                let labels = if rows.is_empty() {
                    "none".to_string()
                } else {
                    rows.iter()
                        .map(|&i| self.rows[i].0)
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let labels = cut(&labels, width.saturating_sub(4));
                return glyph::header(crate::ui::stderr_caps(), state, self.prompt)
                    + &format!("{bar}  {}\n", style(labels).dim().for_stderr())
                    + &format!("{bar}\n");
            }
            Step::Cancel => {
                return glyph::header(crate::ui::stderr_caps(), State::Cancel, self.prompt)
                    + &format!(
                        "{}\n",
                        glyph::tinted(
                            crate::ui::stderr_caps(),
                            State::Cancel,
                            "└  Operation cancelled."
                        )
                    );
            }
            Step::Active => {}
        }

        // Sized for every row and the longest status, so the box keeps its
        // width while the person types; the label gives way last.
        let max_inner = frame::max_inner(width);
        let label_w = self.rows.iter().map(|(l, _)| core_style::measure(l)).max();
        let hint_w = self.rows.iter().map(|(_, h)| core_style::measure(h)).max();
        let (label_w, hint_w) = (label_w.unwrap_or(0), hint_w.unwrap_or(0));
        // A multiselect's checkbox (`◼ `) sits before the label.
        let check_w = if self.checked.is_some() { 2 } else { 0 };
        let table = check_w + label_w + if hint_w > 0 { GAP + hint_w } else { 0 };
        let n = self.rows.len();
        // A select has no chip and says a count only when it scrolls; a
        // picker keeps room for both while the person types.
        let (worst, chip_room) = if self.filterable {
            (format!("{n} of {n} match · {n}–{n}"), CHIP_ROOM)
        } else if self.checked.is_some() {
            (format!("{n} of {n} selected · {n}–{n}"), 0)
        } else if n > self.window() {
            (format!("{n}–{n} of {n}"), 0)
        } else {
            (String::new(), 0)
        };
        let chrome = core_style::measure(self.prompt) + core_style::measure(&worst) + 3;
        let inner = table.max(chrome + chip_room).min(max_inner);
        let label_w = label_w.min(inner.saturating_sub(check_w));
        // A hint narrower than this is noise; drop the column instead.
        let hint_w = hint_w.min(inner.saturating_sub(check_w + label_w + GAP));
        let hint_w = if hint_w < 4 { 0 } else { hint_w };

        let frame = Frame::new(inner, State::Active);
        let mut out = if self.filterable {
            frame.head(self.prompt, &self.query, Filter::Typing, &self.status())
        } else {
            let status = if self.checked.is_some() || n > self.window() {
                self.status()
            } else {
                String::new()
            };
            frame.head(self.prompt, "", Filter::Applied, &status)
        };
        out += &frame.line("");

        let window = self.window();
        let end = (self.start + window).min(self.shown.len());
        for (offset, row) in self.shown[self.start..end].iter().enumerate() {
            let selected = self.start + offset == self.cursor;
            let (label, hint) = self.rows[row.index];
            let label = cut(label, label_w);
            let base = if selected {
                crate::color::Style::new().bold()
            } else {
                crate::color::Style::new()
            };
            let mut text = match &self.checked {
                Some(checked) if checked[row.index] => style("◼ ")
                    .swatch(crate::theme::current().active)
                    .for_stderr()
                    .to_string(),
                Some(_) => dim("◻ "),
                None => String::new(),
            };
            text += &paint_hits(&label, 0, &row.hits, &base);
            if hint_w > 0 && !hint.is_empty() {
                let pad = " ".repeat(label_w - core_style::measure(&label) + GAP);
                let hint = cut(hint, hint_w);
                text += &pad;
                text += &if selected {
                    style(hint)
                        .swatch(crate::theme::current().highlight)
                        .for_stderr()
                        .to_string()
                } else {
                    dim(&hint)
                };
            }
            let marker = if selected {
                style("›")
                    .swatch(crate::theme::current().active)
                    .for_stderr()
                    .to_string()
            } else {
                " ".to_string()
            };
            out += &frame.marked(&marker, &text);
        }
        // Padded to the window, so the box keeps its height while filtering.
        for k in end - self.start..window {
            let empty = self.shown.is_empty() && k == 0;
            out += &frame.line(&if empty {
                dim(&format!("nothing matches '{}'", self.query))
            } else {
                String::new()
            });
        }

        out += &frame.close();
        out + &match &self.error {
            Some(e) => frame.warning(e, width),
            None if self.checked.is_some() => frame.controls(MULTI_HINTS, width),
            None if !self.filterable => frame.controls(SELECT_HINTS, width),
            None if self.query.is_empty() => frame.controls(HINTS, width),
            None => frame.controls(QUERY_HINTS, width),
        }
    }
}

/// Draw `list` on `tty` until a row is chosen (its index in the rows given)
/// or the person cancels.
#[cfg(unix)]
pub(crate) fn run(tty: &mut Tty, list: List<'_>) -> io::Result<Answer<usize>> {
    Ok(match drive(tty, list)? {
        Answer::Submitted(Step::Submit(i)) => Answer::Submitted(i),
        other => other.map(|_| unreachable!("a single pick ends in Submit")),
    })
}

/// [`run`] for a [`List::multi`]: the checked rows, in the order given.
#[cfg(unix)]
pub(crate) fn run_multi(tty: &mut Tty, list: List<'_>) -> io::Result<Answer<Vec<usize>>> {
    Ok(match drive(tty, list)? {
        Answer::Submitted(Step::Chosen(rows)) => Answer::Submitted(rows),
        other => other.map(|_| unreachable!("a multiselect ends in Chosen")),
    })
}

/// Draw `list` until it ends, through the shared [`Surface`]: the final
/// [`Step`] (a `Submit` or `Chosen`), drawn as the collapsed answer, or
/// cancelled, drawn as such. A terminal that cannot redraw has no list
/// ([`io::ErrorKind::Unsupported`]).
#[cfg(unix)]
fn drive(tty: &mut Tty, mut list: List<'_>) -> io::Result<Answer<Step>> {
    if list.rows.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nothing to pick from",
        ));
    }
    let mut surface = Surface::new(tty)?;
    let mut raw = surface.raw()?;
    let mut step = Step::Active;
    loop {
        surface.draw(&list.render(&step, surface.cols()))?;
        match step {
            Step::Active => {}
            Step::Cancel => {
                raw.restore()?;
                surface.keep()?;
                return Ok(Answer::Cancelled);
            }
            done => {
                raw.restore()?;
                surface.keep()?;
                return Ok(Answer::Submitted(done));
            }
        }
        step = match surface.read(&mut raw, None)? {
            Input::Key(key) => list.on(key),
            Input::Resized | Input::Timeout => Step::Active,
            Input::Hangup => {
                raw.restore()?;
                surface.keep()?;
                return Ok(Answer::Hangup);
            }
            Input::Interrupted(sig) => {
                surface.erase()?;
                return Ok(Answer::Interrupted(sig));
            }
        };
    }
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
        crate::style::strip(&l.render(step, 80)).into_owned()
    }

    #[test]
    fn status_counts_all_rows_and_the_window() {
        let mut l = list(40, 10);
        assert_eq!(l.status(), "1–10 of 40");
        l.on(Key::End);
        assert_eq!(l.status(), "31–40 of 40");
        assert_eq!(list(3, 10).status(), "1–3 of 3");
    }

    #[cfg(feature = "fuzzy")]
    #[test]
    fn typing_filters_and_counts_matches() {
        let mut l = list(40, 5);
        typed(&mut l, "item-1");
        assert_eq!(l.status(), "13 of 40 match · 1–5");
        typed(&mut l, "7");
        assert_eq!(l.status(), "1 of 40 match · 1–1");
        assert_eq!(l.on(Key::Enter), Step::Submit(17));
        typed(&mut l, "zz");
        assert_eq!(l.status(), "no match of 40");
        assert_eq!(l.on(Key::Enter), Step::Active, "no match is not a choice");
        let frame = plain(&l, &Step::Active);
        assert!(frame.contains("▲  nothing matches"), "{frame}");
    }

    /// Without the `fuzzy` feature the filter matches substrings, in the
    /// order the rows came in; the rest of the list behaves the same.
    #[cfg(not(feature = "fuzzy"))]
    #[test]
    fn without_fuzzy_typing_filters_by_substring() {
        let mut l = list(40, 5);
        typed(&mut l, "/item-1");
        assert_eq!(l.query, "item-1", "a leading slash is dropped");
        assert_eq!(l.status(), "10 of 40 match · 1–5");
        typed(&mut l, "7");
        assert_eq!(l.on(Key::Enter), Step::Submit(17));
        typed(&mut l, "zz");
        assert_eq!(l.status(), "no match of 40");
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
        assert_eq!(l.status(), "1 of 3 match · 1–1");
        assert_eq!(l.on(Key::Enter), Step::Submit(0));
    }

    #[cfg(feature = "fuzzy")]
    #[test]
    fn the_filter_is_open_from_the_start_and_a_leading_slash_is_dropped() {
        let mut l = List::new("Item", vec![("Private/aws", ""), ("Work/aws", "")], 10);
        assert!(plain(&l, &Step::Active).contains("Item ▸ /"));
        typed(&mut l, "/priv/a");
        assert_eq!(l.query, "priv/a");
        assert_eq!(l.status(), "1 of 2 match · 1–1");
    }

    #[test]
    fn a_select_does_not_filter_and_fits_its_rows() {
        let mut l = List::new(
            "Colour",
            vec![("Red", "warm"), ("Green", ""), ("Blue", "cool")],
            10,
        )
        .select();
        let frame = plain(&l, &Step::Active);
        let lines: Vec<&str> = frame.lines().collect();
        assert_eq!(lines[0], "◆  Colour ──────╮", "{frame}");
        assert!(!frame.contains('▸') && !frame.contains(" of "), "{frame}");
        assert!(lines.last().unwrap().contains("esc cancel"), "{frame}");
        typed(&mut l, "jb");
        assert_eq!(l.query, "", "letters do not filter");
        assert_eq!(l.on(Key::Enter), Step::Submit(1), "j moved down");
        assert_eq!(l.on(Key::Esc), Step::Cancel, "esc leaves at once");
    }

    #[test]
    fn a_long_select_counts_its_window() {
        let l = list(30, 5).select();
        let frame = plain(&l, &Step::Active);
        assert!(
            frame.lines().next().unwrap().ends_with(" 1–5 of 30 ─╮"),
            "{frame}"
        );
    }

    #[test]
    fn a_multiselect_toggles_counts_and_returns_the_checked_rows() {
        let mut l = List::new(
            "Toppings",
            vec![("Cheese", ""), ("Olives", "veg"), ("Ham", "")],
            10,
        )
        .multi(vec![true; 3]);
        let frame = plain(&l, &Step::Active);
        let lines: Vec<&str> = frame.lines().collect();
        assert!(lines[0].contains(" 3 of 3 selected "), "{frame}");
        assert!(lines[2].starts_with("│ › ◼ Cheese"), "{frame}");
        assert!(lines.last().unwrap().contains("space toggle"), "{frame}");
        l.on(Key::Char(' '));
        l.on(Key::Char('j'));
        let frame = plain(&l, &Step::Active);
        assert!(
            frame.contains("2 of 3 selected") && frame.contains("◻ Cheese"),
            "{frame}"
        );
        assert_eq!(l.on(Key::Enter), Step::Chosen(vec![1, 2]));
        l.on(Key::Char('a'));
        assert_eq!(
            l.on(Key::Enter),
            Step::Chosen(vec![0, 1, 2]),
            "a checks all"
        );
        l.on(Key::Char('a'));
        assert_eq!(l.on(Key::Enter), Step::Chosen(vec![]), "a again clears all");
        let done = plain(&l, &Step::Chosen(vec![0, 2]));
        assert!(done.contains("Cheese, Ham"), "{done}");
        assert_eq!(l.on(Key::Esc), Step::Cancel);
    }

    #[test]
    fn esc_clears_the_query_before_cancelling() {
        let mut l = list(5, 10);
        typed(&mut l, "item-3");
        assert_eq!(l.on(Key::Esc), Step::Active);
        assert_eq!(l.status(), "1–5 of 5");
        assert_eq!(l.on(Key::Esc), Step::Cancel);
    }

    #[test]
    fn the_cursor_scrolls_the_window() {
        let mut l = list(20, 5);
        for _ in 0..6 {
            l.on(Key::Down);
        }
        assert_eq!((l.cursor, l.start), (6, 2));
        l.on(Key::PageUp);
        assert_eq!((l.cursor, l.start), (1, 1));
        l.on(Key::Up);
        l.on(Key::Up);
        assert_eq!((l.cursor, l.start), (0, 0), "stops at the top");
        assert_eq!(l.on(Key::Enter), Step::Submit(0));
    }

    #[test]
    fn the_box_has_the_count_the_cursor_and_the_controls() {
        let mut l = list(40, 10);
        l.on(Key::Down);
        let frame = plain(&l, &Step::Active);
        let lines: Vec<&str> = frame.lines().collect();
        // head, blank, 10 rows, bottom rule, bar, controls
        assert_eq!(lines.len(), 15, "{frame}");
        assert!(
            lines[0].starts_with("◆  Item ") && lines[0].ends_with(" 1–10 of 40 ─╮"),
            "{frame}"
        );
        assert!(
            lines[2].starts_with("│   item-00  ") && lines[2].contains("login"),
            "{frame}"
        );
        assert!(
            lines[3].starts_with("│ › item-01  ") && lines[3].contains("note"),
            "{frame}"
        );
        assert!(
            lines[12].starts_with("├") && lines[12].ends_with("╯"),
            "{frame}"
        );
        assert!(lines[14].starts_with("◇  type to filter"), "{frame}");
        let w = core_style::measure(lines[0]);
        assert!(
            lines[..13].iter().all(|x| core_style::measure(x) == w),
            "{frame}"
        );

        let narrow = crate::style::strip(&l.render(&Step::Active, 20)).into_owned();
        assert!(
            narrow.lines().all(|line| core_style::measure(line) < 20),
            "{narrow}"
        );
    }

    #[test]
    fn typing_shows_the_chip_and_the_box_keeps_its_height() {
        let mut l = list(40, 10);
        let open = plain(&l, &Step::Active);
        typed(&mut l, "item-17");
        let frame = plain(&l, &Step::Active);
        assert!(frame.contains("Item ▸ /item-17"), "{frame}");
        assert!(frame.contains("esc clear"), "{frame}");
        assert_eq!(frame.lines().count(), open.lines().count());
    }

    #[test]
    fn a_choice_leaves_only_its_label() {
        let l = list(3, 10);
        let frame = plain(&l, &Step::Submit(1));
        assert!(frame.contains("item-01"), "{frame}");
        assert!(!frame.contains("item-00"), "{frame}");
    }
}
