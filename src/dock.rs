//! The dock: tiles put away from a desktop (the `dock` key, `dock_click`, the tile options),
//! one dock per monitor and desktop. A tile of a tabbed column takes its whole column, tabs
//! and all. The `dock_show` key (Mod+Shift+e) shows the dock of the focused tile's monitor,
//! and a click on the bar's dock module (`dune dock show`) that of the bar's monitor: a bar
//! down its left edge with the app icon of each entry, over everything (`render`), the title
//! of the one under the pointer beside it. A click on one puts it back where it was, a right
//! click pins it: a pinned one is in the dock of every desktop, and comes out on the one
//! shown (a tile floating, a column right of the focused one). It stays until Esc or a click
//! away, and keys drive it (`DockKey`). The entry chosen, by keys or under the pointer, when
//! a tabbed column, fans out its tabs beside the bar (above it, along the bottom): a row per
//! tab, its icon and title; one taken out brings the column back with that tab shown.
//!
//! The scratchpad is the first cell of every dock: chosen, it fans out the windows waiting
//! there the same way; one taken out floats on the desktop shown (`show_scratchpad`), a
//! click on the cell takes out the first. Unlike the scratchpad, whose queue is the same
//! everywhere and only floats, the dock keeps each desktop's own windows.
//!
//! Bars follow each monitor's dock with `dune dock watch` (`watch_dock`), told its windows
//! now and on every change.

use std::{collections::HashMap, io::Write, os::unix::net::UnixStream};

use dune_common::{
    desktop::{App, app_icon},
    icons::{Icon, Icons},
};
use smithay::input::keyboard::Keysym;

use crate::{
    Dune,
    floating::{centered, keep_inside, start_size},
    layout::{Detached, GroupId, Home, Rect, Side, TileId},
    scratchpad::json_escape,
};

/// The dock's cells: their side, the icon in them, the room between them and around the bar.
pub const CELL: i32 = 48;
pub const ICON: i32 = 36;
const CELL_GAP: i32 = 4;
const MARGIN: i32 = 6;
/// Between the bar and the title of the entry under the pointer, and how wide that is at most.
pub const LABEL_GAP: i32 = 6;
pub const LABEL_WIDTH: i32 = 360;
/// Between the left edge of the room bars leave and the dock.
const MARGIN_LEFT: i32 = 8;
/// The scratchpad's cell, first in every dock: the icon it shows (by its name in the icon
/// theme), the letter without one, and what its drawings are kept by (no tile has it).
pub const SCRATCH_ICON: &str = "preferences-system-windows";
pub const SCRATCH_LETTER: char = '*';
pub const SCRATCH_KEY: TileId = TileId::MAX;
/// A row of the fan of a tabbed column's tabs: its icon's cell, and how wide its title.
pub const FAN_CELL: i32 = 40;
pub const FAN_TITLE: i32 = 220;

/// What a dock entry holds.
#[derive(Debug)]
pub enum Piece {
    /// One tile, with its place in the strip or where it floated.
    Tile { id: TileId, home: Option<Home>, float: Option<Rect> },
    /// A tabbed column, whole, with the column it sat by.
    Column { column: Detached, by: Option<(TileId, Side)> },
}

/// What a pinned entry taken out of the dock is remembered by, to be pinned when it goes
/// back: a tile, or a tabbed column by its id, however many tabs it has by then.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pin {
    Tile(TileId),
    Group(GroupId),
}

/// A dock entry.
#[derive(Debug)]
pub struct Docked {
    pub piece: Piece,
    /// In the dock of every desktop.
    pub pinned: bool,
    /// The monitor and desktop whose dock has it (not pinned).
    pub output: String,
    pub desk: u8,
}

impl Docked {
    pub fn tiles(&self) -> Vec<TileId> {
        match &self.piece {
            Piece::Tile { id, .. } => vec![*id],
            Piece::Column { column, .. } => column.tiles(),
        }
    }

    /// The tile it is named by: the one, or the column's shown tab.
    pub fn lead(&self) -> TileId {
        match &self.piece {
            Piece::Tile { id, .. } => *id,
            Piece::Column { column, .. } => column.active().unwrap_or_default(),
        }
    }

    /// The first tile of each of its column's tabs (a tile: itself).
    pub fn leads(&self) -> Vec<TileId> {
        match &self.piece {
            Piece::Tile { id, .. } => vec![*id],
            Piece::Column { column, .. } => column.leads(),
        }
    }

    /// What it is remembered by when taken out pinned (see `Pin`): its column's id, or the tile.
    fn pin(&self) -> Pin {
        match &self.piece {
            Piece::Column { column, .. } => column.id().map_or(Pin::Tile(self.lead()), Pin::Group),
            Piece::Tile { id, .. } => Pin::Tile(*id),
        }
    }

    /// How many tabs its column has (none for a tile).
    pub fn tabs(&self) -> usize {
        match &self.piece {
            Piece::Tile { .. } => 0,
            Piece::Column { column, .. } => column.tabs(),
        }
    }
}

/// What a key does to a dock shown for the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockKey {
    /// An arrow: along the bar it chooses the next entry, across it goes into the fan of a
    /// tabbed column's tabs and out (see `step`).
    Go(Dir),
    /// The chosen entry out of the dock (with the tab chosen in its fan shown).
    Pick,
    /// Pin the chosen entry, or unpin it.
    Pin,
    Close,
    /// Any other key: swallowed.
    Nothing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// What `sym` does to a dock shown for the keys: the arrows or h/j/k/l move, Enter takes
/// the entry out, p or Space pins it, Esc hides the dock. Other keys do nothing to it.
pub fn dock_key(sym: Keysym) -> Option<DockKey> {
    Some(match sym {
        Keysym::Up | Keysym::k => DockKey::Go(Dir::Up),
        Keysym::Down | Keysym::j => DockKey::Go(Dir::Down),
        Keysym::Left | Keysym::h => DockKey::Go(Dir::Left),
        Keysym::Right | Keysym::l => DockKey::Go(Dir::Right),
        Keysym::Return | Keysym::KP_Enter => DockKey::Pick,
        Keysym::p | Keysym::space => DockKey::Pin,
        Keysym::Escape => DockKey::Close,
        _ => return None,
    })
}

/// The line bars get for a dock holding windows with `titles`, and the scratchpad (its
/// first cell) `scratch`: the dock's count (0 too: the module stays, to be clicked), all
/// the titles as tooltip, class `empty` or `filled`.
pub fn status_line(titles: &[String], scratch: &[String]) -> String {
    let dock = std::iter::once("Dock".to_string()).chain(titles.iter().cloned());
    let scratch = (!scratch.is_empty()).then(|| std::iter::once("Scratchpad".to_string()).chain(scratch.iter().cloned()));
    let tooltip = dock.chain(scratch.into_iter().flatten()).collect::<Vec<_>>().join("\n");
    let class = if titles.is_empty() { "empty" } else { "filled" };
    format!(r#"{{"text":"{}","tooltip":"{}","class":"{class}"}}"#, titles.len(), json_escape(&tooltip))
}

/// Which way a dock's cells go: down the left edge, or along the bottom (over a bar there).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Down,
    Across,
}

/// The cells of a dock of `n` entries shown from the top left corner `origin`, top to bottom
/// or left to right.
pub fn rows(origin: (i32, i32), n: usize, shape: Shape) -> Vec<Rect> {
    let step = |i: i32| MARGIN + i * (CELL + CELL_GAP);
    (0..n as i32)
        .map(|i| match shape {
            Shape::Down => Rect::new(origin.0 + MARGIN, origin.1 + step(i), CELL, CELL),
            Shape::Across => Rect::new(origin.0 + step(i), origin.1 + MARGIN, CELL, CELL),
        })
        .collect()
}

/// The room a dock of `n` entries takes from `origin` (one cell at least: "empty").
pub fn panel(origin: (i32, i32), n: usize, shape: Shape) -> Rect {
    let n = n.max(1) as i32;
    let (thick, long) = (CELL + 2 * MARGIN, 2 * MARGIN + n * CELL + (n - 1) * CELL_GAP);
    match shape {
        Shape::Down => Rect::new(origin.0, origin.1, thick, long),
        Shape::Across => Rect::new(origin.0, origin.1, long, thick),
    }
}

/// Where arrow `dir` takes the choice of a `shape` dock of `entries` entries, entry
/// `selected` chosen, and `tab` of its fan of `tabs` tabs (none: the entry itself). Along
/// the bar the next entry is chosen, going round; the arrow away from the bar goes into the
/// fan (of one window too: the scratchpad's), its first tab, which goes on from the bar: down beside a bar down the left edge,
/// up above one along the bottom. The arrow back towards the bar leaves it (from its first
/// tab, along the bottom).
pub fn step(shape: Shape, entries: usize, (selected, tab): (usize, Option<usize>), tabs: usize, dir: Dir) -> (usize, Option<usize>) {
    let along = |d: i32| match entries {
        0 => (selected, None),
        n => ((selected as i32 + d).rem_euclid(n as i32) as usize, None),
    };
    let last = tabs.saturating_sub(1);
    match (shape, tab, dir) {
        (Shape::Down, None, Dir::Up) | (Shape::Across, _, Dir::Left) => along(-1),
        (Shape::Down, None, Dir::Down) | (Shape::Across, _, Dir::Right) => along(1),
        (Shape::Down, None, Dir::Right) | (Shape::Across, None, Dir::Up) if tabs > 0 => (selected, Some(0)),
        (Shape::Down, Some(_), Dir::Left) | (Shape::Across, Some(0), Dir::Down) => (selected, None),
        (Shape::Down, Some(k), Dir::Up) | (Shape::Across, Some(k), Dir::Down) => (selected, Some(k.saturating_sub(1))),
        (Shape::Down, Some(k), Dir::Down) | (Shape::Across, Some(k), Dir::Up) => (selected, Some((k + 1).min(last))),
        _ => (selected, tab),
    }
}

/// The rows of the fan of `n` tabs of the entry in `cell` of the dock in `panel`, kept in
/// `bounds`: down from beside the cell, or up from above the bar along the bottom.
pub fn fan_rows(cell: Rect, panel: Rect, n: usize, shape: Shape, bounds: Rect) -> Vec<Rect> {
    let (w, n) = (FAN_CELL + LABEL_GAP + FAN_TITLE, n as i32);
    let high = n * FAN_CELL + (n - 1).max(0) * CELL_GAP;
    let row = |x: i32, y: i32, k: i32| Rect::new(x, y + k * (FAN_CELL + CELL_GAP), w, FAN_CELL);
    match shape {
        Shape::Down => {
            let x = panel.x + panel.w + LABEL_GAP + MARGIN;
            let y = (cell.y + (cell.h - FAN_CELL) / 2).min(bounds.y + bounds.h - MARGIN - high).max(bounds.y + MARGIN);
            (0..n).map(|k| row(x, y, k)).collect()
        }
        Shape::Across => {
            let x = cell.x.min(bounds.x + bounds.w - MARGIN - w).max(bounds.x + MARGIN);
            let top = panel.y - LABEL_GAP - MARGIN - high;
            // The first tab nearest the bar.
            (0..n).map(|k| row(x, top, n - 1 - k)).collect()
        }
    }
}

/// The room a fan with `rows` takes, round them.
pub fn fan_back(rows: &[Rect]) -> Option<Rect> {
    let (x0, y0) = (rows.iter().map(|r| r.x).min()?, rows.iter().map(|r| r.y).min()?);
    let (x1, y1) = (rows.iter().map(|r| r.x + r.w).max()?, rows.iter().map(|r| r.y + r.h).max()?);
    Some(Rect::new(x0 - MARGIN, y0 - MARGIN, x1 - x0 + 2 * MARGIN, y1 - y0 + 2 * MARGIN))
}

/// What is under the pointer on the dock shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockHit {
    /// An entry, by its index in `Dune::dock`.
    Entry(usize),
    /// A row of the fan of entry `.0`: its tab `.1`, by place in its bar.
    Tab(usize, usize),
    /// The scratchpad's cell.
    Scratchpad,
    /// A row of the scratchpad's fan: its window `.0`, by place in the queue.
    Scratch(usize),
}

/// The app icons of the dock's entries, by app id: found on first use (the icon theme and
/// the desktop apps are read once), drawn `ICON` pixels square.
#[derive(Default)]
pub struct AppIcons {
    found: Option<(Icons, Vec<App>)>,
    /// The icon name of each app id seen.
    names: HashMap<String, Option<String>>,
}

impl AppIcons {
    /// The icon of the app `app_id`: its desktop entry's, else one named like it.
    pub fn get(&mut self, app_id: &str) -> Option<&Icon> {
        let (icons, apps) = self.found.get_or_insert_with(|| (Icons::new(""), dune_common::desktop::apps()));
        let name = self.names.entry(app_id.to_string()).or_insert_with(|| app_icon(apps, app_id)).as_deref()?;
        icons.get(name, ICON as u32)
    }
}

impl Dune {
    /// Whether `id` is in a dock.
    pub fn in_dock(&self, id: TileId) -> bool {
        self.dock.iter().any(|d| d.tiles().contains(&id))
    }

    /// `id` (with its column, when that has tabs) into the dock of the desktop it is on,
    /// pinned when it came out of the dock pinned. Focus goes to the tile focused before it there.
    pub fn send_to_dock(&mut self, id: TileId) {
        if !self.tiles.contains_key(&id) || self.in_dock(id) || self.in_scratchpad(id) {
            return;
        }
        let Some(output) = self.output_of(id) else { return };
        let desk = self.desk_of(&output);
        // Its column, by the id it keeps however many tabs it has: one pinned out of the
        // dock goes back pinned, though left with one tab (then only the tile goes).
        let group = self.layouts.get(&output).and_then(|l| l.group_id(id));
        let piece = match self.float_rect(id) {
            Some(rect) => {
                self.remove_from_layouts(id);
                Piece::Tile { id, home: None, float: Some(rect) }
            }
            None => {
                let Some(layout) = self.layouts.get_mut(&output) else { return };
                if layout.group(id).is_some() {
                    let Some((column, by)) = layout.take_column(id) else { return };
                    Piece::Column { column, by }
                } else {
                    let home = layout.home(id);
                    self.remove_from_layouts(id);
                    Piece::Tile { id, home, float: None }
                }
            }
        };
        let mut entry = Docked { piece, pinned: false, output: output.clone(), desk };
        let tiles = entry.tiles();
        // A tile pinned alone that has gained tabs since goes back pinned too.
        let pins = group.map(Pin::Group).into_iter().chain(tiles.iter().copied().map(Pin::Tile));
        entry.pinned = pins.fold(false, |pinned, p| self.dock_pins.remove(&p) | pinned);
        tracing::info!(?tiles, output, desk, pinned = entry.pinned, "into the dock");
        self.hide_windows(&tiles);
        self.dock.push(entry);
        if self.focused.is_some_and(|f| tiles.contains(&f)) {
            let on = |t: TileId| self.output_of(t).as_deref() == Some(output.as_str());
            self.focused = self.recent.iter().copied().find(|t| on(*t)).or_else(|| {
                let floats = self.floating.get(&output).into_iter().flatten().map(|(t, _)| *t);
                floats.chain(self.layouts.get(&output).map(|l| l.tiles()).unwrap_or_default()).next()
            });
        }
        self.arrange();
        self.update_focus();
    }

    /// The entries of the dock `output` shows, by their index in `Dune::dock`: the pinned
    /// ones first, then those of the desktop it shows.
    pub fn dock_entries(&self, output: &str) -> Vec<usize> {
        let desk = self.desk_of(output);
        let pinned = (0..self.dock.len()).filter(|i| self.dock[*i].pinned);
        let local = (0..self.dock.len()).filter(|i| {
            let d = &self.dock[*i];
            !d.pinned && d.output == output && d.desk == desk
        });
        pinned.chain(local).collect()
    }

    /// Entry `i` of the dock out, focused: back where it was, or (pinned, or its place
    /// gone) on the desktop the active monitor shows.
    pub fn undock(&mut self, i: usize) {
        if i >= self.dock.len() {
            return;
        }
        let entry = self.dock.remove(i);
        let output = match entry.pinned {
            false => Some(entry.output.clone()),
            true => self.active_output().map(|o| o.name()),
        };
        let area = self.space.outputs().find(|o| Some(o.name()) == output).and_then(|o| self.output_rect(o));
        let (Some(output), Some(area)) = (output, area) else {
            self.dock.insert(i, entry);
            return;
        };
        let lead = entry.lead();
        // Next to the focused tile of that monitor's strip, when its place is gone.
        let layout_has = |d: &Dune, t: TileId| d.layouts.get(&output).is_some_and(|l| l.contains(t));
        let beside = self.focused.filter(|f| layout_has(self, *f)).map(|f| (f, Side::Right));
        tracing::info!(tiles = ?entry.tiles(), output, pinned = entry.pinned, "out of the dock");
        if entry.pinned {
            self.dock_pins.insert(entry.pin());
        }
        match entry.piece {
            Piece::Tile { id, float, .. } if entry.pinned => self.float_tile(id, &output, back_at(float, area)),
            Piece::Tile { id, float: Some(rect), .. } => self.float_tile(id, &output, keep_inside(rect, area)),
            Piece::Tile { id, home, float: None } => {
                let layout = self.layouts.entry(output.clone()).or_default();
                if !home.is_some_and(|h| layout.restore(id, h)) {
                    layout.insert_column(id, beside.map(|(t, _)| t), Side::Right);
                }
            }
            Piece::Column { column, by } => {
                let layout = self.layouts.entry(output.clone()).or_default();
                let by = if entry.pinned { beside } else { by };
                if let Err(column) = layout.put_column(column, by) {
                    let _ = layout.put_column(column, beside);
                }
            }
        }
        self.focus_tile(lead);
    }

    /// Entry `i` of the dock out (see `undock`), the tab of its column `tab` is in shown.
    pub fn undock_showing(&mut self, i: usize, tab: TileId) {
        if let Some(Docked { piece: Piece::Column { column, .. }, .. }) = self.dock.get_mut(i) {
            column.show(tab);
        }
        self.undock(i);
    }

    /// The scratchpad's window `k` (none: the first) out, floating on the desktop shown.
    pub fn take_from_scratchpad(&mut self, k: Option<usize>) {
        let tiles = self.scratchpad_tiles();
        if let Some(id) = k.and_then(|k| tiles.get(k)).or(tiles.first()) {
            self.show_scratchpad(*id);
        }
    }

    /// Pin entry `i` of the dock, or unpin it: then it belongs to the dock of the desktop
    /// `output` shows.
    pub fn toggle_dock_pin(&mut self, i: usize, output: &str) {
        let desk = self.desk_of(output);
        let Some(entry) = self.dock.get_mut(i) else { return };
        entry.pinned = !entry.pinned;
        if !entry.pinned {
            entry.output = output.to_string();
            entry.desk = desk;
        }
        tracing::info!(tile = entry.lead(), pinned = entry.pinned, "dock pin");
        self.refresh_dock();
    }

    /// What the dock and bars call entry `entry`: its shown tile's title, with how many
    /// other tabs its column has and a pin when pinned.
    pub fn dock_title(&self, entry: &Docked) -> String {
        let mut title = self.shown_title(entry.lead());
        if entry.tabs() > 1 {
            title = format!("{title} (+{})", entry.tabs() - 1);
        }
        if entry.pinned {
            title = format!("📌 {title}");
        }
        title
    }

    /// Hide the dock shown.
    pub fn close_dock(&mut self) {
        self.dock_open = None;
        self.dock_keys = false;
        self.dock_tab = None;
    }

    /// The `dock_show` key, or a click on a bar's dock (`dune dock show`): show the dock of
    /// `output` (none: the active monitor, the one desktops switch on; the focused tile can
    /// be on another one, when the active one shows an empty desktop) for the keys, its
    /// first cell (the scratchpad) chosen; hide it when shown.
    pub fn toggle_dock_keys(&mut self, output: Option<String>) {
        if self.dock_open.is_some() {
            return self.close_dock();
        }
        let exists = |o: &String| self.space.outputs().any(|out| out.name() == *o);
        let output = output.filter(exists).or_else(|| self.active_output().map(|o| o.name()));
        self.dock_open = output;
        self.dock_keys = self.dock_open.is_some();
        self.dock_selected = 0;
        self.dock_tab = None;
    }

    /// A key on the dock shown for the keys (see `dock_key`).
    pub fn press_dock_key(&mut self, key: DockKey) {
        let Some(output) = self.dock_open.clone() else { return };
        let entries = self.dock_entries(&output);
        // The entry chosen; none on the scratchpad's cell, the first.
        let chosen = self.dock_selected.checked_sub(1).and_then(|row| entries.get(row).copied());
        match key {
            DockKey::Go(dir) => {
                let Some((_, shape)) = self.dock_origin(&output) else { return };
                let tabs = match chosen {
                    Some(i) => Some(self.dock[i].tabs()).filter(|n| *n > 1).unwrap_or(0),
                    None => self.scratchpad_tiles().len(),
                };
                (self.dock_selected, self.dock_tab) = step(shape, entries.len() + 1, (self.dock_selected, self.dock_tab), tabs, dir);
            }
            DockKey::Nothing => {}
            DockKey::Pick if self.dock_selected == 0 => {
                let tab = self.dock_tab;
                self.close_dock();
                self.take_from_scratchpad(tab);
            }
            DockKey::Pick => {
                if let Some(i) = chosen {
                    let tab = self.dock_tab.and_then(|k| self.dock[i].leads().get(k).copied());
                    self.close_dock();
                    match tab {
                        Some(tab) => self.undock_showing(i, tab),
                        None => self.undock(i),
                    }
                }
            }
            DockKey::Pin => {
                if let Some(i) = chosen {
                    self.toggle_dock_pin(i, &output);
                    // Pinned ones go first: the choice follows the entry.
                    self.dock_selected = self.dock_entries(&output).iter().position(|e| *e == i).map_or(0, |row| row + 1);
                    self.dock_tab = None;
                }
            }
            DockKey::Close => self.close_dock(),
        }
    }

    /// The line bars following `output`'s dock (none: the active monitor's) get.
    fn dock_line(&self, output: Option<&str>) -> String {
        let output = output.map(String::from).or_else(|| self.active_output().map(|o| o.name()));
        let entries = output.map(|o| self.dock_entries(&o)).unwrap_or_default();
        status_line(&entries.into_iter().map(|i| self.dock_title(&self.dock[i])).collect::<Vec<_>>(), &self.scratchpad_titles())
    }

    /// A `watch_dock` connection for `output`'s dock: told the state now, and on every change.
    pub fn watch_dock(&mut self, mut stream: UnixStream, output: Option<String>) {
        let line = self.dock_line(output.as_deref());
        if writeln!(stream, "{line}").is_ok() && stream.set_nonblocking(true).is_ok() {
            self.dock_watchers.push((stream, output, line));
        }
    }

    /// After every `arrange`: the watchers get their dock's new state when it changed; those
    /// gone (or not reading) are dropped.
    pub fn refresh_dock(&mut self) {
        let lines: Vec<String> = self.dock_watchers.iter().map(|(_, output, _)| self.dock_line(output.as_deref())).collect();
        let watchers = std::mem::take(&mut self.dock_watchers);
        self.dock_watchers = watchers
            .into_iter()
            .zip(lines)
            .filter_map(|((mut stream, output, last), line)| match line == last {
                true => Some((stream, output, last)),
                false => writeln!(stream, "{line}").ok().map(|_| (stream, output, line)),
            })
            .collect();
    }

    /// `id` leaves the dock (it closed, or went somewhere else).
    pub fn remove_from_dock(&mut self, id: TileId) {
        self.dock.retain_mut(|d| match &mut d.piece {
            Piece::Tile { id: t, .. } => *t != id,
            Piece::Column { column, .. } => !column.tiles().contains(&id) || column.remove(id),
        });
    }

    /// The docks of monitors that went away go to `first`.
    pub fn adopt_docks(&mut self, alive: &[String], first: &str) {
        for entry in self.dock.iter_mut().filter(|d| !alive.contains(&d.output)) {
            entry.output = first.to_string();
        }
    }

    /// How many cells `output`'s dock has: the scratchpad's and one per entry.
    fn dock_cells(&self, output: &str) -> usize {
        self.dock_entries(output).len() + 1
    }

    /// Where `output`'s dock shows, and which way: down from the top left corner of the room
    /// its bars leave, past `MARGIN_LEFT`; along the bottom of that room, centered on it, when
    /// its bars are only at the bottom.
    pub fn dock_origin(&self, output: &str) -> Option<((i32, i32), Shape)> {
        let o = self.space.outputs().find(|o| o.name() == output)?;
        let (geo, area) = (self.space.output_geometry(o)?, self.output_rect(o)?);
        let bottom_bars = area.y == geo.loc.y && area.h < geo.size.h;
        Some(match bottom_bars {
            true => {
                let long = panel((0, 0), self.dock_cells(output), Shape::Across).w;
                ((area.x + (area.w - long) / 2, area.y + area.h - (CELL + 2 * MARGIN)), Shape::Across)
            }
            false => ((area.x + MARGIN_LEFT, area.y), Shape::Down),
        })
    }

    /// The room the dock of `output` takes.
    pub fn dock_panel(&self, output: &str) -> Option<Rect> {
        let (origin, shape) = self.dock_origin(output)?;
        Some(panel(origin, self.dock_cells(output), shape))
    }

    /// The fan of the cell chosen on the dock shown, when a tabbed column or the scratchpad
    /// holding windows: the entry, by its index in `Dune::dock` (none: the scratchpad), its
    /// tabs' first tiles (the scratchpad's windows) and their rows.
    pub fn dock_fan(&self) -> Option<(Option<usize>, Vec<TileId>, Vec<Rect>)> {
        let output = self.dock_open.as_deref()?;
        let ((origin, shape), panel) = (self.dock_origin(output)?, self.dock_panel(output)?);
        let o = self.space.outputs().find(|o| o.name() == output)?;
        let bounds = self.output_rect(o)?;
        let entries = self.dock_entries(output);
        let cell = *rows(origin, entries.len() + 1, shape).get(self.dock_selected)?;
        let (owner, leads) = match self.dock_selected.checked_sub(1) {
            None => (None, self.scratchpad_tiles()),
            Some(row) => {
                let i = *entries.get(row)?;
                if self.dock[i].tabs() < 2 {
                    return None;
                }
                (Some(i), self.dock[i].leads())
            }
        };
        if leads.is_empty() {
            return None;
        }
        let fan = fan_rows(cell, panel, leads.len(), shape, bounds);
        Some((owner, leads, fan))
    }

    /// What of the shown dock is under `pos`, and whether `pos` is on the dock (or its
    /// fan) at all.
    pub fn dock_at(&self, pos: (f64, f64)) -> (Option<DockHit>, bool) {
        let Some(output) = &self.dock_open else { return (None, false) };
        let (Some((origin, shape)), Some(panel)) = (self.dock_origin(output), self.dock_panel(output)) else { return (None, false) };
        let entries = self.dock_entries(output);
        let fan = self.dock_fan();
        let tab = fan.as_ref().and_then(|(owner, _, rows)| {
            let k = rows.iter().position(|r| r.contains(pos.0, pos.1))?;
            Some(owner.map_or(DockHit::Scratch(k), |i| DockHit::Tab(i, k)))
        });
        let on_fan = fan.as_ref().and_then(|(_, _, rows)| fan_back(rows)).is_some_and(|b| b.contains(pos.0, pos.1));
        let row = rows(origin, entries.len() + 1, shape).iter().position(|r| r.contains(pos.0, pos.1));
        let row = row.map(|r| r.checked_sub(1).map_or(DockHit::Scratchpad, |r| DockHit::Entry(entries[r])));
        (tab.or(row), on_fan || panel.contains(pos.0, pos.1))
    }

    /// The pointer moved to `pos`: over a cell of the dock shown (the scratchpad's too), or
    /// a row of its fan, that is chosen.
    pub fn dock_hover(&mut self, pos: (f64, f64)) {
        let Some(output) = self.dock_open.clone() else { return };
        let row = match self.dock_at(pos).0 {
            Some(DockHit::Scratchpad) => 0,
            Some(DockHit::Entry(i)) => self.dock_entries(&output).iter().position(|e| *e == i).map_or(0, |row| row + 1),
            Some(DockHit::Tab(_, k) | DockHit::Scratch(k)) => {
                self.dock_tab = Some(k);
                return;
            }
            None => return,
        };
        if row != self.dock_selected {
            (self.dock_selected, self.dock_tab) = (row, None);
        }
    }
}

/// Where a pinned tile out of the dock floats on `area`: where it floated before (`rect`),
/// kept inside it, else in the middle.
fn back_at(rect: Option<Rect>, area: Rect) -> Rect {
    rect.map(|r| keep_inside(r, area)).unwrap_or_else(|| {
        let (w, h) = start_size(area);
        centered(area, w, h)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_go_down_from_the_corner() {
        let r = rows((100, 0), 2, Shape::Down);
        assert_eq!(r[0], Rect::new(106, 6, CELL, CELL));
        assert_eq!(r[1], Rect::new(106, 6 + CELL + 4, CELL, CELL));
        assert_eq!(panel((100, 0), 2, Shape::Down), Rect::new(100, 0, CELL + 12, 12 + 2 * CELL + 4));
        // Empty, it still has room for one bar.
        assert_eq!(panel((0, 0), 0, Shape::Down), panel((0, 0), 1, Shape::Down));
    }

    #[test]
    fn cells_go_along_the_bottom() {
        let r = rows((100, 500), 2, Shape::Across);
        assert_eq!(r[0], Rect::new(106, 506, CELL, CELL));
        assert_eq!(r[1], Rect::new(106 + CELL + 4, 506, CELL, CELL));
        assert_eq!(panel((100, 500), 2, Shape::Across), Rect::new(100, 500, 12 + 2 * CELL + 4, CELL + 12));
    }

    #[test]
    fn keys_drive_the_dock() {
        assert_eq!(dock_key(Keysym::Down), Some(DockKey::Go(Dir::Down)));
        assert_eq!(dock_key(Keysym::k), Some(DockKey::Go(Dir::Up)));
        assert_eq!(dock_key(Keysym::l), Some(DockKey::Go(Dir::Right)));
        assert_eq!(dock_key(Keysym::Return), Some(DockKey::Pick));
        assert_eq!(dock_key(Keysym::space), Some(DockKey::Pin));
        assert_eq!(dock_key(Keysym::Escape), Some(DockKey::Close));
        assert_eq!(dock_key(Keysym::a), None);
    }

    #[test]
    fn arrows_go_along_the_bar_and_into_the_fan() {
        // Down the left edge: up/down choose entries, right goes into the fan, left out.
        assert_eq!(step(Shape::Down, 3, (2, None), 0, Dir::Down), (0, None));
        assert_eq!(step(Shape::Down, 3, (1, None), 3, Dir::Right), (1, Some(0)));
        assert_eq!(step(Shape::Down, 3, (1, None), 0, Dir::Right), (1, None));
        // A fan of one (the scratchpad holding a window).
        assert_eq!(step(Shape::Down, 3, (0, None), 1, Dir::Right), (0, Some(0)));
        assert_eq!(step(Shape::Down, 3, (1, Some(0)), 3, Dir::Down), (1, Some(1)));
        assert_eq!(step(Shape::Down, 3, (1, Some(2)), 3, Dir::Down), (1, Some(2)));
        assert_eq!(step(Shape::Down, 3, (1, Some(2)), 3, Dir::Left), (1, None));
        // Along the bottom: left/right choose entries (leaving the fan), up goes in and on.
        assert_eq!(step(Shape::Across, 3, (0, None), 2, Dir::Left), (2, None));
        assert_eq!(step(Shape::Across, 3, (0, None), 2, Dir::Up), (0, Some(0)));
        assert_eq!(step(Shape::Across, 3, (0, Some(0)), 2, Dir::Up), (0, Some(1)));
        assert_eq!(step(Shape::Across, 3, (0, Some(1)), 2, Dir::Down), (0, Some(0)));
        assert_eq!(step(Shape::Across, 3, (0, Some(0)), 2, Dir::Down), (0, None));
        assert_eq!(step(Shape::Across, 3, (0, Some(1)), 2, Dir::Right), (1, None));
    }

    #[test]
    fn the_fan_goes_on_from_the_bar() {
        let bounds = Rect::new(0, 0, 1000, 800);
        let cell = Rect::new(14, 64, CELL, CELL);
        let panel = Rect::new(8, 0, CELL + 12, 200);
        let rows = fan_rows(cell, panel, 2, Shape::Down, bounds);
        let x = 8 + CELL + 12 + LABEL_GAP + 6;
        assert_eq!(rows[0], Rect::new(x, 68, FAN_CELL + LABEL_GAP + FAN_TITLE, FAN_CELL));
        assert_eq!(rows[1].y, 68 + FAN_CELL + 4);
        // Kept on the screen at the bottom.
        let low = Rect::new(14, 760, CELL, CELL);
        assert_eq!(fan_rows(low, panel, 2, Shape::Down, bounds)[1].y + FAN_CELL, 800 - 6);
        // Along the bottom: up from above the bar, the first tab nearest it.
        let panel = Rect::new(8, 740, 200, CELL + 12);
        let rows = fan_rows(Rect::new(14, 746, CELL, CELL), panel, 2, Shape::Across, bounds);
        assert_eq!(rows[0].y + FAN_CELL, 740 - LABEL_GAP - 6);
        assert_eq!(rows[1].y + FAN_CELL + 4, rows[0].y);
        assert_eq!(fan_back(&rows), Some(Rect::new(8, rows[1].y - 6, FAN_CELL + LABEL_GAP + FAN_TITLE + 12, 2 * FAN_CELL + 4 + 12)));
    }

    #[test]
    fn the_bar_line_counts_the_docked_windows() {
        assert_eq!(status_line(&[], &[]), r#"{"text":"0","tooltip":"Dock","class":"empty"}"#);
        let line = status_line(&["📌 vim \"a\"".into(), "htop".into()], &[]);
        assert_eq!(line, r#"{"text":"2","tooltip":"Dock\n📌 vim \"a\"\nhtop","class":"filled"}"#);
        // The scratchpad's windows after them, not counted.
        let line = status_line(&["htop".into()], &["foot".into()]);
        assert_eq!(line, r#"{"text":"1","tooltip":"Dock\nhtop\nScratchpad\nfoot","class":"filled"}"#);
    }

    #[test]
    fn a_pinned_tile_comes_back_inside_the_monitor() {
        let area = Rect::new(0, 0, 1000, 800);
        assert_eq!(back_at(Some(Rect::new(1500, 100, 300, 200)), area), Rect::new(700, 100, 300, 200));
        let r = back_at(None, area);
        assert_eq!((r.x + r.w / 2, r.y + r.h / 2), (500, 400));
    }
}
