//! Draw text large, out of block characters.
//!
//! A 5x7 bitmap font (see [`font`]) painted into a [`Buffer`] one character at
//! a time, each with its own [`Style`]. Per-character styling is the seam that
//! keeps *policy* out of this module: it can colour a digit differently from a
//! letter because the caller says so, never because it decided digits matter.
//!
//! Terminal cells are roughly twice as tall as they are wide, so neither scale
//! maps one bitmap pixel to one cell — that would render everything at 1:2 and
//! look stretched. [`Scale::Half`] packs two pixel rows into one cell with
//! `▀▄█`; [`Scale::Full`] instead widens each pixel to two cells. Both come out
//! at the bitmap's true proportions, with `Full` twice the size of `Half`.
//!
//! Follows the free-function convention the rest of `tui::widgets` uses: paint
//! into a `Buffer` at a position, no `Widget` impls, no retained state.

mod font;

use ratatui::buffer::Buffer;
use ratatui::style::Style;

use crate::tui::paint;

/// Glyph width, in bitmap pixels.
pub const GLYPH_W: usize = 5;
/// Glyph height, in bitmap pixels.
pub const GLYPH_H: usize = 7;
/// Blank pixel columns between adjacent glyphs.
const GAP_PX: usize = 1;

/// A space drawn as a *visible* character (U+2423 OPEN BOX), for callers that
/// must show one rather than leave a gap.
///
/// `' '` itself renders blank, which is right for ordinary text and wrong the
/// moment the text is something being transcribed: a blank is indistinguishable
/// from the gap between glyphs, so a space in the middle of a password becomes
/// invisible. Substituting this for `' '` is the caller's decision — the widget
/// does not know whether it is drawing a headline or a secret.
///
/// Shaped unlike `_`, which is a character a password can actually contain.
pub const VISIBLE_SPACE: char = '\u{2423}';

/// How large a bitmap pixel is drawn.
///
/// Both variants preserve the glyph's proportions on a terminal whose cells are
/// about twice as tall as they are wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scale {
    /// One cell wide, half a cell tall. A glyph is 5x4 cells.
    #[default]
    Half,
    /// Two cells wide, one cell tall. A glyph is 10x7 cells — twice [`Scale::Half`].
    Full,
}

impl Scale {
    /// Cells one bitmap pixel occupies horizontally.
    const fn px_w(self) -> usize {
        match self {
            Self::Half => 1,
            Self::Full => 2,
        }
    }

    /// Cell rows one glyph occupies.
    pub const fn height(self) -> u16 {
        match self {
            // Two pixel rows per cell, rounded up: the last cell of an
            // odd-height glyph carries only its upper half.
            Self::Half => GLYPH_H.div_ceil(2) as u16,
            Self::Full => GLYPH_H as u16,
        }
    }

    /// Cell columns from one glyph's left edge to the next one's, gap included.
    pub const fn advance(self) -> u16 {
        ((GLYPH_W + GAP_PX) * self.px_w()) as u16
    }

    /// Cell columns one glyph occupies, gap excluded.
    pub const fn width(self) -> u16 {
        (GLYPH_W * self.px_w()) as u16
    }

    /// The next size down, or `None` at the smallest. Lets a caller shrink to
    /// fit without knowing the ladder.
    pub const fn smaller(self) -> Option<Self> {
        match self {
            Self::Full => Some(Self::Half),
            Self::Half => None,
        }
    }
}

/// The bitmap for `ch`: one row per entry, the low [`GLYPH_W`] bits used,
/// most-significant bit leftmost. `None` when the font has no glyph.
///
/// [`render_row`] does not go through this — it reads the art directly — but it
/// is the useful shape for a caller measuring or transforming glyphs itself.
pub fn glyph(ch: char) -> Option<[u8; GLYPH_H]> {
    let art = font::art(ch)?;
    let bytes = art.as_bytes();
    let mut rows = [0u8; GLYPH_H];
    for (r, row) in rows.iter_mut().enumerate() {
        for col in 0..GLYPH_W {
            if bytes[r * font::ROW_STRIDE + col] == b'#' {
                *row |= 1 << (GLYPH_W - 1 - col);
            }
        }
    }
    Some(rows)
}

/// Whether the font can draw `ch` as itself. A `false` here still renders — as
/// the unknown-character placeholder — it just does not render as `ch`.
pub fn is_supported(ch: char) -> bool {
    font::art(ch).is_some()
}

/// Cell extent `(width, height)` of `len` glyphs at `scale`, trailing gap
/// excluded. Zero-width for an empty run.
pub fn measure(len: usize, scale: Scale) -> (u16, u16) {
    if len == 0 {
        return (0, 0);
    }
    let w = (len as u32 * scale.advance() as u32) - (GAP_PX * scale.px_w()) as u32;
    (w.min(u16::MAX as u32) as u16, scale.height())
}

/// How many glyphs fit in `width` cells at `scale`. The trailing gap is not
/// required, so this is one more than a naive division at exact multiples.
pub fn fits(width: u16, scale: Scale) -> usize {
    let gap = (GAP_PX * scale.px_w()) as u32;
    let advance = scale.advance() as u32;
    // Solve `n * advance - gap <= width` for n.
    ((width as u32 + gap) / advance) as usize
}

/// Paint one row of glyphs left to right from `(x, y)`, each in its own style.
///
/// Characters outside the font render as a hatched placeholder rather than
/// being skipped, so the painted run always has exactly `chars.len()` glyphs —
/// text must never come out shorter than it is.
///
/// Everything outside `buf` is clipped: painting is bounds-checked per cell, so
/// an over-wide run is truncated rather than wrapped or panicking.
pub fn render_row(buf: &mut Buffer, x: u16, y: u16, chars: &[(char, Style)], scale: Scale) {
    for (i, (ch, style)) in chars.iter().enumerate() {
        let Some(origin) = (x as usize).checked_add(i * scale.advance() as usize) else {
            return;
        };
        if origin > u16::MAX as usize {
            return;
        }
        render_glyph(buf, origin as u16, y, *ch, *style, scale);
    }
}

/// Paint a single glyph with its top-left pixel at `(x, y)`.
pub fn render_glyph(buf: &mut Buffer, x: u16, y: u16, ch: char, style: Style, scale: Scale) {
    let art = font::art(ch).unwrap_or(font::UNKNOWN).as_bytes();
    let px = |row: usize, col: usize| -> bool {
        row < GLYPH_H && art[row * font::ROW_STRIDE + col] == b'#'
    };

    match scale {
        Scale::Half => {
            for cy in 0..scale.height() as usize {
                for col in 0..GLYPH_W {
                    // Upper and lower halves of this cell are two adjacent
                    // bitmap rows; the lower one may fall off an odd-height
                    // glyph, which simply leaves that half clear.
                    let sym = match (px(cy * 2, col), px(cy * 2 + 1, col)) {
                        (true, true) => '\u{2588}',  // █
                        (true, false) => '\u{2580}', // ▀
                        (false, true) => '\u{2584}', // ▄
                        (false, false) => continue,
                    };
                    let (Some(cx), Some(cy)) =
                        (x.checked_add(col as u16), y.checked_add(cy as u16))
                    else {
                        continue;
                    };
                    paint::set_cell(buf, cx, cy, sym, style);
                }
            }
        }
        Scale::Full => {
            for row in 0..GLYPH_H {
                for col in 0..GLYPH_W {
                    if !px(row, col) {
                        continue;
                    }
                    // One pixel is two cells wide at this scale.
                    for half in 0..scale.px_w() as u16 {
                        let (Some(cx), Some(cy)) = (
                            x.checked_add(col as u16 * 2 + half),
                            y.checked_add(row as u16),
                        ) else {
                            continue;
                        };
                        paint::set_cell(buf, cx, cy, '\u{2588}', style);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::{Position, Rect};

    const SCALES: [Scale; 2] = [Scale::Half, Scale::Full];

    fn styled(text: &str) -> Vec<(char, Style)> {
        text.chars().map(|c| (c, Style::default())).collect()
    }

    /// Bounding box of painted (non-blank) cells, as `(x0, y0, x1, y1)`
    /// half-open on the far edge.
    fn painted_bbox(buf: &Buffer, area: Rect) -> Option<(u16, u16, u16, u16)> {
        let mut bbox: Option<(u16, u16, u16, u16)> = None;
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                let sym = buf.cell(Position::new(x, y)).unwrap().symbol();
                if sym == " " || sym.is_empty() {
                    continue;
                }
                bbox = Some(match bbox {
                    None => (x, y, x + 1, y + 1),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
                });
            }
        }
        bbox
    }

    #[test]
    fn glyph_packs_the_art_msb_left() {
        // `T` is a full top bar over a centred stem — unambiguous in binary.
        let t = glyph('T').unwrap();
        assert_eq!(t[0], 0b11111);
        for row in &t[1..] {
            assert_eq!(*row, 0b00100);
        }
    }

    #[test]
    fn glyph_is_none_outside_the_font() {
        assert!(glyph('é').is_none());
        assert!(!is_supported('é'));
        assert!(is_supported('~'));
    }

    /// `measure` is what callers lay out against; if it disagrees with what is
    /// actually painted, every panel built on it is misaligned.
    #[test]
    fn measure_matches_the_painted_extent() {
        for scale in SCALES {
            for text in ["A", "Hg", "Tr0ub4dor&3"] {
                let (w, h) = measure(text.chars().count(), scale);
                let area = Rect::new(0, 0, w + 20, h + 10);
                let mut buf = Buffer::empty(area);
                render_row(&mut buf, 0, 0, &styled(text), scale);

                let (x0, y0, x1, y1) = painted_bbox(&buf, area).expect("something was painted");
                assert_eq!(x0, 0, "{text:?} @ {scale:?} does not start at the origin");
                assert_eq!(y0, 0, "{text:?} @ {scale:?} does not start at the origin");
                assert!(
                    x1 <= w,
                    "{text:?} @ {scale:?} painted {x1} cols, measured {w}"
                );
                assert!(
                    y1 <= h,
                    "{text:?} @ {scale:?} painted {y1} rows, measured {h}"
                );
            }
        }
    }

    /// Full is twice Half — that is what makes it a scale rather than a second
    /// font. Exactly twice across, and twice down to within the half-cell that
    /// `Half` rounds up: 7 pixel rows occupy 4 cells, not 3.5.
    #[test]
    fn full_is_twice_half() {
        let (hw, hh) = measure(6, Scale::Half);
        let (fw, fh) = measure(6, Scale::Full);
        assert_eq!(fw, hw * 2, "width must double exactly");
        assert_eq!(
            hh,
            fh.div_ceil(2),
            "Half is Full's rows packed two per cell"
        );
        assert!(fh == hh * 2 || fh == hh * 2 - 1, "fh {fh} vs hh {hh}");
    }

    #[test]
    fn fits_is_the_inverse_of_measure() {
        for scale in SCALES {
            for len in 1..40usize {
                let (w, _) = measure(len, scale);
                assert_eq!(fits(w, scale), len, "len {len} @ {scale:?}");
                // One cell short of the run must not claim to fit it.
                assert!(fits(w - 1, scale) < len, "len {len} @ {scale:?}");
            }
            assert_eq!(fits(0, scale), 0);
        }
    }

    /// The pattern `paint::truncated_str` is tested with: sweep every width so
    /// a clipping bug surfaces as a panic rather than as corrupt output.
    #[test]
    fn render_row_clips_without_panicking() {
        for scale in SCALES {
            for w in 0..40u16 {
                for h in 0..10u16 {
                    let area = Rect::new(0, 0, w.max(1), h.max(1));
                    let mut buf = Buffer::empty(area);
                    render_row(&mut buf, 0, 0, &styled("Tr0ub4dor"), scale);
                    // And starting off the right edge entirely.
                    render_row(&mut buf, w, 0, &styled("xyz"), scale);
                }
            }
        }
    }

    /// Near `u16::MAX` the column arithmetic must saturate rather than wrap
    /// around and paint at the left edge.
    #[test]
    fn render_row_near_the_coordinate_ceiling_paints_nothing_stray() {
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        render_row(&mut buf, u16::MAX - 2, 0, &styled("ABCDEF"), Scale::Full);
        assert!(
            painted_bbox(&buf, area).is_none(),
            "wrapped around the origin"
        );
    }

    /// A dropped character is a mistyped password. Unsupported input has to
    /// occupy its slot.
    #[test]
    fn unmapped_chars_still_occupy_a_cell() {
        for scale in SCALES {
            let (w, h) = measure(3, scale);
            let area = Rect::new(0, 0, w, h);

            let mut with_unknown = Buffer::empty(area);
            render_row(&mut with_unknown, 0, 0, &styled("aéb"), scale);
            let (_, _, x1, _) = painted_bbox(&with_unknown, area).unwrap();

            // The third glyph is painted, so the run reaches the far edge.
            assert!(
                x1 > scale.advance() * 2,
                "the run collapsed to two glyphs @ {scale:?}"
            );
        }
    }

    #[test]
    fn per_character_styles_are_applied_independently() {
        use ratatui::style::Color;
        let red = Style::default().fg(Color::Red);
        let blue = Style::default().fg(Color::Blue);
        let area = Rect::new(0, 0, 40, 8);
        let mut buf = Buffer::empty(area);
        render_row(&mut buf, 0, 0, &[('T', red), ('T', blue)], Scale::Half);

        // Both `T`s paint their top-left pixel; they must not share a colour.
        let first = buf.cell(Position::new(0, 0)).unwrap().style().fg;
        let second = buf
            .cell(Position::new(Scale::Half.advance(), 0))
            .unwrap()
            .style()
            .fg;
        assert_eq!(first, Some(Color::Red));
        assert_eq!(second, Some(Color::Blue));
    }

    #[test]
    fn a_space_paints_nothing_but_still_advances() {
        let scale = Scale::Half;
        let area = Rect::new(0, 0, 40, 8);
        let mut buf = Buffer::empty(area);
        render_row(&mut buf, 0, 0, &styled(" A"), scale);
        let (x0, _, _, _) = painted_bbox(&buf, area).unwrap();
        assert_eq!(x0, scale.advance(), "the space did not hold its slot");
    }

    /// Not an assertion — a way to look at the font, since "is this legible?"
    /// is not a property a test can decide.
    ///
    /// `cargo test -p cli-kit --features tui -- --ignored --nocapture font_sheet`
    #[test]
    #[ignore = "visual inspection only"]
    fn font_sheet() {
        for scale in SCALES {
            for line in [
                "0O 1lI 5S 8B 6G 2Z",
                "ABCDEFGHIJKLM",
                "NOPQRSTUVWXYZ",
                "abcdefghijklm",
                "nopqrstuvwxyz",
                "0123456789",
                "!\"#$%&'()*+,-./",
                ":;<=>?@[\\]^_`{|}~",
            ] {
                let (w, h) = measure(line.chars().count(), scale);
                let area = Rect::new(0, 0, w, h);
                let mut buf = Buffer::empty(area);
                render_row(&mut buf, 0, 0, &styled(line), scale);
                println!("\n{scale:?}  {line}");
                for y in 0..h {
                    let row: String = (0..w)
                        .map(|x| buf.cell(Position::new(x, y)).unwrap().symbol())
                        .collect();
                    println!("{row}");
                }
            }
        }
    }

    #[test]
    fn smaller_walks_down_to_half_and_stops() {
        assert_eq!(Scale::Full.smaller(), Some(Scale::Half));
        assert_eq!(Scale::Half.smaller(), None);
    }
}
