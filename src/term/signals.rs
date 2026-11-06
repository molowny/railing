//! Signals that wake a key read instead of killing a raw terminal.
//!
//! Opt-in: nothing here runs until an app calls [`install`] (cli-kit is a
//! library, and signal disposition is the app's). Once installed:
//!
//! - **While a terminal is raw** (a [`super::Raw`] is alive), the first
//!   signal is handed to the read: the handler writes it to a self-pipe, the
//!   read is polling that pipe beside the terminal, so it wakes at once
//!   (no `SA_RESTART`, no waiting for a key), restores the terminal and
//!   returns [`super::Event::Interrupted`]. The app erases what it drew and
//!   calls [`reraise`].
//! - **While a line is read in the terminal's own line mode**
//!   ([`super::Tty::read_line`], which turns echo and `ISIG` off), the same:
//!   the read wakes, puts the settings back and returns
//!   [`super::Line::Interrupted`].
//! - **A second signal while raw** means the app did not get there: the
//!   handler restores the terminal itself (`tcsetattr` is async-signal-safe)
//!   and takes the default action. The user's shell never comes back
//!   without echo.
//! - **While no terminal is raw**, the default action, at once, as if
//!   nothing were installed.
//!
//! SIGKILL cannot be caught. A parent that wants its child's terminal back
//! sends SIGTERM first.

use std::io;
use std::os::fd::RawFd;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use libc::c_int;

/// The self-pipe: the handler writes the signal's number, the read polls.
static PIPE: OnceLock<(RawFd, RawFd)> = OnceLock::new();
/// The write end, for the handler (a plain load, async-signal-safe).
static WAKE_W: AtomicI32 = AtomicI32::new(-1);

/// A terminal is raw: signals go to its read.
static ARMED: AtomicBool = AtomicBool::new(false);
/// A signal has been handed to the read and not yet acted on.
static PENDING: AtomicBool = AtomicBool::new(false);

/// What the handler restores on a second signal. Written only while not
/// armed, read only while armed, so the handler never sees it half-written.
struct Saved {
    fd: std::cell::UnsafeCell<RawFd>,
    termios: std::cell::UnsafeCell<std::mem::MaybeUninit<libc::termios>>,
}
// SAFETY: access is ordered by ARMED (see `arm`, `disarm`, `handler`).
unsafe impl Sync for Saved {}
static SAVED: Saved = Saved {
    fd: std::cell::UnsafeCell::new(-1),
    termios: std::cell::UnsafeCell::new(std::mem::MaybeUninit::uninit()),
};

/// Route `signals` through the self-pipe: what a raw terminal's read wakes
/// on. Typically SIGINT, SIGTERM, SIGHUP (and SIGPIPE for a helper whose
/// parent may go away). Safe to call more than once.
pub fn install(signals: &[c_int]) -> io::Result<()> {
    let (_, w) = pipe()?;
    WAKE_W.store(w, Ordering::Release);
    // SAFETY: a zeroed sigaction is valid; the mask and handler are set
    // before it is used.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handler as extern "C" fn(c_int) as usize;
        // No SA_RESTART: a blocked call returns EINTR. Every installed
        // signal is blocked while the handler runs, so it never re-enters.
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        for &sig in signals {
            libc::sigaddset(&mut action.sa_mask, sig);
        }
        for &sig in signals {
            if libc::sigaction(sig, &action, std::ptr::null_mut()) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

/// End the process the way `sig` would have: the default action, so the
/// parent sees the right exit status.
pub fn reraise(sig: c_int) -> ! {
    // SAFETY: plain libc calls on this process.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, sig);
        libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
        libc::raise(sig);
    }
    // A signal whose default is to be ignored: exit as a shell reports it.
    std::process::exit(128 + sig)
}

fn pipe() -> io::Result<(RawFd, RawFd)> {
    if let Some(p) = PIPE.get() {
        return Ok(*p);
    }
    let mut fds = [-1; 2];
    // SAFETY: an out-array of two ints.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    for fd in fds {
        // SAFETY: fds just created.
        unsafe {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
            let flags = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
    let _ = PIPE.set((fds[0], fds[1]));
    Ok(*PIPE.get().expect("set"))
}

/// The read end, when [`install`] has run.
pub(crate) fn wake_fd() -> Option<RawFd> {
    PIPE.get().map(|p| p.0)
}

/// The signal waiting in the pipe, if any; the pipe is left empty.
pub(crate) fn take() -> Option<c_int> {
    let fd = wake_fd()?;
    let mut last = None;
    let mut b = 0u8;
    // SAFETY: one byte into a local; the pipe is non-blocking.
    while unsafe { libc::read(fd, (&raw mut b).cast(), 1) } == 1 {
        last = Some(c_int::from(b));
    }
    last
}

/// A terminal goes raw: from now on signals are handed to its read, and a
/// second one restores `termios` on `fd` before dying.
pub(crate) fn arm(fd: RawFd, termios: &libc::termios) -> io::Result<()> {
    if ARMED.load(Ordering::Acquire) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "another terminal is already raw (or reading a line) in this process",
        ));
    }
    // SAFETY: not armed, so the handler does not read these.
    unsafe {
        *SAVED.fd.get() = fd;
        (*SAVED.termios.get()).write(*termios);
    }
    // A signal left over from an earlier read that was not acted on.
    let _ = take();
    PENDING.store(false, Ordering::Release);
    ARMED.store(true, Ordering::Release);
    Ok(())
}

pub(crate) fn disarm() {
    ARMED.store(false, Ordering::Release);
}

/// This thread's `errno`, which libc names per platform.
///
/// # Safety
/// The pointer is valid for the calling thread only.
unsafe fn errno_ptr() -> *mut c_int {
    // SAFETY: each returns the calling thread's errno location.
    unsafe {
        #[cfg(any(
            target_vendor = "apple",
            target_os = "freebsd",
            target_os = "dragonfly"
        ))]
        return libc::__error();
        #[cfg(any(target_os = "netbsd", target_os = "openbsd", target_os = "android"))]
        return libc::__errno();
        #[cfg(not(any(
            target_vendor = "apple",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "android"
        )))]
        return libc::__errno_location();
    }
}

extern "C" fn handler(sig: c_int) {
    // SAFETY: only async-signal-safe calls (write, tcsetattr, signal,
    // raise); errno is put back for the code this interrupted.
    unsafe {
        let errno = *errno_ptr();
        if ARMED.load(Ordering::Acquire) {
            if !PENDING.swap(true, Ordering::AcqRel) {
                let b = sig as u8;
                libc::write(WAKE_W.load(Ordering::Acquire), (&raw const b).cast(), 1);
                *errno_ptr() = errno;
                return;
            }
            libc::tcsetattr(
                *SAVED.fd.get(),
                libc::TCSANOW,
                (*SAVED.termios.get()).as_ptr(),
            );
        }
        // Default action: delivered as soon as this handler returns (the
        // signal is blocked while it runs).
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
        *errno_ptr() = errno;
    }
}
