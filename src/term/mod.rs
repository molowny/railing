//! The terminal: one that the caller names, its caps and size, raw mode, and
//! keys.
//!
//! Nothing here reaches a terminal the caller did not name. There is no
//! implicit stdin, stdout or `/dev/tty`: [`Tty::open`] takes a path,
//! [`Tty::from_fd`] a descriptor, and [`Tty::stderr`] is the one named
//! constructor for the terminal a command inherited. A helper drawing on
//! another session's terminal (askpass on `$SEALWARD_TTY`, with no
//! controlling terminal and its stdio on pipes) uses the same API as a
//! command drawing on its own stderr.
//!
//! ```no_run
//! use cli_kit::style::TermEnv;
//! use cli_kit::term::{Event, Key, Tty};
//!
//! let tty = Tty::open("/dev/pts/3", &TermEnv::of_process())?;
//! let mut raw = tty.raw()?;
//! loop {
//!     match raw.read(None)? {
//!         Event::Key(Key::Enter) => break,
//!         Event::Key(key) => println!("{key:?}"),
//!         _ => break,
//!     }
//! }
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! Unix only. Elsewhere this module is a stub with no [`Tty`]
//! ([`SUPPORTED`] is false), and asking interactively is a usage error.

#[cfg(unix)]
mod alt;
#[cfg(unix)]
mod cooked;
#[cfg(unix)]
mod fdio;
// The key types (and their decoder, which is pure) everywhere, so code that
// handles keys builds off Unix; only reading them needs a terminal.
mod keys;
#[cfg(unix)]
mod raw;
#[cfg(unix)]
pub mod signals;

#[cfg(unix)]
pub use alt::AltScreen;
#[cfg(unix)]
pub use cooked::Line;
pub use keys::{ESC_WAIT, Event, Key};
#[cfg(unix)]
pub use raw::Raw;

/// Whether this platform has interactive terminals (`Tty`, raw mode, keys).
pub const SUPPORTED: bool = cfg!(unix);

/// A terminal's size in cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub rows: u16,
    pub cols: u16,
}

#[cfg(unix)]
pub use unix::{Tty, Writer};

#[cfg(unix)]
mod unix {
    use std::fs::File;
    use std::io::{self, IsTerminal, Write};
    use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
    use std::path::Path;

    use rustix::fs::{Mode, OFlags};

    use super::{AltScreen, Raw, Size};
    use crate::style::{Caps, TermEnv};

    /// A terminal to draw on and read from, and what it can do.
    #[derive(Debug)]
    pub struct Tty {
        file: File,
        /// Where keys are read when it is not `file`: stdin, for a command
        /// drawing on its stderr (see [`Tty::stderr`]).
        input: Option<File>,
        caps: Caps,
    }

    impl Tty {
        /// The terminal at `path`, with caps for it from `env`: the
        /// environment of whoever owns that terminal.
        ///
        /// Opened with `O_NOCTTY`, so it never becomes this process's
        /// controlling terminal by being opened.
        pub fn open(path: impl AsRef<Path>, env: &TermEnv) -> io::Result<Tty> {
            let fd = rustix::fs::open(
                path.as_ref(),
                OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
                Mode::empty(),
            )?;
            Ok(Self::from_fd(fd, env))
        }

        /// A terminal (or anything else) already open as `fd`.
        pub fn from_fd(fd: OwnedFd, env: &TermEnv) -> Tty {
            Self::from_file(File::from(fd), env)
        }

        /// The terminal this command inherited on stderr, with caps from
        /// this process's environment: the one named way to it.
        ///
        /// It is drawn on through stderr. Keys are read from stdin when stdin
        /// is that same terminal (stderr may be open for writing only, as a
        /// shell's `2>/dev/tty` leaves it), and from stderr otherwise.
        pub fn stderr() -> io::Result<Tty> {
            let fd = io::stderr().as_fd().try_clone_to_owned()?;
            let mut tty = Self::from_fd(fd, &TermEnv::of_process());
            let stdin = io::stdin();
            if same_terminal(stdin.as_fd(), tty.file.as_fd()) {
                tty.input = Some(File::from(stdin.as_fd().try_clone_to_owned()?));
            }
            Ok(tty)
        }

        fn from_file(file: File, env: &TermEnv) -> Tty {
            let caps = Caps::detect(is_terminal(file.as_fd()), env);
            Tty {
                file,
                input: None,
                caps,
            }
        }

        fn input_fd(&self) -> BorrowedFd<'_> {
            self.input.as_ref().unwrap_or(&self.file).as_fd()
        }

        /// What this terminal can be asked to do.
        pub fn caps(&self) -> Caps {
            self.caps
        }

        /// The same terminal with `caps` decided by the caller instead (a
        /// `--color` flag, a test).
        pub fn with_caps(mut self, caps: Caps) -> Tty {
            self.caps = caps;
            self
        }

        pub fn is_terminal(&self) -> bool {
            is_terminal(self.file.as_fd())
        }

        /// Its size, from the terminal itself (`TIOCGWINSZ`). `None` when it
        /// cannot say: there is no guessed 80×24, and no `$COLUMNS`. What to
        /// do without a size is the caller's call.
        pub fn size(&self) -> Option<Size> {
            let ws = rustix::termios::tcgetwinsize(&self.file).ok()?;
            (ws.ws_row > 0 && ws.ws_col > 0).then_some(Size {
                rows: ws.ws_row,
                cols: ws.ws_col,
            })
        }

        /// Raw mode and the key reader: see [`Raw`]. One at a time per
        /// process.
        pub fn raw(&self) -> io::Result<Raw> {
            Raw::new(self.input_fd())
        }

        /// One line in the terminal's own line mode, for a terminal that
        /// cannot be redrawn: the driver edits (Backspace, Ctrl-U) and echoes
        /// (unless `echo` is off, for a secret). Signal keys are characters
        /// here too: a Ctrl-C (or Esc) in the line cancels it. An installed
        /// signal ends the read as it ends a raw one ([`signals`]); like
        /// [`Tty::raw`], one at a time per process.
        pub fn read_line(&self, echo: bool) -> io::Result<super::Line> {
            super::cooked::read_line(self.input_fd(), echo)
        }

        /// The alternate screen, entered now; see [`AltScreen`].
        pub fn alt_screen(&self) -> io::Result<AltScreen> {
            AltScreen::enter(self.writer()?)
        }

        /// A second handle that only writes, for another thread (a spinner)
        /// to draw on the same terminal. One writer at a time is the
        /// caller's to keep.
        pub fn writer(&self) -> io::Result<Writer> {
            Ok(Writer(self.file.try_clone()?))
        }
    }

    impl Write for Tty {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.file.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.file.flush()
        }
    }

    impl AsFd for Tty {
        fn as_fd(&self) -> BorrowedFd<'_> {
            self.file.as_fd()
        }
    }

    /// A write-only handle on a [`Tty`]: [`Tty::writer`].
    #[derive(Debug)]
    pub struct Writer(File);

    impl Write for Writer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.0.flush()
        }
    }

    /// Whether two descriptors are the same terminal device.
    fn same_terminal(a: BorrowedFd<'_>, b: BorrowedFd<'_>) -> bool {
        let dev = |fd: BorrowedFd<'_>| {
            is_terminal(fd)
                .then(|| rustix::fs::fstat(fd).ok())
                .flatten()
                .map(|st| st.st_rdev)
        };
        dev(a).is_some_and(|d| Some(d) == dev(b))
    }

    fn is_terminal(fd: BorrowedFd<'_>) -> bool {
        fd.is_terminal()
    }
}
