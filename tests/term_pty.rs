//! `term` on a real pty, the way askpass uses it: a child with no controlling
//! terminal (`setsid`), stdin and stdout on pipes, stderr closed off, drawing
//! and reading only on the terminal it was given by path. What a password
//! prompt in a separate process needs from the core.
//!
//! Each test re-runs this binary as that child (`child` below, picked by
//! environment) and talks to it through the pty's master side and the
//! child's stdout, where it reports every event it read.
#![cfg(unix)]
#![forbid(unsafe_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use railing::style::{Caps, TermEnv};
use railing::term::signals::{self, SIGHUP, SIGINT, SIGTERM};
use railing::term::{Event, Key, Line, Tty};
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::fs::{Mode, OFlags};
use rustix::process::{Pid, Signal};
use rustix::termios::{InputModes, LocalModes, SpecialCodeIndex, Termios, Winsize};

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
    // A new session with no controlling terminal: like askpass under a
    // daemon. Done here, not between fork and exec, which needs `unsafe`.
    rustix::process::setsid().unwrap();
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
        signals::install(&[SIGTERM, SIGINT, SIGHUP]).unwrap();
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
    if mode == "secret" || mode == "mask" {
        return secret_child(out, tty, mode == "mask", say);
    }
    let before = termios(&tty);
    if mode == "line" {
        // The terminal's own line mode, a secret's: echo off.
        say(out, "ready".into());
        let line = tty.read_line(false).unwrap();
        match &line {
            Line::Entered(s) => say(out, format!("Entered({})", s.chars().count())),
            other => say(out, format!("{other:?}")),
        }
        say(out, format!("restored {}", same(&before, &termios(&tty))));
        if let Line::Interrupted(sig) = line {
            signals::reraise(sig);
        }
        return;
    }
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

/// A secret read the way a prompt reads one: the step drawn first, then
/// nothing (or masks on the alternate screen). Reports how each read ended,
/// the length only for an entered secret.
fn secret_child(
    out: &mut std::io::Stdout,
    mut tty: Tty,
    mask: bool,
    say: impl Fn(&mut std::io::Stdout, String),
) {
    use railing::prompt::{self, Echo, Secret};
    use railing::render::Region;
    use railing::step::{self, Body, Foot, Status, Step};
    let step = Step {
        status: Status::Active,
        label: "Password",
        desc: "",
        body: Body::OnLabel,
        foot: Foot::Hint(step::secret_hint(false)),
    };
    let mut region = Region::new(tty.caps());
    let mut alt = None;
    if mask {
        alt = Some(tty.alt_screen().unwrap());
    } else {
        let block = step::render(tty.caps(), &step, 100);
        region.draw(&mut tty, &block).unwrap();
    }
    let mut raw = tty.raw().unwrap();
    say(out, "ready".into());
    loop {
        let mut paint = |s: &mut railing::term::AltScreen, n: usize| {
            write!(s, "\r[{}]", "▪".repeat(n))?;
            s.flush()
        };
        let echo = match alt.as_mut() {
            Some(screen) => Echo::Mask {
                screen,
                paint: &mut paint,
            },
            None => Echo::Silent,
        };
        let result = prompt::secret(&mut raw, echo).unwrap();
        match result {
            Secret::Entered(s) => say(out, format!("Entered({})", s.chars().count())),
            Secret::Empty => {
                say(out, "Empty".into());
                continue;
            }
            other => say(out, format!("{other:?}")),
        }
        break;
    }
    raw.restore().unwrap();
    match alt.as_mut() {
        Some(a) => a.leave().unwrap(),
        None => region.erase(&mut tty).unwrap(),
    }
    say(out, "done".into());
}

fn termios(fd: &impl AsFd) -> Termios {
    rustix::termios::tcgetattr(fd).unwrap()
}

/// Every field, the control codes and speeds with them: `Termios` has no
/// `PartialEq`, its `Debug` shows it whole.
fn same(a: &Termios, b: &Termios) -> bool {
    format!("{a:?}") == format!("{b:?}")
}

// ── the parent ─────────────────────────────────────────────────────────

struct Pty {
    master: std::fs::File,
    /// Kept open so the terminal's settings can be read from this side.
    slave: OwnedFd,
    path: String,
}

fn openpty(rows: u16, cols: u16) -> Pty {
    use rustix::pty::{self, OpenptFlags};
    let master = pty::openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC)
        .expect("openpt");
    pty::grantpt(&master).unwrap();
    pty::unlockpt(&master).unwrap();
    let name = pty::ptsname(&master, Vec::new()).unwrap();
    let slave = rustix::fs::open(
        name.as_c_str(),
        OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .unwrap();
    let ws = Winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    rustix::termios::tcsetwinsize(&slave, ws).unwrap();
    Pty {
        master: master.into(),
        slave,
        path: name.to_string_lossy().into_owned(),
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

    /// What reached the terminal since the last call, waiting `quiet` for
    /// more each time.
    fn painted(&mut self, quiet: Duration) -> String {
        let mut out = Vec::new();
        let quiet = Timespec::try_from(quiet).unwrap();
        loop {
            let mut p = [PollFd::new(&self.pty.master, PollFlags::IN)];
            if !matches!(rustix::event::poll(&mut p, Some(&quiet)), Ok(n) if n > 0) {
                break;
            }
            let mut buf = [0u8; 4096];
            match self.pty.master.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn signal(&self, sig: Signal) {
        rustix::process::kill_process(Pid::from_child(&self.child), sig).unwrap();
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
        (LocalModes::ECHO, "ECHO"),
        (LocalModes::ICANON, "ICANON"),
        (LocalModes::ISIG, "ISIG"),
        (LocalModes::IEXTEN, "IEXTEN"),
    ] {
        assert!(!t.local_modes.intersects(flag), "{name} left on");
    }
    for (flag, name) in [
        (InputModes::IXON, "IXON"),
        (InputModes::ICRNL, "ICRNL"),
        (InputModes::INLCR, "INLCR"),
    ] {
        assert!(!t.input_modes.intersects(flag), "{name} left on");
    }
    assert_eq!(
        (
            t.special_codes[SpecialCodeIndex::VMIN],
            t.special_codes[SpecialCodeIndex::VTIME]
        ),
        (1, 0)
    );
    s.keys(b"\r");
    s.expect("Key(Enter)");
    s.expect("restored true");
    assert_ne!(
        termios(&s.pty.slave).local_modes & LocalModes::ECHO,
        LocalModes::empty()
    );
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
    s.signal(Signal::TERM);
    s.expect("Interrupted(15)");
    s.expect("restored true");
    let status = s.wait();
    assert_eq!(status.signal(), Some(SIGTERM), "{status:?}");
    assert!(
        sent.elapsed() < Duration::from_secs(1),
        "waited {:?}",
        sent.elapsed()
    );
    assert_ne!(
        termios(&s.pty.slave).local_modes & LocalModes::ECHO,
        LocalModes::empty()
    );
}

#[test]
fn a_second_signal_restores_the_terminal_itself() {
    // A5: raw, and the app never reads. The first signal is pending for the
    // read; the second restores the terminal in the handler and dies.
    let mut s = started("stall", true);
    s.signal(Signal::TERM);
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        s.child.try_wait().unwrap().is_none(),
        "the first signal is the read's"
    );
    s.signal(Signal::TERM);
    let status = s.wait();
    assert_eq!(status.signal(), Some(SIGTERM), "{status:?}");
    let t = termios(&s.pty.slave);
    assert!(
        t.local_modes.contains(LocalModes::ECHO),
        "echo not restored"
    );
    assert!(
        t.local_modes.contains(LocalModes::ICANON),
        "canonical mode not restored"
    );
}

#[test]
fn a_signal_with_no_raw_terminal_takes_its_default_action() {
    // A5: installed, but nothing to restore: as if nothing were installed.
    let mut s = start("idle", true);
    s.expect("ready");
    s.signal(Signal::TERM);
    assert_eq!(s.wait().signal(), Some(SIGTERM));
}

#[test]
fn a_line_read_is_entered_and_puts_the_settings_back() {
    let mut s = started("line", true);
    s.keys(b"hunter2\r");
    s.expect("Entered(7)");
    s.expect("restored true");
}

#[test]
fn a_signal_during_a_line_read_puts_echo_back() {
    // Line mode turns echo and ISIG off, so it is armed as raw mode is: the
    // signal wakes the read mid-line, the settings come back, the typed
    // half of the secret is not left for the shell, and the child dies of
    // the signal.
    let mut s = started("line", true);
    s.keys(b"hunt");
    std::thread::sleep(Duration::from_millis(100));
    s.signal(Signal::TERM);
    s.expect("Interrupted(15)");
    s.expect("restored true");
    assert_eq!(s.wait().signal(), Some(SIGTERM));
    let t = termios(&s.pty.slave);
    assert!(
        t.local_modes.contains(LocalModes::ECHO),
        "echo not restored"
    );
    assert!(
        t.local_modes.contains(LocalModes::ISIG),
        "ISIG not restored"
    );
    let pending = rustix::io::ioctl_fionread(&s.pty.slave).unwrap();
    assert_eq!(pending, 0, "typed input left behind");
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
    use railing::glyph::{self, State};
    assert_eq!(glyph::step(a.caps(), State::Active), "\x1b[36m◆\x1b[0m");
    assert_eq!(glyph::step(b.caps(), State::Active), "◆");
    assert_eq!(a.with_caps(Caps::PLAIN).caps(), Caps::PLAIN);
}

#[test]
fn a_pipe_has_no_size_and_no_caps() {
    // A3: no guessed size for something that cannot say.
    let (r, w) = std::io::pipe().unwrap();
    drop(r);
    let tty = Tty::from_fd(
        OwnedFd::from(w),
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

#[test]
fn a_secret_is_read_with_nothing_drawn() {
    // A6: the step is drawn once; between it and Enter the terminal gets
    // zero bytes, whatever is typed or deleted.
    let mut s = started("secret", false);
    let drawn = s.painted(Duration::from_millis(150));
    assert!(drawn.contains("◆\x1b[0m  Password"), "{drawn:?}");
    s.keys("hunter2\x7fł\x1b[D\x15abc".as_bytes());
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        s.painted(Duration::from_millis(100)),
        "",
        "drawn while typing"
    );
    s.keys(b"\r");
    s.expect("Entered(3)");
    s.expect("done");
    // The step goes: down from the question's line, where the input was, to
    // the block's end, up over its two lines, cleared.
    let end = s.painted(Duration::from_millis(100));
    assert!(end.contains("\x1b[2B\r\x1b[2F\x1b[J\x1b[?25h"), "{end:?}");
}

#[test]
fn an_empty_secret_is_not_entered_and_a_hangup_drops_what_was_typed() {
    // A7.
    let mut s = started("secret", false);
    s.keys(b"\r");
    s.expect("Empty");
    s.keys(b"partial");
    std::thread::sleep(Duration::from_millis(100));
    let Session { pty, .. } = &mut s;
    drop(std::mem::replace(
        &mut pty.master,
        std::fs::File::open("/dev/null").unwrap(),
    ));
    s.expect("Hangup");
}

#[test]
fn a_mask_is_drawn_only_on_the_alternate_screen() {
    // A6: Echo::Mask needs a held AltScreen, so a mask can only follow
    // entering it.
    let mut s = start("mask", false);
    s.expect("caps Caps { color: true, redraw: true }");
    s.expect("size Some(Size { rows: 30, cols: 100 })");
    s.expect("ready");
    s.keys(b"abc");
    std::thread::sleep(Duration::from_millis(150));
    s.keys(b"\x1b");
    s.expect("Cancelled");
    s.expect("done");
    let painted = s.painted(Duration::from_millis(100));
    let enter = painted.find("\x1b[?1049h").expect("entered");
    let mask = painted.find('▪').expect("masked");
    let leave = painted.rfind("\x1b[?1049l").expect("left");
    assert!(enter < mask && mask < leave, "{painted:?}");
}
