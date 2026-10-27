//! Blocks: a drawn piece of the flow that is replaced in place, or erased,
//! by whoever drew it.
//!
//! A [`Block`] is pure: its text (every line ending in a newline, none wider
//! than the terminal, so none wraps), how many lines that is, and where the
//! cursor waits for input, if anywhere. A [`Region`] is what is on a terminal
//! now: [`Region::draw`] replaces the previous block with the next by moving
//! up over it from its end and clearing (`ESC[nF ESC[J`), and
//! [`Region::erase`] takes it away. The caller owns the region, so it decides
//! what stays (a submitted step) and what goes (a prompt on `BYE`).
//!
//! On a terminal that cannot move the cursor ([`Caps::redraw`] off), blocks
//! are appended and never replaced, and erasing does nothing.

use std::io::{self, Write};

use crate::style::{self, Caps};

const CURSOR_SHOW: &str = "\x1b[?25h";

/// Where the cursor waits for input in a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Park {
    /// On the line `up` lines above the block's end, at column `col` (from
    /// 0).
    At { up: usize, col: usize },
    /// Where the text left it: a plain block that ends mid-line, at its
    /// input.
    Here,
}

/// A block of lines, ready to draw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Block {
    pub text: String,
    /// Lines the text ends: the cursor is left at the start of the line below.
    pub lines: usize,
    pub park: Option<Park>,
}

impl Block {
    /// A block from whole lines (each without its newline).
    pub fn of_lines(lines: impl IntoIterator<Item = String>) -> Block {
        let mut text = String::new();
        let mut n = 0;
        for l in lines {
            text.push_str(&l);
            text.push('\n');
            n += 1;
        }
        Block {
            text,
            lines: n,
            park: None,
        }
    }

    /// The same block, the cursor waiting at `park`.
    pub fn parked(mut self, park: Park) -> Block {
        self.park = Some(park);
        self
    }
}

/// Move from the end of a block `lines` tall back to its first line and
/// clear from there down.
pub fn erase(lines: usize) -> String {
    match lines {
        0 => "\r\x1b[J".into(),
        n => format!("\x1b[{n}F\x1b[J"),
    }
}

fn park(up: usize, col: usize) -> String {
    let col = col + 1;
    match up {
        0 => format!("\x1b[{col}G"),
        n => format!("\x1b[{n}A\x1b[{col}G"),
    }
}

fn unpark(up: usize) -> String {
    match up {
        0 => "\r".into(),
        n => format!("\x1b[{n}B\r"),
    }
}

/// What a terminal shows of the caller's blocks: the last one drawn.
///
/// It counts in screen rows, not lines. Lines are drawn cut to the width
/// they were rendered for, so each is one row, until the terminal is
/// narrowed: then a terminal that reflows wraps a line into several, and
/// [`Region::resized`] counts them, so erasing climbs exactly what is there.
/// (A terminal that does not reflow, such as xterm, truncates instead; there
/// a narrowing resize leaves the count too high, and the erase takes rows
/// above the block with it. Every common terminal and tmux reflow.)
#[derive(Debug)]
pub struct Region {
    caps: Caps,
    /// The width of each line of the block on screen.
    widths: Vec<usize>,
    /// The terminal's width since a resize; `None` while every line is one
    /// row.
    cols: Option<usize>,
    /// Where the cursor waits: the line (from the top) and its column.
    parked: Option<Parked>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Parked {
    At { line: usize, col: usize },
    Here,
}

impl Region {
    pub fn new(caps: Caps) -> Region {
        Region {
            caps,
            widths: Vec::new(),
            cols: None,
            parked: None,
        }
    }

    pub fn caps(&self) -> Caps {
        self.caps
    }

    /// Rows on screen now.
    pub fn lines(&self) -> usize {
        self.widths.iter().map(|&w| self.rows(w)).sum()
    }

    fn rows(&self, width: usize) -> usize {
        match self.cols {
            Some(cols) if cols > 0 => width.div_ceil(cols).max(1),
            _ => 1,
        }
    }

    /// The terminal is `cols` wide now: count what is on screen at that
    /// width. Draw the block again (rendered for `cols`) to repaint it.
    pub fn resized(&mut self, cols: usize) {
        self.cols = Some(cols);
    }

    /// From the parked cursor down to the start of the row below the block.
    fn unpark(&mut self) -> String {
        match self.parked.take() {
            Some(Parked::At { line, col }) => {
                let here = self.widths.get(line).copied().unwrap_or(0);
                let within = match self.cols {
                    Some(cols) if cols > 0 => col / cols,
                    _ => 0,
                };
                let below: usize = self
                    .widths
                    .iter()
                    .skip(line + 1)
                    .map(|&w| self.rows(w))
                    .sum();
                unpark(self.rows(here) - within + below)
            }
            Some(Parked::Here) => "\n".into(),
            None => String::new(),
        }
    }

    /// Draw `block` in place of the last one, the cursor where it waits.
    pub fn draw(&mut self, w: &mut impl Write, block: &Block) -> io::Result<()> {
        let mut out = self.unpark();
        if self.caps.redraw {
            out += &erase(self.lines());
        }
        out += &block.text;
        self.widths = block.text.lines().map(style::measure).collect();
        self.widths.truncate(block.lines);
        self.cols = None;
        self.parked = match block.park {
            Some(Park::At { up, col }) if self.caps.redraw => {
                out += &park(up, col);
                out += CURSOR_SHOW;
                Some(Parked::At {
                    line: block.lines - up,
                    col,
                })
            }
            Some(Park::Here) => Some(Parked::Here),
            _ => None,
        };
        w.write_all(out.as_bytes())?;
        w.flush()
    }

    /// The input is over: the cursor goes back to the block's end (a newline
    /// after a plain block's input), the block stays as it is.
    pub fn leave_input(&mut self, w: &mut impl Write) -> io::Result<()> {
        let out = self.unpark();
        if out.is_empty() {
            return Ok(());
        }
        w.write_all(out.as_bytes())?;
        w.flush()
    }

    /// Take the block off the screen, the cursor shown.
    pub fn erase(&mut self, w: &mut impl Write) -> io::Result<()> {
        if !self.caps.redraw {
            return self.leave_input(w);
        }
        let mut out = self.unpark();
        let rows = self.lines();
        if rows > 0 {
            out += &erase(rows);
            out += CURSOR_SHOW;
        }
        self.widths.clear();
        w.write_all(out.as_bytes())?;
        w.flush()
    }

    /// Leave the block on screen for good: the next one is drawn below it.
    pub fn keep(&mut self, w: &mut impl Write) -> io::Result<()> {
        self.leave_input(w)?;
        self.widths.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(region: &mut Region, block: &Block) -> String {
        let mut out = Vec::new();
        region.draw(&mut out, block).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn a_block_replaces_the_last_from_its_end() {
        let mut r = Region::new(Caps::COLOR);
        let a = Block::of_lines(["a".into(), "b".into(), "".into(), "c".into()])
            .parked(Park::At { up: 2, col: 3 });
        assert_eq!(
            drawn(&mut r, &a),
            "\r\x1b[Ja\nb\n\nc\n\x1b[2A\x1b[4G\x1b[?25h"
        );
        let b = Block::of_lines(["d".into(), "e".into()]);
        // Back to the end from the input row, up over four lines, cleared.
        assert_eq!(drawn(&mut r, &b), "\x1b[2B\r\x1b[4F\x1b[Jd\ne\n");
        let mut out = Vec::new();
        r.erase(&mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "\x1b[2F\x1b[J\x1b[?25h");
        assert_eq!(r.lines(), 0);
    }

    #[test]
    fn plain_blocks_are_appended_never_erased() {
        let mut r = Region::new(Caps::PLAIN);
        let a = Block {
            text: "◆  Password\n│  ".into(),
            lines: 1,
            park: Some(Park::Here),
        };
        assert_eq!(drawn(&mut r, &a), "◆  Password\n│  ");
        let mut out = Vec::new();
        r.erase(&mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "\n");
        let b = Block::of_lines(["◇  Verifying".into()]);
        assert_eq!(drawn(&mut r, &b), "◇  Verifying\n");
    }

    #[test]
    fn a_narrowed_terminal_counts_the_rows_its_lines_wrapped_into() {
        let mut r = Region::new(Caps::COLOR);
        let wide = "x".repeat(70);
        let b = Block::of_lines([wide.clone(), wide.clone(), "end".into()])
            .parked(Park::At { up: 2, col: 60 });
        drawn(&mut r, &b);
        r.resized(40);
        // 70 columns at 40: two rows each; the cursor at column 60 is on the
        // second row of its line: two rows down is the row below the block.
        assert_eq!(r.lines(), 5);
        let mut out = Vec::new();
        r.erase(&mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\x1b[2B\r\x1b[5F\x1b[J\x1b[?25h"
        );
    }

    #[test]
    fn a_kept_block_is_not_climbed_over() {
        let mut r = Region::new(Caps::COLOR);
        drawn(&mut r, &Block::of_lines(["kept".into()]));
        r.keep(&mut Vec::new()).unwrap();
        assert_eq!(
            drawn(&mut r, &Block::of_lines(["next".into()])),
            "\r\x1b[Jnext\n"
        );
    }
}
