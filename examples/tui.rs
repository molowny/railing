//! Demonstrates the `tui` component / flex layout system (requires `tui` feature).
//!
//! Builds a small layout in an off-screen `Buffer`, renders it, and prints the
//! resulting lines so the example runs non-interactively.
//!
//! Run with: `cargo run --example tui --features tui`

use cli_kit::tui::widgets::{footer::FooterProps, prompt, prompt::PromptState, top_bar};
use cli_kit::tui::{Component, FlexRenderable};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

struct TopBar;
impl Component for TopBar {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        top_bar::render(area, buf, "cli-kit demo", "v0.4", None);
    }
    fn desired_height(&self, _: u16) -> u16 {
        1
    }
}

struct Body;
impl Component for Body {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let lines = [
            "Hello from the tui flex layout!",
            "",
            "  - item one",
            "  - item two",
        ];
        for (i, line) in lines.iter().enumerate() {
            if (i as u16) < area.height {
                buf.set_string(
                    area.x,
                    area.y + i as u16,
                    *line,
                    ratatui::style::Style::default(),
                );
            }
        }
    }
    fn desired_height(&self, _: u16) -> u16 {
        0 // flex
    }
}

struct Footer(FooterProps);
impl Component for Footer {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.0.render(area, buf, area.width);
    }
    fn desired_height(&self, _: u16) -> u16 {
        1
    }
}

struct PromptLine(PromptState);
impl Component for PromptLine {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        prompt::render(area, buf, &self.0);
    }
    fn desired_height(&self, _: u16) -> u16 {
        1
    }
}

fn main() {
    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);

    let mut flex = FlexRenderable::new();
    flex.push(0, TopBar);
    flex.push(1, Body);
    flex.push(0, PromptLine(PromptState::Search("hello".into())));
    flex.push(
        0,
        Footer(FooterProps::new(vec![
            ("↑/↓", "move"),
            ("Enter", "ok"),
            ("Esc", "quit"),
        ])),
    );

    flex.render(area, &mut buf);

    // Dump the buffer as plain text.
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        println!("{}", line.trim_end());
    }

    if let Some((cx, cy)) = flex.cursor_pos(area) {
        println!("(cursor would be at col={cx}, row={cy})");
    }
}
