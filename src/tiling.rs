//! Tile management: spawning terminal tiles, placing new windows and focus.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use smithay::{
    backend::input::{InputTime, KeyState},
    desktop::{Window, layer_map_for_output},
    input::pointer::MotionEvent,
    output::Output,
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{Resource, protocol::wl_surface::WlSurface},
    },
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::{
        compositor::with_states,
        shell::xdg::{ToplevelSurface, XdgToplevelSurfaceData},
    },
};

use dune_common::protocol::{Request, Side, TileOption, WindowInfo};

use crate::{
    config::TerminalHere,
    launcher::Spot,
    layout::{Direction, Layout, Rect, Split, TileId, column_zone, drop_column, drop_side, drop_zone, neighbor, past_edges},
    state::{Dune, Tile},
};

/// Space around and between columns; the focus border is drawn inside it. Tiles inside
/// a column are only `BORDER` apart, so neighbours share one border line.
pub const GAP: i32 = 6;
pub const BORDER: i32 = 1;
/// How long a resize by keys or wheel takes to slide from the old size to the new one.
const RESIZE_ANIMATION: Duration = Duration::from_millis(150);
/// A size sent to a client that has not drawn it after this long no longer holds back
/// the next one (a client that never answers must not freeze its size).
const CONFIGURE_WAIT: Duration = Duration::from_millis(150);

/// How soon after an unclaimed window opens a window of its app closing leaves it its place.
const REPLACE_WINDOW: Duration = Duration::from_secs(3);

/// A resize sliding into place: where each tile it moves started, and since when.
pub struct ResizeAnimation {
    from: HashMap<TileId, Rect>,
    start: Instant,
}

/// `from` moved towards `to` by `t` (0 to 1).
fn lerp_rect(from: Rect, to: Rect, t: f64) -> Rect {
    let mix = |a: i32, b: i32| a + ((b - a) as f64 * t).round() as i32;
    Rect::new(mix(from.x, to.x), mix(from.y, to.y), mix(from.w, to.w), mix(from.h, to.h))
}

/// Ease-out cubic: quick at first, slowing into place.
fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Where a tile being dragged would go: next to `target` on `side` in `output`'s
/// layout (at its end without a target), in a column of its own when `column`, or
/// into `target`'s container as a tab when `tab`, or, when `place`, its own tab moved to
/// that place of its bar; shown as `area` while dragging.
pub struct DropSpot {
    pub output: String,
    pub target: Option<TileId>,
    pub side: Side,
    pub column: bool,
    pub tab: bool,
    pub place: Option<usize>,
    pub area: Rect,
}

impl Dune {
    /// Open a new terminal tile next to `target` (or the focused tile).
    ///
    /// The tile is reserved in the layout right away but takes no space until the
    /// terminal maps its window. The terminal runs dune's shell wrapper as its
    /// `$SHELL` (see `crate::shell`), which starts the user's shell; `run` is handed
    /// to the wrapper through `DUNE_RUN` and typed at the prompt once the shell is ready.
    pub fn spawn_terminal(
        &mut self,
        target: Option<TileId>,
        side: Option<Side>,
        cwd: Option<PathBuf>,
        run: Option<String>,
    ) -> Option<TileId> {
        let id = self.next_tile;
        if !self.insert_tile(id, target, side) {
            tracing::warn!("no output to open a terminal on");
            return None;
        }
        self.next_tile += 1;
        self.tiles.insert(id, Tile::default());
        if self.start_terminal(id, cwd, run, None) {
            Some(id)
        } else {
            self.remove_from_layouts(id);
            self.tiles.remove(&id);
            None
        }
    }

    /// Mod+v / Mod+s / Mod+Shift+t: a terminal to the right of the focused tile, below it or
    /// as a new tab of its container (a floating one has no tabs: to its right), in the
    /// folder the focused window's shell is in; without a focused window, where a terminal
    /// opens and in `$HOME`.
    pub fn terminal_here(&mut self, here: TerminalHere) {
        let target = self.focused.filter(|f| self.tiles.contains_key(f));
        let cwd = target.and_then(|t| self.tile_cwd(t));
        if here == TerminalHere::Tab
            && let Some(id) = target.and_then(|t| self.tab_tile(t))
        {
            if !self.start_terminal(id, cwd, None, None) {
                self.remove_from_layouts(id);
                self.tiles.remove(&id);
                self.arrange();
            }
            return;
        }
        let side = match here {
            TerminalHere::Below => Side::Down,
            TerminalHere::Right | TerminalHere::Tab => Side::Right,
        };
        self.spawn_terminal(target, Some(side), cwd, None);
    }

    /// The folder tile `id`'s window works in: that of the newest process at the bottom of
    /// its client's (the shell in a terminal, or what runs in it).
    fn tile_cwd(&self, id: TileId) -> Option<PathBuf> {
        let surface = self.tiles.get(&id)?.top()?.toplevel()?.wl_surface().clone();
        let mut pid = self.client_pid(&surface)?;
        while let Some(child) = newest_child(pid) {
            pid = child;
        }
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }

    /// Focus left tile `id`: the folder its shell worked in counts as a visit for
    /// dune-finder's jumps. Only windows running something (a terminal's shell) count, and
    /// neither the home folder nor the root, where apps started from the launcher sit.
    fn record_jump(&self, id: TileId) {
        let Some(surface) = self.tiles.get(&id).and_then(|t| t.top()).and_then(|w| w.toplevel()).map(|t| t.wl_surface().clone()) else {
            return;
        };
        if self.client_pid(&surface).and_then(newest_child).is_none() {
            return;
        }
        let Some(dir) = self.tile_cwd(id) else { return };
        let home = std::env::var_os("HOME").map(PathBuf::from);
        if dir == std::path::Path::new("/") || Some(&dir) == home.as_ref() {
            return;
        }
        // A file write: kept off the compositor's thread.
        std::thread::spawn(move || {
            if let Err(err) = dune_common::jumps::record(&dir) {
                tracing::warn!("cannot record the jump to {}: {err}", dir.display());
            }
        });
    }

    /// Start a terminal for tile `id` (its `DUNE_TILE`), running `run` like `spawn_terminal`.
    /// `launch` is the launcher's `DUNE_LAUNCH` number when the launcher placed it.
    pub fn start_terminal(&mut self, id: TileId, cwd: Option<PathBuf>, run: Option<String>, launch: Option<u64>) -> bool {
        let terminal = &self.config.terminal;
        let dir = cwd.filter(|d| d.is_dir()).or_else(|| std::env::var_os("HOME").map(PathBuf::from));
        let mut cmd = Command::new("/bin/sh");
        // `exec` keeps the pid, so the Wayland client pid is the terminal's own. The folder
        // is `$1`, so it needs no quoting.
        cmd.arg("-c").arg(format!("exec {}", with_cwd_flag(terminal))).arg("sh");
        if let Some(dir) = &dir {
            cmd.arg(dir);
        }
        cmd.env("DUNE_TILE", id.to_string()).env_remove("DUNE_RUN").env_remove("DUNE_LAUNCH");
        if let Some(n) = launch {
            cmd.env("DUNE_LAUNCH", n.to_string());
        }
        if let Some(wrapper) = &self.shell_wrapper {
            cmd.env("SHELL", wrapper).env("DUNE_USER_SHELL", &self.user_shell);
        }
        if let Some(run) = run.map(|r| r.trim().to_string()).filter(|r| !r.is_empty()) {
            cmd.env("DUNE_RUN", run);
        }
        if let Some(dir) = dir {
            cmd.current_dir(dir);
        }

        match cmd.spawn() {
            Ok(mut child) => {
                // Reap the process when it exits so it doesn't linger as a zombie.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                true
            }
            Err(err) => {
                tracing::error!("failed to spawn terminal `{terminal}`: {err}");
                false
            }
        }
    }

    /// Put a new tile in a layout: next to `target` on its output when given, else on
    /// the active output (next to the focused tile when it is there). False when
    /// there is no output yet.
    pub(crate) fn insert_tile(&mut self, id: TileId, target: Option<TileId>, side: Option<Side>) -> bool {
        let explicit = target.filter(|t| self.tiles.contains_key(t)).and_then(|t| Some((self.output_of(t)?, t)));
        let (name, target) = match explicit {
            Some((name, t)) => (name, Some(t)),
            None => {
                let Some(name) = self.active_output().map(|o| o.name()) else { return false };
                let focused = self.focused.filter(|f| self.layouts.get(&name).is_some_and(|l| l.contains(*f)));
                (name, focused)
            }
        };
        // Without a side, a new column opens to the right: the strip grows without end.
        let layout = self.layouts.entry(name).or_default();
        match side {
            Some(side) => layout.insert_on(id, target, side),
            None => layout.insert_column(id, target, Side::Right),
        }
        true
    }

    /// A new tile as a tab of `target`'s container (see `Layout::add_tab`). None for a
    /// target out of the strip (floating).
    pub fn tab_tile(&mut self, target: TileId) -> Option<TileId> {
        let id = self.next_tile;
        let name = self.output_of(target)?;
        if !self.layouts.get_mut(&name)?.add_tab(id, target) {
            return None;
        }
        self.next_tile += 1;
        self.tiles.insert(id, Tile::default());
        Some(id)
    }

    /// Show the tab `delta` places after the focused one in its container, focused.
    pub fn cycle_tab(&mut self, delta: i32) {
        let Some(id) = self.focused else { return };
        if let Some(next) = self.layouts.values_mut().find_map(|l| l.cycle_tab(id, delta)) {
            self.focus_tile(next);
        }
    }

    /// Mod+Ctrl+direction: a tab leaves its column, for a column of its own on that side
    /// (above or below: into the tab shown in its place); another tile joins the column
    /// that way as a tab.
    pub fn move_tab(&mut self, dir: Direction) {
        let Some(id) = self.focused.filter(|f| !self.is_floating(*f)) else { return };
        let Some(output) = self.output_of(id) else { return };
        if let Some(sibling) = self.layouts.get(&output).and_then(|l| l.tab_sibling(id)) {
            let column = matches!(dir, Direction::Left | Direction::Right);
            return self.move_tile(id, &output, Some(sibling), dir, column);
        }
        if let Some(other) = neighbor(&self.geometries, id, dir).filter(|t| !self.is_floating(*t)) {
            self.join_tab(id, other);
        }
    }

    /// Mod+t: the focused tile, sharing its tab with others, leaves them for a tab of its
    /// own in its column. Alone, the windows stacked inside it each get a tile of their own, as tabs of its column after it; with none, see
    /// `leave_tab`.
    pub fn stack_to_tabs(&mut self) {
        let Some(id) = self.focused else { return };
        let Some(output) = self.output_of(id) else { return };
        let home = self.layouts.get(&output).and_then(|l| l.home(id));
        if self.layouts.get_mut(&output).is_some_and(|l| l.tile_to_tab(id)) {
            if let Some(home) = home {
                self.tab_homes.insert(id, (output, home));
            }
            return self.focus_tile(id);
        }
        let below = match self.tiles.get_mut(&id) {
            Some(tile) if tile.stack.len() > 1 && !self.floating.values().flatten().any(|(t, _)| *t == id) => {
                tile.stack.drain(..tile.stack.len() - 1).collect::<Vec<_>>()
            }
            _ => return self.leave_tab(id, &output),
        };
        let Some(layout) = self.layouts.get_mut(&output) else { return };
        for window in below {
            let tab = self.next_tile;
            self.next_tile += 1;
            layout.add_tab(tab, id);
            self.tiles.insert(tab, Tile { stack: vec![window], ..Default::default() });
        }
        layout.activate_tab(id);
        self.arrange();
    }

    /// Mod+t on a tile alone in its tab, nothing stacked in it: a tile Mod+t sent to a tab
    /// goes back where it was in its column; another tab leaves the tabs for a column of
    /// its own on their right, and the one tab of a column loses its bar. A column without
    /// tabs gets a bar with it as the one tab.
    fn leave_tab(&mut self, id: TileId, output: &str) {
        let home = self.tab_homes.remove(&id).filter(|(o, _)| o == output).map(|(_, h)| h);
        let Some(layout) = self.layouts.get_mut(output) else { return };
        if home.is_some_and(|h| layout.tab_back(id, h)) {
            return self.focus_tile(id);
        }
        if layout.lone_tab(id) || layout.untab(id) {
            return self.arrange();
        }
        if let Some(sibling) = layout.tab_sibling(id) {
            self.move_tile(id, output, Some(sibling), Side::Right, true);
        }
    }

    /// `id` leaves wherever it is to become a tab of `target`'s container, focused.
    pub fn join_tab(&mut self, id: TileId, target: TileId) {
        if target == id || !self.tiles.contains_key(&id) {
            return;
        }
        let Some(output) = self.output_of(target) else { return };
        self.remove_from_layouts(id);
        if let Some(layout) = self.layouts.get_mut(&output) {
            layout.add_tab(id, target);
        }
        self.focus_tile(id);
    }

    /// The tab under (`x`, `y`) in a tabbed container's bar.
    pub fn tab_at(&self, x: f64, y: f64) -> Option<TileId> {
        let bar = self.tab_bars.iter().find(|b| b.rect.contains(x, y))?;
        bar.strip(self.config.tab_widths).index_at(x, y).map(|i| bar.tabs[i])
    }

    /// The tabbed container whose bar has its "+" under (`x`, `y`): its shown tab.
    pub fn plus_at(&self, x: f64, y: f64) -> Option<TileId> {
        let bar = self.tab_bars.iter().find(|b| b.strip(self.config.tab_widths).plus.contains(x, y))?;
        Some(bar.active)
    }

    /// Take `id` out of whichever layout or floating stack holds it, shown or not.
    pub fn remove_from_layouts(&mut self, id: TileId) {
        self.float_homes.remove(&id);
        self.tab_homes.remove(&id);
        for layout in self.layouts.values_mut() {
            layout.remove(id);
        }
        for stack in self.floating.values_mut() {
            stack.retain(|(t, _)| *t != id);
        }
        self.remove_from_hidden(id);
        self.scratchpad.retain(|(t, _)| *t != id);
        self.scratch_shown.remove(&id);
    }

    /// The tile focus goes to when `id` leaves its spot: a neighbour, else the first of its output.
    pub fn neighbor_of(&self, id: TileId) -> Option<TileId> {
        // A tab leaving its container: the tab shown in its place.
        if let Some(sibling) = self.layouts.values().find_map(|l| l.tab_sibling(id)) {
            return Some(sibling);
        }
        let order = self.output_of(id).and_then(|name| self.layouts.get(&name).map(Layout::tiles)).unwrap_or_default();
        neighbor_or_first(&self.geometries, id, order).filter(|t| *t != id)
    }

    /// Name of the output whose layout (or floating stack) holds `id`.
    pub fn output_of(&self, id: TileId) -> Option<String> {
        self.layouts
            .iter()
            .find(|(_, l)| l.contains(id))
            .map(|(name, _)| name.clone())
            .or_else(|| self.floating.iter().find(|(_, s)| s.iter().any(|(t, _)| *t == id)).map(|(name, _)| name.clone()))
    }

    /// Where new tiles without a target go (see `Dune::active_output`).
    pub fn active_output(&self) -> Option<Output> {
        let outputs = || self.space.outputs();
        self.active_output
            .as_ref()
            .and_then(|name| outputs().find(|o| o.name() == *name))
            .or_else(|| outputs().next())
            .cloned()
    }

    /// The pointer moved: the output under it becomes the active one (locked, the
    /// keyboard goes to that output's lock surface).
    pub fn pointer_at(&mut self, pos: Point<f64, Logical>) {
        let Some(name) = self.space.output_under(pos).next().map(|o| o.name()) else { return };
        let changed = self.active_output.as_ref() != Some(&name);
        self.active_output = Some(name);
        if changed && self.session_lock.is_locked() {
            self.update_focus();
        }
    }

    /// The area tiles get: the output minus the exclusive zones of layers (bars).
    pub fn output_rect(&self, output: &Output) -> Option<Rect> {
        let geo = self.space.output_geometry(output)?;
        let zone = layer_map_for_output(output).non_exclusive_zone();
        Some(Rect::new(geo.loc.x + zone.loc.x, geo.loc.y + zone.loc.y, zone.size.w, zone.size.h))
    }

    /// Decide where a new toplevel goes:
    /// 1. a terminal we spawned claims its reserved tile (`DUNE_TILE`, tile still empty);
    /// 2. the first window of an app the launcher started (`DUNE_LAUNCH`, or its pid; a
    ///    window nothing claims, the latest launch's) goes where the launcher was told:
    ///    a new tab of the focused tile's container, its empty container, or a new tile
    ///    on some monitor and desktop;
    /// 3. a program started with `dune-open` in a terminal tile goes on top of that tile:
    ///    its process descends from the one the `swallow` request named;
    /// 4. anything else gets a new tile next to the focused one.
    pub fn place_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface.clone());
        let pid = self.client_pid(surface.wl_surface());
        let env = pid.map(process_env).unwrap_or_default();
        let env_tile = env.get("DUNE_TILE").and_then(|v| v.parse::<TileId>().ok());
        let launch_n = env.get("DUNE_LAUNCH").and_then(|v| v.parse::<u64>().ok()).or_else(|| {
            let pid = pid?;
            self.launches.iter().find(|(_, l)| l.pid.is_some_and(|p| is_descendant(pid, p))).map(|(n, _)| *n)
        });
        self.swallows.retain(|tile, pid| self.tiles.contains_key(tile) && Path::new(&format!("/proc/{pid}")).exists());
        let swallow = pid.and_then(|pid| self.swallows.iter().find(|(_, p)| is_descendant(pid, **p)).map(|(tile, _)| *tile));
        // A window nothing claims goes where the latest launch was told to.
        let launch_n = launch_n.or_else(|| {
            (env_tile.is_none() && swallow.is_none()).then(|| crate::launcher::unclaimed_launch(&self.launches, std::time::Instant::now())).flatten()
        });
        let launch = launch_n.and_then(|n| self.launches.remove(&n));

        let into = launch.as_ref().and_then(|l| match l.spot {
            Spot::Into(t) => Some(t),
            _ => None,
        });
        let tab_target = launch.as_ref().and_then(|l| match l.spot {
            Spot::Tab(t) => Some(t),
            _ => None,
        });
        let tab_target = tab_target.filter(|t| self.tiles.contains_key(t));
        let (id, how) = if let Some(id) = env_tile.filter(|t| self.tiles.get(t).is_some_and(|t| t.stack.is_empty())) {
            (id, "terminal")
        } else if let Some(id) = into.filter(|t| self.tiles.get(t).is_some_and(|t| t.stack.is_empty())) {
            (id, "its empty container")
        } else if let Some(id) = tab_target.and_then(|t| self.tab_tile(t)) {
            (id, "a tab of its tile")
        } else if let Some(id) = tab_target.or(swallow) {
            if let Some(old) = self.tiles[&id].top().cloned() {
                self.space.unmap_elem(&old);
            }
            (id, "on top of its tile")
        } else if let Some(Spot::New { output, desk, target, side, float }) = launch.map(|l| l.spot) {
            let id = self.next_tile;
            self.next_tile += 1;
            self.tiles.insert(id, Tile::default());
            self.place_on_desk(id, &output, desk, target, side, float);
            (id, "launcher")
        } else {
            let id = self.next_tile;
            if !self.insert_tile(id, None, None) {
                return tracing::warn!("no output to place a window on");
            }
            self.next_tile += 1;
            self.tiles.insert(id, Tile::default());
            self.unclaimed_window = pid.map(|pid| (id, pid, Instant::now()));
            (id, "new tile")
        };
        tracing::info!(tile = id, "placing window: {how}");

        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::TiledLeft);
            state.states.set(xdg_toplevel::State::TiledRight);
            state.states.set(xdg_toplevel::State::TiledTop);
            state.states.set(xdg_toplevel::State::TiledBottom);
        });

        let tile = self.tiles.get_mut(&id).unwrap();
        tile.stack.push(window.clone());
        tile.pid = pid.or(tile.pid);
        // On a desktop not shown (or in the scratchpad), the window waits unmapped and focus
        // stays where it is.
        if !self.put_away(id) {
            self.space.map_element(window, (0, 0), false);
            self.focused = Some(id);
            self.raise_float(id);
        }
        self.arrange();
        self.update_focus();
    }

    /// A toplevel went away: show the window under it, or drop the tile when it was the last window.
    pub fn remove_toplevel(&mut self, surface: &ToplevelSurface) {
        let found = self.tiles.iter().find_map(|(id, tile)| {
            tile.stack
                .iter()
                .position(|w| w.toplevel().is_some_and(|t| t == surface))
                .map(|pos| (*id, pos))
        });
        let Some((id, pos)) = found else {
            return;
        };

        let tile = self.tiles.get_mut(&id).unwrap();
        let window = tile.stack.remove(pos);
        self.space.unmap_elem(&window);

        if let Some(top) = tile.top().cloned() {
            tracing::info!(tile = id, "window closed, tile keeps {} window(s)", tile.stack.len());
            if pos == tile.stack.len() && !self.put_away(id) {
                self.space.map_element(top, (0, 0), false);
            }
        } else if let Some(fresh) = self.replacement_for(id) {
            // A window that opens one to replace it (a browser's profile picker) and
            // closes right after: the new one takes its place.
            tracing::info!(tile = id, from = fresh, "window closed, replaced by its app's new window");
            let window = self.tiles.remove(&fresh).unwrap().stack.remove(0);
            self.remove_from_layouts(fresh);
            let tile = self.tiles.get_mut(&id).unwrap();
            tile.stack.push(window.clone());
            if !self.put_away(id) {
                self.space.map_element(window, (0, 0), false);
            }
            if matches!(self.focused, Some(f) if f == id || f == fresh) {
                self.focused = Some(id);
            }
        } else {
            tracing::info!(tile = id, "window closed, tile removed");
            let next = self.neighbor_of(id);
            self.remove_from_layouts(id);
            self.tiles.remove(&id);
            if self.focused == Some(id) {
                self.focused = next;
            }
        }

        self.arrange();
        self.update_focus();
    }

    /// The tile of the window that opened just before the last window of `id` closed,
    /// from the same process and claimed by nothing (it got a tile of its own).
    fn replacement_for(&mut self, id: TileId) -> Option<TileId> {
        let (fresh, pid, at) = self.unclaimed_window?;
        let same_app = self.tiles.get(&id).and_then(|t| t.pid) == Some(pid);
        let alone = self.tiles.get(&fresh).is_some_and(|t| t.stack.len() == 1);
        let replaces = fresh != id && same_app && alone && at.elapsed() < REPLACE_WINDOW;
        replaces.then(|| self.unclaimed_window.take().map(|(t, ..)| t)).flatten()
    }

    /// Lay out every tile on its output and send the new sizes to the clients.
    pub fn arrange(&mut self) {
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        let Some(first) = outputs.first() else {
            return;
        };
        // Tiles of a monitor that went away move to the first one.
        let gone: Vec<String> = self.layouts.keys().filter(|n| !outputs.iter().any(|o| o.name() == **n)).cloned().collect();
        for name in gone {
            let moved = self.layouts.remove(&name).unwrap().tiles();
            tracing::info!("output {name} is gone, moving its {} tile(s) to {}", moved.len(), first.name());
            let layout = self.layouts.entry(first.name()).or_default();
            for id in moved {
                let last = layout.tiles().last().copied();
                layout.insert(id, last, Split::Vertical);
            }
        }
        self.adopt_desks(&outputs, &first.name());

        // The tab of a tabbed column a focused tile is in shows. A folded (accordion) tile
        // stays folded when focused: a click or Enter opens it (`open_folded`).
        if let Some(id) = self.focused {
            for layout in self.layouts.values_mut() {
                layout.activate_tab(id);
            }
        }

        self.settle_floats();
        // Fullscreen ends when focus goes to another tile of the same monitor.
        let focused_on = self.focused.and_then(|f| self.output_of(f));
        let ended: Vec<TileId> = self
            .fullscreen
            .iter()
            .copied()
            .filter(|t| !self.tiles.contains_key(t) || (Some(*t) != self.focused && focused_on.is_some() && self.output_of(*t) == focused_on))
            .collect();
        for t in ended {
            self.fullscreen.remove(&t);
        }
        // A private tile that closed is forgotten (ids are not reused).
        let tiles = &self.tiles;
        self.privacy.forget_gone(|t| tiles.contains_key(&t));

        let tiles = &self.tiles;
        let visible = |id| tiles.get(&id).is_some_and(|t: &Tile| !t.stack.is_empty() || t.reserved);
        let mut geometries = Vec::new();
        let mut shown = Vec::new();
        let mut tabs = Vec::new();
        let mut tab_bars = Vec::new();
        let mut fold_counts = Vec::new();
        let mut edge_counts = HashMap::new();
        // Floats first, front to back: what is first is hit (and drawn) first.
        for output in &outputs {
            for (id, rect) in self.floating.get(&output.name()).into_iter().flatten().rev() {
                if visible(*id) {
                    geometries.push((*id, *rect));
                    shown.push((*id, *rect));
                }
            }
        }
        for output in &outputs {
            let Some(area) = self.output_rect(output) else { continue };
            let Some(layout) = self.layouts.get_mut(&output.name()) else { continue };
            tracing::debug!(output = output.name(), ?area, "arranging tiles");
            let laid_out = layout.geometries(area, GAP, BORDER, &visible, self.focused);
            edge_counts.insert(output.name(), past_edges(area, &laid_out));
            for (id, rect) in laid_out {
                if layout.is_tab(id) {
                    tabs.push(id);
                }
                if let Some(part) = rect.intersection(&area) {
                    shown.push((id, part));
                }
                geometries.push((id, rect));
            }
            tab_bars.extend(layout.tab_bars().iter().filter(|b| b.rect.intersection(&area).is_some()).cloned());
            fold_counts.extend(layout.fold_counts().iter().copied());
        }
        // A fullscreen tile laid out on its monitor covers it: the whole output, and nothing
        // else of it shown (nor its tab bars and edge marks).
        let mut covers = HashMap::new();
        for output in &outputs {
            let Some(geo) = self.space.output_geometry(output) else { continue };
            let full = Rect::new(geo.loc.x, geo.loc.y, geo.size.w, geo.size.h);
            let name = output.name();
            let cover = self.fullscreen.iter().copied().find(|t| {
                self.output_of(*t).as_ref() == Some(&name) && !tabs.contains(t) && geometries.iter().any(|(g, _)| g == t)
            });
            let Some(cover) = cover else { continue };
            for (t, rect) in geometries.iter_mut() {
                if *t == cover {
                    *rect = full;
                }
            }
            shown.retain(|(t, rect)| *t != cover && rect.intersection(&full).is_none());
            shown.insert(0, (cover, full));
            tab_bars.retain(|b| b.rect.intersection(&full).is_none());
            edge_counts.remove(&name);
            covers.insert(name, cover);
        }
        self.covers = covers;
        tracing::debug!(?geometries, shown = ?shown.iter().map(|(id, _)| id).collect::<Vec<_>>(), ?tabs, "arranged");
        self.geometries = geometries;
        self.shown = shown;
        self.tabs = tabs;
        self.edge_counts = edge_counts;

        // Tabs not shown in their container keep their window unmapped, at the size of
        // the shown tab so switching to them needs no resize.
        let laid_out: Vec<TileId> = self.geometries.iter().map(|(id, _)| *id).collect();
        for layout in self.layouts.values() {
            for id in layout.tiles().into_iter().filter(|t| !laid_out.contains(t)) {
                if let Some(window) = self.tiles.get(&id).and_then(|t| t.top()) {
                    self.space.unmap_elem(window);
                }
            }
        }
        for bar in &tab_bars {
            let Some((_, rect)) = self.geometries.iter().find(|(t, _)| *t == bar.active) else { continue };
            for id in bar.tabs.iter().filter(|t| **t != bar.active) {
                let Some(toplevel) = self.tiles.get(id).and_then(|t| t.top()).and_then(|w| w.toplevel()) else { continue };
                toplevel.with_pending_state(|state| state.size = Some((rect.w, rect.h).into()));
                if toplevel.is_initial_configure_sent() {
                    toplevel.send_pending_configure();
                }
            }
        }
        self.tab_bars = tab_bars;
        self.fold_counts = fold_counts;

        self.place_windows();
        self.refresh_workspaces();
        self.refresh_scratchpad();
    }

    /// Make `id` fullscreen (`on`), the one of its monitor, focused; or stop it.
    pub fn set_fullscreen(&mut self, id: TileId, on: bool) {
        if !self.tiles.contains_key(&id) {
            return;
        }
        if on {
            let output = self.output_of(id);
            let others: Vec<TileId> = self.fullscreen.iter().copied().filter(|t| self.output_of(*t) == output).collect();
            for t in others {
                self.fullscreen.remove(&t);
            }
            self.fullscreen.insert(id);
            // On a desktop not shown it waits there, focus staying where it is.
            if output.is_some() {
                self.focus_tile(id);
            }
        } else if self.fullscreen.remove(&id) {
            self.arrange();
        }
    }

    /// Map the windows of the strip where `geometries` (or a resize sliding into place)
    /// puts them, and send them their sizes.
    fn place_windows(&mut self) {
        // Back to front: mapping an element puts it on top of the space, so floats end up
        // above the strip and clicks reach them.
        let placed: Vec<(TileId, Rect)> = self.geometries.iter().rev().map(|(id, r)| (*id, self.drawn_rect(*id, *r))).collect();
        for (id, rect) in &placed {
            let Some(window) = self.tiles[id].top().cloned() else {
                continue;
            };
            let floating = self.is_floating(*id);
            let fullscreen = self.covers.values().any(|c| c == id);
            // Columns scrolled off the output are not drawn (nor shown on a neighbour);
            // a tab shows a bar instead of its window, which keeps its last size.
            if self.tabs.contains(id) {
                self.space.unmap_elem(&window);
                continue;
            } else if !self.shown.iter().any(|(t, _)| t == id) {
                self.space.unmap_elem(&window);
            } else {
                self.space.map_element(window.clone(), (rect.x, rect.y), false);
            }
            let toplevel = window.toplevel().unwrap();
            toplevel.with_pending_state(|state| {
                state.size = Some((rect.w, rect.h).into());
                // Tiled windows drop their shadows and rounded corners; floating ones keep them.
                use xdg_toplevel::State::*;
                for edge in [TiledLeft, TiledRight, TiledTop, TiledBottom] {
                    if floating {
                        state.states.unset(edge);
                    } else {
                        state.states.set(edge);
                    }
                }
                if fullscreen {
                    state.states.set(Fullscreen);
                } else {
                    state.states.unset(Fullscreen);
                }
            });
            self.send_size(toplevel);
        }
    }

    /// Where `id`, laid out at `target`, is drawn now: on its way there while a resize slides.
    pub fn drawn_rect(&self, id: TileId, target: Rect) -> Rect {
        let Some(anim) = &self.resize_animation else { return target };
        let Some(from) = anim.from.get(&id) else { return target };
        let t = anim.start.elapsed().as_secs_f64() / RESIZE_ANIMATION.as_secs_f64();
        lerp_rect(*from, target, ease_out(t))
    }

    /// Start a resize sliding from where the tiles are drawn now (so repeated keys glide on).
    pub fn animate_resize(&mut self) {
        let from = self.geometries.iter().map(|(id, r)| (*id, self.drawn_rect(*id, *r))).collect();
        self.resize_animation = Some(ResizeAnimation { from, start: Instant::now() });
    }

    /// Before a frame: move the windows of a sliding resize on (and stop it once there).
    pub fn step_resize_animation(&mut self) {
        let Some(anim) = &self.resize_animation else { return };
        if anim.start.elapsed() >= RESIZE_ANIMATION {
            self.resize_animation = None;
        }
        self.place_windows();
    }

    /// Send `toplevel` its pending state; a new size waits while the client has not drawn
    /// the last one (up to `CONFIGURE_WAIT`), so a resize does not bury it in sizes.
    fn send_size(&mut self, toplevel: &ToplevelSurface) {
        // Before the initial commit the size rides along with the initial configure.
        if !toplevel.is_initial_configure_sent() {
            return;
        }
        let key = toplevel.wl_surface().id();
        let pending = toplevel.with_pending_state(|state| state.size);
        let waiting = self.configure_waits.get(&key).is_some_and(|(size, since)| *size != pending && since.elapsed() < CONFIGURE_WAIT);
        if waiting {
            return;
        }
        if toplevel.send_pending_configure().is_some() {
            self.configure_waits.insert(key, (pending, Instant::now()));
        }
    }

    /// A toplevel committed: once it has drawn the size it was sent, the next may go.
    pub fn size_committed(&mut self, toplevel: &ToplevelSurface) {
        let key = toplevel.wl_surface().id();
        let Some((sent, _)) = self.configure_waits.get(&key).copied() else { return };
        if toplevel.with_committed_state(|state| state.map(|s| s.size)) == Some(sent) {
            self.configure_waits.remove(&key);
            if toplevel.has_pending_changes() {
                self.send_size(toplevel);
            }
        }
    }

    /// Give keyboard focus to the visible window of the focused tile.
    pub fn update_focus(&mut self) {
        if self.focused.is_none_or(|id| !self.tiles.contains_key(&id)) {
            self.focused = self.geometries.first().map(|(id, _)| *id);
        }
        if let Some(id) = self.focused.filter(|id| self.recent.first() != Some(id)) {
            if let Some(left) = self.recent.first().copied() {
                self.record_jump(left);
            }
            self.recent.retain(|t| *t != id && self.tiles.contains_key(t));
            self.recent.insert(0, id);
        }
        let focused_window = self.focused.and_then(|id| self.tiles[&id].top().cloned());

        for window in self.space.elements() {
            let toplevel = window.toplevel().unwrap();
            if window.set_activated(Some(window) == focused_window.as_ref()) && toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }

        // Locked, only a lock surface gets it. A layer that asked for the keyboard
        // (launcher) keeps it while open.
        let surface: Option<WlSurface> = if self.session_lock.is_locked() {
            self.lock_focus()
        } else {
            self.layer_focus
                .clone()
                .or_else(|| focused_window.map(|w| w.toplevel().unwrap().wl_surface().clone()))
        };
        let keyboard = self.seat.get_keyboard().unwrap();
        keyboard.set_focus(self, surface, SERIAL_COUNTER.next_serial());
    }

    /// Whether `id` is folded into an accordion bar.
    pub fn is_folded(&self, id: TileId) -> bool {
        self.layouts.values().any(|l| l.is_tab(id))
    }

    /// Open the folded tile `id` (a click or Enter on its bar), focused.
    pub fn open_folded(&mut self, id: TileId) {
        for layout in self.layouts.values_mut() {
            layout.open_tab(id);
        }
        self.focus_tile(id);
    }

    pub fn focus_tile(&mut self, id: TileId) {
        if self.tiles.contains_key(&id) {
            self.focused = Some(id);
            self.active_output = self.output_of(id).or(self.active_output.take());
            self.raise_float(id);
            // The view slides to show the newly focused column.
            self.arrange();
            self.update_focus();
        }
    }

    pub fn focus_direction(&mut self, dir: Direction) {
        if let Some(next) = self.focused.and_then(|from| neighbor(&self.geometries, from, dir)) {
            self.focus_tile(next);
        }
    }

    pub fn swap_direction(&mut self, dir: Direction) {
        let Some(from) = self.focused.filter(|f| !self.is_floating(*f)) else { return };
        if let Some(other) = neighbor(&self.geometries, from, dir) {
            // Tiles on different monitors trade places between their layouts too. A
            // folded tile stays folded where it goes, an open one open.
            let (from_folded, other_folded) = (self.is_folded(from), self.is_folded(other));
            for layout in self.layouts.values_mut() {
                layout.swap_keeping_folds(from, other, from_folded, other_folded);
            }
            self.arrange();
        } else if let Some(output) = self.output_toward(from, dir) {
            // Nothing that way on this monitor: the tile moves to the next monitor, at
            // the edge facing the one it comes from.
            let tiles = self.layouts.get(&output).map(Layout::tiles).unwrap_or_default();
            let (target, side) = match dir {
                Direction::Right | Direction::Down => (tiles.first().copied(), Side::Left),
                Direction::Left | Direction::Up => (tiles.last().copied(), Side::Right),
            };
            self.move_tile(from, &output, target, side, true);
        }
    }

    /// Focus the monitor next to the active one in `dir`: the tile focused there last
    /// (shown), else its first one. The pointer goes to the middle of that tile, or of the
    /// monitor when it has none, so the next motion does not take focus back.
    pub fn focus_output_direction(&mut self, dir: Direction) {
        let Some(from) = self.active_output().map(|o| o.name()) else { return };
        let Some(name) = self.output_next_to(&from, dir) else { return };
        let shown_there = |id: &TileId| {
            self.output_of(*id).as_deref() == Some(name.as_str()) && self.geometries.iter().any(|(t, _)| t == id)
        };
        let tile = (self.recent.iter())
            .chain(self.geometries.iter().map(|(id, _)| id))
            .find(|id| shown_there(id))
            .copied();
        let center = match tile.and_then(|id| self.geometries.iter().find(|(t, _)| *t == id)) {
            Some((_, rect)) => rect.center(),
            None => {
                let Some(output) = self.space.outputs().find(|o| o.name() == name).cloned() else { return };
                let Some(geo) = self.space.output_geometry(&output) else { return };
                (geo.loc.x + geo.size.w / 2, geo.loc.y + geo.size.h / 2)
            }
        };
        match tile {
            Some(id) => self.focus_tile(id),
            None => self.active_output = Some(name),
        }
        self.warp_pointer(Point::from((center.0 as f64, center.1 as f64)));
    }

    /// Move the pointer to `pos`, pointer focus to what is under it there.
    fn warp_pointer(&mut self, pos: Point<f64, Logical>) {
        let pointer = self.seat.get_pointer().unwrap();
        let under = self.surface_under(pos);
        let event = MotionEvent { location: pos, serial: SERIAL_COUNTER.next_serial(), time: InputTime::now() };
        pointer.motion(self, under, &event);
        pointer.frame(self);
    }

    /// The output next to the one holding `id`, in `dir`.
    fn output_toward(&self, id: TileId, dir: Direction) -> Option<String> {
        self.output_next_to(&self.output_of(id)?, dir)
    }

    /// The output next to the one named `name`, in `dir`.
    fn output_next_to(&self, name: &str, dir: Direction) -> Option<String> {
        let geo = |o: &Output| self.space.output_geometry(o);
        let cur = geo(self.space.outputs().find(|o| o.name() == name)?)?;
        let rects: Vec<(TileId, Rect)> = self
            .space
            .outputs()
            .enumerate()
            .filter_map(|(i, o)| geo(o).map(|g| (i as TileId, Rect::new(g.loc.x, g.loc.y, g.size.w, g.size.h))))
            .collect();
        let me = rects.iter().find(|(_, r)| (r.x, r.y) == (cur.loc.x, cur.loc.y))?.0;
        let next = neighbor(&rects, me, dir)?;
        self.space.outputs().nth(next as usize).map(|o| o.name())
    }

    /// Take `id` out of its layout and put it on `side` of `target` in the layout of
    /// `output` (at the end of it without a target), in a column of its own with
    /// `column`; it stays focused.
    pub fn move_tile(&mut self, id: TileId, output: &str, target: Option<TileId>, side: Side, column: bool) {
        if target == Some(id) || !self.tiles.contains_key(&id) {
            return;
        }
        self.remove_from_layouts(id);
        let layout = self.layouts.entry(output.to_string()).or_default();
        if column {
            layout.insert_column(id, target, side);
        } else {
            layout.insert_on(id, target, side);
        }
        self.focus_tile(id);
    }

    /// A Mod+drag ended at `pos`: a tab let go on its own bar takes the place under the
    /// pointer; on another tab bar the tile joins that container as a tab;
    /// otherwise it goes next to the tile under it (on the side nearest to the pointer; at
    /// its left or right edge, in a new column there), or in a new column at the end of
    /// the output when no tile is under it.
    pub fn drop_tile(&mut self, id: TileId, pos: Point<f64, Logical>) {
        match self.drop_spot(id, pos) {
            Some(DropSpot { place: Some(index), .. }) => self.change_tile(id, |l, id| {
                l.move_tab(id, index);
            }),
            Some(DropSpot { tab: true, target: Some(target), .. }) => self.join_tab(id, target),
            Some(spot) => self.move_tile(id, &spot.output, spot.target, spot.side, spot.column),
            None => {}
        }
    }

    /// Where `id` would land if dropped at `pos`, or `None` when nothing would move.
    pub fn drop_spot(&self, id: TileId, pos: Point<f64, Logical>) -> Option<DropSpot> {
        // On a tab bar: a tab of that container; on its own bar, the tab under the pointer
        // is the place it moves to (its own: nothing moves).
        if let Some(bar) = self.tab_bars.iter().find(|b| b.rect.contains(pos.x, pos.y)) {
            if let Some(from) = bar.tabs.iter().position(|t| *t == id) {
                let strip = bar.strip(self.config.tab_widths);
                let index = strip.index_at(pos.x, pos.y).unwrap_or(strip.first + strip.count - 1);
                let output = self.output_of(id)?;
                // Over a mark at an end: the mark is lit, the tab goes just past it.
                let area = match (strip.left, strip.right) {
                    (Some((r, _)), _) if index + 1 == strip.first => r,
                    (_, Some((r, _))) if index == strip.first + strip.count => r,
                    _ => strip.tabs[index],
                };
                return (index != from).then(|| DropSpot {
                    output,
                    target: Some(bar.tabs[index]),
                    side: Side::Right,
                    column: false,
                    tab: false,
                    place: Some(index),
                    area,
                });
            }
            let output = self.output_of(bar.active)?;
            let content = self.shown.iter().find(|(t, _)| *t == bar.active).map(|(_, r)| *r);
            let bottom = content.map_or(bar.rect.y + bar.rect.h, |r| r.y + r.h);
            let area = Rect::new(bar.rect.x, bar.rect.y, bar.rect.w, bottom - bar.rect.y);
            return Some(DropSpot { output, target: Some(bar.active), side: Side::Right, column: false, tab: true, place: None, area });
        }
        // Tiles land in the strip: a float under the pointer is looked through.
        let under = self.shown.iter().find(|(t, r)| *t != id && !self.is_floating(*t) && r.contains(pos.x, pos.y)).copied();
        if let Some((target, rect)) = under {
            let side = drop_side(&rect, pos.x, pos.y);
            let output = self.output_of(target)?;
            let column = drop_column(&rect, pos.x, side);
            let area = if column { column_zone(&rect, side) } else { drop_zone(&rect, side) };
            return Some(DropSpot { output, target: Some(target), side, column, tab: false, place: None, area });
        }
        // Dropped on the tile itself: nothing moves.
        if self.shown.iter().any(|(t, r)| *t == id && r.contains(pos.x, pos.y)) {
            return None;
        }
        // Empty space: a column of its own at the end of the strip there.
        let output = self.space.output_under(pos).next()?;
        let area = self.output_rect(output)?;
        Some(DropSpot { output: output.name(), target: None, side: Side::Right, column: true, tab: false, place: None, area })
    }

    /// Apply a layout change to the focused tile's layout, then lay everything out again.
    pub fn change_focused(&mut self, change: impl Fn(&mut Layout, TileId)) {
        if let Some(id) = self.focused {
            self.change_tile(id, change);
        }
    }

    /// Fold `id` into the accordion, or open it when folded (`Layout::toggle_fold`).
    pub fn toggle_fold(&mut self, id: TileId) {
        if self.is_folded(id) {
            return self.open_folded(id);
        }
        self.change_tile(id, Layout::toggle_fold);
    }

    pub fn change_tile(&mut self, id: TileId, change: impl Fn(&mut Layout, TileId)) {
        if let Some(layout) = self.layouts.values_mut().find(|l| l.contains(id)) {
            change(layout, id);
            self.arrange();
        }
    }

    /// Close the focused tile's window; an empty container (Mod+Enter) just goes away.
    pub fn close_focused(&mut self) {
        let Some(id) = self.focused else { return };
        match self.tiles.get(&id).and_then(|t| t.top()) {
            Some(window) => window.toplevel().unwrap().send_close(),
            None if self.tiles.get(&id).is_some_and(|t| t.reserved) => {
                self.forget_finder_slot(id);
                self.drop_empty(id, None);
            }
            None => {}
        }
    }

    pub fn tile_at(&self, x: f64, y: f64) -> Option<TileId> {
        self.shown.iter().find(|(_, r)| r.contains(x, y)).map(|(id, _)| *id)
    }

    /// What a tab says: the title of the tile's window, else its app id.
    pub fn tile_title(&self, id: TileId) -> String {
        let surface = self.tiles.get(&id).and_then(|t| t.top()).and_then(|w| w.toplevel()).map(|t| t.wl_surface().clone());
        let title = surface.and_then(|surface| {
            with_states(&surface, |states| {
                let data = states.data_map.get::<XdgToplevelSurfaceData>()?.lock().unwrap();
                data.title.clone().filter(|t| !t.is_empty()).or_else(|| data.app_id.clone())
            })
        });
        title.unwrap_or_else(|| format!("tile {id}"))
    }

    /// The app id of the tile's window.
    pub fn tile_app_id(&self, id: TileId) -> Option<String> {
        let surface = self.tiles.get(&id)?.top()?.toplevel()?.wl_surface().clone();
        with_states(&surface, |states| states.data_map.get::<XdgToplevelSurfaceData>()?.lock().unwrap().app_id.clone())
    }

    /// Pid of the process behind the tile's window.
    pub fn tile_pid(&self, id: TileId) -> Option<u32> {
        let surface = self.tiles.get(&id)?.top()?.toplevel()?.wl_surface().clone();
        self.client_pid(&surface)
    }

    /// Pid of the process behind a Wayland client.
    pub fn client_pid(&self, surface: &WlSurface) -> Option<u32> {
        let client = self.display_handle.get_client(surface.id()).ok()?;
        let creds = client.get_credentials(&self.display_handle).ok()?;
        u32::try_from(creds.pid).ok()
    }

    /// A control socket request; some are answered (the returned text).
    pub fn handle_request(&mut self, req: Request) -> Option<String> {
        match req {
            Request::Open { side, from, cwd, command } => {
                self.spawn_terminal(from, side, cwd, Some(command));
            }
            Request::Swallow { tile, pid } => {
                if self.tiles.contains_key(&tile) {
                    self.swallows.insert(tile, pid);
                }
                return Some("ok".into());
            }
            Request::Windows => return Some(WindowInfo::encode_all(&self.window_list(false))),
            Request::LockedWindows => return Some(WindowInfo::encode_all(&self.window_list(true))),
            Request::GoTo { tile } => {
                self.go_to(tile);
                return Some("ok".into());
            }
            Request::Unlock { tile } => {
                self.unlock_and_go(tile);
                return Some("ok".into());
            }
            Request::Launch {
                dest,
                target,
                slot,
                terminal,
                cwd,
                command,
            } => {
                self.finder_launch(dest, target, slot, command, terminal, cwd);
                return Some("ok".into());
            }
            Request::TileOptions { tile } => return Some(TileOption::encode_all(&self.tile_options(tile))),
            Request::TileAction { tile, action } => {
                self.run_tile_action(tile, action);
                return Some("ok".into());
            }
            Request::Quit => {
                tracing::info!("quit asked over the control socket");
                self.loop_signal.stop();
                return Some("ok".into());
            }
            Request::ScratchpadShow => {
                self.toggle_scratchpad();
                return Some("ok".into());
            }
            Request::Recording { started, area } => self.recording_state(started, area.map(|(x, y, w, h)| Rect::new(x, y, w, h))),
            Request::RecordingStop => {
                self.stop_recording();
                return Some("ok".into());
            }
            // The listener keeps the connection (`ipc::listen`).
            Request::WatchScratchpad | Request::WatchRecording => {}
            Request::Keys(keys) => {
                if std::env::var("DUNE_DEBUG").as_deref() != Ok("1") {
                    tracing::warn!("ignoring key injection: DUNE_DEBUG=1 is not set");
                    return None;
                }
                for (code, pressed) in keys {
                    let state = if pressed { KeyState::Pressed } else { KeyState::Released };
                    self.handle_key(code.into(), state, InputTime::now());
                }
            }
        }
        None
    }
}

fn neighbor_or_first(geoms: &[(TileId, Rect)], id: TileId, order: Vec<TileId>) -> Option<TileId> {
    [Direction::Left, Direction::Up, Direction::Right, Direction::Down]
        .into_iter()
        .find_map(|dir| neighbor(geoms, id, dir))
        .or_else(|| order.into_iter().find(|t| *t != id))
}

/// `DUNE_*` variables of a process, read from `/proc`.
fn process_env(pid: u32) -> HashMap<String, String> {
    let Ok(raw) = std::fs::read(format!("/proc/{pid}/environ")) else {
        return HashMap::new();
    };
    raw.split(|b| *b == 0)
        .filter_map(|entry| {
            let entry = String::from_utf8_lossy(entry);
            let (k, v) = entry.split_once('=')?;
            k.starts_with("DUNE_").then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// Whether `pid` is `ancestor` or one of its descendants.
pub fn is_descendant(mut pid: u32, ancestor: u32) -> bool {
    for _ in 0..64 {
        if pid == ancestor {
            return true;
        }
        match parent_pid(pid) {
            Some(parent) if parent > 1 => pid = parent,
            _ => return false,
        }
    }
    false
}

fn parent_pid(pid: u32) -> Option<u32> {
    stat_field(pid, 4)?.parse().ok()
}

/// The `terminal` command run by `start_terminal`, its folder `$1`. Most terminals start
/// their shell in their own working folder; `wezterm start` in the home folder unless
/// given `--cwd` (empty `$1`: nothing, it stays home).
fn with_cwd_flag(terminal: &str) -> String {
    let mut words = terminal.split_whitespace();
    let wezterm = words.next().and_then(|p| p.rsplit('/').next()) == Some("wezterm");
    let start = words.clone().any(|w| w == "start");
    if !wezterm || !start || words.any(|w| w == "--cwd" || w.starts_with("--cwd=") || w == "--") {
        return terminal.to_string();
    }
    format!("{terminal} ${{1:+--cwd \"$1\"}}")
}

/// The child process of `pid` (of any of its threads) started last.
fn newest_child(pid: u32) -> Option<u32> {
    let tasks = std::fs::read_dir(format!("/proc/{pid}/task")).ok()?;
    tasks
        .flatten()
        .filter_map(|task| std::fs::read_to_string(task.path().join("children")).ok())
        .flat_map(|children| children.split_whitespace().filter_map(|c| c.parse::<u32>().ok()).collect::<Vec<_>>())
        .max_by_key(|child| stat_field(*child, 22).and_then(|t| t.parse::<u64>().ok()).unwrap_or(0))
}

/// Field `n` (from 1, as `man proc` numbers them) of `/proc/<pid>/stat`.
fn stat_field(pid: u32, n: usize) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name (field 2) is in parentheses and may contain spaces: parse after the last `)`.
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_whitespace().nth(n.checked_sub(3)?).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resize_slides_from_the_old_rect_to_the_new() {
        let (from, to) = (Rect::new(0, 0, 500, 600), Rect::new(0, 0, 600, 600));
        assert_eq!(lerp_rect(from, to, ease_out(0.0)), from);
        assert_eq!(lerp_rect(from, to, ease_out(1.0)), to);
        assert_eq!(lerp_rect(from, to, ease_out(2.0)), to);
        // Eased out: more than half the way at half the time.
        assert!(lerp_rect(from, to, ease_out(0.5)).w > 550);
    }

    #[test]
    fn wezterm_start_is_given_the_folder() {
        assert_eq!(with_cwd_flag("wezterm start --always-new-process"), "wezterm start --always-new-process ${1:+--cwd \"$1\"}");
        assert_eq!(with_cwd_flag("/usr/bin/wezterm start"), "/usr/bin/wezterm start ${1:+--cwd \"$1\"}");
        // Its own folder already, a program after `--`, or another terminal: as it is.
        for terminal in ["wezterm start --cwd /tmp", "wezterm start -- htop", "wezterm", "foot", "kitty --single-instance"] {
            assert_eq!(with_cwd_flag(terminal), terminal);
        }
        let run = |dir: &str| {
            let out = Command::new("/bin/sh")
                .arg("-c")
                .arg(format!("printf '[%s]' {}", with_cwd_flag("wezterm start")))
                .args(["sh", dir])
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap()
        };
        assert_eq!(run("/tmp/a b"), "[wezterm][start][--cwd][/tmp/a b]");
        assert_eq!(run(""), "[wezterm][start]");
    }

    #[test]
    fn process_ancestry() {
        let me = std::process::id();
        let mut child = std::process::Command::new("sleep").arg("5").spawn().unwrap();
        assert!(is_descendant(child.id(), me));
        assert!(is_descendant(me, me));
        assert!(!is_descendant(me, child.id()));
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn newest_child_is_the_last_started() {
        // A shell of its own (other tests start children of this process too): `sleep 7`
        // starts after `sleep 8` (start times tick in 1/100 s).
        let mut sh = std::process::Command::new("sh").args(["-c", "sleep 8 & sleep 0.05; sleep 7 & wait"]).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let newest = newest_child(sh.id()).unwrap();
        assert_eq!(std::fs::read_to_string(format!("/proc/{newest}/cmdline")).unwrap(), "sleep\07\0");
        assert_eq!(newest_child(newest), None);
        let _ = std::process::Command::new("pkill").args(["-P", &sh.id().to_string()]).status();
        sh.kill().unwrap();
        sh.wait().unwrap();
    }
}
