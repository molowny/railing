//! Inline detail view using ratatui `Viewport::Inline`.
//!
//! Displays item fields in navigable sections with reveal/copy support.
//! Same rendering approach as the picker — `/dev/tty` output, inline viewport.

use std::collections::HashSet;
use std::io::{self, Write};
use std::time::Instant;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::Terminal;

use ratatui::style::Color;

use crate::picker::Badge;
use crate::tui::{paint, theme};

/// Chrome rows: header(2) + hr + feedback + hr + hints.
const CHROME_ROWS: usize = 6;

pub struct DetailItem {
    pub title: String,
    pub badge: Badge,
    pub tags: Vec<String>,
    pub sections: Vec<Section>,
    pub meta: Vec<(String, String)>,
}

pub struct Section {
    pub title: &'static str,
    pub fields: Vec<FieldEntry>,
}

pub struct FieldEntry {
    pub label: String,
    pub value: FieldKind,
}

pub enum FieldKind {
    Plain(String),
    Secret(String),
    Url(String),
    File { name: String, size: u64 },
    Totp,
    Note(String),
}

/// Action emitted to the caller's callback.
pub enum DetailAction {
    /// Copy field at the given navigable field index.
    Copy(usize),
}

/// Show an inline detail view. Returns when the user presses Esc/q.
///
/// `on_action` is called for copy requests. Return `Ok(msg)` for success
/// feedback or `Err(msg)` for error feedback displayed temporarily.
pub fn show<F>(item: &DetailItem, hints: &[(&str, &str)], mut on_action: F) -> io::Result<()>
where
    F: FnMut(DetailAction) -> Result<String, String>,
{
    let tty = std::fs::File::options()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|e| io::Error::other(format!("cannot open /dev/tty: {e}")))?;

    let term_height = terminal::size().map(|(_, h)| h as usize).unwrap_or(24);
    let content_lines = count_content_lines(&item.sections, !item.meta.is_empty());

    let ratio = if term_height >= 40 {
        0.6
    } else if term_height >= 24 {
        0.7
    } else {
        0.8
    };
    let budget = ((term_height as f64 * ratio) as usize).saturating_sub(CHROME_ROWS);
    let max_visible = budget.max(3);
    let viewport_height = (content_lines.min(max_visible) + CHROME_ROWS) as u16;

    let backend = CrosstermBackend::new(tty);
    let mut term = Terminal::with_options(
        backend,
        ratatui::TerminalOptions {
            viewport: ratatui::Viewport::Inline(viewport_height),
        },
    )?;

    terminal::enable_raw_mode()?;
    term.hide_cursor()?;

    let nav_fields = build_nav_fields(&item.sections);
    let result = run_loop(
        &mut term,
        item,
        &nav_fields,
        hints,
        max_visible,
        &mut on_action,
    );

    term.show_cursor()?;
    terminal::disable_raw_mode()?;
    let _ = term.clear();
    drop(term);

    result
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct DetailState {
    selected: usize,
    scroll_offset: usize,
    revealed: HashSet<usize>,
    feedback: Option<(String, bool, Instant)>,
}

fn build_nav_fields(sections: &[Section]) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    for (si, section) in sections.iter().enumerate() {
        for (fi, _) in section.fields.iter().enumerate() {
            result.push((si, fi));
        }
    }
    result
}

fn count_content_lines(sections: &[Section], has_meta: bool) -> usize {
    let mut lines = 0;
    for (i, section) in sections.iter().enumerate() {
        if i > 0 {
            lines += 1; // gap between sections
        }
        lines += 1; // section header
        lines += section.fields.len();
    }
    if has_meta {
        lines += 2; // gap + meta line
    }
    lines
}

// ---------------------------------------------------------------------------
// Event loop
// ---------------------------------------------------------------------------

fn run_loop<F>(
    term: &mut Terminal<CrosstermBackend<std::fs::File>>,
    item: &DetailItem,
    nav_fields: &[(usize, usize)],
    hints: &[(&str, &str)],
    max_visible: usize,
    on_action: &mut F,
) -> io::Result<()>
where
    F: FnMut(DetailAction) -> Result<String, String>,
{
    let mut state = DetailState {
        selected: 0,
        scroll_offset: 0,
        revealed: HashSet::new(),
        feedback: None,
    };
    let count = nav_fields.len();

    loop {
        // Expire old feedback
        if let Some((_, _, when)) = &state.feedback {
            if when.elapsed().as_secs() >= 3 {
                state.feedback = None;
            }
        }

        // Ensure selected is visible
        if state.selected < state.scroll_offset {
            state.scroll_offset = state.selected;
        } else if state.selected >= state.scroll_offset + max_visible {
            state.scroll_offset = state.selected - max_visible + 1;
        }

        crossterm::queue!(
            term.backend_mut(),
            crossterm::terminal::BeginSynchronizedUpdate
        )?;
        term.draw(|frame| {
            let area = frame.area();
            let buf = frame.buffer_mut();
            render_detail(
                buf,
                area,
                item,
                nav_fields,
                state.selected,
                state.scroll_offset,
                &state.revealed,
                &state.feedback,
                hints,
            );
        })?;
        crossterm::queue!(
            term.backend_mut(),
            crossterm::terminal::EndSynchronizedUpdate
        )?;
        term.backend_mut().flush()?;

        let ev = event::read()?;
        let Event::Key(key) = ev else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        let is_up = matches!(key.code, KeyCode::Up)
            || matches!((key.code, key.modifiers), (KeyCode::Char('k'), m) if !m.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT))
            || matches!(
                (key.code, key.modifiers),
                (KeyCode::Char('p'), KeyModifiers::CONTROL)
            );
        let is_down = matches!(key.code, KeyCode::Down)
            || matches!((key.code, key.modifiers), (KeyCode::Char('j'), m) if !m.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT))
            || matches!(
                (key.code, key.modifiers),
                (KeyCode::Char('n'), KeyModifiers::CONTROL)
            );
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);

        if matches!(
            (key.code, key.modifiers),
            (KeyCode::Esc, _)
                | (KeyCode::Char('q'), _)
                | (KeyCode::Char('c'), KeyModifiers::CONTROL)
        ) {
            return Ok(());
        } else if is_up {
            if state.selected > 0 {
                state.selected -= 1;
            }
        } else if is_down {
            if count > 0 && state.selected < count - 1 {
                state.selected += 1;
            }
        } else if key.code == KeyCode::Char('r') && plain {
            if let Some(&(si, fi)) = nav_fields.get(state.selected) {
                let field = &item.sections[si].fields[fi];
                if matches!(field.value, FieldKind::Secret(_)) {
                    if state.revealed.contains(&state.selected) {
                        state.revealed.remove(&state.selected);
                    } else {
                        state.revealed.insert(state.selected);
                    }
                }
            }
        } else if key.code == KeyCode::Char('y') && plain {
            match on_action(DetailAction::Copy(state.selected)) {
                Ok(msg) => state.feedback = Some((msg, false, Instant::now())),
                Err(msg) => state.feedback = Some((msg, true, Instant::now())),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn render_detail(
    buf: &mut Buffer,
    area: Rect,
    item: &DetailItem,
    nav_fields: &[(usize, usize)],
    selected: usize,
    scroll_offset: usize,
    revealed: &HashSet<usize>,
    feedback: &Option<(String, bool, Instant)>,
    hints: &[(&str, &str)],
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let hr_style = Style::default().fg(Color::DarkGray);
    let mut y = area.y;

    // -- Header: badge + title + tags --
    let badge_style = Style::default()
        .fg(item.badge.color)
        .add_modifier(Modifier::BOLD);
    let badge_text = format!(" {} ", item.badge.text);
    buf.set_string(area.x + 2, y, &badge_text, badge_style);
    buf.set_string(
        area.x + 2 + badge_text.len() as u16 + 1,
        y,
        &item.title,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    if !item.tags.is_empty() {
        let tags: String = item
            .tags
            .iter()
            .map(|t| format!("[{t}]"))
            .collect::<Vec<_>>()
            .join(" ");
        let tx = area.width.saturating_sub(tags.len() as u16 + 2);
        buf.set_string(area.x + tx, y, &tags, Style::default().fg(Color::DarkGray));
    }
    y += 2; // header + gap

    // Content area ends before bottom chrome (4 rows)
    let content_bottom = area.y + area.height.saturating_sub(4);

    // -- Sections + fields --
    let mut nav_idx: usize = 0;
    let mut line_idx: usize = 0;

    for (si, section) in item.sections.iter().enumerate() {
        // Gap between sections
        if si > 0 {
            if line_idx >= scroll_offset && y < content_bottom {
                y += 1;
            }
            line_idx += 1;
        }

        // Section header
        if line_idx >= scroll_offset && y < content_bottom {
            paint::section_header(buf, area.x + 2, y, area.width - 4, section.title, hr_style);
            y += 1;
        }
        line_idx += 1;

        // Fields
        for field in section.fields.iter() {
            let is_sel = nav_idx == selected;

            if line_idx >= scroll_offset && y < content_bottom {
                let is_revealed = revealed.contains(&nav_idx);
                render_field_line(buf, area.x, y, area.width, field, is_sel, is_revealed);
                y += 1;
            }
            line_idx += 1;
            nav_idx += 1;
        }
    }

    // -- Meta line (not navigable) --
    if !item.meta.is_empty() && y + 1 < content_bottom {
        y += 1;
        let meta_str: String = item
            .meta
            .iter()
            .map(|(k, v)| format!("{k}  {v}"))
            .collect::<Vec<_>>()
            .join("    ");
        buf.set_string(
            area.x + 4,
            y,
            &meta_str,
            Style::default().fg(Color::DarkGray),
        );
    }

    // -- Bottom chrome (anchored to bottom) --
    let bottom = area.y + area.height;

    // Row 4 (bottom): hints right-aligned
    if !hints.is_empty() {
        let hints_y = bottom - 1;
        render_hints_right(buf, area.x, hints_y, area.width, hints);
    }

    // Row 3: lower HR
    if bottom >= 3 {
        let hr_y = bottom - 2;
        let hr_line: String = "\u{2500}".repeat(area.width as usize);
        buf.set_string(area.x, hr_y, &hr_line, hr_style);
    }

    // Row 2: feedback line
    if bottom >= 4 {
        let fb_y = bottom - 3;
        if let Some((msg, is_err, _)) = feedback {
            let style = if *is_err {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            };
            buf.set_string(area.x + 1, fb_y, msg, style);
        }
    }

    // Row 1: upper HR with field count
    if bottom >= 5 {
        let hr_y = bottom - 4;
        let status = format!(" {}/{} ", selected + 1, nav_fields.len());
        let status_w = status.len() as u16;
        let rule_w = area.width.saturating_sub(status_w);
        let hr_left: String = "\u{2500}".repeat(rule_w as usize);
        buf.set_string(area.x, hr_y, &hr_left, hr_style);
        buf.set_string(area.x + rule_w, hr_y, &status, theme::dim());
    }
}

fn render_field_line(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    field: &FieldEntry,
    selected: bool,
    revealed: bool,
) {
    let bg = if selected {
        theme::BG_SELECTED
    } else {
        Color::Reset
    };

    if selected {
        paint::fill_bg(buf, Rect::new(x, y, width, 1), bg);
    }

    let label_style = Style::default().fg(Color::DarkGray).bg(bg);
    let value_style = if selected {
        Style::default().fg(Color::White).bg(bg)
    } else {
        Style::default().fg(Color::Gray).bg(bg)
    };
    let hint_style = if selected {
        Style::default().fg(Color::DarkGray).bg(bg)
    } else {
        Style::default().fg(Color::Black).bg(bg)
    };

    let pad = 4u16;
    let label_w = 14u16;

    // Label (left-aligned, fixed width)
    let label_display = format!("{:<w$}", field.label, w = label_w as usize);
    buf.set_string(x + pad, y, &label_display, label_style);

    // Hint (right-aligned)
    let hint = field_hint(&field.value, revealed);
    let hint_len = hint.len() as u16;
    let hint_x = x + width - hint_len - pad;
    if !hint.is_empty() {
        buf.set_string(hint_x, y, hint, hint_style);
    }

    // Value (between label and hint)
    let val_x = x + pad + label_w + 1;
    let max_val_w = if hint.is_empty() {
        (x + width - pad).saturating_sub(val_x) as usize
    } else {
        hint_x.saturating_sub(val_x).saturating_sub(1) as usize
    };
    let value = field_display_value(&field.value, revealed);

    if value.len() <= max_val_w {
        buf.set_string(val_x, y, &value, value_style);
    } else if max_val_w > 2 {
        buf.set_string(val_x, y, &value[..max_val_w - 1], value_style);
        buf.set_string(val_x + max_val_w as u16 - 1, y, "\u{2026}", value_style);
    }
}

fn render_hints_right(buf: &mut Buffer, x: u16, y: u16, width: u16, hints: &[(&str, &str)]) {
    let action_style = Style::default().fg(Color::DarkGray);
    let key_style = Style::default().fg(Color::Gray);

    let total_w: u16 = hints
        .iter()
        .enumerate()
        .map(|(i, (key, action))| {
            let sep = if i > 0 { 2u16 } else { 0 };
            sep + unicode_width::UnicodeWidthStr::width(*key) as u16 + 1 + action.len() as u16
        })
        .sum();

    let mut fx = (x + width).saturating_sub(total_w + 1);
    for (i, (key, action)) in hints.iter().enumerate() {
        if i > 0 {
            fx += 2;
        }
        let key_w = unicode_width::UnicodeWidthStr::width(*key) as u16;
        buf.set_string(fx, y, key, key_style);
        fx += key_w;
        buf.set_string(fx, y, " ", action_style);
        fx += 1;
        buf.set_string(fx, y, action, action_style);
        fx += action.len() as u16;
    }
}

fn field_display_value(kind: &FieldKind, revealed: bool) -> String {
    match kind {
        FieldKind::Plain(s) => s.clone(),
        FieldKind::Secret(s) => {
            if revealed {
                s.clone()
            } else {
                "\u{2022}".repeat(s.len().min(20))
            }
        }
        FieldKind::Url(u) => u.clone(),
        FieldKind::File { name, size } => format!("{name}  ({size} B)"),
        FieldKind::Totp => "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}".into(),
        FieldKind::Note(s) => {
            let first = s.lines().next().unwrap_or("");
            if first.len() > 60 {
                format!("{}\u{2026}", &first[..59])
            } else {
                first.to_string()
            }
        }
    }
}

fn field_hint(kind: &FieldKind, revealed: bool) -> &'static str {
    match kind {
        FieldKind::Secret(_) => {
            if revealed {
                "r hide  y copy"
            } else {
                "r show  y copy"
            }
        }
        FieldKind::Totp => "y copy",
        FieldKind::File { .. } => "",
        FieldKind::Note(_) => "",
        _ => "y copy",
    }
}
