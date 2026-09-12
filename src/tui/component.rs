use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// A component that can render itself, report its desired height, and
/// optionally report where the terminal cursor should be placed.
pub trait Component {
    fn render(&self, area: Rect, buf: &mut Buffer);
    fn desired_height(&self, width: u16) -> u16;
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let _ = area;
        None
    }
}

impl Component for Box<dyn Component + '_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        (**self).render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        (**self).desired_height(width)
    }
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        (**self).cursor_pos(area)
    }
}

/// A vertical flex layout engine inspired by Flutter / codex.
///
/// Children with flex weight 0 get their `desired_height`. Remaining space
/// is distributed proportionally among children with weight > 0.
pub struct FlexRenderable<'a> {
    children: Vec<FlexChild<'a>>,
}

struct FlexChild<'a> {
    weight: u16,
    child: Box<dyn Component + 'a>,
}

impl<'a> FlexRenderable<'a> {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    pub fn push(&mut self, weight: u16, child: impl Component + 'a) {
        self.children.push(FlexChild {
            weight,
            child: Box::new(child),
        });
    }

    /// Compute the rect for each child within `area`.
    fn layout(&self, area: Rect) -> Vec<Rect> {
        let mut rects = vec![Rect::default(); self.children.len()];
        let mut remaining = area.height;
        let mut total_flex: u16 = 0;

        // First pass: allocate fixed (weight 0) children
        for (i, fc) in self.children.iter().enumerate() {
            if fc.weight == 0 {
                let h = fc.child.desired_height(area.width).min(remaining);
                rects[i] = Rect::new(area.x, 0, area.width, h); // y set later
                remaining = remaining.saturating_sub(h);
            } else {
                total_flex += fc.weight;
            }
        }

        // Second pass: distribute remaining space to flex children
        if total_flex > 0 {
            let mut flex_remaining = remaining;
            for (i, fc) in self.children.iter().enumerate() {
                if fc.weight > 0 {
                    let h = (remaining as u32 * fc.weight as u32 / total_flex as u32) as u16;
                    let h = h.min(flex_remaining);
                    rects[i] = Rect::new(area.x, 0, area.width, h);
                    flex_remaining = flex_remaining.saturating_sub(h);
                }
            }
        }

        // Third pass: assign y positions top-to-bottom
        let mut y = area.y;
        for r in &mut rects {
            r.y = y;
            y += r.height;
        }

        rects
    }
}

impl Component for FlexRenderable<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let rects = self.layout(area);
        for (i, fc) in self.children.iter().enumerate() {
            if rects[i].height > 0 {
                fc.child.render(rects[i], buf);
            }
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.children
            .iter()
            .filter(|fc| fc.weight == 0)
            .map(|fc| fc.child.desired_height(width))
            .sum()
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let rects = self.layout(area);
        // Last child with a cursor wins (prompt is last)
        for (i, fc) in self.children.iter().enumerate().rev() {
            if let Some(pos) = fc.child.cursor_pos(rects[i]) {
                return Some(pos);
            }
        }
        None
    }
}

/// Horizontal split: left (flex:left_weight) | separator | right (flex:right_weight).
pub struct HorizontalSplit<'a> {
    pub left: Box<dyn Component + 'a>,
    pub right: Box<dyn Component + 'a>,
    pub left_weight: u16,
    pub right_weight: u16,
}

impl HorizontalSplit<'_> {
    fn layout(&self, area: Rect) -> (Rect, Rect) {
        let total = self.left_weight + self.right_weight;
        let sep = 1u16;
        let usable = area.width.saturating_sub(sep);
        let left_w = (usable as u32 * self.left_weight as u32 / total as u32) as u16;
        let right_w = usable.saturating_sub(left_w);
        let left = Rect::new(area.x, area.y, left_w, area.height);
        let right = Rect::new(area.x + left_w + sep, area.y, right_w, area.height);
        (left, right)
    }
}

impl Component for HorizontalSplit<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let (left_area, right_area) = self.layout(area);
        self.left.render(left_area, buf);
        self.right.render(right_area, buf);

        // Vertical separator
        let sep_x = left_area.x + left_area.width;
        let sep_style = ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray);
        for y in area.y..area.y + area.height {
            if let Some(cell) = buf.cell_mut(ratatui::layout::Position::new(sep_x, y)) {
                cell.set_char('│');
                cell.set_style(sep_style);
            }
        }
    }

    fn desired_height(&self, _width: u16) -> u16 {
        0 // flex child
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedBox(u16);
    impl Component for FixedBox {
        fn render(&self, _area: Rect, _buf: &mut Buffer) {}
        fn desired_height(&self, _width: u16) -> u16 {
            self.0
        }
    }

    struct CursorBox(u16);
    impl Component for CursorBox {
        fn render(&self, _area: Rect, _buf: &mut Buffer) {}
        fn desired_height(&self, _width: u16) -> u16 {
            self.0
        }
        fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
            Some((area.x + 2, area.y))
        }
    }

    #[test]
    fn flex_layout_splits_space() {
        let mut flex = FlexRenderable::new();
        flex.push(1, FixedBox(0));
        flex.push(0, FixedBox(3));

        let area = Rect::new(0, 0, 80, 24);
        let rects = flex.layout(area);

        assert_eq!(rects[0], Rect::new(0, 0, 80, 21));
        assert_eq!(rects[1], Rect::new(0, 21, 80, 3));
    }

    #[test]
    fn cursor_pos_from_last_child() {
        let mut flex = FlexRenderable::new();
        flex.push(1, FixedBox(0));
        flex.push(0, CursorBox(3));

        let area = Rect::new(0, 0, 80, 24);
        let pos = flex.cursor_pos(area);
        assert_eq!(pos, Some((2, 21)));
    }
}
