//! The live group's own lines, as plain strings: the `┌` that opens it, the
//! one-line log entries in its gutter, and the `└` that closes it. Pure, like
//! [`crate::step`]: [`crate::flow::Flow`] writes them on stderr, and anything
//! drawing the flow's look on a terminal of its own can write them there.
//!
//! ```text
//! ┌  sealward unlock        ← open (grey)
//! │
//! ◇  Vault unlocked         ← log(Step): the mark, then the text
//! │                         ← each entry leaves a bar below it
//! └  Done                   ← close (grey corner)
//! ```
//!
//! The marks are always these Unicode ones, whatever the locale, as the box
//! rules (`─╮│`) always are. Colours are clack's default theme.

use crate::glyph::{self, State};
use crate::style::Caps;

/// The mark a log entry starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// `◇` green: a step that is done.
    Step,
    /// `●` blue.
    Info,
    /// `├` grey: a remark hung off the gutter.
    Remark,
    /// `◆` green.
    Success,
    /// `▲` yellow.
    Warning,
    /// `■` red.
    Error,
}

impl Mark {
    fn paint(self, caps: Caps) -> String {
        let (mark, style) = match self {
            Mark::Step => ("◇", crate::theme::done()),
            Mark::Info => ("●", crate::theme::info()),
            Mark::Remark => return gray(caps, "├"),
            Mark::Success => ("◆", crate::theme::success()),
            Mark::Warning => ("▲", crate::theme::warning()),
            Mark::Error => ("■", crate::theme::danger()),
        };
        style.paint(caps, mark)
    }
}

/// The grey of the group's frame: a submitted step's bar.
fn gray(caps: Caps, text: &str) -> String {
    glyph::tinted(caps, State::Submit, text)
}

/// `┌  title` and the bar under it.
pub fn open(caps: Caps, title: &str) -> String {
    format!("{}  {title}\n{}\n", gray(caps, "┌"), gray(caps, "│"))
}

/// `└  message`, the message as given.
pub fn close(caps: Caps, message: &str) -> String {
    format!("{}  {message}\n", gray(caps, "└"))
}

/// `└  message`, the message red: the group given up.
pub fn close_cancelled(caps: Caps, message: &str) -> String {
    close(caps, &crate::theme::danger().paint(caps, message))
}

/// A log entry: `mark  first line`, a `│` before each line after it, and an
/// empty `│  ` row below, so the next entry is spaced from it.
pub fn log(caps: Caps, mark: Mark, text: &str) -> String {
    let bar = gray(caps, "│");
    let mut out = String::new();
    let mut lines = text.lines().chain(std::iter::once(""));
    if let Some(first) = lines.next() {
        out += &format!("{}  {first}\n", mark.paint(caps));
    }
    for line in lines {
        out += &format!("{bar}  {line}\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_group_opens_and_closes_in_grey() {
        assert_eq!(
            open(Caps::PLAIN, "sealward unlock"),
            "┌  sealward unlock\n│\n"
        );
        assert_eq!(
            open(Caps::COLOR, "t"),
            "\x1b[38;5;8m┌\x1b[0m  t\n\x1b[38;5;8m│\x1b[0m\n"
        );
        assert_eq!(close(Caps::PLAIN, "Done"), "└  Done\n");
        assert_eq!(
            close_cancelled(Caps::COLOR, "Aborted."),
            "\x1b[38;5;8m└\x1b[0m  \x1b[31mAborted.\x1b[0m\n"
        );
    }

    #[test]
    fn a_log_entry_hangs_its_lines_off_the_bar() {
        assert_eq!(log(Caps::PLAIN, Mark::Step, "one"), "◇  one\n│  \n");
        assert_eq!(
            log(Caps::PLAIN, Mark::Warning, "first\nsecond"),
            "▲  first\n│  second\n│  \n"
        );
        // An empty entry is still its mark.
        assert_eq!(log(Caps::PLAIN, Mark::Info, ""), "●  \n");
    }

    #[test]
    fn each_mark_in_its_colour() {
        let c = Caps::COLOR;
        for (mark, want) in [
            (Mark::Step, "\x1b[32m◇\x1b[0m"),
            (Mark::Info, "\x1b[34m●\x1b[0m"),
            (Mark::Remark, "\x1b[38;5;8m├\x1b[0m"),
            (Mark::Success, "\x1b[32m◆\x1b[0m"),
            (Mark::Warning, "\x1b[33m▲\x1b[0m"),
            (Mark::Error, "\x1b[31m■\x1b[0m"),
        ] {
            assert_eq!(mark.paint(c), want, "{mark:?}");
        }
    }
}
