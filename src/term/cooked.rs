//! A line in the terminal's own line mode: how a prompt asks on a terminal
//! that cannot be redrawn (`TERM=dumb`), the driver doing the editing.

use std::io;
use std::os::fd::BorrowedFd;

use rustix::termios::{self, InputModes, LocalModes, OptionalActions, QueueSelector};
use zeroize::Zeroizing;

use super::fdio::{self, Ready};
use super::signals;

/// How a line read ended.
#[derive(Debug)]
pub enum Line {
    /// Enter: the line, without its newline.
    Entered(Zeroizing<String>),
    /// A Ctrl-C or Esc in the line.
    Cancelled,
    /// The terminal went away (or end of input) before Enter.
    Hangup,
    /// A signal routed by [`super::signals::install`] came while reading:
    /// the terminal is back as it was and what was typed is dropped. The
    /// app ends the way the signal would ([`super::signals::reraise`]).
    Interrupted(i32),
}

/// Read one line from `fd` in canonical mode, `echo` as asked, `ISIG` off
/// (Ctrl-C is a character in the line, not a signal at a process), and put
/// the settings back. Without echo the newline is not echoed either, so a
/// caller drawing after a secret starts a line of its own.
///
/// The settings are changed, so signals are armed as for a raw terminal
/// ([`super::signals`]): the first wakes the read, which puts them back and
/// returns [`Line::Interrupted`]; a second restores them on the signal
/// thread.
pub(crate) fn read_line(fd: BorrowedFd<'_>, echo: bool) -> io::Result<Line> {
    let orig = termios::tcgetattr(fd)?;
    let mut t = orig.clone();
    t.local_modes.insert(LocalModes::ICANON);
    t.local_modes.remove(LocalModes::ISIG);
    t.local_modes.set(LocalModes::ECHO, echo);
    t.input_modes.insert(InputModes::ICRNL);
    signals::arm(fd, &orig)?;
    if let Err(e) = termios::tcsetattr(fd, OptionalActions::Now, &t) {
        signals::disarm();
        return Err(e.into());
    }
    let result = read_until_newline(fd);
    let _ = termios::tcsetattr(fd, OptionalActions::Now, &orig);
    if matches!(result, Ok(Read::Signal(_))) {
        // What was typed of the line is not left for the shell to read.
        let _ = termios::tcflush(fd, QueueSelector::IFlush);
    }
    signals::disarm();
    let mut bytes = match result? {
        Read::Signal(sig) => return Ok(Line::Interrupted(sig)),
        Read::Bytes(bytes) => bytes,
    };
    let line = match bytes.last() {
        Some(b'\n') => {
            bytes.pop();
            if bytes.iter().any(|&b| b == 0x03 || b == 0x1b) {
                Line::Cancelled
            } else {
                Line::Entered(Zeroizing::new(String::from_utf8_lossy(&bytes).into_owned()))
            }
        }
        _ => Line::Hangup,
    };
    zeroize::Zeroize::zeroize(&mut *bytes);
    Ok(line)
}

enum Read {
    Bytes(Zeroizing<Vec<u8>>),
    Signal(i32),
}

/// Bytes up to and with the newline (fewer on end of input or hangup),
/// waiting on the terminal and the signal pipe both, so a signal wakes it
/// however it falls between the calls.
fn read_until_newline(fd: BorrowedFd<'_>) -> io::Result<Read> {
    let mut out = Zeroizing::new(Vec::new());
    loop {
        match fdio::wait(fd, None)? {
            Ready::Signal(sig) => return Ok(Read::Signal(sig)),
            Ready::Timeout => continue,
            Ready::Input => {}
        }
        // Readable in canonical mode: a whole line (or its end) is there.
        match fdio::read_byte(fd)? {
            Some(fdio::Byte::Got(b)) => {
                out.push(b);
                if b == b'\n' {
                    return Ok(Read::Bytes(out));
                }
            }
            Some(fdio::Byte::End) => return Ok(Read::Bytes(out)),
            None => {}
        }
    }
}
