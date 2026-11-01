//! A frame redrawn in place on a [`Tty`] while keys are read from it: what
//! the picker, the browsed grid and the wait draw through.
//!
//! The frame is any text whose lines are cut to the terminal's width
//! ([`crate::frame`] cuts them), drawn as a [`Block`] in a [`Region`], so
//! each frame replaces the last and erasing climbs exactly what is there.
//! Keys come from the core's reader ([`Raw`]), in raw mode with `ISIG` off:
//! Ctrl-C is a key, never a signal at the process, so nothing has to ignore
//! SIGINT around it. The width is checked between keys (a timed read), so a
//! resize is reported and the caller redraws at the new width.
//!
//! A terminal that cannot move the cursor ([`Caps::redraw`] off) has no
//! surface: [`Surface::new`] refuses it with [`io::ErrorKind::Unsupported`],
//! and the caller does what it does without one (prints the table, asks for
//! an argument).

use std::borrow::BorrowMut;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use crate::render::{Block, Region};
use crate::term::{Event, Key, Raw, Size, Tty};

/// How often the width is checked while waiting for a key.
const RESIZE_POLL: Duration = Duration::from_millis(200);

const CURSOR_HIDE: &str = "\x1b[?25l";
const CURSOR_SHOW: &str = "\x1b[?25h";

/// What a wait for input gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Input {
    Key(Key),
    /// The terminal changed size: draw the frame again at [`Surface::cols`].
    Resized,
    /// Nothing within the time asked for.
    Timeout,
    /// The terminal went away.
    Hangup,
    /// A signal installed through `term::signals`: the terminal is restored;
    /// erase the frame and re-raise it.
    Interrupted(i32),
}

/// The frame on a terminal, owned or borrowed (`Tty` or `&mut Tty`), the
/// cursor hidden while it is drawn.
pub(crate) struct Surface<T: BorrowMut<Tty>> {
    tty: T,
    region: Region,
    size: Option<Size>,
    hidden: bool,
}

impl<T: BorrowMut<Tty>> Surface<T> {
    pub(crate) fn new(tty: T) -> io::Result<Surface<T>> {
        let caps = tty.borrow().caps();
        if !caps.redraw {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "this terminal cannot redraw in place",
            ));
        }
        let size = tty.borrow().size();
        Ok(Surface {
            tty,
            region: Region::new(caps),
            size,
            hidden: false,
        })
    }

    /// The terminal's width; 80 when it cannot say.
    pub(crate) fn cols(&self) -> usize {
        self.size.map_or(80, |s| usize::from(s.cols))
    }

    /// The terminal's height; 24 when it cannot say.
    #[cfg(feature = "table")]
    pub(crate) fn rows(&self) -> usize {
        self.size.map_or(24, |s| usize::from(s.rows))
    }

    /// Raw mode and the key reader on this terminal. Held as long as keys
    /// are wanted; dropping it (or [`Raw::restore`]) puts the terminal back.
    pub(crate) fn raw(&self) -> io::Result<Raw> {
        self.tty.borrow().raw()
    }

    /// Draw `frame` in place of the last one.
    pub(crate) fn draw(&mut self, frame: &str) -> io::Result<()> {
        if !self.hidden {
            self.tty.borrow_mut().write_all(CURSOR_HIDE.as_bytes())?;
            self.hidden = true;
        }
        let block = Block::of_lines(frame.lines().map(str::to_owned));
        self.region.draw(self.tty.borrow_mut(), &block)
    }

    /// The next key within `timeout` (`None`: as long as it takes), or a
    /// resize. The size is checked every [`RESIZE_POLL`] meanwhile.
    pub(crate) fn read(&mut self, raw: &mut Raw, timeout: Option<Duration>) -> io::Result<Input> {
        let end = timeout.map(|t| Instant::now() + t);
        loop {
            let left = end.map(|e| e.saturating_duration_since(Instant::now()));
            let slice = left.map_or(RESIZE_POLL, |l| l.min(RESIZE_POLL));
            match raw.read(Some(slice))? {
                Event::Key(key) => return Ok(Input::Key(key)),
                Event::Hangup => return Ok(Input::Hangup),
                Event::Interrupted(sig) => return Ok(Input::Interrupted(sig)),
                Event::Timeout => {
                    let now = self.tty.borrow().size();
                    if now != self.size {
                        self.size = now;
                        let cols = self.cols();
                        self.region.resized(cols);
                        return Ok(Input::Resized);
                    }
                    if left.is_some_and(|l| l <= slice) {
                        return Ok(Input::Timeout);
                    }
                }
            }
        }
    }

    /// Leave the last frame where it is: what follows is drawn below it.
    pub(crate) fn keep(mut self) -> io::Result<()> {
        self.region.keep(self.tty.borrow_mut())?;
        self.show_cursor()
    }

    /// Take the frame off the screen.
    pub(crate) fn erase(mut self) -> io::Result<()> {
        self.region.erase(self.tty.borrow_mut())?;
        self.show_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        if self.hidden {
            self.hidden = false;
            let tty = self.tty.borrow_mut();
            tty.write_all(CURSOR_SHOW.as_bytes())?;
            tty.flush()?;
        }
        Ok(())
    }
}

impl<T: BorrowMut<Tty>> Drop for Surface<T> {
    /// An early return (an error) still gives the cursor back.
    fn drop(&mut self) {
        let _ = self.show_cursor();
    }
}
