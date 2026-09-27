//! A hand-authored 5x7 bitmap font covering printable ASCII.
//!
//! Each glyph is one string of [`GLYPH_H`] rows, every row exactly [`GLYPH_W`]
//! characters of `.` (clear) and `#` (set), separated by newlines. The art *is*
//! the source: a reviewer has to be able to see at a glance that `0` carries a
//! slash and `O` does not, which is the entire reason this table is written out
//! rather than pulled from a font crate.
//!
//! ## Disambiguation
//!
//! This font exists to be read aloud off a screen and typed into another
//! device, so the pairs that ordinarily blur are drawn deliberately apart:
//!
//! | Pair | How they differ |
//! |------|-----------------|
//! | `0` / `O` | `0` is slashed corner-to-corner; `O` is a clean oval |
//! | `1` / `l` / `I` | `1` has a top flag and a symmetric foot; `l` has a top-left serif and a foot that kicks right; `I` has full serifs top and bottom |
//! | `5` / `S` | `5` opens with a flat square bar; `S` opens with a curve |
//! | `8` / `B` | `8` is two free-standing ovals; `B` hangs its bowls off a flat stem |
//! | `6` / `G` | `6` closes its bowl; `G` carries an inward spur |
//! | `2` / `Z` | `2` opens with a curve; `Z` opens with a flat bar |
//!
//! [`GLYPH_H`]: super::GLYPH_H
//! [`GLYPH_W`]: super::GLYPH_W

use super::{GLYPH_W, VISIBLE_SPACE};

/// Bytes per art row: the glyph columns plus the `\n` that follows them. The
/// final row has no trailing newline, so the whole string is one byte short of
/// `GLYPH_H * ROW_STRIDE`.
pub(super) const ROW_STRIDE: usize = GLYPH_W + 1;

/// Art for `ch`, or `None` when it is outside printable ASCII.
///
/// Callers index the returned string by `row * ROW_STRIDE + col`; every row is
/// exactly `GLYPH_W` bytes and every byte is ASCII, so byte offsets and display
/// columns coincide. `art_is_well_formed` in the tests holds that invariant.
pub(super) fn art(ch: char) -> Option<&'static str> {
    let art = match ch {
        ' ' => {
            "\
.....
.....
.....
.....
.....
.....
....."
        }
        '!' => {
            "\
..#..
..#..
..#..
..#..
..#..
.....
..#.."
        }
        '"' => {
            "\
.#.#.
.#.#.
.....
.....
.....
.....
....."
        }
        '#' => {
            "\
.#.#.
.#.#.
#####
.#.#.
#####
.#.#.
.#.#."
        }
        '$' => {
            "\
..#..
.####
#.#..
.###.
..#.#
####.
..#.."
        }
        '%' => {
            "\
##...
##..#
...#.
..#..
.#...
#..##
...##"
        }
        '&' => {
            "\
.##..
#..#.
#.#..
.#...
#.#.#
#..#.
.##.#"
        }
        '\'' => {
            "\
..#..
..#..
.....
.....
.....
.....
....."
        }
        '(' => {
            "\
...#.
..#..
.#...
.#...
.#...
..#..
...#."
        }
        ')' => {
            "\
.#...
..#..
...#.
...#.
...#.
..#..
.#..."
        }
        '*' => {
            "\
.....
#.#.#
.###.
#####
.###.
#.#.#
....."
        }
        '+' => {
            "\
.....
..#..
..#..
#####
..#..
..#..
....."
        }
        ',' => {
            "\
.....
.....
.....
.....
..##.
..#..
.#..."
        }
        '-' => {
            "\
.....
.....
.....
#####
.....
.....
....."
        }
        '.' => {
            "\
.....
.....
.....
.....
.....
.##..
.##.."
        }
        '/' => {
            "\
....#
...#.
...#.
..#..
.#...
.#...
#...."
        }

        // -- digits ---------------------------------------------------------
        // Slashed, so it cannot be read as `O`.
        '0' => {
            "\
.###.
#...#
#..##
#.#.#
##..#
#...#
.###."
        }
        // Top flag and a symmetric foot: not `l`, not `I`.
        '1' => {
            "\
..#..
.##..
..#..
..#..
..#..
..#..
.###."
        }
        // Curved shoulder, against `Z`'s flat bar.
        '2' => {
            "\
.###.
#...#
....#
...#.
..#..
.#...
#####"
        }
        '3' => {
            "\
#####
...#.
..#..
...#.
....#
#...#
.###."
        }
        '4' => {
            "\
...#.
..##.
.#.#.
#..#.
#####
...#.
...#."
        }
        // Flat square bar across the top, against `S`'s curve.
        '5' => {
            "\
#####
#....
####.
....#
....#
#...#
.###."
        }
        // Closed bowl, against `G`'s spur.
        '6' => {
            "\
..##.
.#...
#....
####.
#...#
#...#
.###."
        }
        '7' => {
            "\
#####
....#
...#.
..#..
.#...
.#...
.#..."
        }
        // Two free-standing ovals, against `B`'s flat stem.
        '8' => {
            "\
.###.
#...#
#...#
.###.
#...#
#...#
.###."
        }
        '9' => {
            "\
.###.
#...#
#...#
.####
....#
...#.
.##.."
        }

        ':' => {
            "\
.....
.##..
.##..
.....
.##..
.##..
....."
        }
        ';' => {
            "\
.....
.##..
.##..
.....
.##..
..#..
.#..."
        }
        '<' => {
            "\
...#.
..#..
.#...
#....
.#...
..#..
...#."
        }
        '=' => {
            "\
.....
.....
#####
.....
#####
.....
....."
        }
        '>' => {
            "\
.#...
..#..
...#.
....#
...#.
..#..
.#..."
        }
        '?' => {
            "\
.###.
#...#
....#
...#.
..#..
.....
..#.."
        }
        '@' => {
            "\
.###.
#...#
#.###
#.#.#
#.###
#....
.###."
        }

        // -- uppercase ------------------------------------------------------
        'A' => {
            "\
.###.
#...#
#...#
#####
#...#
#...#
#...#"
        }
        // Bowls hang off a flat stem, against `8`'s free-standing ovals.
        'B' => {
            "\
####.
#...#
#...#
####.
#...#
#...#
####."
        }
        'C' => {
            "\
.###.
#...#
#....
#....
#....
#...#
.###."
        }
        'D' => {
            "\
####.
#...#
#...#
#...#
#...#
#...#
####."
        }
        'E' => {
            "\
#####
#....
#....
####.
#....
#....
#####"
        }
        'F' => {
            "\
#####
#....
#....
####.
#....
#....
#...."
        }
        // Inward spur, against `6`'s closed bowl.
        'G' => {
            "\
.###.
#...#
#....
#.###
#...#
#...#
.###."
        }
        'H' => {
            "\
#...#
#...#
#...#
#####
#...#
#...#
#...#"
        }
        // Full serifs top and bottom: not `1`, not `l`.
        'I' => {
            "\
.###.
..#..
..#..
..#..
..#..
..#..
.###."
        }
        'J' => {
            "\
..###
...#.
...#.
...#.
...#.
#..#.
.##.."
        }
        'K' => {
            "\
#...#
#..#.
#.#..
##...
#.#..
#..#.
#...#"
        }
        'L' => {
            "\
#....
#....
#....
#....
#....
#....
#####"
        }
        'M' => {
            "\
#...#
##.##
#.#.#
#.#.#
#...#
#...#
#...#"
        }
        'N' => {
            "\
#...#
#...#
##..#
#.#.#
#..##
#...#
#...#"
        }
        // Clean oval, no slash: not `0`.
        'O' => {
            "\
.###.
#...#
#...#
#...#
#...#
#...#
.###."
        }
        'P' => {
            "\
####.
#...#
#...#
####.
#....
#....
#...."
        }
        'Q' => {
            "\
.###.
#...#
#...#
#...#
#.#.#
#..#.
.##.#"
        }
        'R' => {
            "\
####.
#...#
#...#
####.
#.#..
#..#.
#...#"
        }
        // Curved shoulder, against `5`'s flat bar.
        'S' => {
            "\
.####
#....
#....
.###.
....#
....#
####."
        }
        'T' => {
            "\
#####
..#..
..#..
..#..
..#..
..#..
..#.."
        }
        'U' => {
            "\
#...#
#...#
#...#
#...#
#...#
#...#
.###."
        }
        'V' => {
            "\
#...#
#...#
#...#
#...#
#...#
.#.#.
..#.."
        }
        'W' => {
            "\
#...#
#...#
#...#
#.#.#
#.#.#
##.##
#...#"
        }
        'X' => {
            "\
#...#
#...#
.#.#.
..#..
.#.#.
#...#
#...#"
        }
        'Y' => {
            "\
#...#
#...#
.#.#.
..#..
..#..
..#..
..#.."
        }
        // Flat bar across the top, against `2`'s curve.
        'Z' => {
            "\
#####
....#
...#.
..#..
.#...
#....
#####"
        }

        '[' => {
            "\
.###.
.#...
.#...
.#...
.#...
.#...
.###."
        }
        '\\' => {
            "\
#....
.#...
.#...
..#..
...#.
...#.
....#"
        }
        ']' => {
            "\
.###.
...#.
...#.
...#.
...#.
...#.
.###."
        }
        '^' => {
            "\
..#..
.#.#.
#...#
.....
.....
.....
....."
        }
        '_' => {
            "\
.....
.....
.....
.....
.....
.....
#####"
        }
        '`' => {
            "\
.#...
..#..
.....
.....
.....
.....
....."
        }

        // -- lowercase ------------------------------------------------------
        'a' => {
            "\
.....
.....
.###.
....#
.####
#...#
.####"
        }
        'b' => {
            "\
#....
#....
####.
#...#
#...#
#...#
####."
        }
        'c' => {
            "\
.....
.....
.###.
#....
#....
#....
.###."
        }
        'd' => {
            "\
....#
....#
.####
#...#
#...#
#...#
.####"
        }
        'e' => {
            "\
.....
.....
.###.
#...#
#####
#....
.###."
        }
        'f' => {
            "\
..##.
.#...
.#...
####.
.#...
.#...
.#..."
        }
        'g' => {
            "\
.....
.####
#...#
#...#
.####
....#
.###."
        }
        'h' => {
            "\
#....
#....
####.
#...#
#...#
#...#
#...#"
        }
        'i' => {
            "\
..#..
.....
.##..
..#..
..#..
..#..
.###."
        }
        'j' => {
            "\
...#.
.....
..##.
...#.
...#.
#..#.
.##.."
        }
        'k' => {
            "\
#....
#....
#..#.
#.#..
##...
#.#..
#..#."
        }
        // Top-left serif and a foot that kicks right: not `1`, not `I`.
        'l' => {
            "\
.##..
..#..
..#..
..#..
..#..
..#..
...##"
        }
        'm' => {
            "\
.....
.....
##.##
#.#.#
#.#.#
#.#.#
#.#.#"
        }
        'n' => {
            "\
.....
.....
####.
#...#
#...#
#...#
#...#"
        }
        'o' => {
            "\
.....
.....
.###.
#...#
#...#
#...#
.###."
        }
        'p' => {
            "\
.....
####.
#...#
#...#
####.
#....
#...."
        }
        'q' => {
            "\
.....
.####
#...#
#...#
.####
....#
....#"
        }
        'r' => {
            "\
.....
.....
#.##.
##...
#....
#....
#...."
        }
        's' => {
            "\
.....
.....
.####
#....
.###.
....#
####."
        }
        't' => {
            "\
.#...
.#...
####.
.#...
.#...
.#..#
..##."
        }
        'u' => {
            "\
.....
.....
#...#
#...#
#...#
#..##
.##.#"
        }
        'v' => {
            "\
.....
.....
#...#
#...#
#...#
.#.#.
..#.."
        }
        'w' => {
            "\
.....
.....
#...#
#...#
#.#.#
#.#.#
.#.#."
        }
        'x' => {
            "\
.....
.....
#...#
.#.#.
..#..
.#.#.
#...#"
        }
        'y' => {
            "\
.....
#...#
#...#
#...#
.####
....#
.###."
        }
        'z' => {
            "\
.....
.....
#####
...#.
..#..
.#...
#####"
        }

        '{' => {
            "\
...##
..#..
..#..
.#...
..#..
..#..
...##"
        }
        '|' => {
            "\
..#..
..#..
..#..
..#..
..#..
..#..
..#.."
        }
        '}' => {
            "\
##...
..#..
..#..
...#.
..#..
..#..
##..."
        }
        '~' => {
            "\
.....
.....
.#...
#.#.#
...#.
.....
....."
        }

        // The one glyph outside ASCII. A blank cannot be read: a caller that
        // must show a space *as a character* substitutes this for it. Shaped as
        // U+2423 OPEN BOX, and deliberately unlike `_` (which is a real
        // password character) so the two can never be confused.
        VISIBLE_SPACE => {
            "\
.....
.....
.....
.....
#...#
#...#
#####"
        }

        _ => return None,
    };
    Some(art)
}

/// Stand-in for a character this font has no glyph for.
///
/// A hatched box rather than a blank: a value must never *render* shorter than
/// it is, or the reader silently drops a character while typing it back in.
pub(super) const UNKNOWN: &str = "\
#####
#.#.#
##.##
#.#.#
##.##
#.#.#
#####";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::widgets::big_text::GLYPH_H;

    /// Printable ASCII, in code-point order.
    const COVERED: std::ops::RangeInclusive<char> = ' '..='~';

    /// Every character this font draws — the ASCII range plus the one glyph
    /// outside it. The single source of truth for the coverage, well-formedness
    /// and uniqueness tests.
    fn all_art() -> Vec<(char, &'static str)> {
        COVERED
            .chain([VISIBLE_SPACE])
            .map(|c| (c, art(c).expect("listed as covered")))
            .collect()
    }

    /// The whole printable-ASCII range, no holes. A password is not required to
    /// stay inside any smaller set.
    #[test]
    fn every_printable_ascii_has_a_glyph() {
        for ch in COVERED {
            assert!(
                art(ch).is_some(),
                "no glyph for {ch:?} (U+{:04X})",
                ch as u32
            );
        }
    }

    #[test]
    fn non_ascii_has_no_glyph() {
        for ch in ['\u{7f}', '\u{80}', 'é', '“', '€', '👍'] {
            assert!(art(ch).is_none(), "unexpected glyph for {ch:?}");
        }
    }

    /// Row/column indexing is raw byte arithmetic, so a ragged or non-ASCII row
    /// would silently shear a glyph instead of failing.
    #[test]
    fn art_is_well_formed() {
        for (ch, art) in all_art().into_iter().chain([('\u{0}', UNKNOWN)]) {
            let rows: Vec<&str> = art.split('\n').collect();
            assert_eq!(rows.len(), GLYPH_H, "{ch:?} has {} rows", rows.len());
            for (i, row) in rows.iter().enumerate() {
                assert_eq!(row.len(), GLYPH_W, "{ch:?} row {i} is {:?}", row);
                assert!(
                    row.bytes().all(|b| b == b'.' || b == b'#'),
                    "{ch:?} row {i} has stray characters: {row:?}"
                );
            }
            assert_eq!(
                art.len(),
                GLYPH_H * ROW_STRIDE - 1,
                "{ch:?} does not match the byte stride the renderer indexes by"
            );
        }
    }

    /// The guard that gives this table its reason to exist. Two glyphs with
    /// identical bitmaps are two characters the reader cannot tell apart —
    /// which is exactly the failure mode large type is meant to remove. It also
    /// catches the likeliest authoring slip: pasting `O` over `0`.
    #[test]
    fn no_two_glyphs_are_identical() {
        let all = all_art();
        for (i, (a_ch, a)) in all.iter().enumerate() {
            for (b_ch, b) in &all[i + 1..] {
                assert_ne!(a, b, "{a_ch:?} and {b_ch:?} render identically");
            }
        }
    }

    /// Named individually so a regression names the pair it broke rather than
    /// pointing at the whole table.
    #[test]
    fn the_confusable_pairs_are_drawn_apart() {
        for (a, b) in [
            ('0', 'O'),
            ('1', 'l'),
            ('1', 'I'),
            ('l', 'I'),
            ('5', 'S'),
            ('8', 'B'),
            ('6', 'G'),
            ('2', 'Z'),
        ] {
            assert_ne!(art(a), art(b), "{a:?} and {b:?} render identically");
        }
    }

    /// `0` must carry ink on a diagonal that `O` leaves clear — the difference
    /// has to be inside the counter, not just anywhere in the bitmap.
    #[test]
    fn zero_is_slashed_and_the_letter_o_is_not() {
        let zero = art('0').unwrap();
        let oh = art('O').unwrap();
        let px = |art: &str, row: usize, col: usize| art.as_bytes()[row * ROW_STRIDE + col] == b'#';

        // The counter of `O` is empty on every interior row.
        for row in 1..GLYPH_H - 1 {
            for col in 1..GLYPH_W - 1 {
                assert!(
                    !px(oh, row, col),
                    "O has ink inside its counter at {row},{col}"
                );
            }
        }
        // `0` does not.
        let slash = (1..GLYPH_H - 1)
            .flat_map(|r| (1..GLYPH_W - 1).map(move |c| (r, c)))
            .filter(|(r, c)| px(zero, *r, *c))
            .count();
        assert!(slash >= 3, "0 is not slashed: only {slash} interior pixels");
    }

    /// The placeholder has to be visibly *not* a character, and has to be
    /// distinct from every real glyph so it cannot be misread as one.
    #[test]
    fn the_unknown_placeholder_is_not_a_real_glyph() {
        for (ch, art) in all_art() {
            assert_ne!(art, UNKNOWN, "{ch:?} collides with the unknown placeholder");
        }
        assert!(UNKNOWN.contains('#'), "the placeholder must be visible");
    }

    /// The point of the visible space: it has to be *visible*, and it has to be
    /// impossible to read back as `_`, which is a character a password can
    /// actually contain.
    #[test]
    fn the_visible_space_is_inked_and_unlike_underscore() {
        let vs = art(VISIBLE_SPACE).expect("no glyph for the visible space");
        assert!(vs.bytes().any(|b| b == b'#'), "the visible space is blank");
        assert_ne!(vs, art('_').unwrap(), "reads as an underscore");
        assert_ne!(vs, art(' ').unwrap(), "reads as a blank");
        assert_ne!(vs, UNKNOWN, "reads as an unrenderable character");
    }

    /// A glyph with no ink is indistinguishable from a missing one. Space is
    /// the sole legitimate exception.
    #[test]
    fn only_space_is_blank() {
        for (ch, art) in all_art() {
            let inked = art.bytes().any(|b| b == b'#');
            assert_eq!(inked, ch != ' ', "{ch:?} inked={inked}");
        }
    }
}
