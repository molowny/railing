//! Raw mode, and the only way to read keys.

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
use std::time::{Duration, Instant};

use super::keys::{self, Byte, Event, Source};
use super::signals;

/// A terminal in raw mode, reading keys. It holds a read handle only: there
/// is no way to write through it, so a loop given just a `Raw` (a secret
/// prompt's) cannot draw what is typed.
///
/// The settings are askpass's hardened ones:
///
/// - `ECHO`, `ICANON` off: keys one at a time, nothing echoed.
/// - `ISIG` off: Ctrl-C, Ctrl-\ and Ctrl-Z are keys, not signals at the
///   process (which, on a terminal another session owns, would kill a
///   client that is not even this process).
/// - `IEXTEN` off: Ctrl-V does not swallow the key after it.
/// - `IXON` off: Ctrl-S does not freeze the terminal's output.
/// - `ICRNL`, `INLCR` off: Enter arrives as the byte typed.
/// - `VMIN=1`, `VTIME=0`: a read returns as soon as one byte is there.
///
/// Restored, with typed-ahead input flushed, by [`Raw::restore`], on drop,
/// before an [`Event::Interrupted`] is returned, and by the signal handler
/// on a second signal. `restore` is the one to call before
/// `std::process::exit`, which runs no destructors.
pub struct Raw {
    fd: OwnedFd,
    orig: libc::termios,
    active: bool,
}

/// `termios` with askpass's raw settings applied.
pub(crate) fn raw_termios(orig: &libc::termios) -> libc::termios {
    let mut t = *orig;
    t.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN);
    t.c_iflag &= !(libc::IXON | libc::ICRNL | libc::INLCR);
    t.c_cc[libc::VMIN] = 1;
    t.c_cc[libc::VTIME] = 0;
    t
}

impl Raw {
    pub(crate) fn new(tty: BorrowedFd<'_>) -> io::Result<Raw> {
        let fd = tty.try_clone_to_owned()?;
        // SAFETY: a zeroed termios is overwritten by tcgetattr.
        let mut orig: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: a valid fd and an out-pointer.
        if unsafe { libc::tcgetattr(fd.as_raw_fd(), &mut orig) } != 0 {
            return Err(io::Error::last_os_error());
        }
        signals::arm(fd.as_raw_fd(), &orig)?;
        let raw = raw_termios(&orig);
        // SAFETY: as above.
        if unsafe { libc::tcsetattr(fd.as_raw_fd(), libc::TCSANOW, &raw) } != 0 {
            let e = io::Error::last_os_error();
            signals::disarm();
            return Err(e);
        }
        Ok(Raw {
            fd,
            orig,
            active: true,
        })
    }

    /// The next key, waiting at most `timeout` for it (`None`: until one
    /// comes, the terminal goes away, or a signal arrives).
    pub fn read(&mut self, timeout: Option<Duration>) -> io::Result<Event> {
        let event = keys::decode(self, timeout)?;
        if let Event::Interrupted(_) = event {
            self.restore()?;
        }
        Ok(event)
    }

    /// Put the terminal back as it was, dropping typed-ahead input. Once:
    /// later calls do nothing.
    pub fn restore(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        // SAFETY: a valid fd and the termios read from it.
        let rc = unsafe { libc::tcsetattr(self.fd.as_raw_fd(), libc::TCSANOW, &self.orig) };
        let e = io::Error::last_os_error();
        // SAFETY: as above.
        unsafe { libc::tcflush(self.fd.as_raw_fd(), libc::TCIFLUSH) };
        signals::disarm();
        if rc != 0 { Err(e) } else { Ok(()) }
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

impl Source for Raw {
    fn byte(&mut self, wait: Option<Duration>) -> io::Result<Byte> {
        let deadline = wait.map(|w| Instant::now() + w);
        let wake = signals::wake_fd();
        loop {
            if let Some(sig) = signals::take() {
                return Ok(Byte::Signal(sig));
            }
            let mut fds = [
                libc::pollfd {
                    fd: self.fd.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: wake.unwrap_or(-1),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let ms = match deadline {
                None => -1,
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    i32::try_from(left.as_millis()).unwrap_or(i32::MAX)
                }
            };
            // SAFETY: two pollfds; a negative fd is ignored by poll.
            let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, ms) };
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            if n == 0 {
                return Ok(Byte::Timeout);
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
            let mut b = 0u8;
            // SAFETY: one byte into a local.
            match unsafe { libc::read(self.fd.as_raw_fd(), (&raw mut b).cast(), 1) } {
                1 => return Ok(Byte::Got(b)),
                // In raw mode a terminal read gives 0 only on hangup.
                0 => return Ok(Byte::Hangup),
                _ => {
                    let e = io::Error::last_os_error();
                    match e.raw_os_error() {
                        Some(libc::EINTR | libc::EAGAIN) => continue,
                        // A pty whose other side closed.
                        Some(libc::EIO) => return Ok(Byte::Hangup),
                        _ => return Err(e),
                    }
                }
            }
        }
    }
}
