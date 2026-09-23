//! Right-aligned key-hint row for inline TUI footers.
//!
//! One flat style, shared with the chord which-key popup
//! ([`super::which_key`]): an optional bold **accent** group title, then
//! [`Hint`]s with the key in [`theme::ACCENT`], the label in [`theme::FG_DIM`],
//! separated by a dim ` · `.
//!
//! A hint declares *what it is*, not how it's drawn: [`Hint::action`] for a
//! direct action, [`Hint::chord`] for a key that opens a submenu. Chord hints
//! get the [`CHORD_INDICATOR`] glyph appended to their key automatically, so
//! the indicator lives in one place instead of being baked into each label.
//!
//! Example: `Copy   u url · c› copy · q back`
//!
//! The whole row is right-aligned; when it doesn't fit, whole hints are dropped
//! from the **left** (closest to the title) so the rightmost hints stay
//! anchored.

use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::tui::theme;

/// Gap (columns) between the title and the first hint.
const TITLE_GAP: u16 = 3;
/// The ` · ` separator drawn between adjacent hints.
const SEP: &str = " · ";
/// Keep the row this many columns off the right edge.
const TRAILING: u16 = 1;
/// Glyph appended to a chord hint's key to signal "opens a submenu".
pub const CHORD_INDICATOR: &str = "\u{203A}"; // ›

/// A single footer hint: a key and what it does.
///
/// Use [`Hint::action`] for a direct action and [`Hint::chord`] for a key that
/// opens a chord submenu — the latter renders with the [`CHORD_INDICATOR`]
/// appended to its key.
#[derive(Clone, Copy)]
pub struct Hint<'a> {
    pub key: &'a str,
    pub label: &'a str,
    pub chord: bool,
}

impl<'a> Hint<'a> {
    /// A direct action hint (no submenu indicator).
    pub const fn action(key: &'a str, label: &'a str) -> Self {
        Self {
            key,
            label,
            chord: false,
        }
    }

    /// A chord-leader hint: its key opens a submenu, so it renders with the
    /// [`CHORD_INDICATOR`] glyph appended.
    pub const fn chord(key: &'a str, label: &'a str) -> Self {
        Self {
            key,
            label,
            chord: true,
        }
    }
}

#[inline]
fn w(s: &str) -> u16 {
    UnicodeWidthStr::width(s) as u16
}

/// Render `hints` right-aligned within `[x, x + width)`, dropping hints from
/// the left when they don't fit.
pub fn render_right(buf: &mut Buffer, x: u16, y: u16, width: u16, hints: &[Hint]) {
    render_hint_row(buf, x, y, width, None, hints);
}

/// Shared hint-row renderer: an optional bold accent `title`, then the `hints`
/// in the unified flat style (accent keys, dim labels, ` · ` separators),
/// right-aligned with left-drop overflow.
///
/// This is the single source of truth for footer hints and the chord which-key
/// popup — see [`super::which_key::render`], which delegates here.
pub(crate) fn render_hint_row(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    title: Option<&str>,
    hints: &[Hint],
) {
    if width == 0 || (title.is_none() && hints.is_empty()) {
        return;
    }

    let title_style = Style::default()
        .fg(theme::SEARCH)
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(theme::ACCENT);
    let label_style = Style::default().fg(theme::FG_DIM);

    let has_title = title.is_some();
    let header = title.unwrap_or("");
    let header_w = w(header);
    if has_title && header_w >= width {
        return;
    }

    let indicator_w = w(CHORD_INDICATOR);
    // Displayed key width includes the chord indicator when present.
    let key_w = |h: &Hint| w(h.key) + if h.chord { indicator_w } else { 0 };

    // Per-hint geometry: key(+indicator) + single space + label.
    let n = hints.len();
    let hint_w: Vec<u16> = hints.iter().map(|h| key_w(h) + 1 + w(h.label)).collect();
    let sep_w = w(SEP);

    // Total width for a given left-drop start index.
    let total = |start: usize| -> u16 {
        let shown = n - start;
        let mut t = header_w + TRAILING;
        if shown > 0 {
            if has_title {
                t = t.saturating_add(TITLE_GAP);
            }
            for hw in hint_w.iter().skip(start) {
                t = t.saturating_add(*hw);
            }
            t = t.saturating_add(sep_w.saturating_mul(shown as u16 - 1));
        }
        t
    };

    let mut start = 0usize;
    while start < n && total(start) > width {
        start += 1;
    }

    let final_total = total(start);
    if final_total > width {
        // Only the title can be shown (or nothing).
        if has_title {
            let fx = x + width.saturating_sub(header_w);
            buf.set_string(fx, y, header, title_style);
        }
        return;
    }

    let mut fx = x + width.saturating_sub(final_total);
    if has_title {
        buf.set_string(fx, y, header, title_style);
        fx += header_w;
        if start < n {
            fx += TITLE_GAP;
        }
    }

    for (i, h) in hints.iter().enumerate().skip(start) {
        if i > start {
            buf.set_string(fx, y, SEP, label_style);
            fx += sep_w;
        }
        buf.set_string(fx, y, h.key, key_style);
        fx += w(h.key);
        if h.chord {
            buf.set_string(fx, y, CHORD_INDICATOR, key_style);
            fx += indicator_w;
        }
        buf.set_string(fx, y, " ", label_style);
        fx += 1;
        buf.set_string(fx, y, h.label, label_style);
        fx += w(h.label);
    }
}
