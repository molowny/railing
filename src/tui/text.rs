//! String measurement and wrapping, independent of any [`Buffer`].
//!
//! These live apart from [`super::paint`] because nothing here touches a
//! terminal buffer: a widget that needs to know how tall it will be — a
//! multi-line input sizing itself, an outline row reserving lines — has to
//! measure *before* it can paint, and [`super::widgets::text_area`] needs the
//! measurement without painting at all.
//!
//! Everything counts **display columns**, not chars or bytes. A CJK ideograph
//! is two columns wide, a combining mark is zero, and mixing those up produces
//! layouts that look right in ASCII tests and fall apart on real input.
//!
//! [`Buffer`]: ratatui::buffer::Buffer

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Display width of `s` in terminal columns.
pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Display width of a single char, treating unprintable chars as zero-width.
pub fn char_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// Hard-wrap `s` into lines of at most `width` display columns.
///
/// Breaks at char boundaries with **no word awareness**, which is deliberate:
/// the strings this exists for — secrets, URLs, base64, hex digests — have no
/// meaningful spaces to break at, and word-wrapping them would only make it
/// harder to read a value back character by character.
///
/// Never splits a char across lines: a char that would overflow the current
/// line starts the next one instead. A char wider than `width` itself (a CJK
/// ideograph at `width == 1`) gets a line of its own and overflows it, because
/// the alternative is an empty line and an infinite loop.
///
/// Returns `vec![String::new()]` for empty input — one empty line, not zero
/// lines, so a caller sizing a widget still reserves a row. Returns no lines
/// at all when `width == 0`, where nothing can be laid out.
pub fn wrap_hard(s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    if s.is_empty() {
        return vec![String::new()];
    }

    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;

    for ch in s.chars() {
        let w = char_width(ch);
        // Break *before* the overflowing char. Guarding on a non-empty line
        // is what stops a too-wide char from looping forever.
        if cur_w + w > width && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            cur_w = 0;
        }
        cur.push(ch);
        cur_w += w;
    }

    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Wrap every `\n`-separated line of `s` with [`wrap_hard`], flattened.
///
/// An embedded blank line stays one empty row rather than vanishing, so the
/// paragraph shape of a multi-line value survives.
pub fn wrap_hard_multiline(s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    if s.is_empty() {
        return vec![String::new()];
    }
    s.split('\n')
        .flat_map(|line| wrap_hard(line, width))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_hard_splits_ascii_at_width() {
        assert_eq!(wrap_hard("abcdef", 3), vec!["abc", "def"]);
        assert_eq!(wrap_hard("abcdefg", 3), vec!["abc", "def", "g"]);
        assert_eq!(wrap_hard("ab", 3), vec!["ab"]);
    }

    #[test]
    fn wrap_hard_edge_cases() {
        assert_eq!(wrap_hard("", 3), vec![String::new()]);
        assert!(wrap_hard("abc", 0).is_empty());
        assert!(wrap_hard("", 0).is_empty());
    }

    /// Regression: the sealward original counted chars, so a row of CJK was
    /// laid out at half its real width and overflowed the pane.
    #[test]
    fn wrap_hard_counts_columns_not_chars() {
        // Each ideograph is 2 columns, so only two fit in a width of 5.
        let lines = wrap_hard("日本語テスト", 5);
        assert_eq!(lines, vec!["日本", "語テ", "スト"]);
        for line in &lines {
            assert!(width(line) <= 5, "{line:?} exceeds 5 columns");
        }
    }

    #[test]
    fn wrap_hard_never_splits_a_char() {
        // Width 3 leaves one spare column after one 2-column char; the next
        // char moves to a new line rather than being cut in half.
        let lines = wrap_hard("日本", 3);
        assert_eq!(lines, vec!["日", "本"]);
    }

    #[test]
    fn wrap_hard_char_wider_than_width_gets_its_own_line() {
        // Overflows, but terminates and loses nothing.
        assert_eq!(wrap_hard("日本", 1), vec!["日", "本"]);
    }

    #[test]
    fn wrap_hard_keeps_zero_width_marks_attached() {
        // "e" + combining acute: one column, two chars.
        let s = "e\u{301}f";
        assert_eq!(width(s), 2);
        assert_eq!(wrap_hard(s, 2), vec![s]);
    }

    #[test]
    fn wrap_hard_multiline_preserves_blank_lines() {
        assert_eq!(wrap_hard_multiline("ab\n\ncd", 4), vec!["ab", "", "cd"]);
    }

    #[test]
    fn wrap_hard_multiline_wraps_each_line() {
        assert_eq!(
            wrap_hard_multiline("abcdef\ngh", 3),
            vec!["abc", "def", "gh"]
        );
    }
}
