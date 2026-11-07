//! Editing a `String` without leaving copies of it behind.
//!
//! The input widgets zeroize their buffer on drop, but two ordinary `String`
//! operations leave plaintext elsewhere first: an insert that outgrows the
//! capacity reallocates and frees the old allocation as it was, and a
//! deletion leaves the removed bytes in the spare capacity past the new end.
//! These helpers are the only way the widgets change their buffers.

use std::ops::Range;

use zeroize::Zeroize;

/// Insert `c` at byte `at`. Growing, the buffer is moved by hand so the old
/// allocation can be wiped before it is freed.
pub(crate) fn insert(buf: &mut String, at: usize, c: char) {
    if buf.len() + c.len_utf8() > buf.capacity() {
        let mut grown = String::with_capacity((buf.capacity() * 2).max(buf.len() + 8).max(16));
        grown.push_str(&buf[..at]);
        grown.push(c);
        grown.push_str(&buf[at..]);
        buf.zeroize();
        *buf = grown;
    } else {
        buf.insert(at, c);
    }
}

/// Remove byte `range`, then zero what it leaves past the new end.
pub(crate) fn remove(buf: &mut String, range: Range<usize>) {
    buf.replace_range(range, "");
    wipe_spare(buf);
}

/// Zero the spare capacity: fill it with NUL bytes — valid UTF-8, and
/// exactly the capacity, so nothing reallocates — then cut them off again.
fn wipe_spare(buf: &mut String) {
    let len = buf.len();
    fill_spare(buf);
    buf.truncate(len);
}

/// The first half of [`wipe_spare`]: the NULs written, not yet cut off.
fn fill_spare(buf: &mut String) {
    let spare = buf.capacity() - buf.len();
    buf.extend(std::iter::repeat_n('\0', spare));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_zeroes_what_it_removed() {
        // The spare capacity cannot be read back without `unsafe`, so the
        // wipe is checked in its two halves: the fill overwrites every byte
        // up to the capacity, in the same allocation, and the cut that
        // follows leaves the text and that allocation as they are.
        let mut buf = String::with_capacity(32);
        buf.push_str("hunter2-secret");
        buf.replace_range(6..14, "");
        let (ptr, cap) = (buf.as_ptr(), buf.capacity());
        fill_spare(&mut buf);
        assert_eq!((buf.as_ptr(), buf.capacity(), buf.len()), (ptr, cap, cap));
        assert!(buf.as_bytes()[6..].iter().all(|b| *b == 0));

        let mut buf = String::with_capacity(32);
        buf.push_str("hunter2-secret");
        let (ptr, cap) = (buf.as_ptr(), buf.capacity());
        remove(&mut buf, 6..14);
        assert_eq!(buf, "hunter");
        assert_eq!((buf.as_ptr(), buf.capacity()), (ptr, cap));
    }

    #[test]
    fn growing_keeps_the_text_and_room_for_more() {
        let mut buf = String::with_capacity(4);
        buf.push_str("abcd");
        insert(&mut buf, 2, 'X');
        assert_eq!(buf, "abXcd");
        assert!(buf.capacity() >= 16);
        insert(&mut buf, 0, '\u{00E9}');
        assert_eq!(buf, "\u{00E9}abXcd");
    }
}
