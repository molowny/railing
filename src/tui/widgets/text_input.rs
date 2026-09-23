//! Single-line text input widget with optional masking.
//!
//! Used for inline edits inside a TUI row — the caller stamps a [`Rect`] over
//! the existing row, blanks it, and renders the input on top. The widget owns
//! a [`String`] buffer and a byte-index cursor that always sits on a UTF-8
//! char boundary.
//!
//! Bindings:
//!
//! - `Char(c)`: insert at cursor (newlines are rejected — the widget is
//!   single-line)
//! - `Backspace` / `Delete`
//! - `Left`, `Right`, `Home`, `End`
//! - `Ctrl-b`, `Ctrl-f`, `Ctrl-a`, `Ctrl-e`
//! - `Ctrl-w`: delete previous whitespace-delimited word
//! - `Ctrl-u`: clear the line
//! - `Enter`: emit [`InputEvent::Submit`]
//! - `Esc`: emit [`InputEvent::Cancel`]
//!
//! Security: the buffer is zeroized on `Drop`. On submit the caller pulls the
//! plaintext via [`TextInput::into_value`] (which `mem::take`s the buffer);
//! the moved-out `String` is then the caller's responsibility to zeroize.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use zeroize::Zeroize;

const MASK_CHAR: char = '\u{2022}';

/// Single-line text input with byte-indexed cursor.
pub struct TextInput {
    buf: String,
    cursor: usize,
    masked: bool,
}

/// Outcome of a single key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// User pressed Enter; caller should pull the value and apply.
    Submit,
    /// User pressed Esc; caller should drop the input.
    Cancel,
    /// Buffer or cursor changed.
    Edited,
    /// Key was not handled.
    Ignored,
}

impl TextInput {
    pub fn new(initial: impl Into<String>) -> Self {
        let buf: String = initial.into();
        let cursor = buf.len();
        Self {
            buf,
            cursor,
            masked: false,
        }
    }

    pub fn with_masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    pub fn value(&self) -> &str {
        &self.buf
    }

    /// Move the buffer out of this input. The original buffer is replaced
    /// with an empty `String`; the caller becomes responsible for zeroizing
    /// the returned plaintext when done.
    pub fn into_value(mut self) -> String {
        std::mem::take(&mut self.buf)
    }

    pub fn set_masked(&mut self, masked: bool) {
        self.masked = masked;
    }

    pub fn is_masked(&self) -> bool {
        self.masked
    }

    pub fn on_key(&mut self, key: KeyEvent) -> InputEvent {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl) {
            (KeyCode::Enter, _) => InputEvent::Submit,
            (KeyCode::Esc, _) => InputEvent::Cancel,

            (KeyCode::Char('a'), true) | (KeyCode::Home, _) => {
                if self.cursor == 0 {
                    InputEvent::Ignored
                } else {
                    self.cursor = 0;
                    InputEvent::Edited
                }
            }
            (KeyCode::Char('e'), true) | (KeyCode::End, _) => {
                if self.cursor == self.buf.len() {
                    InputEvent::Ignored
                } else {
                    self.cursor = self.buf.len();
                    InputEvent::Edited
                }
            }
            (KeyCode::Char('b'), true) | (KeyCode::Left, _) => {
                if self.move_left() {
                    InputEvent::Edited
                } else {
                    InputEvent::Ignored
                }
            }
            (KeyCode::Char('f'), true) | (KeyCode::Right, _) => {
                if self.move_right() {
                    InputEvent::Edited
                } else {
                    InputEvent::Ignored
                }
            }

            (KeyCode::Char('u'), true) => {
                if self.buf.is_empty() && self.cursor == 0 {
                    InputEvent::Ignored
                } else {
                    self.buf.zeroize();
                    self.cursor = 0;
                    InputEvent::Edited
                }
            }
            (KeyCode::Char('w'), true) => {
                if self.delete_prev_word() {
                    InputEvent::Edited
                } else {
                    InputEvent::Ignored
                }
            }

            (KeyCode::Backspace, _) => {
                if self.delete_prev_char() {
                    InputEvent::Edited
                } else {
                    InputEvent::Ignored
                }
            }
            (KeyCode::Delete, _) => {
                if self.delete_next_char() {
                    InputEvent::Edited
                } else {
                    InputEvent::Ignored
                }
            }

            (KeyCode::Char(c), false) | (KeyCode::Char(c), true)
                if !ctrl && c != '\n' && c != '\r' && !c.is_control() =>
            {
                self.insert_char(c);
                InputEvent::Edited
            }

            _ => InputEvent::Ignored,
        }
    }

    fn move_left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let mut i = self.cursor - 1;
        while i > 0 && !self.buf.is_char_boundary(i) {
            i -= 1;
        }
        self.cursor = i;
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
        self.buf.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    fn delete_prev_char(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let mut i = self.cursor - 1;
        while i > 0 && !self.buf.is_char_boundary(i) {
            i -= 1;
        }
        self.buf.replace_range(i..self.cursor, "");
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
        self.buf.replace_range(self.cursor..j, "");
        true
    }

    fn delete_prev_word(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let bytes = self.buf.as_bytes();
        let mut i = self.cursor;
        // Skip trailing whitespace.
        while i > 0 {
            let prev = prev_boundary(&self.buf, i);
            let ch = self.buf[prev..i].chars().next().unwrap();
            if !ch.is_whitespace() {
                break;
            }
            i = prev;
        }
        // Then consume non-whitespace.
        while i > 0 {
            let prev = prev_boundary(&self.buf, i);
            let ch = self.buf[prev..i].chars().next().unwrap();
            if ch.is_whitespace() {
                break;
            }
            i = prev;
        }
        let _ = bytes; // suppress unused warning if optimized
        if i == self.cursor {
            return false;
        }
        self.buf.replace_range(i..self.cursor, "");
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

impl Drop for TextInput {
    fn drop(&mut self) {
        self.buf.zeroize();
    }
}

/// Render `state` into `area`. The widget is single-line; only the first row
/// of `area` is touched. Long content scrolls horizontally so the cursor
/// stays visible at the right edge.
pub fn render(area: Rect, buf: &mut Buffer, state: &TextInput, style: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let y = area.y;
    let width = area.width as usize;

    // Build the visible string (masked or raw) along with the cursor's
    // visual column in the unscrolled string.
    let visible_full = if state.masked {
        let mut s = String::new();
        for c in state.buf.chars() {
            // Masked char width matches the bullet's display width (1 col),
            // independent of the original char's width. This is consistent
            // with how most UIs mask passwords.
            let _ = UnicodeWidthChar::width(c).unwrap_or(0);
            s.push(MASK_CHAR);
        }
        s
    } else {
        state.buf.clone()
    };

    let cursor_col_full: usize = if state.masked {
        // One column per char up to cursor.
        state.buf[..state.cursor].chars().count()
    } else {
        UnicodeWidthStr::width(&state.buf[..state.cursor])
    };

    // Horizontal scroll: keep cursor inside `[scroll, scroll + width)`.
    // Reserve one column for the cursor cell itself when at end-of-text.
    let total_w = if state.masked {
        visible_full.chars().count()
    } else {
        UnicodeWidthStr::width(visible_full.as_str())
    };
    let scroll = if cursor_col_full >= width {
        cursor_col_full + 1 - width
    } else {
        0
    };

    // Paint background.
    for col in 0..area.width {
        let cell = &mut buf[(area.x + col, y)];
        cell.set_char(' ');
        cell.set_style(style);
    }

    // Paint visible characters starting at logical column `scroll`.
    let mut col = 0u16;
    let mut logical_col: usize = 0;
    for ch in visible_full.chars() {
        let w = if state.masked {
            1
        } else {
            UnicodeWidthChar::width(ch).unwrap_or(0)
        };
        if logical_col + w > scroll && col < area.width {
            // Within the visible window.
            if logical_col >= scroll {
                let cell = &mut buf[(area.x + col, y)];
                cell.set_char(ch);
                cell.set_style(style);
                col = col.saturating_add(w as u16);
            } else {
                // Partially clipped wide char at the left edge — render a
                // placeholder space.
                let cell = &mut buf[(area.x + col, y)];
                cell.set_char(' ');
                cell.set_style(style);
                col += 1;
            }
        }
        logical_col += w;
        if col >= area.width {
            break;
        }
    }
    let _ = total_w;

    // Paint cursor as reverse-video on top of the content cell.
    let cursor_visible_col = cursor_col_full.saturating_sub(scroll) as u16;
    if cursor_visible_col < area.width {
        let cell = &mut buf[(area.x + cursor_visible_col, y)];
        let mut s = style;
        s = s.add_modifier(Modifier::REVERSED);
        cell.set_style(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::chord::{key_char, key_code, key_ctrl};
    use crossterm::event::KeyCode;
    use ratatui::layout::Position;

    fn render_to_string(state: &TextInput, width: u16) -> String {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        render(area, &mut buf, state, Style::default());
        let mut out = String::new();
        for cx in 0..width {
            out.push_str(buf.cell(Position::new(cx, 0)).unwrap().symbol());
        }
        out.trim_end().to_string()
    }

    #[test]
    fn new_seeds_buffer_and_places_cursor_at_end() {
        let ti = TextInput::new("hello");
        assert_eq!(ti.value(), "hello");
        assert_eq!(ti.cursor, 5);
    }

    #[test]
    fn insert_and_delete_at_cursor() {
        let mut ti = TextInput::new("");
        assert_eq!(ti.on_key(key_char('a')), InputEvent::Edited);
        assert_eq!(ti.on_key(key_char('b')), InputEvent::Edited);
        assert_eq!(ti.on_key(key_char('c')), InputEvent::Edited);
        assert_eq!(ti.value(), "abc");
        assert_eq!(ti.on_key(key_code(KeyCode::Backspace)), InputEvent::Edited);
        assert_eq!(ti.value(), "ab");
    }

    #[test]
    fn cursor_movement() {
        let mut ti = TextInput::new("abc");
        assert_eq!(ti.on_key(key_ctrl('a')), InputEvent::Edited);
        assert_eq!(ti.cursor, 0);
        assert_eq!(ti.on_key(key_ctrl('f')), InputEvent::Edited);
        assert_eq!(ti.cursor, 1);
        assert_eq!(ti.on_key(key_ctrl('e')), InputEvent::Edited);
        assert_eq!(ti.cursor, 3);
        assert_eq!(ti.on_key(key_ctrl('b')), InputEvent::Edited);
        assert_eq!(ti.cursor, 2);
        assert_eq!(ti.on_key(key_code(KeyCode::Home)), InputEvent::Edited);
        assert_eq!(ti.cursor, 0);
        assert_eq!(ti.on_key(key_code(KeyCode::End)), InputEvent::Edited);
        assert_eq!(ti.cursor, 3);
    }

    #[test]
    fn ctrl_w_deletes_previous_word() {
        let mut ti = TextInput::new("hello world  ");
        assert_eq!(ti.on_key(key_ctrl('w')), InputEvent::Edited);
        assert_eq!(ti.value(), "hello ");
        assert_eq!(ti.on_key(key_ctrl('w')), InputEvent::Edited);
        assert_eq!(ti.value(), "");
        assert_eq!(ti.on_key(key_ctrl('w')), InputEvent::Ignored);
    }

    #[test]
    fn ctrl_u_clears_line() {
        let mut ti = TextInput::new("abcdef");
        assert_eq!(ti.on_key(key_ctrl('u')), InputEvent::Edited);
        assert_eq!(ti.value(), "");
        assert_eq!(ti.cursor, 0);
        assert_eq!(ti.on_key(key_ctrl('u')), InputEvent::Ignored);
    }

    #[test]
    fn enter_submits_esc_cancels() {
        let mut ti = TextInput::new("x");
        assert_eq!(ti.on_key(key_code(KeyCode::Enter)), InputEvent::Submit);
        assert_eq!(ti.on_key(key_code(KeyCode::Esc)), InputEvent::Cancel);
    }

    #[test]
    fn newline_char_is_rejected() {
        let mut ti = TextInput::new("");
        assert_eq!(ti.on_key(key_char('\n')), InputEvent::Ignored);
        assert_eq!(ti.value(), "");
    }

    #[test]
    fn utf8_insert_and_backspace_keeps_boundaries() {
        let mut ti = TextInput::new("");
        for c in "héllo🦀".chars() {
            ti.on_key(key_char(c));
        }
        assert_eq!(ti.value(), "héllo🦀");
        assert_eq!(ti.cursor, ti.value().len());
        ti.on_key(key_code(KeyCode::Backspace));
        assert_eq!(ti.value(), "héllo");
        ti.on_key(key_code(KeyCode::Home));
        ti.on_key(key_code(KeyCode::Right));
        ti.on_key(key_code(KeyCode::Right));
        // After moving past 'h' and 'é', cursor should be on a boundary.
        assert!(ti.value().is_char_boundary(ti.cursor));
    }

    #[test]
    fn into_value_empties_the_buffer() {
        let mut ti = TextInput::new("secret");
        // Force a known cursor position to verify Drop runs cleanly.
        ti.cursor = 0;
        let v = ti.into_value();
        assert_eq!(v, "secret");
    }

    #[test]
    fn render_shows_plain_text() {
        let ti = TextInput::new("hi");
        let s = render_to_string(&ti, 10);
        assert!(s.starts_with("hi"), "got {s:?}");
    }

    #[test]
    fn render_masks_when_masked() {
        let ti = TextInput::new("secret").with_masked(true);
        let s = render_to_string(&ti, 10);
        // Six bullets at the start.
        let bullets: String = std::iter::repeat_n(MASK_CHAR, 6).collect();
        assert!(s.starts_with(&bullets), "got {s:?}");
        // Real value still preserved.
        assert_eq!(ti.value(), "secret");
    }

    #[test]
    fn render_scrolls_to_keep_cursor_visible() {
        let mut ti = TextInput::new("abcdefghij");
        ti.cursor = ti.buf.len();
        let s = render_to_string(&ti, 5);
        // With width 5, cursor at position 10, scroll = 6 → visible = "ghij".
        assert!(s.starts_with("ghij"), "got {s:?}");
    }
}
