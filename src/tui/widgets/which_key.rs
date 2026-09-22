//! Which-key.nvim-style popup line that shows the current chord prefix and
//! the available continuations after it.
//!
//! Layout (one line, **right-aligned** so it sits in the same place as
//! `footer_hints::render_right`):
//!
//! ```text
//!                          c…  u url  p password  y username  t totp
//! ```
//!
//! The prefix (`c…`) is styled with [`theme::FG_PRIMARY`]; each `(key, label)`
//! pair has the key in [`theme::FG_SECONDARY`] and the label in
//! [`theme::FG_DIM`]. Pairs that don't fit are dropped from the **left**
//! (closest to the prefix is dropped first), so the rightmost pairs remain
//! visible — keeps the popup in a stable position relative to the hint row.
//!
//! Construction is wired through [`super::super::chord`]: the consumer feeds
//! `resolver.pending()` and `resolver.continuations(scope)` directly.

use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};

use crate::tui::chord::{chord_label, key_label};
use crate::tui::theme;

/// Render the which-key line at row `y` right-aligned within
/// `[x, x + width)`.
///
/// `pending` is the chord prefix already entered (typically
/// `resolver.pending()`); `continuations` is the list of valid next-key
/// labels (typically `resolver.continuations(&scope)`).
pub fn render(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    pending: &[KeyEvent],
    continuations: &[(KeyEvent, &str)],
) {
    if width == 0 || pending.is_empty() {
        return;
    }

    let prefix_style = Style::default()
        .fg(theme::FG_PRIMARY)
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(theme::FG_SECONDARY);
    let label_style = Style::default().fg(theme::FG_DIM);

    let prefix = format!("{}…", chord_label(pending));
    let prefix_w = unicode_width::UnicodeWidthStr::width(prefix.as_str()) as u16;
    if prefix_w >= width {
        return;
    }

    // Cache widths and decide which pairs fit. Drop from the left (i.e.
    // pairs closest to the prefix) when space is tight, so the rightmost
    // pair stays anchored.
    let widths: Vec<(u16, u16)> = continuations
        .iter()
        .map(|(k, lab)| {
            let kw = unicode_width::UnicodeWidthStr::width(key_label(k).as_str()) as u16;
            let lw = unicode_width::UnicodeWidthStr::width(*lab) as u16;
            (kw, lw)
        })
        .collect();

    // Trailing-space buffer to keep the popup off the right edge.
    let trailing: u16 = 1;
    // Each pair occupies "  key SPACE label" = 2 + kw + 1 + lw.
    let mut start_idx = 0usize;
    let mut total_w = prefix_w + trailing;
    for (kw, lw) in &widths {
        total_w = total_w.saturating_add(2 + kw + 1 + lw);
    }
    while start_idx < continuations.len() && total_w > width {
        let (kw, lw) = widths[start_idx];
        total_w = total_w.saturating_sub(2 + kw + 1 + lw);
        start_idx += 1;
    }
    if total_w > width {
        // Not enough room for prefix + trailing buffer + at least one pair.
        // Render just the prefix at the right edge (`prefix_w < width` is
        // guaranteed by the early return above).
        let fx = x + width.saturating_sub(prefix_w);
        buf.set_string(fx, y, &prefix, prefix_style);
        return;
    }

    let mut fx = x + width.saturating_sub(total_w);
    buf.set_string(fx, y, &prefix, prefix_style);
    fx += prefix_w;
    for (i, (k, lab)) in continuations.iter().enumerate().skip(start_idx) {
        let (kw, _lw) = widths[i];
        fx += 2;
        let key_str = key_label(k);
        buf.set_string(fx, y, &key_str, key_style);
        fx += kw;
        buf.set_string(fx, y, " ", label_style);
        fx += 1;
        buf.set_string(fx, y, *lab, label_style);
        fx += widths[i].1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::chord::key_char;
    use ratatui::layout::{Position, Rect};

    fn render_to_string(width: u16, pending: &[KeyEvent], conts: &[(KeyEvent, &str)]) -> String {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        render(&mut buf, 0, 0, width, pending, conts);
        let mut out = String::new();
        for cx in 0..width {
            let cell = buf.cell(Position::new(cx, 0)).unwrap();
            out.push_str(cell.symbol());
        }
        out.trim_end().to_string()
    }

    #[test]
    fn empty_pending_renders_nothing() {
        let s = render_to_string(40, &[], &[(key_char('u'), "url")]);
        assert_eq!(s, "");
    }

    #[test]
    fn renders_prefix_and_continuations_right_aligned() {
        let s = render_to_string(
            40,
            &[key_char('c')],
            &[(key_char('u'), "url"), (key_char('p'), "password")],
        );
        // Right-aligned: the line should *end* with the rightmost pair
        // (followed by the trailing space buffer), not start with the prefix.
        assert!(s.contains("c\u{2026}"));
        assert!(s.contains("u url"));
        assert!(s.contains("p password"));
        assert!(s.starts_with(' '), "expected left padding, got {s:?}");
    }

    #[test]
    fn drops_pairs_from_the_left_when_tight() {
        // Tight width: not enough for both pairs. The popup keeps the
        // rightmost pair (`p password`) anchored and drops `u url`.
        let s = render_to_string(
            18,
            &[key_char('c')],
            &[(key_char('u'), "url"), (key_char('p'), "password")],
        );
        assert!(s.contains("p password"), "got {s:?}");
        assert!(!s.contains("u url"), "got {s:?}");
        assert!(s.contains("c\u{2026}"));
    }
}
