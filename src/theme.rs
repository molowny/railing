//! The colours the kit draws with, by role, and the app's say over them.
//!
//! Kit code never names a colour: it paints a role ([`muted`], [`active`],
//! [`Theme::selection`] …). The process has one [`Theme`], which an app may
//! [`install`], and one [`Background`], which [`detect`] decides once at
//! start. A role is a [`Swatch`]: one [`Ink`] for a dark background and one
//! for a light one.
//!
//! Nothing here decides *whether* colour is drawn: that stays per stream
//! ([`crate::color`]) and per terminal ([`crate::style::Caps`]).
//!
//! ```
//! use railing::style::{Caps, Color};
//! use railing::theme::{self, Background, Ink, Swatch};
//!
//! let mut t = theme::Theme::default();
//! t.accent = Swatch::both(Ink::Color(Color::Magenta));
//! theme::install(t);
//! assert_eq!(theme::accent().paint(Caps::COLOR, "x"), "\x1b[35mx\x1b[0m");
//! theme::set_background(Background::Light);
//! assert_eq!(theme::muted().paint(Caps::COLOR, "x"), "\x1b[38;5;240mx\x1b[0m");
//! # theme::install(theme::Theme::default());
//! # theme::set_background(Background::Dark);
//! ```

use std::sync::RwLock;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::style::{Color, Style};

/// The terminal's background, as far as the kit knows: what each
/// [`Swatch`] picks its [`Ink`] by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Background {
    #[default]
    Dark,
    Light,
}

impl Background {
    /// The background for a colour, by its relative luminance (sRGB).
    pub fn of_rgb(r: u8, g: u8, b: u8) -> Background {
        let lin = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let y = 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
        if y > 0.5 {
            Background::Light
        } else {
            Background::Dark
        }
    }
}

/// One colour for one background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// The terminal's own foreground (or background, as a background).
    Terminal,
    /// One of the 16 (painted by the user's theme) or a 256-palette index
    /// (fixed).
    Color(Color),
    /// 24-bit. Drawn as such where the terminal takes it (`COLORTERM` is
    /// `truecolor` or `24bit`), as the nearest 256-palette entry elsewhere.
    Rgb(u8, u8, u8),
}

impl Ink {
    /// A 256-palette index.
    pub const fn ansi256(n: u8) -> Ink {
        Ink::Color(Color::Ansi256(n))
    }

    /// The colour to draw, `None` for the terminal's own.
    pub fn color(self) -> Option<Color> {
        match self {
            Ink::Terminal => None,
            Ink::Color(c) => Some(c),
            Ink::Rgb(r, g, b) if truecolor() => Some(Color::Rgb(r, g, b)),
            Ink::Rgb(r, g, b) => Some(Color::Ansi256(nearest_256(r, g, b))),
        }
    }
}

/// A role's ink on each background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Swatch {
    pub dark: Ink,
    pub light: Ink,
}

impl Swatch {
    pub const fn new(dark: Ink, light: Ink) -> Swatch {
        Swatch { dark, light }
    }

    /// The same ink on both backgrounds.
    pub const fn both(ink: Ink) -> Swatch {
        Swatch {
            dark: ink,
            light: ink,
        }
    }

    /// The ink for this process's background.
    pub fn ink(self) -> Ink {
        match background() {
            Background::Dark => self.dark,
            Background::Light => self.light,
        }
    }

    /// The colour for this process's background, `None` for the terminal's
    /// own.
    pub fn color(self) -> Option<Color> {
        self.ink().color()
    }

    /// A style with this swatch as its foreground.
    pub fn style(self) -> Style {
        match self.color() {
            Some(c) => Style::new().fg(c),
            None => Style::new(),
        }
    }

    /// A style with this swatch as its background.
    pub fn on(self) -> Style {
        match self.color() {
            Some(c) => Style::new().bg(c),
            None => Style::new(),
        }
    }
}

/// Every colour the kit draws, by role. The
/// default is the kit's own look: on a dark background, exactly what it drew
/// before themes; on a light one, the greys moved so they still read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// The TUI's primary text.
    pub text: Swatch,
    /// TUI subtitles and descriptions, an unfocused pane's cursor row.
    pub text_secondary: Swatch,
    /// Ids, timestamps, hints: [`crate::Tone::Muted`], the spinner's glow,
    /// the TUI's dim text.
    pub muted: Swatch,
    /// [`crate::Tone::Accent`], keycaps, group titles.
    pub accent: Swatch,
    /// Search and filter queries, mode titles.
    pub highlight: Swatch,
    /// The open step: its `◆`, bar and corner, an open box.
    pub active: Swatch,
    /// A submitted step's `◇`.
    pub done: Swatch,
    /// A finished step's bar and box.
    pub rule: Swatch,
    pub success: Swatch,
    pub warning: Swatch,
    pub danger: Swatch,
    /// A group's `●` info mark.
    pub info: Swatch,
    /// The TUI cursor row's background.
    pub selection: Swatch,
    /// An unfocused pane's cursor row background.
    pub selection_inactive: Swatch,
    /// Dim text on [`Theme::selection`].
    pub muted_on_selection: Swatch,
    /// Lines between panes.
    pub separator: Swatch,
}

const fn c(color: Color) -> Ink {
    Ink::Color(color)
}

impl Theme {
    /// The kit's look (also [`Default`]): clack's colours for the flow, the
    /// 16 ANSI colours for meaning, fixed greys where a theme's slot would
    /// be unpredictable.
    pub const fn kit() -> Theme {
        Theme {
            text: Swatch::new(c(Color::Ansi256(15)), Ink::Terminal),
            text_secondary: Swatch::new(c(Color::White), Ink::ansi256(242)),
            // F1 (sealward docs/identity.md §7.4): 246 is 2.77 : 1 on a light
            // background; 240 is 6.5 : 1 there.
            muted: Swatch::new(Ink::ansi256(246), Ink::ansi256(240)),
            accent: Swatch::both(c(Color::Cyan)),
            highlight: Swatch::both(c(Color::Yellow)),
            active: Swatch::both(c(Color::Cyan)),
            done: Swatch::both(c(Color::Green)),
            rule: Swatch::both(Ink::ansi256(8)),
            success: Swatch::both(c(Color::Green)),
            warning: Swatch::both(c(Color::Yellow)),
            danger: Swatch::both(c(Color::Red)),
            info: Swatch::both(c(Color::Blue)),
            selection: Swatch::new(Ink::ansi256(240), Ink::ansi256(252)),
            selection_inactive: Swatch::new(Ink::ansi256(237), Ink::ansi256(254)),
            muted_on_selection: Swatch::new(Ink::ansi256(248), Ink::ansi256(243)),
            separator: Swatch::new(Ink::ansi256(238), Ink::ansi256(250)),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::kit()
    }
}

static THEME: RwLock<Theme> = RwLock::new(Theme::kit());
/// `0` dark (the default), `1` light.
static BACKGROUND: AtomicU8 = AtomicU8::new(0);
/// `0` not looked at yet, `1` no, `2` yes.
static TRUECOLOR: AtomicU8 = AtomicU8::new(0);

// The kit's own unit tests run in parallel threads of one process: there the
// theme and background are per thread, so a test that sets them cannot
// change what another one draws.
#[cfg(test)]
thread_local! {
    static TEST_THEME: std::cell::Cell<Theme> = const { std::cell::Cell::new(Theme::kit()) };
    static TEST_BACKGROUND: std::cell::Cell<Background> =
        const { std::cell::Cell::new(Background::Dark) };
}

/// Make `theme` this process's: every role drawn from now on is its.
pub fn install(theme: Theme) {
    #[cfg(test)]
    {
        TEST_THEME.set(theme);
        let _ = &THEME;
    }
    #[cfg(not(test))]
    {
        *THEME.write().unwrap_or_else(|e| e.into_inner()) = theme;
    }
}

/// This process's theme.
pub fn current() -> Theme {
    #[cfg(test)]
    {
        TEST_THEME.get()
    }
    #[cfg(not(test))]
    {
        *THEME.read().unwrap_or_else(|e| e.into_inner())
    }
}

/// Decide the background (an app's own setting, a test).
pub fn set_background(bg: Background) {
    #[cfg(test)]
    {
        TEST_BACKGROUND.set(bg);
        let _ = &BACKGROUND;
    }
    #[cfg(not(test))]
    BACKGROUND.store(bg as u8, Ordering::Relaxed);
}

/// This process's background: [`Background::Dark`] until decided.
pub fn background() -> Background {
    #[cfg(test)]
    {
        TEST_BACKGROUND.get()
    }
    #[cfg(not(test))]
    match BACKGROUND.load(Ordering::Relaxed) {
        1 => Background::Light,
        _ => Background::Dark,
    }
}

/// Decide the background once, before anything is drawn, and return it:
///
/// 1. `setting`, when it says `dark` or `light` (any case);
/// 2. otherwise the terminal on stderr, asked (OSC 11, at most 150 ms), when
///    stdin and stderr are that terminal and stderr takes colour;
/// 3. `COLORFGBG`'s background (rxvt, Konsole);
/// 4. dark.
///
/// What `setting` is called and where it comes from (an env var, a config
/// key) is the app's.
#[cfg(feature = "flow")]
pub fn detect(setting: Option<&str>) -> Background {
    let bg = match setting.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("dark") => Background::Dark,
        Some("light") => Background::Light,
        _ => ask_terminal()
            .or_else(|| colorfgbg(std::env::var("COLORFGBG").ok().as_deref()))
            .unwrap_or_default(),
    };
    set_background(bg);
    bg
}

#[cfg(all(feature = "flow", unix))]
fn ask_terminal() -> Option<Background> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() || !crate::color::enabled(crate::color::Stream::Stderr) {
        return None;
    }
    let mut tty = crate::term::Tty::stderr().ok()?;
    let (r, g, b) = tty.background_color(std::time::Duration::from_millis(150))?;
    Some(Background::of_rgb(r, g, b))
}

#[cfg(all(feature = "flow", not(unix)))]
fn ask_terminal() -> Option<Background> {
    None
}

/// `COLORFGBG` is `fg;bg` (sometimes `fg;x;bg`): a background of 7 or
/// 9–15 is a light one.
#[cfg(any(feature = "flow", test))]
fn colorfgbg(value: Option<&str>) -> Option<Background> {
    let bg: u8 = value?.rsplit(';').next()?.trim().parse().ok()?;
    Some(if bg == 7 || (9..=15).contains(&bg) {
        Background::Light
    } else {
        Background::Dark
    })
}

/// Whether the terminal takes 24-bit colour (`COLORTERM`).
pub fn truecolor() -> bool {
    match TRUECOLOR.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let yes = std::env::var("COLORTERM")
                .is_ok_and(|v| matches!(v.as_str(), "truecolor" | "24bit"));
            TRUECOLOR.store(if yes { 2 } else { 1 }, Ordering::Relaxed);
            yes
        }
    }
}

/// Decide 24-bit colour (a `--color` flag that says so, a test).
pub fn set_truecolor(yes: bool) {
    TRUECOLOR.store(if yes { 2 } else { 1 }, Ordering::Relaxed);
}

/// The nearest entry of the 256-colour cube or grey ramp (16–255).
fn nearest_256(r: u8, g: u8, b: u8) -> u8 {
    const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let near = |v: u8| {
        (0..6)
            .min_by_key(|&i| (i32::from(STEPS[i]) - i32::from(v)).abs())
            .unwrap_or(0)
    };
    let (ri, gi, bi) = (near(r), near(g), near(b));
    let cube = (STEPS[ri], STEPS[gi], STEPS[bi]);
    let avg = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let gi_ramp = ((avg.saturating_sub(8)) / 10).min(23) as u8;
    let grey = 8 + 10 * gi_ramp;
    let dist = |(x, y, z): (u8, u8, u8)| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(x, r) + d(y, g) + d(z, b)
    };
    if dist((grey, grey, grey)) < dist(cube) {
        232 + gi_ramp
    } else {
        16 + 36 * ri as u8 + 6 * gi as u8 + bi as u8
    }
}

// --- the roles as styles, for kit code and apps --------------------------

/// A style in `pick`'s role of this process's theme.
fn role(pick: impl FnOnce(&Theme) -> Swatch) -> Style {
    pick(&current()).style()
}

pub fn text() -> Style {
    role(|t| t.text)
}
pub fn text_secondary() -> Style {
    role(|t| t.text_secondary)
}
pub fn muted() -> Style {
    role(|t| t.muted)
}
pub fn accent() -> Style {
    role(|t| t.accent)
}
pub fn highlight() -> Style {
    role(|t| t.highlight)
}
pub fn active() -> Style {
    role(|t| t.active)
}
pub fn done() -> Style {
    role(|t| t.done)
}
pub fn rule() -> Style {
    role(|t| t.rule)
}
pub fn success() -> Style {
    role(|t| t.success)
}
pub fn warning() -> Style {
    role(|t| t.warning)
}
pub fn danger() -> Style {
    role(|t| t.danger)
}
pub fn info() -> Style {
    role(|t| t.info)
}

/// The shades of the spinner's glow, brightest first, then the muted tone it
/// fades into: towards the foreground of this background (white on dark,
/// black on light).
pub fn glow() -> [Style; 4] {
    let shades: [u8; 3] = match background() {
        Background::Dark => [255, 252, 249],
        Background::Light => [232, 235, 238],
    };
    shades
        .map(|n| Style::new().color256(n))
        .into_iter()
        .chain([muted()])
        .collect::<Vec<_>>()
        .try_into()
        .unwrap_or_else(|_| unreachable!())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::style::Caps;

    /// The theme and background are process-wide; tests that set them hold
    /// this.
    pub(crate) fn lock() -> std::sync::MutexGuard<'static, ()> {
        static L: std::sync::Mutex<()> = std::sync::Mutex::new(());
        L.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn the_kit_theme_on_dark_is_the_old_look() {
        let _g = lock();
        install(Theme::kit());
        set_background(Background::Dark);
        let p = |s: Style| s.paint(Caps::COLOR, "x");
        assert_eq!(p(muted()), "\x1b[38;5;246mx\x1b[0m");
        assert_eq!(p(active()), "\x1b[36mx\x1b[0m");
        assert_eq!(p(done()), "\x1b[32mx\x1b[0m");
        assert_eq!(p(rule()), "\x1b[38;5;8mx\x1b[0m");
        assert_eq!(p(danger()), "\x1b[31mx\x1b[0m");
    }

    #[test]
    fn light_moves_the_greys_only() {
        let _g = lock();
        install(Theme::kit());
        set_background(Background::Light);
        let p = |s: Style| s.paint(Caps::COLOR, "x");
        assert_eq!(p(muted()), "\x1b[38;5;240mx\x1b[0m");
        assert_eq!(p(active()), "\x1b[36mx\x1b[0m");
        assert_eq!(p(text()), "x", "the terminal's own foreground");
        set_background(Background::Dark);
    }

    #[test]
    fn an_installed_theme_wins() {
        let _g = lock();
        let mut t = Theme::kit();
        t.active = Swatch::both(Ink::ansi256(166));
        install(t);
        assert_eq!(active().paint(Caps::COLOR, "x"), "\x1b[38;5;166mx\x1b[0m");
        install(Theme::kit());
    }

    #[test]
    fn rgb_is_24_bit_only_where_the_terminal_takes_it() {
        let _g = lock();
        set_truecolor(true);
        assert_eq!(Ink::Rgb(215, 95, 0).color(), Some(Color::Rgb(215, 95, 0)));
        set_truecolor(false);
        assert_eq!(Ink::Rgb(215, 95, 0).color(), Some(Color::Ansi256(166)));
        assert_eq!(Ink::Rgb(148, 148, 148).color(), Some(Color::Ansi256(246)));
        assert_eq!(Ink::Rgb(0, 0, 0).color(), Some(Color::Ansi256(16)));
    }

    #[test]
    fn backgrounds_by_luminance_and_colorfgbg() {
        assert_eq!(Background::of_rgb(255, 255, 255), Background::Light);
        assert_eq!(Background::of_rgb(0xfa, 0xf6, 0xee), Background::Light);
        assert_eq!(Background::of_rgb(0x1e, 0x1e, 0x2e), Background::Dark);
        assert_eq!(Background::of_rgb(128, 128, 128), Background::Dark);
        assert_eq!(colorfgbg(Some("15;0")), Some(Background::Dark));
        assert_eq!(colorfgbg(Some("0;15")), Some(Background::Light));
        assert_eq!(colorfgbg(Some("0;default;15")), Some(Background::Light));
        assert_eq!(colorfgbg(Some("12;8")), Some(Background::Dark));
        assert_eq!(colorfgbg(None), None);
    }
}
