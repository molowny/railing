//! A child of this binary on a real pty, driven like a user would.

use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::termios::Winsize;

/// How long one wait may take before the scene is declared stuck.
const PATIENCE: Duration = Duration::from_secs(10);
/// Output is settled once the pty has been quiet this long.
const QUIET: Duration = Duration::from_millis(120);

pub struct Session {
    master: std::fs::File,
    child: Child,
    term: avt::Terminal,
    /// The tail of a UTF-8 sequence split across reads.
    pending: Vec<u8>,
    rows: u16,
    eof: bool,
}

/// The locale a scene runs under. The marks are Unicode whatever it is, so
/// both draw the same screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    /// `LC_ALL=C.UTF-8`: what the fixtures are captured under.
    Utf8,
    /// `LANG=C`, nothing else: CI's locale, where cliclack fell back to ASCII.
    C,
}

impl Session {
    /// Start `scene` in a child of this binary: a new session with the pty
    /// as its controlling terminal, stdin and stderr on it, stdout a pipe so
    /// what a prompt returned can be told apart from what it drew.
    pub fn start(
        scene: &str,
        color: bool,
        cols: u16,
        rows: u16,
        term: &str,
        locale: Locale,
    ) -> Result<Self, String> {
        let (master, slave) = openpty(cols, rows)?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(exe);
        cmd.env_clear()
            .env(super::SCENE_ENV, scene)
            .env(super::COLOR_ENV, if color { "1" } else { "0" })
            .env("TERM", term)
            .stdin(Stdio::from(slave.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(slave))
            .stdout(Stdio::piped());
        match locale {
            Locale::Utf8 => cmd.env("LANG", "C.UTF-8").env("LC_ALL", "C.UTF-8"),
            Locale::C => cmd.env("LANG", "C"),
        };
        // The child makes the pty its controlling terminal itself
        // (`take_terminal`), first thing in `main`.
        let child = cmd.spawn().map_err(|e| format!("spawn: {e}"))?;
        Ok(Self {
            master: std::fs::File::from(master),
            child,
            term: avt::Terminal::new(usize::from(cols), usize::from(rows))
                .map_err(|_| "terminal size".to_string())?,
            pending: Vec::new(),
            rows,
            eof: false,
        })
    }

    /// The window is now `cols` wide: the emulator reflows, then the pty
    /// reports the new size, as a terminal does.
    pub fn resize(&mut self, cols: u16) -> Result<(), String> {
        self.term.resize(usize::from(cols), usize::from(self.rows));
        rustix::termios::tcsetwinsize(&self.master, winsize(cols, self.rows))
            .map_err(|e| format!("resize: {e}"))
    }

    pub fn term(&self) -> &avt::Terminal {
        &self.term
    }

    fn text(&self) -> String {
        self.term.screen().text().join("\n")
    }

    /// Read whatever the child wrote within `wait`. False once nothing came.
    fn pump(&mut self, wait: Duration) -> bool {
        if self.eof {
            return false;
        }
        let Ok(wait) = Timespec::try_from(wait) else {
            return false;
        };
        let mut fds = [PollFd::new(&self.master, PollFlags::IN)];
        if !matches!(rustix::event::poll(&mut fds, Some(&wait)), Ok(n) if n > 0) {
            return false;
        }
        let mut buf = [0u8; 4096];
        match self.master.read(&mut buf) {
            Ok(0) | Err(_) => {
                // EIO once every copy of the slave is closed: the child is gone.
                self.eof = true;
                false
            }
            Ok(n) => {
                self.pending.extend_from_slice(&buf[..n]);
                let valid = match std::str::from_utf8(&self.pending) {
                    Ok(s) => s.len(),
                    Err(e) if e.error_len().is_none() => e.valid_up_to(),
                    // Not UTF-8 at all: a bug in what was drawn. Show it.
                    Err(_) => {
                        let s = String::from_utf8_lossy(&self.pending).into_owned();
                        self.term.feed(s.as_str());
                        self.pending.clear();
                        return true;
                    }
                };
                let rest = self.pending.split_off(valid);
                let s =
                    String::from_utf8(std::mem::replace(&mut self.pending, rest)).expect("checked");
                self.term.feed(s.as_str());
                true
            }
        }
    }

    /// Until the screen shows `needle`, then until it settles.
    pub fn expect(&mut self, needle: &str) -> Result<(), String> {
        let start = Instant::now();
        while !self.text().contains(needle) {
            if start.elapsed() > PATIENCE || (self.eof && !self.pump(Duration::ZERO)) {
                return Err(format!("never showed {needle:?}; screen:\n{}", self.text()));
            }
            self.pump(Duration::from_millis(50));
        }
        self.settle();
        Ok(())
    }

    /// Until the child has been quiet for a moment.
    pub fn settle(&mut self) {
        let start = Instant::now();
        while self.pump(QUIET) && start.elapsed() < PATIENCE {}
    }

    pub fn keys(&mut self, keys: &[u8]) -> Result<(), String> {
        self.master
            .write_all(keys)
            .map_err(|e| format!("keys: {e}"))?;
        self.master.flush().map_err(|e| format!("keys: {e}"))
    }

    /// Wait for the child to exit; its exit code and stdout.
    pub fn finish(&mut self) -> Result<(i32, String), String> {
        let start = Instant::now();
        loop {
            self.pump(Duration::from_millis(50));
            if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
                // Drain what it wrote on the way out.
                while self.pump(Duration::from_millis(20)) {}
                let mut out = String::new();
                if let Some(mut stdout) = self.child.stdout.take() {
                    let _ = stdout.read_to_string(&mut out);
                }
                let code = status.code().unwrap_or_else(|| {
                    128 + std::os::unix::process::ExitStatusExt::signal(&status).unwrap_or(0)
                });
                return Ok((code, out));
            }
            if start.elapsed() > PATIENCE {
                let _ = self.child.kill();
                return Err(format!("did not exit; screen:\n{}", self.text()));
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn winsize(cols: u16, rows: u16) -> Winsize {
    Winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

/// In the child, before the scene: a new session with the pty (its stdin)
/// as the controlling terminal, as a shell gives a command.
pub fn take_terminal() -> std::io::Result<()> {
    rustix::process::setsid()?;
    rustix::process::ioctl_tiocsctty(std::io::stdin())?;
    Ok(())
}

/// A pty, both ends close-on-exec: the child gets the slave as
/// stdin/stderr through dup2, which clears the flag on the copies.
fn openpty(cols: u16, rows: u16) -> Result<(OwnedFd, OwnedFd), String> {
    use rustix::fs::{Mode, OFlags};
    use rustix::pty::{self, OpenptFlags};
    fn err(what: &'static str) -> impl Fn(rustix::io::Errno) -> String {
        move |e| format!("{what}: {e}")
    }
    let master = pty::openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC)
        .map_err(err("openpt"))?;
    pty::grantpt(&master).map_err(err("grantpt"))?;
    pty::unlockpt(&master).map_err(err("unlockpt"))?;
    let name = pty::ptsname(&master, Vec::new()).map_err(err("ptsname"))?;
    let slave = rustix::fs::open(
        name.as_c_str(),
        OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(err("open slave"))?;
    rustix::termios::tcsetwinsize(&slave, winsize(cols, rows)).map_err(err("winsize"))?;
    Ok((master, slave))
}
