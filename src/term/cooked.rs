//! A line in the terminal's own line mode: how a prompt asks on a terminal
//! that cannot be redrawn (`TERM=dumb`), the driver doing the editing.

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd};

use zeroize::Zeroizing;

/// How a line read ended.
#[derive(Debug)]
pub enum Line {
    /// Enter: the line, without its newline.
    Entered(Zeroizing<String>),
    /// A Ctrl-C or Esc in the line.
    Cancelled,
    /// The terminal went away (or end of input) before Enter.
    Hangup,
}

/// Read one line from `fd` in canonical mode, `echo` as asked, `ISIG` off
/// (Ctrl-C is a character in the line, not a signal at a process), and put
/// the settings back. Without echo the newline is not echoed either, so a
/// caller drawing after a secret starts a line of its own.
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
    // SAFETY: as above.
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &t) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let result = read_until_newline(fd);
    // SAFETY: as above.
    unsafe { libc::tcsetattr(fd, libc::TCSANOW, &orig) };
    let mut bytes = result?;
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

fn read_until_newline(fd: i32) -> io::Result<Zeroizing<Vec<u8>>> {
    let mut out = Zeroizing::new(Vec::new());
    let mut b = 0u8;
    loop {
        // SAFETY: one byte into a local.
        match unsafe { libc::read(fd, (&raw mut b).cast(), 1) } {
            1 => {
                out.push(b);
                if b == b'\n' {
                    return Ok(out);
                }
            }
            0 => return Ok(out),
            _ => {
                let e = io::Error::last_os_error();
                match e.raw_os_error() {
                    // No terminal is raw here, so an installed signal takes
                    // its default action rather than waking anything.
                    Some(libc::EINTR) => continue,
                    Some(libc::EIO) => return Ok(out),
                    _ => return Err(e),
                }
            }
        }
    }
}
