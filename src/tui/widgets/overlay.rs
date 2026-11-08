use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::tui::paint;

/// Render a bordered panel with title and close hint.
/// Returns the y position after the top border (where content can start).
pub fn render_panel_top(area: Rect, buf: &mut Buffer, title: &str, close_hint: &str) -> u16 {
    let dim = Style::default().fg(crate::tui::theme::fg_dim());

    let mut y = area.y;

    // Top border
    paint::hline(buf, area.x, y, area.width, '─', dim);
    y += 1;

    // Title + close hint
    let header_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    buf.set_string(area.x + 2, y, title, header_style);
    buf.set_string(
        area.x + area.width.saturating_sub(close_hint.len() as u16 + 2),
        y,
        close_hint,
        dim,
    );
    y += 1;

    y
}

/// Render key-binding rows for a section.
/// Updates `y` as rows are rendered.
pub fn render_keybindings(
    buf: &mut Buffer,
    x: u16,
    y: &mut u16,
    max_y: u16,
    section_title: &str,
    bindings: &[(&str, &str)],
) {
    if *y >= max_y {
        return;
    }
    let section_style = Style::default().fg(Color::Yellow);
    let key_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(Color::Gray);

    buf.set_string(x + 2, *y, format!("  {section_title}"), section_style);
    *y += 1;

    for (key, desc) in bindings {
        if *y >= max_y {
            break;
        }
        buf.set_string(x + 4, *y, format!("{key:<10}"), key_style);
        buf.set_string(x + 15, *y, *desc, desc_style);
        *y += 1;
    }
}

/// Render a confirmation dialog with warning icon and hint.
pub fn render_confirm(area: Rect, buf: &mut Buffer, message: &str) {
    if area.height < 3 || area.width < 20 {
        return;
    }

    let dim = Style::default().fg(crate::tui::theme::fg_dim());
    let warn_style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
    let hint_style = Style::default().fg(Color::Gray);

    let mut y = area.y;

    // Top border
    paint::hline(buf, area.x, y, area.width, '─', dim);
    y += 1;

    // Warning icon + message
    buf.set_string(area.x + 2, y, "\u{26a0} ", warn_style);
    buf.set_string(area.x + 5, y, message, Style::default().fg(Color::White));
    y += 1;

    // Hint
    buf.set_string(area.x + 2, y, "Enter confirm  Esc cancel", hint_style);
    y += 1;

    // Bottom border
    if y < area.y + area.height {
        paint::hline(buf, area.x, y, area.width, '─', dim);
    }
}
