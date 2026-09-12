use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use unicode_width::UnicodeWidthStr;

use crate::tui::theme;

/// Render a top bar: left text + right text, filled with `─`.
/// Optionally shows a warning badge before the right text.
pub fn render(
    area: Rect,
    buf: &mut Buffer,
    left: &str,
    right: &str,
    warning: Option<(&str, Style)>,
) {
    if area.height == 0 {
        return;
    }

    let bar_style = theme::dim();
    let text_style = theme::normal();

    let left_padded = format!(" {left} ");

    // Fill entire line with ─
    for x in area.x..area.x + area.width {
        crate::tui::paint::set_cell(buf, x, area.y, '─', bar_style);
    }

    // Write left text
    buf.set_string(area.x, area.y, &left_padded, text_style);

    // Write right text, right-aligned
    let right_width = right.width() as u16;
    let warn_width = warning.as_ref().map_or(0, |(w, _)| w.width() as u16 + 1);
    let total_right = right_width + warn_width;

    if total_right < area.width {
        let x = area.x + area.width - total_right;
        if let Some((warn, warn_style)) = warning {
            buf.set_string(x, area.y, warn, warn_style);
        }
        buf.set_string(area.x + area.width - right_width, area.y, right, text_style);
    }
}
