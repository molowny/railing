//! Tables for command output: [`Grid`] with semantic [`Tone`]s.
//!
//! A grid is drawn two ways: *boxed*, a borderless table in its own titled
//! box inside a live flow, and *plain*, the bordered table on stdout. Tones
//! are applied at render time, so each way colours only when its stream does.
//!
//! The box is drawn here, not with `cliclack::note`: cliclack dims a note's
//! whole body, which suits prose and flattens a table to one gray.

use comfy_table::presets::{NOTHING, UTF8_BORDERS_ONLY};
use comfy_table::{Attribute, Cell, CellAlignment, Color, ContentArrangement, Table};
use console::{Emoji, Style, style};

/// What a piece of table text means; the palette maps it to a style.
///
/// The colours are the 16 ANSI ones, so the terminal's theme picks their
/// shades for light and dark backgrounds alike. Muted is the exception: it is
/// [`crate::ui::DIM_256`], because SGR 2 "faint" is unreadable on some
/// terminals and indistinguishable from normal text on others.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    /// Plain text.
    #[default]
    Default,
    /// Bold — what the row is about (a title, an invoice number).
    Strong,
    /// Dim — supporting detail (ids, timestamps, units).
    Muted,
    /// Cyan — kinds, types, tags.
    Accent,
    /// Green — a good state.
    Success,
    /// Yellow — a state that needs attention.
    Warn,
    /// Red — a failed or dangerous state.
    Danger,
}

impl Tone {
    /// A cell with this tone, for [`Grid::row`].
    pub fn cell(self, text: impl Into<String>) -> GridCell {
        GridCell {
            text: text.into(),
            tone: Some(self),
            key: None,
        }
    }

    /// This tone as a `console` style, for text outside a grid.
    pub fn style(self) -> Style {
        let s = Style::new();
        match self {
            Tone::Default => s,
            Tone::Strong => s.bold(),
            Tone::Muted => s.color256(crate::ui::DIM_256),
            Tone::Accent => s.cyan(),
            Tone::Success => s.green(),
            Tone::Warn => s.yellow(),
            Tone::Danger => s.red(),
        }
    }

    /// This tone on a comfy-table cell, for a table built by hand
    /// ([`crate::make_table`]). `Dark*` are comfy-table's names for the base
    /// ANSI colours, the ones `console` uses; its plain names are the bright
    /// variants.
    pub fn apply(self, cell: Cell) -> Cell {
        match self {
            Tone::Default => cell,
            Tone::Strong => cell.add_attribute(Attribute::Bold),
            Tone::Muted => cell.fg(Color::AnsiValue(crate::ui::DIM_256)),
            Tone::Accent => cell.fg(Color::DarkCyan),
            Tone::Success => cell.fg(Color::DarkGreen),
            Tone::Warn => cell.fg(Color::DarkYellow),
            Tone::Danger => cell.fg(Color::DarkRed),
        }
    }
}

/// What a cell sorts by when a person sorts its column in a browsed grid.
///
/// Set it with [`GridCell::key`] wherever the text does not sort the way the
/// value does: numbers, sizes, money (in minor units) and timestamps. A cell
/// without one sorts by its text, case-insensitively; alignment never implies
/// a numeric sort.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SortKey {
    Num(i64),
    Text(String),
    /// No value (`—`). Sorts last whichever way the column is sorted.
    Missing,
}

impl SortKey {
    /// Order two present keys. Numbers come before text, so a column that
    /// mixes them (it should not) still sorts the same way every time.
    #[cfg_attr(not(feature = "tui"), allow(dead_code))]
    fn cmp_present(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        match (self, other) {
            (SortKey::Num(a), SortKey::Num(b)) => a.cmp(b),
            (SortKey::Text(a), SortKey::Text(b)) => a.cmp(b),
            (SortKey::Num(_), SortKey::Text(_)) => Less,
            (SortKey::Text(_), SortKey::Num(_)) => Greater,
            _ => Equal,
        }
    }

    /// Order for a column sorted ascending (`desc` false) or descending.
    /// Missing keys are last either way.
    #[cfg_attr(not(feature = "tui"), allow(dead_code))]
    pub(crate) fn cmp_sorted(&self, other: &Self, desc: bool) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        match (self, other) {
            (SortKey::Missing, SortKey::Missing) => Equal,
            (SortKey::Missing, _) => Greater,
            (_, SortKey::Missing) => Less,
            _ if desc => other.cmp_present(self),
            _ => self.cmp_present(other),
        }
    }
}

/// One grid cell: its text and, when it differs from its column's, a tone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GridCell {
    text: String,
    tone: Option<Tone>,
    key: Option<SortKey>,
}

impl GridCell {
    /// Sort this cell by `key` instead of its text ([`SortKey`]).
    pub fn key(mut self, key: SortKey) -> Self {
        self.key = Some(key);
        self
    }

    #[cfg_attr(not(feature = "tui"), allow(dead_code))]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The cell's sort key: its own, or its text (empty and `—` are missing).
    #[cfg_attr(not(feature = "tui"), allow(dead_code))]
    pub(crate) fn sort_key(&self) -> SortKey {
        if let Some(k) = &self.key {
            return k.clone();
        }
        match self.text.trim() {
            "" | "—" => SortKey::Missing,
            t => SortKey::Text(t.to_lowercase()),
        }
    }
}

impl From<&str> for GridCell {
    fn from(text: &str) -> Self {
        text.to_string().into()
    }
}

impl From<String> for GridCell {
    fn from(text: String) -> Self {
        Self {
            text,
            tone: None,
            key: None,
        }
    }
}

impl From<&String> for GridCell {
    fn from(text: &String) -> Self {
        text.as_str().into()
    }
}

/// A table with optional lead lines.
#[derive(Clone, Debug, Default)]
pub struct Grid {
    lead: Vec<String>,
    headers: Vec<String>,
    header_tone: Tone,
    tones: Vec<Tone>,
    right: Vec<usize>,
    rows: Vec<Vec<GridCell>>,
    no_header: bool,
    /// `(column, width)`: cut to `width` with `…` when drawn as a table.
    clip: Vec<(usize, usize)>,
}

impl Grid {
    /// A grid with a bold header row.
    pub fn new(headers: &[&str]) -> Self {
        Self {
            headers: headers.iter().map(|h| h.to_string()).collect(),
            header_tone: Tone::Strong,
            ..Self::default()
        }
    }

    /// A two-column key/value grid with no header row: muted keys, plain
    /// values.
    pub fn kv() -> Self {
        Self {
            headers: vec!["Property".into(), "Value".into()],
            tones: vec![Tone::Muted, Tone::Default],
            no_header: true,
            ..Self::default()
        }
    }

    /// Hide the header row (`--no-header`).
    pub fn no_header(mut self, yes: bool) -> Self {
        self.no_header |= yes;
        self
    }

    /// The header row's tone (bold unless set).
    pub fn header_tone(mut self, tone: Tone) -> Self {
        self.header_tone = tone;
        self
    }

    /// Each column's tone, in order; columns past the end are plain.
    pub fn tones(mut self, tones: impl IntoIterator<Item = Tone>) -> Self {
        self.tones = tones.into_iter().collect();
        self
    }

    /// Right-align these columns (amounts, counts, sizes).
    pub fn right(mut self, columns: &[usize]) -> Self {
        self.right = columns.to_vec();
        self
    }

    /// Cut `column` to `width` columns with `…` in the printed table. The cell
    /// keeps its whole text, so a browsed grid filters on all of it and fits
    /// the column to the screen itself.
    pub fn clip(mut self, column: usize, width: usize) -> Self {
        self.clip.retain(|&(c, _)| c != column);
        self.clip.push((column, width));
        self
    }

    /// `text` as drawn in `column` of the printed table.
    fn drawn<'t>(&self, column: usize, text: &'t str) -> std::borrow::Cow<'t, str> {
        match self.clip.iter().find(|&&(c, _)| c == column) {
            Some(&(_, width)) => console::truncate_str(text, width, "…"),
            None => text.into(),
        }
    }

    /// A line above the table, inside the box (`Branch: master`).
    pub fn lead(&mut self, line: impl Into<String>) -> &mut Self {
        self.lead.push(line.into());
        self
    }

    /// A row of cells; a `&str`/`String` takes its column's tone,
    /// [`Tone::cell`] its own.
    pub fn row<I, S>(&mut self, cells: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<GridCell>,
    {
        self.rows.push(cells.into_iter().map(Into::into).collect());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// The same grid with only `rows`, in that order — the view a browsed
    /// grid ended on.
    pub fn pick(&self, rows: &[usize]) -> Grid {
        Grid {
            rows: rows
                .iter()
                .filter_map(|&i| self.rows.get(i).cloned())
                .collect(),
            ..self.clone()
        }
    }

    #[cfg_attr(not(feature = "tui"), allow(dead_code))]
    pub(crate) fn headers(&self) -> &[String] {
        &self.headers
    }

    #[cfg_attr(not(feature = "tui"), allow(dead_code))]
    pub(crate) fn lead_lines(&self) -> &[String] {
        &self.lead
    }

    #[cfg_attr(not(feature = "tui"), allow(dead_code))]
    pub(crate) fn rows(&self) -> &[Vec<GridCell>] {
        &self.rows
    }

    #[cfg_attr(not(feature = "tui"), allow(dead_code))]
    pub(crate) fn is_right(&self, column: usize) -> bool {
        self.right.contains(&column)
    }

    /// The tone `cell` in `column` is drawn with.
    pub(crate) fn tone_of(&self, column: usize, cell: &GridCell) -> Tone {
        cell.tone
            .unwrap_or_else(|| self.tones.get(column).copied().unwrap_or_default())
    }

    /// The comfy table with tones applied, or none when `color` is off.
    fn table(&self, headers: Vec<String>, color: bool) -> Table {
        let mut t = Table::new();
        if color {
            t.enforce_styling();
        } else {
            t.force_no_tty();
        }
        if !self.no_header {
            t.set_header(
                headers
                    .into_iter()
                    .map(|h| self.header_tone.apply(Cell::new(h))),
            );
        }
        for r in &self.rows {
            t.add_row(
                r.iter()
                    .enumerate()
                    .map(|(i, c)| self.tone_of(i, c).apply(Cell::new(self.drawn(i, &c.text)))),
            );
        }
        for &i in &self.right {
            if let Some(col) = t.column_mut(i) {
                col.set_cell_alignment(CellAlignment::Right);
            }
        }
        t
    }

    /// Borderless, fitted to `width` columns — the box around it is the
    /// border.
    pub(crate) fn boxed(&self, width: usize) -> String {
        let headers = self.headers.iter().map(|h| h.to_uppercase()).collect();
        let mut t = self.table(headers, console::colors_enabled_stderr());
        t.load_preset(NOTHING)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_width(width as u16);
        // NOTHING pads every cell by one; the box already has its margin.
        if let Some(first) = t.column_mut(0) {
            first.set_padding((0, 1));
        }
        let mut out: Vec<String> = self.lead.clone();
        out.extend(t.to_string().lines().map(|l| l.trim_end().to_string()));
        out.join("\n")
    }

    /// The bordered table for plain output, coloured only when stdout is.
    pub fn plain(&self) -> String {
        let mut t = self.table(self.headers.clone(), console::colors_enabled());
        t.load_preset(UTF8_BORDERS_ONLY)
            .set_content_arrangement(ContentArrangement::Dynamic);
        let mut out = self.lead.clone();
        out.push(t.to_string());
        out.join("\n")
    }
}

/// The default cliclack theme, for its bar colour and step symbol — so the
/// table box matches the notes and log lines around it.
pub(crate) struct Clack;
impl cliclack::Theme for Clack {}

const S_BAR: Emoji = Emoji("│", "|");
const S_BAR_H: Emoji = Emoji("─", "-");
const S_CORNER_TOP_RIGHT: Emoji = Emoji("╮", "+");
const S_CONNECT_LEFT: Emoji = Emoji("├", "+");
const S_CORNER_BOTTOM_RIGHT: Emoji = Emoji("╯", "+");

/// The bare `│`, coloured like the box frames.
pub(crate) fn gutter_bar() -> String {
    use cliclack::{Theme, ThemeState};
    Clack
        .bar_color(&ThemeState::Submit)
        .for_stderr()
        .apply_to(S_BAR)
        .to_string()
}

/// `◇  title ───╮`, the body undimmed between `│` bars, `├───╯`: cliclack's
/// note frame, with the body's own styling left alone.
pub(crate) fn boxed_note(title: &str, body: &str) -> String {
    use cliclack::{Theme, ThemeState};
    let width = |s: &str| console::measure_text_width(s);
    let title = style(title).bold().to_string();
    let lines: Vec<&str> = std::iter::once("")
        .chain(body.lines())
        .chain(std::iter::once(""))
        .collect();
    let inner = 2 + lines
        .iter()
        .map(|l| width(l))
        .max()
        .unwrap_or(0)
        .max(width(&title));
    let bar = Clack.bar_color(&ThemeState::Submit).for_stderr();
    let mut out = format!(
        "{symbol}  {title} {rule}{corner}\n",
        symbol = Clack.state_symbol(&ThemeState::Submit),
        rule = bar.apply_to(S_BAR_H.to_string().repeat(inner - width(&title))),
        corner = bar.apply_to(S_CORNER_TOP_RIGHT),
    );
    for l in lines {
        out.push_str(&format!(
            "{b}  {l}{pad}{b}\n",
            b = bar.apply_to(S_BAR),
            pad = " ".repeat(inner - width(l) + 1),
        ));
    }
    out.push_str(&format!(
        "{}\n{}\n",
        bar.apply_to(format!(
            "{S_CONNECT_LEFT}{}{S_CORNER_BOTTOM_RIGHT}",
            S_BAR_H.to_string().repeat(inner + 3)
        )),
        bar.apply_to(S_BAR),
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Colour globals are process-wide; hold `ui`'s lock while set.
    fn colors(on: bool) -> std::sync::MutexGuard<'static, ()> {
        let g = crate::ui::tests::lock();
        console::set_colors_enabled(on);
        console::set_colors_enabled_stderr(on);
        g
    }

    #[test]
    fn boxed_grid_is_borderless_and_unindented() {
        let _c = colors(false);
        let mut g = Grid::new(&["Id", "Title"]);
        g.row(["a1", "github"]);
        let s = g.boxed(80);
        assert!(!s.contains('│') && !s.contains('─'), "{s}");
        assert!(s.lines().next().unwrap().starts_with("ID"), "{s}");
    }

    #[test]
    fn kv_has_no_header_and_lead_comes_first() {
        let _c = colors(false);
        let mut g = Grid::kv();
        g.lead("Branch: master").row(["Agent", "running"]);
        let s = g.boxed(80);
        let lines: Vec<_> = s.lines().collect();
        assert_eq!(lines[0], "Branch: master");
        assert!(lines[1].starts_with("Agent"), "{s}");
        assert!(!s.contains("PROPERTY"));
    }

    #[test]
    fn plain_grid_keeps_the_bordered_table() {
        let _c = colors(false);
        let mut g = Grid::new(&["Id"]);
        g.row(["a1"]);
        assert!(g.plain().contains('│'));
    }

    #[test]
    fn no_color_means_no_ansi_at_all() {
        let _c = colors(false);
        let mut g = Grid::new(&["Id", "State"]).tones([Tone::Muted]).right(&[1]);
        g.row(["a1".into(), Tone::Danger.cell("failed")]);
        for s in [g.boxed(80), g.plain(), boxed_note("T", &g.boxed(80))] {
            assert!(!s.contains('\x1b'), "{s:?}");
        }
    }

    #[test]
    fn tones_colour_columns_and_cells() {
        let _c = colors(true);
        let mut g = Grid::new(&["Id", "State"]).tones([Tone::Muted]);
        g.row(["a1".into(), Tone::Danger.cell("failed")]);
        let s = g.boxed(80);
        assert!(s.contains("\x1b[38;5;246ma1"), "{s:?}");
        assert!(s.contains("\x1b[38;5;1m failed"), "base red: {s:?}");
        assert!(s.contains("\x1b[1mID"), "header bold: {s:?}");
        let p = g.plain();
        assert!(p.contains("\x1b[38;5;246m a1"), "{p:?}");
    }

    #[test]
    fn header_tone_is_the_callers() {
        let _c = colors(true);
        let mut g = Grid::new(&["Id"]).header_tone(Tone::Muted);
        g.row(["a1"]);
        assert!(g.boxed(80).contains("\x1b[38;5;246mID"));
    }

    #[test]
    fn right_aligns_columns() {
        let _c = colors(false);
        let mut g = Grid::new(&["N", "Amount"]).right(&[1]);
        g.row(["a", "1.00"]).row(["b", "1000.00"]);
        let s = g.boxed(80);
        let ends: Vec<_> = s.lines().skip(1).map(|l| l.len()).collect();
        assert_eq!(ends[0], ends[1], "{s}");
    }

    #[test]
    fn missing_sorts_last_both_ways_and_numbers_by_value() {
        use std::cmp::Ordering::*;
        let n = |v| SortKey::Num(v);
        assert_eq!(n(9).cmp_sorted(&n(10), false), Less);
        assert_eq!(n(9).cmp_sorted(&n(10), true), Greater);
        for desc in [false, true] {
            assert_eq!(SortKey::Missing.cmp_sorted(&n(1), desc), Greater);
            assert_eq!(n(1).cmp_sorted(&SortKey::Missing, desc), Less);
        }
    }

    #[test]
    fn a_cell_sorts_by_its_key_else_its_text() {
        assert_eq!(GridCell::from("—").sort_key(), SortKey::Missing);
        assert_eq!(GridCell::from("  ").sort_key(), SortKey::Missing);
        assert_eq!(
            GridCell::from("GitHub").sort_key(),
            SortKey::Text("github".into())
        );
        let c = Tone::Muted.cell("1.2 GB").key(SortKey::Num(1_200));
        assert_eq!(c.sort_key(), SortKey::Num(1_200));
    }

    #[test]
    fn clip_cuts_the_drawn_table_and_keeps_the_text() {
        let _c = colors(false);
        let mut g = Grid::new(&["Name"]).no_header(true).clip(0, 8);
        g.row(["a rather long name"]);
        assert_eq!(g.boxed(80).trim(), "a rathe…");
        assert!(g.plain().contains("a rathe…"));
        assert_eq!(g.rows()[0][0].text(), "a rather long name");
        // A clip survives `pick`, so the view printed after browsing matches.
        assert_eq!(g.pick(&[0]).boxed(80).trim(), "a rathe…");
    }

    #[test]
    fn pick_keeps_the_shape_and_takes_rows_in_order() {
        let _c = colors(false);
        let mut g = Grid::new(&["Id"]).no_header(true);
        g.row(["a"]).row(["b"]).row(["c"]);
        let p = g.pick(&[2, 0, 9]);
        assert_eq!(p.len(), 2);
        let s = p.boxed(80);
        let lines: Vec<_> = s.lines().map(str::trim).collect();
        assert_eq!(lines, ["c", "a"]);
    }

    #[test]
    fn box_body_is_not_dimmed_and_lines_line_up() {
        let _c = colors(true);
        let mut g = Grid::new(&["Id", "Title"]).tones([Tone::Muted, Tone::Strong]);
        g.row(["a1", "github"]).row(["b22", "a much longer title"]);
        let s = boxed_note("Items", &g.boxed(80));
        assert!(!s.contains("\x1b[2m\x1b[1mID"), "body dimmed: {s:?}");
        // Every line but the trailing `│` lines up; `Emoji` glyphs fall back
        // to ASCII off a unicode terminal (CI), so don't match on them.
        let lines: Vec<_> = s.lines().collect();
        let widths: Vec<_> = lines[..lines.len() - 1]
            .iter()
            .map(|l| console::measure_text_width(l))
            .collect();
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "{widths:?}\n{s}");
    }
}
