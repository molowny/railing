use ratatui::style::{Color, Modifier, Style};

/// Background color for the selected/active item.
pub const BG_SELECTED: Color = Color::DarkGray;

/// Primary text (titles, values).
pub const FG_PRIMARY: Color = Color::White;

/// Secondary text (subtitles, descriptions).
pub const FG_SECONDARY: Color = Color::Gray;

/// Dim text (borders, metadata, inactive hints).
pub const FG_DIM: Color = Color::DarkGray;

/// Style for selected row text.
pub fn selected(bold: bool) -> Style {
    let s = Style::default().fg(FG_PRIMARY).bg(BG_SELECTED);
    if bold {
        s.add_modifier(Modifier::BOLD)
    } else {
        s
    }
}

/// Style for normal (unselected) text.
pub fn normal() -> Style {
    Style::default().fg(FG_PRIMARY)
}

/// Style for dim/metadata text.
pub fn dim() -> Style {
    Style::default().fg(FG_DIM)
}

/// Style for dim text on selected background.
pub fn dim_selected() -> Style {
    Style::default().fg(FG_DIM).bg(BG_SELECTED)
}

/// Background style for selected area (spaces/fill).
pub fn selected_bg() -> Style {
    Style::default().bg(BG_SELECTED)
}
