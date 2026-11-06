//! Tables for command output: [`Grid`] with semantic [`Tone`]s.
//!
//! A grid is drawn two ways: *boxed*, a borderless table in its own titled
//! box inside a live flow, and *plain*, the bordered table on stdout. Tones
//! are applied at render time, so each way colours only when its stream does.
//!
//! The layout is the kit's own (core refactor M7; it was comfy-table's, and
//! draws the same bytes): each column as wide as its widest line, measured
//! without escapes; every cell padded by a space each side (the boxed
//! table's first column has none on the left, the box gives the margin); a
//! cell's lines, `\n`-separated, take as many rows as they need. When the
//! table is wider than it may be, the widest column gives way a column at a
//! time and its cells wrap at words.
//!
//! ```text
//! ┌──────────────────────┐     NAME   KIND   SIZE     ← boxed: no rules
//! │ Name    Kind    Size │     alpha  file  12 KB
//! ╞══════════════════════╡
//! │ alpha   file   12 KB │     ← plain: the bordered table
//! └──────────────────────┘
//! ```
//!
//! The box is the note's frame ([`crate::frame`]), its body left as the
//! table styled it: a note dims its whole body, which suits prose and would
//! flatten a table to one grey.

use crate::color::Style;
use crate::style::measure;

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

    /// This tone as a style ([`crate::color::Style`]), for text outside a
    /// grid.
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

    /// A whole padded cell in this tone, as a table draws it: bold reset
    /// with `0m`, a colour with `39m` (the foreground only), each as the 256
    /// palette index of the base ANSI colour, as comfy-table wrote them.
    fn paint_cell(self, text: &str) -> String {
        let fg = |n: u8| format!("\x1b[38;5;{n}m{text}\x1b[39m");
        match self {
            Tone::Default => text.to_string(),
            Tone::Strong => format!("\x1b[1m{text}\x1b[0m"),
            Tone::Muted => fg(crate::glyph::DIM_256),
            Tone::Accent => fg(6),
            Tone::Success => fg(2),
            Tone::Warn => fg(3),
            Tone::Danger => fg(1),
        }
    }
}

/// How a table is drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    /// The bordered table: `┌─┐`, a `╞═╡` under the header, `└─┘`, a space
    /// between columns.
    Bordered,
    /// No rules and no space between columns (the box is the border); the
    /// first column unpadded on the left.
    Bare,
}

/// A laid-out cell: its lines (cut to its column), how it is drawn.
struct Laid {
    lines: Vec<String>,
    tone: Tone,
    right: bool,
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

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The cell's sort key: its own, or its text (empty and `—` are missing).
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
            Some(&(_, width)) => crate::style::truncate(text, width, "…"),
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

    pub(crate) fn headers(&self) -> &[String] {
        &self.headers
    }

    pub(crate) fn lead_lines(&self) -> &[String] {
        &self.lead
    }

    pub(crate) fn rows(&self) -> &[Vec<GridCell>] {
        &self.rows
    }

    pub(crate) fn is_right(&self, column: usize) -> bool {
        self.right.contains(&column)
    }

    /// The tone `cell` in `column` is drawn with.
    pub(crate) fn tone_of(&self, column: usize, cell: &GridCell) -> Tone {
        cell.tone
            .unwrap_or_else(|| self.tones.get(column).copied().unwrap_or_default())
    }

    /// The table's rows as text: the header (when shown) and every row, each
    /// cell with its tone, laid out by [`Look`], no wider than `max` columns
    /// when one is given.
    fn render(
        &self,
        look: Look,
        headers: Vec<String>,
        color: bool,
        max: Option<usize>,
    ) -> Vec<String> {
        let mut rows: Vec<Vec<(String, Tone)>> = Vec::new();
        if !self.no_header {
            rows.push(headers.into_iter().map(|h| (h, self.header_tone)).collect());
        }
        for r in &self.rows {
            rows.push(
                r.iter()
                    .enumerate()
                    .map(|(i, c)| (self.drawn(i, &c.text).into_owned(), self.tone_of(i, c)))
                    .collect(),
            );
        }
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return Vec::new();
        }
        let pad = |c: usize| match (look, c) {
            (Look::Bare, 0) => (0, 1),
            _ => (1, 1),
        };
        let mut widths = vec![0; columns];
        for row in &rows {
            for (c, (text, _)) in row.iter().enumerate() {
                for line in text.split('\n') {
                    widths[c] = widths[c].max(measure(line));
                }
            }
        }
        let chrome = match look {
            Look::Bordered => 2 + (columns - 1),
            Look::Bare => 0,
        } + (0..columns).map(|c| pad(c).0 + pad(c).1).sum::<usize>();
        // Too wide: the widest column gives way, a column at a time.
        if let Some(max) = max {
            while chrome + widths.iter().sum::<usize>() > max {
                let Some((c, _)) = widths
                    .iter()
                    .enumerate()
                    .filter(|&(_, &w)| w > 1)
                    .max_by_key(|&(c, &w)| (w, std::cmp::Reverse(c)))
                else {
                    break;
                };
                widths[c] -= 1;
            }
        }
        let laid: Vec<Vec<Laid>> = rows
            .iter()
            .map(|row| {
                (0..columns)
                    .map(|c| match row.get(c) {
                        Some((text, tone)) => Laid {
                            lines: text.split('\n').flat_map(|l| fit(l, widths[c])).collect(),
                            tone: *tone,
                            right: self.right.contains(&c),
                        },
                        None => Laid {
                            lines: Vec::new(),
                            tone: Tone::Default,
                            right: false,
                        },
                    })
                    .collect()
            })
            .collect();

        let inner = chrome - 2 * usize::from(look == Look::Bordered) + widths.iter().sum::<usize>();
        let rule = |l: &str, fill: &str, r: &str| format!("{l}{}{r}", fill.repeat(inner));
        let mut out = Vec::new();
        if look == Look::Bordered {
            out.push(rule("┌", "─", "┐"));
        }
        for (i, row) in laid.iter().enumerate() {
            let height = row.iter().map(|c| c.lines.len()).max().unwrap_or(0).max(1);
            for k in 0..height {
                let cells: Vec<String> = row
                    .iter()
                    .enumerate()
                    .map(|(c, cell)| {
                        let (l, r) = pad(c);
                        let w = widths[c];
                        let Some(line) = cell.lines.get(k) else {
                            return " ".repeat(l + w + r);
                        };
                        let room = " ".repeat(w.saturating_sub(measure(line)));
                        let body = if cell.right {
                            format!("{room}{line}")
                        } else {
                            format!("{line}{room}")
                        };
                        let padded = format!("{}{body}{}", " ".repeat(l), " ".repeat(r));
                        if color {
                            cell.tone.paint_cell(&padded)
                        } else {
                            padded
                        }
                    })
                    .collect();
                out.push(match look {
                    Look::Bordered => format!("│{}│", cells.join(" ")),
                    Look::Bare => cells.concat(),
                });
            }
            if look == Look::Bordered && i == 0 && !self.no_header {
                out.push(rule("╞", "═", "╡"));
            }
        }
        if look == Look::Bordered {
            out.push(rule("└", "─", "┘"));
        }
        out
    }

    /// Borderless, fitted to `width` columns — the box around it is the
    /// border.
    pub(crate) fn boxed(&self, width: usize) -> String {
        let headers = self.headers.iter().map(|h| h.to_uppercase()).collect();
        let color = crate::color::enabled(crate::color::Stream::Stderr);
        let mut out: Vec<String> = self.lead.clone();
        out.extend(
            self.render(Look::Bare, headers, color, Some(width))
                .into_iter()
                .map(|l| l.trim_end().to_string()),
        );
        out.join("\n")
    }

    /// The bordered table for plain output, coloured only when stdout is,
    /// and fitted to stdout's width when stdout is a terminal.
    pub fn plain(&self) -> String {
        let color = crate::color::enabled(crate::color::Stream::Stdout);
        let mut out = self.lead.clone();
        out.extend(self.render(
            Look::Bordered,
            self.headers.clone(),
            color,
            crate::ui::stdout_cols(),
        ));
        out.join("\n")
    }
}

/// `line` in lines no wider than `width`: as it is when it fits, else
/// wrapped at words (a word longer than the line broken, a styled one cut).
fn fit(line: &str, width: usize) -> Vec<String> {
    if measure(line) <= width {
        return vec![line.to_string()];
    }
    crate::ui::wrap_to(line, width)
        .lines()
        .map(|l| {
            if measure(l) > width {
                crate::style::truncate(l, width, "…").into_owned()
            } else {
                l.to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Colour globals are process-wide; hold `ui`'s lock while set.
    fn colors(on: bool) -> std::sync::MutexGuard<'static, ()> {
        let g = crate::ui::tests::lock();
        crate::color::set_stream(crate::color::Stream::Stdout, on);
        crate::color::set_stream(crate::color::Stream::Stderr, on);
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
    fn a_wide_cell_wraps_without_losing_characters() {
        let _c = colors(false);
        let mut g = Grid::new(&["Id", "Title"]);
        let title = "界".repeat(20);
        g.row(["a1", title.as_str()]);
        let s = g.boxed(20);
        assert!(s.lines().all(|l| measure(l) <= 20), "{s}");
        assert_eq!(s.matches('界').count(), 20, "{s}");
        assert!(!s.contains('…'), "{s}");
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
        for s in [
            g.boxed(80),
            g.plain(),
            crate::frame::Frame::boxed("T", &g.boxed(80)),
        ] {
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
        let s = crate::frame::Frame::boxed("Items", &g.boxed(80));
        assert!(!s.contains("\x1b[2m\x1b[1mID"), "body dimmed: {s:?}");
        // Every line but the trailing `│` lines up.
        let lines: Vec<_> = s.lines().collect();
        let widths: Vec<_> = lines[..lines.len() - 1]
            .iter()
            .map(|l| crate::style::measure(l))
            .collect();
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "{widths:?}\n{s}");
    }
}
