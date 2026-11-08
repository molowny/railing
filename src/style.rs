//! Styling text for one terminal: SGR colours and attributes, painted against
//! that terminal's [`Caps`], plus the width arithmetic that ignores escapes.
//!
//! There is no process-wide colour switch here. Whether colour is drawn is a
//! property of the terminal being drawn on, so every call that styles takes
//! the [`Caps`] of that terminal: two terminals in one process (a flow on
//! stderr, an unlock prompt on `$SEALWARD_TTY`) can disagree, and neither
//! follows a flag that belongs to stdout.
//!
//! ```
//! use cli_kit::style::{Caps, Style};
//!
//! let cyan = Style::new().cyan();
//! assert_eq!(cyan.paint(Caps::COLOR, "◆"), "\x1b[36m◆\x1b[0m");
//! assert_eq!(cyan.paint(Caps::PLAIN, "◆"), "◆");
//! ```
//!
//! The bytes are the ones `console` wrote for the same style (foreground,
//! background, then each attribute in order, one reset at the end), which
//! the kit drew with until M5: the golden fixtures hold every surface to
//! them.

use std::borrow::Cow;
use std::fmt::{Display, Write as _};

use unicode_width::UnicodeWidthChar;

/// What a terminal can be asked to do, decided once for that terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// Draw colours and attributes.
    pub color: bool,
    /// Move the cursor: replace a block in place, animate a line. Off, output
    /// is appended and never redrawn.
    pub redraw: bool,
}

impl Caps {
    /// A terminal that takes neither: a pipe, a file, `TERM=dumb`.
    pub const PLAIN: Caps = Caps {
        color: false,
        redraw: false,
    };
    /// Colour and cursor movement.
    pub const COLOR: Caps = Caps {
        color: true,
        redraw: true,
    };

    /// The caps of a terminal from what is known about it: whether it is a
    /// terminal at all, and the environment of whoever owns it.
    ///
    /// `env` is passed, never read here: for a terminal this process did not
    /// inherit, its owner's `TERM` and `NO_COLOR` are what count, not ours.
    pub fn detect(is_tty: bool, env: &TermEnv) -> Caps {
        let dumb = env
            .term
            .as_deref()
            .is_none_or(|t| t.is_empty() || t == "dumb");
        let redraw = is_tty && !dumb;
        Caps {
            color: redraw && !env.no_color,
            redraw,
        }
    }
}

/// The parts of an environment that decide a terminal's [`Caps`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TermEnv {
    /// `TERM`. Unset or `dumb`: no colour, no cursor movement.
    pub term: Option<String>,
    /// `NO_COLOR` set to anything but the empty string (no-color.org).
    pub no_color: bool,
}

impl TermEnv {
    /// This process's own `TERM` and `NO_COLOR`: right for a terminal it
    /// inherited (stderr), and only by assumption for any other.
    ///
    /// The one place the core reads the environment.
    pub fn of_process() -> TermEnv {
        TermEnv {
            term: std::env::var("TERM").ok(),
            no_color: std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()),
        }
    }
}

/// One of the eight colours, or an index into the 256-colour palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    /// `38;5;n` / `48;5;n`.
    Ansi256(u8),
    /// 24-bit, `38;2;r;g;b` / `48;2;r;g;b`. Drawn as given: whether the
    /// terminal takes it is the caller's to know ([`crate::theme`] downgrades
    /// it to [`Color::Ansi256`] where it doesn't).
    Rgb(u8, u8, u8),
}

impl Color {
    fn index(self) -> u8 {
        match self {
            Self::Black => 0,
            Self::Red => 1,
            Self::Green => 2,
            Self::Yellow => 3,
            Self::Blue => 4,
            Self::Magenta => 5,
            Self::Cyan => 6,
            Self::White => 7,
            Self::Ansi256(n) => n,
            Self::Rgb(..) => unreachable!("written by `sgr` itself"),
        }
    }
}

/// Attributes, in the order they are written.
const BOLD: u8 = 1 << 0;
const DIM: u8 = 1 << 1;
const ITALIC: u8 = 1 << 2;
const UNDERLINE: u8 = 1 << 3;
const REVERSE: u8 = 1 << 4;
const STRIKETHROUGH: u8 = 1 << 5;
const SGR: [(u8, u8); 6] = [
    (BOLD, 1),
    (DIM, 2),
    (ITALIC, 3),
    (UNDERLINE, 4),
    (REVERSE, 7),
    (STRIKETHROUGH, 9),
];

/// A style: built with `const` methods, painted with [`Style::paint`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    fg: Option<Color>,
    fg_bright: bool,
    bg: Option<Color>,
    bg_bright: bool,
    attrs: u8,
}

macro_rules! colors {
    ($($fg:ident $bg:ident $color:ident),* $(,)?) => {$(
        pub const fn $fg(mut self) -> Self { self.fg = Some(Color::$color); self }
        pub const fn $bg(mut self) -> Self { self.bg = Some(Color::$color); self }
    )*};
}

impl Style {
    pub const fn new() -> Self {
        Self {
            fg: None,
            fg_bright: false,
            bg: None,
            bg_bright: false,
            attrs: 0,
        }
    }

    colors! {
        black on_black Black, red on_red Red, green on_green Green,
        yellow on_yellow Yellow, blue on_blue Blue, magenta on_magenta Magenta,
        cyan on_cyan Cyan, white on_white White,
    }

    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    /// A 256-colour foreground (`38;5;n`).
    pub const fn color256(self, n: u8) -> Self {
        self.fg(Color::Ansi256(n))
    }

    /// The bright variant of the foreground's colour (`bright().black()` is
    /// grey, `38;5;8`).
    pub const fn bright(mut self) -> Self {
        self.fg_bright = true;
        self
    }

    pub const fn on_bright(mut self) -> Self {
        self.bg_bright = true;
        self
    }

    pub const fn bold(mut self) -> Self {
        self.attrs |= BOLD;
        self
    }

    pub const fn dim(mut self) -> Self {
        self.attrs |= DIM;
        self
    }

    pub const fn italic(mut self) -> Self {
        self.attrs |= ITALIC;
        self
    }

    pub const fn underline(mut self) -> Self {
        self.attrs |= UNDERLINE;
        self
    }

    pub const fn reverse(mut self) -> Self {
        self.attrs |= REVERSE;
        self
    }

    pub const fn strikethrough(mut self) -> Self {
        self.attrs |= STRIKETHROUGH;
        self
    }

    /// `text` in this style on a terminal with `caps`; unstyled where it
    /// takes no colour.
    pub fn paint(&self, caps: Caps, text: impl Display) -> String {
        let mut out = String::new();
        let styled = caps.color && *self != Style::new();
        if styled {
            if let Some(fg) = self.fg {
                sgr(&mut out, 30, fg, self.fg_bright);
            }
            if let Some(bg) = self.bg {
                sgr(&mut out, 40, bg, self.bg_bright);
            }
            for (bit, code) in SGR {
                if self.attrs & bit != 0 {
                    let _ = write!(out, "\x1b[{code}m");
                }
            }
        }
        let _ = write!(out, "{text}");
        if styled {
            out.push_str("\x1b[0m");
        }
        out
    }
}

/// One colour's SGR: `3n`/`4n` for the eight, `38;5;n`/`48;5;n` for the
/// palette and the bright eight.
fn sgr(out: &mut String, base: u8, color: Color, bright: bool) {
    let ext = base + 8;
    let _ = match color {
        Color::Ansi256(n) => write!(out, "\x1b[{ext};5;{n}m"),
        Color::Rgb(r, g, b) => write!(out, "\x1b[{ext};2;{r};{g};{b}m"),
        c if bright => write!(out, "\x1b[{ext};5;{}m", c.index() + 8),
        c => write!(out, "\x1b[{}m", base + c.index()),
    };
}

/// Where the escape sequence starting at `s[i]` (an ESC) ends: CSI to its
/// final byte, OSC to BEL or ST, anything else the one character after ESC.
fn escape_end(s: &str, i: usize) -> usize {
    let b = s.as_bytes();
    match b.get(i + 1) {
        Some(b'[') => {
            let mut j = i + 2;
            while j < b.len() && !(0x40..=0x7e).contains(&b[j]) {
                j += 1;
            }
            (j + 1).min(b.len())
        }
        Some(b']') => {
            let mut j = i + 2;
            while j < b.len() {
                if b[j] == 0x07 {
                    return j + 1;
                }
                if b[j] == 0x1b && b.get(j + 1) == Some(&b'\\') {
                    return j + 2;
                }
                j += 1;
            }
            b.len()
        }
        Some(_) => {
            let next = s[i + 1..].chars().next().map_or(0, char::len_utf8);
            i + 1 + next
        }
        None => b.len(),
    }
}

/// The pieces of `s`: text runs, and escape sequences (`true`).
fn pieces(s: &str) -> impl Iterator<Item = (&str, bool)> {
    let mut i = 0;
    std::iter::from_fn(move || {
        if i >= s.len() {
            return None;
        }
        let start = i;
        if s.as_bytes()[i] == 0x1b {
            i = escape_end(s, i);
            return Some((&s[start..i], true));
        }
        i = s[i..].find('\x1b').map_or(s.len(), |n| i + n);
        Some((&s[start..i], false))
    })
}

fn char_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// Columns `s` takes on a terminal, escape sequences not counted.
pub fn measure(s: &str) -> usize {
    pieces(s)
        .filter(|(_, esc)| !esc)
        .flat_map(|(t, _)| t.chars())
        .map(char_width)
        .sum()
}

/// `s` without its escape sequences.
pub fn strip(s: &str) -> Cow<'_, str> {
    if !s.contains('\x1b') {
        return Cow::Borrowed(s);
    }
    Cow::Owned(pieces(s).filter(|(_, esc)| !esc).map(|(t, _)| t).collect())
}

/// `s` cut to `width` columns, `tail` (`…`) marking the cut. Text that fits
/// is returned as it is. Escape sequences are kept, those after the cut
/// too, so a reset that closes a style still closes it.
pub fn truncate<'a>(s: &'a str, width: usize, tail: &str) -> Cow<'a, str> {
    if measure(s) <= width {
        return Cow::Borrowed(s);
    }
    let room = width.saturating_sub(measure(tail));
    let mut out = String::new();
    let mut used = 0;
    let mut cut = false;
    for (piece, esc) in pieces(s) {
        if esc {
            out.push_str(piece);
            continue;
        }
        if cut {
            continue;
        }
        for c in piece.chars() {
            let w = char_width(c);
            if used + w > room {
                cut = true;
                out.push_str(tail);
                break;
            }
            used += w;
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// `s` padded with spaces to `width` columns (left-aligned); as it is when
/// already that wide.
pub fn pad(s: &str, width: usize) -> String {
    let w = measure(s);
    let mut out = s.to_string();
    out.extend(std::iter::repeat_n(' ', width.saturating_sub(w)));
    out
}

/// Text styled per character (a cell or note in runs) as lines no wider than `width`, each character keeping its
/// style: broken at `\n` and wherever the width runs out, never at words.
/// `text` may be shorter than `styles` (a clipped column's `…`, which takes
/// the default style).
pub(crate) fn fit_runs<S: Copy + Default>(
    text: &str,
    styles: &[S],
    width: usize,
) -> (Vec<String>, Vec<Vec<S>>) {
    let mut lines = vec![String::new()];
    let mut line_styles = vec![Vec::new()];
    let mut used = 0;
    for (i, ch) in text.chars().enumerate() {
        if ch == '\n' {
            lines.push(String::new());
            line_styles.push(Vec::new());
            used = 0;
            continue;
        }
        let w = measure(ch.encode_utf8(&mut [0; 4]));
        if used + w > width.max(1) && used > 0 {
            lines.push(String::new());
            line_styles.push(Vec::new());
            used = 0;
        }
        lines.last_mut().expect("one line").push(ch);
        line_styles
            .last_mut()
            .expect("one line")
            .push(styles.get(i).copied().unwrap_or_default());
        used += w;
    }
    (lines, line_styles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_come_from_the_terminal_and_its_owner_s_env() {
        let env = |term: Option<&str>, no_color| TermEnv {
            term: term.map(Into::into),
            no_color,
        };
        assert_eq!(
            Caps::detect(true, &env(Some("xterm-256color"), false)),
            Caps::COLOR
        );
        assert_eq!(Caps::detect(false, &env(Some("xterm"), false)), Caps::PLAIN);
        assert_eq!(Caps::detect(true, &env(Some("dumb"), false)), Caps::PLAIN);
        assert_eq!(Caps::detect(true, &env(None, false)), Caps::PLAIN);
        assert_eq!(
            Caps::detect(true, &env(Some("xterm"), true)),
            Caps {
                color: false,
                redraw: true
            }
        );
    }

    #[test]
    fn sgr_bytes_match_console() {
        let c = Caps::COLOR;
        assert_eq!(Style::new().cyan().paint(c, "x"), "\x1b[36mx\x1b[0m");
        assert_eq!(
            Style::new().bright().black().paint(c, "x"),
            "\x1b[38;5;8mx\x1b[0m"
        );
        assert_eq!(
            Style::new().color256(246).paint(c, "x"),
            "\x1b[38;5;246mx\x1b[0m"
        );
        assert_eq!(
            Style::new().bold().red().on_blue().dim().paint(c, "x"),
            "\x1b[31m\x1b[44m\x1b[1m\x1b[2mx\x1b[0m"
        );
        assert_eq!(Style::new().paint(c, "x"), "x");
        assert_eq!(Style::new().red().bold().paint(Caps::PLAIN, "x"), "x");
        // What console wrote for the same style, byte for byte (compared
        // against console itself until it left, core refactor M5).
        assert_eq!(
            Style::new().yellow().bold().italic().paint(c, "w"),
            "\x1b[33m\x1b[1m\x1b[3mw\x1b[0m"
        );
    }

    #[test]
    fn widths_ignore_escapes_and_count_wide_characters() {
        let red = Style::new().red().paint(Caps::COLOR, "abc");
        assert_eq!(measure(&red), 3);
        assert_eq!(measure("日本"), 4);
        assert_eq!(measure("\x1b]8;;https://x\x07link\x1b]8;;\x07"), 4);
        assert_eq!(strip(&red), "abc");
        assert_eq!(pad(&red, 5), format!("{red}  "));
    }

    #[test]
    fn truncate_cuts_only_what_does_not_fit_and_keeps_the_reset() {
        assert_eq!(truncate("abcd", 4, "…"), "abcd");
        assert_eq!(truncate("abcdef", 4, "…"), "abc…");
        assert_eq!(truncate("日本語", 5, "…"), "日本…");
        let red = Style::new().red().paint(Caps::COLOR, "abcdef");
        let cut = truncate(&red, 4, "…");
        assert_eq!(cut, "\x1b[31mabc…\x1b[0m");
        assert_eq!(measure(&cut), 4);
        assert_eq!(truncate("abc", 0, "…"), "…");
    }
}
