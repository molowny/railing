//! Probe app for `tools/altscreen-canary.sh`: paints a canary string that must
//! never survive in the terminal's buffer once the app has exited.
//!
//! Run with: `cargo run --example screen_canary --features tui -- [inline|bigtext]`
//!
//! Every row is painted and the first one carries a `TOP <w>x<h>` marker, so a
//! harness can also count how many frames are on screen after a resize. Quits
//! on `q`.
//!
//! `bigtext` paints the canary the way sealward's Large Type panel does — block
//! glyphs with the literal characters underneath. That is the largest plaintext
//! footprint anything built on this runner produces, so it is the strongest
//! case for the surface contract. The literal row is what keeps the harness's
//! grep meaningful: glyphs alone contain no greppable text, and a test that
//! passes because there was nothing to find is not a test.

use cli_kit::tui::screen::{self, Flow, ScreenApp, Surface};
use cli_kit::tui::widgets::big_text::{self, Scale};
use cli_kit::tui::{paint, theme};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// The string the harness greps for. Stands in for a revealed secret.
const CANARY: &str = "CANARY-SECRET-7Q4Z";

struct Canary {
    surface: Surface,
    big: bool,
}

impl ScreenApp for Canary {
    type Output = ();

    fn surface(&self, _term_height: u16) -> Surface {
        self.surface
    }

    fn tick_interval(&self) -> Option<std::time::Duration> {
        Some(std::time::Duration::from_millis(100))
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if self.big {
            self.render_big(area, buf);
            return;
        }
        for row in 0..area.height {
            let text = if row == 0 {
                format!("TOP {}x{} {}", area.width, area.height, "=".repeat(200))
            } else {
                // Multi-byte content on every row: truncation must stay on char
                // boundaries at any width (see paint::truncated_str).
                format!(
                    "row{row} {CANARY} \u{2022}\u{2022}\u{2022} Sze\u{15b}\u{107} {}",
                    "-".repeat(200)
                )
            };
            paint::truncated_str(
                buf,
                area.x,
                area.y + row,
                &text,
                area.width,
                theme::normal(),
            );
        }
    }

    fn on_key(&mut self, key: KeyEvent) -> Flow<()> {
        if key.code == KeyCode::Char('q') {
            return Flow::Exit(());
        }
        Flow::Continue
    }
}

impl Canary {
    /// The canary as Large Type paints a secret: block glyphs, with the literal
    /// characters on the row beneath them.
    fn render_big(&self, area: Rect, buf: &mut Buffer) {
        paint::truncated_str(
            buf,
            area.x,
            area.y,
            &format!("TOP {}x{}", area.width, area.height),
            area.width,
            theme::normal(),
        );

        let chars: Vec<(char, ratatui::style::Style)> =
            CANARY.chars().map(|c| (c, theme::normal())).collect();
        let per_row = big_text::fits(area.width, Scale::Half).max(1);
        let row_h = Scale::Half.height() + 2;

        for (i, chunk) in chars.chunks(per_row).enumerate() {
            let top = area.y + 1 + i as u16 * row_h;
            if top + row_h > area.y + area.height {
                break;
            }
            big_text::render_row(buf, area.x, top, chunk, Scale::Half);
            // The literal row: what the harness greps for, and what the real
            // panel paints under each glyph.
            let literal: String = chunk.iter().map(|(c, _)| *c).collect();
            paint::truncated_str(
                buf,
                area.x,
                top + Scale::Half.height(),
                &literal,
                area.width,
                theme::normal(),
            );
        }
    }
}

fn main() -> std::io::Result<()> {
    let arg = std::env::args().nth(1);
    let (surface, big) = match arg.as_deref() {
        Some("inline") => (Surface::Inline { height: 24 }, false),
        Some("bigtext") => (Surface::AltScreen, true),
        _ => (Surface::AltScreen, false),
    };
    screen::run(Canary { surface, big })?;
    Ok(())
}
