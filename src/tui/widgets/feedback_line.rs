//! Transient feedback line (success / error) for inline TUIs.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};

/// Transient feedback attached to an inline UI. Typically held as
/// `Option<Feedback>` inside app state and rendered near the bottom.
pub struct Feedback {
    pub message: String,
    pub is_error: bool,
    pub shown_at: Instant,
}

impl Feedback {
    pub fn ok(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            is_error: false,
            shown_at: Instant::now(),
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            is_error: true,
            shown_at: Instant::now(),
        }
    }

    /// Has this feedback been displayed for at least `secs` seconds?
    pub fn expired(&self, secs: u64) -> bool {
        self.shown_at.elapsed().as_secs() >= secs
    }
}

/// Render a single feedback line at `(x, y)`. No-op when `feedback`
/// is `None`.
pub fn render(buf: &mut Buffer, x: u16, y: u16, feedback: Option<&Feedback>) {
    let Some(fb) = feedback else { return };
    let style = if fb.is_error {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::Green)
    };
    buf.set_string(x, y, &fb.message, style);
}
