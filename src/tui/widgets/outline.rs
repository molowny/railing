//! Fold-aware outline: a tree rendered as one indented, scrollable document.
//!
//! ```text
//!   title        GitHub
//! ▾ Credentials
//!     username     alice
//!   ▾ password     ••••••••
//!       role       Password
//!     ▸ attrs
//!   ▸ totp         123 456
//! ```
//!
//! The consumer walks its own tree once per frame and emits rows through an
//! [`OutlineBuilder`]; the builder answers whether each branch is open, so a
//! collapsed subtree is **never visited**. That matters beyond performance —
//! a model holding secrets never formats a row the user cannot see.
//!
//! # Fold state lives here, keyed, not in the tree
//!
//! Nothing is stored in the caller's nodes, because the tree may be rebuilt
//! from scratch every frame — derived from a mutable draft, re-sorted,
//! reordered. A positional path (`[0, 3, 1]`) would silently come to mean a
//! different node the moment a sibling moves. [`OutlineState`] instead keys on
//! whatever stable identity the consumer already has, and stores only
//! *departures* from each node's declared default, so an empty state renders
//! the intended shape with nothing pre-seeded.
//!
//! # It does not handle keys
//!
//! [`super::text_input::TextInput`] owns its keymap because a text buffer's
//! bindings are universal. An outline's are not: one app makes `l` mean
//! expand-or-edit-or-toggle and routes it through a [`ChordResolver`], another
//! makes it open a diff pane. So this module offers semantic operations —
//! [`OutlineState::step`], [`fold_left`], [`fold_right`] — and leaves policy to
//! the caller, the same split [`ChordResolver`] uses.
//!
//! Rows may be taller than one line ([`OutlineBuilder::leaf_tall`], for a
//! wrapped value or an inline editor), so scrolling is solved in *visual
//! lines* rather than row counts.
//!
//! [`ChordResolver`]: crate::tui::chord::ChordResolver

use std::collections::HashMap;
use std::hash::Hash;

use ratatui::buffer::Buffer;
use ratatui::style::Style;

use crate::tui::paint;

/// Glyph for a branch whose children are showing.
pub const GLYPH_EXPANDED: char = '\u{25BE}'; // ▾
/// Glyph for a branch whose children are hidden.
pub const GLYPH_COLLAPSED: char = '\u{25B8}'; // ▸
/// Columns of indent per depth level.
pub const INDENT_COLS: u16 = 2;
/// Columns reserved for the fold glyph and the space after it.
pub const GUTTER_COLS: u16 = 2;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// One emitted row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineRow<K, P> {
    /// Stable identity, supplied by the consumer.
    pub key: K,
    /// Nesting level; row content is indented by `depth * INDENT_COLS`.
    pub depth: u16,
    /// `Some(is_expanded)` for a branch, `None` for a leaf.
    pub fold: Option<bool>,
    /// Visual lines this row occupies. Always at least 1.
    pub height: u16,
    /// Whatever the consumer needs to paint the row.
    pub payload: P,
}

impl<K, P> OutlineRow<K, P> {
    /// True when this row has children (whether or not they are showing).
    pub fn is_branch(&self) -> bool {
        self.fold.is_some()
    }

    /// True for a branch that is currently showing its children.
    pub fn is_expanded(&self) -> bool {
        self.fold == Some(true)
    }
}

/// The flattened, fold-resolved row list for one frame.
#[derive(Debug, Clone)]
pub struct Outline<K, P> {
    rows: Vec<OutlineRow<K, P>>,
}

impl<K, P> Outline<K, P> {
    pub fn rows(&self) -> &[OutlineRow<K, P>] {
        &self.rows
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn row(&self, i: usize) -> Option<&OutlineRow<K, P>> {
        self.rows.get(i)
    }

    /// Total visual lines, summing row heights.
    pub fn total_lines(&self) -> usize {
        self.rows.iter().map(|r| r.height as usize).sum()
    }

    /// First visual line of row `i`.
    pub fn line_of(&self, i: usize) -> usize {
        self.rows.iter().take(i).map(|r| r.height as usize).sum()
    }

    /// Drop every row whose `keep` entry is `false`.
    ///
    /// For narrowing a built outline to a search result. Filtering happens
    /// here rather than in the emitter because deciding what to keep needs the
    /// *whole* tree in view — a branch stays when a descendant matches, which
    /// nothing walking top-down can know yet.
    ///
    /// Entries beyond `keep`'s length are dropped, so a short mask truncates.
    pub fn keep(&mut self, keep: &[bool]) {
        let mut i = 0;
        self.rows.retain(|_| {
            let k = keep.get(i).copied().unwrap_or(false);
            i += 1;
            k
        });
    }

    /// Add `extra` visual lines to row `i`.
    ///
    /// For a row that grows *after* the tree is built — a leaf hosting an
    /// inline multi-line editor, which only knows its height once the editor is
    /// open. Adjusting the built outline keeps that a presentation concern
    /// rather than making the emitter aware of a modal.
    pub fn grow_row(&mut self, i: usize, extra: u16) {
        if let Some(row) = self.rows.get_mut(i) {
            row.height = row.height.saturating_add(extra);
        }
    }

    /// Nearest preceding row at a smaller depth, or `None` at the top level.
    pub fn parent_of(&self, i: usize) -> Option<usize> {
        let depth = self.rows.get(i)?.depth;
        if depth == 0 {
            return None;
        }
        self.rows[..i].iter().rposition(|r| r.depth < depth)
    }
}

impl<K: PartialEq, P> Outline<K, P> {
    pub fn index_of(&self, key: &K) -> Option<usize> {
        self.rows.iter().position(|r| r.key == *key)
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Collects rows for one frame, consulting [`OutlineState`] for fold decisions.
pub struct OutlineBuilder<'a, K, P> {
    state: &'a OutlineState<K>,
    rows: Vec<OutlineRow<K, P>>,
}

impl<'a, K: Eq + Hash + Clone, P> OutlineBuilder<'a, K, P> {
    pub fn new(state: &'a OutlineState<K>) -> Self {
        Self {
            state,
            rows: Vec::new(),
        }
    }

    /// Push a childless row occupying one line.
    pub fn leaf(&mut self, depth: u16, key: K, payload: P) {
        self.rows.push(OutlineRow {
            key,
            depth,
            fold: None,
            height: 1,
            payload,
        });
    }

    /// Push a childless row occupying `height` lines — a wrapped value, or a
    /// leaf hosting an inline multi-line editor. `height` is floored at 1.
    pub fn leaf_tall(&mut self, depth: u16, key: K, height: u16, payload: P) {
        self.rows.push(OutlineRow {
            key,
            depth,
            fold: None,
            height: height.max(1),
            payload,
        });
    }

    /// Push a branch header and report whether its children should be emitted.
    ///
    /// `default_expanded` applies only while the user has not decided for this
    /// key, which is what lets one tree mix defaults — a section open, the
    /// fields inside it closed — with no setup.
    ///
    /// Recursing when this returns `false` is a bug: the children would be
    /// emitted as though visible.
    #[must_use = "emit the branch's children only when this returns true"]
    pub fn branch(&mut self, depth: u16, key: K, default_expanded: bool, payload: P) -> bool {
        let expanded = self.state.is_expanded(&key, default_expanded);
        self.rows.push(OutlineRow {
            key,
            depth,
            fold: Some(expanded),
            height: 1,
            payload,
        });
        expanded
    }

    pub fn finish(self) -> Outline<K, P> {
        Outline { rows: self.rows }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Cursor, scroll, and fold decisions. Survives rebuilds of the tree.
#[derive(Debug, Clone)]
pub struct OutlineState<K> {
    /// Per-key departures from the default declared at emit time. A key that
    /// is absent is still following its default.
    overrides: HashMap<K, bool>,
    /// Set by [`expand_all`]/[`collapse_all`]: a baseline replacing every
    /// node's declared default until a per-key override lands on top.
    ///
    /// [`expand_all`]: OutlineState::expand_all
    /// [`collapse_all`]: OutlineState::collapse_all
    force: Option<bool>,
    cursor: usize,
    /// Scroll position in visual lines, not rows.
    offset: usize,
}

impl<K> Default for OutlineState<K> {
    fn default() -> Self {
        Self {
            overrides: HashMap::new(),
            force: None,
            cursor: 0,
            offset: 0,
        }
    }
}

impl<K: Eq + Hash + Clone> OutlineState<K> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, i: usize) {
        self.cursor = i;
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    // -- folding ------------------------------------------------------------

    /// Whether `key` is expanded, given the default its emitter declares.
    ///
    /// Precedence: an explicit per-key decision, then any [`expand_all`] /
    /// [`collapse_all`] baseline, then the emitter's default.
    ///
    /// [`expand_all`]: OutlineState::expand_all
    /// [`collapse_all`]: OutlineState::collapse_all
    pub fn is_expanded(&self, key: &K, default_expanded: bool) -> bool {
        self.overrides
            .get(key)
            .copied()
            .or(self.force)
            .unwrap_or(default_expanded)
    }

    pub fn set_expanded(&mut self, key: K, expanded: bool) {
        self.overrides.insert(key, expanded);
    }

    /// Flip `key`, given the default its emitter declares.
    pub fn toggle(&mut self, key: K, default_expanded: bool) {
        let now = self.is_expanded(&key, default_expanded);
        self.overrides.insert(key, !now);
    }

    /// Drop the decision for `key`, returning it to its declared default.
    pub fn reset(&mut self, key: &K) {
        self.overrides.remove(key);
    }

    /// Expand everything, overriding declared defaults, until individual keys
    /// are toggled again.
    pub fn expand_all(&mut self) {
        self.overrides.clear();
        self.force = Some(true);
    }

    /// Collapse everything, overriding declared defaults.
    pub fn collapse_all(&mut self) {
        self.overrides.clear();
        self.force = Some(false);
    }

    /// Forget every decision and baseline; all nodes return to their defaults.
    pub fn reset_all(&mut self) {
        self.overrides.clear();
        self.force = None;
    }

    /// Drop decisions for keys the emitter no longer produces, so a deleted
    /// node cannot pin fold state for the rest of the session.
    ///
    /// Only prunes what is reachable: a key inside a currently-collapsed
    /// subtree is not emitted, so callers should pass a predicate over their
    /// model rather than over the last frame's rows.
    pub fn retain_keys(&mut self, live: impl Fn(&K) -> bool) {
        self.overrides.retain(|k, _| live(k));
    }

    // -- cursor -------------------------------------------------------------

    pub fn step(&mut self, delta: isize, total: usize) {
        if total == 0 {
            self.cursor = 0;
            return;
        }
        let next = (self.cursor as isize + delta).clamp(0, total as isize - 1);
        self.cursor = next as usize;
    }

    pub fn jump_top(&mut self) {
        self.cursor = 0;
    }

    pub fn jump_bottom(&mut self, total: usize) {
        self.cursor = total.saturating_sub(1);
    }

    pub fn clamp(&mut self, total: usize) {
        if total == 0 {
            self.cursor = 0;
            self.offset = 0;
        } else if self.cursor >= total {
            self.cursor = total - 1;
        }
    }

    // -- scrolling ----------------------------------------------------------

    /// Move `offset` so the cursor row is fully visible, keeping `scrolloff`
    /// lines of context where the content allows.
    ///
    /// Works in visual lines so a tall row scrolls as the block it is. A row
    /// taller than the whole viewport is pinned to its first line rather than
    /// its last — losing the top of a value is worse than losing the bottom.
    pub fn solve_scroll<P>(
        &mut self,
        outline: &Outline<K, P>,
        viewport_h: usize,
        scrolloff: usize,
    ) {
        let total = outline.total_lines();
        if viewport_h == 0 || total == 0 {
            self.offset = 0;
            return;
        }
        if total <= viewport_h {
            self.offset = 0;
            return;
        }

        let cursor = self.cursor.min(outline.len().saturating_sub(1));
        let start = outline.line_of(cursor);
        let height = outline.row(cursor).map_or(1, |r| r.height as usize);
        let end = start + height;

        // A large scrolloff in a short viewport would fight itself.
        let pad = scrolloff.min(viewport_h.saturating_sub(1) / 2);

        if start < self.offset + pad {
            self.offset = start.saturating_sub(pad);
        } else if end + pad > self.offset + viewport_h {
            self.offset = (end + pad).saturating_sub(viewport_h);
        }

        // Never scroll past the end, and keep the cursor's first line visible
        // even when the row alone is taller than the viewport.
        let max_offset = total.saturating_sub(viewport_h);
        self.offset = self.offset.min(max_offset).min(start);
    }

    /// Rows intersecting the viewport, as `(row_index, y)` where `y` is the
    /// row's first line relative to the top of the viewport.
    ///
    /// `y` is negative for a row clipped at the top; the caller skips that many
    /// of its lines. Returns a `Vec` rather than an iterator because row counts
    /// are small and the borrow it would otherwise need makes every call site
    /// noisier than the allocation is worth.
    pub fn viewport<P>(&self, outline: &Outline<K, P>, viewport_h: usize) -> Vec<(usize, i32)> {
        let mut out = Vec::new();
        if viewport_h == 0 {
            return out;
        }
        let top = self.offset;
        let bottom = top + viewport_h;

        let mut line = 0usize;
        for (i, row) in outline.rows().iter().enumerate() {
            let h = row.height as usize;
            if line >= bottom {
                break;
            }
            if line + h > top {
                out.push((i, line as i32 - top as i32));
            }
            line += h;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Fold navigation
// ---------------------------------------------------------------------------

/// Outcome of [`fold_left`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldLeft {
    /// An expanded branch was closed.
    Collapsed,
    /// Already closed, or a leaf — the cursor moved to the parent.
    MovedToParent,
    /// Nothing to do: a top-level row that was already collapsed.
    None,
}

/// Outcome of [`fold_right`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldRight {
    /// A collapsed branch was opened.
    Expanded,
    /// Already open — the cursor moved to the first child.
    MovedToChild,
    /// A leaf: the caller should run its own open/edit action.
    Activate,
}

/// `h`-style navigation: close an open branch, else rise to the parent.
pub fn fold_left<K: Eq + Hash + Clone, P>(
    outline: &Outline<K, P>,
    state: &mut OutlineState<K>,
) -> FoldLeft {
    let Some(row) = outline.row(state.cursor()) else {
        return FoldLeft::None;
    };
    if row.is_expanded() {
        state.set_expanded(row.key.clone(), false);
        return FoldLeft::Collapsed;
    }
    match outline.parent_of(state.cursor()) {
        Some(p) => {
            state.set_cursor(p);
            FoldLeft::MovedToParent
        }
        None => FoldLeft::None,
    }
}

/// `l`-style navigation: open a closed branch, descend into an open one, or
/// report that a leaf needs the caller's own action.
pub fn fold_right<K: Eq + Hash + Clone, P>(
    outline: &Outline<K, P>,
    state: &mut OutlineState<K>,
) -> FoldRight {
    let i = state.cursor();
    let Some(row) = outline.row(i) else {
        return FoldRight::Activate;
    };
    match row.fold {
        Some(false) => {
            state.set_expanded(row.key.clone(), true);
            FoldRight::Expanded
        }
        Some(true) => {
            // The child, if the branch actually has one.
            match outline.row(i + 1) {
                Some(next) if next.depth > row.depth => {
                    state.set_cursor(i + 1);
                    FoldRight::MovedToChild
                }
                _ => FoldRight::Activate,
            }
        }
        None => FoldRight::Activate,
    }
}

// ---------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------

/// Paint a row's indent and fold glyph, returning the x where content starts.
///
/// Leaves get blank gutter of the same width as a glyph, so labels at one depth
/// line up whether or not their siblings have children.
pub fn paint_gutter(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    depth: u16,
    fold: Option<bool>,
    style: Style,
) -> u16 {
    let indent = depth.saturating_mul(INDENT_COLS);
    let content_x = x.saturating_add(indent).saturating_add(GUTTER_COLS);
    if width == 0 {
        return content_x;
    }
    let glyph_x = x.saturating_add(indent);
    // Out of room for the glyph — content_x is still reported so the caller's
    // own clipping does the rest.
    if glyph_x >= x.saturating_add(width) {
        return content_x;
    }
    if let Some(expanded) = fold {
        let glyph = if expanded {
            GLYPH_EXPANDED
        } else {
            GLYPH_COLLAPSED
        };
        paint::set_cell(buf, glyph_x, y, glyph, style);
    }
    content_x
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny fixture tree:
    /// ```text
    /// a            branch, default expanded
    ///   a1         leaf
    ///   a2         branch, default collapsed
    ///     a2x      leaf
    /// b            leaf
    /// ```
    fn build(state: &OutlineState<&'static str>) -> Outline<&'static str, &'static str> {
        let mut b = OutlineBuilder::new(state);
        if b.branch(0, "a", true, "a") {
            b.leaf(1, "a1", "a1");
            if b.branch(1, "a2", false, "a2") {
                b.leaf(2, "a2x", "a2x");
            }
        }
        b.leaf(0, "b", "b");
        b.finish()
    }

    fn keys(o: &Outline<&'static str, &'static str>) -> Vec<&'static str> {
        o.rows().iter().map(|r| r.key).collect()
    }

    #[test]
    fn defaults_apply_with_empty_state() {
        let st = OutlineState::new();
        // "a" open by default, "a2" closed by default.
        assert_eq!(keys(&build(&st)), vec!["a", "a1", "a2", "b"]);
    }

    #[test]
    fn collapsed_branch_children_are_never_emitted() {
        let mut st = OutlineState::new();
        st.set_expanded("a", false);
        // Not just hidden — `branch` returned false, so a1/a2 were never built.
        assert_eq!(keys(&build(&st)), vec!["a", "b"]);
    }

    #[test]
    fn override_opens_a_default_collapsed_branch() {
        let mut st = OutlineState::new();
        st.set_expanded("a2", true);
        assert_eq!(keys(&build(&st)), vec!["a", "a1", "a2", "a2x", "b"]);
    }

    /// The reason fold state is keyed rather than positional: rebuilding the
    /// tree with siblings in a different order must not move the fold.
    #[test]
    fn override_survives_sibling_reorder() {
        let mut st = OutlineState::new();
        st.set_expanded("a2", true);

        // Same keys, emitted in the opposite order.
        let mut b = OutlineBuilder::new(&st);
        b.leaf(0, "b", "b");
        if b.branch(0, "a", true, "a") {
            if b.branch(1, "a2", false, "a2") {
                b.leaf(2, "a2x", "a2x");
            }
            b.leaf(1, "a1", "a1");
        }
        let o = b.finish();

        assert_eq!(keys(&o), vec!["b", "a", "a2", "a2x", "a1"]);
        // "a2" is still the open one, despite sitting at a different index.
        assert!(o.row(o.index_of(&"a2").unwrap()).unwrap().is_expanded());
    }

    #[test]
    fn expand_all_and_collapse_all_beat_declared_defaults() {
        let mut st = OutlineState::new();
        st.expand_all();
        assert_eq!(keys(&build(&st)), vec!["a", "a1", "a2", "a2x", "b"]);

        st.collapse_all();
        assert_eq!(keys(&build(&st)), vec!["a", "b"]);

        // A later per-key decision layers on top of the baseline.
        st.set_expanded("a", true);
        assert_eq!(keys(&build(&st)), vec!["a", "a1", "a2", "b"]);

        st.reset_all();
        assert_eq!(keys(&build(&st)), vec!["a", "a1", "a2", "b"]);
    }

    #[test]
    fn retain_keys_prunes_dead_overrides() {
        let mut st = OutlineState::new();
        st.set_expanded("a2", true);
        st.set_expanded("gone", true);
        st.retain_keys(|k| *k != "gone");
        assert!(st.is_expanded(&"a2", false));
        // Back to its default, since the override was dropped.
        assert!(!st.is_expanded(&"gone", false));
    }

    #[test]
    fn keep_drops_masked_rows_and_leaves_the_rest_intact() {
        let mut st = OutlineState::new();
        st.set_expanded("a2", true);
        let mut o = build(&st); // a, a1, a2, a2x, b
        assert_eq!(keys(&o), vec!["a", "a1", "a2", "a2x", "b"]);

        // Keep the "a2" branch and its ancestor, as a search would.
        o.keep(&[true, false, true, true, false]);
        assert_eq!(keys(&o), vec!["a", "a2", "a2x"]);
        // Depths are untouched, so indentation still reads as a tree.
        assert_eq!(o.row(0).unwrap().depth, 0);
        assert_eq!(o.row(2).unwrap().depth, 2);
        assert_eq!(o.parent_of(2), Some(1));
    }

    #[test]
    fn keep_treats_a_short_mask_as_dropping_the_tail() {
        let st = OutlineState::new();
        let mut o = build(&st);
        o.keep(&[true]);
        assert_eq!(keys(&o), vec!["a"]);
    }

    #[test]
    fn parent_of_walks_up_depths() {
        let mut st = OutlineState::new();
        st.set_expanded("a2", true);
        let o = build(&st); // a, a1, a2, a2x, b
        assert_eq!(o.parent_of(0), None); // a is top level
        assert_eq!(o.parent_of(1), Some(0)); // a1 -> a
        assert_eq!(o.parent_of(3), Some(2)); // a2x -> a2
        assert_eq!(o.parent_of(4), None); // b is top level
    }

    #[test]
    fn fold_left_collapses_then_rises() {
        let mut st = OutlineState::new();
        let o = build(&st);
        st.set_cursor(0); // on "a", expanded
        assert_eq!(fold_left(&o, &mut st), FoldLeft::Collapsed);
        assert!(!st.is_expanded(&"a", true));

        let o = build(&st); // a, b
        st.set_cursor(0);
        // Already collapsed and top-level: nowhere to go.
        assert_eq!(fold_left(&o, &mut st), FoldLeft::None);
    }

    #[test]
    fn fold_left_on_a_leaf_moves_to_parent() {
        let mut st = OutlineState::new();
        let o = build(&st);
        st.set_cursor(1); // "a1", a leaf
        assert_eq!(fold_left(&o, &mut st), FoldLeft::MovedToParent);
        assert_eq!(st.cursor(), 0);
    }

    #[test]
    fn fold_right_expands_then_descends() {
        let mut st = OutlineState::new();
        let o = build(&st);
        st.set_cursor(2); // "a2", collapsed
        assert_eq!(fold_right(&o, &mut st), FoldRight::Expanded);

        let o = build(&st);
        st.set_cursor(2); // "a2", now expanded
        assert_eq!(fold_right(&o, &mut st), FoldRight::MovedToChild);
        assert_eq!(st.cursor(), 3); // "a2x"
    }

    #[test]
    fn fold_right_on_a_leaf_asks_the_caller_to_act() {
        let mut st = OutlineState::new();
        let o = build(&st);
        st.set_cursor(1); // "a1"
        assert_eq!(fold_right(&o, &mut st), FoldRight::Activate);
        assert_eq!(st.cursor(), 1, "cursor must not move");
    }

    /// An expanded branch that emitted no children is a leaf in practice.
    #[test]
    fn fold_right_on_an_empty_expanded_branch_activates() {
        let st = OutlineState::new();
        let mut b: OutlineBuilder<&str, &str> = OutlineBuilder::new(&st);
        let _ = b.branch(0, "empty", true, "empty");
        let o = b.finish();
        let mut st = st;
        st.set_cursor(0);
        assert_eq!(fold_right(&o, &mut st), FoldRight::Activate);
    }

    // -- scrolling ----------------------------------------------------------

    fn tall(heights: &[u16]) -> Outline<usize, ()> {
        let st = OutlineState::new();
        let mut b = OutlineBuilder::new(&st);
        for (i, h) in heights.iter().enumerate() {
            b.leaf_tall(0, i, *h, ());
        }
        b.finish()
    }

    #[test]
    fn line_of_and_total_lines_sum_heights() {
        let o = tall(&[1, 3, 1, 2]);
        assert_eq!(o.total_lines(), 7);
        assert_eq!(o.line_of(0), 0);
        assert_eq!(o.line_of(1), 1);
        assert_eq!(o.line_of(2), 4);
        assert_eq!(o.line_of(3), 5);
    }

    #[test]
    fn solve_scroll_stays_at_top_when_everything_fits() {
        let o = tall(&[1, 1, 1]);
        let mut st: OutlineState<usize> = OutlineState::new();
        st.set_cursor(2);
        st.solve_scroll(&o, 10, 0);
        assert_eq!(st.offset(), 0);
    }

    #[test]
    fn solve_scroll_follows_cursor_down_and_back_up() {
        let o = tall(&[1; 10]);
        let mut st: OutlineState<usize> = OutlineState::new();

        st.set_cursor(9);
        st.solve_scroll(&o, 4, 0);
        assert_eq!(
            st.offset(),
            6,
            "last row sits at the bottom of the viewport"
        );

        st.set_cursor(0);
        st.solve_scroll(&o, 4, 0);
        assert_eq!(st.offset(), 0);
    }

    #[test]
    fn solve_scroll_accounts_for_tall_rows() {
        // Lines: row0 @0, row1 @1..5, row2 @5.
        let o = tall(&[1, 4, 1]);
        let mut st: OutlineState<usize> = OutlineState::new();
        st.set_cursor(1);
        st.solve_scroll(&o, 3, 0);
        // The 4-line row cannot fit in 3 lines; pin its start, don't chase the end.
        assert_eq!(st.offset(), 1);
    }

    #[test]
    fn solve_scroll_honors_scrolloff_without_fighting_a_short_viewport() {
        let o = tall(&[1; 20]);
        let mut st: OutlineState<usize> = OutlineState::new();
        st.set_cursor(10);
        st.solve_scroll(&o, 9, 3);
        // Scrolls the minimum that satisfies the constraint: lines 5..=13 are
        // visible, leaving exactly 3 rows of context below the cursor.
        assert_eq!(st.offset(), 5);
        assert_eq!(st.offset() + 9 - 1 - 10, 3, "3 lines below the cursor");

        // A scrolloff larger than the viewport is clamped, not obeyed.
        st.set_cursor(10);
        st.solve_scroll(&o, 3, 99);
        assert!(st.offset() <= 10);
    }

    #[test]
    fn viewport_reports_clipped_rows_with_negative_y() {
        let o = tall(&[2, 2, 2]);
        let mut st: OutlineState<usize> = OutlineState::new();
        st.offset = 1; // start mid-way through row 0

        let vis = st.viewport(&o, 3);
        assert_eq!(vis, vec![(0, -1), (1, 1)]);
    }

    #[test]
    fn viewport_is_empty_for_zero_height() {
        let o = tall(&[1, 1]);
        let st: OutlineState<usize> = OutlineState::new();
        assert!(st.viewport(&o, 0).is_empty());
    }

    // -- painting -----------------------------------------------------------

    fn gutter_to_string(depth: u16, fold: Option<bool>, width: u16) -> (String, u16) {
        use ratatui::layout::{Position, Rect};
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        let content_x = paint_gutter(&mut buf, 0, 0, width, depth, fold, Style::default());
        let s: String = (0..width)
            .map(|c| {
                buf.cell(Position::new(c, 0))
                    .unwrap()
                    .symbol()
                    .chars()
                    .next()
                    .unwrap_or(' ')
            })
            .collect();
        (s, content_x)
    }

    #[test]
    fn gutter_glyphs_and_content_offset_by_depth() {
        assert_eq!(gutter_to_string(0, Some(true), 8), ("▾       ".into(), 2));
        assert_eq!(gutter_to_string(0, Some(false), 8), ("▸       ".into(), 2));
        // Depth 1 indents by INDENT_COLS before the glyph.
        assert_eq!(gutter_to_string(1, Some(false), 8), ("  ▸     ".into(), 4));
        assert_eq!(gutter_to_string(2, Some(true), 8), ("    ▾   ".into(), 6));
    }

    #[test]
    fn gutter_leaves_blank_space_so_labels_align() {
        let (painted, content_x) = gutter_to_string(1, None, 8);
        assert_eq!(painted, "        ");
        // Same content column as a branch at the same depth.
        assert_eq!(content_x, gutter_to_string(1, Some(true), 8).1);
    }

    #[test]
    fn gutter_does_not_paint_past_the_edge() {
        // Depth 3 pushes the glyph to column 6, outside a 4-wide row.
        let (painted, _) = gutter_to_string(3, Some(true), 4);
        assert_eq!(painted, "    ");
    }
}
