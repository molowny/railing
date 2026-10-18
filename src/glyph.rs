//! The flow's marks and tones as plain strings, with no terminal attached:
//! what a process draws when it writes the flow's look on a terminal it
//! chose itself rather than through [`crate::flow`] on stderr.
//!
//! ```text
//! ◆  Master password      ← step(Active): the open step, cyan
//! │  …                    ← bar(Active)
//! └  enter submit         ← corner(Active)
//! ◇  Master password      ← step(Submit): done, green; its bar grey
//! ▲  Wrong password       ← step(Error): yellow
//! ```
//!
//! Colours are cliclack's default theme, which the live flow draws with
//! (`grid::Clack`); a test holds the two together. The marks are always
//! these Unicode ones: cliclack swaps in ASCII (`o`, `*`) when the locale
//! is not UTF-8, which would mix with the box rules (`─╮│`) that never do.
//! Like the rest of cli-kit, colour follows `console`'s stderr flag
//! (`console::set_colors_enabled_stderr`).

use console::{Style, style};

/// 256-colour index of dim text, everywhere cli-kit draws it: CLI output,
/// flows, grids, the spinner's glow, and the TUI's `theme::FG_DIM`.
///
/// A grayscale-ramp entry (`#949494`) rather than SGR 2 "faint", which
/// terminals render anywhere from invisible to identical to normal text, or
/// palette `DarkGray`, whose shade is up to the terminal theme. Readable on
/// dark backgrounds and still clearly below normal text on light ones.
pub const DIM_256: u8 = 246;

/// Where a step is: the four states a clack prompt draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Open, waiting on the person: `◆`, cyan.
    Active,
    /// Answered: `◇` in green, its bar grey.
    Submit,
    /// What was given was not accepted: `▲`, yellow.
    Error,
    /// Given up: `■`, red.
    Cancel,
}

impl State {
    fn bar_style(self) -> Style {
        match self {
            Self::Active => Style::new().cyan(),
            Self::Cancel => Style::new().red(),
            Self::Submit => Style::new().bright().black(),
            Self::Error => Style::new().yellow(),
        }
        .for_stderr()
    }
}

/// The step's mark: `◆` `◇` `▲` `■`, coloured.
pub fn step(state: State) -> String {
    let (mark, color) = match state {
        State::Active => ("◆", state.bar_style()),
        State::Submit => ("◇", Style::new().green().for_stderr()),
        State::Error => ("▲", state.bar_style()),
        State::Cancel => ("■", state.bar_style()),
    };
    color.apply_to(mark).to_string()
}

/// The gutter's `│`, in the step's colour.
pub fn bar(state: State) -> String {
    state.bar_style().apply_to("│").to_string()
}

/// The `└` that closes an open step, in its colour.
pub fn corner(state: State) -> String {
    state.bar_style().apply_to("└").to_string()
}

/// `text` in the step's colour: an error's message beside its `└`.
pub fn tinted(state: State, text: &str) -> String {
    state.bar_style().apply_to(text).to_string()
}

/// A prompt's header, as clack draws it: `◆  first line`, then `│  line`
/// for each line after it.
pub fn header(state: State, prompt: &str) -> String {
    let mut out = String::new();
    for (i, line) in prompt.lines().enumerate() {
        let mark = if i == 0 { step(state) } else { bar(state) };
        out += &format!("{mark}  {line}\n");
    }
    out
}

/// `text` in the muted tone ([`DIM_256`]).
pub fn dim(text: &str) -> String {
    style(text).color256(DIM_256).for_stderr().to_string()
}

#[cfg(all(test, feature = "full"))]
mod tests {
    use super::*;
    use cliclack::{Theme, ThemeState};

    /// The live flow draws with cliclack's theme; these strings must be the
    /// same marks in the same colours, or a step drawn here would not match
    /// the one above it. The mark is compared through cliclack's colour,
    /// not its `state_symbol`: that one is locale-dependent (ASCII under
    /// `LANG=C`, as on CI), ours is not.
    #[test]
    fn marks_match_the_live_flow() {
        let _g = crate::ui::tests::lock();
        // cliclack styles follow stdout's flag, these follow stderr's.
        console::set_colors_enabled(true);
        console::set_colors_enabled_stderr(true);
        let theme = crate::grid::Clack;
        for (ours, theirs, mark) in [
            (State::Active, ThemeState::Active, "◆"),
            (State::Submit, ThemeState::Submit, "◇"),
            (State::Error, ThemeState::Error(String::new()), "▲"),
            (State::Cancel, ThemeState::Cancel, "■"),
        ] {
            assert_eq!(
                step(ours),
                theme.state_symbol_color(&theirs).apply_to(mark).to_string(),
                "{ours:?}"
            );
            assert_eq!(
                bar(ours),
                theme.bar_color(&theirs).apply_to("│").to_string(),
                "{ours:?}"
            );
        }
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }
}
