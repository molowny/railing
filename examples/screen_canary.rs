//! Probe app for `tools/altscreen-canary.sh`: paints a canary string that must
//! never survive in the terminal's buffer once the app has exited.
//!
//! Run with: `cargo run --example screen_canary --features tui -- [inline]`
//!
//! Every row is painted and the first one carries a `TOP <w>x<h>` marker, so a
//! harness can also count how many frames are on screen after a resize. Quits
//! on `q`.

use cli_kit::tui::screen::{self, Flow, ScreenApp, Surface};
use cli_kit::tui::{paint, theme};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// The string the harness greps for. Stands in for a revealed secret.
const CANARY: &str = "CANARY-SECRET-7Q4Z";

struct Canary {
    surface: Surface,
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

fn main() -> std::io::Result<()> {
    let surface = match std::env::args().nth(1).as_deref() {
        Some("inline") => Surface::Inline { height: 24 },
        _ => Surface::AltScreen,
    };
    screen::run(Canary { surface })?;
    Ok(())
}
