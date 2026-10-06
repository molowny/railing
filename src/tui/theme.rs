use ratatui::style::{Color, Modifier, Style};

/// Background color for the selected/active item.
///
/// A grayscale-ramp entry (`#585858`), not palette `DarkGray`: what that slot
/// looks like is up to the terminal theme, which made the text drawn on it
/// unpredictable. Fixed, it sits reliably above [`BG_SELECTED_INACTIVE`] and
/// below [`FG_DIM_SELECTED`].
pub const BG_SELECTED: Color = Color::Indexed(240);

/// Dim text on [`BG_SELECTED`]: labels and metadata on the cursor row.
///
/// A fixed grey (`#a8a8a8`), because the palette's `Gray` ([`FG_SECONDARY`])
/// renders as white in many themes and so stops reading as dimmer than the
/// value beside it. Clearly below any terminal's white, clearly above the row.
pub const FG_DIM_SELECTED: Color = Color::Indexed(248);

/// Background for the cursor row of a pane that has lost focus.
///
/// Fainter than [`BG_SELECTED`], so it never competes with the focused pane's
/// cursor, but still there: moving focus to a detail pane must not hide which
/// row the detail belongs to. A grayscale-ramp entry (`#3a3a3a`), the same in
/// every terminal theme.
pub const BG_SELECTED_INACTIVE: Color = Color::Indexed(237);

/// Primary text (titles, values).
pub const FG_PRIMARY: Color = Color::White;

/// Secondary text (subtitles, descriptions).
pub const FG_SECONDARY: Color = Color::Gray;

/// Dim text (borders, metadata, inactive hints).
///
/// A fixed 256-colour grey (`#949494`) rather than the palette's `DarkGray`
/// (ANSI 8): what slot 8 looks like is up to the terminal theme, and many make
/// it too dark to read as text. Grayscale-ramp entries are the same everywhere,
/// so dim text keeps one contrast across themes. Still clearly below
/// [`FG_SECONDARY`].
pub const FG_DIM: Color = Color::Indexed(crate::ui::DIM_256);

/// Lines that only mark where one pane ends and the next begins.
///
/// A grayscale-ramp entry (`#444444`) well below [`FG_DIM`]: a separator is
/// structure, not text, and should be found rather than read.
pub const FG_SEPARATOR: Color = Color::Indexed(238);

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
/// Not `dim().bg(BG_SELECTED)`: [`FG_DIM`] sits too close to [`BG_SELECTED`]
/// to read on it. Metadata on a selected row has to dim *relative to* that
/// row, which means stepping the foreground down from [`FG_PRIMARY`] rather
/// than all the way to [`FG_DIM`] — to [`FG_DIM_SELECTED`].
pub fn dim_selected() -> Style {
    Style::default().fg(FG_DIM_SELECTED).bg(BG_SELECTED)
}

/// Background style for selected area (spaces/fill).
pub fn selected_bg() -> Style {
    Style::default().bg(BG_SELECTED)
}

/// Style for the cursor row of an unfocused pane, on [`BG_SELECTED_INACTIVE`].
pub fn selected_inactive() -> Style {
    Style::default().fg(FG_SECONDARY).bg(BG_SELECTED_INACTIVE)
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

/// Colours for a countdown, by how much of it is left: green with time to
/// spare, yellow from `warn_at` seconds, red from `urgent_at`. The thresholds
/// are the app's to choose; what counts as "short" depends on what runs out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Countdown {
    pub warn_at: u64,
    pub urgent_at: u64,
}

impl Countdown {
    pub const fn new(warn_at: u64, urgent_at: u64) -> Self {
        Self { warn_at, urgent_at }
    }

    /// The colour for `secs_left`.
    pub fn color(self, secs_left: u64) -> Color {
        if secs_left <= self.urgent_at {
            Color::Red
        } else if secs_left <= self.warn_at {
            Color::Yellow
        } else {
            Color::Green
        }
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

    #[test]
    fn a_countdown_goes_green_yellow_red() {
        let c = Countdown::new(10, 5);
        assert_eq!(c.color(30), Color::Green);
        assert_eq!(c.color(11), Color::Green);
        assert_eq!(c.color(10), Color::Yellow);
        assert_eq!(c.color(6), Color::Yellow);
        assert_eq!(c.color(5), Color::Red);
        assert_eq!(c.color(0), Color::Red);
    }

    /// The greys a cursor row is built from are fixed ramp entries, ordered
    /// dark to light, so their contrast doesn't depend on the terminal theme:
    /// unfocused row < focused row < dim text < dim text on the focused row.
    #[test]
    fn the_selection_greys_are_fixed_and_ordered() {
        let index = |c: Color| match c {
            Color::Indexed(i) => i,
            other => panic!("{other:?} is a palette colour, theme-dependent"),
        };
        let ladder = [BG_SELECTED_INACTIVE, BG_SELECTED, FG_DIM, FG_DIM_SELECTED].map(index);
        assert!(ladder.windows(2).all(|w| w[0] < w[1]), "{ladder:?}");
    }
}
