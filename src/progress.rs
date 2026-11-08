//! Progress as strings: a bar of coloured parts, and the flow's progress
//! line around it. Pure, like [`crate::spin`]: whoever draws it decides when.
//!
//! ```text
//! ━━━━━━━━━━━━━━━━━━━━━━━━──────────────── ← bar: the parts, then the rest
//! ◒  Copying files  ━━━━━━━━━━──────  42/100 ← line: spinner, message, bar, tail
//! ```
//!
//! A bar is one or more parts of a whole, each in its own [`Style`]: a
//! single part for "this much is done", several for a batch whose items end
//! in different ways (done, failed, running). The rest of the whole is the
//! muted `─`. A part that is not zero always shows, at least one cell, so a
//! single failure among thousands is never rounded away.
//!
//! The live flow animates the line ([`crate::Flow::progress`]); an app that
//! draws a panel of its own puts the bar in it ([`crate::Flow::live`]).

use crate::spin::Spin;
use crate::style::{self, Caps, Style};

/// One part of a bar's whole: how many, and the colour it is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub n: u64,
    pub style: Style,
}

impl Part {
    pub const fn new(n: u64, style: Style) -> Part {
        Part { n, style }
    }
}

/// The colour of the flow's single-part bar: the theme's
/// [`active`](crate::theme::active), as the spinner and the open `◆`.
pub fn done() -> Style {
    crate::theme::active()
}

/// `━` cells for each part in its style, then `─` muted for the rest of
/// `total`, `width` cells in all. A total smaller than the parts' sum is
/// taken as that sum, so the bar never overflows.
pub fn bar(caps: Caps, parts: &[Part], total: u64, width: usize) -> String {
    let cells = cells(parts, total, width);
    let mut out = String::new();
    for (part, &n) in parts.iter().zip(&cells) {
        if n > 0 {
            out += &part.style.paint(caps, "━".repeat(n));
        }
    }
    let rest = width.saturating_sub(cells.iter().sum());
    if rest > 0 {
        out += &crate::theme::muted().paint(caps, "─".repeat(rest));
    }
    out
}

/// How many cells each part gets. Edges are rounded where the running sum
/// falls, so the cells add up to the parts' share of the width exactly;
/// then a part rounded to nothing takes a cell, from the rest if there is
/// any left, else from the widest part.
fn cells(parts: &[Part], total: u64, width: usize) -> Vec<usize> {
    let sum: u64 = parts.iter().map(|p| p.n).sum();
    let total = u128::from(total.max(sum).max(1));
    let w = width as u128;
    let mut out = Vec::with_capacity(parts.len());
    let (mut run, mut prev) = (0u128, 0usize);
    for p in parts {
        run += u128::from(p.n);
        let edge = ((run * w + total / 2) / total) as usize;
        out.push(edge - prev);
        prev = edge;
    }
    for i in 0..parts.len() {
        if parts[i].n == 0 || out[i] > 0 {
            continue;
        }
        if out.iter().sum::<usize>() < width {
            out[i] = 1;
        } else if let Some(j) = (0..out.len())
            .filter(|&j| out[j] > 1)
            .max_by_key(|&j| out[j])
        {
            out[j] -= 1;
            out[i] = 1;
        }
    }
    out
}

/// `42/100`: where a single count is.
pub fn count(pos: u64, len: u64) -> String {
    format!("{}/{len}", pos.min(len))
}

/// `42%`, rounded down, so it reads 100% only when it is.
pub fn percent(pos: u64, len: u64) -> String {
    let pct = u128::from(pos.min(len)) * 100 / u128::from(len.max(1));
    format!("{pct}%")
}

/// The widest a line's bar grows: past this it says no more.
const BAR_MAX: usize = 40;
/// The narrowest bar worth drawing; below it the message gives way.
const BAR_MIN: usize = 10;

/// `◒  msg  ━━━━━━────  tail`, cut to `cols` so it never wraps: the
/// spinner's symbol at this frame, the message muted, the bar as wide as is
/// left (up to [`BAR_MAX`]), then `tail` ([`count`], [`percent`], or the
/// caller's own). With no parts and no total it is the spinner's line.
///
/// A message too long for the line is cut before the bar is: the bar is
/// what moves.
pub fn line(
    caps: Caps,
    spin: &Spin,
    msg: &str,
    parts: &[Part],
    total: u64,
    tail: &str,
    cols: usize,
) -> String {
    let symbol = spin.symbol(caps);
    if total == 0 && parts.iter().all(|p| p.n == 0) {
        return spin.line(caps, msg, cols);
    }
    // `◒  ` and one column spare, so the last one is never written.
    let room = cols.saturating_sub(4);
    let tail_w = style::measure(tail);
    let fixed = 2 + tail_w + if tail_w > 0 { 2 } else { 0 };
    let msg_w = style::measure(msg);
    let bar_w = room
        .saturating_sub(fixed + msg_w)
        .clamp(BAR_MIN.min(room.saturating_sub(fixed)), BAR_MAX);
    let msg = style::truncate(msg, room.saturating_sub(fixed + bar_w), "…");
    let mut out = format!(
        "{symbol}  {}  {}",
        crate::glyph::dim(caps, &msg),
        bar(caps, parts, total, bar_w)
    );
    if tail_w > 0 {
        out += "  ";
        out += tail;
    }
    style::truncate(&out, cols.saturating_sub(1), "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(parts: &[Part], total: u64, width: usize) -> String {
        bar(Caps::PLAIN, parts, total, width)
    }

    fn one(n: u64) -> [Part; 1] {
        [Part::new(n, done())]
    }

    #[test]
    fn a_single_part_fills_its_share() {
        assert_eq!(plain(&one(0), 100, 10), "──────────");
        assert_eq!(plain(&one(50), 100, 10), "━━━━━─────");
        assert_eq!(plain(&one(100), 100, 10), "━━━━━━━━━━");
        assert_eq!(plain(&one(250), 100, 10), "━━━━━━━━━━", "never overflows");
        assert_eq!(plain(&one(3), 0, 4), "━━━━", "no total: the parts are all");
        assert_eq!(plain(&one(1), 1, 0), "", "no room");
    }

    #[test]
    fn parts_add_up_to_their_share_exactly() {
        let parts = [
            Part::new(1, done()),
            Part::new(1, done()),
            Part::new(1, done()),
        ];
        // 1/3 each of 10: rounded per part it would be 3+3+3 = 9 or 4+4+4.
        assert_eq!(cells(&parts, 3, 10).iter().sum::<usize>(), 10);
        assert_eq!(cells(&parts, 6, 10).iter().sum::<usize>(), 5);
    }

    #[test]
    fn a_part_that_is_not_zero_always_shows() {
        let red = Style::new().red();
        // 1 failure in 1308: rounds to nothing, takes a cell from the rest.
        let parts = [Part::new(412, done()), Part::new(1, red)];
        assert_eq!(cells(&parts, 1308, 40), vec![13, 1]);
        // Nothing left over: it takes one from the widest part.
        let parts = [Part::new(999, done()), Part::new(1, red)];
        assert_eq!(cells(&parts, 1000, 20), vec![19, 1]);
        // A zero part stays empty.
        let parts = [Part::new(5, done()), Part::new(0, red)];
        assert_eq!(cells(&parts, 10, 10), vec![5, 0]);
    }

    #[test]
    fn colours_each_part_and_mutes_the_rest() {
        let parts = [
            Part::new(1, Style::new().green()),
            Part::new(1, Style::new().red()),
        ];
        assert_eq!(
            bar(Caps::COLOR, &parts, 4, 4),
            "\x1b[32m━\x1b[0m\x1b[31m━\x1b[0m\x1b[38;5;246m──\x1b[0m"
        );
    }

    #[test]
    fn tails() {
        assert_eq!(count(42, 100), "42/100");
        assert_eq!(count(120, 100), "100/100");
        assert_eq!(percent(999, 1000), "99%");
        assert_eq!(percent(1000, 1000), "100%");
        assert_eq!(percent(0, 0), "0%");
    }

    fn spin() -> Spin {
        let mut s = Spin::new();
        s.fix(0);
        s
    }

    #[test]
    fn the_line_never_wraps() {
        let s = spin();
        for cols in [0, 5, 12, 20, 40, 80, 200] {
            let l = line(
                Caps::PLAIN,
                &s,
                "Copying files",
                &one(42),
                100,
                "42/100",
                cols,
            );
            assert!(
                style::measure(&l) < cols.max(1),
                "{cols}: {l:?} is {} wide",
                style::measure(&l)
            );
        }
    }

    #[test]
    fn the_message_gives_way_before_the_bar() {
        let s = spin();
        let msg = "Copying a great many files from one place to another";
        let l = line(Caps::PLAIN, &s, msg, &one(50), 100, "50/100", 40);
        assert!(l.contains("━━━━━─────"), "{l}");
        assert!(l.contains('…'), "{l}");
        assert!(l.ends_with("  50/100"), "{l}");
    }

    #[test]
    fn the_bar_stops_growing() {
        let s = spin();
        let l = line(Caps::PLAIN, &s, "x", &one(1), 2, "", 200);
        assert_eq!(l.matches(['━', '─']).count(), BAR_MAX);
    }

    #[test]
    fn nothing_to_measure_is_the_spinners_line() {
        let s = spin();
        assert_eq!(
            line(Caps::PLAIN, &s, "Connecting", &[], 0, "", 80),
            s.line(Caps::PLAIN, "Connecting", 80)
        );
    }
}
