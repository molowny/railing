//! Demonstrates the chord resolver + which-key popup widget (requires
//! `tui` feature).
//!
//! This example runs non-interactively: it feeds a synthetic key sequence
//! through a [`ChordResolver`] and renders the which-key popup state to
//! `stdout` as plain text.
//!
//! Run with: `cargo run --example chord --features tui`

use cli_kit::tui::chord::{
    ChordOutcome, ChordResolver, ChordScope, ChordSpec, key_char, key_code, key_ctrl,
};
use cli_kit::tui::widgets::which_key;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};

#[derive(Clone, Debug)]
enum Action {
    Quit,
    Down,
    Up,
    CopyUrl,
    CopyPassword,
    CopyUsername,
    CopyTotp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Scope {
    Any,
    Detail,
}

impl ChordScope for Scope {
    fn matches(&self, current: &Self) -> bool {
        matches!(self, Scope::Any) || self == current
    }
}

fn render_popup(width: u16, resolver: &ChordResolver<Action, Scope>, scope: &Scope) -> String {
    let area = Rect::new(0, 0, width, 1);
    let mut buf = Buffer::empty(area);
    let conts = resolver.continuations(scope);
    let conts_borrowed: Vec<(KeyEvent, &str)> = conts.iter().map(|(k, l)| (*k, *l)).collect();
    which_key::render(
        &mut buf,
        0,
        0,
        width,
        None,
        resolver.pending(),
        &conts_borrowed,
    );
    let mut line = String::new();
    for x in 0..width {
        line.push_str(buf.cell(Position::new(x, 0)).unwrap().symbol());
    }
    line.trim_end().to_string()
}

fn main() {
    let mut r = ChordResolver::new(vec![
        ChordSpec::new(vec![key_char('q')], "quit", Action::Quit, Scope::Any),
        ChordSpec::new(vec![key_char('j')], "down", Action::Down, Scope::Any),
        ChordSpec::new(
            vec![key_code(KeyCode::Down)],
            "down",
            Action::Down,
            Scope::Any,
        ),
        ChordSpec::new(vec![key_ctrl('n')], "down", Action::Down, Scope::Any),
        ChordSpec::new(vec![key_char('k')], "up", Action::Up, Scope::Any),
        ChordSpec::new(
            vec![key_char('c'), key_char('u')],
            "url",
            Action::CopyUrl,
            Scope::Detail,
        ),
        ChordSpec::new(
            vec![key_char('c'), key_char('p')],
            "password",
            Action::CopyPassword,
            Scope::Detail,
        ),
        ChordSpec::new(
            vec![key_char('c'), key_char('y')],
            "username",
            Action::CopyUsername,
            Scope::Detail,
        ),
        ChordSpec::new(
            vec![key_char('c'), key_char('t')],
            "totp",
            Action::CopyTotp,
            Scope::Detail,
        ),
    ]);

    let scope = Scope::Detail;
    let script: &[(&str, KeyEvent)] = &[
        ("press 'j'", key_char('j')),
        ("press 'q'", key_char('q')),
        ("press 'c' (start chord)", key_char('c')),
        ("press 'u' (complete cu)", key_char('u')),
        ("press 'c' again", key_char('c')),
        ("press Esc (cancel)", key_code(KeyCode::Esc)),
        ("press 'c'", key_char('c')),
        ("press 't' (complete ct)", key_char('t')),
        ("press 'z' (unbound)", key_char('z')),
    ];

    println!("=== chord engine demo ===\n");
    for (label, key) in script {
        let outcome = r.on_key(*key, &scope);
        println!("{label}");
        println!("  outcome: {outcome:?}");
        if r.is_pending() {
            println!("  popup:   {}", render_popup(60, &r, &scope));
        } else {
            println!("  popup:   (none)");
        }
        if matches!(outcome, ChordOutcome::Resolved(Action::Quit)) {
            println!("  → would quit");
        }
        println!();
    }
}
