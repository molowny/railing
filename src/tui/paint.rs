use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};

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
