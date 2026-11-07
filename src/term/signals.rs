//! Signals that wake a key read instead of killing a raw terminal.
//!
//! Opt-in: nothing here runs until an app calls [`install`] (cli-kit is a
//! library, and signal disposition is the app's). Once installed, each
//! signal is received on a thread of its own (signal-hook's self-pipe, read
//! by a thread named `cli-kit-signals`), so everything below is ordinary
//! code, not a signal handler:
//!
//! - **While a terminal is raw** (a [`super::Raw`] is alive), the first
//!   signal is handed to the read: the thread writes it to a wake pipe, the
//!   read is polling that pipe beside the terminal, so it wakes at once (no
//!   waiting for a key), restores the terminal and returns
//!   [`super::Event::Interrupted`]. The app erases what it drew and calls
//!   [`reraise`].
//! - **While a line is read in the terminal's own line mode**
//!   ([`super::Tty::read_line`], which turns echo and `ISIG` off), the same:
//!   the read wakes, puts the settings back and returns
//!   [`super::Line::Interrupted`].
//! - **A second signal while raw** means the app did not get there: the
//!   thread restores the terminal itself and takes the default action. The
//!   user's shell never comes back without echo.
//! - **While no terminal is raw**, the default action, at once, as if
//!   nothing were installed.
//!
//! SIGKILL cannot be caught. A parent that wants its child's terminal back
//! sends SIGTERM first.

use std::ffi::c_int;
use std::io::{self, PipeReader, PipeWriter, Read as _, Write as _};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use rustix::termios::{self, OptionalActions, Termios};
use signal_hook::iterator::{Handle, Signals};
use signal_hook::low_level::emulate_default_handler;

/// The signals an app usually routes here, so it needs no libc of its own.
pub use signal_hook::consts::{SIGHUP, SIGINT, SIGPIPE, SIGTERM};

/// The wake pipe: the signal thread writes the signal's number, the read
/// polls the other end (non-blocking, so draining it never waits).
struct Wake {
    r: PipeReader,
    w: PipeWriter,
}
static WAKE: OnceLock<Wake> = OnceLock::new();

/// The signal thread's registration, once [`install`] has run: a later call
/// adds its signals to it instead of starting a second thread.
static INSTALLED: Mutex<Option<Handle>> = Mutex::new(None);

/// The terminal that is raw (or reading a line), and how to put it back.
struct Armed {
    fd: OwnedFd,
    termios: Termios,
    /// A signal has been handed to the read and not yet acted on.
    pending: bool,
}
static ARMED: Mutex<Option<Armed>> = Mutex::new(None);

/// A lock that a panic elsewhere does not take away: what it guards is
/// whole after every write.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Route `signals` through the wake pipe: what a raw terminal's read wakes
/// on. Typically [`SIGINT`], [`SIGTERM`], [`SIGHUP`] (and [`SIGPIPE`] for a
/// helper whose parent may go away). Safe to call more than once. Signals
/// that cannot be handled (SIGKILL, SIGSTOP, SIGSEGV, SIGILL, SIGFPE) are
/// an error.
pub fn install(signals: &[c_int]) -> io::Result<()> {
    let wake = wake()?;
    let mut installed = lock(&INSTALLED);
    if let Some(handle) = installed.as_ref() {
        for &sig in signals {
            handle.add_signal(sig)?;
        }
        return Ok(());
    }
    let mut sigs = Signals::new(signals)?;
    *installed = Some(sigs.handle());
    std::thread::Builder::new()
        .name("cli-kit-signals".into())
        .spawn(move || {
            for sig in sigs.forever() {
                deliver(wake, sig);
            }
        })?;
    Ok(())
}

/// What one signal does, on the signal thread.
fn deliver(wake: &Wake, sig: c_int) {
    let mut armed = lock(&ARMED);
    if let Some(a) = armed.as_mut() {
        if !a.pending {
            a.pending = true;
            // Signal numbers fit a byte.
            let _ = (&wake.w).write(&[sig as u8]);
            return;
        }
        let _ = termios::tcsetattr(&a.fd, OptionalActions::Now, &a.termios);
    }
    drop(armed);
    // Ends the process for any signal whose default is to (unblocked and
    // raised with its handler reset); returns for one ignored by default.
    let _ = emulate_default_handler(sig);
}

/// End the process the way `sig` would have: the default action, so the
/// parent sees the right exit status.
pub fn reraise(sig: c_int) -> ! {
    let _ = emulate_default_handler(sig);
    // A signal whose default is to be ignored: exit as a shell reports it.
    std::process::exit(128 + sig)
}

fn wake() -> io::Result<&'static Wake> {
    if let Some(w) = WAKE.get() {
        return Ok(w);
    }
    // Close-on-exec, from std.
    let (r, w) = std::io::pipe()?;
    rustix::io::ioctl_fionbio(&r, true)?;
    // Two racing first calls: one pipe is kept, the other dropped.
    Ok(WAKE.get_or_init(|| Wake { r, w }))
}

/// The read end, when [`install`] has run.
pub(crate) fn wake_fd() -> Option<BorrowedFd<'static>> {
    WAKE.get().map(|w| w.r.as_fd())
}

/// The signal waiting in the pipe, if any; the pipe is left empty.
pub(crate) fn take() -> Option<c_int> {
    let wake = WAKE.get()?;
    let mut last = None;
    let mut b = [0u8];
    while let Ok(1) = (&wake.r).read(&mut b) {
        last = Some(c_int::from(b[0]));
    }
    last
}

/// A terminal goes raw: from now on signals are handed to its read, and a
/// second one restores `termios` on `fd` before dying.
pub(crate) fn arm(fd: BorrowedFd<'_>, termios: &Termios) -> io::Result<()> {
    let mut armed = lock(&ARMED);
    if armed.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "another terminal is already raw (or reading a line) in this process",
        ));
    }
    // A signal left over from an earlier read that was not acted on.
    let _ = take();
    *armed = Some(Armed {
        fd: fd.try_clone_to_owned()?,
        termios: termios.clone(),
        pending: false,
    });
    Ok(())
}

pub(crate) fn disarm() {
    *lock(&ARMED) = None;
}
