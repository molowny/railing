use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};

/// Overlay [`theme::match_hit`] on the cells covering case-insensitive
/// substring matches of `query` inside `text`, starting at `(x, y)` and clipped
/// to `max_w` display columns.
///
/// A second pass over already-painted cells: it patches style only, never
/// content, so the caller paints the row once and highlights afterwards without
/// re-deriving the layout.
///
/// Positions are resolved in display columns, so a match after a wide char
/// lands on the right cells. Matching happens against the lowercased text, so
/// the rare case-folding that changes a string's length (`İ` → `i̇`) can shift
/// a highlight by a column; nothing is corrupted, and correcting it would take
/// a full case-insensitive matcher rather than a paint helper.
///
/// [`theme::match_hit`]: super::theme::match_hit
pub fn overlay_match_highlights(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    text: &str,
    max_w: u16,
    query: &str,
) {
    if query.is_empty() || max_w == 0 || text.is_empty() {
        return;
    }
    let text_lc = text.to_lowercase();
    let query_lc = query.to_lowercase();
    let style = super::theme::match_hit();

    // Starting display column of each char, indexed by its byte offset, plus a
    // final entry for the end of the string so a match ending at the last char
    // has a column to stop at.
    let mut cols: Vec<(usize, usize)> = Vec::new();
    let mut col = 0usize;
    for (byte, ch) in text_lc.char_indices() {
        cols.push((byte, col));
        col += super::text::char_width(ch);
    }
    cols.push((text_lc.len(), col));

    let column_at = |byte: usize| -> usize {
        match cols.binary_search_by_key(&byte, |(b, _)| *b) {
            Ok(i) => cols[i].1,
            // A byte inside a char maps to that char's starting column.
            Err(i) => cols[i.saturating_sub(1)].1,
        }
    };

    let mut from = 0usize;
    while let Some(rel) = text_lc[from..].find(&query_lc) {
        let start = from + rel;
        let end = start + query_lc.len();
        for c in column_at(start)..column_at(end) {
            if c >= max_w as usize {
                break;
            }
            if let Some(cell) = buf.cell_mut(Position::new(x + c as u16, y)) {
                let merged = cell.style().patch(style);
                cell.set_style(merged);
            }
        }
        // Always advance, so an empty-after-lowercasing query can't spin.
        from = end.max(start + 1);
    }
}

/// Set a single cell (bounds-checked).
pub fn set_cell(buf: &mut Buffer, x: u16, y: u16, ch: char, style: Style) {
    if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
        cell.set_char(ch);
        cell.set_style(style);
    }
}

/// Fill a rectangular region with a background color (spaces).
pub fn fill_bg(buf: &mut Buffer, area: Rect, bg: Color) {
    let style = Style::default().bg(bg);
    for row in area.y..area.y + area.height {
        for col in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut(Position::new(col, row)) {
                cell.set_char(' ');
                cell.set_style(style);
            }
        }
    }
}

/// Draw a horizontal line of a character across a row.
pub fn hline(buf: &mut Buffer, x: u16, y: u16, width: u16, ch: char, style: Style) {
    for bx in x..x + width {
        set_cell(buf, bx, y, ch, style);
    }
}

/// Render text, truncating with ellipsis if it exceeds `max_width`.
/// Returns the number of columns consumed.
///
/// Widths are counted in terminal columns (not bytes and not chars), and the
/// cut always lands on a char boundary — narrow terminals routinely truncate
/// mid-string through multi-byte text (`•`, accented latin, CJK).
pub fn truncated_str(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    text: &str,
    max_width: u16,
    style: Style,
) -> u16 {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

    let max = max_width as usize;
    if max == 0 {
        return 0;
    }
    let text_w = UnicodeWidthStr::width(text);
    if text_w <= max {
        buf.set_string(x, y, text, style);
        return text_w as u16;
    }
    // Fit as many whole chars as possible into `max - 1` columns, leaving one
    // column for the ellipsis. A trailing wide char that no longer fits is
    // dropped whole, so the result may be one column short of `max`.
    let budget = max - 1;
    let mut used = 0usize;
    let mut end = 0usize;
    for (i, ch) in text.char_indices() {
        let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_w > budget {
            break;
        }
        used += ch_w;
        end = i + ch.len_utf8();
    }
    buf.set_string(x, y, &text[..end], style);
    buf.set_string(x + used as u16, y, "\u{2026}", style); // …
    (used + 1) as u16
}

/// Render text centered horizontally within `[x, x + width)`.
pub fn centered_str(buf: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    let text_w = unicode_width::UnicodeWidthStr::width(text) as u16;
    let cx = x + width.saturating_sub(text_w) / 2;
    buf.set_string(cx, y, text, style);
}

/// Render a badge: bold text on colored background, padded.
/// Format: ` TEXT  ` (space + text left-padded to 5 chars).
pub fn badge(buf: &mut Buffer, x: u16, y: u16, text: &str, fg: Color, bg: Color) {
    let styled = Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD);
    let padded = format!(" {text:<5}");
    buf.set_string(x, y, &padded, styled);
}

/// Render a section header: "── Title ──────────" filling width.
pub fn section_header(buf: &mut Buffer, x: u16, y: u16, width: u16, title: &str, style: Style) {
    use unicode_width::UnicodeWidthStr;
    let header = format!("── {} ", title);
    let header_w = UnicodeWidthStr::width(header.as_str());
    let pad_len = (width as usize).saturating_sub(header_w);
    let pad = "─".repeat(pad_len);
    buf.set_string(x, y, format!("{header}{pad}"), style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    /// Columns carrying a highlight, as `#`, for a row painted with `text`.
    fn highlight_map(text: &str, width: u16, query: &str) -> String {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        truncated_str(&mut buf, 0, 0, text, width, Style::default());
        overlay_match_highlights(&mut buf, 0, 0, text, width, query);

        let hit = super::super::theme::match_hit();
        (0..width)
            .map(|c| {
                let style = buf.cell(Position::new(c, 0)).unwrap().style();
                if style.fg == hit.fg { '#' } else { '.' }
            })
            .collect()
    }

    #[test]
    fn overlay_highlights_ascii_match() {
        assert_eq!(highlight_map("hello world", 11, "lo"), "...##......");
    }

    #[test]
    fn overlay_highlights_every_occurrence_case_insensitively() {
        assert_eq!(highlight_map("abAB", 4, "ab"), "####");
    }

    /// Regression: the sealward original used char-count as a stand-in for the
    /// display column, so a match sitting after a wide char was highlighted
    /// one cell to the left for every ideograph preceding it.
    #[test]
    fn overlay_highlights_land_on_columns_not_char_indices() {
        // "日本" is 4 columns, so "ab" occupies columns 4 and 5.
        assert_eq!(highlight_map("日本ab", 6, "ab"), "....##");
    }

    #[test]
    fn overlay_clips_to_max_width() {
        // The match starts at column 3 but only column 3 is inside `max_w`.
        assert_eq!(highlight_map("abcdef", 4, "de"), "...#");
    }

    #[test]
    fn overlay_ignores_empty_query_and_no_match() {
        assert_eq!(highlight_map("hello", 5, ""), ".....");
        assert_eq!(highlight_map("hello", 5, "zz"), ".....");
    }

    #[test]
    fn truncated_str_fits() {
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        let used = truncated_str(&mut buf, 0, 0, "hello", 10, Style::default());
        assert_eq!(used, 5);
    }

    #[test]
    fn truncated_str_overflows() {
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        let used = truncated_str(&mut buf, 0, 0, "hello world", 5, Style::default());
        assert_eq!(used, 5);
    }

    /// Regression: truncating masked values (`•` = 3 bytes, 1 column) used to
    /// slice by byte index and panic on a char boundary when the pane got
    /// narrow enough to cut mid-bullet.
    #[test]
    fn truncated_str_multibyte_does_not_panic() {
        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);
        let text = "\u{2022}".repeat(12);
        for w in 0..=40u16 {
            let used = truncated_str(&mut buf, 0, 0, &text, w, Style::default());
            assert!(used <= w, "used {used} > max {w}");
        }
    }

    #[test]
    fn truncated_str_counts_columns_not_bytes() {
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        // 14 columns, 16 bytes: must render whole, not be truncated.
        let used = truncated_str(&mut buf, 0, 0, "Kucharek Sześć", 14, Style::default());
        assert_eq!(used, 14);
        assert_eq!(buf.cell(Position::new(13, 0)).unwrap().symbol(), "ć");
    }

    #[test]
    fn truncated_str_ellipsis_lands_after_last_char() {
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        let used = truncated_str(&mut buf, 0, 0, "ćććććć", 4, Style::default());
        assert_eq!(used, 4);
        assert_eq!(buf.cell(Position::new(2, 0)).unwrap().symbol(), "ć");
        assert_eq!(buf.cell(Position::new(3, 0)).unwrap().symbol(), "\u{2026}");
    }

    /// Wide (2-column) chars must not overflow the budget: a char that no
    /// longer fits is dropped whole rather than half-painted.
    #[test]
    fn truncated_str_wide_chars_stay_in_budget() {
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        let used = truncated_str(&mut buf, 0, 0, "日本語テキスト", 6, Style::default());
        assert!(used <= 6);
        assert_eq!(
            buf.cell(Position::new(used - 1, 0)).unwrap().symbol(),
            "\u{2026}"
        );
    }

    #[test]
    fn fill_bg_fills_area() {
        let area = Rect::new(0, 0, 3, 2);
        let mut buf = Buffer::empty(area);
        fill_bg(&mut buf, area, Color::Red);
        for y in 0..2 {
            for x in 0..3 {
                let cell = buf.cell(Position::new(x, y)).unwrap();
                assert_eq!(cell.symbol(), " ");
            }
        }
    }
}
