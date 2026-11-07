//! Waiting on a terminal and the signal pipe together, and reading it a
//! byte at a time: what raw mode and line mode share.

use std::ffi::c_int;
use std::io;
use std::os::fd::BorrowedFd;
use std::time::Instant;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;

use super::signals;

pub(crate) enum Ready {
    /// A signal routed by [`signals::install`].
    Signal(c_int),
    /// Something to read (or the terminal hung up, which a read reports).
    Input,
    /// The deadline passed first.
    Timeout,
}

/// Wait until `fd` can be read, a signal arrives, or `deadline` (`None`:
/// no deadline). A signal is checked first, so one that fell between two
/// calls is not missed.
pub(crate) fn wait(fd: BorrowedFd<'_>, deadline: Option<Instant>) -> io::Result<Ready> {
    loop {
        if let Some(sig) = signals::take() {
            return Ok(Ready::Signal(sig));
        }
        let timeout = match deadline {
            None => None,
            Some(d) => Some(
                Timespec::try_from(d.saturating_duration_since(Instant::now()))
                    .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?,
            ),
        };
        let polled = match signals::wake_fd() {
            Some(wake) => {
                let mut fds = [
                    PollFd::new(&fd, PollFlags::IN),
                    PollFd::new(&wake, PollFlags::IN),
                ];
                poll(&mut fds, timeout.as_ref()).map(|n| (n, fds[0].revents()))
            }
            None => {
                let mut fds = [PollFd::new(&fd, PollFlags::IN)];
                poll(&mut fds, timeout.as_ref()).map(|n| (n, fds[0].revents()))
            }
        };
        let revents = match polled {
            Ok((0, _)) => return Ok(Ready::Timeout),
            Ok((_, r)) => r,
            Err(Errno::INTR) => continue,
            Err(e) => return Err(e.into()),
        };
        if revents.contains(PollFlags::NVAL) {
            return Err(Errno::BADF.into());
        }
        if revents.intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR) {
            return Ok(Ready::Input);
        }
        // Only the wake pipe: `take` reads it at the top.
    }
}

pub(crate) enum Byte {
    Got(u8),
    /// End of input, or the terminal went away (a pty whose other side
    /// closed reads as `EIO`).
    End,
}

/// One byte from `fd`, retrying an interrupted or spurious read.
pub(crate) fn read_byte(fd: BorrowedFd<'_>) -> io::Result<Option<Byte>> {
    let mut b = [0u8];
    match rustix::io::read(fd, &mut b) {
        Ok(1) => Ok(Some(Byte::Got(b[0]))),
        Ok(_) => Ok(Some(Byte::End)),
        Err(Errno::INTR | Errno::AGAIN) => Ok(None),
        Err(Errno::IO) => Ok(Some(Byte::End)),
        Err(e) => Err(e.into()),
    }
}
