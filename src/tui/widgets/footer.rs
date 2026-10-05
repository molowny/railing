use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

/// Footer with context-sensitive key hints that progressively collapse
/// when the terminal is too narrow.
pub struct FooterProps {
    /// (key, action) pairs sorted by priority (most important first).
    pub hints: Vec<(&'static str, &'static str)>,
}

impl FooterProps {
    pub fn new(hints: Vec<(&'static str, &'static str)>) -> Self {
        Self { hints }
    }

    /// Render the footer, dropping hints that don't fit.
    pub fn render(&self, area: Rect, buf: &mut Buffer, width: u16) {
        if area.height == 0 || self.hints.is_empty() {
            return;
        }

        let style = Style::default().fg(crate::tui::theme::FG_DIM);
        let key_style = Style::default().fg(Color::Gray);

        let available = width as usize;
        let mut parts: Vec<String> = Vec::new();
        let mut total_len = 0;

        for (key, action) in &self.hints {
            let part = format!("{key} {action}");
            let needed = if parts.is_empty() {
                part.len() + 1
            } else {
                part.len() + 2
            };
            if total_len + needed > available {
                break;
            }
            total_len += needed;
            parts.push(part);
        }

        let line = format!(" {}", parts.join("  "));
        buf.set_string(area.x, area.y, &line, style);

        // Highlight the key portions
        let mut x = area.x + 1;
        for (i, (key, _action)) in self.hints.iter().enumerate() {
            if i >= parts.len() {
                break;
            }
            if i > 0 {
                x += 2;
            }
            buf.set_string(x, area.y, key, key_style);
            x += parts[i].len() as u16;
        }
    }
}
