//! Horizontal rule with optional left content and right-aligned segments.
//!
//! Simple form — a single right-aligned dim label:
//! `────────────────────────── 3/12 ──`
//!
//! Rich form — a styled left segment plus one or more independently-styled
//! right segments:
//! ` Vaults ▸ All ▸ GitHub ───────── /foo  3/12 `

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};
use unicode_width::UnicodeWidthStr;

use crate::tui::{paint, theme};

/// One piece of text to paint onto the rule, with its own style.
pub struct Segment<'a> {
    pub text: &'a str,
    pub style: Style,
}

impl<'a> Segment<'a> {
    /// Segment painted in the default dim rule style.
    pub fn dim(text: &'a str) -> Self {
        Self {
            text,
            style: theme::dim(),
        }
    }

    /// Segment painted with a caller-supplied style.
    pub fn styled(text: &'a str, style: Style) -> Self {
        Self { text, style }
    }
}

/// Builder for a horizontal rule with optional content floating on it.
#[derive(Default)]
pub struct StatusRule<'a> {
    left: Option<Segment<'a>>,
    right: Vec<Segment<'a>>,
}

impl<'a> StatusRule<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the left segment (overwrites any previous left).
    pub fn left(mut self, seg: Segment<'a>) -> Self {
        self.left = Some(seg);
        self
    }

    /// Append a right segment. Right segments are painted in the order
    /// they were pushed, separated by two spaces.
    pub fn push_right(mut self, seg: Segment<'a>) -> Self {
        self.right.push(seg);
        self
    }

    pub fn render(&self, buf: &mut Buffer, x: u16, y: u16, width: u16) {
        if width == 0 {
            return;
        }

        let hr_style = Style::default().fg(Color::DarkGray);
        let pad_style = theme::dim();

        // Base rule fill.
        let rule: String = "\u{2500}".repeat(width as usize);
        buf.set_string(x, y, &rule, hr_style);

        // -- Left segment -------------------------------------------------
        if let Some(seg) = self.left.as_ref() {
            // Leading padding space.
            buf.set_string(x, y, " ", pad_style);
            let crumb_x = x + 1;
            let crumb_w = width.saturating_sub(2);
            paint::truncated_str(buf, crumb_x, y, seg.text, crumb_w, seg.style);
            // Trailing padding space — positioned by *visual* width, not
            // the byte length that `truncated_str` returns. Breadcrumbs use
            // multi-byte glyphs like `▸` and would otherwise leave a gap.
            let visual = (UnicodeWidthStr::width(seg.text) as u16).min(crumb_w);
            if crumb_x + visual < x + width {
                buf.set_string(crumb_x + visual, y, " ", pad_style);
            }
        }

        // -- Right segments ----------------------------------------------
        if self.right.is_empty() {
            return;
        }

        let seg_widths: Vec<u16> = self
            .right
            .iter()
            .map(|s| UnicodeWidthStr::width(s.text) as u16)
            .collect();
        let sep_w: u16 = 2 * (self.right.len().saturating_sub(1)) as u16;
        let inner: u16 = seg_widths.iter().sum::<u16>() + sep_w;
        let total_w: u16 = inner + 2; // 1 leading + 1 trailing pad

        if total_w >= width {
            return;
        }

        let mut rx = x + width - total_w;
        buf.set_string(rx, y, " ", pad_style);
        rx += 1;
        for (i, seg) in self.right.iter().enumerate() {
            if i > 0 {
                buf.set_string(rx, y, "  ", pad_style);
                rx += 2;
            }
            buf.set_string(rx, y, seg.text, seg.style);
            rx += seg_widths[i];
        }
        buf.set_string(rx, y, " ", pad_style);
    }
}

/// Render a horizontal rule across `width`, with `label` right-aligned
/// over the rule. Pass `""` for a plain rule. Thin shim over
/// [`StatusRule`] for the common single-label case.
pub fn render(buf: &mut Buffer, x: u16, y: u16, width: u16, label: &str) {
    let mut rule = StatusRule::new();
    if !label.is_empty() {
        rule = rule.push_right(Segment::dim(label));
    }
    rule.render(buf, x, y, width);
}
