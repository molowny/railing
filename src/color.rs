//! This process's colour decision for its own two streams, and text styled
//! against it: the full kit's `style("x").green().for_stderr()`, the API
//! console gave it, over the core's [`crate::style`].
//!
//! The core takes a terminal's [`Caps`] at every call and holds no switch.
//! A command's own stdout and stderr are different: there is one of each per
//! process, every line the full kit prints goes to one of them, and an app
//! decides for both once (a `--color` flag, [`crate::ui::init_colors`]). So
//! the decision for those two lives here, per stream, and a [`Styled`] value
//! paints when it is displayed, against the stream it is bound to (stdout
//! unless [`Styled::for_stderr`]). Nothing here reaches a terminal other than
//! the process's own; a prompt on another one (`$SEALWARD_TTY`) paints with
//! that terminal's caps through the core.
//!
//! Until an app decides, each stream's colour is [`Caps::detect`]'s for it:
//! a terminal, `TERM` not unset, empty or `dumb`, `NO_COLOR` not set.
//!
//! ```
//! use railing::color::{self, style};
//!
//! color::set(true, false);
//! assert_eq!(style("ok").green().to_string(), "\x1b[32mok\x1b[0m");
//! assert_eq!(style("ok").green().for_stderr().to_string(), "ok");
//! ```

use std::fmt::{self, Display};
use std::io::IsTerminal;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::style::{self as core, Caps, Color, TermEnv};

/// One of this process's two output streams.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stream {
    #[default]
    Stdout,
    Stderr,
}

/// `0` not decided yet, `1` off, `2` on.
static STDOUT: AtomicU8 = AtomicU8::new(0);
static STDERR: AtomicU8 = AtomicU8::new(0);

fn flag(stream: Stream) -> &'static AtomicU8 {
    match stream {
        Stream::Stdout => &STDOUT,
        Stream::Stderr => &STDERR,
    }
}

/// Whether `stream` gets colour.
pub fn enabled(stream: Stream) -> bool {
    match flag(stream).load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let on = detect(stream);
            // A decision made meanwhile wins over the detected default.
            let _ = flag(stream).compare_exchange(
                0,
                if on { 2 } else { 1 },
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
            flag(stream).load(Ordering::Relaxed) == 2
        }
    }
}

/// Decide colour for one stream.
pub fn set_stream(stream: Stream, on: bool) {
    flag(stream).store(if on { 2 } else { 1 }, Ordering::Relaxed);
}

/// Decide colour for stdout and stderr.
pub fn set(stdout: bool, stderr: bool) {
    set_stream(Stream::Stdout, stdout);
    set_stream(Stream::Stderr, stderr);
}

/// The caps to paint `stream` with: its colour decision, and cursor
/// movement, which only a caller drawing on a terminal uses.
pub fn caps(stream: Stream) -> Caps {
    Caps {
        color: enabled(stream),
        redraw: true,
    }
}

fn detect(stream: Stream) -> bool {
    let is_tty = match stream {
        Stream::Stdout => std::io::stdout().is_terminal(),
        Stream::Stderr => std::io::stderr().is_terminal(),
    };
    Caps::detect(is_tty, &TermEnv::of_process()).color
}

/// The builder methods, on both [`Style`] and [`Styled`].
macro_rules! builders {
    ($($name:ident),* $(,)?) => {
        $(
            #[must_use]
            pub fn $name(mut self) -> Self {
                self.style_mut().0 = self.style_mut().0.$name();
                self
            }
        )*

        #[must_use]
        pub fn fg(mut self, color: Color) -> Self {
            self.style_mut().0 = self.style_mut().0.fg(color);
            self
        }

        #[must_use]
        pub fn bg(mut self, color: Color) -> Self {
            self.style_mut().0 = self.style_mut().0.bg(color);
            self
        }

        /// A theme role as the foreground (`style(x).swatch(theme::current().danger)`):
        /// its colour for this process's background, nothing for the
        /// terminal's own.
        #[must_use]
        pub fn swatch(mut self, swatch: crate::theme::Swatch) -> Self {
            if let Some(c) = swatch.color() {
                self.style_mut().0 = self.style_mut().0.fg(c);
            }
            self
        }

        #[must_use]
        pub fn color256(mut self, n: u8) -> Self {
            self.style_mut().0 = self.style_mut().0.color256(n);
            self
        }

        /// console's name for [`Self::underline`].
        #[must_use]
        pub fn underlined(self) -> Self {
            self.underline()
        }

        /// Paint on stderr's decision.
        #[must_use]
        pub fn for_stderr(mut self) -> Self {
            self.style_mut().1 = Stream::Stderr;
            self
        }

        /// Paint on stdout's decision (the default).
        #[must_use]
        pub fn for_stdout(mut self) -> Self {
            self.style_mut().1 = Stream::Stdout;
            self
        }

        /// Paint whatever the stream's decision (`true`), or follow it again.
        #[must_use]
        pub fn force_styling(mut self, force: bool) -> Self {
            self.style_mut().2 = force;
            self
        }
    };
}

/// A core style bound to a stream, painted when applied and displayed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style(core::Style, Stream, bool);

impl Style {
    pub const fn new() -> Style {
        Style(core::Style::new(), Stream::Stdout, false)
    }

    fn style_mut(&mut self) -> &mut Style {
        self
    }

    builders!(
        black,
        red,
        green,
        yellow,
        blue,
        magenta,
        cyan,
        white,
        on_black,
        on_red,
        on_green,
        on_yellow,
        on_blue,
        on_magenta,
        on_cyan,
        on_white,
        bright,
        on_bright,
        bold,
        dim,
        italic,
        underline,
        reverse,
        strikethrough,
    );

    /// `value` in this style.
    pub fn apply_to<D>(self, value: D) -> Styled<D> {
        Styled { style: self, value }
    }

    /// The caps this style paints with now.
    fn caps(self) -> Caps {
        if self.2 { Caps::COLOR } else { caps(self.1) }
    }

    /// `text` painted now, against the stream's decision.
    pub fn paint(self, text: impl Display) -> String {
        self.0.paint(self.caps(), text)
    }
}

/// A value and the style it is displayed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Styled<D> {
    style: Style,
    value: D,
}

impl<D> Styled<D> {
    fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }

    builders!(
        black,
        red,
        green,
        yellow,
        blue,
        magenta,
        cyan,
        white,
        on_black,
        on_red,
        on_green,
        on_yellow,
        on_blue,
        on_magenta,
        on_cyan,
        on_white,
        bright,
        on_bright,
        bold,
        dim,
        italic,
        underline,
        reverse,
        strikethrough,
    );
}

impl<D: Display> Display for Styled<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.style.paint(&self.value))
    }
}

/// `value`, to be styled: `style("ok").green().for_stderr()`.
pub fn style<D>(value: D) -> Styled<D> {
    Style::new().apply_to(value)
}

/// `line` with each character in its style, a run of one style painted once.
pub(crate) fn paint_runs(line: &str, styles: &[Style]) -> String {
    let mut out = String::new();
    let mut run = String::new();
    let mut current: Option<Style> = None;
    for (ch, style) in line.chars().zip(styles.iter().copied()) {
        if current.is_some_and(|c| c != style) {
            out.push_str(&current.expect("set").force_styling(true).paint(&run));
            run.clear();
        }
        current = Some(style);
        run.push(ch);
    }
    if let Some(style) = current {
        out.push_str(&style.force_styling(true).paint(&run));
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The decision is process-wide: tests that set it hold this lock.
    pub(crate) fn lock() -> std::sync::MutexGuard<'static, ()> {
        static M: Mutex<()> = Mutex::new(());
        M.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn each_stream_follows_its_own_decision() {
        let _g = lock();
        set(true, false);
        assert_eq!(style("x").red().to_string(), "\x1b[31mx\x1b[0m");
        assert_eq!(style("x").red().for_stderr().to_string(), "x");
        set(false, true);
        assert_eq!(style("x").red().to_string(), "x");
        assert_eq!(
            Style::new().bold().for_stderr().apply_to("x").to_string(),
            "\x1b[1mx\x1b[0m"
        );
    }

    #[test]
    fn forced_styling_ignores_the_decision() {
        let _g = lock();
        set(false, false);
        assert_eq!(
            Style::new()
                .green()
                .force_styling(true)
                .apply_to("x")
                .to_string(),
            "\x1b[32mx\x1b[0m"
        );
    }

    /// The bytes console wrote for the same styles, which the golden
    /// fixtures hold every surface to.
    #[test]
    fn the_bytes_are_consoles() {
        let _g = lock();
        set(true, true);
        assert_eq!(
            style("x").color256(246).to_string(),
            "\x1b[38;5;246mx\x1b[0m"
        );
        assert_eq!(
            style("x").bright().black().to_string(),
            "\x1b[38;5;8mx\x1b[0m"
        );
        assert_eq!(
            style("x").cyan().underlined().to_string(),
            "\x1b[36m\x1b[4mx\x1b[0m"
        );
        assert_eq!(style("x").to_string(), "x");
    }
}
