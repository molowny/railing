//! `term` on a real pty, the way askpass uses it: a child with no controlling
//! terminal (`setsid`), stdin and stdout on pipes, stderr closed off, drawing
//! and reading only on the terminal it was given by path. See the askpass
//! bar in `docs/core-refactor.md` (A1–A5, A7).
//!
//! Each test re-runs this binary as that child (`child` below, picked by
//! environment) and talks to it through the pty's master side and the
//! child's stdout, where it reports every event it read.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cli_kit::style::{Caps, TermEnv};
use cli_kit::term::{Event, Key, Tty, signals};

const MODE: &str = "TERM_PTY_CHILD";
const PATH: &str = "TERM_PTY_PATH";
/// What starts each line the child reports.
const REPORT: &str = "» ";

// ── the child ──────────────────────────────────────────────────────────

/// Not a test of its own: the child's entry, when [`MODE`] is set.
#[test]
fn child() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    let out = &mut std::io::stdout();
    let say = |out: &mut std::io::Stdout, line: String| {
        // Marked: libtest writes its own lines to the same stdout.
        writeln!(out, "\n{REPORT}{line}").unwrap();
        out.flush().unwrap();
    };
    say(
        out,
        format!("dev-tty {}", std::fs::File::open("/dev/tty").is_ok()),
    );
    if std::env::var_os("TERM_PTY_SIGNALS").is_some() {
        signals::install(&[libc::SIGTERM, libc::SIGINT, libc::SIGHUP]).unwrap();
    }
    if mode == "idle" {
        say(out, "ready".into());
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let env = TermEnv {
        term: Some("xterm-256color".into()),
        no_color: false,
    };
    let tty = Tty::open(std::env::var(PATH).unwrap(), &env).unwrap();
    say(out, format!("caps {:?}", tty.caps()));
    say(out, format!("size {:?}", tty.size()));
    let before = termios(&tty);
    let mut raw = tty.raw().unwrap();
    say(out, "ready".into());
    if mode == "stall" {
        // Raw, and never reading: the app is busy when signals come.
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    loop {
        let event = raw.read(None).unwrap();
        say(out, format!("{event:?}"));
        match event {
            Event::Key(Key::Enter) | Event::Hangup => break,
            Event::Interrupted(sig) => {
                say(out, format!("restored {}", same(&before, &termios(&tty))));
                signals::reraise(sig);
            }
            _ => {}
        }
    }
    raw.restore().unwrap();
    say(out, format!("restored {}", same(&before, &termios(&tty))));
}

fn termios(fd: &impl AsFd) -> libc::termios {
    // SAFETY: zeroed, then filled in.
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: a valid fd.
    assert_eq!(
        unsafe { libc::tcgetattr(fd.as_fd().as_raw_fd(), &mut t) },
        0
    );
    t
}

fn same(a: &libc::termios, b: &libc::termios) -> bool {
    a.c_iflag == b.c_iflag
        && a.c_oflag == b.c_oflag
        && a.c_cflag == b.c_cflag
        && a.c_lflag == b.c_lflag
        && a.c_cc == b.c_cc
}

// ── the parent ─────────────────────────────────────────────────────────

struct Pty {
    master: std::fs::File,
    /// Kept open so the terminal's settings can be read from this side.
    slave: OwnedFd,
    path: String,
}

fn openpty(rows: u16, cols: u16) -> Pty {
    let (mut m, mut s) = (-1, -1);
    let ws = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: out-pointers and a valid winsize.
    let rc = unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws) };
    assert_eq!(rc, 0, "openpty");
    for fd in [m, s] {
        // SAFETY: fresh fds.
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    // SAFETY: fresh fds owned from here on.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(m), OwnedFd::from_raw_fd(s)) };
    let mut buf = [0 as libc::c_char; 128];
    // SAFETY: a buffer of its stated length.
    assert_eq!(
        unsafe { libc::ttyname_r(slave.as_raw_fd(), buf.as_mut_ptr(), buf.len()) },
        0
    );
    // SAFETY: NUL-terminated by ttyname_r.
    let path = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    Pty {
        master: master.into(),
        slave,
        path,
    }
}

struct Session {
    pty: Pty,
    child: Child,
    lines: mpsc::Receiver<String>,
}

fn start(mode: &str, signals: bool) -> Session {
    let pty = openpty(30, 100);
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "child", "--nocapture", "--test-threads=1"])
        .env(MODE, mode)
        .env(PATH, &pty.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if signals {
        cmd.env("TERM_PTY_SIGNALS", "1");
    }
    // SAFETY: setsid is async-signal-safe. A new session with no controlling
    // terminal: like askpass under a daemon.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().unwrap();
    let lines = reader(child.stdout.take().unwrap());
    let mut s = Session { pty, child, lines };
    s.expect("dev-tty false");
    s
}

fn reader(stdout: ChildStdout) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { return };
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    rx
}

impl Session {
    /// The next line the child reports that is not libtest's own.
    fn line(&mut self) -> String {
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(5))
                .expect("the child said nothing");
            if let Some(report) = line.strip_prefix(REPORT) {
                return report.to_string();
            }
        }
    }

    fn expect(&mut self, want: &str) {
        let got = self.line();
        assert_eq!(got, want);
    }

    fn keys(&mut self, bytes: &[u8]) {
        self.pty.master.write_all(bytes).unwrap();
    }

    fn wait(&mut self) -> std::process::ExitStatus {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "the child did not exit"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn signal(&self, sig: libc::c_int) {
        // SAFETY: a child of ours.
        unsafe { libc::kill(self.child.id() as libc::pid_t, sig) };
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn started(mode: &str, signals: bool) -> Session {
    let mut s = start(mode, signals);
    s.expect("caps Caps { color: true, redraw: true }");
    s.expect("size Some(Size { rows: 30, cols: 100 })");
    s.expect("ready");
    s
}

// ── the tests ──────────────────────────────────────────────────────────

#[test]
fn a_terminal_named_by_path_works_without_a_controlling_terminal() {
    // A1, A3: no /dev/tty (start asserts it), caps and size from the path.
    let mut s = started("read", false);
    s.keys(b"\r");
    s.expect("Key(Enter)");
    s.expect("restored true");
}

#[test]
fn raw_mode_is_the_hardened_one_and_is_restored() {
    // A4: what askpass sets, read back from the terminal itself.
    let mut s = started("read", false);
    let t = termios(&s.pty.slave);
    for (flag, name) in [
        (libc::ECHO, "ECHO"),
        (libc::ICANON, "ICANON"),
        (libc::ISIG, "ISIG"),
        (libc::IEXTEN, "IEXTEN"),
    ] {
        assert_eq!(t.c_lflag & flag, 0, "{name} left on");
    }
    for (flag, name) in [
        (libc::IXON, "IXON"),
        (libc::ICRNL, "ICRNL"),
        (libc::INLCR, "INLCR"),
    ] {
        assert_eq!(t.c_iflag & flag, 0, "{name} left on");
    }
    assert_eq!((t.c_cc[libc::VMIN], t.c_cc[libc::VTIME]), (1, 0));
    s.keys(b"\r");
    s.expect("Key(Enter)");
    s.expect("restored true");
    assert_ne!(termios(&s.pty.slave).c_lflag & libc::ECHO, 0);
}

#[test]
fn control_keys_reach_the_reader_instead_of_the_terminal() {
    // A4, A7: Ctrl-S does not freeze it, Ctrl-V does not eat the next key,
    // Ctrl-C is a key, not SIGINT; CR arrives as typed.
    let mut s = started("read", false);
    s.keys(b"\x13a\x16b\x03");
    for want in [
        "Key(Ctrl('s'))",
        "Key(Char('a'))",
        "Key(Ctrl('v'))",
        "Key(Char('b'))",
        "Key(Ctrl('c'))",
    ] {
        s.expect(want);
    }
    s.keys(b"\r");
    s.expect("Key(Enter)");
}

#[test]
fn input_is_decoded_whole() {
    // A7: UTF-8 as typed, every sequence drained, a bare Esc after its wait.
    let mut s = started("read", false);
    s.keys("ł".as_bytes());
    s.expect("Key(Char('ł'))");
    s.keys(b"\x1b[A\x1b[97;5u\x1b[13u");
    s.expect("Key(Up)");
    s.expect("Key(Ctrl('a'))");
    s.expect("Key(Enter)");
    s.expect("restored true");

    let mut s = started("read", false);
    s.keys(b"\x1b");
    s.expect("Key(Esc)");
    s.keys(b"\x7f\r");
    s.expect("Key(Backspace)");
    s.expect("Key(Enter)");
}

#[test]
fn a_hangup_is_not_a_key() {
    // A7: the terminal going away ends the read; it never submits.
    let mut s = started("read", false);
    s.keys(b"pa");
    s.expect("Key(Char('p'))");
    s.expect("Key(Char('a'))");
    let Session { pty, .. } = &mut s;
    let master = std::mem::replace(&mut pty.master, std::fs::File::open("/dev/null").unwrap());
    drop(master);
    s.expect("Hangup");
}

#[test]
fn a_signal_wakes_an_idle_read_and_the_terminal_comes_back() {
    // A5: no key pressed. The read wakes at once, restores, and the child
    // dies of the signal it got.
    let mut s = started("read", true);
    let sent = Instant::now();
    s.signal(libc::SIGTERM);
    s.expect("Interrupted(15)");
    s.expect("restored true");
    let status = s.wait();
    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
    assert!(
        sent.elapsed() < Duration::from_secs(1),
        "waited {:?}",
        sent.elapsed()
    );
    assert_ne!(termios(&s.pty.slave).c_lflag & libc::ECHO, 0);
}

#[test]
fn a_second_signal_restores_the_terminal_itself() {
    // A5: raw, and the app never reads. The first signal is pending for the
    // read; the second restores the terminal in the handler and dies.
    let mut s = started("stall", true);
    s.signal(libc::SIGTERM);
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        s.child.try_wait().unwrap().is_none(),
        "the first signal is the read's"
    );
    s.signal(libc::SIGTERM);
    let status = s.wait();
    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
    let t = termios(&s.pty.slave);
    assert_ne!(t.c_lflag & libc::ECHO, 0, "echo not restored");
    assert_ne!(t.c_lflag & libc::ICANON, 0, "canonical mode not restored");
}

#[test]
fn a_signal_with_no_raw_terminal_takes_its_default_action() {
    // A5: installed, but nothing to restore: as if nothing were installed.
    let mut s = start("idle", true);
    s.expect("ready");
    s.signal(libc::SIGTERM);
    assert_eq!(s.wait().signal(), Some(libc::SIGTERM));
}

#[test]
fn caps_belong_to_each_terminal() {
    // A2: two terminals in one process, each with its owner's env.
    let pty = openpty(24, 80);
    let color = TermEnv {
        term: Some("xterm".into()),
        no_color: false,
    };
    let dumb = TermEnv {
        term: Some("dumb".into()),
        no_color: false,
    };
    let a = Tty::open(&pty.path, &color).unwrap();
    let b = Tty::open(&pty.path, &dumb).unwrap();
    assert_eq!(a.caps(), Caps::COLOR);
    assert_eq!(b.caps(), Caps::PLAIN);
    use cli_kit::glyph::{self, State};
    assert_eq!(glyph::step(a.caps(), State::Active), "\x1b[36m◆\x1b[0m");
    assert_eq!(glyph::step(b.caps(), State::Active), "◆");
    assert_eq!(a.with_caps(Caps::PLAIN).caps(), Caps::PLAIN);
}

#[test]
fn a_pipe_has_no_size_and_no_caps() {
    // A3: no guessed size for something that cannot say.
    let mut fds = [-1; 2];
    // SAFETY: an out-array of two.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    // SAFETY: fresh fds.
    let (r, w) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    drop(r);
    let tty = Tty::from_fd(
        w,
        &TermEnv {
            term: Some("xterm".into()),
            no_color: false,
        },
    );
    assert!(!tty.is_terminal());
    assert_eq!(tty.size(), None);
    assert_eq!(tty.caps(), Caps::PLAIN);
    assert!(tty.raw().is_err(), "a pipe cannot go raw");
}

#[test]
fn only_one_terminal_is_raw_at_a_time() {
    let pty = openpty(24, 80);
    let tty = Tty::open(&pty.path, &TermEnv::default()).unwrap();
    let raw = tty.raw().unwrap();
    let again = tty.raw();
    assert!(again.is_err(), "a second Raw while one is alive");
    drop(raw);
    let mut third = tty.raw().unwrap();
    // Restore is idempotent, and nothing reads after it.
    third.restore().unwrap();
    third.restore().unwrap();
    drop(third);
    let mut buf = [0u8; 1];
    let _ = (&pty.master).read(&mut buf[..0]);
}
