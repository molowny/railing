use ratatui::style::{Color, Modifier, Style};

/// Background color for the selected/active item.
pub const BG_SELECTED: Color = Color::DarkGray;

/// Primary text (titles, values).
pub const FG_PRIMARY: Color = Color::White;

/// Secondary text (subtitles, descriptions).
pub const FG_SECONDARY: Color = Color::Gray;

/// Dim text (borders, metadata, inactive hints).
pub const FG_DIM: Color = Color::DarkGray;

/// Accent color for keycaps, group titles, and other call-to-action chrome.
pub const ACCENT: Color = Color::Cyan;

/// Color for search/filter queries and mode titles (e.g. the which-key group
/// name), distinct from the [`ACCENT`] used for keys.
pub const SEARCH: Color = Color::Yellow;

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

/// Style for dim text on the selected background.
///
/// Not `dim().bg(BG_SELECTED)`: [`FG_DIM`] and [`BG_SELECTED`] are the same
/// color, so that spelling paints the text invisibly. Metadata on a selected
/// row has to dim *relative to* that row, which means stepping the foreground
/// down from [`FG_PRIMARY`] rather than all the way to [`FG_DIM`].
pub fn dim_selected() -> Style {
    Style::default().fg(FG_SECONDARY).bg(BG_SELECTED)
}

/// Background style for selected area (spaces/fill).
pub fn selected_bg() -> Style {
    Style::default().bg(BG_SELECTED)
}

/// Style for one row of a list or outline, given its selection state and
/// whether its pane is unfocused.
///
/// `dim` wins over `selected`: an unfocused pane still tracks a cursor, but
/// showing it at full contrast competes with the pane that actually has focus.
pub fn row(is_selected: bool, dim_pane: bool) -> Style {
    if dim_pane {
        dim()
    } else if is_selected {
        selected(false)
    } else {
        normal()
    }
}

/// Highlight for in-place substring matches (e.g. detail pane search).
pub fn match_hit() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Text the same color as the cell behind it is not dim, it is gone.
    /// `FG_DIM == BG_SELECTED` today, so any style pairing them is a bug.
    #[test]
    fn no_style_paints_text_in_its_own_background_color() {
        let cases = [
            ("selected", selected(false)),
            ("selected_bold", selected(true)),
            ("dim_selected", dim_selected()),
            ("row_selected", row(true, false)),
            ("match_hit", match_hit()),
        ];
        for (name, style) in cases {
            if let (Some(fg), Some(bg)) = (style.fg, style.bg) {
                assert_ne!(fg, bg, "{name} is invisible");
            }
        }
    }

    /// The point of `dim_selected` is to read as metadata next to
    /// full-contrast text on the same row.
    #[test]
    fn dim_selected_sits_between_primary_and_the_background() {
        let s = dim_selected();
        assert_eq!(s.bg, Some(BG_SELECTED), "must keep the selected row's fill");
        assert_ne!(s.fg, Some(FG_PRIMARY), "not dimmer than primary text");
        assert_ne!(s.fg, Some(BG_SELECTED), "invisible against its own row");
    }
}
