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

    fn center(&self) -> (i32, i32) {
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
/// Widest a tab of a tabbed container gets in its bar, in pixels.
pub const TAB_MAX_WIDTH: i32 = 220;
/// Width of the "+" at the end of a tab bar.
pub const PLUS_WIDTH: i32 = TAB_HEIGHT;

/// The bar of a tabbed container: where it is, its tabs in order and the one shown.
#[derive(Clone, Debug, PartialEq)]
pub struct TabBar {
    pub rect: Rect,
    pub tabs: Vec<TileId>,
    pub active: TileId,
}

/// A tabbed container's bar and the room left for its shown tab. Every placement of
/// the bar goes through here (today: along the top).
pub fn split_tab_bar(rect: Rect) -> (Rect, Rect) {
    let bar = Rect::new(rect.x, rect.y, rect.w, TAB_HEIGHT.min(rect.h));
    let content = Rect::new(rect.x, rect.y + bar.h, rect.w, (rect.h - bar.h).max(1));
    (bar, content)
}

/// Where each of `n` tabs sits in `bar`: side by side from the left, as wide as
/// `TAB_MAX_WIDTH` while they fit, sharing the bar (but the "+") when they don't.
pub fn tab_rects(bar: Rect, n: usize) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    let w = tab_width(bar, n);
    (0..n as i32).map(|i| Rect::new(bar.x + i * w, bar.y, w, bar.h)).collect()
}

/// The "+" of a bar with `n` tabs, right after the last one: a new empty tab.
pub fn plus_rect(bar: Rect, n: usize) -> Rect {
    let x = if n == 0 { bar.x } else { bar.x + n as i32 * tab_width(bar, n) };
    Rect::new(x.min(bar.x + bar.w - PLUS_WIDTH), bar.y, PLUS_WIDTH.min(bar.w), bar.h)
}

fn tab_width(bar: Rect, n: usize) -> i32 {
    ((bar.w - PLUS_WIDTH) / n as i32).clamp(1, TAB_MAX_WIDTH)
}

/// Tiles side by side in a column, sharing the column's width.
#[derive(Debug)]
struct Row {
    tiles: Vec<TileId>,
    /// Share of the column's width of each tile; they add up to 1.
    widths: Vec<f64>,
}

impl Row {
    fn new(id: TileId) -> Self {
        Row { tiles: vec![id], widths: vec![1.0] }
    }

    /// Put `id` at `index`, giving it an even share of the width.
    fn insert(&mut self, index: usize, id: TileId) {
        let share = 1.0 / (self.tiles.len() + 1) as f64;
        for w in &mut self.widths {
            *w *= 1.0 - share;
        }
        self.tiles.insert(index, id);
        self.widths.insert(index, share);
    }

    fn remove(&mut self, index: usize) {
        self.tiles.remove(index);
        let w = self.widths.remove(index);
        if w < 1.0 {
            for rest in &mut self.widths {
                *rest /= 1.0 - w;
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
}

impl Column {
    fn new(id: TileId, width: f64) -> Self {
        Column {
            rows: vec![Row::new(id)],
            heights: vec![1.0],
            tabs: vec![false],
            width,
            restore_width: None,
        }
    }

    fn tiles(&self) -> impl Iterator<Item = TileId> + '_ {
        self.rows.iter().flat_map(|r| r.tiles.iter().copied())
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
    /// Tabbed containers, their tabs in order (two or more). The one in a row is the
    /// active tab; the others live only here.
    groups: Vec<Vec<TileId>>,
    /// Bars of the tabbed containers, as last laid out by `geometries`.
    bars: Vec<TabBar>,
}

impl Layout {
    pub fn contains(&self, id: TileId) -> bool {
        self.find(id).is_some()
    }

    /// Column, row and place in the row of a tile; a hidden tab is where its container is.
    fn find(&self, id: TileId) -> Option<(usize, usize, usize)> {
        self.find_slot(id).or_else(|| self.group(id)?.iter().find_map(|t| self.find_slot(*t)))
    }

    /// Column, row and place in the row of a tile that sits in a row itself.
    fn find_slot(&self, id: TileId) -> Option<(usize, usize, usize)> {
        self.columns.iter().enumerate().find_map(|(c, col)| {
            col.rows
                .iter()
                .enumerate()
                .find_map(|(r, row)| row.tiles.iter().position(|t| *t == id).map(|k| (c, r, k)))
        })
    }

    fn column_of(&mut self, id: TileId) -> Option<&mut Column> {
        let (c, _, _) = self.find(id)?;
        Some(&mut self.columns[c])
    }

    /// Ids of all tiles, column by column from the left, top to bottom, left to right;
    /// a tabbed container's tabs in their order.
    pub fn tiles(&self) -> Vec<TileId> {
        self.columns
            .iter()
            .flat_map(|c| c.tiles())
            .flat_map(|t| self.group(t).map_or_else(|| vec![t], <[TileId]>::to_vec))
            .collect()
    }

    // ---- tabbed containers ----

    fn group_index(&self, id: TileId) -> Option<usize> {
        self.groups.iter().position(|g| g.contains(&id))
    }

    /// The tabs of the container `id` is a tab of, in order.
    pub fn group(&self, id: TileId) -> Option<&[TileId]> {
        self.group_index(id).map(|g| self.groups[g].as_slice())
    }

    /// Add `new` as a tab of `target`'s container, after the active tab, and show it.
    pub fn add_tab(&mut self, new: TileId, target: TileId) -> bool {
        let Some((c, r, k)) = self.find(target) else { return false };
        let active = self.columns[c].rows[r].tiles[k];
        let g = match self.group_index(active) {
            Some(g) => g,
            None => {
                self.groups.push(vec![active]);
                self.groups.len() - 1
            }
        };
        let at = self.groups[g].iter().position(|t| *t == active).unwrap() + 1;
        self.groups[g].insert(at, new);
        self.columns[c].rows[r].tiles[k] = new;
        true
    }

    /// Show tab `id` in its container. False when it is not a hidden tab here.
    pub fn activate_tab(&mut self, id: TileId) -> bool {
        if self.find_slot(id).is_some() || self.group_index(id).is_none() {
            return false;
        }
        let Some((c, r, k)) = self.find(id) else { return false };
        self.columns[c].rows[r].tiles[k] = id;
        true
    }

    /// The active tab of `id`'s container, if it is a tabbed one.
    pub fn active_tab(&self, id: TileId) -> Option<TileId> {
        self.group(id)?.iter().copied().find(|t| self.find_slot(*t).is_some())
    }

    /// Show the tab `delta` places after the active one of `id`'s container (going
    /// round); the tab now shown.
    pub fn cycle_tab(&mut self, id: TileId, delta: i32) -> Option<TileId> {
        let group = self.group(id)?;
        let active = self.active_tab(id)?;
        let i = group.iter().position(|t| *t == active)? as i32;
        let next = group[(i + delta).rem_euclid(group.len() as i32) as usize];
        self.activate_tab(next);
        Some(next)
    }

    /// The tab that takes `id`'s place when it leaves its container: the next one, else
    /// the one before.
    pub fn tab_sibling(&self, id: TileId) -> Option<TileId> {
        let group = self.group(id)?;
        let i = group.iter().position(|t| *t == id)?;
        group.get(i + 1).or_else(|| group.get(i.wrapping_sub(1))).copied()
    }

    /// Take `id` out of its tabbed container (a sibling shows in its place); false when
    /// it is in none.
    fn leave_group(&mut self, id: TileId) -> bool {
        let Some(g) = self.group_index(id) else { return false };
        if let Some((c, r, k)) = self.find_slot(id) {
            let sibling = self.tab_sibling(id).unwrap();
            self.columns[c].rows[r].tiles[k] = sibling;
        }
        self.groups[g].retain(|t| *t != id);
        if self.groups[g].len() < 2 {
            self.groups.remove(g);
        }
        true
    }

    /// The tabbed containers' bars, as last laid out by `geometries`.
    pub fn tab_bars(&self) -> &[TabBar] {
        &self.bars
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
        if let Some(group) = self.group(id) {
            let index = group.iter().position(|t| *t == id).unwrap();
            return Some(Home::Tab { with: self.tab_sibling(id).unwrap(), index });
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
                let rest: f64 = row.widths.iter().enumerate().filter(|(i, _)| *i != k).map(|(_, w)| w).sum();
                for (i, w) in row.widths.iter_mut().enumerate() {
                    *w = if i == k { width } else { *w / rest * (1.0 - width) };
                }
            }
            Home::Stacked { on } if here(on) => {
                self.stack_on(id, on);
            }
            Home::Tab { with, index } if here(with) => {
                self.add_tab(id, with);
                let g = self.group_index(id).unwrap();
                let group = &mut self.groups[g];
                group.retain(|t| *t != id);
                group.insert(index.min(group.len()), id);
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

    /// Remove a tile; an emptied column goes away and the strip closes the gap.
    pub fn remove(&mut self, id: TileId) -> bool {
        if self.leave_group(id) {
            return true;
        }
        let Some((c, r, k)) = self.find(id) else { return false };
        self.columns[c].remove_tile(r, k);
        if self.columns[c].rows.is_empty() {
            self.columns.remove(c);
        }
        true
    }

    /// Exchange the positions of two tiles.
    pub fn swap(&mut self, x: TileId, y: TileId) {
        let rows = self.columns.iter_mut().flat_map(|c| c.rows.iter_mut()).flat_map(|r| r.tiles.iter_mut());
        for t in rows.chain(self.groups.iter_mut().flatten()) {
            if *t == x {
                *t = y;
            } else if *t == y {
                *t = x;
            }
        }
    }

    /// Make the tile's column wider (positive `delta`) or narrower, as a share of the output.
    pub fn resize_width(&mut self, id: TileId, delta: f64) {
        if let Some(col) = self.column_of(id) {
            col.width = (col.width + delta).clamp(MIN_WIDTH, MAX_WIDTH);
            col.restore_width = None;
        }
    }

    /// Make the tile's row taller (positive `delta`) or shorter, as a share of its column;
    /// the other rows of the column give or take the difference. Rows squeezed below
    /// `TAB_THRESHOLD` fold into tabs; shrinking a row opens the column's tabs again.
    pub fn resize_height(&mut self, id: TileId, delta: f64) {
        let Some((c, r, _)) = self.find(id) else { return };
        let col = &mut self.columns[c];
        if delta < 0.0 && col.tabs.iter().any(|t| *t) {
            let even = 1.0 / col.rows.len() as f64;
            col.heights.iter_mut().for_each(|h| *h = even);
            col.tabs.iter_mut().for_each(|t| *t = false);
            return;
        }
        if col.tabs.iter().filter(|t| !**t).count() < 2 {
            return;
        }
        col.normalize_open();
        let old = col.heights[r];
        let new = (old + delta).clamp(MIN_HEIGHT, 1.0);
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
    /// column's width; the other tiles of the row give or take the difference.
    pub fn resize_row_width(&mut self, id: TileId, delta: f64) {
        let Some((c, r, k)) = self.find(id) else { return };
        let row = &mut self.columns[c].rows[r];
        let n = row.tiles.len();
        if n < 2 {
            return;
        }
        let old = row.widths[k];
        let new = (old + delta).clamp(MIN_ROW_WIDTH, 1.0 - MIN_ROW_WIDTH * (n - 1) as f64);
        // The others keep their proportions, but none drops below the minimum.
        let rest: f64 = 1.0 - old;
        let scale = (1.0 - new) / rest;
        for (i, w) in row.widths.iter_mut().enumerate() {
            *w = if i == k { new } else { *w * scale };
        }
        let short: f64 = row.widths.iter().map(|w| (MIN_ROW_WIDTH - w).max(0.0)).sum();
        if short > 0.0 {
            let others = row.widths.iter().enumerate().filter(|(i, w)| *i != k && **w > MIN_ROW_WIDTH);
            let spare: f64 = others.map(|(_, w)| w - MIN_ROW_WIDTH).sum();
            for (i, w) in row.widths.iter_mut().enumerate() {
                if i == k {
                    continue;
                }
                *w = if *w <= MIN_ROW_WIDTH {
                    MIN_ROW_WIDTH
                } else {
                    *w - (*w - MIN_ROW_WIDTH) / spare * short
                };
            }
        }
    }

    pub fn is_tab(&self, id: TileId) -> bool {
        self.find(id).is_some_and(|(c, r, _)| self.columns[c].tabs[r])
    }

    /// Open a tab: its row takes the place of the largest open row of its column, which
    /// becomes a tab in turn.
    pub fn open_tab(&mut self, id: TileId) {
        let Some((c, r, _)) = self.find(id) else { return };
        let col = &mut self.columns[c];
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

    /// Lay the strip out in `area`, leaving `gap` pixels around and between columns and
    /// `sep` pixels between the tiles inside a column (room for one shared border), and
    /// slide the view so that `focus` (when it is here) is shown whole. Tiles off the
    /// view get rectangles outside `area`; tabs get the rectangle of their bar. Invisible
    /// tiles (e.g. a terminal that has not mapped its window yet) take no space.
    pub fn geometries(&mut self, area: Rect, gap: i32, sep: i32, has_window: &dyn Fn(TileId) -> bool, focus: Option<TileId>) -> Vec<(TileId, Rect)> {
        // What each row slot shows: its tile, or for a tabbed container whose active tab
        // has no window yet (just launched), another tab that has one.
        let slots: std::collections::HashMap<TileId, TileId> = self
            .columns
            .iter()
            .flat_map(|c| c.tiles())
            .filter_map(|t| {
                let shown = if has_window(t) { Some(t) } else { self.group(t)?.iter().copied().find(|m| has_window(*m)) };
                shown.map(|s| (t, s))
            })
            .collect();
        let pick = |t: TileId| slots.get(&t).copied();
        let visible = &|t: TileId| pick(t).is_some();
        let inner_h = area.h - 2 * gap;
        // Columns at strip coordinates: (x, width, column index).
        let mut placed = Vec::new();
        let mut x = gap;
        for (c, col) in self.columns.iter().enumerate() {
            if !col.tiles().any(|t| visible(t)) {
                continue;
            }
            let w = (((area.w - gap) as f64 * col.width).round() as i32 - gap).max(1);
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

        if let Some((x, w, _)) = centered {
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
        for (x, w, c) in placed {
            let col = &self.columns[c];
            let shown: Vec<(usize, f64, bool)> = (0..col.rows.len())
                .filter(|r| col.rows[*r].tiles.iter().any(|t| visible(*t)))
                .map(|r| (r, col.heights[r], col.tabs[r]))
                .collect();
            let total: f64 = shown.iter().filter(|(_, _, tab)| !tab).map(|(_, h, _)| h).sum();
            let tab_bars = shown.iter().filter(|(_, _, tab)| *tab).count() as i32;
            let space = inner_h - sep * (shown.len() as i32 - 1) - tab_bars * TAB_HEIGHT;
            let last_open = shown.iter().rposition(|(_, _, tab)| !tab);
            let mut y = area.y + gap;
            for (i, (r, h, tab)) in shown.iter().enumerate() {
                let th = if *tab {
                    TAB_HEIGHT
                } else if Some(i) == last_open {
                    // The last open row takes what rounding left, minus the tabs below it.
                    let below = shown[i + 1..].len() as i32;
                    area.y + gap + inner_h - y - below * (TAB_HEIGHT + sep)
                } else {
                    (space as f64 * h / total).round() as i32
                };
                for (t, rect) in row_geometries(&col.rows[*r], Rect::new(area.x + x - self.view, y, w, th), sep, visible) {
                    let shown = pick(t).unwrap();
                    let tabs: Vec<TileId> = self.group(t).into_iter().flatten().copied().filter(|m| has_window(*m)).collect();
                    // A row folded into an accordion tab shows its own bar, not the tabs'.
                    if tabs.len() > 1 && !tab {
                        let (bar, content) = split_tab_bar(rect);
                        bars.push(TabBar { rect: bar, tabs, active: shown });
                        out.push((shown, content));
                    } else {
                        out.push((shown, rect));
                    }
                }
                y += th + sep;
            }
        }
        self.bars = bars;
        out
    }
}

/// The tiles of `row` side by side in `rect`, `sep` pixels apart.
fn row_geometries(row: &Row, rect: Rect, sep: i32, visible: &dyn Fn(TileId) -> bool) -> Vec<(TileId, Rect)> {
    let shown: Vec<(TileId, f64)> =
        row.tiles.iter().zip(&row.widths).filter(|(t, _)| visible(**t)).map(|(t, w)| (*t, *w)).collect();
    let total: f64 = shown.iter().map(|(_, w)| w).sum();
    let space = rect.w - sep * (shown.len() as i32 - 1);
    let mut x = rect.x;
    let mut out = Vec::new();
    for (i, (t, w)) in shown.iter().enumerate() {
        // The last tile takes what rounding left.
        let tw = if i + 1 == shown.len() { rect.x + rect.w - x } else { (space as f64 * w / total).round() as i32 };
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
        assert_eq!(g[0].1.h, TAB_HEIGHT);
        assert_eq!(g[1].1.h, g[2].1.h);
        // Closing the stacked tile brings the target back.
        l.remove(3);
        assert!(!l.is_tab(1));
        assert!(!l.stack_on(4, 99));
    }

    #[test]
    fn row_width_resizes_inside_the_row() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Right);
        l.resize_row_width(1, 0.2);
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, 0, 700, 600)), (2, Rect::new(700, 0, 300, 600))]);
        // Never below the minimum, and a lone tile has nothing to share.
        l.resize_row_width(2, -1.0);
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, 0, 900, 600)), (2, Rect::new(900, 0, 100, 600))]);
        l.remove(2);
        assert!(!l.shares_row(1));
        l.resize_row_width(1, -0.2);
        assert_eq!(geo(&mut l, 1), vec![(1, AREA)]);
    }

    #[test]
    fn row_width_keeps_every_tile_above_the_minimum() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Right);
        l.insert_on(3, Some(2), Side::Right);
        l.resize_row_width(3, -0.2);
        l.resize_row_width(1, 1.0);
        let widths: Vec<i32> = geo(&mut l, 1).iter().map(|(_, r)| r.w).collect();
        assert_eq!(widths, vec![800, 100, 100]);
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
        assert_eq!(g[0], (1, Rect::new(0, 0, 500, TAB_HEIGHT)));
        assert_eq!(g[1], (2, Rect::new(500, 0, 500, TAB_HEIGHT)));
        assert_eq!(g[2], (3, Rect::new(0, TAB_HEIGHT, 1000, 600 - TAB_HEIGHT)));
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
        l.resize_width(1, -5.0);
        assert_eq!(geo(&mut l, 1)[0].1.w, 100);
        l.cycle_width(1); // 0.1 -> 1/3
        assert_eq!(geo(&mut l, 1)[0].1.w, 333);
        l.cycle_width(1); // -> 1/2
        assert_eq!(geo(&mut l, 1)[0].1.w, 500);
        l.toggle_full_width(1);
        assert_eq!(geo(&mut l, 1)[0], (1, Rect::new(0, 0, 1000, 600)));
        l.toggle_full_width(1);
        assert_eq!(geo(&mut l, 1)[0].1.w, 500);
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
        assert_eq!(geo(&mut l, 1), vec![(1, Rect::new(0, 0, 1000, 576)), (2, Rect::new(0, 576, 1000, TAB_HEIGHT))]);
        // Opening the tab folds the other one.
        l.open_tab(2);
        assert!(l.is_tab(1) && !l.is_tab(2));
        assert_eq!(geo(&mut l, 2), vec![(1, Rect::new(0, 0, 1000, TAB_HEIGHT)), (2, Rect::new(0, 24, 1000, 576))]);
        // Shrinking opens every tab again, evenly.
        l.resize_height(2, -0.05);
        assert!(!l.is_tab(1));
        assert_eq!(geo(&mut l, 2), vec![(1, Rect::new(0, 0, 1000, 300)), (2, Rect::new(0, 300, 1000, 300))]);
        // Closing the open tile of a column of tabs opens one.
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
        assert_eq!(l.group(1), Some(&[1, 3, 4][..]));
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
        assert_eq!(l.group(2), Some(&[1, 3, 2][..]));
        assert_eq!(geo(&mut l, 2)[0].0, 2);
        let home = l.home(3).unwrap();
        assert_eq!(home, Home::Tab { with: 2, index: 1 });
        l.remove(3);
        assert!(l.restore(3, home));
        assert_eq!(l.group(1), Some(&[1, 3, 2][..]));
        assert_eq!(l.active_tab(1), Some(3));
    }

    #[test]
    fn tabs_inside_an_accordion_stack_and_a_row() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert_on(2, Some(1), Side::Right);
        assert!(l.add_tab(3, 2));
        let g = geo(&mut l, 3);
        assert_eq!(g[1], (3, Rect::new(500, TAB_HEIGHT, 500, 600 - TAB_HEIGHT)));
        // Stacking on a tabbed container folds it (bar of the accordion, no tab bar).
        assert!(l.stack_on(5, 3));
        let g = geo(&mut l, 5);
        assert_eq!(g.len(), 3);
        assert!(l.tab_bars().is_empty());
        assert!(l.is_tab(2) && l.is_tab(3));
    }

    #[test]
    fn tab_bar_layout() {
        let rect = Rect::new(10, 20, 300, 200);
        assert_eq!(split_tab_bar(rect), (Rect::new(10, 20, 300, TAB_HEIGHT), Rect::new(10, 20 + TAB_HEIGHT, 300, 200 - TAB_HEIGHT)));
        let bar = Rect::new(0, 0, 1000, TAB_HEIGHT);
        assert_eq!(tab_rects(bar, 2), vec![Rect::new(0, 0, TAB_MAX_WIDTH, TAB_HEIGHT), Rect::new(TAB_MAX_WIDTH, 0, TAB_MAX_WIDTH, TAB_HEIGHT)]);
        assert_eq!(plus_rect(bar, 2), Rect::new(2 * TAB_MAX_WIDTH, 0, PLUS_WIDTH, TAB_HEIGHT));
        // A full bar: the tabs share what the "+" leaves, the "+" at the right end.
        let full = Rect::new(0, 0, 300 + PLUS_WIDTH, TAB_HEIGHT);
        assert_eq!(tab_rects(full, 3)[2], Rect::new(200, 0, 100, TAB_HEIGHT));
        assert_eq!(plus_rect(full, 3), Rect::new(300, 0, PLUS_WIDTH, TAB_HEIGHT));
        assert!(tab_rects(bar, 0).is_empty());
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
