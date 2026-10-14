//! The box an interactive list draws in, inline in the flow: the browsed
//! grid ([`crate::browse`]) and the picker ([`crate::picker`]).
//!
//! ```text
//! ◆  Items ▸ [/]git ──────────── 50 of 200 match · 1–4 ─╮
//! │                                                     │
//! │ › first row                                         │
//! │   second row                                        │
//! ├─────────────────────────────────────────────────────╯
//! │
//! ◇  ↑↓ move · enter pick · esc cancel
//! ```
//!
//! The title carries the filter as a chip in the search colour, as sealward
//! browse's breadcrumb does (`/query` while typed, `[/]query` once applied),
//! and the status — counts, window, sort — at the right. The controls are a
//! step of their own below the box, not part of it. Every line of the box is
//! `inner + 6` columns.

use std::borrow::Cow;

use cliclack::{Theme, ThemeState};
use console::{measure_text_width, style};

use crate::grid::Clack;

/// Room kept beside the title for a short filter chip (` ▸ [/]query`), so
/// the box does not widen while the person types.
pub(crate) const CHIP_ROOM: usize = 14;

/// The narrowest content a box gets, however narrow the terminal.
const MIN_INNER: usize = 10;

/// The widest content that fits a terminal `cols` wide, one column spare so
/// nothing wraps.
pub(crate) fn max_inner(cols: usize) -> usize {
    cols.saturating_sub(7).max(MIN_INNER)
}

/// What the person is doing with the filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Filter {
    /// Typing it: `/query` with a cursor.
    Typing,
    /// Applied: `[/]query`.
    Applied,
}

pub(crate) struct Frame {
    inner: usize,
    state: ThemeState,
}

impl Frame {
    /// A box with `inner` columns of content: cyan while `Active`, grey once
    /// `Submit`ted.
    pub(crate) fn new(inner: usize, state: ThemeState) -> Self {
        Self { inner, state }
    }

    fn paint(&self, s: &str) -> String {
        Clack
            .bar_color(&self.state)
            .for_stderr()
            .apply_to(s)
            .to_string()
    }

    /// `◆  title ▸ /query ─── status ─╮`. A filter being typed shows its
    /// chip even while empty (`▸ /▌`), so the person sees where keys go; an
    /// applied one only when there is a query.
    pub(crate) fn head(&self, title: &str, query: &str, filter: Filter, status: &str) -> String {
        let title = cut(title, self.inner.saturating_sub(4));
        // The chip comes before the status: at least ` ▸ /…▌` (` ▸ [/]…`)
        // stays, and the status gives way first.
        let chip_min = match filter {
            Filter::Applied if query.is_empty() => 0,
            Filter::Typing => 6,
            Filter::Applied => 7,
        };
        let status = cut(
            status,
            self.inner
                .saturating_sub(measure_text_width(&title) + 3 + chip_min),
        );
        let used = measure_text_width(&title) + measure_text_width(&status);
        let room = self.inner.saturating_sub(used + 3);
        let chip = match filter {
            Filter::Applied if query.is_empty() => String::new(),
            Filter::Typing => format!(
                " {} {}{}",
                dim("▸"),
                search(&format!("/{}", cut_start(query, room.saturating_sub(5)))),
                style(" ").reverse().for_stderr()
            ),
            Filter::Applied => format!(
                " {} {}",
                dim("▸"),
                search(&format!("[/]{}", cut(query, room.saturating_sub(6))))
            ),
        };
        let rule = self
            .inner
            .saturating_sub(used + 2 + measure_text_width(&chip));
        format!(
            "{}  {}{chip} {} {} {}\n",
            Clack.state_symbol(&self.state),
            style(title).bold().for_stderr(),
            self.paint(&"─".repeat(rule.max(1))),
            dim(&status),
            self.paint("─╮"),
        )
    }

    /// `│   content │`, padded to the box.
    pub(crate) fn line(&self, content: &str) -> String {
        self.marked(" ", content)
    }

    /// `│ › content │`: a one-column `marker` (a cursor) before the content.
    pub(crate) fn marked(&self, marker: &str, content: &str) -> String {
        let bar = self.paint("│");
        let pad = self.inner.saturating_sub(measure_text_width(content));
        format!("{bar} {marker} {content}{} {bar}\n", " ".repeat(pad))
    }

    /// `├───╯` and the bar under it, like a table box's foot.
    pub(crate) fn close(&self) -> String {
        format!(
            "{}\n{}\n",
            self.paint(&format!("├{}╯", "─".repeat(self.inner + 4))),
            self.paint("│")
        )
    }

    /// `◇  hints`, dim, cut to a terminal `cols` wide.
    pub(crate) fn controls(&self, hints: &str, cols: usize) -> String {
        format!(
            "{}  {}\n",
            self.paint("◇"),
            dim(&cut(hints, cols.saturating_sub(4)))
        )
    }

    /// `▲  text` in yellow, in place of the controls: what the last key could
    /// not do.
    pub(crate) fn warning(&self, text: &str, cols: usize) -> String {
        let text = cut(text, cols.saturating_sub(4));
        format!(
            "{}  {}\n",
            style("▲").yellow().for_stderr(),
            style(text).yellow().for_stderr()
        )
    }
}

/// The muted tone ([`crate::ui::DIM_256`]), for stderr.
pub(crate) fn dim(s: &str) -> String {
    style(s)
        .color256(crate::ui::DIM_256)
        .for_stderr()
        .to_string()
}

/// The search colour (`tui::theme::SEARCH`), bold.
fn search(s: &str) -> String {
    style(s).yellow().bold().for_stderr().to_string()
}

/// `s` padded to `w` columns, on the right or the left.
pub(crate) fn align(s: &str, w: usize, right: bool) -> String {
    let pad = " ".repeat(w.saturating_sub(measure_text_width(s)));
    if right {
        format!("{pad}{s}")
    } else {
        format!("{s}{pad}")
    }
}

/// `s` if it fits in `width` columns, else as much as fits followed by `…`.
pub(crate) fn cut(s: &str, width: usize) -> Cow<'_, str> {
    if measure_text_width(s) <= width {
        return s.into();
    }
    if width == 0 {
        return "".into();
    }
    console::truncate_str(s, width, "…")
}

/// The end of `s` when it does not fit: a query being typed shows its tail.
pub(crate) fn cut_start(s: &str, width: usize) -> String {
    if measure_text_width(s) <= width {
        return s.to_string();
    }
    let mut tail: Vec<char> = Vec::new();
    let mut used = 1;
    for c in s.chars().rev() {
        let w = measure_text_width(c.encode_utf8(&mut [0; 4]));
        if used + w > width {
            break;
        }
        used += w;
        tail.push(c);
    }
    std::iter::once('…').chain(tail.into_iter().rev()).collect()
}

/// `text` with the chars at `hits` (char offsets from `start`) underlined in
/// cyan — the filter's matches — over `base`; runs are styled once each.
pub(crate) fn paint_hits(text: &str, start: u32, hits: &[u32], base: &console::Style) -> String {
    let base = base.clone().for_stderr();
    let mut out = String::new();
    let mut run = String::new();
    let mut in_hit = false;
    let mut flush = |run: &mut String, hit: bool| {
        if run.is_empty() {
            return;
        }
        let s = base.apply_to(run.as_str());
        out += &if hit { s.cyan().underlined() } else { s }.to_string();
        run.clear();
    };
    for (k, ch) in text.chars().enumerate() {
        let hit = hits.contains(&(start + k as u32));
        if hit != in_hit {
            flush(&mut run, in_hit);
            in_hit = hit;
        }
        run.push(ch);
    }
    flush(&mut run, in_hit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> std::sync::MutexGuard<'static, ()> {
        let g = crate::ui::tests::lock();
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
        g
    }

    #[test]
    fn every_line_of_the_box_is_inner_plus_six() {
        let _c = plain();
        let f = Frame::new(30, ThemeState::Active);
        for head in [
            f.head("Items", "", Filter::Applied, "1–4 of 9"),
            f.head("Items", "git", Filter::Typing, "1 of 9 match · 1–1"),
            f.head("Items", "git", Filter::Applied, "1 of 9 match · 1–1"),
            f.head("Items", &"x".repeat(80), Filter::Typing, "1–4 of 9"),
        ] {
            assert_eq!(measure_text_width(head.trim_end()), 36, "{head}");
        }
        assert_eq!(measure_text_width(f.line("abc").trim_end()), 36);
        assert_eq!(measure_text_width(f.marked("›", "abc").trim_end()), 36);
        assert_eq!(measure_text_width(f.close().lines().next().unwrap()), 36);
    }

    #[test]
    fn the_chip_says_whether_the_filter_is_typed_or_applied() {
        let _c = plain();
        let f = Frame::new(40, ThemeState::Active);
        assert!(f.head("T", "git", Filter::Typing, "").contains("T ▸ /git"));
        assert!(
            f.head("T", "git", Filter::Applied, "")
                .contains("T ▸ [/]git ")
        );
        assert!(f.head("T", "", Filter::Typing, "").contains("T ▸ /"));
        assert!(!f.head("T", "", Filter::Applied, "").contains('▸'));
        // A long query being typed shows its tail.
        let long = f.head(
            "T",
            "abcdefghijklmnopqrstuvwxyz",
            Filter::Typing,
            "1–1 of 1",
        );
        assert!(long.contains("/…") && long.contains("xyz"), "{long}");
    }

    #[test]
    fn cut_and_cut_start() {
        assert_eq!(cut("abc", 3), "abc");
        assert_eq!(cut("abcd", 3), "ab…");
        assert_eq!(cut("abcd", 0), "");
        assert_eq!(cut_start("abcdef", 4), "…def");
        assert_eq!(cut_start("abc", 4), "abc");
    }
}
