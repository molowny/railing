//! A line in the terminal's own line mode: how a prompt asks on a terminal
//! that cannot be redrawn (`TERM=dumb`), the driver doing the editing.

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd};

use zeroize::Zeroizing;

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
/// returns [`Line::Interrupted`]; a second restores them in the handler.
pub(crate) fn read_line(fd: BorrowedFd<'_>, echo: bool) -> io::Result<Line> {
    let fd = fd.as_raw_fd();
    // SAFETY: zeroed, then filled in.
    let mut orig: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: a valid fd and an out-pointer.
    if unsafe { libc::tcgetattr(fd, &mut orig) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut t = orig;
    t.c_lflag |= libc::ICANON;
    t.c_lflag &= !libc::ISIG;
    if echo {
        t.c_lflag |= libc::ECHO;
    } else {
        t.c_lflag &= !libc::ECHO;
    }
    t.c_iflag |= libc::ICRNL;
    signals::arm(fd, &orig)?;
    // SAFETY: as above.
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &t) } != 0 {
        let e = io::Error::last_os_error();
        signals::disarm();
        return Err(e);
    }
    let result = read_until_newline(fd);
    // SAFETY: as above.
    unsafe { libc::tcsetattr(fd, libc::TCSANOW, &orig) };
    if matches!(result, Ok(Read::Signal(_))) {
        // What was typed of the line is not left for the shell to read.
        // SAFETY: as above.
        unsafe { libc::tcflush(fd, libc::TCIFLUSH) };
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
fn read_until_newline(fd: i32) -> io::Result<Read> {
    let mut out = Zeroizing::new(Vec::new());
    let wake = signals::wake_fd();
    let mut b = 0u8;
    loop {
        if let Some(sig) = signals::take() {
            return Ok(Read::Signal(sig));
        }
        let mut fds = [
            libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake.unwrap_or(-1),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: two pollfds; a negative fd is ignored by poll.
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if fds[1].revents & libc::POLLIN != 0 {
            continue; // `take` reads it at the top
        }
        let r = fds[0].revents;
        if r & libc::POLLNVAL != 0 {
            return Err(io::Error::from_raw_os_error(libc::EBADF));
        }
        if r & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) == 0 {
            continue;
        }
        // Readable in canonical mode: a whole line (or its end) is there.
        // SAFETY: one byte into a local.
        match unsafe { libc::read(fd, (&raw mut b).cast(), 1) } {
            1 => {
                out.push(b);
                if b == b'\n' {
                    return Ok(Read::Bytes(out));
                }
            }
            0 => return Ok(Read::Bytes(out)),
            _ => {
                let e = io::Error::last_os_error();
                match e.raw_os_error() {
                    Some(libc::EINTR | libc::EAGAIN) => continue,
                    Some(libc::EIO) => return Ok(Read::Bytes(out)),
                    _ => return Err(e),
                }
            }
        }
    }
}
