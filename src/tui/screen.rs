//! TUI runner: `/dev/tty`, raw mode, draw + event loop, cleanup.
//!
//! Generic scaffolding for any mini-TUI. Callers implement [`ScreenApp`] and
//! pass the instance to [`run`]; [`ScreenApp::surface`] picks where it draws.
//!
//! # Picking a surface
//!
//! [`Surface::Inline`] renders below the shell prompt, as part of the session.
//! It writes to the **primary screen**, so a terminal that reflows on resize
//! (kitty, VTE, alacritty, …) can push those rows into scrollback, where the
//! app can neither find nor erase them. Never render secrets there.
//!
//! [`Surface::AltScreen`] takes the full window. Its content never enters
//! scrollback and is discarded on exit, which makes it the only safe surface
//! for anything sensitive — and the simpler one to resize. It is the default.

use std::fs::File;
use std::io::{self, IsTerminal, Write};
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use crossterm::terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};

/// Control-flow signal returned from [`ScreenApp::on_key`].
pub enum Flow<T> {
    /// Keep looping.
    Continue,
    /// Exit the loop with a value. [`run`] returns `Ok(Some(value))`.
    Exit(T),
    /// Exit the loop with no value. [`run`] returns `Ok(None)`.
    Cancel,
}

/// Where an app draws. See the [module docs](self#picking-a-surface) for how
/// to choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// A widget of `height` rows below the shell prompt, on the primary
    /// screen.
    ///
    /// Those rows can be pushed into scrollback by a resize and can never be
    /// reliably erased afterwards, so **never render secrets on this
    /// surface**. On the first resize the runner moves the app to the
    /// alternate screen — see [`go_fullscreen`] for why an inline viewport
    /// cannot be repaired once a terminal has reflowed around it.
    Inline { height: u16 },
    /// The full-window alternate screen: never enters scrollback, discarded
    /// on exit, trivially correct across resizes.
    AltScreen,
}

/// A TUI application driven by [`run`].
///
/// The implementor owns all state (selection, scroll, query, feedback, …).
/// The trait stays deliberately small: pick a surface, draw a frame, handle a
/// key. Anything more elaborate (callbacks, side effects) belongs in the
/// concrete type.
pub trait ScreenApp {
    /// Value produced on successful completion.
    type Output;

    /// Pick the surface to draw on, given the current terminal height. Called
    /// once, before the first frame.
    ///
    /// Defaults to [`Surface::AltScreen`]: the surface that cannot leak into
    /// scrollback, so choosing the inline one is always a deliberate act.
    fn surface(&self, _term_height: u16) -> Surface {
        Surface::AltScreen
    }

    /// Draw one frame into `buf` at `area`.
    fn render(&mut self, area: Rect, buf: &mut Buffer);

    /// Handle one key press.
    fn on_key(&mut self, key: KeyEvent) -> Flow<Self::Output>;

    /// Hook called before each draw. Useful for expiring transient UI
    /// (e.g. copy-feedback lines). Default: keep running.
    ///
    /// Returns a [`Flow`] for the same reason [`ScreenApp::on_key`] does: an
    /// app that watches something outside the keyboard — a clock, a socket, a
    /// file — may learn from a tick that there is nothing left to show, and
    /// waiting for a keystroke to act on that would mean drawing a frame the
    /// app has already disowned.
    fn on_tick(&mut self) -> Flow<Self::Output> {
        Flow::Continue
    }

    /// If `Some(d)`, the event loop polls for input with timeout `d`; on
    /// timeout it calls [`ScreenApp::on_tick`] and redraws without
    /// dispatching a key. Use this to drive timeout-based UI such as
    /// chord-prefix expiry or feedback fade. Default: `None` (blocking
    /// `event::read()`, no behavior change for existing apps).
    fn tick_interval(&self) -> Option<Duration> {
        None
    }
}

/// True if an interactive inline TUI can be shown — i.e. `/dev/tty` exists
/// and stderr is a terminal.
pub fn is_interactive() -> bool {
    std::path::Path::new("/dev/tty").exists() && std::io::stderr().is_terminal()
}

/// Run a TUI app on the surface it asks for. Returns the output from
/// [`Flow::Exit`], or `None` if the app [`Flow::Cancel`]led.
pub fn run<A: ScreenApp>(mut app: A) -> io::Result<Option<A::Output>> {
    let tty = File::options()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|e| io::Error::other(format!("cannot open /dev/tty: {e}")))?;

    let size = terminal::size().unwrap_or((80, 24));
    let surface = app.surface(size.1);

    let mut term = match surface {
        // Reserves the rows below the cursor by printing newlines, scrolling
        // earlier output up — what makes the widget part of the session.
        Surface::Inline { height } => Terminal::with_options(
            CrosstermBackend::new(tty.try_clone()?),
            TerminalOptions {
                viewport: Viewport::Inline(height),
            },
        )?,
        Surface::AltScreen => enter_alt_screen(&tty)?,
    };

    install_panic_hook();
    terminal::enable_raw_mode()?;
    term.hide_cursor()?;

    let mut alt_screen = matches!(surface, Surface::AltScreen);
    let result = event_loop(&mut term, &mut app, &tty, size, &mut alt_screen);

    if alt_screen {
        // Wipe the frame *before* handing the screen back. On a terminal where
        // the alternate screen is a no-op (tmux with `alternate-screen off`,
        // TERM=dumb) this is the only thing that erases what was on it — worth
        // clearing the visible screen for, since what's on it may be a secret.
        let _ = term.clear();
        let _ = term.show_cursor();
        let _ = terminal::disable_raw_mode();
        IN_ALT_SCREEN.store(false, Ordering::SeqCst);
        let _ = crossterm::execute!(
            term.backend_mut(),
            crossterm::terminal::LeaveAlternateScreen
        );
        if !matches!(surface, Surface::AltScreen) {
            // Started inline: the terminal restores the cursor to wherever the
            // last inline frame left it, mid-row. Break the line so the shell
            // prompt starts clean instead of continuing that row.
            let _ = term.backend_mut().write_all(b"\r\n");
            let _ = term.backend_mut().flush();
        }
    } else {
        // Still inline: clear the frame and leave the cursor at the top of the
        // viewport so the shell prompt reuses the rows the app occupied.
        let top = term.get_frame().area().y;
        let _ = term.show_cursor();
        let _ = terminal::disable_raw_mode();
        let _ = term.clear();
        let _ = crossterm::execute!(term.backend_mut(), crossterm::cursor::MoveTo(0, top));
    }
    drop(term);

    result
}

/// Switch the terminal to the alternate screen and build a fullscreen
/// ratatui terminal on it.
fn enter_alt_screen(tty: &File) -> io::Result<Terminal<CrosstermBackend<File>>> {
    let mut backend = CrosstermBackend::new(tty.try_clone()?);
    crossterm::execute!(backend, crossterm::terminal::EnterAlternateScreen)?;
    IN_ALT_SCREEN.store(true, Ordering::SeqCst);
    let mut term = Terminal::new(backend)?;
    term.clear()?;
    Ok(term)
}

/// Leave an inline viewport behind and take over the alternate screen.
///
/// Called on the first terminal resize of a [`Surface::Inline`] app, because
/// that is the point where an inline widget stops being repairable. Terminals
/// that reflow (kitty, VTE, alacritty, …) rewrap every line on resize and
/// reshuffle the visible region, and — measured, not assumed — the cursor does
/// *not* stay with the text it was parked on, so the app has no way to find
/// where its own last frame ended up. Repainting then means painting a second
/// frame somewhere else, which is a stack of stale copies; wiping wide enough
/// to be sure of catching the old frame means erasing the command output above
/// it.
///
/// The alternate screen has neither problem: it is never reflowed, resizing it
/// is an ordinary repaint, and on exit the terminal puts the shell screen back
/// exactly as it was. The primary screen is deliberately left untouched here —
/// the last inline frame stays sitting there like any other command's output,
/// which is also why an inline app must never have rendered a secret.
fn go_fullscreen(term: &mut Terminal<CrosstermBackend<File>>, tty: &File) -> io::Result<()> {
    *term = enter_alt_screen(tty)?;
    term.hide_cursor()?;
    Ok(())
}

static IN_ALT_SCREEN: AtomicBool = AtomicBool::new(false);
static PANIC_HOOK: Once = Once::new();

/// Restore the terminal if the app panics mid-frame. Without this a panic on
/// the alternate screen leaves the user staring at a dead full-screen frame in
/// a shell that is still in raw mode.
fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = terminal::disable_raw_mode();
            if IN_ALT_SCREEN.swap(false, Ordering::SeqCst)
                && let Ok(mut tty) = File::options().write(true).open("/dev/tty")
            {
                // Clear before leaving, for the same reason as the teardown in
                // `run`: if this terminal's alternate screen is a no-op, the
                // frame — secrets and all — would otherwise stay on screen.
                let _ = tty.write_all(b"\x1b[2J\x1b[H\x1b[?1049l\x1b[?25h");
            }
            previous(info);
        }));
    });
}

fn event_loop<A: ScreenApp>(
    term: &mut Terminal<CrosstermBackend<File>>,
    app: &mut A,
    tty: &File,
    initial_size: (u16, u16),
    alt_screen: &mut bool,
) -> io::Result<Option<A::Output>> {
    let mut last_size = initial_size;
    loop {
        // While inline, watch for the terminal changing size — the one thing
        // an inline viewport cannot survive. On the alternate screen ratatui's
        // own autoresize handles it, so stop paying for the check.
        if !*alt_screen {
            let size = terminal::size().unwrap_or(last_size);
            if size != last_size {
                last_size = size;
                go_fullscreen(term, tty)?;
                *alt_screen = true;
            }
        }

        match app.on_tick() {
            Flow::Continue => {}
            Flow::Exit(v) => return Ok(Some(v)),
            Flow::Cancel => return Ok(None),
        }

        crossterm::queue!(
            term.backend_mut(),
            crossterm::terminal::BeginSynchronizedUpdate
        )?;
        term.draw(|frame| {
            let area = frame.area();
            let buf = frame.buffer_mut();
            app.render(area, buf);
        })?;
        crossterm::queue!(
            term.backend_mut(),
            crossterm::terminal::EndSynchronizedUpdate
        )?;
        term.backend_mut().flush()?;

        if let Some(timeout) = app.tick_interval()
            && !event::poll(timeout)?
        {
            // Timeout: re-tick + redraw without dispatching a key.
            continue;
        }
        // Resize events need no handling of their own: they wake the loop up,
        // and the check at the top of the next iteration does the rest.
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match app.on_key(key) {
            Flow::Continue => continue,
            Flow::Exit(v) => return Ok(Some(v)),
            Flow::Cancel => return Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Defaults;
    impl ScreenApp for Defaults {
        type Output = ();
        fn render(&mut self, _: Rect, _: &mut Buffer) {}
        fn on_key(&mut self, _: KeyEvent) -> Flow<()> {
            Flow::Continue
        }
    }

    /// An app that says nothing about where it wants to draw must land on the
    /// surface that cannot leak into scrollback. Opting into `Inline` is a
    /// deliberate act, never a default.
    #[test]
    fn default_surface_is_the_alternate_screen() {
        for term_height in [0u16, 10, 24, 200] {
            assert_eq!(Defaults.surface(term_height), Surface::AltScreen);
        }
    }
}
