//! Desktops: every monitor has nine (1-9) and shows one of them. The desktop a monitor
//! shows is its entry in `Dune::layouts` and `Dune::floating`, as before desktops
//! existed; the others wait in `Dune::hidden` with their windows unmapped.

use smithay::output::Output;

use crate::{
    Dune,
    floating::centered,
    layout::{Layout, Rect, Side, TileId},
};

/// Desktops per monitor.
pub const DESKS: u8 = 9;

/// A desktop that is not shown: its strip of columns and its floats.
#[derive(Debug, Default)]
pub struct Desk {
    pub layout: Layout,
    pub floating: Vec<(TileId, Rect)>,
    /// The tile focused when it was left: focus goes back to it.
    pub focused: Option<TileId>,
}

impl Desk {
    fn tiles(&self) -> Vec<TileId> {
        self.floating.iter().map(|(t, _)| *t).chain(self.layout.tiles()).collect()
    }

    fn is_empty(&self) -> bool {
        self.floating.is_empty() && self.layout.tiles().is_empty()
    }

    fn contains(&self, id: TileId) -> bool {
        self.layout.contains(id) || self.floating.iter().any(|(t, _)| *t == id)
    }

    fn remove(&mut self, id: TileId) {
        self.layout.remove(id);
        self.floating.retain(|(t, _)| *t != id);
    }

    /// Add `id`: next to `target` (at the end without one) on `side`, in a column of its
    /// own without a side, or floating at `float`.
    fn add(&mut self, id: TileId, target: Option<TileId>, side: Option<Side>, float: Option<Rect>) {
        let target = target.filter(|t| self.layout.contains(*t));
        match (float, side) {
            (Some(rect), _) => self.floating.push((id, rect)),
            (None, Some(side)) => self.layout.insert_on(id, target, side),
            (None, None) => self.layout.insert_column(id, target, Side::Right),
        }
    }

    /// Everything of `other` goes after what is here.
    fn merge(&mut self, other: Desk) {
        self.floating.extend(other.floating);
        for id in other.layout.tiles() {
            self.add(id, None, None, None);
        }
    }
}

impl Dune {
    /// The desktop `output` shows.
    pub fn desk_of(&self, output: &str) -> u8 {
        self.desks.get(output).copied().unwrap_or(1)
    }

    /// The monitor and desktop of a tile on a desktop that is not shown.
    pub fn hidden_desk(&self, id: TileId) -> Option<(String, u8)> {
        self.hidden.iter().find(|(_, desk)| desk.contains(id)).map(|(key, _)| key.clone())
    }

    /// Monitors numbered from 1, left to right (then top to bottom).
    pub fn numbered_outputs(&self) -> Vec<Output> {
        let mut outputs: Vec<(Output, (i32, i32))> = self
            .space
            .outputs()
            .filter_map(|o| self.space.output_geometry(o).map(|g| (o.clone(), (g.loc.x, g.loc.y))))
            .collect();
        outputs.sort_by_key(|(_, loc)| *loc);
        outputs.into_iter().map(|(o, _)| o).collect()
    }

    /// Mod+N: show desktop `n` on the active monitor.
    pub fn show_desk(&mut self, n: u8) {
        let Some(output) = self.active_output().map(|o| o.name()) else { return };
        self.show_desk_on(&output, n);
    }

    /// Make `output` show its desktop `n`.
    pub fn show_desk_on(&mut self, output: &str, n: u8) {
        let output = output.to_string();
        if !self.space.outputs().any(|o| o.name() == output) {
            return;
        }
        let current = self.desk_of(&output);
        if current == n || !(1..=DESKS).contains(&n) {
            return;
        }
        let mut leaving = self.take_shown(&output);
        leaving.focused = self.focused.filter(|f| leaving.contains(*f));
        self.hide_windows(&leaving.tiles());
        if !leaving.is_empty() {
            self.hidden.insert((output.clone(), current), leaving);
        }
        let coming = self.hidden.remove(&(output.clone(), n)).unwrap_or_default();
        let back = coming.focused.filter(|f| coming.contains(*f)).or_else(|| coming.tiles().first().copied());
        self.show(&output, coming);
        self.desks.insert(output.clone(), n);
        tracing::info!(output, desk = n, "desktop shown");

        // Focus goes to the desktop shown: the tile focused there last. An empty desktop
        // leaves it where it is, unless that was here.
        if back.is_some() || self.focused.is_none_or(|f| self.hidden_desk(f).is_some()) {
            self.focused = back;
        }
        self.arrange();
        self.update_focus();
    }

    /// Mod+Shift+N: send the focused tile to desktop `n` of its monitor. Focus stays here.
    pub fn send_to_desk(&mut self, n: u8) {
        let Some(id) = self.focused else { return };
        let Some(output) = self.output_of(id) else { return };
        if self.desk_of(&output) == n || !(1..=DESKS).contains(&n) {
            return;
        }
        let float = self.float_rect(id);
        let next = self.neighbor_of(id);
        self.remove_from_layouts(id);
        self.hidden.entry((output, n)).or_default().add(id, None, None, float);
        self.hide_windows(&[id]);
        self.focused = next;
        self.arrange();
        self.update_focus();
    }

    /// Put the new tile `id` on desktop `n` of `output`: next to `target` (at the end
    /// without one) on `side`, or floating in the middle.
    pub fn place_on_desk(&mut self, id: TileId, output: &str, n: u8, target: Option<TileId>, side: Option<Side>, float: bool) {
        let float = float.then(|| self.start_float_rect(output)).flatten();
        if self.desk_of(output) == n {
            match float {
                Some(rect) => self.floating.entry(output.to_string()).or_default().push((id, rect)),
                None => {
                    let layout = self.layouts.entry(output.to_string()).or_default();
                    let target = target.filter(|t| layout.contains(*t));
                    match side {
                        Some(side) => layout.insert_on(id, target, side),
                        None => layout.insert_column(id, target, Side::Right),
                    }
                }
            }
        } else {
            self.hidden.entry((output.to_string(), n)).or_default().add(id, target, side, float);
        }
    }

    /// A tile that went away leaves its hidden desktop too.
    pub fn remove_from_hidden(&mut self, id: TileId) {
        for desk in self.hidden.values_mut() {
            desk.remove(id);
        }
        self.hidden.retain(|_, desk| !desk.is_empty());
    }

    /// The desktops of monitors that went away join those of `first` with the same number.
    pub fn adopt_desks(&mut self, outputs: &[Output], first: &str) {
        let gone: Vec<(String, u8)> =
            self.hidden.keys().filter(|(o, _)| !outputs.iter().any(|out| out.name() == *o)).cloned().collect();
        for key in gone {
            let desk = self.hidden.remove(&key).unwrap();
            tracing::info!(output = key.0, desk = key.1, "monitor gone, its desktop moves to {first}");
            if self.desk_of(first) == key.1 {
                let mut shown = self.take_shown(first);
                shown.merge(desk);
                self.show(first, shown);
            } else {
                self.hidden.entry((first.to_string(), key.1)).or_default().merge(desk);
            }
        }
        self.desks.retain(|o, _| outputs.iter().any(|out| out.name() == *o));
    }

    /// Where a tile that starts floating on `output` goes: in the middle.
    fn start_float_rect(&self, output: &str) -> Option<Rect> {
        let area = self.space.outputs().find(|o| o.name() == output).and_then(|o| self.output_rect(o))?;
        let (w, h) = crate::floating::start_size(area);
        Some(centered(area, w, h))
    }

    fn take_shown(&mut self, output: &str) -> Desk {
        Desk {
            layout: self.layouts.remove(output).unwrap_or_default(),
            floating: self.floating.remove(output).unwrap_or_default(),
            focused: None,
        }
    }

    fn show(&mut self, output: &str, desk: Desk) {
        self.layouts.insert(output.to_string(), desk.layout);
        if !desk.floating.is_empty() {
            self.floating.insert(output.to_string(), desk.floating);
        }
    }

    /// Take the windows of `ids` off the screen; `arrange` maps them again once shown.
    pub fn hide_windows(&mut self, ids: &[TileId]) {
        for id in ids {
            for window in self.tiles.get(id).map(|t| t.stack.clone()).unwrap_or_default() {
                self.space.unmap_elem(&window);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desk_keeps_strip_and_floats() {
        let mut desk = Desk::default();
        assert!(desk.is_empty());
        desk.add(1, None, Some(Side::Right), None);
        desk.add(2, Some(1), Some(Side::Down), None);
        desk.add(3, None, Some(Side::Right), Some(Rect::new(0, 0, 10, 10)));
        assert_eq!(desk.tiles(), vec![3, 1, 2]);
        assert!(desk.contains(2) && desk.contains(3));
        desk.remove(3);
        desk.remove(1);
        assert_eq!(desk.tiles(), vec![2]);
        desk.remove(2);
        assert!(desk.is_empty());
    }

    #[test]
    fn merged_desks_keep_every_tile() {
        let mut a = Desk::default();
        a.add(1, None, Some(Side::Right), None);
        let mut b = Desk::default();
        b.add(2, None, Some(Side::Right), None);
        b.add(3, Some(2), Some(Side::Down), None);
        b.add(4, None, Some(Side::Right), Some(Rect::new(0, 0, 10, 10)));
        a.merge(b);
        let mut tiles = a.tiles();
        tiles.sort();
        assert_eq!(tiles, vec![1, 2, 3, 4]);
    }
}
