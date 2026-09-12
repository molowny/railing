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
pub fn truncated_str(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    text: &str,
    max_width: u16,
    style: Style,
) -> u16 {
    let max = max_width as usize;
    if text.len() <= max {
        buf.set_string(x, y, text, style);
        text.len() as u16
    } else if max > 1 {
        buf.set_string(x, y, &text[..max - 1], style);
        buf.set_string(x + max as u16 - 1, y, "\u{2026}", style); // …
        max_width
    } else {
        0
    }
}

/// Render text centered horizontally within `[x, x + width)`.
pub fn centered_str(buf: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    let text_w = text.len() as u16;
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
    let header = format!("── {} ", title);
    let pad_len = (width as usize).saturating_sub(header.len());
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
