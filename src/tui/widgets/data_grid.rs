//! A long [`Grid`] as a full-screen table the person can scroll, filter and
//! sort, then leave with the view they ended on ([`View`]).
//!
//! ```text
//!  Items ─────────────────────────────── title ▲ · 12/37 of 312 ─
//!  ID        TYPE    TITLE ▲            VAULT     UPDATED
//!  019d2a…   login   github             Personal  2026-10-09 12:07
//!  …
//!  / git                         / filter · 1-9 sort · r reverse · q done
//! ```
//!
//! The widget is pure state: [`ScreenApp::on_key`] changes it and
//! [`ScreenApp::render`] paints it, so both are tested on a [`Buffer`] with
//! no terminal. [`crate::Flow::browse_table`] decides whether to open it and
//! prints the result; the keys and what they mean are `docs/cli.md` §8.1.1.
//!
//! Rows keep the order they came in until the person sorts. Sorting is by
//! each cell's [`SortKey`], stable over that incoming order, with missing
//! values last both ways. The filter only hides rows: a table is read in its
//! sort order, so matches are not re-ranked by relevance the way a picker's
//! are.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::fuzzy::Fuzzy;
use crate::grid::{Grid, SortKey, Tone};
use crate::tui::screen::{Flow, ScreenApp};
use crate::tui::text::width;
use crate::tui::widgets::footer_hints::{self, Hint};
use crate::tui::widgets::status_rule::{Segment, StatusRule};
use crate::tui::widgets::text_input::{self, InputEvent, TextInput};
use crate::tui::{paint, theme};

/// Columns between two table columns.
const GAP: u16 = 2;
/// A column is never squeezed below this (or its natural width, if less).
const MIN_COL: u16 = 6;

/// Where the person left the grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    /// Indices into the grid's rows, in the order shown.
    pub rows: Vec<usize>,
    /// The filter, when one was applied.
    pub query: Option<String>,
    /// `(column, descending)`, when the person sorted.
    pub sort: Option<(usize, bool)>,
}

pub struct DataGrid<'a> {
    title: &'a str,
    grid: &'a Grid,
    columns: usize,
    /// Each row's sort key per column, computed once.
    keys: Vec<Vec<SortKey>>,
    /// Each row's text, for the filter.
    haystacks: Vec<String>,
    /// Widest text per column (header included), for fitting.
    natural: Vec<u16>,
    /// Every row, in the current sort order.
    order: Vec<usize>,
    /// `order` with the filter applied: what is on screen.
    shown: Vec<usize>,
    sort: Option<(usize, bool)>,
    query: String,
    /// The filter being edited, while it is.
    input: Option<TextInput>,
    /// Index into `shown`.
    cursor: usize,
    /// First index of `shown` on screen.
    top: usize,
    /// Data rows that fit on the last frame, for paging.
    page: usize,
    fuzzy: Fuzzy,
}

impl<'a> DataGrid<'a> {
    pub fn new(title: &'a str, grid: &'a Grid) -> Self {
        let rows = grid.rows();
        let columns = rows
            .iter()
            .map(Vec::len)
            .chain(std::iter::once(grid.headers().len()))
            .max()
            .unwrap_or(0);
        let missing = || SortKey::Missing;
        let keys = rows
            .iter()
            .map(|r| {
                (0..columns)
                    .map(|c| r.get(c).map_or_else(missing, |cell| cell.sort_key()))
                    .collect()
            })
            .collect();
        let haystacks = rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| one_line(c.text()))
                    .collect::<Vec<_>>()
                    .join("  ")
            })
            .collect();
        let natural = (0..columns)
            .map(|c| {
                // Room for the sort arrow next to the header.
                let head = grid.headers().get(c).map_or(0, |h| width(h) + 2);
                rows.iter()
                    .filter_map(|r| r.get(c))
                    .map(|cell| width(&one_line(cell.text())))
                    .fold(head, usize::max) as u16
            })
            .collect();
        let order: Vec<usize> = (0..rows.len()).collect();
        Self {
            title,
            grid,
            columns,
            keys,
            haystacks,
            natural,
            shown: order.clone(),
            order,
            sort: None,
            query: String::new(),
            input: None,
            cursor: 0,
            top: 0,
            page: 1,
            fuzzy: Fuzzy::new(),
        }
    }

    /// The view as it stands.
    pub fn view(&self) -> View {
        View {
            rows: self.shown.clone(),
            query: (!self.query.is_empty()).then(|| self.query.clone()),
            sort: self.sort,
        }
    }

    fn selected_row(&self) -> Option<usize> {
        self.shown.get(self.cursor).copied()
    }

    fn set_sort(&mut self, sort: Option<(usize, bool)>) {
        let keep = self.selected_row();
        self.sort = sort;
        self.order = (0..self.keys.len()).collect();
        if let Some((col, desc)) = sort {
            // Stable: ties keep the order the command gave.
            self.order
                .sort_by(|&a, &b| self.keys[a][col].cmp_sorted(&self.keys[b][col], desc));
        }
        self.refilter(keep);
    }

    fn set_query(&mut self, query: String) {
        self.query = query;
        self.refilter(None);
    }

    /// Recompute `shown` from `order` and the query; put the cursor on `keep`
    /// if it is still shown, else on the first row.
    fn refilter(&mut self, keep: Option<usize>) {
        if self.query.trim().is_empty() {
            self.shown = self.order.clone();
        } else {
            let mut hit = vec![false; self.haystacks.len()];
            for m in self.fuzzy.score(&self.query, &self.haystacks) {
                hit[m.index] = true;
            }
            self.shown = self.order.iter().copied().filter(|&i| hit[i]).collect();
        }
        self.cursor = keep
            .and_then(|k| self.shown.iter().position(|&i| i == k))
            .unwrap_or(0);
        self.scroll_to_cursor();
    }

    fn move_by(&mut self, delta: isize) {
        let last = self.shown.len().saturating_sub(1) as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
        self.scroll_to_cursor();
    }

    fn scroll_to_cursor(&mut self) {
        let page = self.page.max(1);
        if self.cursor < self.top {
            self.top = self.cursor;
        } else if self.cursor >= self.top + page {
            self.top = self.cursor + 1 - page;
        }
        let max_top = self.shown.len().saturating_sub(page);
        self.top = self.top.min(max_top);
    }

    /// Sort by `col`; sorting by the column already sorted reverses it.
    fn sort_by_column(&mut self, col: usize) {
        if col >= self.columns {
            return;
        }
        let desc = matches!(self.sort, Some((c, d)) if c == col && !d);
        self.set_sort(Some((col, desc)));
    }

    fn on_filter_key(&mut self, key: KeyEvent) -> Flow<View> {
        let Some(input) = self.input.as_mut() else {
            return Flow::Continue;
        };
        match input.on_key(key) {
            InputEvent::Submit => {
                self.input = None;
            }
            InputEvent::Cancel => {
                self.input = None;
                self.set_query(String::new());
            }
            InputEvent::Edited => {
                let q = input.value().to_string();
                self.set_query(q);
            }
            InputEvent::Ignored => match key.code {
                KeyCode::Up => self.move_by(-1),
                KeyCode::Down => self.move_by(1),
                _ => {}
            },
        }
        Flow::Continue
    }

    /// Column widths fitted to `avail` columns: natural widths when they fit,
    /// else the widest columns give way first, down to [`MIN_COL`].
    fn fitted(&self, avail: u16) -> Vec<u16> {
        let mut w = self.natural.clone();
        let gaps = GAP * self.columns.saturating_sub(1) as u16;
        let total = |w: &[u16]| w.iter().map(|&x| x as u32).sum::<u32>() + gaps as u32;
        while total(&w) > avail as u32 {
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

    fn render_row(
        &self,
        buf: &mut Buffer,
        x: u16,
        y: u16,
        widths: &[u16],
        row: usize,
        selected: bool,
    ) {
        let cells = &self.grid.rows()[row];
        let mut cx = x;
        for (c, &w) in widths.iter().enumerate() {
            if let Some(cell) = cells.get(c) {
                let text = one_line(cell.text());
                let style = tone_style(self.grid.tone_of(c, cell), selected);
                let tw = width(&text) as u16;
                let pad = if self.grid.is_right(c) {
                    w.saturating_sub(tw)
                } else {
                    0
                };
                paint::truncated_str(buf, cx + pad, y, &text, w - pad, style);
            }
            cx = cx.saturating_add(w + GAP);
        }
    }

    fn render_header(&self, buf: &mut Buffer, x: u16, y: u16, widths: &[u16]) {
        let style = Style::default()
            .fg(theme::FG_PRIMARY)
            .add_modifier(Modifier::BOLD);
        let mut cx = x;
        for (c, &w) in widths.iter().enumerate() {
            let mut head = self
                .grid
                .headers()
                .get(c)
                .map(|h| h.to_uppercase())
                .unwrap_or_default();
            if let Some((col, desc)) = self.sort
                && col == c
            {
                head.push_str(if desc { " ▼" } else { " ▲" });
            }
            let tw = width(&head) as u16;
            let pad = if self.grid.is_right(c) {
                w.saturating_sub(tw)
            } else {
                0
            };
            paint::truncated_str(buf, cx + pad, y, &head, w - pad, style);
            cx = cx.saturating_add(w + GAP);
        }
    }

    fn render_footer(&self, buf: &mut Buffer, area: Rect, y: u16) {
        let filter_w = if self.input.is_some() || !self.query.is_empty() {
            (area.width / 2).max(12).min(area.width)
        } else {
            0
        };
        if let Some(input) = &self.input {
            buf.set_string(area.x + 1, y, "/", Style::default().fg(theme::SEARCH));
            let field = Rect::new(area.x + 3, y, filter_w.saturating_sub(4), 1);
            text_input::render(field, buf, input, Style::default().fg(theme::SEARCH));
        } else if !self.query.is_empty() {
            let text = format!("/ {}", self.query);
            paint::truncated_str(
                buf,
                area.x + 1,
                y,
                &text,
                filter_w.saturating_sub(2),
                Style::default().fg(theme::SEARCH),
            );
        }
        let hints: &[Hint] = if self.input.is_some() {
            &[
                Hint::action("enter", "keep"),
                Hint::action("esc", "clear"),
                Hint::action("^c", "cancel"),
            ]
        } else if !self.query.is_empty() {
            &[
                Hint::action("/", "filter"),
                Hint::action("1-9", "sort"),
                Hint::action("r", "reverse"),
                Hint::action("esc", "clear"),
                Hint::action("q", "done"),
            ]
        } else {
            &[
                Hint::action("/", "filter"),
                Hint::action("1-9", "sort"),
                Hint::action("r", "reverse"),
                Hint::action("q", "done"),
            ]
        };
        footer_hints::render_right(
            buf,
            area.x + filter_w,
            y,
            area.width.saturating_sub(filter_w),
            hints,
        );
    }
}

impl ScreenApp for DataGrid<'_> {
    type Output = View;

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if area.width < 4 || area.height < 4 {
            return;
        }
        let inner_x = area.x + 1;
        let inner_w = area.width - 2;
        let bottom = area.y + area.height - 1;

        // Title rule with the sort and the position.
        let sort = self.sort.map(|(c, desc)| {
            let name = self.grid.headers().get(c).cloned().unwrap_or_default();
            format!("{} {}", name.to_lowercase(), if desc { "▼" } else { "▲" })
        });
        let total = self.keys.len();
        let at = if self.shown.is_empty() {
            0
        } else {
            self.cursor + 1
        };
        let pos = if self.shown.len() == total {
            format!("{at}/{total}")
        } else {
            format!("{at}/{} of {total}", self.shown.len())
        };
        let title = format!(" {} ", self.title);
        let mut rule = StatusRule::new().left(Segment::styled(
            &title,
            Style::default()
                .fg(theme::FG_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ));
        if let Some(s) = &sort {
            rule = rule.push_right(Segment::styled(s, Style::default().fg(theme::ACCENT)));
        }
        rule = rule.push_right(Segment::dim(&pos));
        rule.render(buf, area.x, area.y, area.width);

        let mut y = area.y + 1;
        for line in self.grid.lead_lines() {
            if y >= bottom {
                break;
            }
            paint::truncated_str(buf, inner_x, y, &one_line(line), inner_w, theme::dim());
            y += 1;
        }

        let widths = self.fitted(inner_w);
        if y < bottom {
            self.render_header(buf, inner_x, y, &widths);
            y += 1;
        }

        self.page = bottom.saturating_sub(y).max(1) as usize;
        self.scroll_to_cursor();
        if self.shown.is_empty() && y < bottom {
            let msg = if self.query.is_empty() {
                "no rows".to_string()
            } else {
                format!("no rows match '{}'", self.query)
            };
            paint::centered_str(buf, area.x, y + 1, area.width, &msg, theme::dim());
        }
        for (i, &row) in self.shown.iter().enumerate().skip(self.top).take(self.page) {
            let selected = i == self.cursor;
            if selected {
                paint::fill_bg(buf, Rect::new(area.x, y, area.width, 1), theme::BG_SELECTED);
            }
            self.render_row(buf, inner_x, y, &widths, row, selected);
            y += 1;
        }

        self.render_footer(buf, area, bottom);
    }

    fn on_key(&mut self, key: KeyEvent) -> Flow<View> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Flow::Cancel;
        }
        if self.input.is_some() {
            return self.on_filter_key(key);
        }
        let page = self.page.max(1) as isize;
        match key.code {
            KeyCode::Char('q') | KeyCode::Enter => return Flow::Exit(self.view()),
            KeyCode::Esc if self.query.is_empty() => return Flow::Exit(self.view()),
            KeyCode::Esc => self.set_query(String::new()),
            KeyCode::Char('/') => self.input = Some(TextInput::new(self.query.clone())),
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-page),
            KeyCode::PageDown | KeyCode::Char(' ') => self.move_by(page),
            KeyCode::Home | KeyCode::Char('g') => self.move_by(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.move_by(isize::MAX / 2),
            KeyCode::Char(d @ '1'..='9') => self.sort_by_column(d as usize - '1' as usize),
            KeyCode::Char('0') => self.set_sort(None),
            KeyCode::Char('s') => {
                let next = self.sort.map_or(0, |(c, _)| (c + 1) % self.columns.max(1));
                self.set_sort(Some((next, false)));
            }
            KeyCode::Char('r') => {
                if let Some((c, d)) = self.sort {
                    self.set_sort(Some((c, !d)));
                }
            }
            _ => {}
        }
        Flow::Continue
    }
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

/// A [`Tone`] as a cell style; on the cursor row, muted text steps down from
/// the row's own foreground so it stays readable on [`theme::BG_SELECTED`].
fn tone_style(tone: Tone, selected: bool) -> Style {
    let fg = match tone {
        Tone::Default | Tone::Strong => theme::FG_PRIMARY,
        Tone::Muted if selected => theme::FG_DIM_SELECTED,
        Tone::Muted => theme::FG_DIM,
        Tone::Accent => Color::Cyan,
        Tone::Success => Color::Green,
        Tone::Warn => Color::Yellow,
        Tone::Danger => Color::Red,
    };
    let s = Style::default().fg(fg);
    let s = if tone == Tone::Strong {
        s.add_modifier(Modifier::BOLD)
    } else {
        s
    };
    if selected {
        s.bg(theme::BG_SELECTED)
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GridCell;
    use ratatui::layout::Position;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ch(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }

    fn press(g: &mut DataGrid, keys: &[KeyEvent]) -> Option<Flow<View>> {
        let mut last = None;
        for k in keys {
            last = Some(g.on_key(*k));
        }
        last
    }

    /// `name`, `size` (numeric key), `owner` (sometimes missing).
    fn sample() -> Grid {
        let mut g = Grid::new(&["Name", "Size", "Owner"]).right(&[1]);
        for (name, size, owner) in [
            ("beta", 900, "ann"),
            ("alpha", 10_000, "—"),
            ("gamma", 900, "bob"),
            ("delta", 50, "ann"),
        ] {
            g.row([
                GridCell::from(name),
                GridCell::from(format!("{size} B")).key(SortKey::Num(size)),
                GridCell::from(owner),
            ]);
        }
        g
    }

    fn names(g: &Grid, v: &View) -> Vec<String> {
        v.rows
            .iter()
            .map(|&i| g.rows()[i][0].text().to_string())
            .collect()
    }

    fn screen(g: &mut DataGrid, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        g.render(area, &mut buf);
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf.cell(Position::new(x, y)).unwrap().symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn rows_keep_the_commands_order_until_sorted() {
        let grid = sample();
        let g = DataGrid::new("T", &grid);
        assert_eq!(names(&grid, &g.view()), ["beta", "alpha", "gamma", "delta"]);
        assert_eq!(g.view().sort, None);
    }

    #[test]
    fn numeric_keys_sort_by_value_with_stable_ties() {
        let grid = sample();
        let mut g = DataGrid::new("T", &grid);
        press(&mut g, &[ch('2')]);
        // 900 vs 10 000: by value, not by text; beta before gamma as given.
        assert_eq!(names(&grid, &g.view()), ["delta", "beta", "gamma", "alpha"]);
        press(&mut g, &[ch('2')]);
        assert_eq!(g.view().sort, Some((1, true)));
        assert_eq!(names(&grid, &g.view()), ["alpha", "beta", "gamma", "delta"]);
    }

    #[test]
    fn missing_values_sort_last_both_ways() {
        let grid = sample();
        let mut g = DataGrid::new("T", &grid);
        press(&mut g, &[ch('3')]);
        assert_eq!(names(&grid, &g.view()).last().unwrap(), "alpha");
        press(&mut g, &[ch('r')]);
        assert_eq!(names(&grid, &g.view()).last().unwrap(), "alpha");
    }

    #[test]
    fn zero_restores_the_original_order() {
        let grid = sample();
        let mut g = DataGrid::new("T", &grid);
        press(&mut g, &[ch('1'), ch('0')]);
        assert_eq!(names(&grid, &g.view()), ["beta", "alpha", "gamma", "delta"]);
        assert_eq!(g.view().sort, None);
    }

    #[test]
    fn s_cycles_columns() {
        let grid = sample();
        let mut g = DataGrid::new("T", &grid);
        press(&mut g, &[ch('s')]);
        assert_eq!(g.view().sort, Some((0, false)));
        press(&mut g, &[ch('s'), ch('s'), ch('s')]);
        assert_eq!(g.view().sort, Some((0, false)));
    }

    #[test]
    fn the_filter_hides_rows_and_keeps_the_sort() {
        let grid = sample();
        let mut g = DataGrid::new("T", &grid);
        press(
            &mut g,
            &[ch('1'), ch('r'), ch('/'), ch('a'), ch('n'), ch('n')],
        );
        // Live while typing; rows owned by ann, still by name descending.
        assert_eq!(names(&grid, &g.view()), ["delta", "beta"]);
        press(&mut g, &[key(KeyCode::Enter)]);
        let v = g.view();
        assert_eq!(v.query.as_deref(), Some("ann"));
        assert_eq!(v.sort, Some((0, true)));
    }

    #[test]
    fn letters_do_not_filter_outside_filter_mode() {
        let grid = sample();
        let mut g = DataGrid::new("T", &grid);
        press(&mut g, &[ch('x'), ch('z')]);
        assert_eq!(g.view().rows.len(), 4);
        assert_eq!(g.view().query, None);
    }

    #[test]
    fn esc_while_editing_clears_the_filter() {
        let grid = sample();
        let mut g = DataGrid::new("T", &grid);
        let f = press(&mut g, &[ch('/'), ch('b'), ch('o'), key(KeyCode::Esc)]);
        assert!(matches!(f, Some(Flow::Continue)));
        assert_eq!(g.view().rows.len(), 4);
        assert_eq!(g.view().query, None);
    }

    #[test]
    fn esc_clears_an_applied_filter_then_finishes() {
        let grid = sample();
        let mut g = DataGrid::new("T", &grid);
        press(
            &mut g,
            &[ch('/'), ch('b'), ch('o'), ch('b'), key(KeyCode::Enter)],
        );
        assert_eq!(g.view().rows.len(), 1);
        assert!(matches!(
            press(&mut g, &[key(KeyCode::Esc)]),
            Some(Flow::Continue)
        ));
        assert_eq!(g.view().rows.len(), 4);
        assert!(matches!(
            press(&mut g, &[key(KeyCode::Esc)]),
            Some(Flow::Exit(_))
        ));
    }

    #[test]
    fn q_and_enter_finish_with_the_view_and_ctrl_c_cancels() {
        let grid = sample();
        for k in [ch('q'), key(KeyCode::Enter)] {
            let mut g = DataGrid::new("T", &grid);
            match g.on_key(k) {
                Flow::Exit(v) => assert_eq!(v.rows.len(), 4),
                _ => panic!("expected exit"),
            }
        }
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        for pre in [vec![], vec![ch('/')]] {
            let mut g = DataGrid::new("T", &grid);
            press(&mut g, &pre);
            assert!(matches!(g.on_key(ctrl_c), Flow::Cancel));
        }
    }

    #[test]
    fn q_while_editing_is_typed() {
        let grid = sample();
        let mut g = DataGrid::new("T", &grid);
        let f = press(&mut g, &[ch('/'), ch('q')]);
        assert!(matches!(f, Some(Flow::Continue)));
    }

    #[test]
    fn the_cursor_follows_its_row_across_a_sort_and_stays_in_bounds() {
        let grid = sample();
        let mut g = DataGrid::new("T", &grid);
        press(&mut g, &[ch('j')]); // alpha
        press(&mut g, &[ch('1')]);
        assert_eq!(g.selected_row(), Some(1));
        assert_eq!(g.cursor, 0);
        press(&mut g, &[ch('G'), ch('j'), ch('j')]);
        assert_eq!(g.cursor, 3);
        press(&mut g, &[ch('g'), ch('k')]);
        assert_eq!(g.cursor, 0);
    }

    #[test]
    fn renders_header_rows_and_position() {
        let grid = sample();
        let mut g = DataGrid::new("Items", &grid);
        let s = screen(&mut g, 60, 8);
        assert!(s[0].contains("Items") && s[0].contains("1/4"), "{s:#?}");
        assert!(s[1].trim_start().starts_with("NAME"), "{s:#?}");
        assert!(s[2].contains("beta") && s[2].contains("900 B"), "{s:#?}");
        assert!(s[7].contains("q done"), "{s:#?}");
    }

    #[test]
    fn right_aligned_columns_line_up() {
        let grid = sample();
        let mut g = DataGrid::new("Items", &grid);
        let s = screen(&mut g, 60, 8);
        let end = |l: &str| l.find(" B").unwrap();
        assert_eq!(end(&s[2]), end(&s[3]), "{s:#?}");
    }

    #[test]
    fn a_narrow_screen_truncates_instead_of_overflowing() {
        let mut grid = Grid::new(&["Title", "Note"]);
        grid.row(["a fairly long title here", "and an even longer note column"]);
        let mut g = DataGrid::new("T", &grid);
        let s = screen(&mut g, 30, 6);
        assert!(s[2].contains('…'), "{s:#?}");
        assert!(s.iter().all(|l| width(l) <= 30));
    }

    #[test]
    fn scrolling_keeps_the_cursor_on_screen() {
        let mut grid = Grid::new(&["N"]);
        for i in 0..50 {
            grid.row([format!("row{i:02}")]);
        }
        let mut g = DataGrid::new("T", &grid);
        screen(&mut g, 30, 10); // 7 data rows
        press(&mut g, &[key(KeyCode::PageDown), key(KeyCode::PageDown)]);
        let s = screen(&mut g, 30, 10);
        assert!(s.iter().any(|l| l.contains("row14")), "{s:#?}");
        press(&mut g, &[ch('G')]);
        let s = screen(&mut g, 30, 10);
        assert!(s[8].contains("row49"), "{s:#?}");
        assert!(s[0].contains("50/50"), "{s:#?}");
    }

    #[test]
    fn filtering_shows_the_count_of_the_total_and_an_empty_message() {
        let grid = sample();
        let mut g = DataGrid::new("T", &grid);
        press(
            &mut g,
            &[ch('/'), ch('a'), ch('n'), ch('n'), key(KeyCode::Enter)],
        );
        let s = screen(&mut g, 60, 8);
        assert!(s[0].contains("1/2 of 4"), "{s:#?}");
        assert!(s[7].contains("/ ann"), "{s:#?}");
        press(&mut g, &[ch('/'), ch('z'), ch('z'), ch('z')]);
        let s = screen(&mut g, 60, 8);
        assert!(s.iter().any(|l| l.contains("no rows match")), "{s:#?}");
    }

    #[test]
    fn multiline_cells_render_on_one_line() {
        let mut grid = Grid::new(&["Note"]);
        grid.row(["one\ntwo"]);
        let mut g = DataGrid::new("T", &grid);
        let s = screen(&mut g, 30, 6);
        assert!(s[2].contains("one two"), "{s:#?}");
    }
}
