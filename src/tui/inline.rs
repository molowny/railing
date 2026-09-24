//! Inline TUI runner: `/dev/tty`, raw mode, draw + event loop, cleanup.
//!
//! This is the generic scaffolding shared by any mini-TUI that wants to
//! render a small widget inline below the cursor (no alternate screen).
//! Callers implement [`InlineApp`] and pass the instance to [`run`].

use std::fs::File;
use std::io::{self, IsTerminal, Write};
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use crossterm::terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};

/// Control-flow signal returned from [`InlineApp::on_key`].
pub enum Flow<T> {
    /// Keep looping.
    Continue,
    /// Exit the loop with a value. [`run`] returns `Ok(Some(value))`.
    Exit(T),
    /// Exit the loop with no value. [`run`] returns `Ok(None)`.
    Cancel,
}

/// An inline TUI application driven by [`run`].
///
/// The implementor owns all state (selection, scroll, query, feedback, …).
/// The trait stays deliberately small: size the viewport, draw a frame,
/// handle a key. Anything more elaborate (callbacks, side effects) belongs
/// in the concrete type.
pub trait InlineApp {
    /// Value produced on successful completion.
    type Output;

    /// Pick a viewport height given the current terminal height.
    /// Called before entering the event loop and again on every terminal
    /// resize, so it must be a pure function of `term_height`.
    fn initial_viewport_height(&self, term_height: u16) -> u16;

    /// Draw one frame into `buf` at `area`.
    fn render(&mut self, area: Rect, buf: &mut Buffer);

    /// Handle one key press.
    fn on_key(&mut self, key: KeyEvent) -> Flow<Self::Output>;

    /// Hook called before each draw. Useful for expiring transient UI
    /// (e.g. copy-feedback lines). Default: no-op.
    fn on_tick(&mut self) {}

    /// If `Some(d)`, the event loop polls for input with timeout `d`; on
    /// timeout it calls [`InlineApp::on_tick`] and redraws without
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

/// Run an inline TUI app. Returns the output from [`Flow::Exit`], or
/// `None` if the app [`Flow::Cancel`]led.
pub fn run<A: InlineApp>(mut app: A) -> io::Result<Option<A::Output>> {
    let tty = File::options()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|e| io::Error::other(format!("cannot open /dev/tty: {e}")))?;

    let term_height = terminal::size().map(|(_, h)| h).unwrap_or(24);
    let viewport_height = app.initial_viewport_height(term_height);

    let mut term = new_terminal(&tty, viewport_height)?;

    terminal::enable_raw_mode()?;
    term.hide_cursor()?;

    let result = event_loop(&mut term, &mut app, &tty, viewport_height);

    let _ = term.show_cursor();
    let _ = terminal::disable_raw_mode();
    let _ = term.clear();
    drop(term);

    result
}

fn new_terminal(tty: &File, viewport_height: u16) -> io::Result<Terminal<CrosstermBackend<File>>> {
    let backend = CrosstermBackend::new(tty.try_clone()?);
    Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Inline(viewport_height),
        },
    )
}

/// Re-create the terminal with a new inline viewport height.
///
/// `Viewport::Inline(h)` is fixed at construction: ratatui clamps it down to
/// the terminal height on resize but never grows it back past the height it
/// was built with. So a window that starts small keeps a small app forever
/// unless we rebuild. Clearing first leaves the cursor at the old viewport's
/// top-left, which is where the new viewport reserves its rows — the frame
/// re-anchors in place instead of marching down the scrollback.
fn resize_viewport(
    term: &mut Terminal<CrosstermBackend<File>>,
    tty: &File,
    viewport_height: u16,
) -> io::Result<()> {
    term.clear()?;
    term.backend_mut().flush()?;
    *term = new_terminal(tty, viewport_height)?;
    term.hide_cursor()?;
    Ok(())
}

fn event_loop<A: InlineApp>(
    term: &mut Terminal<CrosstermBackend<File>>,
    app: &mut A,
    tty: &File,
    initial_viewport_height: u16,
) -> io::Result<Option<A::Output>> {
    let mut viewport_height = initial_viewport_height;
    loop {
        app.on_tick();

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
        let key = match event::read()? {
            Event::Key(k) => k,
            // Ratatui's autoresize handles the width and clamps the height on
            // the next draw; the viewport only has to be rebuilt when the app
            // now wants a different height (e.g. the window grew).
            Event::Resize(_, h) => {
                let desired = app.initial_viewport_height(h);
                if desired != viewport_height {
                    viewport_height = desired;
                    resize_viewport(term, tty, desired)?;
                }
                continue;
            }
            _ => continue,
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
