//! Private tiles and locked desktops, for screen sharing.
//!
//! A private tile is blocked on screen and in captures alike (`render::output_elements`
//! draws both), so what is shared is what the user sees; it shows as "🔒 <app>" in tab
//! bars and dune-finder's windows. Set by hand, from the tile options (Mod+/) or the
//! `private_tile` key.
//!
//! A locked desktop ("🔒 <n>" in bars) is never shown: a right click on it in the bar
//! (`deactivate` on its ext-workspace handle, see `workspaces`) locks it, another unlocks
//! it. Mod+N, the bar, dune-finder and window activation do not show it; locking the one
//! a monitor shows switches that monitor to another first. Its windows are left out of
//! dune-finder's list but for `:l`, which lists only them; choosing one there unlocks its
//! desktop and goes to it. Tiles can still be sent there. Config reloads keep both.

use std::collections::HashSet;

use dune_common::protocol::{TileAction, TileOption};

use crate::{Dune, config::Binding, desktops::DESKS, layout::TileId};

#[derive(Debug, Default)]
pub struct Privacy {
    /// Private tiles; one that goes away is forgotten (`forget_gone`).
    tiles: HashSet<TileId>,
    /// Locked desktops, by output name and number.
    locked: HashSet<(String, u8)>,
}

impl Privacy {
    pub fn tile(&self, id: TileId) -> bool {
        self.tiles.contains(&id)
    }

    /// Whether desktop `n` of `output` is locked.
    pub fn locked(&self, output: &str, n: u8) -> bool {
        self.locked.contains(&(output.to_string(), n))
    }

    /// Make `id` private, or not any more; whether it is now.
    pub fn toggle_tile(&mut self, id: TileId) -> bool {
        if self.tiles.remove(&id) {
            return false;
        }
        self.tiles.insert(id)
    }

    /// Lock desktop `n` of `output`, or unlock it; whether it is locked now.
    pub fn toggle_lock(&mut self, output: &str, n: u8) -> bool {
        let key = (output.to_string(), n);
        if self.locked.remove(&key) {
            return false;
        }
        self.locked.insert(key)
    }

    /// Forget the tiles that are gone (`alive` says no).
    pub fn forget_gone(&mut self, alive: impl Fn(TileId) -> bool) {
        self.tiles.retain(|t| alive(*t));
    }

    /// The desktops of `output` whose windows dune-finder lists: the unlocked ones, or
    /// with `locked` (`:l`) the locked ones only.
    pub fn listed_desks(&self, output: &str, locked: bool) -> Vec<u8> {
        (1..=DESKS).filter(|n| self.locked(output, *n) == locked).collect()
    }

    /// Whether a tile at `place` (its monitor and desktop, if it is on one) is on a
    /// locked desktop.
    pub fn holds(&self, place: Option<&(String, u8)>) -> bool {
        place.is_some_and(|(output, n)| self.locked(output, *n))
    }
}

/// Where a monitor showing desktop `current` goes when it is locked: the desktop it showed
/// before (`previous`) if that one is not `locked`, else the lowest one that is not;
/// `None` when every other one is.
pub fn jump_target(current: u8, previous: Option<u8>, locked: impl Fn(u8) -> bool) -> Option<u8> {
    let open = |n: &u8| *n != current && (1..=DESKS).contains(n) && !locked(*n);
    previous.filter(open).or_else(|| (1..=DESKS).find(open))
}

/// What tab bars and dune-finder's windows show for a private tile of `app`.
pub fn masked_title(app: Option<&str>) -> String {
    match app.filter(|a| !a.is_empty()) {
        Some(app) => format!("🔒 {app}"),
        None => "🔒 private".into(),
    }
}

/// The name bars show for desktop `n`.
pub fn workspace_name(n: u8, locked: bool) -> String {
    if locked { format!("🔒 {n}") } else { n.to_string() }
}

/// The binding doing what `action` does: its keys show in the menu.
pub fn binding_of(action: TileAction) -> Binding {
    match action {
        TileAction::PrivateTile => Binding::PrivateTile,
        TileAction::Float => Binding::Float,
        TileAction::Center => Binding::Center,
        TileAction::FullWidth => Binding::FullWidth,
        TileAction::ToTab => Binding::StackToTabs,
        TileAction::SendToDesk(n) => Binding::SendToDesk(n),
        TileAction::Close => Binding::Close,
    }
}

/// The rows of the tile options menu: every action for a tile (`tile`: whether it is
/// private), none without one. `keys` gives each one's keys.
pub fn option_rows(tile: Option<bool>, keys: impl Fn(TileAction) -> String) -> Vec<TileOption> {
    let Some(private) = tile else { return Vec::new() };
    TileAction::all()
        .into_iter()
        .map(|action| {
            let on = (action == TileAction::PrivateTile).then_some(private);
            TileOption { action, on, keys: keys(action) }
        })
        .collect()
}

impl Dune {
    /// What tab bars and dune-finder's windows call `id`: its title, or for a private tile
    /// its app (`masked_title`).
    pub fn shown_title(&self, id: TileId) -> String {
        if self.privacy.tile(id) {
            return masked_title(self.tile_app_id(id).as_deref());
        }
        self.tile_title(id)
    }

    /// The monitor and desktop `id` is on, shown or not.
    fn place_of(&self, id: TileId) -> Option<(String, u8)> {
        match self.output_of(id) {
            Some(output) => {
                let n = self.desk_of(&output);
                Some((output, n))
            }
            None => self.hidden_desk(id),
        }
    }

    /// Whether `id` is on a locked desktop: nothing goes to it.
    pub fn on_locked_desk(&self, id: TileId) -> bool {
        self.privacy.holds(self.place_of(id).as_ref())
    }

    /// Make `id` private, or show it again.
    pub fn toggle_private_tile(&mut self, id: TileId) {
        if !self.tiles.contains_key(&id) {
            return;
        }
        let private = self.privacy.toggle_tile(id);
        tracing::info!(tile = id, private, "private tile");
        self.arrange();
    }

    /// The `private_tile` key: the focused tile.
    pub fn toggle_private_focused(&mut self) {
        if let Some(id) = self.focused {
            self.toggle_private_tile(id);
        }
    }

    /// A right click on desktop `n` of `output` in the bar: lock it, or unlock it. The
    /// desktop shown there, locked, gives way first to the one shown before (or the lowest
    /// unlocked one), focus going along; with none left, it stays unlocked.
    pub fn toggle_desk_lock(&mut self, output: &str, n: u8) {
        if !(1..=DESKS).contains(&n) {
            return;
        }
        if !self.privacy.locked(output, n) && self.desk_of(output) == n {
            let previous = self.previous_desks.get(output).copied();
            let Some(to) = jump_target(n, previous, |d| self.privacy.locked(output, d)) else {
                return tracing::warn!(output, desk = n, "not locking the last unlocked desktop");
            };
            self.show_desk_on(output, to);
            if self.desk_of(output) == n {
                return tracing::warn!(output, desk = n, "desktop not left, not locked");
            }
        }
        let locked = self.privacy.toggle_lock(output, n);
        tracing::info!(output, desk = n, locked, "desktop lock");
        self.arrange();
    }

    /// A window chosen with `:l`: its desktop is unlocked, then shown with it focused.
    pub fn unlock_and_go(&mut self, id: TileId) {
        if let Some((output, n)) = self.place_of(id).filter(|p| self.privacy.holds(Some(p))) {
            self.toggle_desk_lock(&output, n);
        }
        self.go_to(id);
    }

    /// The answer to `tile_options`: the menu's rows for `tile` (gone or none: no rows),
    /// with their keys.
    pub fn tile_options(&self, tile: Option<TileId>) -> Vec<TileOption> {
        let tile = tile.filter(|t| self.tiles.contains_key(t) && !self.on_locked_desk(*t));
        let private = tile.map(|t| self.privacy.tile(t));
        option_rows(private, |action| self.config.key_label(binding_of(action)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_tiles_and_locked_desks_toggle() {
        let mut p = Privacy::default();
        assert!(p.toggle_tile(3) && p.tile(3));
        assert!(!p.toggle_tile(3) && !p.tile(3));
        assert!(p.toggle_lock("DP-1", 2) && p.locked("DP-1", 2));
        assert!(!p.locked("DP-1", 1) && !p.locked("HDMI-A-1", 2));
        assert!(!p.toggle_lock("DP-1", 2) && !p.locked("DP-1", 2));
    }

    #[test]
    fn a_closed_tile_is_no_longer_private() {
        let mut p = Privacy::default();
        p.toggle_tile(3);
        p.toggle_tile(4);
        p.toggle_lock("DP-1", 1);
        p.forget_gone(|t| t != 3);
        assert!(!p.tile(3) && p.tile(4));
        assert!(p.locked("DP-1", 1));
    }

    #[test]
    fn masked_titles_and_workspace_names() {
        assert_eq!(masked_title(Some("Slack")), "🔒 Slack");
        assert_eq!(masked_title(Some("")), "🔒 private");
        assert_eq!(masked_title(None), "🔒 private");
        assert_eq!(workspace_name(3, false), "3");
        assert_eq!(workspace_name(3, true), "🔒 3");
    }

    #[test]
    fn the_menu_has_every_action_for_a_tile_and_none_without() {
        let keys = |a: TileAction| if a == TileAction::Float { "super+shift+space".to_string() } else { String::new() };
        let rows = option_rows(Some(true), keys);
        assert_eq!(rows.len(), TileAction::all().len());
        assert_eq!(rows[0], TileOption { action: TileAction::PrivateTile, on: Some(true), keys: String::new() });
        assert_eq!(rows.iter().find(|r| r.action == TileAction::Float).map(|r| (r.on, r.keys.as_str())), Some((None, "super+shift+space")));
        assert!(rows[1..].iter().all(|r| r.on.is_none()));
        assert_eq!(option_rows(Some(false), keys)[0].on, Some(false));
        assert_eq!(option_rows(None, keys), []);
        assert_eq!(binding_of(TileAction::ToTab), Binding::StackToTabs);
    }

    #[test]
    fn tiles_on_locked_desktops_shown_or_not() {
        let mut p = Privacy::default();
        p.toggle_lock("DP-1", 2);
        assert!(p.holds(Some(&("DP-1".into(), 2))));
        assert!(!p.holds(Some(&("DP-1".into(), 1))) && !p.holds(Some(&("HDMI-A-1".into(), 2))));
        // A tile on no desktop (gone) is on no locked one.
        assert!(!p.holds(None));
    }

    #[test]
    fn the_windows_of_locked_desktops_are_listed_only_when_asked() {
        let mut p = Privacy::default();
        assert_eq!(p.listed_desks("DP-1", false), [1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(p.listed_desks("DP-1", true), []);
        p.toggle_lock("DP-1", 2);
        p.toggle_lock("HDMI-A-1", 1);
        assert_eq!(p.listed_desks("DP-1", false), [1, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(p.listed_desks("DP-1", true), [2]);
        assert_eq!(p.listed_desks("HDMI-A-1", true), [1]);
    }

    #[test]
    fn locking_the_shown_desktop_goes_back_or_to_the_lowest_unlocked() {
        let none = |_| false;
        // The one shown before, when it is unlocked.
        assert_eq!(jump_target(3, Some(5), none), Some(5));
        // Locked (or none, or the same one): the lowest unlocked other than this one.
        assert_eq!(jump_target(3, Some(5), |d| d == 5 || d == 1), Some(2));
        assert_eq!(jump_target(1, None, none), Some(2));
        assert_eq!(jump_target(3, Some(3), none), Some(1));
        // Every other one locked: no lock.
        assert_eq!(jump_target(4, Some(2), |d| d != 4), None);
    }
}
