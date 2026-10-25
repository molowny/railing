//! A line editor: text and a cursor, with clack's editing keys.
//!
//! The text is a `Vec<char>` and is wiped when the editor is dropped: what is
//! typed into a prompt may be a token or a key's passphrase.

use zeroize::Zeroize;

use crate::term::Key;

/// What a key did to the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    /// The text or the cursor changed.
    Changed,
    /// The key is not an editing key.
    Ignored,
}

#[derive(Default)]
pub struct LineEditor {
    text: Vec<char>,
    cursor: usize,
}

impl Drop for LineEditor {
    fn drop(&mut self) {
        self.text.zeroize();
    }
}

impl std::fmt::Debug for LineEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the text itself.
        f.debug_struct("LineEditor")
            .field("len", &self.text.len())
            .field("cursor", &self.cursor)
            .finish()
    }
}

/// Indices where words start, and both ends.
fn word_starts(text: &[char]) -> Vec<usize> {
    let mut out = vec![0];
    let mut in_word = false;
    for (i, c) in text.iter().enumerate() {
        if c.is_whitespace() {
            in_word = false;
        } else if !in_word {
            out.push(i);
            in_word = true;
        }
    }
    out.push(text.len());
    out
}

impl LineEditor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The text, in a string the caller owns (and wipes, if it matters).
    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    /// The text's lines, and the cursor as (line, column in characters).
    pub fn lines(&self) -> (Vec<String>, (usize, usize)) {
        let mut lines = vec![String::new()];
        let mut at = (0, 0);
        for (i, c) in self.text.iter().enumerate() {
            if i == self.cursor {
                at = (
                    lines.len() - 1,
                    lines.last().map_or(0, |l| l.chars().count()),
                );
            }
            if *c == '\n' {
                lines.push(String::new());
            } else {
                lines.last_mut().expect("one line").push(*c);
            }
        }
        if self.cursor == self.text.len() {
            at = (
                lines.len() - 1,
                lines.last().map_or(0, |l| l.chars().count()),
            );
        }
        (lines, at)
    }

    pub fn insert(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += 1;
    }

    pub fn clear(&mut self) {
        self.text.zeroize();
        self.text.clear();
        self.cursor = 0;
    }

    /// Start and end (exclusive of the newline) of the line the cursor is on.
    fn line_bounds(&self, at: usize) -> (usize, usize) {
        let start = self.text[..at]
            .iter()
            .rposition(|c| *c == '\n')
            .map_or(0, |i| i + 1);
        let end = self.text[at..]
            .iter()
            .position(|c| *c == '\n')
            .map_or(self.text.len(), |i| at + i);
        (start, end)
    }

    fn vertical(&mut self, down: bool) {
        let (start, end) = self.line_bounds(self.cursor);
        let col = self.cursor - start;
        let target = if down {
            if end == self.text.len() {
                return;
            }
            end + 1
        } else {
            if start == 0 {
                return;
            }
            self.line_bounds(start - 1).0
        };
        let (ts, te) = self.line_bounds(target);
        self.cursor = ts + col.min(te - ts);
    }

    /// Apply an editing key: characters, Backspace and Delete, the arrows,
    /// Home/End and Ctrl-A/Ctrl-E, word jumps (Alt-B/Alt-F) and word delete
    /// (Ctrl-W), Ctrl-U to clear. `newline` lets Enter insert a line break.
    pub fn apply(&mut self, key: Key, newline: bool) -> Edit {
        match key {
            Key::Char(c) => self.insert(c),
            Key::Enter if newline => self.insert('\n'),
            Key::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.text.remove(self.cursor);
            }
            Key::Delete if self.cursor < self.text.len() => {
                self.text.remove(self.cursor);
            }
            Key::Left if self.cursor > 0 => self.cursor -= 1,
            Key::Right if self.cursor < self.text.len() => self.cursor += 1,
            Key::Up => self.vertical(false),
            Key::Down => self.vertical(true),
            Key::Home | Key::Ctrl('a') => self.cursor = self.line_bounds(self.cursor).0,
            Key::End | Key::Ctrl('e') => self.cursor = self.line_bounds(self.cursor).1,
            Key::Alt('b') => {
                let starts = word_starts(&self.text);
                let i = starts.binary_search(&self.cursor).unwrap_or_else(|i| i);
                self.cursor = starts[i.saturating_sub(1)];
            }
            Key::Alt('f') => {
                let starts = word_starts(&self.text);
                let i = starts
                    .binary_search(&self.cursor)
                    .map_or_else(|i| i, |i| i + 1);
                self.cursor = starts[i.min(starts.len() - 1)];
            }
            Key::Ctrl('w') if self.cursor > 0 => {
                let starts = word_starts(&self.text);
                let i = starts.binary_search(&self.cursor).unwrap_or_else(|i| i);
                let start = starts[i.saturating_sub(1)];
                self.text.drain(start..self.cursor).for_each(drop);
                self.cursor = start;
            }
            Key::Ctrl('u') => self.clear(),
            _ => return Edit::Ignored,
        }
        Edit::Changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> LineEditor {
        let mut e = LineEditor::new();
        for c in s.chars() {
            if c == '\n' {
                e.apply(Key::Enter, true);
            } else {
                e.insert(c);
            }
        }
        e
    }

    #[test]
    fn edits_at_the_cursor() {
        let mut e = typed("helo");
        e.apply(Key::Left, false);
        e.apply(Key::Char('l'), false);
        assert_eq!(e.text(), "hello");
        e.apply(Key::Home, false);
        e.apply(Key::Delete, false);
        assert_eq!(e.text(), "ello");
        e.apply(Key::Ctrl('e'), false);
        e.apply(Key::Backspace, false);
        assert_eq!(e.text(), "ell");
        assert_eq!(e.lines(), (vec!["ell".to_string()], (0, 3)));
    }

    #[test]
    fn words_jump_and_delete() {
        let mut e = typed("one two  three");
        e.apply(Key::Alt('b'), false);
        assert_eq!(e.lines().1, (0, 9));
        e.apply(Key::Alt('b'), false);
        assert_eq!(e.lines().1, (0, 4));
        e.apply(Key::Alt('f'), false);
        assert_eq!(e.lines().1, (0, 9));
        e.apply(Key::End, false);
        e.apply(Key::Ctrl('w'), false);
        assert_eq!(e.text(), "one two  ");
    }

    #[test]
    fn lines_move_up_and_down_keeping_the_column() {
        let mut e = typed("first line\nab\nthird");
        assert_eq!(e.lines().1, (2, 5));
        e.apply(Key::Up, true);
        assert_eq!(e.lines().1, (1, 2));
        e.apply(Key::Up, true);
        assert_eq!(e.lines().1, (0, 2));
        e.apply(Key::End, true);
        e.apply(Key::Down, true);
        assert_eq!(e.lines().1, (1, 2));
        assert_eq!(e.lines().0, ["first line", "ab", "third"]);
    }

    #[test]
    fn enter_is_a_line_break_only_where_asked() {
        let mut e = typed("a");
        assert_eq!(e.apply(Key::Enter, false), Edit::Ignored);
        e.apply(Key::Enter, true);
        assert_eq!(e.lines(), (vec!["a".to_string(), String::new()], (1, 0)));
    }

    #[test]
    fn debug_never_shows_the_text() {
        assert!(!format!("{:?}", typed("hunter2")).contains("hunter2"));
    }
}
