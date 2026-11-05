//! The one spinner: a pulsing shape beside what is in progress, and a glow
//! sweeping across its text. [`crate::ui::Spinner`] (behind `Flow::task`)
//! and [`crate::Wait`]'s active step both draw through [`Spin`], so they
//! pulse, glow and keep pace alike.
//!
//! ```text
//! ◈  Decrypting 312 items      ← the shape fills · ◦ ◇ ◈ ◆ and back, in cyan;
//!        ^^^                     a bright band walks over the muted text
//! ```
//!
//! The shape is a [`SpinnerStyle`], one for the whole program
//! ([`set_spinner_style`]); each spinner takes it when it starts.
//!
//! [`Spin`] only makes strings, coloured for the [`Caps`] it is given: an
//! app that draws on a terminal of its own (not stderr) animates one itself,
//! redrawing [`Spin::line`] every [`PACE`]. It is in the slim core, so that
//! needs no `full` feature.

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use crate::glyph::DIM_256;
use crate::style::{self, Caps, Style};

/// What the spinner pulses through. Every style is a ping-pong from a dot to
/// a solid shape and back, one column wide.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpinnerStyle {
    /// `· ✢ ✳ ✶ ✻ ✽` — a star opening.
    Star,
    /// `· ◦ ◇ ◈ ◆` — the flow's own `◇` (a step) filling into `◆` (the open
    /// one), so a running step reads as the step coming alive.
    #[default]
    Diamond,
    /// `· ▫ ◻ ▣ ■` — a square filling.
    Square,
    /// `· ∘ ○ ◎ ●` — a ring breathing into a disc.
    Circle,
}

impl SpinnerStyle {
    pub const ALL: [SpinnerStyle; 4] = [Self::Star, Self::Diamond, Self::Square, Self::Circle];

    /// The frames, the dot first.
    pub(crate) fn frames(self) -> &'static [&'static str] {
        match self {
            Self::Star => &["·", "✢", "✳", "✶", "✻", "✽", "✻", "✶", "✳", "✢"],
            Self::Diamond => &["·", "◦", "◇", "◈", "◆", "◈", "◇", "◦"],
            Self::Square => &["·", "▫", "◻", "▣", "■", "▣", "◻", "▫"],
            Self::Circle => &["·", "∘", "○", "◎", "●", "◎", "○", "∘"],
        }
    }

    /// Its name, as [`str::parse`] takes it: `star`, `diamond`, `square`,
    /// `circle`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Star => "star",
            Self::Diamond => "diamond",
            Self::Square => "square",
            Self::Circle => "circle",
        }
    }
}

impl std::str::FromStr for SpinnerStyle {
    type Err = String;

    /// `star`, `diamond`, `square` or `circle`, any case — for an app that
    /// lets its users choose in a config file or a flag.
    fn from_str(s: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|st| st.name().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| format!("unknown spinner style '{s}' (star, diamond, square, circle)"))
    }
}

/// The style spinners start with; [`SpinnerStyle::Diamond`] until set.
static STYLE: AtomicU8 = AtomicU8::new(1);

/// Use `style` for every spinner started from now on (`Flow::task`,
/// `ui::Spinner`, a `Wait`'s active step). The app's choice — cli-kit only
/// offers the styles.
pub fn set_spinner_style(style: SpinnerStyle) {
    let i = SpinnerStyle::ALL
        .iter()
        .position(|s| *s == style)
        .unwrap_or(1);
    STYLE.store(i as u8, Ordering::Relaxed);
}

/// The style set with [`set_spinner_style`].
pub fn spinner_style() -> SpinnerStyle {
    SpinnerStyle::ALL
        .get(STYLE.load(Ordering::Relaxed) as usize)
        .copied()
        .unwrap_or_default()
}

/// How long one frame shows; whoever draws a [`Spin`] redraws at this pace.
/// An app animating a [`Spin`] on a terminal of its own sleeps this long
/// between draws.
pub const PACE: Duration = Duration::from_millis(100);

/// Frames the glow rests past the text's end before it starts again.
const GLOW_REST: usize = 8;

/// One spinner: [`Spin::symbol`] and [`Spin::glow`] at the same frame. The
/// frame is the time since it started in [`PACE`]s, not a count of draws, so
/// however often a caller redraws — a `Wait` ticking every 120 ms, the
/// spinner thread every 100 — every spinner moves at the same speed.
#[derive(Clone, Debug)]
pub struct Spin {
    started: Instant,
    /// Tests pin the frame instead of reading the clock.
    #[cfg(test)]
    fixed: Option<usize>,
    style: SpinnerStyle,
    /// Skip the dot frame, where `·` already means something (a pending
    /// step in a wait).
    no_dot: bool,
}

impl Spin {
    /// A spinner in the program's style ([`set_spinner_style`]).
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            #[cfg(test)]
            fixed: None,
            style: spinner_style(),
            no_dot: false,
        }
    }

    /// The frame shown now: whole [`PACE`]s since it started.
    fn frame(&self) -> usize {
        #[cfg(test)]
        if let Some(t) = self.fixed {
            return t;
        }
        (self.started.elapsed().as_millis() / PACE.as_millis()) as usize
    }

    /// Pin the frame (tests elsewhere in the crate).
    #[cfg(test)]
    pub(crate) fn fix(&mut self, frame: usize) {
        self.fixed = Some(frame);
    }

    /// The shape without its dot frame.
    pub fn without_dot(mut self) -> Self {
        self.no_dot = true;
        self
    }

    /// The pulsing shape, in cli-kit's colour for what is in progress: the
    /// cyan of the open `◆`, the active frame and the picker's cursor.
    pub fn symbol(&self, caps: Caps) -> String {
        let all = self.style.frames();
        let frames = if self.no_dot { &all[1..] } else { all };
        let symbol = frames[self.frame() % frames.len()];
        Style::new().cyan().paint(caps, symbol)
    }

    /// `text` with the glow at this frame: the letters under it brighten
    /// (`255`, fading through `252` and `249` either side) over the muted
    /// tone ([`DIM_256`]). It walks one column a frame, then rests past the
    /// end for [`GLOW_REST`] frames. Only colour changes, never the text, so
    /// the line never moves.
    pub fn glow(&self, caps: Caps, text: &str) -> String {
        let n = text.chars().count();
        let head = (self.frame() % (n + GLOW_REST)) as isize;
        text.chars()
            .enumerate()
            .map(|(i, c)| {
                let shade = match (i as isize - head).unsigned_abs() {
                    0 => 255,
                    1 => 252,
                    2 => 249,
                    _ => DIM_256,
                };
                Style::new().color256(shade).paint(caps, c)
            })
            .collect()
    }

    /// `◆  text`, glowing, cut to `cols` so it never wraps.
    pub fn line(&self, caps: Caps, text: &str, cols: usize) -> String {
        let text = style::truncate(text, cols.saturating_sub(4), "…");
        format!("{}  {}", self.symbol(caps), self.glow(caps, &text))
    }
}

impl Default for Spin {
    fn default() -> Self {
        Self::new()
    }
}

/// A [`Spin`] animated on a thread of its own, drawing through `out` every
/// [`PACE`] until [`Animation::stop`] (or drop), which joins it, so nothing
/// it draws can land after whatever the caller writes next.
///
/// `out` is the thread's own handle (`term::Tty::writer`): while it runs it
/// is the terminal's only writer, which is the caller's to keep.
pub struct Animation {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Animation {
    /// Run `paint` every frame, then `end` once, on a thread.
    pub fn start<W, P, E>(mut out: W, mut paint: P, end: E) -> Animation
    where
        W: std::io::Write + Send + 'static,
        P: FnMut(&mut W, &Spin) + Send + 'static,
        E: FnOnce(&mut W) + Send + 'static,
    {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopping = std::sync::Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            let spin = Spin::new();
            while !stopping.load(Ordering::Relaxed) {
                paint(&mut out, &spin);
                std::thread::sleep(PACE);
            }
            end(&mut out);
        });
        Animation {
            stop,
            thread: Some(thread),
        }
    }

    /// `◆  msg`, glowing, on the line the cursor is on (the one under a
    /// step), cut to `cols`; the cursor hidden meanwhile. Stopped, the line
    /// is cleared and the cursor shown.
    pub fn line<W>(out: W, caps: Caps, msg: &str, cols: usize) -> Animation
    where
        W: std::io::Write + Send + 'static,
    {
        let msg = msg.to_string();
        let mut first = true;
        Animation::start(
            out,
            move |w, spin| {
                let hide = if std::mem::take(&mut first) {
                    "\x1b[?25l"
                } else {
                    ""
                };
                let _ = write!(w, "{hide}\r\x1b[2K{}", spin.line(caps, &msg, cols));
                let _ = w.flush();
            },
            |w| {
                let _ = write!(w, "\r\x1b[2K\x1b[?25h");
                let _ = w.flush();
            },
        )
    }

    /// Stop it and wait for its last draw.
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Animation {
    fn drop(&mut self) {
        self.halt();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: usize) -> Spin {
        Spin {
            fixed: Some(t),
            style: SpinnerStyle::Diamond,
            ..Spin::new()
        }
    }

    #[test]
    fn the_glow_brightens_a_moving_band_and_never_changes_the_text() {
        let c = Caps::COLOR;
        let msg = "Loading items";
        for t in 0..40 {
            assert_eq!(style::strip(&at(t).glow(c, msg)), msg, "frame {t}");
        }
        // Frame 3: the brightest letter is the fourth.
        let lit = at(3).glow(c, msg);
        let cells: Vec<&str> = lit.split("\x1b[0m").filter(|c| !c.is_empty()).collect();
        assert!(cells[3].contains("38;5;255"), "{cells:?}");
        assert!(
            cells[2].contains("38;5;252") && cells[4].contains("38;5;252"),
            "{cells:?}"
        );
        assert!(cells[0].contains(&format!("38;5;{DIM_256}")), "{cells:?}");
        // Past the end the glow rests: every letter is the muted tone.
        let rest = at(msg.len() + 4).glow(c, msg);
        assert!(
            !rest.contains("38;5;255") && !rest.contains("38;5;249"),
            "{rest:?}"
        );
    }

    #[test]
    fn the_diamond_pulses_and_the_line_never_wraps() {
        let p = Caps::PLAIN;
        assert_eq!(at(0).line(p, "Fetching", 80), "·  Fetching");
        assert_eq!(at(2).line(p, "Fetching", 80), "◇  Fetching");
        assert_eq!(at(4).line(p, "Fetching", 80), "◆  Fetching");
        assert_eq!(at(5).line(p, "Fetching", 80), "◈  Fetching");
        assert_eq!(at(8).line(p, "Fetching", 80), "·  Fetching");
        let long = at(1).line(p, &"x".repeat(200), 40);
        assert!(style::measure(&long) <= 40, "{long}");
    }

    #[test]
    fn every_style_pulses_from_the_dot_and_parses_by_name() {
        for style in SpinnerStyle::ALL {
            let frames = style.frames();
            assert_eq!(frames[0], "·", "{style:?}");
            // A ping-pong: the second half mirrors the first.
            let n = frames.len();
            assert!(
                n % 2 == 0 && (1..n / 2).all(|i| frames[i] == frames[n - i]),
                "{style:?}"
            );
            assert!(frames.iter().all(|f| style::measure(f) == 1), "{style:?}");
            assert_eq!(style.name().parse::<SpinnerStyle>(), Ok(style));
            let peak = Spin {
                fixed: Some(n / 2),
                style,
                ..Spin::new()
            };
            assert_eq!(peak.symbol(Caps::PLAIN), frames[n / 2]);
        }
        assert_eq!(" Square ".parse(), Ok(SpinnerStyle::Square));
        assert!("swirl".parse::<SpinnerStyle>().is_err());
    }

    #[test]
    fn the_program_style_is_what_new_spinners_take() {
        let before = spinner_style();
        assert_eq!(before, SpinnerStyle::Diamond, "the default");
        set_spinner_style(SpinnerStyle::Circle);
        assert_eq!(Spin::new().style, SpinnerStyle::Circle);
        set_spinner_style(before);
        assert_eq!(Spin::new().style, before);
    }

    #[test]
    fn without_the_dot_it_never_shows_one() {
        for t in 0..30 {
            let s = Spin {
                fixed: Some(t),
                ..Spin::new().without_dot()
            };
            assert_ne!(s.symbol(Caps::PLAIN), "·");
        }
    }

    #[test]
    fn an_animated_line_hides_the_cursor_and_clears_itself_when_stopped() {
        use std::sync::{Arc, Mutex};
        #[derive(Clone)]
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Sink {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let sink = Sink(Arc::default());
        let a = Animation::line(sink.clone(), Caps::PLAIN, "Working", 40);
        std::thread::sleep(PACE * 3);
        a.stop();
        let out = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        assert!(out.starts_with("\x1b[?25l\r\x1b[2K"), "{out:?}");
        assert_eq!(out.matches("\x1b[?25l").count(), 1, "{out:?}");
        assert!(out.contains("Working"), "{out:?}");
        assert!(out.ends_with("\r\x1b[2K\x1b[?25h"), "{out:?}");
        // Joined: nothing is drawn after stop returns.
        let len = sink.0.lock().unwrap().len();
        std::thread::sleep(PACE * 2);
        assert_eq!(sink.0.lock().unwrap().len(), len);
    }

    #[test]
    fn the_frame_follows_the_clock_not_the_draws() {
        let s = Spin::new();
        // However often it is drawn, the frame holds until a pace has passed.
        let first: Vec<usize> = (0..50).map(|_| s.frame()).collect();
        assert!(first.iter().all(|&f| f == first[0]), "{first:?}");
        std::thread::sleep(PACE * 2);
        assert!(s.frame() >= first[0] + 2);
    }
}
