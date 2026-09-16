//! Horizontal rule with a right-aligned status label.
//!
//! Example: `────────────────────────── 3/12 ──`

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};

use crate::tui::theme;

/// Render a horizontal rule across `width`, with `label` right-aligned
/// over the rule (padded with spaces). Pass `""` for a plain rule.
pub fn render(buf: &mut Buffer, x: u16, y: u16, width: u16, label: &str) {
    let hr_style = Style::default().fg(Color::DarkGray);

    if label.is_empty() {
        let line: String = "\u{2500}".repeat(width as usize);
        buf.set_string(x, y, &line, hr_style);
        return;
    }

    let padded = format!(" {label} ");
    let label_w = padded.len() as u16;
    let rule_w = width.saturating_sub(label_w);
    let rule: String = "\u{2500}".repeat(rule_w as usize);
    buf.set_string(x, y, &rule, hr_style);
    buf.set_string(x + rule_w, y, &padded, theme::dim());
}
