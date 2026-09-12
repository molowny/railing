use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

/// Prompt state — determines input routing and prompt display.
#[derive(Default)]
pub enum PromptState {
    #[default]
    Empty,
    Search(String),
    Filter(String),
    Command(String),
}

impl PromptState {
    pub fn text(&self) -> &str {
        match self {
            PromptState::Empty => "",
            PromptState::Search(s) | PromptState::Filter(s) | PromptState::Command(s) => s,
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, PromptState::Empty)
    }

    /// The prefix shown before the input text.
    pub fn prefix(&self) -> &str {
        match self {
            PromptState::Empty => "> ",
            PromptState::Search(_) => "> ",
            PromptState::Filter(_) => "/",
            PromptState::Command(_) => ":",
        }
    }
}

/// Render the prompt line with current input text.
pub fn render(area: Rect, buf: &mut Buffer, state: &PromptState) {
    if area.height == 0 || area.width < 3 {
        return;
    }
    let prefix = state.prefix();
    let text = state.text();
    let style = Style::default().fg(Color::White);
    buf.set_string(area.x, area.y, prefix, style);
    buf.set_string(area.x + prefix.len() as u16, area.y, text, style);
}

/// Cursor position for the prompt.
pub fn cursor_pos(area: Rect, state: &PromptState) -> Option<(u16, u16)> {
    if area.height > 0 && area.width >= 3 {
        let prefix_len = state.prefix().len() as u16;
        let text_len = state.text().len() as u16;
        Some((area.x + prefix_len + text_len, area.y))
    } else {
        None
    }
}
