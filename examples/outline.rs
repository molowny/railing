//! Demonstrates the fold-aware outline widget (requires `tui` feature).
//!
//! This example runs non-interactively: it builds a small tree, renders it to
//! an off-screen [`Buffer`] at a few fold states, and prints each frame to
//! `stdout` as plain text.
//!
//! The shape to notice is that the consumer owns the tree and the keymap; the
//! widget owns fold state, flattening, and scroll. Emitting children only when
//! [`OutlineBuilder::branch`] returns `true` means a collapsed subtree is never
//! walked at all.
//!
//! Run with: `cargo run --example outline --features tui`

use cli_kit::tui::paint;
use cli_kit::tui::widgets::outline::{
    Outline, OutlineBuilder, OutlineState, fold_right, paint_gutter,
};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;

/// What each row shows. In a real app this would carry whatever the model
/// needs — a value, a dirty marker, a badge.
struct Row {
    label: &'static str,
    value: &'static str,
}

/// The tree, walked fresh every frame. Nothing here stores fold state.
fn build(state: &OutlineState<&'static str>) -> Outline<&'static str, Row> {
    let mut b = OutlineBuilder::new(state);

    b.leaf(
        0,
        "title",
        Row {
            label: "title",
            value: "GitHub",
        },
    );

    if b.branch(
        0,
        "creds",
        true,
        Row {
            label: "Credentials",
            value: "",
        },
    ) {
        b.leaf(
            1,
            "user",
            Row {
                label: "username",
                value: "alice",
            },
        );

        // Default-collapsed: at rest this row reads `password  ••••••••`,
        // exactly like a flat list, and opens into structure on demand.
        if b.branch(
            1,
            "pw",
            false,
            Row {
                label: "password",
                value: "••••••••",
            },
        ) {
            b.leaf(
                2,
                "pw.value",
                Row {
                    label: "value",
                    value: "••••••••",
                },
            );
            b.leaf(
                2,
                "pw.role",
                Row {
                    label: "role",
                    value: "Password",
                },
            );
            if b.branch(
                2,
                "pw.attrs",
                false,
                Row {
                    label: "attrs",
                    value: "",
                },
            ) {
                b.leaf(
                    3,
                    "pw.required",
                    Row {
                        label: "required",
                        value: "no",
                    },
                );
                b.leaf(
                    3,
                    "pw.revealable",
                    Row {
                        label: "revealable",
                        value: "yes",
                    },
                );
            }
        }
    }

    b.leaf(
        0,
        "notes",
        Row {
            label: "notes",
            value: "(empty)",
        },
    );
    b.finish()
}

fn render(
    outline: &Outline<&'static str, Row>,
    state: &OutlineState<&'static str>,
    h: u16,
) -> Vec<String> {
    let area = Rect::new(0, 0, 44, h);
    let mut buf = Buffer::empty(area);

    for (i, y) in state.viewport(outline, h as usize) {
        if y < 0 {
            continue;
        }
        let row = outline.row(i).unwrap();
        let y = y as u16;
        let selected = i == state.cursor();
        let style = if selected {
            Style::default().add_modifier(ratatui::style::Modifier::REVERSED)
        } else {
            Style::default()
        };

        // Values share one absolute column regardless of depth, so a collapsed
        // branch (`password  ••••••••`) reads exactly like a flat list row.
        const VALUE_COL: u16 = 22;

        let x = paint_gutter(&mut buf, 0, y, area.width, row.depth, row.fold, style);
        paint::truncated_str(
            &mut buf,
            x,
            y,
            row.payload.label,
            VALUE_COL.saturating_sub(x).saturating_sub(1),
            style,
        );
        paint::truncated_str(
            &mut buf,
            VALUE_COL,
            y,
            row.payload.value,
            area.width.saturating_sub(VALUE_COL),
            style,
        );
    }

    (0..h)
        .map(|y| {
            (0..area.width)
                .map(|x| {
                    buf.cell(Position::new(x, y))
                        .unwrap()
                        .symbol()
                        .chars()
                        .next()
                        .unwrap_or(' ')
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn frame(title: &str, outline: &Outline<&'static str, Row>, state: &OutlineState<&'static str>) {
    println!("--- {title} ---");
    for line in render(outline, state, 10) {
        println!("|{line}");
    }
    println!();
}

fn main() {
    let mut state: OutlineState<&'static str> = OutlineState::new();

    let outline = build(&state);
    frame("at rest (password collapsed)", &outline, &state);

    // Put the cursor on `password` and open it, the way `l` would.
    state.set_cursor(outline.index_of(&"pw").unwrap());
    let outline = build(&state);
    println!(
        "fold_right on `password` -> {:?}\n",
        fold_right(&outline, &mut state)
    );

    let outline = build(&state);
    frame("password expanded", &outline, &state);

    // Reach `attrs` and open that too — the nested level a flat list can't show.
    state.set_cursor(outline.index_of(&"pw.attrs").unwrap());
    let outline = build(&state);
    let _ = fold_right(&outline, &mut state);
    let outline = build(&state);
    frame("attrs expanded", &outline, &state);

    state.expand_all();
    let outline = build(&state);
    frame("expand_all", &outline, &state);

    state.collapse_all();
    let outline = build(&state);
    frame("collapse_all", &outline, &state);
}
