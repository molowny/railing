//! A step of the flow, as a [`Block`]: what every prompt draws, open,
//! answered, failed or given up, whether a [`crate::flow::Flow`] is open in
//! this process or another process opened it (askpass continues the client's
//! `┌ sealward unlock`).
//!
//! ```text
//! ◆  Master password                      ← the label (Active: cyan ◆)
//! │  Unlock your sealward vault           ← desc, dim
//! │  █                                    ← body: the input rows; the cursor parks here
//! └  hidden as you type · enter to submit ← the foot
//! ```
//!
//! The foot follows the state: [`Foot`] while active, the error beside `└`
//! (`▲`, yellow), `└  Operation cancelled.` (`■`, red), and a bare `│` once
//! submitted (`◇`, green, the bars grey), so the next step follows it.
//!
//! Every line is cut to the terminal's width, never wrapped: a wrapped line
//! would throw off the count a [`crate::render::Region`] erases by.

use crate::glyph::{self, State};
use crate::render::{Block, Park};
use crate::style::{self, Caps, Style};

/// Columns the gutter takes (`│  `).
pub const GUTTER: usize = 3;

/// Where the step is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status<'a> {
    Active,
    /// Active again after a wrong answer, the reason beside `└`.
    Error(&'a str),
    Submit,
    Cancel,
}

impl Status<'_> {
    fn glyph(self) -> State {
        match self {
            Status::Active => State::Active,
            Status::Error(_) => State::Error,
            Status::Submit => State::Submit,
            Status::Cancel => State::Cancel,
        }
    }
}

/// What an open step says beside its `└`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Foot<'a> {
    None,
    /// In the step's colour, like clack's (`[Esc](Preview)`).
    Message(&'a str),
    /// Dim, for keys and how the input behaves (`hidden as you type · …`).
    Hint(&'a str),
}

/// The rows under the label and description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// Nothing: a submitted secret, which shows no value at all.
    None,
    /// Styled rows, each drawn after the gutter; the cursor, if the step
    /// takes input, at `(row, column)` within them.
    Rows {
        rows: Vec<String>,
        cursor: Option<(usize, usize)>,
    },
}

impl Body {
    /// One empty input row with the cursor on it: a secret being typed,
    /// echoed nowhere.
    pub fn silent_input() -> Body {
        Body::Rows {
            rows: vec![String::new()],
            cursor: Some((0, 0)),
        }
    }
}

/// One step to draw.
#[derive(Clone, Debug)]
pub struct Step<'a> {
    pub status: Status<'a>,
    /// The question. A second line and more go on gutter rows.
    pub label: &'a str,
    /// Dim lines under the label (blank lines dropped).
    pub desc: &'a str,
    pub body: Body,
    pub foot: Foot<'a>,
}

/// `text` cut to fit `cols` beside the gutter, so no line wraps.
pub fn fit(text: &str, cols: usize) -> String {
    style::truncate(text, cols.saturating_sub(GUTTER + 1).max(1), "…").into_owned()
}

/// The step as a block for a terminal `cols` wide with `caps`.
///
/// Without redraw, an open step is drawn the plain way instead: appended,
/// never replaced, the error (if any) on a line of its own above, and the
/// cursor left after `│  ` for the input.
pub fn render(caps: Caps, step: &Step, cols: usize) -> Block {
    if !caps.redraw && matches!(step.status, Status::Active | Status::Error(_)) {
        return plain(step);
    }
    let state = step.status.glyph();
    let bar = glyph::bar(caps, state);
    let mut lines = Vec::new();
    for (i, l) in step.label.lines().enumerate() {
        let mark = if i == 0 {
            glyph::step(caps, state)
        } else {
            bar.clone()
        };
        lines.push(format!("{mark}  {}", fit(l, cols)));
    }
    if step.label.is_empty() {
        lines.push(format!("{}  ", glyph::step(caps, state)));
    }
    for l in step.desc.lines().filter(|l| !l.trim().is_empty()) {
        lines.push(format!("{bar}  {}", glyph::dim(caps, &fit(l, cols))));
    }
    let mut park = None;
    if let Body::Rows { rows, cursor } = &step.body {
        let first = lines.len();
        for r in rows {
            lines.push(format!("{bar}  {}", fit(r, cols)));
        }
        // Only an open step waits for input.
        let open = matches!(step.status, Status::Active | Status::Error(_));
        if let (true, Some((row, col))) = (open, cursor) {
            park = Some((first + row, GUTTER + col));
        }
    }
    let corner = |text: &str| glyph::tinted(caps, state, &format!("└  {text}"));
    lines.push(match (step.status, step.foot) {
        (Status::Active, Foot::None) => corner(""),
        (Status::Active, Foot::Message(m)) => corner(&fit(m, cols)),
        (Status::Active, Foot::Hint(h)) => {
            format!(
                "{}  {}",
                glyph::corner(caps, state),
                glyph::dim(caps, &fit(h, cols))
            )
        }
        (Status::Error(e), _) => corner(&fit(e, cols)),
        (Status::Cancel, _) => corner("Operation cancelled."),
        (Status::Submit, _) => bar,
    });
    let total = lines.len();
    let block = Block::of_lines(lines);
    match park {
        Some((line, col)) => block.parked(Park::At {
            up: total - line,
            col,
        }),
        None => block,
    }
}

fn plain(step: &Step) -> Block {
    let caps = Caps::PLAIN;
    let mut text = String::new();
    if let Status::Error(e) = step.status {
        text += &format!("{}  {e}\n", glyph::step(caps, State::Error));
    }
    text += &format!("{}  {}\n", glyph::step(caps, State::Active), step.label);
    text += &format!("{}  ", glyph::bar(caps, State::Active));
    let lines = text.matches('\n').count();
    Block {
        text,
        lines,
        park: Some(Park::Here),
    }
}

/// A dim value, as a submitted step shows its answer.
pub fn answer(caps: Caps, text: &str) -> String {
    Style::new().dim().paint(caps, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unlock(status: Status) -> Step {
        Step {
            status,
            label: "Master password",
            desc: "Unlock your sealward vault",
            body: Body::silent_input(),
            foot: Foot::Hint("hidden as you type · enter to submit · esc to cancel"),
        }
    }

    fn plain_lines(b: &Block) -> Vec<String> {
        b.text
            .lines()
            .map(|l| style::strip(l).into_owned())
            .collect()
    }

    #[test]
    fn an_open_step_waits_on_its_input_row() {
        let b = render(Caps::COLOR, &unlock(Status::Active), 80);
        assert_eq!(
            plain_lines(&b),
            [
                "◆  Master password",
                "│  Unlock your sealward vault",
                "│  ",
                "└  hidden as you type · enter to submit · esc to cancel",
            ]
        );
        assert_eq!(b.lines, 4);
        assert_eq!(b.park, Some(Park::At { up: 2, col: 3 }));
    }

    #[test]
    fn an_error_is_the_same_height_with_the_reason_beside_the_corner() {
        let ok = render(Caps::COLOR, &unlock(Status::Active), 80);
        let err = render(
            Caps::COLOR,
            &unlock(Status::Error("Wrong password (4 attempts remaining)")),
            80,
        );
        assert_eq!(ok.lines, err.lines);
        let l = plain_lines(&err);
        assert_eq!(l[0], "▲  Master password");
        assert_eq!(l[3], "└  Wrong password (4 attempts remaining)");
    }

    #[test]
    fn a_submitted_secret_shows_no_value() {
        let s = Step {
            status: Status::Submit,
            body: Body::None,
            ..unlock(Status::Submit)
        };
        let s = Step { desc: "", ..s };
        let b = render(Caps::COLOR, &s, 80);
        assert_eq!(plain_lines(&b), ["◇  Master password", "│"]);
        assert_eq!(b.park, None);
    }

    #[test]
    fn the_clack_states_read_as_clack_draws_them() {
        let input = |status| Step {
            status,
            label: "Project name",
            desc: "",
            body: Body::Rows {
                rows: vec!["alice".into()],
                cursor: None,
            },
            foot: Foot::None,
        };
        let c = Caps::COLOR;
        assert_eq!(
            plain_lines(&render(c, &input(Status::Submit), 80)),
            ["◇  Project name", "│  alice", "│"]
        );
        assert_eq!(
            plain_lines(&render(c, &input(Status::Cancel), 80)),
            ["■  Project name", "│  alice", "└  Operation cancelled."]
        );
        let active = render(c, &input(Status::Active), 80);
        assert_eq!(active.text.lines().last().unwrap(), "\x1b[36m└  \x1b[0m");
    }

    #[test]
    fn nothing_is_wider_than_the_terminal() {
        let long = "x".repeat(200);
        let s = Step {
            status: Status::Error(&long),
            label: &long,
            desc: &long,
            body: Body::Rows {
                rows: vec![long.clone()],
                cursor: None,
            },
            foot: Foot::None,
        };
        for l in render(Caps::COLOR, &s, 40).text.lines() {
            assert!(style::measure(l) < 40, "{l}");
        }
    }

    #[test]
    fn a_plain_terminal_gets_the_step_appended() {
        let b = render(Caps::PLAIN, &unlock(Status::Error("Wrong password")), 80);
        assert_eq!(b.text, "▲  Wrong password\n◆  Master password\n│  ");
        assert_eq!(b.park, Some(Park::Here));
    }
}
