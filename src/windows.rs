//! The windows dune-finder lists for Mod+/ (a `windows` request): every window, on every
//! monitor and desktop, in every tab and folded row, the most recently focused first (the
//! focused one last: Enter right away goes back to the window before). Choosing one
//! (`goto`) shows the window's desktop and focuses it; chosen for an empty container
//! (Mod+Enter, then `/` or Mod+/), the window moves into it instead.

use dune_common::protocol::WindowInfo;

use crate::{Dune, layout::TileId};

impl Dune {
    /// Show the desktop `id` is on and focus it: its tab shows, its row opens and the view
    /// slides to it (see `arrange`).
    pub fn go_to(&mut self, id: TileId) {
        if !self.tiles.contains_key(&id) {
            return;
        }
        let slot = self.finder.as_mut().and_then(|r| r.slot.take()).map(|(slot, _)| slot);
        if let Some(slot) = slot.filter(|s| *s != id && self.tiles.get(s).is_some_and(|t| t.stack.is_empty())) {
            return self.fill_slot(id, slot);
        }
        if let Some((output, desk)) = self.hidden_desk(id) {
            self.show_desk_on(&output, desk);
        }
        self.focus_tile(id);
    }

    /// `id` leaves wherever it is (any monitor, desktop, tab or float) and takes the place
    /// of the empty container `slot`, focused.
    fn fill_slot(&mut self, id: TileId, slot: TileId) {
        self.remove_from_layouts(id);
        for layout in self.layouts.values_mut() {
            layout.swap(slot, id);
        }
        for (t, _) in self.floating.values_mut().flatten() {
            if *t == slot {
                *t = id;
            }
        }
        self.tiles.remove(&slot);
        self.focus_tile(id);
    }

    /// Every tile with a window, on every monitor and desktop: the most recently focused
    /// first, then the rest monitor by monitor, desktop by desktop; the focused one last.
    pub fn window_list(&self) -> Vec<WindowInfo> {
        let outputs = self.numbered_outputs();
        let mut placed: Vec<(TileId, String)> = Vec::new();
        for (m, output) in outputs.iter().enumerate() {
            let name = output.name();
            let monitor = if outputs.len() > 1 { format!(" · m{}", m + 1) } else { String::new() };
            let shown = self.desk_of(&name);
            for desk in 1..=crate::desktops::DESKS {
                let place = format!("desk {desk}{monitor}");
                let tiles = if desk == shown {
                    let floats = self.floating.get(&name).into_iter().flatten().map(|(t, _)| *t);
                    floats.chain(self.layouts.get(&name).map(|l| l.tiles()).unwrap_or_default()).collect()
                } else {
                    self.hidden.get(&(name.clone(), desk)).map(|d| d.tiles()).unwrap_or_default()
                };
                placed.extend(tiles.into_iter().map(|t| (t, place.clone())));
            }
        }
        placed.retain(|(t, _)| self.tiles.get(t).is_some_and(|tile| tile.top().is_some()));
        let rank = |t: TileId| match self.recent.iter().position(|r| *r == t) {
            Some(_) if self.focused == Some(t) => usize::MAX,
            Some(i) => i,
            None => usize::MAX - 1,
        };
        placed.sort_by_key(|(t, _)| rank(*t));
        placed
            .into_iter()
            .map(|(tile, place)| WindowInfo {
                tile,
                title: self.tile_title(tile),
                app: self.tile_app_id(tile).unwrap_or_default(),
                place,
            })
            .collect()
    }
}
