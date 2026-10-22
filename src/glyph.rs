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
//! Every function takes the [`Caps`] of the terminal it draws for, so the
//! marks are coloured for that terminal and no other.

use crate::style::{Caps, Style};

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
    const fn bar_style(self) -> Style {
        match self {
            Self::Active => Style::new().cyan(),
            Self::Cancel => Style::new().red(),
            Self::Submit => Style::new().bright().black(),
            Self::Error => Style::new().yellow(),
        }
    }
}

/// The step's mark: `◆` `◇` `▲` `■`, coloured.
pub fn step(caps: Caps, state: State) -> String {
    let (mark, color) = match state {
        State::Active => ("◆", state.bar_style()),
        State::Submit => ("◇", Style::new().green()),
        State::Error => ("▲", state.bar_style()),
        State::Cancel => ("■", state.bar_style()),
    };
    color.paint(caps, mark)
}

/// The gutter's `│`, in the step's colour.
pub fn bar(caps: Caps, state: State) -> String {
    state.bar_style().paint(caps, "│")
}

/// The `└` that closes an open step, in its colour.
pub fn corner(caps: Caps, state: State) -> String {
    state.bar_style().paint(caps, "└")
}

/// `text` in the step's colour: an error's message beside its `└`.
pub fn tinted(caps: Caps, state: State, text: &str) -> String {
    state.bar_style().paint(caps, text)
}

/// A prompt's header, as clack draws it: `◆  first line`, then `│  line`
/// for each line after it.
pub fn header(caps: Caps, state: State, prompt: &str) -> String {
    let mut out = String::new();
    for (i, line) in prompt.lines().enumerate() {
        let mark = if i == 0 {
            step(caps, state)
        } else {
            bar(caps, state)
        };
        out += &format!("{mark}  {line}\n");
    }
    out
}

/// `text` in the muted tone ([`DIM_256`]).
pub fn dim(caps: Caps, text: &str) -> String {
    Style::new().color256(DIM_256).paint(caps, text)
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
        let caps = Caps::COLOR;
        for (ours, theirs, mark) in [
            (State::Active, ThemeState::Active, "◆"),
            (State::Submit, ThemeState::Submit, "◇"),
            (State::Error, ThemeState::Error(String::new()), "▲"),
            (State::Cancel, ThemeState::Cancel, "■"),
        ] {
            assert_eq!(
                step(caps, ours),
                theme.state_symbol_color(&theirs).apply_to(mark).to_string(),
                "{ours:?}"
            );
            assert_eq!(
                bar(caps, ours),
                theme.bar_color(&theirs).apply_to("│").to_string(),
                "{ours:?}"
            );
        }
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }
}
