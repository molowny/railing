//! Demonstrates the multi-line text input (requires `tui` feature).
//!
//! This example runs non-interactively: it feeds a synthetic key sequence into
//! a [`TextArea`] and renders each state to an off-screen [`Buffer`], printed
//! to `stdout` as plain text.
//!
//! The contrast worth seeing is against [`TextInput`]: `Enter` inserts a
//! newline instead of submitting, `Ctrl-d` submits, and `Ctrl-a`/`Ctrl-e` act
//! on the current line rather than the whole buffer.
//!
//! Run with: `cargo run --example text_area --features tui`
//!
//! [`TextInput`]: cli_kit::tui::widgets::text_input::TextInput

use cli_kit::tui::chord::{key_char, key_code, key_ctrl};
use cli_kit::tui::widgets::text_area::{InputEvent, TextArea, render};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;

const WIDTH: u16 = 24;

fn show(label: &str, area_h: u16, state: &TextArea) {
    let area = Rect::new(0, 0, WIDTH, area_h);
    let mut buf = Buffer::empty(area);
    render(area, &mut buf, state, Style::default());

    println!("--- {label} ---");
    println!("+{}+", "-".repeat(WIDTH as usize));
    for y in 0..area_h {
        let line: String = (0..WIDTH)
            .map(|x| {
                buf.cell(Position::new(x, y))
                    .unwrap()
                    .symbol()
                    .chars()
                    .next()
                    .unwrap_or(' ')
            })
            .collect();
        println!("|{line}|");
    }
    println!("+{}+", "-".repeat(WIDTH as usize));
    println!("height at width {WIDTH}: {}\n", state.desired_height(WIDTH));
}

fn feed(state: &mut TextArea, keys: &[(&str, KeyEvent)]) {
    for (label, key) in keys {
        match state.on_key(*key) {
            InputEvent::Submit => println!("  {label:<12} -> Submit"),
            InputEvent::Cancel => println!("  {label:<12} -> Cancel"),
            InputEvent::Edited => {}
            InputEvent::Ignored => println!("  {label:<12} -> Ignored"),
        }
    }
}

fn main() {
    let mut area = TextArea::new("");

    // Enter builds up lines instead of ending the edit.
    let mut keys: Vec<(&str, KeyEvent)> = Vec::new();
    for c in "first line".chars() {
        keys.push(("char", key_char(c)));
    }
    keys.push(("Enter", key_code(KeyCode::Enter)));
    for c in "second".chars() {
        keys.push(("char", key_char(c)));
    }
    feed(&mut area, &keys);
    show("two lines, Enter inserted a newline", 4, &area);

    // Long input wraps; the widget reports the height it needs.
    let wrapped = TextArea::new("a value long enough to wrap across several display rows");
    show("wrapped, cursor at the end", 4, &wrapped);

    // Ctrl-u is line-scoped: it clears the line the cursor is on and leaves
    // the rest of the buffer alone. (Cursor motion itself is reverse-video, so
    // it cannot be shown in a plain-text dump — this is the visible proof.)
    let mut scoped = TextArea::new("keep this line\nand drop this one");
    feed(&mut scoped, &[("Ctrl-u", key_ctrl('u'))]);
    show("after Ctrl-u on the second line", 3, &scoped);

    // A window shorter than the content scrolls to follow the cursor.
    let tall = TextArea::new("l1\nl2\nl3\nl4\nl5");
    show("5 lines in a 2-row window, cursor at end", 2, &tall);

    // Submit and cancel.
    let mut done = TextArea::new("payload");
    feed(
        &mut done,
        &[("Ctrl-d", key_ctrl('d')), ("Esc", key_code(KeyCode::Esc))],
    );
    println!("value after Submit/Cancel is untouched: {:?}", done.value());
}
