//! A terminal screen as reviewable text.
//!
//! Plain: the rows as a user reads them. Styled: the same rows with every run
//! of non-default attributes wrapped as `⟦cyan bold⟧text⟦⟧`. Colours of the
//! first sixteen indices are named, the rest `c245`, true colour `#rrggbb`.
//! A blank cell's foreground and weight are not recorded (nothing shows
//! them); its background, inverse and underline are.

use avt::{Cell, Color, Line, Terminal};

pub fn snapshot(term: &Terminal, styled: bool) -> String {
    let mut lines: Vec<String> = term
        .screen()
        .visible_lines()
        .map(|l| row(l, styled))
        .collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let cursor = term.cursor();
    let mut out = format!(
        "cursor {}:{} {}\n",
        cursor.row + 1,
        cursor.col + 1,
        if cursor.visible { "shown" } else { "hidden" }
    );
    for l in lines {
        out.push_str(&l);
        out.push('\n');
    }
    out
}

fn row(line: &Line, styled: bool) -> String {
    // A wide character's second column carries nothing of its own.
    let cells: Vec<&Cell> = line.cells().iter().filter(|c| c.width() > 0).collect();
    let end = cells
        .iter()
        .rposition(|cell| visible(cell, styled))
        .map_or(0, |i| i + 1);
    let mut out = String::new();
    let mut current = String::new();
    for cell in &cells[..end] {
        if styled {
            let tag = tag(cell);
            if tag != current {
                out.push_str(&format!("⟦{tag}⟧"));
                current = tag;
            }
        }
        out.push(cell.char());
    }
    if !current.is_empty() {
        out.push_str("⟦⟧");
    }
    out
}

fn blank(cell: &Cell) -> bool {
    cell.char().is_whitespace()
}

fn visible(cell: &Cell, styled: bool) -> bool {
    let pen = cell.pen();
    !blank(cell)
        || (styled && (pen.background().is_some() || pen.is_inverse() || pen.is_underline()))
}

fn tag(cell: &Cell) -> String {
    let pen = cell.pen();
    let mut parts = Vec::new();
    if !blank(cell) {
        if let Some(fg) = pen.foreground() {
            parts.push(name(fg));
        }
        for (on, attr) in [
            (pen.is_bold(), "bold"),
            (pen.is_faint(), "dim"),
            (pen.is_italic(), "italic"),
            (pen.is_strikethrough(), "strike"),
            (pen.is_blink(), "blink"),
        ] {
            if on {
                parts.push(attr.into());
            }
        }
    }
    if let Some(bg) = pen.background() {
        parts.push(format!("bg={}", name(bg)));
    }
    if pen.is_underline() {
        parts.push("underline".into());
    }
    if pen.is_inverse() {
        parts.push("inverse".into());
    }
    parts.join(" ")
}

fn name(color: Color) -> String {
    const NAMES: [&str; 8] = [
        "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    ];
    match color {
        Color::Indexed(i @ 0..=7) => NAMES[usize::from(i)].into(),
        Color::Indexed(i @ 8..=15) => format!("bright-{}", NAMES[usize::from(i - 8)]),
        Color::Indexed(i) => format!("c{i}"),
        Color::RGB(c) => format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b),
    }
}
