//! Pure tiling layout: a strip of columns that scrolls horizontally without end (as in
//! niri). Tiles stacked in a column share the height of the output; columns sit side by
//! side and the view slides along the strip to show the focused one.
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
/// A tile squeezed below this share of its column by a neighbour growing becomes a tab.
const TAB_THRESHOLD: f64 = 0.15;
/// Height of a tab's bar, in pixels.
pub const TAB_HEIGHT: i32 = 24;

#[derive(Debug)]
struct Column {
    tiles: Vec<TileId>,
    /// Share of the column's height of each tile; they add up to 1.
    heights: Vec<f64>,
    /// Tiles folded into a tab: a bar instead of their window.
    tabs: Vec<bool>,
    width: f64,
    /// Width to go back to when leaving full width.
    restore_width: Option<f64>,
}

impl Column {
    fn new(id: TileId, width: f64) -> Self {
        Column {
            tiles: vec![id],
            heights: vec![1.0],
            tabs: vec![false],
            width,
            restore_width: None,
        }
    }

    /// Put `id` at `index`, giving it an even share of the height.
    fn insert(&mut self, index: usize, id: TileId) {
        let share = 1.0 / (self.tiles.len() + 1) as f64;
        for h in &mut self.heights {
            *h *= 1.0 - share;
        }
        self.tiles.insert(index, id);
        self.heights.insert(index, share);
        self.tabs.insert(index, false);
    }

    fn remove(&mut self, index: usize) {
        self.tiles.remove(index);
        self.tabs.remove(index);
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

    /// Share of the height among the tiles that are not tabs, normalized to add up to 1.
    fn normalize_open(&mut self) {
        let total: f64 = self.heights.iter().zip(&self.tabs).filter(|(_, t)| !**t).map(|(h, _)| h).sum();
        if total > 0.0 {
            for (h, _) in self.heights.iter_mut().zip(&self.tabs).filter(|(_, t)| !**t) {
                *h /= total;
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct Layout {
    columns: Vec<Column>,
    /// How far the view has slid along the strip, in pixels.
    view: i32,
}

impl Layout {
    pub fn contains(&self, id: TileId) -> bool {
        self.find(id).is_some()
    }

    /// Column and row of a tile.
    fn find(&self, id: TileId) -> Option<(usize, usize)> {
        self.columns
            .iter()
            .enumerate()
            .find_map(|(c, col)| col.tiles.iter().position(|t| *t == id).map(|r| (c, r)))
    }

    fn column_of(&mut self, id: TileId) -> Option<&mut Column> {
        let (c, _) = self.find(id)?;
        Some(&mut self.columns[c])
    }

    /// Ids of all tiles, column by column from the left, top to bottom.
    pub fn tiles(&self) -> Vec<TileId> {
        self.columns.iter().flat_map(|c| c.tiles.iter().copied()).collect()
    }

    /// Insert `new` after `target`: in a new column to its right (`Vertical`) or below it
    /// in its column (`Horizontal`). Without a (known) target it goes at the end.
    pub fn insert(&mut self, new: TileId, target: Option<TileId>, split: Split) {
        let side = match split {
            Split::Vertical => Side::Right,
            Split::Horizontal => Side::Down,
        };
        self.insert_on(new, target, side);
    }

    /// Insert `new` on a side of `target`: left/right open a new column, up/down share
    /// the target's column.
    pub fn insert_on(&mut self, new: TileId, target: Option<TileId>, side: Side) {
        if self.columns.is_empty() {
            // A lone tile fills the output; the second column splits it in halves.
            self.columns.push(Column::new(new, MAX_WIDTH));
            return;
        }
        let (c, r) = match target.and_then(|t| self.find(t)) {
            Some(found) => found,
            None => {
                let c = self.columns.len() - 1;
                (c, self.columns[c].tiles.len() - 1)
            }
        };
        match side {
            Side::Up | Side::Down => {
                let r = if side.before() { r } else { r + 1 };
                self.columns[c].insert(r, new);
            }
            Side::Left | Side::Right => {
                if self.columns.len() == 1 && self.columns[0].width == MAX_WIDTH {
                    self.columns[0].width = DEFAULT_WIDTH;
                }
                let c = if side.before() { c } else { c + 1 };
                self.columns.insert(c, Column::new(new, DEFAULT_WIDTH));
            }
        }
    }

    /// Remove a tile; an emptied column goes away and the strip closes the gap.
    pub fn remove(&mut self, id: TileId) -> bool {
        let Some((c, r)) = self.find(id) else { return false };
        self.columns[c].remove(r);
        if self.columns[c].tiles.is_empty() {
            self.columns.remove(c);
        }
        true
    }

    /// Exchange the positions of two tiles.
    pub fn swap(&mut self, x: TileId, y: TileId) {
        for t in self.columns.iter_mut().flat_map(|c| c.tiles.iter_mut()) {
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

    /// Make the tile taller (positive `delta`) or shorter, as a share of its column; the
    /// other tiles of the column give or take the difference. Tiles squeezed below
    /// `TAB_THRESHOLD` fold into tabs; shrinking a tile opens the column's tabs again.
    pub fn resize_height(&mut self, id: TileId, delta: f64) {
        let Some((c, r)) = self.find(id) else { return };
        let col = &mut self.columns[c];
        if delta < 0.0 && col.tabs.iter().any(|t| *t) {
            let even = 1.0 / col.tiles.len() as f64;
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
                // Rounding noise must not fold a tile that is exactly at the threshold.
                *tab = *h < TAB_THRESHOLD - 1e-9;
            }
        }
        col.normalize_open();
    }

    pub fn is_tab(&self, id: TileId) -> bool {
        self.find(id).is_some_and(|(c, r)| self.columns[c].tabs[r])
    }

    /// Open a tab: it takes the place of the largest open tile of its column, which
    /// becomes a tab in turn.
    pub fn open_tab(&mut self, id: TileId) {
        let Some((c, r)) = self.find(id) else { return };
        let col = &mut self.columns[c];
        if !col.tabs[r] {
            return;
        }
        let largest = (0..col.tiles.len())
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

    /// Lay the strip out in `area`, leaving `gap` pixels around and between tiles, and
    /// slide the view so that `focus` (when it is here) is shown whole. Tiles off the
    /// view get rectangles outside `area`; tabs get the rectangle of their bar. Invisible
    /// tiles (e.g. a terminal that has not mapped its window yet) take no space.
    pub fn geometries(&mut self, area: Rect, gap: i32, visible: &dyn Fn(TileId) -> bool, focus: Option<TileId>) -> Vec<(TileId, Rect)> {
        let inner_h = area.h - 2 * gap;
        // Columns at strip coordinates: (x, width, column index).
        let mut placed = Vec::new();
        let mut x = gap;
        for (c, col) in self.columns.iter().enumerate() {
            if !col.tiles.iter().any(|t| visible(*t)) {
                continue;
            }
            let w = (((area.w - gap) as f64 * col.width).round() as i32 - gap).max(1);
            placed.push((x, w, c));
            x += w + gap;
        }
        let strip = x;

        if let Some((x, w, _)) = focus
            .and_then(|f| self.find(f))
            .and_then(|(fc, _)| placed.iter().find(|(_, _, c)| *c == fc))
        {
            if x - gap < self.view {
                self.view = x - gap;
            } else if x + w + gap > self.view + area.w {
                self.view = x + w + gap - area.w;
            }
        }
        // No empty space at either end while there is something to show.
        self.view = self.view.min(strip - area.w).max(0);

        let mut out = Vec::new();
        for (x, w, c) in placed {
            let col = &self.columns[c];
            let shown: Vec<(TileId, f64, bool)> = (0..col.tiles.len())
                .filter(|i| visible(col.tiles[*i]))
                .map(|i| (col.tiles[i], col.heights[i], col.tabs[i]))
                .collect();
            let total: f64 = shown.iter().filter(|(_, _, tab)| !tab).map(|(_, h, _)| h).sum();
            let tab_bars = shown.iter().filter(|(_, _, tab)| *tab).count() as i32;
            let space = inner_h - gap * (shown.len() as i32 - 1) - tab_bars * TAB_HEIGHT;
            let last_open = shown.iter().rposition(|(_, _, tab)| !tab);
            let mut y = area.y + gap;
            for (i, (t, h, tab)) in shown.iter().enumerate() {
                let th = if *tab {
                    TAB_HEIGHT
                } else if Some(i) == last_open {
                    // The last open tile takes what rounding left, minus the tabs below it.
                    let below = shown[i + 1..].len() as i32;
                    area.y + gap + inner_h - y - below * (TAB_HEIGHT + gap)
                } else {
                    (space as f64 * h / total).round() as i32
                };
                out.push((*t, Rect::new(area.x + x - self.view, y, w, th)));
                y += th + gap;
            }
        }
        out
    }
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
        l.geometries(AREA, 0, &all, Some(focus))
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
        assert_eq!(
            l.geometries(AREA, 10, &all, Some(1)),
            vec![(1, Rect::new(10, 10, 980, 285)), (2, Rect::new(10, 305, 980, 285))]
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
    fn invisible_tiles_take_no_space() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        l.insert(3, Some(1), Split::Horizontal);
        assert_eq!(l.geometries(AREA, 0, &|t| t == 1, Some(1)), vec![(1, Rect::new(0, 0, 500, 600))]);
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
}
