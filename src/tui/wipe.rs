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
    let spare = buf.capacity() - len;
    buf.extend(std::iter::repeat_n('\0', spare));
    buf.truncate(len);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes past the end, up to the capacity. Only sound after
    /// `wipe_spare`, which leaves them initialized (and valid UTF-8).
    fn spare_bytes(buf: &mut String) -> Vec<u8> {
        let (len, cap) = (buf.len(), buf.capacity());
        // SAFETY: the allocation is `cap` bytes, all initialized by
        // `wipe_spare`; the borrowed `String` is forgotten, never dropped.
        let probe = std::mem::ManuallyDrop::new(unsafe {
            String::from_raw_parts(buf.as_mut_ptr(), cap, cap)
        });
        probe.as_bytes()[len..cap].to_vec()
    }

    #[test]
    fn removing_zeroes_what_it_removed() {
        let mut buf = String::with_capacity(32);
        buf.push_str("hunter2-secret");
        remove(&mut buf, 6..14);
        assert_eq!(buf, "hunter");
        assert!(spare_bytes(&mut buf).iter().all(|b| *b == 0));
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
