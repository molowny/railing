use ratatui::style::{Color, Modifier, Style};

// Every colour here is a role of the process's theme (`crate::theme`,
// `docs/theming.md`), resolved for its background when called: the kit's own
// look on a dark terminal is the fixed greys and ANSI colours below; on a
// light one the greys move; an app's installed theme overrides any of them.

/// Background of the selected/active row: the theme's `selection` (kit:
/// 240 `#585858` on dark, 252 on light). A grayscale-ramp entry, not palette
/// `DarkGray`: what that slot looks like is up to the terminal theme, which
/// made the text drawn on it unpredictable.
pub fn bg_selected() -> Color {
    crate::theme::current().selection.ratatui()
}

/// Dim text on [`bg_selected`]: labels and metadata on the cursor row (kit:
/// 248 on dark, 243 on light). Fixed, because the palette's `Gray` renders
/// as white in many themes and stops reading as dimmer than the value beside
/// it.
pub fn fg_dim_selected() -> Color {
    crate::theme::current().muted_on_selection.ratatui()
}

/// Background for the cursor row of a pane that has lost focus: fainter than
/// [`bg_selected`], so it never competes with the focused pane's cursor, but
/// still there (kit: 237 on dark, 254 on light).
pub fn bg_selected_inactive() -> Color {
    crate::theme::current().selection_inactive.ratatui()
}

/// Primary text (titles, values): kit white on dark, the terminal's own
/// foreground on light.
pub fn fg_primary() -> Color {
    crate::theme::current().text.ratatui()
}

/// Secondary text (subtitles, descriptions).
pub fn fg_secondary() -> Color {
    crate::theme::current().text_secondary.ratatui()
}

/// Dim text (borders, metadata, inactive hints): the theme's `muted`, the
/// same grey the flow and tables use.
pub fn fg_dim() -> Color {
    crate::theme::current().muted.ratatui()
}

/// Lines that only mark where one pane ends and the next begins: well below
/// [`fg_dim`], structure to be found rather than read.
pub fn fg_separator() -> Color {
    crate::theme::current().separator.ratatui()
}

/// Keycaps, group titles and other call-to-action chrome.
pub fn accent() -> Color {
    crate::theme::current().accent.ratatui()
}

/// Search/filter queries and mode titles (e.g. the which-key group name),
/// distinct from the [`accent`] used for keys.
pub fn search() -> Color {
    crate::theme::current().highlight.ratatui()
}

/// Style for selected row text.
pub fn selected(bold: bool) -> Style {
    let s = Style::default().fg(fg_primary()).bg(bg_selected());
    if bold {
        s.add_modifier(Modifier::BOLD)
    } else {
        s
    }
}

/// Style for normal (unselected) text.
pub fn normal() -> Style {
    Style::default().fg(fg_primary())
}

/// Style for dim/metadata text.
pub fn dim() -> Style {
    Style::default().fg(fg_dim())
}

/// Style for dim text on the selected background.
///
/// Not `dim().bg(bg_selected())`: [`fg_dim()`] sits too close to [`bg_selected()`]
/// to read on it. Metadata on a selected row has to dim *relative to* that
/// row, which means stepping the foreground down from [`fg_primary()`] rather
/// than all the way to [`fg_dim()`] — to [`fg_dim_selected()`].
pub fn dim_selected() -> Style {
    Style::default().fg(fg_dim_selected()).bg(bg_selected())
}

/// Background style for selected area (spaces/fill).
pub fn selected_bg() -> Style {
    Style::default().bg(bg_selected())
}

/// Style for the cursor row of an unfocused pane, on [`bg_selected_inactive()`].
pub fn selected_inactive() -> Style {
    Style::default()
        .fg(fg_secondary())
        .bg(bg_selected_inactive())
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
        let t = crate::theme::current();
        if secs_left <= self.urgent_at {
            t.danger.ratatui()
        } else if secs_left <= self.warn_at {
            t.warning.ratatui()
        } else {
            t.success.ratatui()
        }
    }
}

/// Highlight for in-place substring matches (e.g. detail pane search).
pub fn match_hit() -> Style {
    Style::default()
        .fg(search())
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
        assert_eq!(
            s.bg,
            Some(bg_selected()),
            "must keep the selected row's fill"
        );
        assert_ne!(s.fg, Some(fg_primary()), "not dimmer than primary text");
        assert_ne!(s.fg, Some(bg_selected()), "invisible against its own row");
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
        let ladder = [
            bg_selected_inactive(),
            bg_selected(),
            fg_dim(),
            fg_dim_selected(),
        ]
        .map(index);
        assert!(ladder.windows(2).all(|w| w[0] < w[1]), "{ladder:?}");
    }
}
