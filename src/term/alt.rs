//! The alternate screen, held: what a secret prompt needs to echo a mask.

use std::io::{self, Write};

use super::unix::Writer;

const ENTER: &str = "\x1b[?1049h\x1b[2J\x1b[H";
/// Cleared before leaving: where the alternate screen is a no-op (tmux with
/// `alternate-screen off`), that clear is the only thing that erases the
/// frame. The cursor is shown: leaving does not reliably restore it.
const LEAVE: &str = "\x1b[2J\x1b[H\x1b[?1049l\x1b[?25h";

/// The terminal's alternate screen, entered by [`super::Tty::alt_screen`]
/// and left by [`AltScreen::leave`] or on drop.
///
/// Nothing drawn here survives the session: it is thrown away with the
/// screen, never scrolled into the primary screen's history or a log of it.
/// That is why it is the one place a secret prompt may echo what is typed
/// (`prompt::Echo::Mask` takes one), and why getting one means entering it.
#[derive(Debug)]
pub struct AltScreen {
    out: Writer,
    active: bool,
}

impl AltScreen {
    pub(crate) fn enter(mut out: Writer) -> io::Result<AltScreen> {
        out.write_all(ENTER.as_bytes())?;
        out.flush()?;
        Ok(AltScreen { out, active: true })
    }

    /// Clear it, cursor home.
    pub fn clear(&mut self) -> io::Result<()> {
        self.out.write_all(b"\x1b[2J\x1b[H")?;
        self.out.flush()
    }

    /// Back to the primary screen, as it was. Once: later calls (and the
    /// drop) do nothing. The one to call before `std::process::exit`.
    pub fn leave(&mut self) -> io::Result<()> {
        if !std::mem::replace(&mut self.active, false) {
            return Ok(());
        }
        self.out.write_all(LEAVE.as_bytes())?;
        self.out.flush()
    }
}

impl Write for AltScreen {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.out.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

impl Drop for AltScreen {
    fn drop(&mut self) {
        let _ = self.leave();
    }
}
