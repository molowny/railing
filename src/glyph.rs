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
//! Colours are clack's default theme, which the live flow draws with. The
//! marks are always these Unicode ones, whatever the locale: cliclack, which
//! drew the flow until M3, swapped in ASCII (`o`, `*`) when the locale was
//! not UTF-8, which mixed with the box rules (`─╮│`) that never did.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// clack's default theme, the colours the flow has always drawn with
    /// (they were cliclack's until it left, M3): a step drawn here matches
    /// one drawn by another process.
    #[test]
    fn marks_in_clacks_colours() {
        let c = Caps::COLOR;
        for (state, mark, bar) in [
            (State::Active, "\x1b[36m◆\x1b[0m", "\x1b[36m│\x1b[0m"),
            (State::Submit, "\x1b[32m◇\x1b[0m", "\x1b[38;5;8m│\x1b[0m"),
            (State::Error, "\x1b[33m▲\x1b[0m", "\x1b[33m│\x1b[0m"),
            (State::Cancel, "\x1b[31m■\x1b[0m", "\x1b[31m│\x1b[0m"),
        ] {
            assert_eq!(step(c, state), mark, "{state:?}");
            assert_eq!(super::bar(c, state), bar, "{state:?}");
        }
        assert_eq!(step(Caps::PLAIN, State::Active), "◆");
    }
}
