//! A secret, typed with nothing drawn: the read loop.

use std::io;

use zeroize::Zeroizing;

use crate::term::{AltScreen, Event, Key, Raw};

/// What a secret prompt shows while it is typed.
///
/// Inline, on the primary screen, whatever is written stays in scrollback
/// and in anything logging the terminal, so a mask, a counter or even a
/// redraw per key would publish the secret's length and the rhythm it was
/// typed in. Inline there are only [`Echo::Silent`] and [`Echo::Presence`],
/// which learns one bit, whether anything is typed, and only when that bit
/// changes. A mask needs the alternate screen, held:
///
/// ```compile_fail
/// # use cli_kit::prompt::Echo;
/// # fn f(tty: &mut cli_kit::term::Tty) {
/// // A Tty is not an AltScreen: there is no way to mask inline.
/// let echo = Echo::Mask { screen: tty, paint: &mut |_, _| Ok(()) };
/// # }
/// ```
pub enum Echo<'a> {
    /// Nothing at all, not even a redraw: the caller's step stays as drawn.
    Silent,
    /// `paint(entered)` when the input goes from empty to typed (`true`) or
    /// back to empty (`false`), and at no other key: what a log of the
    /// terminal learns is that something was typed, and when the first key
    /// and the clear came, never how much or how fast. The callback is
    /// given the bit, nothing else, so it cannot draw more.
    Presence {
        paint: &'a mut dyn FnMut(bool) -> io::Result<()>,
    },
    /// `paint(screen, n)` after every change, `n` the characters typed.
    Mask {
        screen: &'a mut AltScreen,
        paint: &'a mut dyn FnMut(&mut AltScreen, usize) -> io::Result<()>,
    },
}

/// How a secret read ended.
#[derive(Debug)]
pub enum Secret {
    /// Enter, with something typed.
    Entered(Zeroizing<String>),
    /// Enter with nothing typed. Never a secret: the caller says why (an
    /// error state) or reads again.
    Empty,
    /// Esc or Ctrl-C.
    Cancelled,
    /// The terminal went away. Whatever was typed is dropped, never
    /// submitted.
    Hangup,
    /// A signal: the terminal is restored already. Erase what was drawn and
    /// `term::signals::reraise` it.
    Interrupted(i32),
}

/// Read a secret from `raw`.
///
/// Enter submits; Backspace deletes one character (not one byte); Ctrl-U
/// clears; Esc and Ctrl-C cancel. Every other key is swallowed, the arrows
/// and word jumps included: moving a cursor nobody can see would edit
/// blind. The typed characters live only in a buffer that is wiped when it
/// is dropped.
pub fn secret(raw: &mut Raw, mut echo: Echo<'_>) -> io::Result<Secret> {
    let mut typed = Zeroizing::new(String::new());
    // `was` is whether anything was typed before this key: Presence paints
    // only when that changes.
    let paint = |echo: &mut Echo<'_>, was: bool, n: usize| match echo {
        Echo::Silent => Ok(()),
        Echo::Presence { paint } if was != (n > 0) => paint(n > 0),
        Echo::Presence { .. } => Ok(()),
        Echo::Mask { screen, paint } => paint(screen, n),
    };
    if let Echo::Mask { screen, paint } = &mut echo {
        paint(screen, 0)?;
    }
    loop {
        match raw.read(None)? {
            Event::Key(Key::Enter) if typed.is_empty() => return Ok(Secret::Empty),
            Event::Key(Key::Enter) => return Ok(Secret::Entered(typed)),
            Event::Key(Key::Backspace) => {
                if typed.pop().is_some() {
                    paint(&mut echo, true, typed.chars().count())?;
                }
            }
            Event::Key(Key::Ctrl('u')) => {
                if !typed.is_empty() {
                    typed.clear();
                    paint(&mut echo, true, 0)?;
                }
            }
            Event::Key(Key::Char(c)) => {
                let was = !typed.is_empty();
                typed.push(c);
                paint(&mut echo, was, typed.chars().count())?;
            }
            Event::Key(Key::Esc | Key::Ctrl('c')) => return Ok(Secret::Cancelled),
            Event::Hangup => return Ok(Secret::Hangup),
            Event::Interrupted(sig) => return Ok(Secret::Interrupted(sig)),
            Event::Key(_) | Event::Timeout => {}
        }
    }
}
