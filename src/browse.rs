//! The grid behind [`crate::Flow::browse_table`]: a long [`Grid`] drawn
//! inline in the flow, scrolled, filtered and sorted in place.
//!
//! ```text
//! ◆  Items ▸ [/]git ──────────── 50 of 200 match · 1–4 · size ▲ ─╮
//! │                                                              │
//! │   ID      TYPE    TITLE      SIZE ▲  STATE                   │
//! │   784f84  login   item 076   1844 B  synced                  │
//! │   …                                                          │
//! ├──────────────────────────────────────────────────────────────╯
//! │
//! ◇  ↑↓ scroll · pgup/pgdn page · / filter · 1-9 sort · r reverse · q done
//! ```
//!
//! It is drawn on stderr below the flow's gutter and redrawn in place, the
//! way the picker is (`clear_last_lines` + `read_key`) — never on the
//! alternate screen. When the person finishes, the frame freezes where it is,
//! like a submitted clack prompt: the window they ended on, the filter and
//! sort in its title. Ctrl-C clears it.
//!
//! The grid is read, not picked from (that is [`crate::prompts::Picker`]), so
//! there is no cursor: ↑↓ scroll the window. Rows keep the order they came in
//! until the person sorts. Sorting is by each cell's [`SortKey`], stable over
//! that incoming order, with missing values last both ways. The filter only
//! hides rows: a table is read in its sort order, so matches are not
//! re-ranked by relevance the way a picker's are. The keys and what they mean
//! are `docs/cli.md` §8.1.1.

use std::io;

use crate::glyph::State;
use console::{Key, Term, measure_text_width, style};

use crate::frame::{self, CHIP_ROOM, Filter, Frame, align, cut, dim, paint_hits};
use crate::fuzzy::Fuzzy;
use crate::grid::{Grid, SortKey, Tone};

/// Columns between two table columns.
const GAP: usize = 2;
/// A column is never squeezed below this (or its natural width, if less).
const MIN_COL: usize = 6;
/// Lines of frame besides the lead lines and rows: the title rule, the blank
/// under it, the column header, the bottom rule, the bar and the controls.
const CHROME: usize = 6;
/// Lines kept free above the frame, so the steps before it stay in view.
const ABOVE: usize = 6;
/// The tallest window, however tall the terminal.
const MAX_ROWS: usize = 40;
const HINTS: &str = "↑↓ scroll · pgup/pgdn page · / filter · 1-9 sort · r reverse · q done";
const EDIT_HINTS: &str = "enter keep · esc clear · ^c cancel";

/// Where the person left the grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct View {
    /// Indices into the grid's rows, in the order shown.
    pub rows: Vec<usize>,
    /// The filter, when one was applied.
    pub query: Option<String>,
    /// `(column, descending)`, when the person sorted.
    pub sort: Option<(usize, bool)>,
}

/// What a key did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Active,
    Done,
    Cancel,
}

pub(crate) struct Browser<'a> {
    title: &'a str,
    grid: &'a Grid,
    columns: usize,
    /// Each row's cells on one line, per column.
    cells: Vec<Vec<String>>,
    /// Each row's sort key per column, computed once.
    keys: Vec<Vec<SortKey>>,
    /// Each row's text, for the filter.
    hays: Vec<String>,
    /// Char offset of each cell inside its row's hay, to place matches.
    starts: Vec<Vec<u32>>,
    /// Widest text per column (header and sort arrow included), for fitting.
    natural: Vec<usize>,
    /// Every row, in the current sort order.
    order: Vec<usize>,
    /// `order` with the filter applied: `(row, matched chars in its hay)`.
    shown: Vec<(usize, Vec<u32>)>,
    sort: Option<(usize, bool)>,
    query: String,
    /// The filter is being typed.
    editing: bool,
    /// Index into `shown` of the first row in the window.
    top: usize,
    /// Rows in the window, as of the last frame.
    height: usize,
    fuzzy: Fuzzy,
}

impl<'a> Browser<'a> {
    pub(crate) fn new(title: &'a str, grid: &'a Grid) -> Self {
        let rows = grid.rows();
        let columns = rows
            .iter()
            .map(Vec::len)
            .chain(std::iter::once(grid.headers().len()))
            .max()
            .unwrap_or(0);
        let cells: Vec<Vec<String>> = rows
            .iter()
            .map(|r| {
                (0..columns)
                    .map(|c| {
                        r.get(c)
                            .map(|cell| one_line(cell.text()))
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .collect();
        let keys = rows
            .iter()
            .map(|r| {
                (0..columns)
                    .map(|c| r.get(c).map_or(SortKey::Missing, |cell| cell.sort_key()))
                    .collect()
            })
            .collect();
        let mut hays = Vec::with_capacity(cells.len());
        let mut starts = Vec::with_capacity(cells.len());
        for row in &cells {
            let mut hay = String::new();
            let mut at = Vec::with_capacity(columns);
            for (c, text) in row.iter().enumerate() {
                if c > 0 {
                    hay.push_str("  ");
                }
                at.push(hay.chars().count() as u32);
                hay.push_str(text);
            }
            hays.push(hay);
            starts.push(at);
        }
        let natural = (0..columns)
            .map(|c| {
                let head = grid
                    .headers()
                    .get(c)
                    .map_or(0, |h| measure_text_width(h) + 2);
                cells
                    .iter()
                    .map(|r| measure_text_width(&r[c]))
                    .fold(head, usize::max)
            })
            .collect();
        let order: Vec<usize> = (0..cells.len()).collect();
        Self {
            title,
            grid,
            columns,
            cells,
            keys,
            hays,
            starts,
            natural,
            shown: order.iter().map(|&r| (r, Vec::new())).collect(),
            order,
            sort: None,
            query: String::new(),
            editing: false,
            top: 0,
            height: 1,
            fuzzy: Fuzzy::new(),
        }
    }

    /// The view as it stands.
    pub(crate) fn view(&self) -> View {
        View {
            rows: self.shown.iter().map(|(r, _)| *r).collect(),
            query: (!self.query.trim().is_empty()).then(|| self.query.clone()),
            sort: self.sort,
        }
    }

    fn set_sort(&mut self, sort: Option<(usize, bool)>) {
        self.sort = sort;
        self.order = (0..self.keys.len()).collect();
        if let Some((col, desc)) = sort {
            // Stable: ties keep the order the command gave.
            self.order
                .sort_by(|&a, &b| self.keys[a][col].cmp_sorted(&self.keys[b][col], desc));
        }
        self.top = 0;
        self.refilter();
    }

    /// Sort by `col`; sorting by the column already sorted reverses it.
    fn sort_by_column(&mut self, col: usize) {
        if col >= self.columns {
            return;
        }
        let desc = matches!(self.sort, Some((c, d)) if c == col && !d);
        self.set_sort(Some((col, desc)));
    }

    /// Recompute `shown` from `order` and the query.
    fn refilter(&mut self) {
        if self.query.trim().is_empty() {
            self.shown = self.order.iter().map(|&r| (r, Vec::new())).collect();
        } else {
            let mut hits: Vec<Option<Vec<u32>>> = vec![None; self.hays.len()];
            for m in self.fuzzy.score(&self.query, &self.hays) {
                hits[m.index] = Some(m.indices);
            }
            self.shown = self
                .order
                .iter()
                .filter_map(|&r| hits[r].take().map(|ix| (r, ix)))
                .collect();
        }
        self.scroll_to(self.top as isize);
    }

    /// Put row `top` of `shown` first in the window, never past the last
    /// full window.
    fn scroll_to(&mut self, top: isize) {
        let max_top = self.shown.len().saturating_sub(self.height) as isize;
        self.top = top.clamp(0, max_top.max(0)) as usize;
    }

    pub(crate) fn on(&mut self, key: Key) -> Step {
        if matches!(key, Key::CtrlC | Key::Char('\u{3}')) {
            return Step::Cancel;
        }
        let top = self.top as isize;
        let page = self.height.max(1) as isize;
        if self.editing {
            match key {
                Key::Enter => self.editing = false,
                Key::Escape => {
                    self.editing = false;
                    self.query.clear();
                    self.refilter();
                }
                Key::Backspace => {
                    self.query.pop();
                    self.refilter();
                }
                Key::ArrowUp => self.scroll_to(top - 1),
                Key::ArrowDown => self.scroll_to(top + 1),
                Key::Char(ch) if !ch.is_control() => {
                    self.query.push(ch);
                    self.top = 0;
                    self.refilter();
                }
                _ => {}
            }
            return Step::Active;
        }
        match key {
            Key::Char('q') | Key::Enter => return Step::Done,
            Key::Escape if self.query.is_empty() => return Step::Done,
            Key::Escape => {
                self.query.clear();
                self.refilter();
            }
            Key::Char('/') => self.editing = true,
            Key::ArrowUp | Key::Char('k') => self.scroll_to(top - 1),
            Key::ArrowDown | Key::Char('j') => self.scroll_to(top + 1),
            Key::PageUp => self.scroll_to(top - page),
            Key::PageDown | Key::Char(' ') => self.scroll_to(top + page),
            Key::Home | Key::Char('g') => self.scroll_to(0),
            Key::End | Key::Char('G') => self.scroll_to(isize::MAX),
            Key::Char(d @ '1'..='9') => self.sort_by_column(d as usize - '1' as usize),
            Key::Char('0') => self.set_sort(None),
            Key::Char('s') => {
                let next = self.sort.map_or(0, |(c, _)| (c + 1) % self.columns.max(1));
                self.set_sort(Some((next, false)));
            }
            Key::Char('r') => {
                if let Some((c, d)) = self.sort {
                    self.set_sort(Some((c, !d)));
                }
            }
            _ => {}
        }
        Step::Active
    }

    /// Column widths fitted into `room`: natural widths when they fit, else
    /// the widest columns give way first, down to [`MIN_COL`].
    fn fitted(&self, room: usize) -> Vec<usize> {
        let mut w = self.natural.clone();
        let gaps = GAP * self.columns.saturating_sub(1);
        while w.iter().sum::<usize>() + gaps > room {
            let Some((i, _)) = w
                .iter()
                .enumerate()
                .filter(|&(i, &x)| x > MIN_COL.min(self.natural[i]))
                .max_by_key(|&(i, &x)| (x, std::cmp::Reverse(i)))
            else {
                break;
            };
            w[i] -= 1;
        }
        w
    }

    /// `1–12 of 200`, `37 of 200 match · 1–12`, `no match of 200`, then the
    /// sort. Frozen it is only the window and the sort (`1–12 · size ▲`): the
    /// command's closing line carries the counts.
    fn status(&self, finished: bool) -> String {
        let total = self.keys.len();
        let n = self.shown.len();
        let end = (self.top + self.height).min(n);
        let window = format!("{}–{end}", self.top + 1);
        let mut parts = Vec::new();
        if finished {
            if n > 0 {
                parts.push(window);
            }
        } else if self.query.trim().is_empty() {
            parts.push(format!("{window} of {total}"));
        } else if n == 0 {
            parts.push(format!("no match of {total}"));
        } else {
            parts.push(format!("{n} of {total} match · {window}"));
        }
        if let Some((c, desc)) = self.sort {
            let name = self.grid.headers().get(c).map(|h| h.to_lowercase());
            parts.push(format!("{} {}", name.unwrap_or_default(), arrow(desc)));
        }
        parts.join(" · ")
    }

    /// The frame on a terminal `cols` wide and `rows` high; `finished`
    /// freezes it (grey, no controls). Every line is cut to fit, so the frame
    /// is exactly as many lines as it has `\n`s.
    pub(crate) fn render(&mut self, cols: usize, rows: usize, finished: bool) -> String {
        let lead = self.grid.lead_lines();
        self.height = rows
            .saturating_sub(CHROME + lead.len() + ABOVE)
            .clamp(3, MAX_ROWS)
            .min(self.keys.len())
            .max(1);
        self.scroll_to(self.top as isize);

        // Every line is `inner + 6` columns — `│   ` + content + ` │` — with
        // one column spare so nothing wraps. The box is sized for the longest
        // status it can show, so it keeps its width while the person filters
        // and sorts.
        let max_inner = frame::max_inner(cols);
        let widths = self.fitted(max_inner);
        let table = widths.iter().sum::<usize>() + GAP * widths.len().saturating_sub(1);
        let n = self.keys.len();
        let longest_head = self.grid.headers().iter().map(|h| measure_text_width(h));
        let worst_status = format!(
            "{n} of {n} match · {n}–{n} · {} ▼",
            "x".repeat(longest_head.max().unwrap_or(0))
        );
        let chrome = measure_text_width(self.title) + measure_text_width(&worst_status) + 3;
        let lead_w = lead.iter().map(|l| measure_text_width(&one_line(l)));
        let inner = table
            .max(chrome + CHIP_ROOM)
            .max(lead_w.max().unwrap_or(0))
            .min(max_inner);

        let frame = Frame::new(
            inner,
            if finished {
                State::Submit
            } else {
                State::Active
            },
        );
        let line = |content: &str| frame.line(content);

        // ◆  Items ▸ /git ─── 3 of 200 match · title ▲ ─╮
        let filter = if self.editing && !finished {
            Filter::Typing
        } else {
            Filter::Applied
        };
        let mut out = frame.head(
            self.title,
            self.query.trim(),
            filter,
            &self.status(finished),
        );
        out += &line("");
        for l in lead {
            out += &line(&dim(&cut(&one_line(l), inner)));
        }

        // Column header; the sorted column carries its arrow.
        let mut head = String::new();
        for (c, &w) in widths.iter().enumerate() {
            let mut h = self
                .grid
                .headers()
                .get(c)
                .map(|h| h.to_uppercase())
                .unwrap_or_default();
            let sorted = matches!(self.sort, Some((col, _)) if col == c);
            if let Some((_, desc)) = self.sort.filter(|_| sorted) {
                h = format!("{h} {}", arrow(desc));
            }
            let cell = align(&cut(&h, w), w, self.grid.is_right(c));
            let s = style(cell).bold().for_stderr();
            head += &if sorted { s.cyan() } else { s }.to_string();
            if c + 1 < widths.len() {
                head += &" ".repeat(GAP);
            }
        }
        out += &line(&head);

        // The window; padded to its height so the frame keeps its size while
        // the person filters.
        let end = (self.top + self.height).min(self.shown.len());
        for (r, hits) in &self.shown[self.top..end] {
            out += &line(&self.paint_row(*r, hits, &widths));
        }
        for k in end - self.top..self.height {
            let empty = self.shown.is_empty() && k == 0;
            out += &line(&if empty {
                dim(&format!("no rows match '{}'", self.query))
            } else {
                String::new()
            });
        }

        // The controls are a step of their own below the box, and go once
        // the person is done.
        out += &frame.close();
        if !finished {
            let hints = if self.editing { EDIT_HINTS } else { HINTS };
            out += &frame.controls(hints, cols);
        }
        out
    }

    /// One row in its column widths: each cell in its tone, the filter's
    /// matches underlined in cyan.
    fn paint_row(&self, row: usize, hits: &[u32], widths: &[usize]) -> String {
        let mut text = String::new();
        for (c, &w) in widths.iter().enumerate() {
            let cell = &self.cells[row][c];
            let raw = cut(cell, w);
            let tone = self
                .grid
                .rows()
                .get(row)
                .and_then(|r| r.get(c))
                .map_or(Tone::Default, |g| self.grid.tone_of(c, g))
                .style();
            let painted = paint_hits(&raw, self.starts[row][c], hits, &tone);
            let pad = " ".repeat(w.saturating_sub(measure_text_width(&raw)));
            if self.grid.is_right(c) {
                text += &pad;
                text += &painted;
            } else {
                text += &painted;
                text += &pad;
            }
            if c + 1 < widths.len() {
                text += &" ".repeat(GAP);
            }
        }
        text
    }
}

fn arrow(desc: bool) -> &'static str {
    if desc { "▼" } else { "▲" }
}

/// A cell on one screen line: line breaks become spaces.
fn one_line(text: &str) -> String {
    if text.contains(['\n', '\r']) {
        text.split(['\n', '\r'])
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        text.to_string()
    }
}

/// Draw `browser` on stderr until the person finishes (`Some`, the frame
/// left frozen in place) or cancels (`None`, the frame cleared).
pub(crate) fn run(mut browser: Browser<'_>) -> io::Result<Option<View>> {
    let term = Term::stderr();
    if !term.is_term() {
        return Err(io::ErrorKind::NotConnected.into());
    }
    term.hide_cursor()?;
    let result = crate::prompts::without_sigint(|| {
        let mut drawn = 0;
        loop {
            let (rows, cols) = term.size();
            let frame = browser.render(cols as usize, rows as usize, false);
            term.clear_last_lines(drawn)?;
            term.write_str(&frame)?;
            drawn = frame.lines().count();
            let step = match term.read_key() {
                Ok(key) => browser.on(key),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => Step::Cancel,
                Err(e) => return Err(e),
            };
            match step {
                Step::Active => {}
                Step::Done => {
                    let (rows, cols) = term.size();
                    let frame = browser.render(cols as usize, rows as usize, true);
                    term.clear_last_lines(drawn)?;
                    term.write_str(&frame)?;
                    return Ok(Some(browser.view()));
                }
                Step::Cancel => {
                    term.clear_last_lines(drawn)?;
                    return Ok(None);
                }
            }
        }
    });
    term.show_cursor()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GridCell;

    /// Colour globals are process-wide; hold `ui`'s lock while they are off.
    fn plain() -> std::sync::MutexGuard<'static, ()> {
        let g = crate::ui::tests::lock();
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
        g
    }

    /// name, size (900 B, —, 5 KB, 900 B), owner.
    fn sample() -> Grid {
        let mut g = Grid::new(&["Name", "Size", "Owner"]).right(&[1]);
        for (name, size, owner) in [
            ("beta", Some(900), "ann"),
            ("alpha", None, "bob"),
            ("gamma", Some(5000), "cy"),
            ("delta", Some(900), "anna"),
        ] {
            let size = match size {
                Some(v) => GridCell::from(format!("{v} B")).key(SortKey::Num(v)),
                None => GridCell::from("—"),
            };
            g.row([name.into(), size, owner.into()]);
        }
        g
    }

    fn names(grid: &Grid, view: &View) -> Vec<String> {
        view.rows
            .iter()
            .map(|&r| grid.rows()[r][0].text().to_string())
            .collect()
    }

    fn keys(b: &mut Browser, keys: &[Key]) -> Step {
        let mut last = Step::Active;
        for k in keys {
            last = b.on(k.clone());
        }
        last
    }

    fn typed(text: &str) -> Vec<Key> {
        text.chars().map(Key::Char).collect()
    }

    fn lines(frame: &str) -> Vec<&str> {
        frame.lines().collect()
    }

    #[test]
    fn rows_keep_the_commands_order_until_sorted() {
        let grid = sample();
        let b = Browser::new("T", &grid);
        assert_eq!(names(&grid, &b.view()), ["beta", "alpha", "gamma", "delta"]);
        assert_eq!(b.view().sort, None);
    }

    #[test]
    fn numeric_keys_sort_by_value_with_stable_ties() {
        let grid = sample();
        let mut b = Browser::new("T", &grid);
        keys(&mut b, &[Key::Char('2')]);
        // 900 (beta), 900 (delta) keep their order; — last.
        assert_eq!(names(&grid, &b.view()), ["beta", "delta", "gamma", "alpha"]);
        keys(&mut b, &[Key::Char('2')]);
        assert_eq!(b.view().sort, Some((1, true)));
        assert_eq!(names(&grid, &b.view()), ["gamma", "beta", "delta", "alpha"]);
    }

    #[test]
    fn zero_restores_the_original_order_and_s_cycles_columns() {
        let grid = sample();
        let mut b = Browser::new("T", &grid);
        keys(&mut b, &[Key::Char('1'), Key::Char('0')]);
        assert_eq!(names(&grid, &b.view()), ["beta", "alpha", "gamma", "delta"]);
        assert_eq!(b.view().sort, None);
        keys(&mut b, &[Key::Char('s')]);
        assert_eq!(b.view().sort, Some((0, false)));
        keys(&mut b, &[Key::Char('s'), Key::Char('s'), Key::Char('s')]);
        assert_eq!(b.view().sort, Some((0, false)));
        keys(&mut b, &[Key::Char('r')]);
        assert_eq!(b.view().sort, Some((0, true)));
    }

    #[test]
    fn the_filter_hides_rows_and_keeps_the_sort() {
        let grid = sample();
        let mut b = Browser::new("T", &grid);
        keys(&mut b, &[Key::Char('1'), Key::Char('1'), Key::Char('/')]);
        keys(&mut b, &typed("ann"));
        keys(&mut b, &[Key::Enter]);
        let v = b.view();
        assert_eq!(names(&grid, &v), ["delta", "beta"]);
        assert_eq!(v.query.as_deref(), Some("ann"));
        assert_eq!(v.sort, Some((0, true)));
    }

    #[test]
    fn letters_do_not_filter_outside_filter_mode() {
        let grid = sample();
        let mut b = Browser::new("T", &grid);
        keys(&mut b, &typed("xyz"));
        assert_eq!(b.view().rows.len(), 4);
        assert_eq!(b.view().query, None);
    }

    #[test]
    fn esc_while_typing_clears_the_filter_and_q_is_typed() {
        let grid = sample();
        let mut b = Browser::new("T", &grid);
        keys(&mut b, &[Key::Char('/')]);
        assert_eq!(keys(&mut b, &typed("q")), Step::Active);
        assert_eq!(keys(&mut b, &[Key::Escape]), Step::Active);
        assert_eq!(b.view().rows.len(), 4);
        assert_eq!(b.view().query, None);
    }

    #[test]
    fn esc_clears_an_applied_filter_then_finishes() {
        let grid = sample();
        let mut b = Browser::new("T", &grid);
        keys(&mut b, &[Key::Char('/')]);
        keys(&mut b, &typed("gamma"));
        keys(&mut b, &[Key::Enter]);
        assert_eq!(b.view().rows.len(), 1);
        assert_eq!(keys(&mut b, &[Key::Escape]), Step::Active);
        assert_eq!(b.view().rows.len(), 4);
        assert_eq!(keys(&mut b, &[Key::Escape]), Step::Done);
    }

    #[test]
    fn q_and_enter_finish_and_ctrl_c_cancels() {
        let grid = sample();
        for k in [Key::Char('q'), Key::Enter] {
            assert_eq!(Browser::new("T", &grid).on(k), Step::Done);
        }
        for k in [Key::CtrlC, Key::Char('\u{3}')] {
            assert_eq!(Browser::new("T", &grid).on(k.clone()), Step::Cancel);
            let mut editing = Browser::new("T", &grid);
            editing.on(Key::Char('/'));
            assert_eq!(editing.on(k), Step::Cancel);
        }
    }

    #[test]
    fn the_window_scrolls_and_stops_at_the_last_page() {
        let _c = plain();
        let mut grid = Grid::new(&["Name"]);
        for i in 0..50 {
            grid.row([format!("row{i:02}")]);
        }
        let mut b = Browser::new("T", &grid);
        // 3 rows: a terminal just tall enough for the smallest window.
        let frame = b.render(80, 3, false);
        assert!(frame.contains("1–3 of 50"), "{frame}");
        keys(&mut b, &[Key::ArrowDown, Key::Char('j')]);
        let frame = b.render(80, 3, false);
        assert!(
            frame.contains("3–5 of 50") && frame.contains("row02"),
            "{frame}"
        );
        keys(&mut b, &[Key::PageDown]);
        assert!(b.render(80, 3, false).contains("6–8 of 50"));
        keys(&mut b, &[Key::Char('G'), Key::ArrowDown]);
        let frame = b.render(80, 3, false);
        assert!(
            frame.contains("48–50 of 50") && frame.contains("row49"),
            "{frame}"
        );
        keys(&mut b, &[Key::Char('g'), Key::ArrowUp]);
        assert!(b.render(80, 3, false).contains("1–3 of 50"));
    }

    #[test]
    fn the_frame_shows_title_header_rows_and_controls() {
        let _c = plain();
        let grid = sample();
        let mut b = Browser::new("Items", &grid);
        let frame = b.render(80, 40, false);
        let l = lines(&frame);
        assert!(
            l[0].starts_with("◆  Items ") && l[0].contains("1–4 of 4"),
            "{frame}"
        );
        assert!(l[0].ends_with("─╮"), "{frame}");
        assert!(
            l[2].trim_start_matches("│")
                .trim_start()
                .starts_with("NAME"),
            "{frame}"
        );
        assert!(l[3].contains("beta") && l[3].contains("900 B"), "{frame}");
        assert!(l[7].starts_with("├") && l[7].ends_with("╯"), "{frame}");
        assert_eq!(l[8], "│");
        assert!(
            l[9].starts_with("◇  ↑↓ scroll") && l[9].contains("q done"),
            "{frame}"
        );
        // Every line of the box is the same width.
        let w = measure_text_width(l[0]);
        assert!(l[..8].iter().all(|x| measure_text_width(x) == w), "{frame}");
    }

    #[test]
    fn a_finished_frame_is_frozen_without_controls() {
        let _c = plain();
        let grid = sample();
        let mut b = Browser::new("Items", &grid);
        keys(&mut b, &[Key::Char('/')]);
        keys(&mut b, &typed("ann"));
        keys(&mut b, &[Key::Enter, Key::Char('1')]);
        let frame = b.render(80, 40, true);
        let l = lines(&frame);
        assert!(l[0].starts_with("◇  Items ▸ [/]ann "), "{frame}");
        // The counts are the closing line's; the frozen title keeps the
        // window and the sort.
        assert!(l[0].contains("─ 1–2 · name ▲ ─╮"), "{frame}");
        assert!(!l[0].contains("of 4"), "{frame}");
        assert_eq!(*l.last().unwrap(), "│", "{frame}");
        assert!(!frame.contains("q done"), "{frame}");
    }

    #[test]
    fn typing_shows_the_chip_and_an_empty_match_says_so() {
        let _c = plain();
        let grid = sample();
        let mut b = Browser::new("Items", &grid);
        keys(&mut b, &[Key::Char('/')]);
        keys(&mut b, &typed("zzz"));
        let frame = b.render(80, 40, false);
        assert!(frame.contains("Items ▸ /zzz"), "{frame}");
        assert!(frame.contains("no match of 4"), "{frame}");
        assert!(frame.contains("no rows match 'zzz'"), "{frame}");
        assert!(frame.contains("enter keep"), "{frame}");
        // The frame keeps its height while nothing matches.
        let open = Browser::new("Items", &grid).render(80, 40, false);
        assert_eq!(frame.lines().count(), open.lines().count());
    }

    #[test]
    fn right_aligned_columns_line_up() {
        let _c = plain();
        let grid = sample();
        let frame = Browser::new("T", &grid).render(80, 40, false);
        let l = lines(&frame);
        let end = |s: &str| s.find(" B").unwrap();
        assert_eq!(end(l[3]), end(l[6]), "{frame}");
    }

    #[test]
    fn a_narrow_terminal_cuts_cells_instead_of_wrapping() {
        let _c = plain();
        let mut grid = Grid::new(&["Name", "Note"]);
        grid.row(["x", &"long ".repeat(30)]);
        let frame = Browser::new("T", &grid).render(40, 40, false);
        assert!(frame.contains('…'), "{frame}");
        assert!(frame.lines().all(|l| measure_text_width(l) < 40), "{frame}");
    }

    #[test]
    fn lead_lines_and_multiline_cells_stay_on_one_line_each() {
        let _c = plain();
        let mut grid = Grid::new(&["Name"]);
        grid.lead("First 1 of 9 loaded");
        grid.row(["one\ntwo"]);
        let frame = Browser::new("T", &grid).render(80, 40, false);
        let l = lines(&frame);
        assert!(l[2].contains("First 1 of 9 loaded"), "{frame}");
        assert!(l[4].contains("one two"), "{frame}");
    }
}
