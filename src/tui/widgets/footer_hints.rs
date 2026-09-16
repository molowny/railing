//! Right-aligned key-hint list for inline TUI footers.
//!
//! Example: `r reveal  y copy  q back`

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};

/// Render `hints` right-aligned to `area.x + width`, dropping hints that
/// don't fit. Each hint is a `(key, action)` pair; key is rendered in
/// light gray, action in dark gray.
pub fn render_right(buf: &mut Buffer, x: u16, y: u16, width: u16, hints: &[(&str, &str)]) {
    if hints.is_empty() {
        return;
    }

    let action_style = Style::default().fg(Color::DarkGray);
    let key_style = Style::default().fg(Color::Gray);

    let total_w: u16 = hints
        .iter()
        .enumerate()
        .map(|(i, (key, action))| {
            let sep = if i > 0 { 2u16 } else { 0 };
            sep + unicode_width::UnicodeWidthStr::width(*key) as u16 + 1 + action.len() as u16
        })
        .sum();

    let mut fx = (x + width).saturating_sub(total_w + 1);
    for (i, (key, action)) in hints.iter().enumerate() {
        if i > 0 {
            fx += 2;
        }
        let key_w = unicode_width::UnicodeWidthStr::width(*key) as u16;
        let action_w = action.len() as u16;
        if fx + key_w + 1 + action_w > x + width {
            break;
        }
        buf.set_string(fx, y, key, key_style);
        fx += key_w;
        buf.set_string(fx, y, " ", action_style);
        fx += 1;
        buf.set_string(fx, y, action, action_style);
        fx += action_w;
    }
}
