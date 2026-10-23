//! Starting apps: dune-finder (Mod+d, a separate program, see `dune-finder/`) chooses an
//! app and where it opens (on a side of the focused tile, floating, as a tab of it, on
//! another monitor or desktop) and asks dune to start it with a `launch` request. Graphical
//! apps start on their own, like rofi does; console programs get a terminal.
//!
//! Mod+Enter opens dune-finder for a new, empty container instead: Enter right away puts
//! a terminal there, anything chosen or typed goes there too. The `switcher` key opens it
//! on the list of windows (`crate::windows`), Mod+/ on the focused tile's options
//! (`crate::privacy::option_rows`), whose rows come back as `tile_action` requests.

use std::{
    collections::HashMap,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

use dune_common::protocol::{Dest, Place};
use nix::{sys::signal, unistd::Pid};
use smithay::desktop::{LayerSurface, layer_map_for_output};

use crate::{
    Dune,
    layout::{Side, TileId},
    state::Tile,
};

/// The namespace of dune-finder's layer surface.
const FINDER_NAMESPACE: &str = "dune-finder";

/// Where a launched app's window goes, worked out when Enter was pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spot {
    /// A tab of this tile's container.
    Tab(TileId),
    /// A new tile on desktop `desk` of `output`, next to `target` (at the end without one).
    /// `side` None: a column of its own.
    New { output: String, desk: u8, target: Option<TileId>, side: Option<Side>, float: bool },
    /// Into this empty container (Mod+Enter).
    Into(TileId),
}

/// An app the launcher started, until its first window shows up.
pub struct Launch {
    pub spot: Spot,
    pub started: Instant,
    /// The process started for a graphical app: its window is matched by pid too, as
    /// Chromium-based browsers overwrite their environment (`DUNE_LAUNCH` with it).
    pub pid: Option<u32>,
}

/// A launch whose window has not shown up by then is forgotten (the app opened
/// none, or an already running instance opened it).
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a launch may claim a window that is not provably its own.
const UNCLAIMED_WINDOW: Duration = Duration::from_secs(20);

/// The launch a window nothing else claims belongs to: the latest one started in the
/// last `UNCLAIMED_WINDOW`. Its process cannot always be told: an already running
/// instance opens the window (Vivaldi, 1Password), or it comes through
/// xwayland-satellite (X11 apps like Spotify), whose pid is satellite's.
pub fn unclaimed_launch(launches: &HashMap<u64, Launch>, now: Instant) -> Option<u64> {
    launches
        .iter()
        .filter(|(_, l)| now.saturating_duration_since(l.started) < UNCLAIMED_WINDOW)
        .max_by_key(|(_, l)| l.started)
        .map(|(n, _)| *n)
}

/// The dune-finder running, started by dune.
#[derive(Debug)]
pub struct FinderRun {
    pub pid: u32,
    pub mode: FinderMode,
    /// The empty container it was opened for, and what had focus before; None once
    /// something was launched into it.
    pub slot: Option<(TileId, Option<TileId>)>,
}

/// What dune-finder lists first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinderMode {
    Apps,
    Windows,
    /// The focused tile's options (Mod+/).
    TileOptions,
}

impl FinderMode {
    fn arg(self) -> &'static str {
        match self {
            FinderMode::Apps => "apps",
            FinderMode::Windows => "windows",
            FinderMode::TileOptions => "tile-options",
        }
    }
}

impl Dune {
    /// Mod+d / Mod+/ / `switcher`: show dune-finder on the active output, or hide it.
    pub fn toggle_finder(&mut self, mode: FinderMode) {
        if self.finder_running() {
            // The `switcher` key on the apps of an empty container: its windows instead, the
            // one chosen goes into it (`go_to`).
            if let Some(run) = self.finder.as_mut().filter(|r| r.mode == FinderMode::Apps && mode == FinderMode::Windows)
                && let Some(slot) = run.slot.take()
            {
                self.close_finder();
                return self.start_finder(mode, Some(slot), false);
            }
            return self.close_finder();
        }
        // The tile options are a focused tile's: none, no menu.
        if mode == FinderMode::TileOptions && self.focused.is_none() {
            return tracing::debug!("no focused tile: no tile options");
        }
        self.start_finder(mode, None, false);
    }

    /// Whether a dune-finder started by dune is still there; one that went away
    /// without a layer to notice it by is forgotten here.
    fn finder_running(&mut self) -> bool {
        let Some(run) = &self.finder else { return false };
        if signal::kill(Pid::from_raw(run.pid as i32), None).is_ok() {
            return true;
        }
        self.finder_gone(None);
        false
    }

    /// Start dune-finder in `mode`; with `slot`, for that empty container; `compact`: with no
    /// list until something is typed.
    fn start_finder(&mut self, mode: FinderMode, slot: Option<(TileId, Option<TileId>)>, compact: bool) {
        // The tile options show over the focused tile, wherever it is.
        let output = match slot {
            Some((id, _)) => self.output_of(id),
            None if mode == FinderMode::TileOptions => self.focused.and_then(|t| self.output_of(t)),
            None => self.active_output().map(|o| o.name()),
        };
        let Some(output) = output else {
            return tracing::warn!("no output to show dune-finder on");
        };
        let mut cmd = Command::new(sibling_command("dune-finder"));
        cmd.args(["--mode", mode.arg(), "--output", &output]);
        // Its own settings it reads from finder.toml; the terminal is dune's.
        cmd.args(["--terminal", &self.config.terminal]);
        // A focused tile on another output (the pointer moved away from it) is not where
        // dune-finder shows: what it launches goes on its own output. The tile options
        // are the focused tile's wherever it is.
        let target = match slot {
            Some((_, back)) => back,
            None if mode == FinderMode::TileOptions => self.focused,
            None => self.focused.filter(|&t| self.output_of(t).as_ref() == Some(&output)),
        };
        if let Some(target) = target {
            cmd.args(["--target", &target.to_string()]);
        }
        if compact {
            cmd.arg("--compact");
        }
        if let Some((id, _)) = slot {
            cmd.args(["--slot", &id.to_string()]);
        }
        // Where the empty container (or the tile whose options these are) is on its
        // output, to show dune-finder in its middle.
        let over = slot.map(|(id, _)| id).or(target.filter(|_| mode == FinderMode::TileOptions));
        if let Some(id) = over {
            let origin = self.space.outputs().find(|o| o.name() == output).and_then(|o| self.space.output_geometry(o));
            if let (Some(origin), Some((_, r))) = (origin, self.shown.iter().find(|(t, _)| *t == id)) {
                let at = format!("{},{},{},{}", r.x - origin.loc.x, r.y - origin.loc.y, r.w, r.h);
                cmd.args(["--at", &at]);
            }
        }
        cmd.env_remove("DUNE_TILE").env_remove("DUNE_RUN").env_remove("DUNE_LAUNCH");
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from).filter(|d| d.is_dir()) {
            cmd.current_dir(home);
        }
        match cmd.spawn() {
            Ok(mut child) => {
                self.finder = Some(FinderRun { pid: child.id(), mode, slot });
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(err) => {
                tracing::error!("cannot start dune-finder: {err}");
                if let Some((id, back)) = slot {
                    self.drop_empty(id, back);
                }
            }
        }
    }

    /// dune-finder's layer surface, on whichever output it is.
    fn finder_layer(&self) -> Option<LayerSurface> {
        self.space
            .outputs()
            .find_map(|o| layer_map_for_output(o).layers().find(|l| l.namespace() == FINDER_NAMESPACE).cloned())
    }

    /// Whether `layer` is dune-finder's.
    pub fn is_finder(&self, layer: &LayerSurface) -> bool {
        layer.namespace() == FINDER_NAMESPACE
    }

    /// Ask dune-finder to go away; `finder_gone` follows when its layer does.
    pub fn close_finder(&mut self) {
        let Some(run) = &self.finder else { return };
        match self.finder_layer() {
            Some(layer) => layer.layer_surface().send_close(),
            None => {
                let _ = signal::kill(Pid::from_raw(run.pid as i32), signal::Signal::SIGTERM);
                self.finder_gone(None);
            }
        }
    }

    /// Whether a click at `pos` lands on dune-finder; false when it is not shown.
    pub fn finder_under(&self, pos: smithay::utils::Point<f64, smithay::utils::Logical>) -> Option<bool> {
        let layer = self.finder_layer()?;
        Some(self.surface_under(pos).is_some_and(|(surface, _)| &surface == layer.wl_surface()))
    }

    /// dune-finder went away: the empty container it was opened for goes too, unless
    /// something was launched into it. With `pid`, only when it is the one running now:
    /// one replaced by another (Mod+/ on an empty container's) leaves the new one alone.
    pub fn finder_gone(&mut self, pid: Option<u32>) {
        if pid.is_some_and(|pid| self.finder.as_ref().is_some_and(|r| r.pid != pid)) {
            return;
        }
        if let Some(FinderRun { slot: Some((id, back)), .. }) = self.finder.take() {
            self.drop_empty(id, back);
        }
    }

    /// The empty container `id` is going away by itself (Mod+q): dune-finder, open for it,
    /// is closed without touching it again.
    pub fn forget_finder_slot(&mut self, id: TileId) {
        if let Some(run) = self.finder.as_mut().filter(|r| r.slot.is_some_and(|(slot, _)| slot == id)) {
            run.slot = None;
            self.close_finder();
        }
    }

    /// Mod+Enter: a new, empty container where a terminal would open, with dune-finder
    /// for it to choose what goes there (Enter right away: a terminal).
    pub fn open_empty_container(&mut self) {
        self.open_empty(None, None, false);
    }

    /// A Mod+click near an edge of `target`: a new, empty container splitting it on `side`,
    /// with a compact dune-finder for it.
    pub fn open_empty_split(&mut self, target: TileId, side: Side) {
        self.open_empty(Some(target), Some(side), true);
    }

    /// A new, empty container next to `target` on `side` (see `insert_tile`), with dune-finder.
    fn open_empty(&mut self, target: Option<TileId>, side: Option<Side>, compact: bool) {
        let id = self.next_tile;
        if !self.insert_tile(id, target, side) {
            return tracing::warn!("no output to open a container on");
        }
        self.next_tile += 1;
        self.tiles.insert(id, Tile { reserved: true, ..Default::default() });
        self.launch_in_empty(id, compact);
    }

    /// The "+" of a tab bar: a new empty tab in `target`'s container, dune-finder for it.
    pub fn open_empty_tab(&mut self, target: TileId) {
        let Some(id) = self.tab_tile(target) else { return };
        self.tiles.get_mut(&id).unwrap().reserved = true;
        self.launch_in_empty(id, false);
    }

    /// Mod+Shift+t: a new empty tab right after the focused tile's, dune-finder for it. A
    /// floating tile has no tabs: an empty container to its right; nothing focused: where
    /// Mod+Enter opens one.
    pub fn open_empty_tab_here(&mut self) {
        let Some(target) = self.focused.filter(|f| self.tiles.contains_key(f)) else {
            return self.open_empty_container();
        };
        if self.is_floating(target) {
            return self.open_empty(Some(target), Some(Side::Right), false);
        }
        self.open_empty_tab(target);
    }

    /// Focus the empty container `id` and open dune-finder for it.
    fn launch_in_empty(&mut self, id: TileId, compact: bool) {
        let back = self.focused;
        if self.finder_running() {
            self.close_finder();
        }
        self.focus_tile(id);
        self.start_finder(FinderMode::Apps, Some((id, back)), compact);
    }

    /// Take away the empty container `id` if nothing went into it, focusing `back`.
    pub fn drop_empty(&mut self, id: TileId, back: Option<TileId>) {
        if !self.tiles.get(&id).is_some_and(|t| t.stack.is_empty()) {
            return;
        }
        self.remove_from_layouts(id);
        self.tiles.remove(&id);
        if self.focused == Some(id) {
            self.focused = back.filter(|b| self.tiles.contains_key(b));
        }
        self.arrange();
        self.update_focus();
    }

    /// A `launch` request from dune-finder: start `command` where `dest` says, next to
    /// `target`, or into the empty container `slot`; in a terminal with `terminal`.
    pub fn finder_launch(
        &mut self,
        dest: Dest,
        target: Option<TileId>,
        slot: Option<TileId>,
        command: String,
        terminal: bool,
        cwd: Option<PathBuf>,
    ) {
        let spot = match slot.filter(|s| self.tiles.contains_key(s)) {
            Some(slot) => {
                // It is filled now: dune-finder going away leaves it.
                if let Some(run) = self.finder.as_mut().filter(|r| r.slot.is_some_and(|(s, _)| s == slot)) {
                    run.slot = None;
                }
                Spot::Into(slot)
            }
            None => match self.spot(dest, target) {
                Some(spot) => spot,
                None => return tracing::warn!("no output to launch `{command}` on"),
            },
        };
        tracing::info!(?spot, terminal, "launching `{command}`");
        // With `follow`, the desktop it opens on is shown first, so its window maps and takes focus.
        let follow = match &spot {
            Spot::New { output, desk, .. } if self.config.follow => Some((output.clone(), *desk)),
            _ => None,
        };
        if let Some((output, desk)) = &follow
            && !terminal
        {
            self.show_desk_on(output, *desk);
        }
        self.launch(command, spot, terminal, cwd);
        // A terminal's tile is placed by `launch`; showing it afterwards brings it along.
        if let Some((output, desk)) = follow
            && terminal
        {
            self.show_desk_on(&output, desk);
        }
    }

    /// Where `dest` puts an app launched while `target` was focused.
    fn spot(&self, dest: Dest, target: Option<TileId>) -> Option<Spot> {
        let target = target.filter(|t| self.tiles.contains_key(t));
        if dest.tab && let Some(t) = target {
            return Some(Spot::Tab(t));
        }
        let here = target.and_then(|t| self.output_of(t)).or_else(|| self.active_output().map(|o| o.name()))?;
        let output = dest
            .monitor
            .and_then(|m| self.numbered_outputs().get(m as usize - 1).map(|o| o.name()))
            .unwrap_or(here.clone());
        let desk = dest.desk.unwrap_or(self.desk_of(&output));
        // Next to the focused tile when it is there, else after the last tile there.
        let target = target.filter(|_| output == here && desk == self.desk_of(&output));
        let (side, float) = match dest.place {
            Place::Column => (None, false),
            Place::Side(side) => (Some(side), false),
            Place::Float => (None, true),
        };
        Some(Spot::New { output, desk, target, side, float })
    }

    /// Start `command` for `spot`. Graphical apps start on their own, like rofi; console
    /// programs in a terminal. The process carries `DUNE_LAUNCH`, so `place_toplevel`
    /// knows where its first window goes.
    fn launch(&mut self, command: String, spot: Spot, terminal: bool, cwd: Option<PathBuf>) {
        let n = self.next_launch;
        self.next_launch += 1;
        // A forgotten launch leaves its empty container behind: take it away too.
        let expired: Vec<TileId> = self
            .launches
            .values()
            .filter(|l| l.started.elapsed() >= LAUNCH_TIMEOUT)
            .filter_map(|l| match l.spot {
                Spot::Into(t) => Some(t),
                _ => None,
            })
            .collect();
        self.launches.retain(|_, l| l.started.elapsed() < LAUNCH_TIMEOUT);
        for tile in expired {
            self.drop_empty(tile, None);
        }

        if terminal {
            // The terminal belongs to its tile: a new tab of the target or placed now, or
            // the floating target it stacks inside of.
            let stacked = match &spot {
                Spot::Tab(t) => self.tab_tile(*t),
                Spot::New { .. } | Spot::Into(_) => None,
            };
            let tile = match (&spot, stacked) {
                (_, Some(id)) => id,
                (Spot::Tab(t) | Spot::Into(t), None) => *t,
                (Spot::New { output, desk, target, side, float }, None) => {
                    let id = self.next_tile;
                    self.next_tile += 1;
                    self.tiles.insert(id, Default::default());
                    self.place_on_desk(id, output, *desk, *target, *side, *float);
                    id
                }
            };
            let stack = matches!(spot, Spot::Tab(_)) && stacked.is_none();
            if self.start_terminal(tile, cwd, Some(command), stack.then_some(n)) {
                if stack {
                    self.launches.insert(n, Launch { spot, started: Instant::now(), pid: None });
                }
            } else if !stack {
                self.remove_from_layouts(tile);
                self.tiles.remove(&tile);
                self.arrange();
            }
            return;
        }

        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(format!("exec {command}"));
        cmd.env("DUNE_LAUNCH", n.to_string()).env_remove("DUNE_TILE").env_remove("DUNE_RUN");
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from).filter(|d| d.is_dir()) {
            cmd.current_dir(home);
        }
        match cmd.spawn() {
            Ok(mut child) => {
                self.launches.insert(n, Launch { spot, started: Instant::now(), pid: Some(child.id()) });
                // Reap the process when it exits so it doesn't linger as a zombie.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(err) => tracing::error!("failed to launch `{command}`: {err}"),
        }
    }
}


/// A program of dune's own, looked up next to the dune binary first, so it is found even
/// when its directory is not on the session's `PATH`.
pub fn sibling_command(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(name)))
        .filter(|sibling| sibling.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unclaimed_windows_go_to_the_latest_recent_launch() {
        let now = Instant::now();
        let launch = |ago: u64| Launch { spot: Spot::Into(1), started: now - Duration::from_secs(ago), pid: None };
        let launches = HashMap::from([(1, launch(10)), (2, launch(3)), (3, launch(30))]);
        assert_eq!(unclaimed_launch(&launches, now), Some(2));
        assert_eq!(unclaimed_launch(&HashMap::from([(3, launch(30))]), now), None);
        assert_eq!(unclaimed_launch(&HashMap::new(), now), None);
    }

}
