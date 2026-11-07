//! Raw mode, and the only way to read keys.

use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::time::{Duration, Instant};

use rustix::termios::{
    self, InputModes, LocalModes, OptionalActions, QueueSelector, SpecialCodeIndex, Termios,
};

use super::fdio::{self, Ready};
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
/// before an [`Event::Interrupted`] is returned, and by the signal thread on
/// a second signal. `restore` is the one to call before
/// `std::process::exit`, which runs no destructors.
pub struct Raw {
    fd: OwnedFd,
    orig: Termios,
    active: bool,
}

/// `termios` with askpass's raw settings applied.
fn raw_termios(orig: &Termios) -> Termios {
    let mut t = orig.clone();
    t.local_modes
        .remove(LocalModes::ECHO | LocalModes::ICANON | LocalModes::ISIG | LocalModes::IEXTEN);
    t.input_modes
        .remove(InputModes::IXON | InputModes::ICRNL | InputModes::INLCR);
    t.special_codes[SpecialCodeIndex::VMIN] = 1;
    t.special_codes[SpecialCodeIndex::VTIME] = 0;
    t
}

impl Raw {
    pub(crate) fn new(tty: BorrowedFd<'_>) -> io::Result<Raw> {
        let fd = tty.try_clone_to_owned()?;
        let orig = termios::tcgetattr(&fd)?;
        signals::arm(fd.as_fd(), &orig)?;
        if let Err(e) = termios::tcsetattr(&fd, OptionalActions::Now, &raw_termios(&orig)) {
            signals::disarm();
            return Err(e.into());
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
        let restored = termios::tcsetattr(&self.fd, OptionalActions::Now, &self.orig);
        let _ = termios::tcflush(&self.fd, QueueSelector::IFlush);
        signals::disarm();
        Ok(restored?)
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
        loop {
            match fdio::wait(self.fd.as_fd(), deadline)? {
                Ready::Signal(sig) => return Ok(Byte::Signal(sig)),
                Ready::Timeout => return Ok(Byte::Timeout),
                Ready::Input => {}
            }
            match fdio::read_byte(self.fd.as_fd())? {
                Some(fdio::Byte::Got(b)) => return Ok(Byte::Got(b)),
                // In raw mode a terminal read ends only on hangup.
                Some(fdio::Byte::End) => return Ok(Byte::Hangup),
                None => {}
            }
        }
    }
}
