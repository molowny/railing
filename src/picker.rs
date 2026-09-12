//! Inline fuzzy picker using ratatui `Viewport::Inline`.
//!
//! Renders a small interactive widget below the cursor — no alternate screen.
//! Uses `/dev/tty` for output so stdout stays clean for piping.
//! Item rows use the same visual style as the shell TUI (type badges, vault labels).

use std::io::{self, IsTerminal, Write};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::Terminal;

// Re-export Color so callers don't need to depend on ratatui directly.
pub use ratatui::style::Color;

use crate::tui::{paint, theme};

/// Minimum visible items in the picker list.
const MIN_LIST_ROWS: usize = 3;

/// Rows per item.
const ROWS_PER_ITEM: usize = 1;

/// Chrome rows: scroll_hint + hr + prompt + hr + hints.
const CHROME_ROWS: usize = 5;

/// Badge descriptor for an item type (e.g. "LOGIN", Color::Blue).
pub struct Badge {
    pub text: &'static str,
    pub color: Color,
}

/// An item that can be displayed and searched in the picker.
pub struct PickerItem {
    /// Unique identifier (returned to caller).
    pub id: String,
    /// Item title.
    pub title: String,
    /// Type badge (rendered as colored label).
    pub badge: Badge,
    /// Vault or group name (right-aligned).
    pub subtitle: String,
    /// Text to match against for fuzzy search.
    pub match_text: String,
    /// Whether this item should be boosted to the top when no filter is active.
    pub is_recent: bool,
}

/// Configuration for the picker.
pub struct PickerConfig<'a> {
    /// Prompt string (e.g. "> ").
    pub prompt: &'a str,
    /// Pre-filled query (from CLI args).
    pub initial_query: &'a str,
    /// Footer key hints (key, action) — shown at the bottom, progressively collapsed.
    pub footer_hints: Vec<(&'a str, &'a str)>,
}

impl<'a> Default for PickerConfig<'a> {
    fn default() -> Self {
        Self {
            prompt: "> ",
            initial_query: "",
            footer_hints: vec![
                ("\u{2191}/\u{2193}", "navigate"),
                ("Enter", "select"),
                ("Esc", "cancel"),
            ],
        }
    }
}

/// Result of a picker interaction.
pub enum PickerResult {
    /// User selected an item (index into original items slice).
    Selected(usize),
    /// User cancelled (Esc or Ctrl-C).
    Cancelled,
}

/// Check if an interactive picker is available.
pub fn is_available() -> bool {
    std::path::Path::new("/dev/tty").exists() && std::io::stderr().is_terminal()
}

/// Open an interactive fuzzy picker rendered inline below the cursor.
pub fn pick(items: &[PickerItem], config: &PickerConfig) -> io::Result<PickerResult> {
    if items.is_empty() {
        return Ok(PickerResult::Cancelled);
    }

    // Open /dev/tty for ratatui output (keeps stdout clean)
    let tty = std::fs::File::options()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("cannot open /dev/tty: {e}")))?;

    // Adaptive list size based on terminal height:
    //   tall (>=40):  ~30% of terminal
    //   medium (>=24): ~40%
    //   small (<24):   ~50%
    let term_height = terminal::size().map(|(_, h)| h as usize).unwrap_or(24);
    let ratio = if term_height >= 40 {
        0.45
    } else if term_height >= 24 {
        0.55
    } else {
        0.65
    };
    let budget = ((term_height as f64 * ratio) as usize).saturating_sub(CHROME_ROWS);
    let max_list = (budget / ROWS_PER_ITEM).max(MIN_LIST_ROWS);
    let list_count = items.len().min(max_list);
    let viewport_height = (list_count * ROWS_PER_ITEM + CHROME_ROWS) as u16;

    // Set up ratatui with inline viewport
    let backend = CrosstermBackend::new(tty);
    let mut term = Terminal::with_options(
        backend,
        ratatui::TerminalOptions {
            viewport: ratatui::Viewport::Inline(viewport_height),
        },
    )?;

    terminal::enable_raw_mode()?;
    term.hide_cursor()?;

    let result = run_loop(&mut term, items, config, list_count);

    // Cleanup
    term.show_cursor()?;
    terminal::disable_raw_mode()?;
    let _ = term.clear();
    drop(term);

    result
}

struct PickerState {
    query: String,
    selected: usize,
    scroll_offset: usize,
    matcher: Matcher,
}

struct ScoredItem {
    original_index: usize,
    score: u32,
    /// Matched character indices in match_text (for highlighting).
    match_indices: Vec<u32>,
}

fn run_loop(
    term: &mut Terminal<CrosstermBackend<std::fs::File>>,
    items: &[PickerItem],
    config: &PickerConfig,
    max_list: usize,
) -> io::Result<PickerResult> {
    let mut state = PickerState {
        query: config.initial_query.to_string(),
        selected: 0,
        scroll_offset: 0,
        matcher: Matcher::new(Config::DEFAULT.match_paths()),
    };

    loop {
        let filtered = score_items(items, &state.query, &mut state.matcher);
        let count = filtered.len();

        if count == 0 {
            state.selected = 0;
        } else if state.selected >= count {
            state.selected = count - 1;
        }

        if state.selected < state.scroll_offset {
            state.scroll_offset = state.selected;
        } else if state.selected >= state.scroll_offset + max_list {
            state.scroll_offset = state.selected - max_list + 1;
        }

        let prompt_text = config.prompt;
        let query = &state.query;
        let sel = state.selected;
        let scroll = state.scroll_offset;
        let hints = &config.footer_hints;

        // Synchronized output: terminal buffers all writes and paints atomically.
        // Eliminates flicker on GPU terminals (kitty, alacritty, wezterm).
        crossterm::queue!(
            term.backend_mut(),
            crossterm::terminal::BeginSynchronizedUpdate
        )?;
        term.draw(|frame| {
            let area = frame.area();
            let buf = frame.buffer_mut();
            render_picker(
                buf,
                area,
                items,
                &filtered,
                query,
                prompt_text,
                sel,
                scroll,
                max_list,
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

        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                return Ok(PickerResult::Cancelled);
            }
            (KeyCode::Enter, _) => {
                if count > 0 {
                    return Ok(PickerResult::Selected(
                        filtered[state.selected].original_index,
                    ));
                }
            }
            (KeyCode::Up, _) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                if state.selected > 0 {
                    state.selected -= 1;
                }
            }
            (KeyCode::Down, _) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                if count > 0 && state.selected < count - 1 {
                    state.selected += 1;
                }
            }
            (KeyCode::Backspace, _) => {
                state.query.pop();
                state.selected = 0;
                state.scroll_offset = 0;
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                state.query.clear();
                state.selected = 0;
                state.scroll_offset = 0;
            }
            (KeyCode::Char(c), m) if !m.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                state.query.push(c);
                state.selected = 0;
                state.scroll_offset = 0;
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_picker(
    buf: &mut Buffer,
    area: Rect,
    items: &[PickerItem],
    filtered: &[ScoredItem],
    query: &str,
    prompt: &str,
    selected: usize,
    scroll_offset: usize,
    max_list: usize,
    footer_hints: &[(&str, &str)],
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let mut y = area.y;
    let hr_style = Style::default().fg(Color::DarkGray);

    // -- Item rows (content + gap between items) --
    let visible_end = (scroll_offset + max_list).min(filtered.len());
    for i in scroll_offset..visible_end {
        if y + 1 >= area.y + area.height {
            break;
        }

        let scored = &filtered[i];
        let item = &items[scored.original_index];
        let is_sel = i == selected;

        render_item_row(
            buf,
            area.x,
            y,
            area.width,
            item,
            is_sel,
            &scored.match_indices,
        );
        y += ROWS_PER_ITEM as u16;
    }

    // -- Scroll hint --
    if visible_end < filtered.len() && y < area.y + area.height {
        let remaining = filtered.len() - visible_end;
        let hint = format!("  \u{2026} {remaining} more");
        buf.set_string(area.x, y, &hint, theme::dim());
    }

    // -- Empty state --
    if filtered.is_empty() && y < area.y + area.height {
        buf.set_string(area.x, y, "  no matches", theme::dim());
    }

    // -- Bottom chrome (anchored to bottom of area) --
    // Layout from bottom: hints | hr | prompt | hr
    let bottom = area.y + area.height;

    // Row 4 (bottom): hints right-aligned
    if !footer_hints.is_empty() {
        let hints_y = bottom - 1;
        let action_style = Style::default().fg(Color::DarkGray);
        let key_style = Style::default().fg(Color::Gray);

        // Calculate total width of hints to right-align
        let total_w: u16 = footer_hints
            .iter()
            .enumerate()
            .map(|(i, (key, action))| {
                let sep = if i > 0 { 2u16 } else { 0 };
                sep + unicode_width::UnicodeWidthStr::width(*key) as u16 + 1 + action.len() as u16
            })
            .sum();

        let mut fx = (area.x + area.width).saturating_sub(total_w + 1);
        for (i, (key, action)) in footer_hints.iter().enumerate() {
            if i > 0 {
                fx += 2;
            }
            let key_w = unicode_width::UnicodeWidthStr::width(*key) as u16;
            let action_w = action.len() as u16;
            if fx + key_w + 1 + action_w > area.x + area.width {
                break;
            }
            buf.set_string(fx, hints_y, key, key_style);
            fx += key_w;
            buf.set_string(fx, hints_y, " ", action_style);
            fx += 1;
            buf.set_string(fx, hints_y, action, action_style);
            fx += action_w;
        }
    }

    // Row 3: lower HR
    if bottom >= 3 {
        let hr_y = bottom - 2;
        let hr_line: String = "\u{2500}".repeat(area.width as usize);
        buf.set_string(area.x, hr_y, &hr_line, hr_style);
    }

    // Row 2: prompt line
    if bottom >= 4 {
        let prompt_y = bottom - 3;
        let prompt_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        buf.set_string(area.x, prompt_y, prompt, prompt_style);
        buf.set_string(
            area.x + prompt.len() as u16,
            prompt_y,
            query,
            Style::default().fg(theme::FG_PRIMARY),
        );
    }

    // Row 1: upper HR with count right-aligned
    if bottom >= 5 {
        let hr_y = bottom - 4;
        let status = format!(" {}/{} ", filtered.len(), items.len());
        let status_w = status.len() as u16;
        let rule_w = area.width.saturating_sub(status_w);
        let hr_left: String = "\u{2500}".repeat(rule_w as usize);
        buf.set_string(area.x, hr_y, &hr_left, hr_style);
        buf.set_string(area.x + rule_w, hr_y, &status, theme::dim());
    }
}

/// Render a single item row with type badge, title, and right-aligned vault name.
///
/// Layout: `  > ` + ` TYPE ` + ` Title...` + `  Vault  `
fn render_item_row(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    item: &PickerItem,
    selected: bool,
    match_indices: &[u32],
) {
    let bg = if selected {
        theme::BG_SELECTED
    } else {
        Color::Reset
    };

    // Fill row background
    if selected {
        paint::fill_bg(buf, Rect::new(x, y, width, 1), bg);
    }

    let base_style = if selected {
        theme::selected(false)
    } else {
        theme::normal()
    };

    // Left margin (4 cols) — selected has bg highlight, no extra indicator needed
    buf.set_string(x, y, "    ", base_style);
    let mut cx = x + 4;

    // Type badge: ` LG ` — short 2-char codes, padded, colored bg
    let badge_style = Style::default()
        .fg(item.badge.color)
        .add_modifier(Modifier::BOLD);
    let badge_text = format!(" {} ", item.badge.text);
    buf.set_string(cx, y, &badge_text, badge_style);
    cx += badge_text.len() as u16 + 1; // +1 gap after badge

    // Right-aligned: optional "recent" tag + vault/subtitle
    let right_pad = 2u16;
    let recent_tag = if item.is_recent { " recent" } else { "" };
    let subtitle = &item.subtitle;
    let right_text = if recent_tag.is_empty() {
        subtitle.clone()
    } else if subtitle.is_empty() {
        recent_tag.to_string()
    } else {
        format!("{subtitle}  {recent_tag}")
    };
    let right_w = right_text.len() as u16;
    let right_x = x + width - right_pad - right_w;
    let subtitle_style = if selected {
        Style::default()
            .fg(theme::FG_SECONDARY)
            .bg(theme::BG_SELECTED)
    } else {
        theme::dim()
    };

    let title_end = if !right_text.is_empty() && right_x > cx + 2 {
        // Render right-aligned block (vault + recent tag)
        if item.is_recent {
            // Vault in dim, "recent" in yellow
            let vault_end = right_text.len() - recent_tag.len();
            buf.set_string(right_x, y, &right_text[..vault_end], subtitle_style);
            let recent_style = Style::default().fg(Color::Yellow).bg(bg);
            buf.set_string(right_x + vault_end as u16, y, recent_tag, recent_style);
        } else {
            buf.set_string(right_x, y, &right_text, subtitle_style);
        }
        right_x.saturating_sub(1)
    } else {
        x + width - right_pad
    };

    // Title with match highlighting
    let max_title_w = title_end.saturating_sub(cx) as usize;
    let title_style = if selected {
        theme::selected(true)
    } else {
        theme::normal()
    };
    let highlight_style = if selected {
        Style::default()
            .fg(Color::Cyan)
            .bg(theme::BG_SELECTED)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    };

    // match_indices refer to match_text which starts with title
    let title_len = item.title.len();
    let mut col = cx;
    for (i, ch) in item.title.chars().enumerate() {
        if i >= max_title_w {
            break;
        }
        let is_match = match_indices.contains(&(i as u32)) && i < title_len;
        let style = if is_match {
            highlight_style
        } else {
            title_style
        };
        buf.set_string(col, y, &ch.to_string(), style);
        col += 1;
    }
}

// ---------------------------------------------------------------------------
// Fuzzy scoring
// ---------------------------------------------------------------------------

fn score_items(items: &[PickerItem], query: &str, matcher: &mut Matcher) -> Vec<ScoredItem> {
    if query.is_empty() {
        let mut scored: Vec<ScoredItem> = items
            .iter()
            .enumerate()
            .map(|(i, item)| ScoredItem {
                original_index: i,
                score: if item.is_recent { 1 } else { 0 },
                match_indices: Vec::new(),
            })
            .collect();
        scored.sort_by(|a, b| b.score.cmp(&a.score));
        return scored;
    }

    let pattern = Pattern::new(
        query,
        CaseMatching::Smart,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut buf = Vec::new();

    let mut scored: Vec<ScoredItem> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let haystack = Utf32Str::new(&item.match_text, &mut buf);
            let mut indices = Vec::new();
            let score = pattern.indices(haystack, matcher, &mut indices)?;
            Some(ScoredItem {
                original_index: i,
                score,
                match_indices: indices,
            })
        })
        .collect();

    scored.sort_by(|a, b| b.score.cmp(&a.score));
    scored
}
