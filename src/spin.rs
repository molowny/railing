//! The one spinner: a turning star beside what is in progress, and a glow
//! sweeping across its text. [`crate::ui::Spinner`] (behind `Flow::task`)
//! and [`crate::Wait`]'s active step both draw through [`Spin`], so they turn,
//! glow and keep pace alike.
//!
//! ```text
//! ✶  Decrypting 312 items      ← the star turns · ✢ ✳ ✶ ✻ ✽ and back, in cyan;
//!        ^^^                     a bright band walks over the muted text
//! ```

use std::time::Duration;

use console::style;

use crate::ui::DIM_256;

/// How long one frame shows; whoever draws a [`Spin`] redraws at this pace.
pub(crate) const PACE: Duration = Duration::from_millis(100);

/// A ping-pong from a dot to a full star and back.
const FRAMES: [&str; 10] = ["·", "✢", "✳", "✶", "✻", "✽", "✻", "✶", "✳", "✢"];

/// Frames the glow rests past the text's end before it starts again.
const GLOW_REST: usize = 8;

/// One spinner's frame counter: [`Spin::symbol`] and [`Spin::glow`] at the
/// same frame, moved on by [`Spin::advance`] once a [`PACE`].
#[derive(Clone, Debug, Default)]
pub(crate) struct Spin {
    t: usize,
    /// Skip the dot frame, where `·` already means something (a pending
    /// step in a wait).
    no_dot: bool,
}

impl Spin {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The star without its dot frame.
    pub(crate) fn without_dot(mut self) -> Self {
        self.no_dot = true;
        self
    }

    /// The next frame.
    pub(crate) fn advance(&mut self) {
        self.t += 1;
    }

    /// The turning star, in cli-kit's colour for what is in progress: the
    /// cyan of the open `◆`, the active frame and the picker's cursor.
    pub(crate) fn symbol(&self) -> String {
        let frames = if self.no_dot {
            &FRAMES[1..]
        } else {
            &FRAMES[..]
        };
        let symbol = frames[self.t % frames.len()];
        style(symbol).cyan().for_stderr().to_string()
    }

    /// `text` with the glow at this frame: the letters under it brighten
    /// (`255`, fading through `252` and `249` either side) over the muted
    /// tone ([`DIM_256`]). It walks one column a frame, then rests past the
    /// end for [`GLOW_REST`] frames. Only colour changes, never the text, so
    /// the line never moves.
    pub(crate) fn glow(&self, text: &str) -> String {
        let n = text.chars().count();
        let head = (self.t % (n + GLOW_REST)) as isize;
        text.chars()
            .enumerate()
            .map(|(i, c)| {
                let shade = match (i as isize - head).unsigned_abs() {
                    0 => 255,
                    1 => 252,
                    2 => 249,
                    _ => DIM_256,
                };
                style(c).color256(shade).for_stderr().to_string()
            })
            .collect()
    }

    /// `star  text`, glowing, cut to `cols` so it never wraps.
    pub(crate) fn line(&self, text: &str, cols: usize) -> String {
        let text = console::truncate_str(text, cols.saturating_sub(4), "…");
        format!("{}  {}", self.symbol(), self.glow(&text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: usize) -> Spin {
        Spin { t, no_dot: false }
    }

    #[test]
    fn the_glow_brightens_a_moving_band_and_never_changes_the_text() {
        let _g = crate::ui::tests::lock();
        console::set_colors_enabled_stderr(true);
        let msg = "Loading items";
        for t in 0..40 {
            assert_eq!(
                console::strip_ansi_codes(&at(t).glow(msg)),
                msg,
                "frame {t}"
            );
        }
        // Frame 3: the brightest letter is the fourth.
        let lit = at(3).glow(msg);
        let cells: Vec<&str> = lit.split("\x1b[0m").filter(|c| !c.is_empty()).collect();
        assert!(cells[3].contains("38;5;255"), "{cells:?}");
        assert!(
            cells[2].contains("38;5;252") && cells[4].contains("38;5;252"),
            "{cells:?}"
        );
        assert!(cells[0].contains(&format!("38;5;{DIM_256}")), "{cells:?}");
        // Past the end the glow rests: every letter is the muted tone.
        let rest = at(msg.len() + 4).glow(msg);
        assert!(
            !rest.contains("38;5;255") && !rest.contains("38;5;249"),
            "{rest:?}"
        );
        console::set_colors_enabled_stderr(false);
    }

    #[test]
    fn the_star_turns_and_the_line_never_wraps() {
        let _g = crate::ui::tests::lock();
        console::set_colors_enabled_stderr(false);
        assert_eq!(at(0).line("Fetching", 80), "·  Fetching");
        assert_eq!(at(5).line("Fetching", 80), "✽  Fetching");
        assert_eq!(at(10).line("Fetching", 80), "·  Fetching");
        let long = at(1).line(&"x".repeat(200), 40);
        assert!(console::measure_text_width(&long) <= 40, "{long}");
    }

    #[test]
    fn without_the_dot_it_never_shows_one() {
        let _g = crate::ui::tests::lock();
        console::set_colors_enabled_stderr(false);
        let mut s = Spin::new().without_dot();
        for _ in 0..30 {
            assert_ne!(s.symbol(), "·");
            s.advance();
        }
    }
}
