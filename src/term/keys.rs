//! Bytes from a raw terminal into keys.
//!
//! Pure over a [`Source`], so every sequence is testable without a terminal;
//! [`super::Raw`] is the source that reads one. Whatever follows an ESC is
//! consumed whole (CSI to its final byte, SS3, the kitty keyboard protocol's
//! `CSI … u`), so a key the caller does not handle never leaks its tail as
//! typed text, into the prompt or the shell after it.
//!
//! Off Unix the types build (code that handles keys builds everywhere) but
//! nothing reads them.
#![cfg_attr(not(unix), allow(dead_code))]

use std::io;
use std::time::Duration;

use zeroize::Zeroize;

/// How long the bytes of one key may take to arrive. A bare Esc is an ESC
/// with nothing after it within this; so is a sequence cut short.
pub const ESC_WAIT: Duration = Duration::from_millis(100);

/// The longest CSI sequence taken; anything longer is consumed as unknown.
const CSI_MAX: usize = 64;

/// One key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A printable character, as typed (any script).
    Char(char),
    /// CR or LF: the terminal's Enter with `ICRNL` off, or a pasted newline.
    Enter,
    Tab,
    BackTab,
    /// DEL or BS.
    Backspace,
    Delete,
    Insert,
    /// ESC on its own.
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    /// A control key, by its letter: `Ctrl('c')` is 0x03. Arrives as a key
    /// because raw mode turns off what the terminal would do with it (ISIG,
    /// IXON, IEXTEN).
    Ctrl(char),
    /// ESC then a character.
    Alt(char),
    /// A sequence read whole that means nothing here (a function key, a
    /// mouse report, a key release, invalid UTF-8).
    Unknown,
}

/// What a read gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Key(Key),
    /// Nothing within the time asked for.
    Timeout,
    /// The terminal went away. Never a key, never a submit.
    Hangup,
    /// A signal arrived (installed with [`super::signals::install`]). The
    /// terminal has been restored already; erase what was drawn and
    /// [`super::signals::reraise`] it.
    Interrupted(i32),
}

/// One byte from the terminal, or why there is none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Byte {
    Got(u8),
    Timeout,
    Hangup,
    Signal(i32),
}

pub(crate) trait Source {
    /// The next byte, waiting at most `wait` (`None`: as long as it takes).
    fn byte(&mut self, wait: Option<Duration>) -> io::Result<Byte>;
}

/// A byte that is not the next part of a key: what to return instead.
fn interrupt(b: Byte) -> Option<Event> {
    match b {
        Byte::Signal(s) => Some(Event::Interrupted(s)),
        Byte::Hangup => Some(Event::Hangup),
        _ => None,
    }
}

/// The next key, waiting at most `wait` for its first byte.
pub(crate) fn decode(src: &mut impl Source, wait: Option<Duration>) -> io::Result<Event> {
    let b = match src.byte(wait)? {
        Byte::Got(b) => b,
        Byte::Timeout => return Ok(Event::Timeout),
        other => return Ok(interrupt(other).expect("not a byte")),
    };
    let key = match b {
        0x1b => return escape(src),
        b'\r' | b'\n' => Key::Enter,
        b'\t' => Key::Tab,
        0x7f | 0x08 => Key::Backspace,
        0x01..=0x1a => Key::Ctrl(char::from(b'a' + b - 1)),
        0x20..=0x7e => Key::Char(char::from(b)),
        0xc2..=0xf4 => return utf8(src, b),
        _ => Key::Unknown,
    };
    Ok(Event::Key(key))
}

fn escape(src: &mut impl Source) -> io::Result<Event> {
    let b = match src.byte(Some(ESC_WAIT))? {
        Byte::Got(b) => b,
        Byte::Timeout | Byte::Hangup => return Ok(Event::Key(Key::Esc)),
        other => return Ok(interrupt(other).expect("signal")),
    };
    let key = match b {
        b'[' => return csi(src),
        b'O' => return ss3(src),
        0x1b => Key::Esc,
        0x20..=0x7e => Key::Alt(char::from(b)),
        _ => Key::Unknown,
    };
    Ok(Event::Key(key))
}

/// `ESC [ params intermediates final`, read to its final byte.
fn csi(src: &mut impl Source) -> io::Result<Event> {
    let mut params = Vec::new();
    let fin = loop {
        let b = match src.byte(Some(ESC_WAIT))? {
            Byte::Got(b) => b,
            Byte::Timeout => return Ok(Event::Key(Key::Unknown)),
            other => return Ok(interrupt(other).expect("signal")),
        };
        if (0x40..=0x7e).contains(&b) {
            break b;
        }
        if params.len() < CSI_MAX {
            params.push(b);
        }
    };
    let params = String::from_utf8_lossy(&params).into_owned();
    let mut fields = params.split(';');
    let first = fields.next().unwrap_or("");
    let number = |s: &str| s.split(':').next().unwrap_or("").parse::<u32>().ok();
    let key = match fin {
        b'A' => Key::Up,
        b'B' => Key::Down,
        b'C' => Key::Right,
        b'D' => Key::Left,
        b'H' => Key::Home,
        b'F' => Key::End,
        b'Z' => Key::BackTab,
        b'~' => match number(first) {
            Some(1 | 7) => Key::Home,
            Some(2) => Key::Insert,
            Some(3) => Key::Delete,
            Some(4 | 8) => Key::End,
            Some(5) => Key::PageUp,
            Some(6) => Key::PageDown,
            _ => Key::Unknown,
        },
        b'u' => kitty(number(first), fields.next()),
        _ => Key::Unknown,
    };
    Ok(Event::Key(key))
}

/// The kitty keyboard protocol: `CSI code ; modifiers[:event] u`.
fn kitty(code: Option<u32>, modifiers: Option<&str>) -> Key {
    let mut m = modifiers.unwrap_or("1").split(':');
    let bits = m
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1)
        .saturating_sub(1);
    // 1 press, 2 repeat, 3 release: a release is not a key.
    if m.next() == Some("3") {
        return Key::Unknown;
    }
    let (alt, ctrl) = (bits & 2 != 0, bits & 4 != 0);
    match code {
        Some(13) => Key::Enter,
        Some(9) => Key::Tab,
        Some(27) => Key::Esc,
        Some(127) => Key::Backspace,
        Some(c) => match char::from_u32(c) {
            Some(c) if ctrl && c.is_ascii_alphabetic() => Key::Ctrl(c.to_ascii_lowercase()),
            Some(c) if alt && !c.is_control() => Key::Alt(c),
            Some(c) if !c.is_control() && !ctrl => Key::Char(c),
            _ => Key::Unknown,
        },
        None => Key::Unknown,
    }
}

/// `ESC O x`: arrows and Home/End in application mode.
fn ss3(src: &mut impl Source) -> io::Result<Event> {
    let key = match src.byte(Some(ESC_WAIT))? {
        Byte::Got(b'A') => Key::Up,
        Byte::Got(b'B') => Key::Down,
        Byte::Got(b'C') => Key::Right,
        Byte::Got(b'D') => Key::Left,
        Byte::Got(b'H') => Key::Home,
        Byte::Got(b'F') => Key::End,
        Byte::Got(_) | Byte::Timeout => Key::Unknown,
        other => return Ok(interrupt(other).expect("signal")),
    };
    Ok(Event::Key(key))
}

/// A multi-byte character: its continuation bytes, then the character as
/// typed. Assembled in a buffer that is wiped, since it may be a password's.
fn utf8(src: &mut impl Source, lead: u8) -> io::Result<Event> {
    let len = match lead {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    };
    let mut buf = [0u8; 4];
    buf[0] = lead;
    let mut result = Event::Key(Key::Unknown);
    let mut complete = true;
    for slot in buf.iter_mut().take(len).skip(1) {
        match src.byte(Some(ESC_WAIT))? {
            Byte::Got(b) if b & 0xc0 == 0x80 => *slot = b,
            Byte::Got(_) | Byte::Timeout => {
                complete = false;
                break;
            }
            other => {
                result = interrupt(other).expect("signal");
                complete = false;
                break;
            }
        }
    }
    if complete
        && let Some(c) = std::str::from_utf8(&buf[..len])
            .ok()
            .and_then(|s| s.chars().next())
    {
        result = Event::Key(Key::Char(c));
    }
    buf.zeroize();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Bytes as they arrive: `None` is a pause longer than [`ESC_WAIT`].
    struct Script(VecDeque<Option<u8>>);

    impl Source for Script {
        fn byte(&mut self, _wait: Option<Duration>) -> io::Result<Byte> {
            Ok(match self.0.pop_front() {
                Some(Some(b)) => Byte::Got(b),
                Some(None) => Byte::Timeout,
                None => Byte::Hangup,
            })
        }
    }

    fn keys(bytes: &[u8]) -> Vec<Event> {
        keys_paused(&bytes.iter().map(|&b| Some(b)).collect::<Vec<_>>())
    }

    fn keys_paused(bytes: &[Option<u8>]) -> Vec<Event> {
        let mut s = Script(bytes.iter().copied().collect());
        let mut out = Vec::new();
        loop {
            match decode(&mut s, None).unwrap() {
                Event::Hangup => return out,
                Event::Timeout => {}
                e => out.push(e),
            }
        }
    }

    fn k(key: Key) -> Event {
        Event::Key(key)
    }

    #[test]
    fn control_bytes_are_keys_not_terminal_actions() {
        assert_eq!(
            keys(b"a\x03\x13\x16b\r\n\x7f\x08\t\x15"),
            [
                k(Key::Char('a')),
                k(Key::Ctrl('c')),
                k(Key::Ctrl('s')),
                k(Key::Ctrl('v')),
                k(Key::Char('b')),
                k(Key::Enter),
                k(Key::Enter),
                k(Key::Backspace),
                k(Key::Backspace),
                k(Key::Tab),
                k(Key::Ctrl('u')),
            ]
        );
    }

    #[test]
    fn utf8_is_read_as_typed() {
        assert_eq!(
            keys("ł日🦀".as_bytes()),
            [k(Key::Char('ł')), k(Key::Char('日')), k(Key::Char('🦀'))]
        );
        // A lead byte without its continuation is dropped, not mangled.
        assert_eq!(keys(&[0xc5, b'a']), [k(Key::Unknown)]);
        assert_eq!(keys(&[0x80, 0xff]), [k(Key::Unknown), k(Key::Unknown)]);
    }

    #[test]
    fn escape_sequences_are_consumed_whole() {
        assert_eq!(
            keys(b"\x1b[A\x1b[1;5C\x1bOD\x1b[3~\x1b[5~\x1b[H\x1b[Zx"),
            [
                k(Key::Up),
                k(Key::Right),
                k(Key::Left),
                k(Key::Delete),
                k(Key::PageUp),
                k(Key::Home),
                k(Key::BackTab),
                k(Key::Char('x')),
            ]
        );
        // A function key and a mouse report mean nothing, and leave nothing.
        assert_eq!(
            keys(b"\x1b[15~\x1b[<0;10;5Mz"),
            [k(Key::Unknown), k(Key::Unknown), k(Key::Char('z'))]
        );
    }

    #[test]
    fn the_kitty_protocol_decodes_and_releases_are_dropped() {
        assert_eq!(
            keys(b"\x1b[97u\x1b[99;5u\x1b[13u\x1b[27u\x1b[97;1:3u\x1b[322u"),
            [
                k(Key::Char('a')),
                k(Key::Ctrl('c')),
                k(Key::Enter),
                k(Key::Esc),
                k(Key::Unknown),
                k(Key::Char('ł')),
            ]
        );
    }

    #[test]
    fn a_bare_escape_is_esc_and_a_cut_sequence_is_not_text() {
        assert_eq!(
            keys_paused(&[Some(0x1b), None, Some(b'a')]),
            [k(Key::Esc), k(Key::Char('a'))]
        );
        assert_eq!(keys(b"\x1b"), [k(Key::Esc)]);
        assert_eq!(keys(b"\x1bx"), [k(Key::Alt('x'))]);
        // ESC [ 1 then a pause: consumed as unknown, the next key is clean.
        assert_eq!(
            keys_paused(&[Some(0x1b), Some(b'['), Some(b'1'), None, Some(b'q')]),
            [k(Key::Unknown), k(Key::Char('q'))]
        );
    }

    #[test]
    fn a_signal_mid_key_is_reported() {
        struct Sig(u8);
        impl Source for Sig {
            fn byte(&mut self, _: Option<Duration>) -> io::Result<Byte> {
                self.0 += 1;
                Ok(if self.0 == 1 {
                    Byte::Got(0x1b)
                } else {
                    Byte::Signal(15)
                })
            }
        }
        assert_eq!(decode(&mut Sig(0), None).unwrap(), Event::Interrupted(15));
    }
}
