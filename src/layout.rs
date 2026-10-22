//! Pure tiling layout: a strip of columns that scrolls horizontally without end (as in
//! niri). Rows stacked in a column share the height of the output, tiles in a row share
//! the column's width; columns sit side by side and the view slides along the strip to
//! show the focused one.
//!
//! Kept free of any Smithay types so it can be unit-tested in isolation.

pub use dune_common::protocol::{Side, Split, TileId};

/// Focus and swaps move towards a side of the focused tile.
pub use dune_common::protocol::Side as Direction;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x as f64 && y >= self.y as f64 && x < (self.x + self.w) as f64 && y < (self.y + self.h) as f64
    }

    /// The part of `self` inside `other`, if any.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let (x0, y0) = (self.x.max(other.x), self.y.max(other.y));
        let (x1, y1) = ((self.x + self.w).min(other.x + other.w), (self.y + self.h).min(other.y + other.h));
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }
}

/// Column widths are shares of the output width.
pub const DEFAULT_WIDTH: f64 = 0.5;
/// Widths `cycle_width` steps through.
pub const WIDTH_PRESETS: [f64; 4] = [1.0 / 3.0, 0.5, 2.0 / 3.0, 1.0];
const MIN_WIDTH: f64 = 0.1;
const MAX_WIDTH: f64 = 1.0;
/// Smallest share of a column's height a tile can be shrunk to.
const MIN_HEIGHT: f64 = 0.1;
/// Smallest share of a row's width a tile can be shrunk to.
const MIN_ROW_WIDTH: f64 = 0.1;
/// A tile squeezed below this share of its column by a neighbour growing becomes a tab.
const TAB_THRESHOLD: f64 = 0.15;
/// Height of a tab's bar, in pixels.
pub const TAB_HEIGHT: i32 = 24;
/// Thickness of a folded (accordion) row's bar, or a folded tile's upright one.
pub const FOLD_WIDTH: i32 = TAB_HEIGHT * 3 / 2;
/// Widest a tab of a tabbed container gets in its bar, in pixels, unless the config says.
pub const TAB_MAX_WIDTH: i32 = 220;
/// Narrowest a tab gets: past that, the bar scrolls (see `tab_strip`). The config may change it.
pub const TAB_MIN_WIDTH: i32 = 200;

/// How narrow and how wide the tabs in a bar get (`tab_min_width`, `tab_max_width`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabWidths {
    pub min: i32,
    pub max: i32,
}

impl Default for TabWidths {
    fn default() -> Self {
        TabWidths { min: TAB_MIN_WIDTH, max: TAB_MAX_WIDTH }
    }
}
/// Width of the "+" at the end of a tab bar.
pub const PLUS_WIDTH: i32 = TAB_HEIGHT;
/// Width of the marks at the ends of a scrolled tab bar, telling how many tabs are past them.
pub const ARROW_WIDTH: i32 = 40;

/// The bar of a tabbed container: where it is, its tabs in order and the one shown.
#[derive(Clone, Debug, PartialEq)]
pub struct TabBar {
    pub rect: Rect,
    pub tabs: Vec<TileId>,
    pub active: TileId,
}

impl TabBar {
    /// Where its tabs, marks and "+" are drawn.
    pub fn strip(&self, widths: TabWidths) -> TabStrip {
        tab_strip(self.rect, self.tabs.len(), self.tabs.iter().position(|t| *t == self.active).unwrap_or(0), widths)
    }
}

/// A tabbed container's bar and the room left for its shown tab. Every placement of
/// the bar goes through here (today: along the top).
pub fn split_tab_bar(rect: Rect) -> (Rect, Rect) {
    let bar = Rect::new(rect.x, rect.y, rect.w, TAB_HEIGHT.min(rect.h));
    let content = Rect::new(rect.x, rect.y + bar.h, rect.w, (rect.h - bar.h).max(1));
    (bar, content)
}

/// How a tab bar is laid out (see `tab_strip`).
#[derive(Clone, Debug, PartialEq)]
pub struct TabStrip {
    /// Each tab's place; empty for the tabs scrolled out of the bar.
    pub tabs: Vec<Rect>,
    /// The tabs in the bar: `count` of them from `first`.
    pub first: usize,
    pub count: usize,
    /// The marks at the ends, when tabs are past them: where, and how many tabs.
    pub left: Option<(Rect, usize)>,
    pub right: Option<(Rect, usize)>,
    /// The "+", right after the last tab in the bar: a new empty tab.
    pub plus: Rect,
}

impl TabStrip {
    /// The tab under (`x`, `y`): a mark at an end stands for the tab just past it.
    pub fn index_at(&self, x: f64, y: f64) -> Option<usize> {
        if self.left.is_some_and(|(r, _)| r.contains(x, y)) {
            return Some(self.first - 1);
        }
        if self.right.is_some_and(|(r, _)| r.contains(x, y)) {
            return Some(self.first + self.count);
        }
        self.tabs.iter().position(|r| r.contains(x, y))
    }
}

/// Where each of `n` tabs sits in `bar`, the one at `shown` being shown: side by side from
/// the left, as wide as `widths.max` while they fit, sharing the bar (but the "+")
/// down to `widths.min`. Past that the bar scrolls: as many tabs as fit, the shown one
/// in the middle as far as it goes, with a mark at each end telling how many are past it.
pub fn tab_strip(bar: Rect, n: usize, shown: usize, widths: TabWidths) -> TabStrip {
    let room = bar.w - PLUS_WIDTH;
    let plus_at = |x: i32| Rect::new(x.min(bar.x + bar.w - PLUS_WIDTH), bar.y, PLUS_WIDTH.min(bar.w), bar.h);
    if n <= 1 || n as i32 * widths.min <= room {
        let w = if n == 0 { 0 } else { (room / n as i32).clamp(1, widths.max) };
        let tabs = (0..n as i32).map(|i| Rect::new(bar.x + i * w, bar.y, w, bar.h)).collect();
        return TabStrip { tabs, first: 0, count: n, left: None, right: None, plus: plus_at(bar.x + n as i32 * w) };
    }
    let room = room - 2 * ARROW_WIDTH;
    let count = ((room / widths.min).max(1) as usize).min(n);
    let w = (room / count as i32).clamp(1, widths.max);
    let first = shown.min(n - 1).saturating_sub(count / 2).min(n - count);
    let start = bar.x + ARROW_WIDTH;
    let end = start + count as i32 * w;
    let tabs = (0..n)
        .map(|i| match i {
            i if i < first => Rect::new(bar.x, bar.y, 0, bar.h),
            i if i >= first + count => Rect::new(end, bar.y, 0, bar.h),
            i => Rect::new(start + (i - first) as i32 * w, bar.y, w, bar.h),
        })
        .collect();
    let hidden_right = n - first - count;
    TabStrip {
        tabs,
        first,
        count,
        left: (first > 0).then(|| (Rect::new(bar.x, bar.y, ARROW_WIDTH, bar.h), first)),
        right: (hidden_right > 0).then(|| (Rect::new(end, bar.y, ARROW_WIDTH, bar.h), hidden_right)),
        plus: plus_at(end + ARROW_WIDTH),
    }
}

/// Tiles side by side in a column, sharing the column's width.
#[derive(Debug)]
struct Row {
    tiles: Vec<TileId>,
    /// Share of the column's width of each tile; the open ones add up to 1.
    widths: Vec<f64>,
    /// Tiles folded into an upright bar by a neighbour growing (see `resize_row_width`).
    folded: Vec<bool>,
}

impl Row {
    fn new(id: TileId) -> Self {
        Row { tiles: vec![id], widths: vec![1.0], folded: vec![false] }
    }

    /// Put `id` at `index`, giving it an even share of the width of the open tiles.
    fn insert(&mut self, index: usize, id: TileId) {
        let open = self.folded.iter().filter(|f| !**f).count();
        let share = 1.0 / (open + 1) as f64;
        for (w, _) in self.widths.iter_mut().zip(&self.folded).filter(|(_, f)| !**f) {
            *w *= 1.0 - share;
        }
        self.tiles.insert(index, id);
        self.widths.insert(index, share);
        self.folded.insert(index, false);
    }

    fn remove(&mut self, index: usize) {
        self.tiles.remove(index);
        self.widths.remove(index);
        self.folded.remove(index);
        // A lone tile, or only folded ones left: they all open again.
        if self.tiles.len() < 2 || self.folded.iter().all(|f| *f) {
            self.unfold();
        }
        self.normalize_open();
    }

    /// Every tile open again, with an even share of the width.
    fn unfold(&mut self) {
        let even = 1.0 / self.tiles.len().max(1) as f64;
        self.widths.iter_mut().for_each(|w| *w = even);
        self.folded.iter_mut().for_each(|f| *f = false);
    }

    /// The open tiles' widths scaled to add up to 1.
    fn normalize_open(&mut self) {
        let total: f64 = self.widths.iter().zip(&self.folded).filter(|(_, f)| !**f).map(|(w, _)| w).sum();
        if total > 0.0 {
            for (w, _) in self.widths.iter_mut().zip(&self.folded).filter(|(_, f)| !**f) {
                *w /= total;
            }
        }
    }
}

#[derive(Debug)]
struct Column {
    rows: Vec<Row>,
    /// Share of the column's height of each row; they add up to 1.
    heights: Vec<f64>,
    /// Rows folded into tabs: a bar instead of their windows.
    tabs: Vec<bool>,
    width: f64,
    /// Width to go back to when leaving full width.
    restore_width: Option<f64>,
    /// Folded into an upright bar, narrowed too much (see `resize_width`); `width` is
    /// what it opens to.
    folded: bool,
    /// The column's tabs in order, when it has two or more: each one rows of its own. The
    /// shown one is `None`: its rows are the column's.
    panes: Vec<Option<Pane>>,
}

/// A tab of a column that is not shown: its rows, kept as `Column` keeps the shown ones.
#[derive(Debug)]
struct Pane {
    rows: Vec<Row>,
    heights: Vec<f64>,
    tabs: Vec<bool>,
}

impl Pane {
    fn tiles(&self) -> impl Iterator<Item = TileId> + '_ {
        self.rows.iter().flat_map(|r| r.tiles.iter().copied())
    }
}

impl Column {
    fn new(id: TileId, width: f64) -> Self {
        Column {
            rows: vec![Row::new(id)],
            heights: vec![1.0],
            tabs: vec![false],
            width,
            restore_width: None,
            folded: false,
            panes: Vec::new(),
        }
    }

    fn tiles(&self) -> impl Iterator<Item = TileId> + '_ {
        self.rows.iter().flat_map(|r| r.tiles.iter().copied())
    }

    /// Row and place in the row of a tile in the shown rows.
    fn slot(&self, id: TileId) -> Option<(usize, usize)> {
        self.rows.iter().enumerate().find_map(|(r, row)| row.tiles.iter().position(|t| *t == id).map(|k| (r, k)))
    }

    /// Rows, heights and folds of each tab (only the shown rows when not tabbed).
    fn views(&self) -> Vec<(&[Row], &[f64], &[bool])> {
        if self.panes.is_empty() {
            return vec![(&self.rows, &self.heights, &self.tabs)];
        }
        self.panes
            .iter()
            .map(|p| match p {
                Some(p) => (&p.rows[..], &p.heights[..], &p.tabs[..]),
                None => (&self.rows[..], &self.heights[..], &self.tabs[..]),
            })
            .collect()
    }

    /// Every tile of the column, tab by tab.
    fn all_tiles(&self) -> Vec<TileId> {
        self.views().into_iter().flat_map(|(rows, _, _)| rows.iter().flat_map(|r| r.tiles.iter().copied())).collect()
    }

    /// The first tile of each tab, which names it.
    fn leads(&self) -> Vec<TileId> {
        self.views().into_iter().map(|(rows, _, _)| rows[0].tiles[0]).collect()
    }

    /// Which tab `id` is in; None when the column has no tabs (or not `id`).
    fn pane_of(&self, id: TileId) -> Option<usize> {
        self.panes.iter().position(|p| match p {
            Some(p) => p.tiles().any(|t| t == id),
            None => self.tiles().any(|t| t == id),
        })
    }

    /// Index of the shown tab.
    fn shown(&self) -> usize {
        self.panes.iter().position(Option::is_none).unwrap_or(0)
    }

    /// Show tab `p`: the shown rows go back into their tab, `p`'s become the column's.
    fn show(&mut self, p: usize) {
        let a = self.shown();
        let Some(pane) = self.panes.get_mut(p).and_then(Option::take) else { return };
        let rows = std::mem::replace(&mut self.rows, pane.rows);
        let heights = std::mem::replace(&mut self.heights, pane.heights);
        let tabs = std::mem::replace(&mut self.tabs, pane.tabs);
        self.panes[a] = Some(Pane { rows, heights, tabs });
        self.panes[p] = None;
    }

    /// A new tab with `id` alone, after the shown one, shown.
    fn add_pane(&mut self, id: TileId) {
        if self.panes.is_empty() {
            self.panes.push(None);
        }
        let at = self.shown() + 1;
        let new = Column::new(id, 0.0);
        self.panes.insert(at, Some(Pane { rows: new.rows, heights: new.heights, tabs: new.tabs }));
        self.show(at);
    }

    /// Close the shown tab (emptied): the next one shows, else the one before. A last
    /// tab is the column's plain rows.
    fn drop_shown(&mut self) {
        let a = self.shown();
        self.show(if a + 1 < self.panes.len() { a + 1 } else { a - 1 });
        self.panes.remove(a);
        if self.panes.len() < 2 {
            self.panes.clear();
        }
    }

    /// Put a row with `id` at `index`, giving it an even share of the height.
    fn insert(&mut self, index: usize, id: TileId) {
        let share = 1.0 / (self.rows.len() + 1) as f64;
        for h in &mut self.heights {
            *h *= 1.0 - share;
        }
        self.rows.insert(index, Row::new(id));
        self.heights.insert(index, share);
        self.tabs.insert(index, false);
    }

    /// Take tile `k` out of row `r`; an emptied row goes away.
    fn remove_tile(&mut self, r: usize, k: usize) {
        if self.rows[r].tiles.len() > 1 {
            self.rows[r].remove(k);
        } else {
            self.remove(r);
        }
    }

    fn remove(&mut self, index: usize) {
        self.rows.remove(index);
        let tab = self.tabs.remove(index);
        // An open row leaving gives its place to the tab just above it (the tile it
        // was stacked on, see `Layout::stack_on`).
        if !tab && index > 0 && self.tabs[index - 1] {
            self.tabs[index - 1] = false;
            self.heights[index - 1] = self.heights.remove(index);
            return;
        }
        let h = self.heights.remove(index);
        if h < 1.0 {
            for rest in &mut self.heights {
                *rest /= 1.0 - h;
            }
        }
        // A column of tabs only: the first one opens.
        if !self.tabs.is_empty() && self.tabs.iter().all(|t| *t) {
            self.tabs[0] = false;
        }
    }

    /// Share of the height among the rows that are not tabs, normalized to add up to 1.
    fn normalize_open(&mut self) {
        let total: f64 = self.heights.iter().zip(&self.tabs).filter(|(_, t)| !**t).map(|(h, _)| h).sum();
        if total > 0.0 {
            for (h, _) in self.heights.iter_mut().zip(&self.tabs).filter(|(_, t)| !**t) {
                *h /= total;
            }
        }
    }
}

/// A tile's place in the strip, told by a neighbour that stays (see `Layout::home`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Home {
    /// Sharing a row: on `side` of `by`, with its share of the row's width.
    Row { by: TileId, side: Side, width: f64 },
    /// The open row stacked on the tab of `on`.
    Stacked { on: TileId },
    /// A tab of the container `with` is in, at `index` among its tabs.
    Tab { with: TileId, index: usize },
    /// A row of its own in a column: on `side` of `by`, with its share of the height.
    Column { by: TileId, side: Side, height: f64 },
    /// A column of its own, on a side of a neighbouring column (none: alone) and its width.
    Strip { by: Option<(TileId, Side)>, width: f64 },
}

#[derive(Debug, Default)]
pub struct Layout {
    columns: Vec<Column>,
    /// How far the view has slid along the strip, in pixels.
    view: i32,
    /// A tile whose column the view keeps centered, until focus goes to another column here.
    centered: Option<TileId>,
    /// The view was slid by hand (the pointer pushing a screen edge) with this tile focused
    /// (or none): it stays where it was slid until focus moves.
    slid: Option<Option<TileId>>,
    /// Bars of the tabbed columns, as last laid out by `geometries`.
    bars: Vec<TabBar>,
    /// Folded columns with tabs, as last laid out: the tiles of their bar, with the tab count.
    fold_counts: Vec<(TileId, usize)>,
}

impl Layout {
    pub fn contains(&self, id: TileId) -> bool {
        self.find(id).is_some()
    }

    /// Column, row and place in the row of a tile; a tile of a hidden tab is where its
    /// column's first tile is.
    fn find(&self, id: TileId) -> Option<(usize, usize, usize)> {
        self.find_slot(id).or_else(|| self.columns.iter().position(|col| col.pane_of(id).is_some()).map(|c| (c, 0, 0)))
    }

    /// Column, row and place in the row of a tile that sits in shown rows.
    fn find_slot(&self, id: TileId) -> Option<(usize, usize, usize)> {
        self.columns.iter().enumerate().find_map(|(c, col)| col.slot(id).map(|(r, k)| (c, r, k)))
    }

    fn column_of(&mut self, id: TileId) -> Option<&mut Column> {
        let (c, _, _) = self.find(id)?;
        Some(&mut self.columns[c])
    }

    /// Ids of all tiles, column by column from the left, tab by tab, top to bottom, left
    /// to right.
    pub fn tiles(&self) -> Vec<TileId> {
        self.columns.iter().flat_map(Column::all_tiles).collect()
    }

    // ---- tabbed columns ----

    /// The tabs of the column `id` is in, each named by its first tile; None when it has none.
    pub fn group(&self, id: TileId) -> Option<Vec<TileId>> {
        let (c, _, _) = self.find(id)?;
        let col = &self.columns[c];
        (!col.panes.is_empty()).then(|| col.leads())
    }

    /// Add `new` as a tab of `target`'s column, after `target`'s tab, and show it.
    pub fn add_tab(&mut self, new: TileId, target: TileId) -> bool {
        let Some((c, _, _)) = self.find(target) else { return false };
        let col = &mut self.columns[c];
        if let Some(p) = col.pane_of(target) {
            col.show(p);
        }
        col.add_pane(new);
        true
    }

    /// Give the column `id` is alone in a bar with its one tab. False when the column has
    /// other tiles or tabs already.
    pub fn lone_tab(&mut self, id: TileId) -> bool {
        let Some((c, _, _)) = self.find_slot(id) else { return false };
        let col = &mut self.columns[c];
        if !col.panes.is_empty() || col.tiles().count() > 1 {
            return false;
        }
        col.panes.push(None);
        true
    }

    /// Take `id` out of the rows it shares in its column into a new tab of the column,
    /// after the shown one, shown. False when it is alone in its tab (or not shown here).
    pub fn tile_to_tab(&mut self, id: TileId) -> bool {
        let Some((c, r, k)) = self.find_slot(id) else { return false };
        let col = &mut self.columns[c];
        if col.tiles().count() < 2 {
            return false;
        }
        col.remove_tile(r, k);
        col.add_pane(id);
        true
    }

    /// Take the bar off the column `id` is the one tab of. False when the column has no
    /// bar or other tabs.
    pub fn untab(&mut self, id: TileId) -> bool {
        let Some(col) = self.column_of(id) else { return false };
        if col.panes.len() != 1 {
            return false;
        }
        col.panes.clear();
        true
    }

    /// Put `id`, alone in its tab, back where `home` says among the tiles of another tab of
    /// its column (that tab shown). False when the neighbour `home` is told by is not in a
    /// tab of `id`'s column any more (and nothing was done).
    pub fn tab_back(&mut self, id: TileId, home: Home) -> bool {
        let by = match home {
            Home::Row { by, .. } | Home::Column { by, .. } | Home::Stacked { on: by } => by,
            Home::Tab { .. } | Home::Strip { .. } => return false,
        };
        let Some((c, _, _)) = self.find(id) else { return false };
        let col = &self.columns[c];
        if by == id || col.pane_of(by).is_none() || col.pane_of(by) == col.pane_of(id) {
            return false;
        }
        self.remove(id);
        self.activate_tab(by);
        self.restore(id, home)
    }

    /// Show the tab `id` is in. False when it is not in a hidden tab here.
    pub fn activate_tab(&mut self, id: TileId) -> bool {
        if self.find_slot(id).is_some() {
            return false;
        }
        let Some(col) = self.columns.iter_mut().find(|col| col.pane_of(id).is_some()) else { return false };
        let p = col.pane_of(id).unwrap();
        col.show(p);
        true
    }

    /// Put the tab `id` is in at place `index` of its column's bar, the rest keeping their
    /// order. False when nothing moves.
    pub fn move_tab(&mut self, id: TileId, index: usize) -> bool {
        let Some((c, _, _)) = self.find(id) else { return false };
        let col = &mut self.columns[c];
        let Some(p) = col.pane_of(id) else { return false };
        if index == p || index >= col.panes.len() {
            return false;
        }
        let pane = col.panes.remove(p);
        col.panes.insert(index, pane);
        true
    }

    /// The shown tab of `id`'s column (its first tile), if the column has tabs.
    pub fn active_tab(&self, id: TileId) -> Option<TileId> {
        let (c, _, _) = self.find(id)?;
        let col = &self.columns[c];
        (!col.panes.is_empty()).then(|| col.rows[0].tiles[0])
    }

    /// Show the tab `delta` places after the shown one of `id`'s column (going round);
    /// the first tile of the tab now shown.
    pub fn cycle_tab(&mut self, id: TileId, delta: i32) -> Option<TileId> {
        let (c, _, _) = self.find(id)?;
        let col = &mut self.columns[c];
        if col.panes.is_empty() {
            return None;
        }
        let next = (col.shown() as i32 + delta).rem_euclid(col.panes.len() as i32) as usize;
        col.show(next);
        Some(col.rows[0].tiles[0])
    }

    /// The tab shown in place of `id`'s when `id`, alone in it, leaves its column: the
    /// next one, else the one before (named by its first tile).
    pub fn tab_sibling(&self, id: TileId) -> Option<TileId> {
        let (c, _, _) = self.find(id)?;
        let col = &self.columns[c];
        let p = col.pane_of(id)?;
        if col.views()[p].0.iter().map(|r| r.tiles.len()).sum::<usize>() > 1 {
            return None;
        }
        let leads = col.leads();
        leads.get(p + 1).or_else(|| leads.get(p.wrapping_sub(1))).copied()
    }

    /// The tabbed columns' bars, as last laid out by `geometries`.
    pub fn tab_bars(&self) -> &[TabBar] {
        &self.bars
    }

    /// The tiles of the folded tabbed columns' bars, with the column's tab count, as last
    /// laid out: their bar shows the count instead of a title.
    pub fn fold_counts(&self) -> &[(TileId, usize)] {
        &self.fold_counts
    }

    /// Insert `new` after `target`: in a new column to its right (`Vertical`) or below it
    /// in its column (`Horizontal`). Without a (known) target it goes at the end.
    pub fn insert(&mut self, new: TileId, target: Option<TileId>, split: Split) {
        match split {
            Split::Vertical => self.insert_column(new, target, Side::Right),
            Split::Horizontal => self.insert_on(new, target, Side::Down),
        }
    }

    /// Insert `new` in a column of its own on a side of `target`'s: before it (left/up)
    /// or after it (right/down). Without a known target, at that end of the strip.
    pub fn insert_column(&mut self, new: TileId, target: Option<TileId>, side: Side) {
        if self.columns.is_empty() {
            // A lone tile fills the output; the second column splits it in halves.
            self.columns.push(Column::new(new, MAX_WIDTH));
            return;
        }
        if self.columns.len() == 1 && self.columns[0].width == MAX_WIDTH {
            self.columns[0].width = DEFAULT_WIDTH;
        }
        let c = match target.and_then(|t| self.find(t)) {
            Some((c, _, _)) if side.before() => c,
            Some((c, _, _)) => c + 1,
            None if side.before() => 0,
            None => self.columns.len(),
        };
        self.columns.insert(c, Column::new(new, DEFAULT_WIDTH));
    }

    /// Insert `new` on a side of `target`, inside its column: up/down in a row of its own,
    /// left/right sharing the target's row. Without a (known) target, left/right open a
    /// column at the end and up/down go below the last tile.
    pub fn insert_on(&mut self, new: TileId, target: Option<TileId>, side: Side) {
        let found = target.and_then(|t| self.find(t));
        if self.columns.is_empty() || (found.is_none() && matches!(side, Side::Left | Side::Right)) {
            return self.insert_column(new, None, side);
        }
        let (c, r, k) = found.unwrap_or_else(|| {
            let c = self.columns.len() - 1;
            (c, self.columns[c].rows.len() - 1, 0)
        });
        let col = &mut self.columns[c];
        match side {
            Side::Up | Side::Down => col.insert(if side.before() { r } else { r + 1 }, new),
            Side::Left | Side::Right => {
                // A folded row opens: the new tile must be seen.
                if col.tabs[r] {
                    let id = col.rows[r].tiles[0];
                    col.rows[r].insert(if side.before() { k } else { k + 1 }, new);
                    self.open_tab(id);
                } else {
                    col.rows[r].insert(if side.before() { k } else { k + 1 }, new);
                }
            }
        }
    }

    /// Stack `new` on `target`: it takes the place of the target's row in its column and
    /// that row folds into a tab just above it (focusing the tab opens it again).
    pub fn stack_on(&mut self, new: TileId, target: TileId) -> bool {
        let Some((c, r, _)) = self.find(target) else { return false };
        let col = &mut self.columns[c];
        col.rows.insert(r + 1, Row::new(new));
        col.heights.insert(r + 1, col.heights[r]);
        col.tabs.insert(r + 1, false);
        col.tabs[r] = true;
        true
    }

    /// Where `id` sits, told by its neighbours, so it can be put back there after leaving
    /// the strip (see `Layout::restore`).
    pub fn home(&self, id: TileId) -> Option<Home> {
        let (c, r, k) = self.find(id)?;
        let col = &self.columns[c];
        let row = &col.rows[r];
        // A tab of its own, or in a hidden tab: back as a tab at that place.
        if let Some(p) = col.pane_of(id).filter(|p| col.panes.len() > 1 && (col.panes[*p].is_some() || col.tiles().count() == 1)) {
            let with = self.tab_sibling(id).unwrap_or(col.rows[0].tiles[0]);
            return Some(Home::Tab { with, index: p });
        }
        Some(if row.tiles.len() > 1 {
            let (by, side) = if k > 0 { (row.tiles[k - 1], Side::Right) } else { (row.tiles[1], Side::Left) };
            Home::Row { by, side, width: row.widths[k] }
        } else if r > 0 && col.tabs[r - 1] && !col.tabs[r] {
            Home::Stacked { on: col.rows[r - 1].tiles[0] }
        } else if col.rows.len() > 1 {
            let (by, side) = if r > 0 { (col.rows[r - 1].tiles[0], Side::Down) } else { (col.rows[1].tiles[0], Side::Up) };
            Home::Column { by, side, height: col.heights[r] }
        } else {
            let by = match c {
                0 => self.columns.get(1).map(|n| (n.rows[0].tiles[0], Side::Left)),
                _ => Some((self.columns[c - 1].rows[0].tiles[0], Side::Right)),
            };
            Home::Strip { by, width: col.width }
        })
    }

    /// Put `id` back where `home` says, with the share of space it had. False when the
    /// neighbour it was told by is not here any more (and nothing was done).
    pub fn restore(&mut self, id: TileId, home: Home) -> bool {
        let here = |t: TileId| self.contains(t);
        match home {
            Home::Row { by, side, width } if here(by) => {
                self.insert_on(id, Some(by), side);
                let (c, r, k) = self.find(id).unwrap();
                let row = &mut self.columns[c].rows[r];
                let width = width.clamp(MIN_ROW_WIDTH, 1.0 - MIN_ROW_WIDTH * (row.tiles.len() - 1) as f64);
                let open = |i: &usize| *i != k && !row.folded[*i];
                let rest: f64 = (0..row.tiles.len()).filter(open).map(|i| row.widths[i]).sum();
                for i in 0..row.tiles.len() {
                    if i == k {
                        row.widths[i] = width;
                    } else if open(&i) {
                        row.widths[i] = row.widths[i] / rest * (1.0 - width);
                    }
                }
            }
            Home::Stacked { on } if here(on) => {
                self.stack_on(id, on);
            }
            Home::Tab { with, index } if here(with) => {
                self.add_tab(id, with);
                let col = self.column_of(id).unwrap();
                let pane = col.panes.remove(col.pane_of(id).unwrap());
                col.panes.insert(index.min(col.panes.len()), pane);
            }
            Home::Column { by, side, height } if here(by) => {
                self.insert_on(id, Some(by), side);
                let (c, r, _) = self.find(id).unwrap();
                let col = &mut self.columns[c];
                let open = col.tabs.iter().filter(|t| !**t).count();
                let height = height.clamp(MIN_HEIGHT, 1.0 - MIN_HEIGHT * (open - 1) as f64);
                let rest: f64 = col.heights.iter().zip(&col.tabs).enumerate().filter(|(i, (_, t))| *i != r && !**t).map(|(_, (h, _))| h).sum();
                for (i, (h, t)) in col.heights.iter_mut().zip(&col.tabs).enumerate() {
                    if i == r {
                        *h = height;
                    } else if !*t && rest > 0.0 {
                        *h = *h / rest * (1.0 - height);
                    }
                }
            }
            Home::Strip { by, width } if by.is_none_or(|(t, _)| here(t)) => {
                let (target, side) = by.map_or((None, Side::Right), |(t, s)| (Some(t), s));
                self.insert_column(id, target, side);
                if self.columns.len() > 1 {
                    self.column_of(id).unwrap().width = width;
                }
            }
            _ => return false,
        }
        true
    }

    /// Remove a tile; an emptied tab closes, an emptied column goes away and the strip
    /// closes the gap.
    pub fn remove(&mut self, id: TileId) -> bool {
        let Some((c, _, _)) = self.find(id) else { return false };
        let col = &mut self.columns[c];
        // A tile of a hidden tab is taken out with its tab shown for a moment.
        let lead = col.rows[0].tiles[0];
        if let Some(p) = col.pane_of(id) {
            col.show(p);
        }
        let (r, k) = col.slot(id).unwrap();
        col.remove_tile(r, k);
        if col.rows.is_empty() {
            if col.panes.len() < 2 {
                self.columns.remove(c);
                // Only folded columns left: they all open again.
                if self.columns.iter().all(|c| c.folded) {
                    self.columns.iter_mut().for_each(|c| c.folded = false);
                }
                return true;
            }
            col.drop_shown();
        }
        if let Some(p) = col.pane_of(lead) {
            col.show(p);
        }
        true
    }

    /// Exchange the positions of two tiles.
    pub fn swap(&mut self, x: TileId, y: TileId) {
        let rows = self.columns.iter_mut().flat_map(|c| c.rows.iter_mut().chain(c.panes.iter_mut().flatten().flat_map(|p| p.rows.iter_mut())));
        for t in rows.flat_map(|r| r.tiles.iter_mut()) {
            if *t == x {
                *t = y;
            } else if *t == y {
                *t = x;
            }
        }
    }

    /// Exchange the positions of two tiles, each keeping whether it is folded
    /// (`x_folded`, `y_folded`: told by the caller, as they may be on another monitor).
    pub fn swap_keeping_folds(&mut self, x: TileId, y: TileId, x_folded: bool, y_folded: bool) {
        self.swap(x, y);
        let wanted = [(x, x_folded), (y, y_folded)];
        // Fold first: the one opening afterwards then has an open neighbour to trade with.
        for (t, folded) in wanted {
            if folded && self.contains(t) && !self.is_tab(t) {
                self.toggle_fold(t);
            }
        }
        for (t, folded) in wanted {
            if !folded && self.is_tab(t) {
                self.open_tab(t);
            }
        }
    }

    /// Make the tile's column wider (positive `delta`) or narrower, as a share of the output.
    /// A column narrowed below `TAB_THRESHOLD` folds into an upright bar, keeping the width
    /// it opens to; the last open one does not. A folded one does not resize.
    pub fn resize_width(&mut self, id: TileId, delta: f64) {
        let Some((c, _, _)) = self.find(id) else { return };
        let others_open = self.columns.iter().enumerate().any(|(i, col)| i != c && !col.folded);
        let col = &mut self.columns[c];
        if col.folded {
            return;
        }
        let new = (col.width + delta).clamp(MIN_WIDTH, MAX_WIDTH);
        if delta < 0.0 && new < TAB_THRESHOLD - 1e-9 && others_open {
            col.folded = true;
            col.width = col.width.max(TAB_THRESHOLD);
        } else {
            col.width = new;
        }
        col.restore_width = None;
    }

    /// Make the tile's row taller (positive `delta`) or shorter, as a share of its column;
    /// the other open rows of the column give or take the difference. A row squeezed below
    /// `TAB_THRESHOLD` (by another growing, or shrunk itself) folds into a tab; the last open
    /// one does not. Tabs open again by a click or Enter on them (`open_tab`).
    pub fn resize_height(&mut self, id: TileId, delta: f64) {
        let Some((c, r, _)) = self.find(id) else { return };
        let col = &mut self.columns[c];
        if col.tabs[r] || col.tabs.iter().filter(|t| !**t).count() < 2 {
            return;
        }
        col.normalize_open();
        let old = col.heights[r];
        let new = (old + delta).clamp(MIN_HEIGHT, 1.0);
        if delta < 0.0 && new < TAB_THRESHOLD - 1e-9 {
            col.tabs[r] = true;
            return col.normalize_open();
        }
        let scale = if old < 1.0 { (1.0 - new) / (1.0 - old) } else { 0.0 };
        for (i, (h, tab)) in col.heights.iter_mut().zip(col.tabs.iter_mut()).enumerate() {
            if *tab {
                continue;
            }
            if i == r {
                *h = new;
            } else {
                *h *= scale;
                // Rounding noise must not fold a row that is exactly at the threshold.
                *tab = *h < TAB_THRESHOLD - 1e-9;
            }
        }
        col.normalize_open();
    }

    /// Whether the tile has neighbours side by side in its row.
    pub fn shares_row(&self, id: TileId) -> bool {
        self.find(id).is_some_and(|(c, r, _)| self.columns[c].rows[r].tiles.len() > 1)
    }

    /// Make the tile wider (positive `delta`) or narrower inside its row, as a share of the
    /// column's width; the other open tiles of the row give or take the difference. Tiles
    /// squeezed below `TAB_THRESHOLD` (by another growing, or shrunk themselves) fold into
    /// upright bars, as `resize_height` does with rows.
    pub fn resize_row_width(&mut self, id: TileId, delta: f64) {
        let Some((c, r, k)) = self.find(id) else { return };
        let row = &mut self.columns[c].rows[r];
        if row.folded[k] || row.folded.iter().filter(|f| !**f).count() < 2 {
            return;
        }
        row.normalize_open();
        let old = row.widths[k];
        let new = (old + delta).clamp(MIN_ROW_WIDTH, 1.0);
        if delta < 0.0 && new < TAB_THRESHOLD - 1e-9 {
            row.folded[k] = true;
            return row.normalize_open();
        }
        let scale = if old < 1.0 { (1.0 - new) / (1.0 - old) } else { 0.0 };
        for (i, (w, folded)) in row.widths.iter_mut().zip(row.folded.iter_mut()).enumerate() {
            if *folded {
                continue;
            }
            if i == k {
                *w = new;
            } else {
                *w *= scale;
                // Rounding noise must not fold a tile that is exactly at the threshold.
                *folded = *w < TAB_THRESHOLD - 1e-9;
            }
        }
        row.normalize_open();
    }

    /// Fold the tile into the accordion by hand, at the smallest level with open
    /// neighbours: inside its row, else its row in the column, else its column in the
    /// strip. The last open one folds at the smallest level with any neighbours (all
    /// folded, the space is left empty); a lone tile folds its column. A folded one
    /// opens instead (`open_tab`).
    pub fn toggle_fold(&mut self, id: TileId) {
        if self.is_tab(id) {
            return self.open_tab(id);
        }
        let Some((c, r, k)) = self.find(id) else { return };
        let others_open = self.columns.iter().enumerate().any(|(i, col)| i != c && !col.folded);
        let col = &mut self.columns[c];
        let row = &mut col.rows[r];
        if row.folded.iter().filter(|f| !**f).count() > 1 {
            row.folded[k] = true;
            row.normalize_open();
        } else if col.tabs.iter().filter(|t| !**t).count() > 1 {
            col.tabs[r] = true;
            col.normalize_open();
        } else if others_open {
            col.folded = true;
        } else if row.tiles.len() > 1 {
            row.folded[k] = true;
        } else if col.rows.len() > 1 {
            col.tabs[r] = true;
        } else {
            col.folded = true;
        }
    }

    pub fn is_tab(&self, id: TileId) -> bool {
        self.find(id).is_some_and(|(c, r, k)| {
            let col = &self.columns[c];
            col.folded || col.tabs[r] || col.rows[r].folded[k]
        })
    }

    /// Open a tab: its row takes the place of the largest open row of its column, which
    /// becomes a tab in turn. A tile folded inside its row does the same with the widest
    /// open tile of the row. A folded column opens to its width, at least that of a new
    /// column (`DEFAULT_WIDTH`): it folded narrowed to almost nothing.
    pub fn open_tab(&mut self, id: TileId) {
        let Some((c, r, k)) = self.find(id) else { return };
        let col = &mut self.columns[c];
        if col.folded {
            col.folded = false;
            col.width = col.width.max(DEFAULT_WIDTH);
            return;
        }
        if col.rows[r].folded[k] {
            let row = &mut col.rows[r];
            let widest = (0..row.tiles.len()).filter(|i| !row.folded[*i]).max_by(|a, b| row.widths[*a].total_cmp(&row.widths[*b]));
            match widest {
                Some(wide) => {
                    row.widths.swap(wide, k);
                    row.folded[wide] = true;
                }
                None => row.widths[k] = 1.0,
            }
            row.folded[k] = false;
        }
        if !col.tabs[r] {
            return;
        }
        let largest = (0..col.rows.len())
            .filter(|i| !col.tabs[*i])
            .max_by(|a, b| col.heights[*a].total_cmp(&col.heights[*b]));
        match largest {
            Some(big) => {
                col.heights.swap(big, r);
                col.tabs[big] = true;
            }
            None => col.heights[r] = 1.0,
        }
        col.tabs[r] = false;
    }

    /// Step the tile's column through `WIDTH_PRESETS`.
    pub fn cycle_width(&mut self, id: TileId) {
        if let Some(col) = self.column_of(id) {
            col.width = WIDTH_PRESETS
                .into_iter()
                .find(|w| *w > col.width + 0.01)
                .unwrap_or(WIDTH_PRESETS[0]);
            col.restore_width = None;
        }
    }

    /// Center the tile's column on the output, or stop centering it if it already is.
    pub fn toggle_center(&mut self, id: TileId) {
        let column = |t: TileId| self.find(t).map(|(c, _, _)| c);
        let already = self.centered.is_some_and(|c| column(c) == column(id));
        self.centered = if already || !self.contains(id) { None } else { Some(id) };
        // A slide by hand gives way: the view goes where centering (or focus) says.
        self.slid = None;
    }

    /// Whether focusing `id` would end the centering of `from`'s column: `id` in another
    /// column of this strip.
    pub fn ends_centering(&self, from: TileId, id: TileId) -> bool {
        let column = |t: TileId| self.find(t).map(|(c, _, _)| c);
        let centered = self.centered.and_then(column);
        centered.is_some() && centered == column(from) && column(id).is_some_and(|c| Some(c) != centered)
    }

    /// Toggle the tile's column between the full output width and its previous width.
    pub fn toggle_full_width(&mut self, id: TileId) {
        if let Some(col) = self.column_of(id) {
            match col.restore_width.take() {
                Some(w) => col.width = w,
                None if col.width < MAX_WIDTH => {
                    col.restore_width = Some(col.width);
                    col.width = MAX_WIDTH;
                }
                None => col.width = DEFAULT_WIDTH,
            }
        }
    }

    /// Slide the view `dx` pixels along the strip (right: positive), focus being on `focus`;
    /// it stays there, but at the ends of the strip, until focus moves (see `geometries`).
    pub fn slide_view(&mut self, dx: i32, focus: Option<TileId>) {
        self.view += dx;
        self.slid = Some(focus);
    }

    /// Let the view follow focus again after a slide by hand.
    pub fn stop_sliding(&mut self) {
        self.slid = None;
    }

    /// Lay the strip out in `area`, leaving `gap` pixels around and between columns and
    /// `sep` pixels between the tiles inside a column (room for one shared border), and
    /// slide the view so that `focus` (when it is here) is shown whole. Tiles off the
    /// view get rectangles outside `area`; tabs get the rectangle of their bar. Invisible
    /// tiles (e.g. a terminal that has not mapped its window yet) take no space.
    pub fn geometries(&mut self, area: Rect, gap: i32, sep: i32, has_window: &dyn Fn(TileId) -> bool, focus: Option<TileId>) -> Vec<(TileId, Rect)> {
        let visible = has_window;
        let inner_h = area.h - 2 * gap;
        // Columns at strip coordinates: (x, width, column index).
        let mut placed = Vec::new();
        let mut x = gap;
        for (c, col) in self.columns.iter().enumerate() {
            if !col.all_tiles().into_iter().any(visible) {
                continue;
            }
            let w = if col.folded { FOLD_WIDTH } else { (((area.w - gap) as f64 * col.width).round() as i32 - gap).max(1) };
            placed.push((x, w, c));
            x += w + gap;
        }
        let strip = x;

        // Centering lasts until focus moves to another column of this strip.
        let column = |t: TileId| self.find(t).map(|(c, _, _)| c);
        let focus_col = focus.and_then(column);
        let centered_col = self.centered.and_then(column);
        if centered_col.is_none() || focus_col.is_some_and(|f| Some(f) != centered_col) {
            self.centered = None;
        }
        let centered = self.centered.and(centered_col).and_then(|cc| placed.iter().find(|(_, _, c)| *c == cc));
        // Slid by hand: focus has to move for the view to follow it again. With the whole
        // strip on the output there is nothing to slide to, so the push changes nothing.
        if self.slid.is_some_and(|f| f != focus) || strip <= area.w {
            self.slid = None;
        }

        if self.slid.is_some() {
            self.view = self.view.min(strip - area.w).max(0);
        } else if let Some((x, w, _)) = centered {
            // Centered even when that leaves empty space at an end.
            self.view = x + w / 2 - area.w / 2;
        } else {
            if let Some((x, w, _)) = focus_col.and_then(|fc| placed.iter().find(|(_, _, c)| *c == fc)) {
                if x - gap < self.view {
                    self.view = x - gap;
                } else if x + w + gap > self.view + area.w {
                    self.view = x + w + gap - area.w;
                }
            }
            // No empty space at either end while there is something to show.
            self.view = self.view.min(strip - area.w).max(0);
        }

        let mut out = Vec::new();
        let mut bars = Vec::new();
        let mut fold_counts = Vec::new();
        for (x, w, c) in placed {
            let col = &self.columns[c];
            // The shown tab, or while it has no window yet (just launched) one that has.
            let views = col.views();
            let has_any = |(rows, _, _): &(&[Row], &[f64], &[bool])| rows.iter().flat_map(|r| &r.tiles).any(|t| visible(*t));
            let p = Some(col.shown()).filter(|p| has_any(&views[*p])).or_else(|| views.iter().position(has_any)).unwrap();
            let (rows, heights, folded) = views[p];
            let mut content = Rect::new(area.x + x - self.view, area.y + gap, w, inner_h);
            let tabs: Vec<TileId> = views.iter().filter(|v| has_any(v)).map(|(rows, _, _)| rows[0].tiles[0]).collect();
            if col.folded {
                // One upright bar: the shown tab's tiles share it top to bottom, showing
                // the tab count (two or more) instead of their titles.
                let tiles: Vec<TileId> = rows.iter().flat_map(|r| r.tiles.iter().copied()).filter(|t| visible(*t)).collect();
                if tabs.len() > 1 {
                    fold_counts.extend(tiles.iter().map(|t| (*t, tabs.len())));
                }
                let n = tiles.len() as i32;
                let each = (content.h - sep * (n - 1)) / n.max(1);
                for (i, t) in tiles.into_iter().enumerate() {
                    let y = content.y + i as i32 * (each + sep);
                    let h = if i as i32 == n - 1 { content.y + content.h - y } else { each };
                    out.push((t, Rect::new(content.x, y, w, h)));
                }
                continue;
            }
            if tabs.len() > 1 || col.panes.len() == 1 {
                let (bar, rest) = split_tab_bar(content);
                bars.push(TabBar { rect: bar, tabs, active: rows[0].tiles[0] });
                content = rest;
            }
            let shown: Vec<(usize, f64, bool)> = (0..rows.len())
                .filter(|r| rows[*r].tiles.iter().any(|t| visible(*t)))
                .map(|r| (r, heights[r], folded[r]))
                .collect();
            let total: f64 = shown.iter().filter(|(_, _, tab)| !tab).map(|(_, h, _)| h).sum();
            let tab_bars = shown.iter().filter(|(_, _, tab)| *tab).count() as i32;
            let space = content.h - sep * (shown.len() as i32 - 1) - tab_bars * FOLD_WIDTH;
            let last_open = shown.iter().rposition(|(_, _, tab)| !tab);
            let mut y = content.y;
            for (i, (r, h, tab)) in shown.iter().enumerate() {
                let th = if *tab {
                    FOLD_WIDTH
                } else if Some(i) == last_open {
                    // The last open row takes what rounding left, minus the tabs below it.
                    let below = shown[i + 1..].len() as i32;
                    content.y + content.h - y - below * (FOLD_WIDTH + sep)
                } else {
                    (space as f64 * h / total).round() as i32
                };
                out.extend(row_geometries(&rows[*r], Rect::new(content.x, y, w, th), sep, *tab, visible));
                y += th + sep;
            }
        }
        self.bars = bars;
        self.fold_counts = fold_counts;
        out
    }
}

/// How many of the tiles laid out at `rects` reach past the left and past the right edge
/// of `area` (the strip scrolled that way would show them).
pub fn past_edges(area: Rect, rects: &[(TileId, Rect)]) -> (usize, usize) {
    let left = rects.iter().filter(|(_, r)| r.x < area.x).count();
    let right = rects.iter().filter(|(_, r)| r.x + r.w > area.x + area.w).count();
    (left, right)
}

/// The tiles of `row` side by side in `rect`, `sep` pixels apart; folded ones as upright
/// bars `FOLD_WIDTH` wide. A row folded in its column (`in_bar`) gives each tile its share
/// of the bar, folded or not.
fn row_geometries(row: &Row, rect: Rect, sep: i32, in_bar: bool, visible: &dyn Fn(TileId) -> bool) -> Vec<(TileId, Rect)> {
    let shown: Vec<(TileId, f64, bool)> = (0..row.tiles.len())
        .filter(|i| visible(row.tiles[*i]))
        .map(|i| (row.tiles[i], row.widths[i], row.folded[i] && !in_bar))
        .collect();
    // Folded widths are stale shares: only the open tiles split what the bars leave.
    let total: f64 = shown.iter().filter(|(_, _, f)| !f).map(|(_, w, _)| w).sum();
    let bars = shown.iter().filter(|(_, _, f)| *f).count() as i32;
    let space = rect.w - sep * (shown.len() as i32 - 1) - bars * FOLD_WIDTH;
    let last_open = shown.iter().rposition(|(_, _, f)| !f);
    let mut x = rect.x;
    let mut out = Vec::new();
    for (i, (t, w, folded)) in shown.iter().enumerate() {
        let tw = if *folded {
            FOLD_WIDTH
        } else if Some(i) == last_open {
            // The last open tile takes what rounding left, minus the bars after it.
            let after = shown[i + 1..].len() as i32;
            rect.x + rect.w - x - after * (FOLD_WIDTH + sep)
        } else {
            (space as f64 * w / total).round() as i32
        };
        out.push((*t, Rect::new(x, rect.y, tw, rect.h)));
        x += tw + sep;
    }
    out
}

/// The side of `rect` a dragged tile dropped at (`x`, `y`) goes to: the edge nearest to
/// the point, relative to the tile's size (so a wide tile still takes drops above it).
pub fn drop_side(rect: &Rect, x: f64, y: f64) -> Side {
    let fx = (x - rect.x as f64) / rect.w.max(1) as f64;
    let fy = (y - rect.y as f64) / rect.h.max(1) as f64;
    [(Side::Left, fx), (Side::Right, 1.0 - fx), (Side::Up, fy), (Side::Down, 1.0 - fy)]
        .into_iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(side, _)| side)
        .unwrap()
}

/// Pixels in from a tile's edge where Mod and the pointer offer a split on that side.
pub const SPLIT_EDGE: f64 = 48.0;

/// The side of `rect` whose edge (x, y) is near: within `SPLIT_EDGE` of it, and a quarter
/// of the tile at most (small tiles keep a middle). None in the middle.
pub fn split_edge(rect: &Rect, x: f64, y: f64) -> Option<Side> {
    let side = drop_side(rect, x, y);
    let (from_edge, size) = match side {
        Side::Left => (x - rect.x as f64, rect.w),
        Side::Right => ((rect.x + rect.w) as f64 - x, rect.w),
        Side::Up => (y - rect.y as f64, rect.h),
        Side::Down => ((rect.y + rect.h) as f64 - y, rect.h),
    };
    (from_edge < split_reach(size)).then_some(side)
}

/// How far in from an edge a split is offered on a tile `size` pixels across.
fn split_reach(size: i32) -> f64 {
    SPLIT_EDGE.min(size as f64 / 4.0)
}

/// The band along `side` of `rect` where a split is offered (`split_edge`), lit with Mod.
pub fn split_band(rect: &Rect, side: Side) -> Rect {
    let reach = |size: i32| (split_reach(size).ceil() as i32).max(1);
    match side {
        Side::Left => Rect::new(rect.x, rect.y, reach(rect.w), rect.h),
        Side::Right => Rect::new(rect.x + rect.w - reach(rect.w), rect.y, reach(rect.w), rect.h),
        Side::Up => Rect::new(rect.x, rect.y, rect.w, reach(rect.h)),
        Side::Down => Rect::new(rect.x, rect.y + rect.h - reach(rect.h), rect.w, reach(rect.h)),
    }
}

/// Share of a tile's width, at its left and right edges, where a dropped tile opens a
/// column of its own instead of sharing the tile's row.
const COLUMN_EDGE: f64 = 0.2;

/// Whether a tile dropped at `x` on `side` of `rect` opens a new column there.
pub fn drop_column(rect: &Rect, x: f64, side: Side) -> bool {
    let fx = (x - rect.x as f64) / rect.w.max(1) as f64;
    match side {
        Side::Left => fx < COLUMN_EDGE,
        Side::Right => fx > 1.0 - COLUMN_EDGE,
        Side::Up | Side::Down => false,
    }
}

/// The edge band of `rect` on `side` shown while a drop would open a new column there.
pub fn column_zone(rect: &Rect, side: Side) -> Rect {
    let w = (rect.w as f64 * COLUMN_EDGE) as i32;
    match side {
        Side::Left => Rect::new(rect.x, rect.y, w, rect.h),
        _ => Rect::new(rect.x + rect.w - w, rect.y, w, rect.h),
    }
}

/// The half of `rect` on `side`: where a tile dropped on that side will go.
pub fn drop_zone(rect: &Rect, side: Side) -> Rect {
    let (hw, hh) = (rect.w / 2, rect.h / 2);
    match side {
        Side::Left => Rect::new(rect.x, rect.y, hw, rect.h),
        Side::Right => Rect::new(rect.x + hw, rect.y, rect.w - hw, rect.h),
        Side::Up => Rect::new(rect.x, rect.y, rect.w, hh),
        Side::Down => Rect::new(rect.x, rect.y + hh, rect.w, rect.h - hh),
    }
}

/// Find the closest tile from `from` in `dir`, given already computed geometries.
pub fn neighbor(geoms: &[(TileId, Rect)], from: TileId, dir: Direction) -> Option<TileId> {
    let (_, cur) = geoms.iter().find(|(id, _)| *id == from)?;
    let (cx, cy) = cur.center();
    geoms
        .iter()
        .filter(|(id, _)| *id != from)
        .filter(|(_, r)| match dir {
            Direction::Left => r.x + r.w <= cur.x,
            Direction::Right => r.x >= cur.x + cur.w,
            Direction::Up => r.y + r.h <= cur.y,
            Direction::Down => r.y >= cur.y + cur.h,
        })
        .min_by_key(|(_, r)| {
            let (x, y) = r.center();
            // Prefer tiles that overlap on the perpendicular axis, then the nearest one.
            let (along, across) = match dir {
                Direction::Left | Direction::Right => ((x - cx).abs(), (y - cy).abs()),
                Direction::Up | Direction::Down => ((y - cy).abs(), (x - cx).abs()),
            };
            along + 2 * across
        })
        .map(|(id, _)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect { x: 0, y: 0, w: 1000, h: 600 };

    fn all(_: TileId) -> bool {
        true
    }

    fn geo(l: &mut Layout, focus: TileId) -> Vec<(TileId, Rect)> {
        l.geometries(AREA, 0, 0, &all, Some(focus))
    }

    #[test]
    fn first_tile_fills_area() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        assert_eq!(geo(&mut l, 1), vec![(1, AREA)]);
    }

    #[test]
    fn second_column_halves_the_first() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        assert_eq!(geo(&mut l, 2), vec![(1, Rect::new(0, 0, 500, 600)), (2, Rect::new(500, 0, 500, 600))]);
    }

    #[test]
    fn horizontal_split_is_stacked_with_gaps() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Horizontal);
        // The gap goes around the column, the tiles inside it are `sep` apart.
        assert_eq!(
            l.geometries(AREA, 10, 2, &all, Some(1)),
            vec![(1, Rect::new(10, 10, 980, 289)), (2, Rect::new(10, 301, 980, 289))]
        );
    }

    #[test]
    fn columns_go_on_past_the_output_and_the_view_follows_focus() {
        let mut l = Layout::default();
        for id in 1..=4 {
            l.insert(id, Some(id - 1), Split::Vertical);
        }
        // Four half-width columns: the strip is twice the output.
        assert_eq!(
            geo(&mut l, 4),
            vec![
                (1, Rect::new(-1000, 0, 500, 600)),
                (2, Rect::new(-500, 0, 500, 600)),
                (3, Rect::new(0, 0, 500, 600)),
                (4, Rect::new(500, 0, 500, 600)),
            ]
        );
        // Focusing column 2 slides just enough to show it; then column 1.
        assert_eq!(geo(&mut l, 2)[1], (2, Rect::new(0, 0, 500, 600)));
        assert_eq!(geo(&mut l, 1)[0], (1, Rect::new(0, 0, 500, 600)));
        // Column 2 is already shown: nothing moves.
        assert_eq!(geo(&mut l, 2)[0], (1, Rect::new(0, 0, 500, 600)));
        // Closing columns never leaves empty space on the right.
        l.remove(4);
        l.remove(3);
        assert_eq!(geo(&mut l, 4), vec![(1, Rect::new(0, 0, 500, 600)), (2, Rect::new(500, 0, 500, 600))]);
    }

    #[test]
    fn a_slid_view_stays_until_focus_moves() {
        let mut l = Layout::default();
        for id in 1..=4 {
            l.insert(id, if id == 1 { None } else { Some(id - 1) }, Split::Vertical);
        }
        let x_of = |g: &[(TileId, Rect)], id: TileId| g.iter().find(|(t, _)| *t == id).unwrap().1.x;
        let before = x_of(&geo(&mut l, 1), 1);
        l.slide_view(300, Some(1));
        let g = geo(&mut l, 1);
        assert_eq!(x_of(&g, 1), before - 300, "focused 1 slides off");
        // Not past the end of the strip.
        l.slide_view(100_000, Some(1));
        let end = geo(&mut l, 1);
        assert_eq!(x_of(&end, 4) + end.iter().find(|(t, _)| *t == 4).unwrap().1.w, AREA.w);
        l.slide_view(-100_000, Some(1));
        assert_eq!(x_of(&geo(&mut l, 1), 1), before);
        // Focus moving brings the view back to the focused tile.
        l.slide_view(300, Some(1));
        geo(&mut l, 1);
        let two = geo(&mut l, 2);
        let r2 = two.iter().find(|(t, _)| *t == 2).unwrap().1;
        assert!(r2.x >= 0 && r2.x + r2.w <= AREA.w, "2 focused: shown whole");
        let back = geo(&mut l, 1);
        assert_eq!(x_of(&back, 1), before, "1 focused again: shown whole");
        // Or the slide stopping, focus staying.
        l.slide_view(300, Some(1));
        geo(&mut l, 1);
        l.stop_sliding();
        assert_eq!(x_of(&geo(&mut l, 1), 1), before);
        // Centering after a slide centers right away.
        l.slide_view(300, Some(1));
        geo(&mut l, 1);
        l.toggle_center(1);
        let g = geo(&mut l, 1);
        let r1 = g.iter().find(|(t, _)| *t == 1).unwrap().1;
        assert_eq!(r1.x + r1.w / 2, AREA.w / 2, "1 centered");
    }

    #[test]
    fn a_strip_that_fits_does_not_slide() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.cycle_width(1);
        l.toggle_center(1);
        let centered = geo(&mut l, 1);
        l.slide_view(-300, Some(1));
        assert_eq!(geo(&mut l, 1), centered);
        l.slide_view(300, Some(1));
        assert_eq!(geo(&mut l, 1), centered);
    }

    #[test]
    fn centering_keeps_the_column_in_the_middle_until_focus_leaves_it() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        // A lone full-width column: centering it changes nothing.
        l.toggle_center(1);
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, 0, 1000, 600))]);
        l.insert(2, Some(1), Split::Vertical);
        l.insert(3, Some(2), Split::Horizontal);
        l.toggle_center(2);
        assert_eq!(geo(&mut l, 2)[1], (2, Rect::new(250, 0, 500, 300)));
        // Focus elsewhere in the column, or on another output, keeps it centered.
        assert_eq!(geo(&mut l, 3)[1], (2, Rect::new(250, 0, 500, 300)));
        assert_eq!(l.geometries(AREA, 0, 0, &all, Some(99))[1], (2, Rect::new(250, 0, 500, 300)));
        // Pressing again stops centering: the view settles back with no empty space.
        l.toggle_center(3);
        assert_eq!(geo(&mut l, 2)[1], (2, Rect::new(500, 0, 500, 300)));
        // Focusing another column here also ends it.
        l.toggle_center(2);
        geo(&mut l, 2);
        assert!(l.ends_centering(2, 1));
        assert!(!l.ends_centering(2, 3) && !l.ends_centering(1, 2) && !l.ends_centering(2, 99));
        assert_eq!(geo(&mut l, 1)[0], (1, Rect::new(0, 0, 500, 600)));
        assert_eq!(l.centered, None);
    }

    #[test]
    fn stacking_stays_in_the_column() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        l.insert(3, Some(2), Split::Horizontal);
        assert_eq!(
            geo(&mut l, 3),
            vec![
                (1, Rect::new(0, 0, 500, 600)),
                (2, Rect::new(500, 0, 500, 300)),
                (3, Rect::new(500, 300, 500, 300)),
            ]
        );
    }

    #[test]
    fn stacked_tile_takes_the_place_and_folds_the_target_into_a_tab() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Horizontal);
        assert!(l.stack_on(3, 1));
        assert!(l.is_tab(1));
        assert!(!l.is_tab(3) && !l.is_tab(2));
        let g = geo(&mut l, 3);
        assert_eq!(g.iter().map(|(t, _)| *t).collect::<Vec<_>>(), vec![1, 3, 2]);
        // The tab is a bar; the stacked tile and its neighbour share the rest evenly.
        assert_eq!(g[0].1.h, FOLD_WIDTH);
        assert_eq!(g[1].1.h, g[2].1.h);
        // Closing the stacked tile brings the target back.
        l.remove(3);
        assert!(!l.is_tab(1));
        assert!(!l.stack_on(4, 99));
    }

    #[test]
    fn folding_by_hand_takes_the_smallest_level_with_open_neighbours() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        // A lone tile folds its column into a bar, the rest left empty.
        l.toggle_fold(1);
        assert!(l.is_tab(1));
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, 0, FOLD_WIDTH, 600))]);
        l.toggle_fold(1);
        assert!(!l.is_tab(1));
        l.insert_on(2, Some(1), Side::Right);
        l.insert(3, Some(2), Split::Horizontal);
        l.insert(4, Some(3), Split::Vertical);
        // Sharing a row: it folds inside it.
        l.toggle_fold(2);
        assert!(l.is_tab(2) && !l.is_tab(1));
        // Alone in its row: the row folds in the column.
        l.toggle_fold(3);
        assert!(l.is_tab(3));
        // Alone in its column: the column folds, the last open one too.
        l.toggle_fold(4);
        assert!(l.is_tab(4));
        l.toggle_fold(1);
        assert!(l.is_tab(1));
        // Again on a folded one: it opens.
        l.toggle_fold(1);
        assert!(!l.is_tab(1));
        l.toggle_fold(4);
        assert!(!l.is_tab(4));
        l.toggle_fold(2);
        assert!(!l.is_tab(2));
    }

    #[test]
    fn the_last_open_tile_of_a_row_or_column_folds_too() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Right);
        l.toggle_fold(2);
        l.toggle_fold(1);
        assert!(l.is_tab(1) && l.is_tab(2));
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, 0, FOLD_WIDTH, 600)), (2, Rect::new(FOLD_WIDTH, 0, FOLD_WIDTH, 600))]);
        l.toggle_fold(1);
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, 0, 1000 - FOLD_WIDTH, 600)), (2, Rect::new(1000 - FOLD_WIDTH, 0, FOLD_WIDTH, 600))]);

        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Horizontal);
        l.toggle_fold(2);
        l.toggle_fold(1);
        assert!(l.is_tab(1) && l.is_tab(2));
        l.toggle_fold(2);
        assert!(!l.is_tab(2) && l.is_tab(1));
    }

    #[test]
    fn a_swapped_tile_keeps_its_fold() {
        // Side by side in a row, and in columns of their own.
        for column in [false, true] {
            let mut l = Layout::default();
            l.insert(1, None, Split::Vertical);
            if column {
                l.insert(2, Some(1), Split::Vertical);
            } else {
                l.insert_on(2, Some(1), Side::Right);
            }
            l.toggle_fold(1);
            l.swap_keeping_folds(1, 2, true, false);
            assert!(l.is_tab(1) && !l.is_tab(2), "column: {column}");
            let rects = geo(&mut l, 2);
            assert_eq!((rects[0].0, rects[0].1.w), (2, rects[0].1.w), "column: {column}");
            assert_eq!((rects[1].0, rects[1].1.w), (1, FOLD_WIDTH), "column: {column}");
        }
    }

    #[test]
    fn row_width_resizes_inside_the_row() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Right);
        l.resize_row_width(1, 0.2);
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, 0, 700, 600)), (2, Rect::new(700, 0, 300, 600))]);
        // Shrunk too much it folds into a bar; a lone tile has nothing to share.
        l.resize_row_width(2, -1.0);
        let w = 1000 - FOLD_WIDTH;
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, 0, w, 600)), (2, Rect::new(w, 0, FOLD_WIDTH, 600))]);
        l.remove(2);
        assert!(!l.shares_row(1));
        l.resize_row_width(1, -0.2);
        assert_eq!(geo(&mut l, 1), vec![(1, AREA)]);
    }

    #[test]
    fn squeezed_tiles_of_a_row_fold_into_upright_bars() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Right);
        l.insert_on(3, Some(2), Side::Right);
        l.resize_row_width(1, 1.0);
        assert!(!l.is_tab(1) && l.is_tab(2) && l.is_tab(3));
        let w = 1000 - 2 * FOLD_WIDTH;
        assert_eq!(
            geo(&mut l, 1),
            vec![
                (1, Rect::new(0, 0, w, 600)),
                (2, Rect::new(w, 0, FOLD_WIDTH, 600)),
                (3, Rect::new(w + FOLD_WIDTH, 0, FOLD_WIDTH, 600)),
            ]
        );
        // Shrinking the open one does not open the others, nor fold the last one open.
        l.resize_row_width(1, -0.05);
        assert!(!l.is_tab(1) && l.is_tab(2) && l.is_tab(3));
        l.resize_row_width(1, -1.0);
        assert!(!l.is_tab(1));
    }

    #[test]
    fn a_tile_shrunk_too_much_folds_itself() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Right);
        l.insert_on(3, Some(2), Side::Right);
        // 1/3 each; 1 shrunk to 0.18, then below the threshold: folded, the others share its room.
        l.resize_row_width(1, -0.15);
        assert!(!l.is_tab(1));
        l.resize_row_width(1, -0.05);
        assert!(l.is_tab(1) && !l.is_tab(2) && !l.is_tab(3));
        let w = (1000 - FOLD_WIDTH) / 2;
        assert_eq!(geo(&mut l, 2)[1], (2, Rect::new(FOLD_WIDTH, 0, w, 600)));
        // A folded one does not resize; the same with rows of a column.
        l.resize_row_width(1, 0.5);
        assert!(l.is_tab(1));
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Horizontal);
        for _ in 0..7 {
            l.resize_height(2, -0.05); // 0.5 → 0.15: still open
        }
        assert!(!l.is_tab(2));
        l.resize_height(2, -0.05);
        assert!(l.is_tab(2) && !l.is_tab(1));
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, 0, 1000, 600 - FOLD_WIDTH)), (2, Rect::new(0, 600 - FOLD_WIDTH, 1000, FOLD_WIDTH))]);
    }

    #[test]
    fn a_folded_tile_of_a_row_opens_in_place_of_the_widest() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Right);
        l.insert_on(3, Some(2), Side::Right);
        l.resize_row_width(1, 0.2); // 2 and 3 squeezed to 0.23 each: still open
        assert!(!l.is_tab(2));
        l.resize_row_width(1, 0.2); // now 0.13: folded
        assert!(l.is_tab(2) && l.is_tab(3));
        l.open_tab(3);
        assert!(l.is_tab(1) && l.is_tab(2) && !l.is_tab(3));
        let g = geo(&mut l, 3);
        assert_eq!(g[2], (3, Rect::new(2 * FOLD_WIDTH, 0, 1000 - 2 * FOLD_WIDTH, 600)));
        // Taking out the one open tile opens the rest.
        l.remove(3);
        assert!(!l.is_tab(1) && !l.is_tab(2));
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, 0, 500, 600)), (2, Rect::new(500, 0, 500, 600))]);
    }

    #[test]
    fn side_splits_share_the_row() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Horizontal);
        l.insert_on(3, Some(2), Side::Right);
        l.insert_on(4, Some(2), Side::Left);
        assert_eq!(
            geo(&mut l, 3),
            vec![
                (1, Rect::new(0, 0, 1000, 300)),
                (4, Rect::new(0, 300, 333, 300)),
                (2, Rect::new(333, 300, 333, 300)),
                (3, Rect::new(666, 300, 334, 300)),
            ]
        );
        // Still one column; leaving the row gives its width back.
        l.remove(4);
        assert_eq!(geo(&mut l, 3)[1..], [(2, Rect::new(0, 300, 500, 300)), (3, Rect::new(500, 300, 500, 300))]);
        l.remove(2);
        l.remove(3);
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, 0, 1000, 600))]);
    }

    #[test]
    fn side_without_a_target_opens_a_column() {
        let mut l = Layout::default();
        l.insert_on(1, None, Side::Right);
        l.insert_on(2, None, Side::Right);
        l.insert_column(3, Some(1), Side::Right);
        l.insert_column(4, Some(1), Side::Left);
        l.insert_column(5, None, Side::Left);
        assert_eq!(l.tiles(), vec![5, 4, 1, 3, 2]);
    }

    #[test]
    fn folded_row_shows_a_bar_per_tile() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Right);
        assert!(l.stack_on(3, 2));
        assert!(l.is_tab(1) && l.is_tab(2));
        let g = geo(&mut l, 3);
        assert_eq!(g[0], (1, Rect::new(0, 0, 500, FOLD_WIDTH)));
        assert_eq!(g[1], (2, Rect::new(500, 0, 500, FOLD_WIDTH)));
        assert_eq!(g[2], (3, Rect::new(0, FOLD_WIDTH, 1000, 600 - FOLD_WIDTH)));
        // A split beside a tab opens its row.
        l.insert_on(4, Some(1), Side::Right);
        assert!(!l.is_tab(1) && l.is_tab(3));
    }

    #[test]
    fn invisible_tiles_take_no_space() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        l.insert(3, Some(1), Split::Horizontal);
        assert_eq!(l.geometries(AREA, 0, 0, &|t| t == 1, Some(1)), vec![(1, Rect::new(0, 0, 500, 600))]);
    }

    #[test]
    fn remove_closes_the_gap() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        l.insert(3, Some(2), Split::Horizontal);
        assert!(l.remove(2));
        assert!(!l.remove(42));
        assert_eq!(l.tiles(), vec![1, 3]);
        assert_eq!(geo(&mut l, 3)[1], (3, Rect::new(500, 0, 500, 600)));
        assert!(l.remove(1));
        assert!(l.remove(3));
        assert!(l.tiles().is_empty());
    }

    #[test]
    fn insert_on_each_side() {
        let place = |side| {
            let mut l = Layout::default();
            l.insert(1, None, Split::Vertical);
            l.insert_on(2, Some(1), side);
            geo(&mut l, 1)
        };
        let (left, right) = (Rect::new(0, 0, 500, 600), Rect::new(500, 0, 500, 600));
        let (top, bottom) = (Rect::new(0, 0, 1000, 300), Rect::new(0, 300, 1000, 300));
        assert_eq!(place(Side::Right), vec![(1, left), (2, right)]);
        assert_eq!(place(Side::Left), vec![(2, left), (1, right)]);
        assert_eq!(place(Side::Down), vec![(1, top), (2, bottom)]);
        assert_eq!(place(Side::Up), vec![(2, top), (1, bottom)]);
    }

    #[test]
    fn resize_width_and_presets() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        l.resize_width(1, 0.1);
        assert_eq!(geo(&mut l, 1)[0], (1, Rect::new(0, 0, 600, 600)));
        l.resize_width(1, -0.45);
        assert_eq!(geo(&mut l, 1)[0].1.w, 150);
        l.resize_width(1, -0.05);
        l.open_tab(1); // narrowed past the threshold it folded; open again as wide as a new one
        assert_eq!(geo(&mut l, 1)[0].1.w, 500);
        l.resize_width(1, -0.35); // 0.15: still open
        l.cycle_width(1); // 0.15 -> 1/3
        assert_eq!(geo(&mut l, 1)[0].1.w, 333);
        l.cycle_width(1); // -> 1/2
        assert_eq!(geo(&mut l, 1)[0].1.w, 500);
        l.toggle_full_width(1);
        assert_eq!(geo(&mut l, 1)[0], (1, Rect::new(0, 0, 1000, 600)));
        l.toggle_full_width(1);
        assert_eq!(geo(&mut l, 1)[0].1.w, 500);
    }

    #[test]
    fn a_column_narrowed_too_much_folds_showing_its_tab_count() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        l.add_tab(3, 2);
        l.resize_width(2, -0.3); // 0.2: open
        assert!(!l.is_tab(2));
        l.resize_width(2, -0.1); // below the threshold: folded, with its two tabs
        assert!(l.is_tab(2) && l.is_tab(3));
        let g = geo(&mut l, 1);
        assert_eq!(g[0], (1, Rect::new(0, 0, 500, 600)));
        assert_eq!(g[1], (3, Rect::new(500, 0, FOLD_WIDTH, 600)));
        assert_eq!(l.fold_counts(), &[(3, 2)]);
        // The last open column does not fold; a folded one does not resize.
        l.resize_width(1, -0.45);
        assert!(!l.is_tab(1));
        l.resize_width(2, 0.5);
        assert_eq!(geo(&mut l, 1)[1].1.w, FOLD_WIDTH);
        // Opened, it is as wide as a new column, not the sliver it folded at.
        l.open_tab(3);
        assert!(!l.is_tab(3));
        assert_eq!(geo(&mut l, 3)[1].1.w, 500);
        assert!(l.fold_counts().is_empty());
    }

    #[test]
    fn resize_height_in_a_column() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Horizontal);
        l.insert(3, Some(2), Split::Horizontal);
        l.resize_height(1, 1.0 / 3.0); // 1/3 -> 2/3, the others share the rest
        let g = geo(&mut l, 1);
        assert_eq!((g[0].1.h, g[1].1.h, g[2].1.h), (400, 100, 100));
        l.resize_height(1, -0.2); // back to 7/15 + 4/15 + 4/15
        let g = geo(&mut l, 1);
        assert_eq!((g[0].1.h, g[1].1.h, g[2].1.h), (280, 160, 160));
        let mut single = Layout::default();
        single.insert(1, None, Split::Vertical);
        single.resize_height(1, 0.3);
        assert_eq!(geo(&mut single, 1), vec![(1, AREA)]);
    }

    #[test]
    fn squeezed_tile_becomes_a_tab() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Horizontal);
        for _ in 0..7 {
            l.resize_height(1, 0.05); // 0.85 / 0.15: still a tile
        }
        assert!(!l.is_tab(2));
        l.resize_height(1, 0.05);
        assert!(l.is_tab(2));
        // The open tile takes the column but the tab's bar.
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, 0, 1000, 600 - FOLD_WIDTH)), (2, Rect::new(0, 600 - FOLD_WIDTH, 1000, FOLD_WIDTH))]);
        // Opening the tab folds the other one.
        l.open_tab(2);
        assert!(l.is_tab(1) && !l.is_tab(2));
        assert_eq!(geo(&mut l, 2), vec![(1, Rect::new(0, 0, 1000, FOLD_WIDTH)), (2, Rect::new(0, FOLD_WIDTH, 1000, 600 - FOLD_WIDTH))]);
        // Shrinking the one open row does nothing: it does not fold, the tab stays.
        l.resize_height(2, -0.05);
        assert!(l.is_tab(1) && !l.is_tab(2));
        // Closing the open tile of a column of tabs opens one.
        l.remove(2);
        assert!(!l.is_tab(1));
        l.insert(2, Some(1), Split::Horizontal);
        l.resize_height(1, 1.0);
        assert!(l.is_tab(2));
        l.remove(1);
        assert!(!l.is_tab(2));
        assert_eq!(geo(&mut l, 2), vec![(2, AREA)]);
    }

    #[test]
    fn swap_tiles() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        l.swap(1, 2);
        assert_eq!(l.tiles(), vec![2, 1]);
    }

    #[test]
    fn directional_neighbor() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        l.insert(3, Some(2), Split::Horizontal);
        let g = geo(&mut l, 1);
        assert_eq!(neighbor(&g, 1, Direction::Right), Some(2));
        assert_eq!(neighbor(&g, 3, Direction::Left), Some(1));
        assert_eq!(neighbor(&g, 2, Direction::Down), Some(3));
        assert_eq!(neighbor(&g, 3, Direction::Up), Some(2));
        assert_eq!(neighbor(&g, 1, Direction::Left), None);
    }

    #[test]
    fn rect_intersection() {
        let r = Rect::new(-100, 0, 300, 100);
        assert_eq!(r.intersection(&AREA), Some(Rect::new(0, 0, 200, 100)));
        assert_eq!(Rect::new(1000, 0, 10, 10).intersection(&AREA), None);
    }

    #[test]
    fn drops_go_to_the_nearest_edge() {
        let r = Rect::new(100, 0, 400, 200);
        assert_eq!(drop_side(&r, 110.0, 100.0), Side::Left);
        assert_eq!(drop_side(&r, 490.0, 100.0), Side::Right);
        assert_eq!(drop_side(&r, 300.0, 10.0), Side::Up);
        assert_eq!(drop_side(&r, 300.0, 190.0), Side::Down);
    }

    #[test]
    fn drops_at_the_outer_edges_open_a_column() {
        let r = Rect::new(100, 0, 400, 200);
        assert!(drop_column(&r, 150.0, Side::Left));
        assert!(!drop_column(&r, 250.0, Side::Left));
        assert!(drop_column(&r, 450.0, Side::Right));
        assert!(!drop_column(&r, 350.0, Side::Right));
        assert!(!drop_column(&r, 110.0, Side::Up));
        assert_eq!(column_zone(&r, Side::Left), Rect::new(100, 0, 80, 200));
        assert_eq!(column_zone(&r, Side::Right), Rect::new(420, 0, 80, 200));
    }

    #[test]
    fn splits_are_offered_near_the_edges() {
        let r = Rect::new(100, 0, 400, 200);
        assert_eq!(split_edge(&r, 110.0, 100.0), Some(Side::Left));
        assert_eq!(split_edge(&r, 490.0, 100.0), Some(Side::Right));
        assert_eq!(split_edge(&r, 300.0, 5.0), Some(Side::Up));
        assert_eq!(split_edge(&r, 300.0, 195.0), Some(Side::Down));
        assert_eq!(split_edge(&r, 300.0, 100.0), None);
        assert_eq!(split_edge(&r, 160.0, 100.0), None); // past SPLIT_EDGE
        // A small tile: a quarter of it at most.
        let small = Rect::new(0, 0, 80, 80);
        assert_eq!(split_edge(&small, 19.0, 40.0), Some(Side::Left));
        assert_eq!(split_edge(&small, 25.0, 40.0), None);
    }

    #[test]
    fn split_bands_are_where_splits_are_offered() {
        let r = Rect::new(100, 0, 400, 200);
        assert_eq!(split_band(&r, Side::Left), Rect::new(100, 0, 48, 200));
        assert_eq!(split_band(&r, Side::Right), Rect::new(452, 0, 48, 200));
        assert_eq!(split_band(&r, Side::Up), Rect::new(100, 0, 400, 48));
        assert_eq!(split_band(&r, Side::Down), Rect::new(100, 152, 400, 48));
        // A small tile: a quarter of it.
        assert_eq!(split_band(&Rect::new(0, 0, 80, 200), Side::Left), Rect::new(0, 0, 20, 200));
    }

    #[test]
    fn drop_zones_are_the_half_on_the_side() {
        let r = Rect::new(100, 0, 401, 201);
        assert_eq!(drop_zone(&r, Side::Left), Rect::new(100, 0, 200, 201));
        assert_eq!(drop_zone(&r, Side::Right), Rect::new(300, 0, 201, 201));
        assert_eq!(drop_zone(&r, Side::Up), Rect::new(100, 0, 401, 100));
        assert_eq!(drop_zone(&r, Side::Down), Rect::new(100, 100, 401, 101));
    }

    /// Take `id` out of `l` and put it back where it was: the layout must not change.
    /// Tiles 1 and 2 in columns, then 3 and 4 added as tabs of 1.
    fn tabbed() -> Layout {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        assert!(l.add_tab(3, 1));
        assert!(l.add_tab(4, 3));
        l
    }

    #[test]
    fn tabs_share_the_container_and_only_the_active_one_is_laid_out() {
        let mut l = tabbed();
        assert_eq!(l.group(1), Some(vec![1, 3, 4]));
        assert_eq!(l.tiles(), vec![1, 3, 4, 2]);
        assert!(l.contains(3) && l.contains(1));
        let g = geo(&mut l, 4);
        // The active tab gets the container minus its bar; the others are not laid out.
        assert_eq!(g, vec![(4, Rect::new(0, TAB_HEIGHT, 500, 600 - TAB_HEIGHT)), (2, Rect::new(500, 0, 500, 600))]);
        assert_eq!(l.tab_bars(), &[TabBar { rect: Rect::new(0, 0, 500, TAB_HEIGHT), tabs: vec![1, 3, 4], active: 4 }]);
    }

    #[test]
    fn switching_tabs() {
        let mut l = tabbed();
        assert!(l.activate_tab(1));
        assert!(!l.activate_tab(1), "already shown");
        assert!(!l.activate_tab(2), "not a tab");
        assert_eq!(geo(&mut l, 1)[0].0, 1);
        assert_eq!(l.cycle_tab(1, 1), Some(3));
        assert_eq!(l.cycle_tab(3, -2), Some(4));
        assert_eq!(l.cycle_tab(4, 1), Some(1), "goes round");
        assert_eq!(l.active_tab(3), Some(1));
        assert_eq!(l.cycle_tab(2, 1), None);
    }

    #[test]
    fn closing_tabs_shows_a_sibling_and_a_last_tab_is_a_plain_tile() {
        let mut l = tabbed();
        assert_eq!(l.tab_sibling(4), Some(3), "the one before when last");
        assert_eq!(l.tab_sibling(3), Some(4));
        assert!(l.remove(4));
        assert_eq!(geo(&mut l, 3)[0].0, 3);
        assert!(l.remove(1), "a hidden tab");
        assert_eq!(l.group(3), None);
        assert!(l.tab_bars().len() == 1, "bars are those of the last layout");
        assert_eq!(geo(&mut l, 3), vec![(3, Rect::new(0, 0, 500, 600)), (2, Rect::new(500, 0, 500, 600))]);
        assert!(l.tab_bars().is_empty());
    }

    #[test]
    fn a_tab_without_a_window_yet_shows_the_tab_that_has_one() {
        let mut l = tabbed();
        let shown = l.geometries(AREA, 0, 0, &|t| t != 4, Some(4));
        // 4 was just launched: 1 keeps showing, with a bar of the tabs that have a window.
        assert_eq!(shown[0].0, 1);
        assert_eq!(l.tab_bars()[0].tabs, vec![1, 3]);
        let alone = l.geometries(AREA, 0, 0, &|t| t == 1 || t == 2, Some(1));
        assert_eq!(alone[0], (1, Rect::new(0, 0, 500, 600)));
        assert!(l.tab_bars().is_empty());
    }

    #[test]
    fn tabs_swap_and_go_back_to_their_container() {
        let mut l = tabbed();
        l.swap(4, 2);
        assert_eq!(l.group(2), Some(vec![1, 3, 2]));
        assert_eq!(geo(&mut l, 2)[0].0, 2);
        let home = l.home(3).unwrap();
        assert_eq!(home, Home::Tab { with: 2, index: 1 });
        l.remove(3);
        assert!(l.restore(3, home));
        assert_eq!(l.group(1), Some(vec![1, 3, 2]));
        assert_eq!(l.active_tab(1), Some(3));
    }

    #[test]
    fn tabs_belong_to_the_column_and_each_keeps_its_splits() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Right);
        // A tab of a tile inside a split is a tab of its column: one bar over the column.
        assert!(l.add_tab(3, 2));
        assert_eq!(geo(&mut l, 3), vec![(3, Rect::new(0, TAB_HEIGHT, 1000, 600 - TAB_HEIGHT))]);
        assert_eq!(l.tab_bars(), &[TabBar { rect: Rect::new(0, 0, 1000, TAB_HEIGHT), tabs: vec![1, 3], active: 3 }]);
        // Back on the first tab, its split is still there.
        assert_eq!(l.cycle_tab(3, 1), Some(1));
        let g = geo(&mut l, 1);
        assert_eq!(g, vec![(1, Rect::new(0, TAB_HEIGHT, 500, 600 - TAB_HEIGHT)), (2, Rect::new(500, TAB_HEIGHT, 500, 600 - TAB_HEIGHT))]);
        // A tab can be split in turn; the bar stays the column's only one.
        l.activate_tab(3);
        l.insert_on(4, Some(3), Side::Down);
        assert_eq!(geo(&mut l, 4).len(), 2);
        assert_eq!(l.tab_bars().len(), 1);
        assert_eq!(l.tiles(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn a_lone_tile_gets_a_bar_with_its_one_tab() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        assert!(l.lone_tab(1));
        assert!(!l.lone_tab(1));
        assert_eq!(l.group(1), Some(vec![1]));
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, TAB_HEIGHT, 1000, 600 - TAB_HEIGHT))]);
        assert_eq!(l.tab_bars(), &[TabBar { rect: Rect::new(0, 0, 1000, TAB_HEIGHT), tabs: vec![1], active: 1 }]);
        // More tabs join it; closing back to one keeps the plain column.
        l.add_tab(2, 1);
        assert_eq!(l.group(1), Some(vec![1, 2]));
        l.remove(2);
        assert_eq!(l.group(1), None);
        // Its last tile leaving takes the column away.
        assert!(l.lone_tab(1));
        assert!(l.remove(1));
        assert_eq!(l.tiles(), Vec::<TileId>::new());
    }

    #[test]
    fn a_tab_moves_to_another_place_in_its_bar() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.add_tab(2, 1);
        l.add_tab(3, 2);
        l.insert_on(4, Some(3), Side::Down);
        assert_eq!(l.group(1), Some(vec![1, 2, 3]));
        // The shown tab (with its split) goes first; it stays shown.
        assert!(l.move_tab(4, 0));
        assert_eq!(l.group(1), Some(vec![3, 1, 2]));
        assert_eq!(l.active_tab(1), Some(3));
        assert_eq!(l.tiles(), vec![3, 4, 1, 2]);
        // A hidden one goes last.
        assert!(l.move_tab(1, 2));
        assert_eq!(l.group(1), Some(vec![3, 2, 1]));
        assert_eq!(l.active_tab(1), Some(3));
        // Its own place, or past the end: nothing moves.
        assert!(!l.move_tab(1, 2));
        assert!(!l.move_tab(1, 3));
    }

    #[test]
    fn an_inner_tile_goes_to_a_tab_of_its_column() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Down);
        l.insert(5, Some(1), Split::Vertical);
        assert!(l.tile_to_tab(2));
        assert_eq!(l.group(1), Some(vec![1, 2]));
        assert_eq!(geo(&mut l, 2)[0], (2, Rect::new(0, TAB_HEIGHT, 500, 600 - TAB_HEIGHT)));
        // Alone in its tab (or its column): nothing to take out.
        assert!(!l.tile_to_tab(2));
        assert!(!l.tile_to_tab(5));
        // Closing it shows the tab it came from, and the column has no tabs any more.
        assert!(l.remove(2));
        assert_eq!(l.group(1), None);
        assert_eq!(geo(&mut l, 1)[0], (1, Rect::new(0, 0, 500, 600)));
    }

    #[test]
    fn an_inner_tile_goes_back_from_its_tab() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Down);
        l.insert_on(3, Some(2), Side::Right);
        let home = l.home(3).unwrap();
        assert!(l.tile_to_tab(3));
        assert!(l.add_tab(4, 1));
        assert!(!l.tab_back(1, home), "1 shares its tab with its neighbour");
        assert!(l.tab_back(3, home));
        assert_eq!(l.group(1), Some(vec![1, 4]));
        assert_eq!(l.active_tab(4), Some(1), "the tab it went back to shows");
        assert_eq!(l.home(3), Some(home));
        // Its neighbour gone, it stays a tab.
        assert!(l.tile_to_tab(3));
        assert!(l.remove(2));
        assert!(!l.tab_back(3, home));
        assert_eq!(l.group(1), Some(vec![1, 3, 4]));
    }

    #[test]
    fn the_one_tab_of_a_column_loses_its_bar() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        assert!(!l.untab(1));
        assert!(l.lone_tab(1));
        assert!(l.untab(1));
        assert_eq!(l.group(1), None);
        l.lone_tab(1);
        l.add_tab(2, 1);
        assert!(!l.untab(1), "other tabs keep the bar");
    }

    #[test]
    fn a_tile_of_a_hidden_tab_leaves_and_comes_back() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Right);
        assert!(l.add_tab(3, 1));
        // 2 is in the hidden tab, beside 1.
        let home = l.home(2).unwrap();
        assert_eq!(home, Home::Tab { with: 3, index: 0 });
        assert!(l.remove(2));
        assert_eq!(l.active_tab(3), Some(3), "the shown tab stays");
        assert_eq!(l.tiles(), vec![1, 3]);
        assert!(l.restore(2, home));
        assert_eq!(l.group(1), Some(vec![2, 1, 3]));
    }

    #[test]
    fn accordion_stack_inside_a_tab() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        assert!(l.add_tab(3, 1));
        assert!(l.stack_on(5, 3));
        let g = geo(&mut l, 5);
        assert_eq!(g.len(), 2);
        assert_eq!(l.tab_bars().len(), 1);
        assert!(l.is_tab(3) && !l.is_tab(5));
    }

    #[test]
    fn tiles_past_the_edges_are_counted() {
        let area = Rect::new(100, 0, 1000, 600);
        let rects = [
            (1, Rect::new(-500, 0, 400, 600)),
            (2, Rect::new(50, 0, 400, 600)),
            (3, Rect::new(500, 0, 400, 600)),
            (4, Rect::new(1000, 0, 400, 300)),
            (5, Rect::new(1200, 300, 400, 300)),
        ];
        assert_eq!(past_edges(area, &rects), (2, 2));
        assert_eq!(past_edges(area, &rects[2..3]), (0, 0));
        assert_eq!(past_edges(area, &[]), (0, 0));
    }

    #[test]
    fn tab_bar_layout() {
        let rect = Rect::new(10, 20, 300, 200);
        assert_eq!(split_tab_bar(rect), (Rect::new(10, 20, 300, TAB_HEIGHT), Rect::new(10, 20 + TAB_HEIGHT, 300, 200 - TAB_HEIGHT)));
        let bar = Rect::new(0, 0, 1000, TAB_HEIGHT);
        let two = tab_strip(bar, 2, 0, TabWidths::default());
        assert_eq!(two.tabs, vec![Rect::new(0, 0, TAB_MAX_WIDTH, TAB_HEIGHT), Rect::new(TAB_MAX_WIDTH, 0, TAB_MAX_WIDTH, TAB_HEIGHT)]);
        assert_eq!(two.plus, Rect::new(2 * TAB_MAX_WIDTH, 0, PLUS_WIDTH, TAB_HEIGHT));
        assert_eq!((two.left, two.right), (None, None));
        // A full bar: the tabs share what the "+" leaves, the "+" at the right end.
        let full = Rect::new(0, 0, 600 + PLUS_WIDTH, TAB_HEIGHT);
        assert_eq!(tab_strip(full, 3, 0, TabWidths::default()).tabs[2], Rect::new(400, 0, 200, TAB_HEIGHT));
        assert_eq!(tab_strip(full, 3, 0, TabWidths::default()).plus, Rect::new(600, 0, PLUS_WIDTH, TAB_HEIGHT));
        assert!(tab_strip(bar, 0, 0, TabWidths::default()).tabs.is_empty());
        assert_eq!(tab_strip(bar, 0, 0, TabWidths::default()).plus, Rect::new(0, 0, PLUS_WIDTH, TAB_HEIGHT));
    }

    #[test]
    fn a_bar_of_many_tabs_scrolls_with_marks_at_its_ends() {
        // Room for 4 tabs of TAB_MIN_WIDTH between the marks.
        let bar = Rect::new(0, 0, 4 * TAB_MIN_WIDTH + 2 * ARROW_WIDTH + PLUS_WIDTH, TAB_HEIGHT);
        let tab = |s: &TabStrip, i: usize| s.tabs[i];
        // The first shown: nothing to its left, 6 past the right end.
        let s = tab_strip(bar, 10, 0, TabWidths::default());
        assert_eq!((s.first, s.count, s.left), (0, 4, None));
        assert_eq!(s.right, Some((Rect::new(ARROW_WIDTH + 4 * TAB_MIN_WIDTH, 0, ARROW_WIDTH, TAB_HEIGHT), 6)));
        assert_eq!(tab(&s, 0), Rect::new(ARROW_WIDTH, 0, TAB_MIN_WIDTH, TAB_HEIGHT));
        assert_eq!(tab(&s, 5).w, 0, "scrolled out");
        assert_eq!(s.plus.x, ARROW_WIDTH + 4 * TAB_MIN_WIDTH + ARROW_WIDTH);
        // One in the middle stays in the middle, tabs past both ends.
        let s = tab_strip(bar, 10, 5, TabWidths::default());
        assert_eq!((s.first, s.left.map(|l| l.1), s.right.map(|r| r.1)), (3, Some(3), Some(3)));
        assert_eq!(tab(&s, 5).x, ARROW_WIDTH + 2 * TAB_MIN_WIDTH);
        // The last: the bar ends with it.
        let s = tab_strip(bar, 10, 9, TabWidths::default());
        assert_eq!((s.first, s.left.map(|l| l.1), s.right), (6, Some(6), None));
        // A mark stands for the tab just past it.
        let s = tab_strip(bar, 10, 5, TabWidths::default());
        assert_eq!(s.index_at(1.0, 1.0), Some(2));
        assert_eq!(s.index_at((ARROW_WIDTH + 4 * TAB_MIN_WIDTH + 1) as f64, 1.0), Some(7));
        assert_eq!(s.index_at((ARROW_WIDTH + 1) as f64, 1.0), Some(3));
    }

    fn round_trip(l: &mut Layout, id: TileId) {
        let before = geo(l, id);
        let home = l.home(id).unwrap();
        l.remove(id);
        assert!(l.restore(id, home));
        assert_eq!(geo(l, id), before);
    }

    #[test]
    fn a_tile_goes_back_to_its_place_in_a_row() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Right);
        l.insert_on(3, Some(2), Side::Right);
        l.resize_row_width(2, 0.2);
        round_trip(&mut l, 2);
        round_trip(&mut l, 1);
    }

    #[test]
    fn a_tile_goes_back_to_its_row_in_a_column() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Horizontal);
        l.insert(3, Some(2), Split::Horizontal);
        l.insert(4, Some(1), Split::Vertical);
        round_trip(&mut l, 2);
        round_trip(&mut l, 1);
    }

    #[test]
    fn a_tile_goes_back_to_its_column() {
        let mut l = Layout::default();
        for id in 1..=3 {
            l.insert(id, Some(id - 1), Split::Vertical);
        }
        l.cycle_width(2);
        round_trip(&mut l, 2);
        round_trip(&mut l, 1);
        round_trip(&mut l, 3);
    }

    #[test]
    fn a_stacked_tile_goes_back_on_its_tab() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        assert!(l.stack_on(2, 1));
        round_trip(&mut l, 2);
        assert!(l.is_tab(1));
    }

    #[test]
    fn no_place_to_go_back_to_without_the_neighbour() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Horizontal);
        let home = l.home(2).unwrap();
        l.remove(2);
        l.remove(1);
        assert!(!l.restore(2, home));
        assert!(!l.contains(2));
    }
}

