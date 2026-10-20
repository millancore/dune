//! Tile management: spawning terminal tiles, placing new windows, swallowing and focus.

use std::{collections::HashMap, path::PathBuf, process::Command};

use smithay::{
    backend::input::{InputTime, KeyState},
    desktop::{Window, layer_map_for_output},
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

use dune_common::protocol::{Request, Side, direction_answer};

use crate::{
    launcher::Spot,
    layout::{Direction, Layout, Rect, Split, TileId, column_zone, drop_column, drop_side, drop_zone, neighbor},
    state::{Dune, Tile},
};

/// Space around and between columns; the focus border is drawn inside it. Tiles inside
/// a column are only `BORDER` apart, so neighbours share one border line.
pub const GAP: i32 = 6;
pub const BORDER: i32 = 1;

/// Where a tile being dragged would go: next to `target` on `side` in `output`'s
/// layout (at its end without a target), in a column of its own when `column`,
/// shown as `area` while dragging.
pub struct DropSpot {
    pub output: String,
    pub target: Option<TileId>,
    pub side: Side,
    pub column: bool,
    pub area: Rect,
}

impl Dune {
    /// Open a new terminal tile next to `target` (or the focused tile).
    ///
    /// The tile is reserved in the layout right away but takes no space until the
    /// terminal maps its window. The terminal runs dune's shell wrapper as its
    /// `$SHELL` (see `crate::shell`), which starts the user's shell; `run` is handed
    /// to the wrapper through `DUNE_RUN` and executed as if typed as `@<run>`.
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

    /// Start a terminal for tile `id` (its `DUNE_TILE`), running `run` like `spawn_terminal`.
    /// `launch` is the launcher's `DUNE_LAUNCH` number when the launcher placed it.
    pub fn start_terminal(&mut self, id: TileId, cwd: Option<PathBuf>, run: Option<String>, launch: Option<u64>) -> bool {
        let terminal = terminal_command(&self.config.terminal);
        let mut cmd = Command::new("/bin/sh");
        // `exec` keeps the pid, so the Wayland client pid is the terminal's own.
        cmd.arg("-c").arg(format!("exec {terminal}"));
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
        match cwd.filter(|d| d.is_dir()).or_else(|| std::env::var_os("HOME").map(PathBuf::from)) {
            Some(dir) => {
                cmd.current_dir(dir);
            }
            None => {}
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
    fn insert_tile(&mut self, id: TileId, target: Option<TileId>, side: Option<Side>) -> bool {
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

    /// A new tile stacked on `target` (see `Layout::stack_on`). None for a target out of
    /// the strip (floating): windows then stack inside it instead.
    pub fn stack_tile(&mut self, target: TileId) -> Option<TileId> {
        let id = self.next_tile;
        let name = self.output_of(target)?;
        if !self.layouts.get_mut(&name)?.stack_on(id, target) {
            return None;
        }
        self.next_tile += 1;
        self.tiles.insert(id, Tile::default());
        Some(id)
    }

    /// Take `id` out of whichever layout or floating stack holds it, shown or not.
    pub fn remove_from_layouts(&mut self, id: TileId) {
        self.float_homes.remove(&id);
        for layout in self.layouts.values_mut() {
            layout.remove(id);
        }
        for stack in self.floating.values_mut() {
            stack.retain(|(t, _)| *t != id);
        }
        self.remove_from_hidden(id);
    }

    /// The tile focus goes to when `id` leaves its spot: a neighbour, else the first of its output.
    pub fn neighbor_of(&self, id: TileId) -> Option<TileId> {
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

    /// The pointer moved: the output under it becomes the active one.
    pub fn pointer_at(&mut self, pos: Point<f64, Logical>) {
        if let Some(output) = self.space.output_under(pos).next() {
            self.active_output = Some(output.name());
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
    /// 2. the first window of an app the launcher started (`DUNE_LAUNCH`, or its pid) goes where the
    ///    launcher was told: a new tile stacked on the focused one (which folds into a
    ///    tab), or a new tile on some monitor and desktop;
    /// 3. a program started with `@prog` swallows the tile it was started from: its
    ///    process descends from a shell that announced a swallow for that tile;
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
        let launch = launch_n.and_then(|n| self.launches.remove(&n));
        let swallow = pid.and_then(|pid| {
            self.swallows
                .iter()
                .find(|(tile, shell)| self.tiles.contains_key(tile) && is_descendant(pid, **shell))
                .map(|(tile, _)| *tile)
        });

        let stack_on = |spot: &Spot| match spot {
            Spot::Stack(t) => Some(*t),
            Spot::New { .. } => None,
        };
        let stack_target = launch.as_ref().and_then(|l| stack_on(&l.spot)).filter(|t| self.tiles.contains_key(t));
        let (id, how) = if let Some(id) = env_tile.filter(|t| self.tiles.get(t).is_some_and(|t| t.stack.is_empty())) {
            (id, "terminal")
        } else if let Some(id) = stack_target.and_then(|t| self.stack_tile(t)) {
            (id, "stacked on its tile")
        } else if let Some(id) = stack_target.or(swallow).filter(|t| self.tiles.contains_key(t)) {
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
            (id, "new tile")
        };
        tracing::info!(tile = id, "placing window: {how}");

        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::TiledLeft);
            state.states.set(xdg_toplevel::State::TiledRight);
            state.states.set(xdg_toplevel::State::TiledTop);
            state.states.set(xdg_toplevel::State::TiledBottom);
        });

        self.tiles.get_mut(&id).unwrap().stack.push(window.clone());
        // On a desktop not shown, the window waits unmapped and focus stays where it is.
        if self.hidden_desk(id).is_none() {
            self.space.map_element(window, (0, 0), false);
            self.focused = Some(id);
            self.raise_float(id);
        }
        self.arrange();
        self.update_focus();
    }

    /// A toplevel went away: reveal what it swallowed, or drop the tile when it was the last window.
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
            if pos == tile.stack.len() && self.hidden_desk(id).is_none() {
                self.space.map_element(top, (0, 0), false);
            }
        } else {
            tracing::info!(tile = id, "window closed, tile removed");
            self.directions.remove(&id);
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

        // A focused tab (by keyboard, click or after a close) opens.
        if let Some(id) = self.focused {
            self.layouts.values_mut().for_each(|l| l.open_tab(id));
        }

        self.settle_floats();

        let tiles = &self.tiles;
        let visible = |id| tiles.get(&id).is_some_and(|t: &Tile| !t.stack.is_empty());
        let mut geometries = Vec::new();
        let mut shown = Vec::new();
        let mut tabs = Vec::new();
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
            for (id, rect) in layout.geometries(area, GAP, BORDER, &visible, self.focused) {
                if layout.is_tab(id) {
                    tabs.push(id);
                }
                if let Some(part) = rect.intersection(&area) {
                    shown.push((id, part));
                }
                geometries.push((id, rect));
            }
        }
        tracing::debug!(?geometries, shown = ?shown.iter().map(|(id, _)| id).collect::<Vec<_>>(), ?tabs, "arranged");
        self.geometries = geometries;
        self.shown = shown;
        self.tabs = tabs;

        // Back to front: mapping an element puts it on top of the space, so floats end up
        // above the strip and clicks reach them.
        for (id, rect) in self.geometries.iter().rev() {
            let Some(window) = self.tiles[id].top().cloned() else {
                continue;
            };
            let floating = self.is_floating(*id);
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
            });
            // Before the initial commit the size rides along with the initial configure.
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
        self.refresh_workspaces();
    }

    /// Give keyboard focus to the visible window of the focused tile.
    pub fn update_focus(&mut self) {
        if self.focused.is_none_or(|id| !self.tiles.contains_key(&id)) {
            self.focused = self.geometries.first().map(|(id, _)| *id);
        }
        let focused_window = self.focused.and_then(|id| self.tiles[&id].top().cloned());

        for window in self.space.elements() {
            let toplevel = window.toplevel().unwrap();
            if window.set_activated(Some(window) == focused_window.as_ref()) && toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }

        // A layer that asked for the keyboard (launcher) keeps it while open.
        let surface: Option<WlSurface> = self
            .layer_focus
            .clone()
            .or_else(|| focused_window.map(|w| w.toplevel().unwrap().wl_surface().clone()));
        let keyboard = self.seat.get_keyboard().unwrap();
        keyboard.set_focus(self, surface, SERIAL_COUNTER.next_serial());
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
            // Tiles on different monitors trade places between their layouts too.
            for layout in self.layouts.values_mut() {
                layout.swap(from, other);
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

    /// The output next to the one holding `id`, in `dir`.
    fn output_toward(&self, id: TileId, dir: Direction) -> Option<String> {
        let name = self.output_of(id)?;
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

    /// A Mod+drag ended at `pos`: the tile goes next to the tile under it (on the side
    /// nearest to the pointer; at its left or right edge, in a new column there), or in a
    /// new column at the end of the output when no tile is under it.
    pub fn drop_tile(&mut self, id: TileId, pos: Point<f64, Logical>) {
        if let Some(spot) = self.drop_spot(id, pos) {
            self.move_tile(id, &spot.output, spot.target, spot.side, spot.column);
        }
    }

    /// Where `id` would land if dropped at `pos`, or `None` when nothing would move.
    pub fn drop_spot(&self, id: TileId, pos: Point<f64, Logical>) -> Option<DropSpot> {
        // Tiles land in the strip: a float under the pointer is looked through.
        let under = self.shown.iter().find(|(t, r)| *t != id && !self.is_floating(*t) && r.contains(pos.x, pos.y)).copied();
        if let Some((target, rect)) = under {
            let side = drop_side(&rect, pos.x, pos.y);
            let output = self.output_of(target)?;
            let column = drop_column(&rect, pos.x, side);
            let area = if column { column_zone(&rect, side) } else { drop_zone(&rect, side) };
            return Some(DropSpot { output, target: Some(target), side, column, area });
        }
        // Dropped on the tile itself: nothing moves.
        if self.shown.iter().any(|(t, r)| *t == id && r.contains(pos.x, pos.y)) {
            return None;
        }
        // Empty space: a column of its own at the end of the strip there.
        let output = self.space.output_under(pos).next()?;
        let area = self.output_rect(output)?;
        Some(DropSpot { output: output.name(), target: None, side: Side::Right, column: true, area })
    }

    /// Apply a layout change to the focused tile's layout, then lay everything out again.
    pub fn change_focused(&mut self, change: impl Fn(&mut Layout, TileId)) {
        if let Some(id) = self.focused {
            self.change_tile(id, change);
        }
    }

    pub fn change_tile(&mut self, id: TileId, change: impl Fn(&mut Layout, TileId)) {
        if let Some(layout) = self.layouts.values_mut().find(|l| l.contains(id)) {
            change(layout, id);
            self.arrange();
        }
    }

    pub fn close_focused(&mut self) {
        if let Some(window) = self.focused.and_then(|id| self.tiles[&id].top()) {
            window.toplevel().unwrap().send_close();
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

    /// Pid of the process behind a Wayland client.
    fn client_pid(&self, surface: &WlSurface) -> Option<u32> {
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
            Request::Swallow { tile, shell_pid } => {
                self.swallows.insert(tile, shell_pid);
            }
            Request::Unswallow { tile } => {
                self.swallows.remove(&tile);
            }
            // The indicator is used once: asking for it clears it.
            Request::Direction { tile } => return Some(direction_answer(self.directions.remove(&tile))),
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

    /// Point the focused tile's direction indicator at `side`; the same side again clears it.
    pub fn toggle_direction(&mut self, side: Side) {
        let Some(id) = self.focused else { return };
        if self.directions.get(&id) == Some(&side) {
            self.directions.remove(&id);
        } else {
            self.directions.insert(id, side);
        }
    }
}

fn neighbor_or_first(geoms: &[(TileId, Rect)], id: TileId, order: Vec<TileId>) -> Option<TileId> {
    [Direction::Left, Direction::Up, Direction::Right, Direction::Down]
        .into_iter()
        .find_map(|dir| neighbor(geoms, id, dir))
        .or_else(|| order.into_iter().find(|t| *t != id))
}

/// The terminal to run. dune's own `dune-term` is looked up next to the dune binary
/// first, so it is found even when its directory is not on the session's `PATH`.
fn terminal_command(configured: &str) -> String {
    if configured == "dune-term"
        && let Some(sibling) = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join("dune-term")))
        && sibling.is_file()
    {
        return sibling.to_string_lossy().into_owned();
    }
    configured.to_string()
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
fn is_descendant(mut pid: u32, ancestor: u32) -> bool {
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
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name is in parentheses and may contain spaces: parse after the last `)`.
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
