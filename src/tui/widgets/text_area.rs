//! Multi-line text input, the counterpart to [`super::text_input::TextInput`].
//!
//! Same contract: the widget owns a buffer and a byte cursor, `on_key` returns
//! an [`InputEvent`], the buffer is zeroized on `Drop`, and rendering is a
//! separate free function so the state stays immutable during a paint.
//!
//! # Where it differs from [`TextInput`]
//!
//! | Key | `TextInput` | `TextArea` |
//! |-----|-------------|------------|
//! | `Enter` | `Submit` | **inserts a newline** |
//! | `Ctrl-d` | — | **`Submit`** |
//! | `Up`/`Down`, `Ctrl-p`/`Ctrl-n` | — | move a line, preserving the goal column |
//! | `Ctrl-a`/`Home`, `Ctrl-e`/`End` | buffer start/end | **line** start/end |
//! | `Ctrl-u` | clears the buffer | deletes to **line** start |
//!
//! A caller swapping one for the other must re-bind its submit key; `Enter` no
//! longer ends editing, which is the whole point of the widget.
//!
//! # No masking
//!
//! [`TextInput::with_masked`] has no counterpart here. Masking a multi-line
//! buffer would have to lie about where the lines break, and the model this
//! serves is reveal-then-edit: a secret is already on screen by the time it is
//! being edited.
//!
//! # Vertical motion is by logical line
//!
//! `Up`/`Down` move between `\n`-separated lines, not between wrapped display
//! rows. `on_key` has no width to wrap against, and threading the render width
//! into key handling would duplicate state that only the painter knows —
//! leaving two sources of truth to drift. Wrapping stays a display concern.
//!
//! [`TextInput`]: super::text_input::TextInput
//! [`TextInput::with_masked`]: super::text_input::TextInput::with_masked

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use zeroize::Zeroize;

use crate::tui::text;

pub use super::text_input::InputEvent;

/// Multi-line text input with a byte-indexed cursor.
pub struct TextArea {
    buf: String,
    /// Byte index, always on a char boundary.
    cursor: usize,
    /// Column to aim for during a run of vertical moves, so crossing a short
    /// line does not permanently pull the cursor left. Cleared by any
    /// horizontal move or edit.
    goal_col: Option<usize>,
}

impl TextArea {
    pub fn new(initial: impl Into<String>) -> Self {
        let buf: String = initial.into();
        let cursor = buf.len();
        Self {
            buf,
            cursor,
            goal_col: None,
        }
    }

    pub fn value(&self) -> &str {
        &self.buf
    }

    /// Move the buffer out. The original is left empty; the caller becomes
    /// responsible for zeroizing the returned plaintext.
    pub fn into_value(mut self) -> String {
        std::mem::take(&mut self.buf)
    }

    /// Number of `\n`-separated lines. Always at least 1.
    pub fn line_count(&self) -> usize {
        self.buf.split('\n').count()
    }

    /// Rows this buffer needs when wrapped at `width`. Always at least 1, so a
    /// caller reserving space for an empty editor still gets a row.
    pub fn desired_height(&self, width: u16) -> u16 {
        if width == 0 {
            return 1;
        }
        let n = text::wrap_hard_multiline(&self.buf, width as usize).len();
        n.clamp(1, u16::MAX as usize) as u16
    }

    pub fn on_key(&mut self, key: KeyEvent) -> InputEvent {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl) {
            (KeyCode::Char('d'), true) => InputEvent::Submit,
            (KeyCode::Esc, _) => InputEvent::Cancel,

            (KeyCode::Enter, _) => {
                self.goal_col = None;
                self.insert_char('\n');
                InputEvent::Edited
            }

            (KeyCode::Char('a'), true) | (KeyCode::Home, _) => {
                self.goal_col = None;
                let start = self.line_start(self.cursor);
                if start == self.cursor {
                    InputEvent::Ignored
                } else {
                    self.cursor = start;
                    InputEvent::Edited
                }
            }
            (KeyCode::Char('e'), true) | (KeyCode::End, _) => {
                self.goal_col = None;
                let end = self.line_end(self.cursor);
                if end == self.cursor {
                    InputEvent::Ignored
                } else {
                    self.cursor = end;
                    InputEvent::Edited
                }
            }

            (KeyCode::Char('b'), true) | (KeyCode::Left, _) => {
                self.goal_col = None;
                if self.move_left() {
                    InputEvent::Edited
                } else {
                    InputEvent::Ignored
                }
            }
            (KeyCode::Char('f'), true) | (KeyCode::Right, _) => {
                self.goal_col = None;
                if self.move_right() {
                    InputEvent::Edited
                } else {
                    InputEvent::Ignored
                }
            }

            (KeyCode::Char('p'), true) | (KeyCode::Up, _) => {
                if self.move_vertical(-1) {
                    InputEvent::Edited
                } else {
                    InputEvent::Ignored
                }
            }
            (KeyCode::Char('n'), true) | (KeyCode::Down, _) => {
                if self.move_vertical(1) {
                    InputEvent::Edited
                } else {
                    InputEvent::Ignored
                }
            }

            (KeyCode::Char('u'), true) => {
                self.goal_col = None;
                let start = self.line_start(self.cursor);
                if start == self.cursor {
                    InputEvent::Ignored
                } else {
                    crate::tui::wipe::remove(&mut self.buf, start..self.cursor);
                    self.cursor = start;
                    InputEvent::Edited
                }
            }
            (KeyCode::Char('w'), true) => {
                self.goal_col = None;
                if self.delete_prev_word() {
                    InputEvent::Edited
                } else {
                    InputEvent::Ignored
                }
            }

            (KeyCode::Backspace, _) => {
                self.goal_col = None;
                if self.delete_prev_char() {
                    InputEvent::Edited
                } else {
                    InputEvent::Ignored
                }
            }
            (KeyCode::Delete, _) => {
                self.goal_col = None;
                if self.delete_next_char() {
                    InputEvent::Edited
                } else {
                    InputEvent::Ignored
                }
            }

            (KeyCode::Char(c), false) if !c.is_control() => {
                self.goal_col = None;
                self.insert_char(c);
                InputEvent::Edited
            }

            _ => InputEvent::Ignored,
        }
    }

    // -- line geometry ------------------------------------------------------

    /// Byte offset of the start of the line containing `at`.
    fn line_start(&self, at: usize) -> usize {
        self.buf[..at].rfind('\n').map_or(0, |i| i + 1)
    }

    /// Byte offset of the end of the line containing `at`, before its newline.
    fn line_end(&self, at: usize) -> usize {
        self.buf[at..]
            .find('\n')
            .map_or(self.buf.len(), |rel| at + rel)
    }

    /// Display column of `at` within its line.
    fn column_of(&self, at: usize) -> usize {
        text::width(&self.buf[self.line_start(at)..at])
    }

    /// Byte offset within `[start, end]` closest to display column `col`.
    fn byte_at_column(&self, start: usize, end: usize, col: usize) -> usize {
        let mut w = 0usize;
        for (rel, ch) in self.buf[start..end].char_indices() {
            if w >= col {
                return start + rel;
            }
            w += text::char_width(ch);
        }
        end
    }

    fn move_vertical(&mut self, delta: isize) -> bool {
        let goal = self.goal_col.unwrap_or_else(|| self.column_of(self.cursor));

        let start = self.line_start(self.cursor);
        let target_line = if delta < 0 {
            if start == 0 {
                self.goal_col = Some(goal);
                return false;
            }
            // Start of the previous line.
            self.line_start(start - 1)
        } else {
            let end = self.line_end(self.cursor);
            if end >= self.buf.len() {
                self.goal_col = Some(goal);
                return false;
            }
            end + 1
        };

        let target_end = self.line_end(target_line);
        self.cursor = self.byte_at_column(target_line, target_end, goal);
        // Remembered *after* moving, so a run of Up/Down keeps aiming for the
        // original column even across short lines.
        self.goal_col = Some(goal);
        true
    }

    // -- editing ------------------------------------------------------------

    fn move_left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor = prev_boundary(&self.buf, self.cursor);
        true
    }

    fn move_right(&mut self) -> bool {
        if self.cursor >= self.buf.len() {
            return false;
        }
        let mut i = self.cursor + 1;
        while i < self.buf.len() && !self.buf.is_char_boundary(i) {
            i += 1;
        }
        self.cursor = i;
        true
    }

    fn insert_char(&mut self, c: char) {
        crate::tui::wipe::insert(&mut self.buf, self.cursor, c);
        self.cursor += c.len_utf8();
    }

    fn delete_prev_char(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let i = prev_boundary(&self.buf, self.cursor);
        crate::tui::wipe::remove(&mut self.buf, i..self.cursor);
        self.cursor = i;
        true
    }

    fn delete_next_char(&mut self) -> bool {
        if self.cursor >= self.buf.len() {
            return false;
        }
        let mut j = self.cursor + 1;
        while j < self.buf.len() && !self.buf.is_char_boundary(j) {
            j += 1;
        }
        crate::tui::wipe::remove(&mut self.buf, self.cursor..j);
        true
    }

    fn delete_prev_word(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let mut i = self.cursor;
        // Skip whitespace, then consume the word before it. Newlines count as
        // whitespace, so Ctrl-w at a line start joins onto the line above.
        while i > 0 {
            let prev = prev_boundary(&self.buf, i);
            if !self.buf[prev..i].chars().next().unwrap().is_whitespace() {
                break;
            }
            i = prev;
        }
        while i > 0 {
            let prev = prev_boundary(&self.buf, i);
            if self.buf[prev..i].chars().next().unwrap().is_whitespace() {
                break;
            }
            i = prev;
        }
        if i == self.cursor {
            return false;
        }
        crate::tui::wipe::remove(&mut self.buf, i..self.cursor);
        self.cursor = i;
        true
    }
}

fn prev_boundary(s: &str, mut i: usize) -> usize {
    if i == 0 {
        return 0;
    }
    i -= 1;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

impl Drop for TextArea {
    fn drop(&mut self) {
        self.buf.zeroize();
    }
}

/// Wrapped display rows as `(start_byte, text)`.
fn visual_lines(s: &str, width: usize) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut base = 0usize;
    for logical in s.split('\n') {
        let pieces = text::wrap_hard(logical, width);
        let mut off = base;
        if pieces.is_empty() {
            out.push((base, String::new()));
        }
        for piece in pieces {
            let len = piece.len();
            out.push((off, piece));
            off += len;
        }
        // +1 for the '\n' that `split` consumed.
        base += logical.len() + 1;
    }
    out
}

/// Render `state` into `area`, wrapping at `area.width` and scrolling
/// vertically so the cursor stays visible.
///
/// Scroll is derived from the cursor here rather than stored on the widget, so
/// the same buffer renders correctly into two different-sized rects and the
/// state needs no `&mut` during a paint.
pub fn render(area: Rect, buf: &mut Buffer, state: &TextArea, style: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let width = area.width as usize;
    let height = area.height as usize;

    let lines = visual_lines(&state.buf, width);

    // The row holding the cursor: the last one it falls inside, so a cursor at
    // a wrap boundary shows at the start of the following row.
    let mut cursor_row = 0usize;
    let mut cursor_col = 0usize;
    for (i, (start, tx)) in lines.iter().enumerate() {
        if state.cursor >= *start && state.cursor <= start + tx.len() {
            cursor_row = i;
            cursor_col = text::width(&tx[..state.cursor - start]);
        }
    }

    let scroll = cursor_row.saturating_sub(height.saturating_sub(1));

    // Blank the whole rect first: the caller may be stamping over live content.
    for row in 0..area.height {
        for col in 0..area.width {
            let cell = &mut buf[(area.x + col, area.y + row)];
            cell.set_char(' ');
            cell.set_style(style);
        }
    }

    for row in 0..height {
        let Some((_, tx)) = lines.get(scroll + row) else {
            break;
        };
        let mut col = 0u16;
        for ch in tx.chars() {
            let w = text::char_width(ch) as u16;
            if col + w > area.width {
                break;
            }
            let cell = &mut buf[(area.x + col, area.y + row as u16)];
            cell.set_char(ch);
            cell.set_style(style);
            col += w.max(1);
        }
    }

    // Cursor as reverse video over whatever occupies the cell.
    let cy = cursor_row.saturating_sub(scroll);
    if cy < height && cursor_col < width {
        let cell = &mut buf[(area.x + cursor_col as u16, area.y + cy as u16)];
        cell.set_style(style.add_modifier(Modifier::REVERSED));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::chord::{key_code, key_ctrl};
    use ratatui::layout::Position;

    fn press(a: &mut TextArea, k: KeyEvent) -> InputEvent {
        a.on_key(k)
    }

    fn render_to_lines(state: &TextArea, width: u16, height: u16) -> Vec<String> {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        render(area, &mut buf, state, Style::default());
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| {
                        buf.cell(Position::new(x, y))
                            .unwrap()
                            .symbol()
                            .chars()
                            .next()
                            .unwrap_or(' ')
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    /// The mirror of `TextInput`'s `newline_char_is_rejected`: here it is the
    /// entire reason the widget exists.
    #[test]
    fn enter_inserts_a_newline() {
        let mut a = TextArea::new("ab");
        assert_eq!(press(&mut a, key_code(KeyCode::Enter)), InputEvent::Edited);
        assert_eq!(a.value(), "ab\n");
        assert_eq!(a.line_count(), 2);
    }

    #[test]
    fn ctrl_d_submits_and_esc_cancels() {
        let mut a = TextArea::new("x");
        assert_eq!(press(&mut a, key_ctrl('d')), InputEvent::Submit);
        assert_eq!(press(&mut a, key_code(KeyCode::Esc)), InputEvent::Cancel);
        // Neither touched the buffer.
        assert_eq!(a.value(), "x");
    }

    #[test]
    fn into_value_empties_the_buffer() {
        let a = TextArea::new("secret");
        assert_eq!(a.into_value(), "secret");
    }

    #[test]
    fn home_and_end_are_line_scoped() {
        let mut a = TextArea::new("one\ntwo");
        // Cursor starts at the very end.
        press(&mut a, key_ctrl('a'));
        assert_eq!(a.cursor, 4, "start of the second line, not the buffer");
        press(&mut a, key_ctrl('e'));
        assert_eq!(a.cursor, 7);
    }

    #[test]
    fn ctrl_u_deletes_to_line_start_only() {
        let mut a = TextArea::new("keep\ndrop");
        assert_eq!(press(&mut a, key_ctrl('u')), InputEvent::Edited);
        assert_eq!(a.value(), "keep\n");
        // Nothing left on this line to delete.
        assert_eq!(press(&mut a, key_ctrl('u')), InputEvent::Ignored);
    }

    #[test]
    fn vertical_motion_preserves_the_goal_column() {
        // Long / short / long: crossing the short line must not pull the
        // cursor permanently left.
        let mut a = TextArea::new("abcdef\nxy\nghijkl");
        a.cursor = 0;
        press(&mut a, key_code(KeyCode::Right)); // col 1
        press(&mut a, key_code(KeyCode::Right)); // col 2
        press(&mut a, key_code(KeyCode::Right)); // col 3
        assert_eq!(a.column_of(a.cursor), 3);

        press(&mut a, key_code(KeyCode::Down));
        assert_eq!(a.column_of(a.cursor), 2, "clamped to the short line");

        press(&mut a, key_code(KeyCode::Down));
        assert_eq!(a.column_of(a.cursor), 3, "goal column restored");
    }

    #[test]
    fn vertical_motion_stops_at_the_edges() {
        let mut a = TextArea::new("one\ntwo");
        a.cursor = 0;
        assert_eq!(press(&mut a, key_code(KeyCode::Up)), InputEvent::Ignored);
        a.cursor = a.buf.len();
        assert_eq!(press(&mut a, key_code(KeyCode::Down)), InputEvent::Ignored);
    }

    #[test]
    fn ctrl_p_and_ctrl_n_mirror_the_arrows() {
        let mut a = TextArea::new("aaa\nbbb");
        a.cursor = 1;
        assert_eq!(press(&mut a, key_ctrl('n')), InputEvent::Edited);
        assert_eq!(a.cursor, 5);
        assert_eq!(press(&mut a, key_ctrl('p')), InputEvent::Edited);
        assert_eq!(a.cursor, 1);
    }

    /// Ported from `text_input.rs`: the cursor must never land inside a char.
    #[test]
    fn multibyte_editing_stays_on_char_boundaries() {
        let mut a = TextArea::new("日本語");
        assert_eq!(
            press(&mut a, key_code(KeyCode::Backspace)),
            InputEvent::Edited
        );
        assert_eq!(a.value(), "日本");
        press(&mut a, key_code(KeyCode::Left));
        assert!(a.buf.is_char_boundary(a.cursor));
        assert_eq!(a.cursor, 3);
    }

    #[test]
    fn ctrl_w_joins_onto_the_previous_line() {
        let mut a = TextArea::new("foo\n");
        assert_eq!(press(&mut a, key_ctrl('w')), InputEvent::Edited);
        assert_eq!(a.value(), "");
    }

    #[test]
    fn desired_height_counts_wrapped_rows() {
        let a = TextArea::new("abcdef");
        assert_eq!(a.desired_height(3), 2);
        assert_eq!(a.desired_height(6), 1);
        // Never zero, so a caller always reserves a row.
        assert_eq!(TextArea::new("").desired_height(10), 1);
        assert_eq!(a.desired_height(0), 1);
    }

    #[test]
    fn desired_height_counts_explicit_newlines() {
        assert_eq!(TextArea::new("a\nb\nc").desired_height(10), 3);
    }

    #[test]
    fn render_wraps_at_width() {
        let mut a = TextArea::new("abcdef");
        a.cursor = 0;
        assert_eq!(render_to_lines(&a, 3, 2), vec!["abc", "def"]);
    }

    #[test]
    fn render_shows_explicit_lines() {
        let mut a = TextArea::new("hi\nthere");
        a.cursor = 0;
        assert_eq!(render_to_lines(&a, 10, 2), vec!["hi", "there"]);
    }

    #[test]
    fn render_scrolls_to_keep_the_cursor_visible() {
        // Four rows of content in a two-row window, cursor at the end.
        let a = TextArea::new("l1\nl2\nl3\nl4");
        assert_eq!(render_to_lines(&a, 10, 2), vec!["l3", "l4"]);
    }

    #[test]
    fn render_marks_the_cursor_cell() {
        let mut a = TextArea::new("ab");
        a.cursor = 1;
        let area = Rect::new(0, 0, 4, 1);
        let mut buf = Buffer::empty(area);
        render(area, &mut buf, &a, Style::default());
        let style = buf.cell(Position::new(1, 0)).unwrap().style();
        assert!(style.add_modifier.contains(Modifier::REVERSED));
    }
}
