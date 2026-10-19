//! Pure tiling layout: a binary tree of splits whose leaves are tile ids.
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
}

#[derive(Debug)]
enum Node {
    Leaf(TileId),
    Split {
        split: Split,
        /// Share of the space given to `a`, in (0, 1).
        ratio: f64,
        a: Box<Node>,
        b: Box<Node>,
    },
}

impl Node {
    fn contains(&self, id: TileId) -> bool {
        match self {
            Node::Leaf(t) => *t == id,
            Node::Split { a, b, .. } => a.contains(id) || b.contains(id),
        }
    }

    fn any_visible(&self, visible: &dyn Fn(TileId) -> bool) -> bool {
        match self {
            Node::Leaf(t) => visible(*t),
            Node::Split { a, b, .. } => a.any_visible(visible) || b.any_visible(visible),
        }
    }
}

#[derive(Debug, Default)]
pub struct Layout {
    root: Option<Node>,
}

const MIN_RATIO: f64 = 0.1;
const MAX_RATIO: f64 = 0.9;

impl Layout {
    pub fn contains(&self, id: TileId) -> bool {
        self.root.as_ref().is_some_and(|n| n.contains(id))
    }

    /// Ids of all tiles, in tree order (left/top first).
    pub fn tiles(&self) -> Vec<TileId> {
        fn walk(node: &Node, out: &mut Vec<TileId>) {
            match node {
                Node::Leaf(t) => out.push(*t),
                Node::Split { a, b, .. } => {
                    walk(a, out);
                    walk(b, out);
                }
            }
        }
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            walk(root, &mut out);
        }
        out
    }

    /// Insert `new` next to `target` (after it), dividing `target`'s space with `split`.
    /// Without a (known) target the whole layout is split.
    pub fn insert(&mut self, new: TileId, target: Option<TileId>, split: Split) {
        self.insert_side(new, target, split, false);
    }

    /// Insert `new` on a side of `target`: `before` puts it left of / above it.
    pub fn insert_on(&mut self, new: TileId, target: Option<TileId>, side: Side) {
        self.insert_side(new, target, side.split(), side.before());
    }

    fn insert_side(&mut self, new: TileId, target: Option<TileId>, split: Split, before: bool) {
        let pair = |old: Node, new: Node| {
            let (a, b) = if before { (new, old) } else { (old, new) };
            Node::Split {
                split,
                ratio: 0.5,
                a: Box::new(a),
                b: Box::new(b),
            }
        };
        fn split_leaf(node: &mut Node, target: TileId, make: &dyn Fn(Node) -> Node) -> bool {
            match node {
                Node::Leaf(t) if *t == target => {
                    *node = make(Node::Leaf(target));
                    true
                }
                Node::Leaf(_) => false,
                Node::Split { a, b, .. } => split_leaf(a, target, make) || split_leaf(b, target, make),
            }
        }
        let make = |old: Node| pair(old, Node::Leaf(new));

        let Some(root) = self.root.as_mut() else {
            self.root = Some(Node::Leaf(new));
            return;
        };
        if let Some(target) = target
            && split_leaf(root, target, &make)
        {
            return;
        }
        let old = self.root.take().unwrap();
        self.root = Some(make(old));
    }

    /// Remove a tile; its sibling takes over the parent's space.
    pub fn remove(&mut self, id: TileId) -> bool {
        fn remove_from(node: Node, id: TileId, removed: &mut bool) -> Option<Node> {
            match node {
                Node::Leaf(t) if t == id => {
                    *removed = true;
                    None
                }
                leaf @ Node::Leaf(_) => Some(leaf),
                Node::Split { split, ratio, a, b } => {
                    let a = remove_from(*a, id, removed);
                    let b = remove_from(*b, id, removed);
                    match (a, b) {
                        (Some(a), Some(b)) => Some(Node::Split {
                            split,
                            ratio,
                            a: Box::new(a),
                            b: Box::new(b),
                        }),
                        (Some(n), None) | (None, Some(n)) => Some(n),
                        (None, None) => None,
                    }
                }
            }
        }

        let mut removed = false;
        if let Some(root) = self.root.take() {
            self.root = remove_from(root, id, &mut removed);
        }
        removed
    }

    /// Exchange the positions of two tiles.
    pub fn swap(&mut self, x: TileId, y: TileId) {
        fn walk(node: &mut Node, x: TileId, y: TileId) {
            match node {
                Node::Leaf(t) if *t == x => *t = y,
                Node::Leaf(t) if *t == y => *t = x,
                Node::Leaf(_) => {}
                Node::Split { a, b, .. } => {
                    walk(a, x, y);
                    walk(b, x, y);
                }
            }
        }
        if let Some(root) = self.root.as_mut() {
            walk(root, x, y);
        }
    }

    /// Grow (positive `delta`) or shrink the tile by moving the divider of its closest split.
    pub fn resize(&mut self, id: TileId, delta: f64) {
        fn walk(node: &mut Node, id: TileId, delta: f64) -> bool {
            let Node::Split { ratio, a, b, .. } = node else {
                return false;
            };
            if walk(a, id, delta) || walk(b, id, delta) {
                return true;
            }
            let sign = if matches!(**a, Node::Leaf(t) if t == id) {
                1.0
            } else if matches!(**b, Node::Leaf(t) if t == id) {
                -1.0
            } else {
                return false;
            };
            *ratio = (*ratio + sign * delta).clamp(MIN_RATIO, MAX_RATIO);
            true
        }
        if let Some(root) = self.root.as_mut() {
            walk(root, id, delta);
        }
    }

    /// Compute the rectangle of every visible tile inside `area`, leaving `gap` pixels
    /// around and between tiles. Invisible tiles (e.g. a terminal that has not mapped
    /// its window yet) take no space.
    pub fn geometries(&self, area: Rect, gap: i32, visible: &dyn Fn(TileId) -> bool) -> Vec<(TileId, Rect)> {
        fn walk(node: &Node, area: Rect, gap: i32, visible: &dyn Fn(TileId) -> bool, out: &mut Vec<(TileId, Rect)>) {
            match node {
                Node::Leaf(t) => {
                    if visible(*t) {
                        out.push((*t, area));
                    }
                }
                Node::Split { split, ratio, a, b } => {
                    match (a.any_visible(visible), b.any_visible(visible)) {
                        (true, true) => {}
                        (true, false) => return walk(a, area, gap, visible, out),
                        (false, true) => return walk(b, area, gap, visible, out),
                        (false, false) => return,
                    }
                    let (ra, rb) = match split {
                        Split::Vertical => {
                            let wa = ((area.w - gap) as f64 * ratio).round() as i32;
                            (
                                Rect::new(area.x, area.y, wa, area.h),
                                Rect::new(area.x + wa + gap, area.y, area.w - wa - gap, area.h),
                            )
                        }
                        Split::Horizontal => {
                            let ha = ((area.h - gap) as f64 * ratio).round() as i32;
                            (
                                Rect::new(area.x, area.y, area.w, ha),
                                Rect::new(area.x, area.y + ha + gap, area.w, area.h - ha - gap),
                            )
                        }
                    };
                    walk(a, ra, gap, visible, out);
                    walk(b, rb, gap, visible, out);
                }
            }
        }

        let mut out = Vec::new();
        if let Some(root) = &self.root {
            let inner = Rect::new(area.x + gap, area.y + gap, area.w - 2 * gap, area.h - 2 * gap);
            walk(root, inner, gap, visible, &mut out);
        }
        out
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

    #[test]
    fn first_tile_fills_area() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        assert_eq!(l.geometries(AREA, 0, &all), vec![(1, AREA)]);
    }

    #[test]
    fn vertical_split_is_side_by_side() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        assert_eq!(
            l.geometries(AREA, 0, &all),
            vec![(1, Rect::new(0, 0, 500, 600)), (2, Rect::new(500, 0, 500, 600))]
        );
    }

    #[test]
    fn horizontal_split_is_stacked_with_gaps() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Horizontal);
        assert_eq!(
            l.geometries(AREA, 10, &all),
            vec![(1, Rect::new(10, 10, 980, 285)), (2, Rect::new(10, 305, 980, 285))]
        );
    }

    #[test]
    fn nested_split_only_divides_target() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        l.insert(3, Some(2), Split::Horizontal);
        assert_eq!(
            l.geometries(AREA, 0, &all),
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
        assert_eq!(l.geometries(AREA, 0, &|t| t != 2), vec![(1, AREA)]);
    }

    #[test]
    fn remove_collapses_parent() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        l.insert(3, Some(2), Split::Horizontal);
        assert!(l.remove(2));
        assert!(!l.remove(42));
        assert_eq!(l.tiles(), vec![1, 3]);
        assert_eq!(l.geometries(AREA, 0, &all)[1], (3, Rect::new(500, 0, 500, 600)));
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
            l.geometries(AREA, 0, &all)
        };
        let (left, right) = (Rect::new(0, 0, 500, 600), Rect::new(500, 0, 500, 600));
        let (top, bottom) = (Rect::new(0, 0, 1000, 300), Rect::new(0, 300, 1000, 300));
        assert_eq!(place(Side::Right), vec![(1, left), (2, right)]);
        assert_eq!(place(Side::Left), vec![(2, left), (1, right)]);
        assert_eq!(place(Side::Down), vec![(1, top), (2, bottom)]);
        assert_eq!(place(Side::Up), vec![(2, top), (1, bottom)]);
    }

    #[test]
    fn resize_and_swap() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        l.resize(1, 0.1);
        assert_eq!(l.geometries(AREA, 0, &all)[0], (1, Rect::new(0, 0, 600, 600)));
        l.resize(2, 0.1); // growing the second tile moves the divider back
        assert_eq!(l.geometries(AREA, 0, &all)[0], (1, Rect::new(0, 0, 500, 600)));
        l.swap(1, 2);
        assert_eq!(l.tiles(), vec![2, 1]);
    }

    #[test]
    fn directional_neighbor() {
        let mut l = Layout::default();
        l.insert(1, None, Split::Vertical);
        l.insert(2, Some(1), Split::Vertical);
        l.insert(3, Some(2), Split::Horizontal);
        let g = l.geometries(AREA, 0, &all);
        assert_eq!(neighbor(&g, 1, Direction::Right), Some(2));
        assert_eq!(neighbor(&g, 3, Direction::Left), Some(1));
        assert_eq!(neighbor(&g, 2, Direction::Down), Some(3));
        assert_eq!(neighbor(&g, 3, Direction::Up), Some(2));
        assert_eq!(neighbor(&g, 1, Direction::Left), None);
    }
}
