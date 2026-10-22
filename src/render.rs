//! What dune draws besides the clients — tile borders, tab
//! bars and, on a real session, the pointer — shared by both backends.

use std::{collections::HashMap, time::Duration};

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer,
        element::{
            AsRenderElements, Kind,
            memory::MemoryRenderBufferRenderElement,
            render_elements,
            utils::CropRenderElement,
            solid::{SolidColorBuffer, SolidColorRenderElement},
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        },
    },
    desktop::{PopupManager, layer_map_for_output},
    input::pointer::{CursorImageAttributes, CursorImageStatus},
    output::Output,
    reexports::wayland_server::Resource,
    utils::{Physical, Point, Rectangle, Scale},
    wayland::shell::wlr_layer::Layer,
    wayland::compositor::with_states,
};

use crate::{
    Dune,
    layout::{Rect, Side, TileId},
    tab_title::WAVES,
    tiling::{BORDER, GAP},
};

pub const BACKGROUND: [f32; 4] = [0.08, 0.08, 0.1, 1.0];
const FOCUS_COLOR: [f32; 4] = [0.36, 0.62, 0.95, 1.0];
/// Where a dragged tile will land (premultiplied, translucent).
const DROP_COLOR: [f32; 4] = [0.36 * 0.35, 0.62 * 0.35, 0.95 * 0.35, 0.35];
/// How far the marks of tiles past a screen edge float in from it, in pixels.
const EDGE_MARK_INSET: i32 = 4;
/// How thick the edge a Mod+click would split is drawn.
const SPLIT_EDGE_WIDTH: i32 = 3;
/// The frame around what a recording takes: the red of the bar's recording mark.
const RECORDING_COLOR: [f32; 4] = [0xef as f32 / 255.0, 0x44 as f32 / 255.0, 0x44 as f32 / 255.0, 1.0];
/// How thick that frame is, drawn just outside the area so the video does not show it.
const RECORDING_FRAME: i32 = 2;
/// Darkens the tile being dragged (premultiplied, translucent).
const DRAGGED_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.45];
/// Under each tiled window, where its client has not drawn yet (it grew): the dark of a
/// tab strip, so a resize shows an empty tile instead of what is behind.
const BACKDROP_COLOR: [f32; 4] = [0x1e as f32 / 255.0, 0x20 as f32 / 255.0, 0x29 as f32 / 255.0, 1.0];
/// What private tiles show instead (`crate::privacy`): opaque, the dark of a
/// tab strip like the mark in their middle.
const PRIVATE_COLOR: [f32; 4] = BACKDROP_COLOR;
/// What fills an empty container (Mod+Enter) until a window goes in: glass, the dark of a
/// tab strip letting the wallpaper show through (premultiplied, translucent).
const GLASS_ALPHA: f32 = 0.55;
const GLASS_COLOR: [f32; 4] = [
    BACKDROP_COLOR[0] * GLASS_ALPHA,
    BACKDROP_COLOR[1] * GLASS_ALPHA,
    BACKDROP_COLOR[2] * GLASS_ALPHA,
    GLASS_ALPHA,
];
/// The light catching the top edge of the glass (premultiplied white).
const GLASS_SHEEN: [f32; 4] = [0.16, 0.16, 0.16, 0.16];
/// How thick that edge is.
const GLASS_SHEEN_HEIGHT: i32 = 1;
const PRIVATE_LABEL: &str = "🔒 private";
/// How long each stage of a folded bar's speaker waves shows (see `tab_title::WAVES`).
const WAVE_STEP_MS: u128 = 300;

render_elements! {
    /// Everything on an output, front first: pointer, decorations, then clients.
    pub OutputRenderElements<R> where R: ImportAll + ImportMem;
    Solid=SolidColorRenderElement,
    /// Clients: windows, layers and pointer surfaces.
    Surface=WaylandSurfaceRenderElement<R>,
    /// Tiled windows, cut to their tile (a buffer bigger than it, until the client draws
    /// the smaller size, stays off its neighbours).
    Cropped=CropRenderElement<WaylandSurfaceRenderElement<R>>,
    /// Pointer images and tab bars.
    Image=MemoryRenderBufferRenderElement<R>,
}

/// Buffers behind the decorations, kept between frames so unchanged ones cause no damage.
#[derive(Default)]
pub struct Decorations {
    /// Top, bottom, left, right edges of each tile's border.
    borders: HashMap<TileId, [SolidColorBuffer; 4]>,
    /// While a tile is dragged: the spot it will land on, and the tile itself.
    drop: SolidColorBuffer,
    dragged: SolidColorBuffer,
    /// The edge of the split a Mod+click there would open.
    split_edge: SolidColorBuffer,
    /// What shows under each tiled window (`BACKDROP_COLOR`).
    backdrops: HashMap<TileId, SolidColorBuffer>,
    /// The sheen along the top of each empty container (`GLASS_SHEEN`).
    sheens: HashMap<TileId, SolidColorBuffer>,
    /// What covers each private tile.
    private: HashMap<TileId, SolidColorBuffer>,
    /// Top, bottom, left, right edges of the frame around what is being recorded.
    recording: [SolidColorBuffer; 4],
}

impl Dune {
    /// The rectangle of `tile` when it is shown on `output`.
    fn shown_rect(&self, tile: TileId, output: &Output) -> Option<Rect> {
        self.on_output(tile, output).then_some(())?;
        self.shown.iter().any(|(t, _)| *t == tile).then_some(())?;
        self.geometries.iter().find(|(t, _)| *t == tile).map(|(_, r)| self.drawn_rect(tile, *r))
    }

    /// A tile's `rect` grown to take in its column's bar, when it fills the shown tab's width
    /// right under it.
    fn with_tab_bar(&self, rect: Rect) -> Rect {
        match self.tab_bars.iter().find(|b| b.rect.x == rect.x && b.rect.w == rect.w && b.rect.y + b.rect.h == rect.y) {
            Some(bar) => Rect::new(rect.x, bar.rect.y, rect.w, rect.y + rect.h - bar.rect.y),
            None => rect,
        }
    }

    /// While a tile of the strip is dragged: where it will land, then the tile darkened.
    /// (A dragged float just follows the pointer.) With Mod held near an edge of a tile
    /// instead: the band along that edge, its outer edge lit (`split_hint`).
    fn drag_elements(&mut self, output: &Output) -> Vec<SolidColorRenderElement> {
        let Some(origin) = self.space.output_geometry(output).map(|g| g.loc) else { return Vec::new() };
        if let Some((_, side, zone)) = self.split_hint() {
            let place = |x: i32, y: i32| Point::<i32, Physical>::from((x - origin.x, y - origin.y));
            let t = SPLIT_EDGE_WIDTH;
            let edge = match side {
                Side::Left => Rect::new(zone.x, zone.y, t, zone.h),
                Side::Right => Rect::new(zone.x + zone.w - t, zone.y, t, zone.h),
                Side::Up => Rect::new(zone.x, zone.y, zone.w, t),
                Side::Down => Rect::new(zone.x, zone.y + zone.h - t, zone.w, t),
            };
            let decorations = &mut self.decorations;
            decorations.split_edge.update((edge.w, edge.h), FOCUS_COLOR);
            decorations.drop.update((zone.w, zone.h), DROP_COLOR);
            return vec![
                SolidColorRenderElement::from_buffer(&decorations.split_edge, place(edge.x, edge.y), 1.0, 1.0, Kind::Unspecified),
                SolidColorRenderElement::from_buffer(&decorations.drop, place(zone.x, zone.y), 1.0, 1.0, Kind::Unspecified),
            ];
        }
        let Some(id) = self.move_drag.map(|d| d.tile).filter(|id| !self.is_floating(*id)) else { return Vec::new() };
        let place = |x: i32, y: i32| Point::<i32, Physical>::from((x - origin.x, y - origin.y));
        let pos = self.seat.get_pointer().unwrap().current_location();
        let area = self.drop_spot(id, pos).map(|spot| spot.area);
        let rect = self.shown_rect(id, output);

        let mut elements = Vec::new();
        let decorations = &mut self.decorations;
        if let Some(a) = area {
            decorations.drop.update((a.w, a.h), DROP_COLOR);
            elements.push(SolidColorRenderElement::from_buffer(&decorations.drop, place(a.x, a.y), 1.0, 1.0, Kind::Unspecified));
        }
        if let Some(r) = rect {
            decorations.dragged.update((r.w, r.h), DRAGGED_COLOR);
            elements.push(SolidColorRenderElement::from_buffer(&decorations.dragged, place(r.x, r.y), 1.0, 1.0, Kind::Unspecified));
        }
        elements
    }

    /// While a recording of a region or a tile runs: a frame just outside what it takes,
    /// where it falls on `output`.
    fn recording_elements(&mut self, output: &Output) -> Vec<SolidColorRenderElement> {
        let (Some(area), Some(geo)) = (self.recording_area, self.space.output_geometry(output)) else {
            return Vec::new();
        };
        let place = |x: i32, y: i32| Point::<i32, Physical>::from((x - geo.loc.x, y - geo.loc.y));
        let on_output = |(x, y, w, h): &(i32, i32, i32, i32)| {
            x + w > geo.loc.x && *x < geo.loc.x + geo.size.w && y + h > geo.loc.y && *y < geo.loc.y + geo.size.h
        };
        let mut elements = Vec::new();
        for (buffer, edge) in self.decorations.recording.iter_mut().zip(border_edges(area, RECORDING_FRAME)) {
            if on_output(&edge) {
                let (x, y, w, h) = edge;
                buffer.update((w, h), RECORDING_COLOR);
                elements.push(SolidColorRenderElement::from_buffer(buffer, place(x, y), 1.0, 1.0, Kind::Unspecified));
            }
        }
        elements
    }

    /// Borders of the tiles picked by `only` that fall on `output`,
    /// front first, in the output's coordinates.
    pub fn decoration_elements(&mut self, output: &Output, only: &dyn Fn(TileId) -> bool) -> Vec<SolidColorRenderElement> {
        let Some(origin) = self.space.output_geometry(output).map(|g| g.loc) else {
            return Vec::new();
        };
        // Only tiles that are on screen get decorations.
        let rects: HashMap<TileId, Rect> = self
            .tiles
            .keys()
            .filter(|t| only(**t))
            .filter_map(|t| Some((*t, self.with_tab_bar(self.shown_rect(*t, output)?))))
            .collect();
        let tile_rect = |tile: &TileId| rects.get(tile).copied();
        let place = |x: i32, y: i32| Point::<i32, Physical>::from((x - origin.x, y - origin.y));

        let mut elements = Vec::new();
        let decorations = &mut self.decorations;
        decorations.borders.retain(|tile, _| self.tiles.contains_key(tile));
        // Neighbours inside a column share their border line: the focused tile goes first
        // so its colour wins there.
        let focused = self.focused;
        let order = self.shown.iter().filter(|(t, _)| Some(*t) == focused).chain(self.shown.iter().filter(|(t, _)| Some(*t) != focused));
        for (tile, _) in order {
            let Some(rect) = tile_rect(tile) else { continue };
            // Tiles without focus take `border_color`, and have no border when it is `none`.
            let Some(color) = (if self.focused == Some(*tile) { Some(FOCUS_COLOR) } else { self.config.border_color }) else {
                continue;
            };
            let buffers = decorations.borders.entry(*tile).or_default();
            for (buffer, (x, y, w, h)) in buffers.iter_mut().zip(border_edges(rect, BORDER.min(GAP))) {
                buffer.update((w, h), color);
                elements.push(SolidColorRenderElement::from_buffer(buffer, place(x, y), 1.0, 1.0, Kind::Unspecified));
            }
        }
        elements
    }

    /// The bars of the tabs on `output`, with their window titles.
    fn tab_elements<R>(&mut self, renderer: &mut R, output: &Output) -> Vec<OutputRenderElements<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + Send + 'static,
    {
        let Some(geo) = self.space.output_geometry(output) else { return Vec::new() };
        self.tab_titles.retain(&self.tabs);
        let mut elements = Vec::new();
        for tile in self.tabs.clone() {
            let Some(rect) = self.shown_rect(tile, output) else { continue };
            // A folded tabbed column's bar shows its tab count instead of a title.
            let count = self.fold_counts.iter().find(|(t, _)| *t == tile).map(|(_, n)| n.to_string());
            let leading = count.is_some();
            let title = count.unwrap_or_else(|| self.shown_title(tile));
            // While its sound plays, the speaker's waves come one after another.
            let waves = (self.start_time.elapsed().as_millis() / WAVE_STEP_MS % (WAVES as u128 + 1)) as u8;
            let sound = self.plays_sound(tile).then_some(waves);
            let buffer = self.tab_titles.bar(tile, &title, (rect.w, rect.h), sound, leading);
            let at = Point::<f64, Physical>::from(((rect.x - geo.loc.x) as f64, (rect.y - geo.loc.y) as f64));
            if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, at, buffer, None, None, None, Kind::Unspecified) {
                elements.push(OutputRenderElements::Image(e));
            }
        }

        // Tabbed containers' bars.
        let actives: Vec<TileId> = self.tab_bars.iter().map(|b| b.active).collect();
        self.tab_titles.retain_strips(&actives);
        for bar in self.tab_bars.clone() {
            if !self.on_output(bar.active, output) {
                continue;
            }
            let titles = bar.tabs.iter().map(|t| self.shown_title(*t)).collect();
            let shown = bar.tabs.iter().position(|t| *t == bar.active).unwrap_or(0);
            let buffer = self.tab_titles.strip(bar.active, titles, shown, (bar.rect.w, bar.rect.h));
            let at = Point::<f64, Physical>::from(((bar.rect.x - geo.loc.x) as f64, (bar.rect.y - geo.loc.y) as f64));
            if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, at, buffer, None, None, None, Kind::Unspecified) {
                elements.push(OutputRenderElements::Image(e));
            }
        }
        elements
    }

    /// The pointer on `output` (a real session draws it; nested, the host does).
    pub fn pointer_elements<R>(&mut self, renderer: &mut R, output: &Output) -> Vec<OutputRenderElements<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + Send + 'static,
    {
        let Some(geo) = self.space.output_geometry(output) else { return Vec::new() };
        let location = self.seat.get_pointer().unwrap().current_location();
        if !geo.to_f64().contains(location) {
            return Vec::new();
        }
        let local = location - geo.loc.to_f64();
        if let CursorImageStatus::Surface(surface) = &self.cursor_status
            && !surface.is_alive()
        {
            self.cursor_status = CursorImageStatus::default_named();
        }
        match &self.cursor_status {
            CursorImageStatus::Hidden => Vec::new(),
            CursorImageStatus::Surface(surface) => {
                let hotspot = with_states(surface, |states| {
                    states
                        .data_map
                        .get::<std::sync::Mutex<CursorImageAttributes>>()
                        .map(|attrs| attrs.lock().unwrap().hotspot)
                        .unwrap_or_default()
                });
                let position = (local - hotspot.to_f64()).to_physical(1.0).to_i32_round();
                render_elements_from_surface_tree(renderer, surface, position, 1.0, 1.0, Kind::Cursor)
                    .into_iter()
                    .map(OutputRenderElements::Surface)
                    .collect()
            }
            // Named shapes all use the theme's default arrow for now.
            CursorImageStatus::Named(_) => {
                let (buffer, hotspot) = self.cursor.image(self.start_time.elapsed());
                let position = (local - hotspot.to_f64()).to_physical(1.0);
                MemoryRenderBufferRenderElement::from_buffer(renderer, position, &buffer, None, None, None, Kind::Cursor)
                    .map(|e| vec![OutputRenderElements::Image(e)])
                    .unwrap_or_default()
            }
        }
    }

    /// Everything to draw on `output`, front first.
    pub fn output_elements<R>(
        &mut self,
        renderer: &mut R,
        output: &Output,
        with_pointer: bool,
    ) -> Vec<OutputRenderElements<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + Send + 'static,
    {
        // Called for every frame: a resize sliding into place moves on.
        self.step_resize_animation();
        // Locked: the lock surface over black, nothing of the session.
        if self.session_lock.is_locked() {
            return self.lock_elements(renderer, output, with_pointer);
        }
        let mut elements = if with_pointer {
            self.pointer_elements(renderer, output)
        } else {
            Vec::new()
        };
        elements.extend(self.recording_elements(output).into_iter().map(OutputRenderElements::Solid));
        elements.extend(
            self.drag_elements(output)
                .into_iter()
                .map(OutputRenderElements::Solid),
        );
        // A fullscreen tile covers the bars: only Overlay layers (and the pointer) are over it.
        if let Some(cover) = self.covers.get(&output.name()).copied() {
            elements.extend(self.layer_elements(renderer, output, &[Layer::Overlay], true));
            let only = |t: TileId| t == cover;
            elements.extend(self.private_elements(renderer, output, &only));
            if !self.privacy.tile(cover) {
                elements.extend(self.window_elements(renderer, output, &only));
            }
            elements.extend(self.backdrop_elements(output, &only).into_iter().map(OutputRenderElements::Solid));
            return elements;
        }
        elements.extend(self.layer_elements(renderer, output, &[Layer::Overlay, Layer::Top], true));
        // The tooltips and menus of a bar under the windows still show over them.
        elements.extend(self.layer_popup_elements(renderer, output, &[Layer::Bottom, Layer::Background]));
        elements.extend(self.edge_mark_elements(renderer, output));
        // Private tiles: a block in place of their window (popups and subsurfaces too).
        let private: Vec<TileId> = self.tiles.keys().copied().filter(|t| self.privacy.tile(*t)).collect();
        let public = |t: TileId| !private.contains(&t);
        // Floats, front to back, each with its border over its window, above the strip.
        let floats: Vec<TileId> = self.floating.get(&output.name()).into_iter().flatten().rev().map(|(t, _)| *t).collect();
        for float in &floats {
            elements.extend(self.decoration_elements(output, &|t| t == *float).into_iter().map(OutputRenderElements::Solid));
            elements.extend(self.private_elements(renderer, output, &|t| t == *float));
            elements.extend(self.window_elements(renderer, output, &|t| t == *float && public(t)));
        }
        let tiled = |t: TileId| !floats.contains(&t);
        elements.extend(self.decoration_elements(output, &tiled).into_iter().map(OutputRenderElements::Solid));
        let tabs = self.tab_elements(renderer, output);
        elements.extend(tabs);
        elements.extend(self.private_elements(renderer, output, &tiled));
        elements.extend(self.window_elements(renderer, output, &|t| tiled(t) && public(t)));
        elements.extend(self.backdrop_elements(output, &tiled).into_iter().map(OutputRenderElements::Solid));
        elements.extend(self.layer_elements(renderer, output, &[Layer::Bottom, Layer::Background], false));
        elements
    }

    /// An opaque block with the lock mark over each private tile of `output` picked by
    /// `only` (folded tabs keep their bar), where its window would be.
    fn private_elements<R>(&mut self, renderer: &mut R, output: &Output, only: &dyn Fn(TileId) -> bool) -> Vec<OutputRenderElements<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + Send + 'static,
    {
        let Some(origin) = self.space.output_geometry(output).map(|g| g.loc) else { return Vec::new() };
        let rects: Vec<(TileId, Rect)> = self
            .tiles
            .iter()
            .filter(|(t, tile)| only(**t) && self.privacy.tile(**t) && tile.top().is_some() && !self.tabs.contains(t))
            .filter_map(|(t, _)| Some((*t, self.shown_rect(*t, output)?)))
            .collect();
        let privacy = &self.privacy;
        self.decorations.private.retain(|t, _| privacy.tile(*t));
        let mut elements = Vec::new();
        for (t, rect) in rects {
            elements.extend(self.lock_mark(renderer, (origin.x, origin.y), rect));
            let buffer = self.decorations.private.entry(t).or_default();
            buffer.update((rect.w, rect.h), PRIVATE_COLOR);
            let at = Point::<i32, Physical>::from((rect.x - origin.x, rect.y - origin.y));
            elements.push(OutputRenderElements::Solid(SolidColorRenderElement::from_buffer(buffer, at, 1.0, 1.0, Kind::Unspecified)));
        }
        elements
    }

    /// The lock mark (`PRIVATE_LABEL`) in the middle of `rect`, placed from the output's
    /// `origin`; none when it does not fit.
    fn lock_mark<R>(&mut self, renderer: &mut R, origin: (i32, i32), rect: Rect) -> Option<OutputRenderElements<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + Send + 'static,
    {
        let (buffer, (w, h)) = self.tab_titles.mark(PRIVATE_LABEL);
        if w > rect.w || h > rect.h {
            return None;
        }
        let (x, y) = (rect.x - origin.0 + (rect.w - w) / 2, rect.y - origin.1 + (rect.h - h) / 2);
        let at = Point::<f64, Physical>::from((x as f64, y as f64));
        MemoryRenderBufferRenderElement::from_buffer(renderer, at, buffer, None, None, None, Kind::Unspecified)
            .ok()
            .map(OutputRenderElements::Image)
    }

    /// The marks floating at the left and right edges of `output` when tiles of its strip
    /// are past them, telling how many: `‹3`, `2›`.
    fn edge_mark_elements<R>(&mut self, renderer: &mut R, output: &Output) -> Vec<OutputRenderElements<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + Send + 'static,
    {
        let Some(geo) = self.space.output_geometry(output) else { return Vec::new() };
        let (left, right) = self.edge_counts.get(&output.name()).copied().unwrap_or_default();
        let mut elements = Vec::new();
        for (is_left, n) in [(true, left), (false, right)] {
            if n == 0 {
                continue;
            }
            let (buffer, (w, h)) = self.tab_titles.edge_mark(is_left, n);
            let x = if is_left { EDGE_MARK_INSET } else { geo.size.w - w - EDGE_MARK_INSET };
            let at = Point::<f64, Physical>::from((x as f64, ((geo.size.h - h) / 2) as f64));
            if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, at, buffer, None, None, None, Kind::Unspecified) {
                elements.push(OutputRenderElements::Image(e));
            }
        }
        elements
    }

    /// Whether `tile` belongs to the layout or the floats of `output`.
    fn on_output(&self, tile: TileId, output: &Output) -> bool {
        self.layouts.get(&output.name()).is_some_and(|l| l.contains(tile))
            || self.floating.get(&output.name()).is_some_and(|s| s.iter().any(|(t, _)| *t == tile))
    }

    /// The windows of the tiles of `output` picked by `only`. A column partly scrolled off
    /// its output reaches into the neighbouring one; it is only drawn on its own.
    fn window_elements<R>(&self, renderer: &mut R, output: &Output, only: &dyn Fn(TileId) -> bool) -> Vec<OutputRenderElements<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + Send + 'static,
    {
        let Some(origin) = self.space.output_geometry(output).map(|g| g.loc) else { return Vec::new() };
        let mut elements = Vec::new();
        for (id, tile) in &self.tiles {
            let Some(window) = tile.top() else { continue };
            if !only(*id) || !self.on_output(*id, output) {
                continue;
            }
            let Some(loc) = self.space.element_location(window) else { continue };
            let at = (loc - window.geometry().loc - origin).to_physical(1);
            let tile_rect = (!self.is_floating(*id)).then(|| self.shown_rect(*id, output)).flatten();
            let (Some(rect), Some(toplevel)) = (tile_rect, window.toplevel()) else {
                let surfaces: Vec<WaylandSurfaceRenderElement<R>> = window.render_elements(renderer, at, Scale::from(1.0), 1.0);
                elements.extend(surfaces.into_iter().map(OutputRenderElements::Surface));
                continue;
            };
            // Menus and tooltips reach past the tile: only the window itself is cut to it.
            let surface = toplevel.wl_surface();
            for (popup, offset) in PopupManager::popups_for_surface(surface) {
                let popup_at = at + (window.geometry().loc + offset - popup.geometry().loc).to_physical(1);
                let popups: Vec<WaylandSurfaceRenderElement<R>> =
                    render_elements_from_surface_tree(renderer, popup.wl_surface(), popup_at, 1.0, 1.0, Kind::Unspecified);
                elements.extend(popups.into_iter().map(OutputRenderElements::Surface));
            }
            let crop = Rectangle::new((rect.x - origin.x, rect.y - origin.y).into(), (rect.w, rect.h).into());
            let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                render_elements_from_surface_tree(renderer, surface, at, 1.0, 1.0, Kind::Unspecified);
            elements.extend(
                surfaces
                    .into_iter()
                    .filter_map(|e| CropRenderElement::from_element(e, Scale::from(1.0), crop))
                    .map(OutputRenderElements::Cropped),
            );
        }
        elements
    }

    /// A solid tile under each window picked by `only` on `output`, at its tile's place;
    /// glass, with a sheen along its top, filling each empty container.
    fn backdrop_elements(&mut self, output: &Output, only: &dyn Fn(TileId) -> bool) -> Vec<SolidColorRenderElement> {
        let Some(origin) = self.space.output_geometry(output).map(|g| g.loc) else { return Vec::new() };
        let rects: Vec<(TileId, Rect, bool)> = self
            .tiles
            .iter()
            .filter(|(t, tile)| only(**t) && (tile.top().is_some() || tile.reserved) && !self.tabs.contains(t))
            .filter_map(|(t, tile)| Some((*t, self.shown_rect(*t, output)?, tile.top().is_none())))
            .collect();
        let decorations = &mut self.decorations;
        decorations.backdrops.retain(|t, _| rects.iter().any(|(r, _, _)| r == t));
        decorations.sheens.retain(|t, _| rects.iter().any(|(r, _, empty)| r == t && *empty));
        let mut elements = Vec::new();
        for (t, rect, empty) in rects {
            let at = Point::<i32, Physical>::from((rect.x - origin.x, rect.y - origin.y));
            if empty {
                let sheen = decorations.sheens.entry(t).or_default();
                sheen.update((rect.w, GLASS_SHEEN_HEIGHT), GLASS_SHEEN);
                elements.push(SolidColorRenderElement::from_buffer(sheen, at, 1.0, 1.0, Kind::Unspecified));
            }
            let buffer = decorations.backdrops.entry(t).or_default();
            buffer.update((rect.w, rect.h), if empty { GLASS_COLOR } else { BACKDROP_COLOR });
            elements.push(SolidColorRenderElement::from_buffer(buffer, at, 1.0, 1.0, Kind::Unspecified));
        }
        elements
    }

    /// Layer surfaces (bars, launchers, wallpapers) of `output` on `layers`, front first;
    /// with their popups, or without them when those are drawn apart (`layer_popup_elements`).
    fn layer_elements<R>(&self, renderer: &mut R, output: &Output, layers: &[Layer], popups: bool) -> Vec<OutputRenderElements<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + Send + 'static,
    {
        let map = layer_map_for_output(output);
        let mut elements = Vec::new();
        for layer in layers {
            for surface in map.layers_on(*layer).rev() {
                let Some(geo) = map.layer_geometry(surface) else { continue };
                let at = geo.loc.to_physical(1);
                let surfaces: Vec<WaylandSurfaceRenderElement<R>> = if popups {
                    surface.render_elements(renderer, at, Scale::from(1.0), 1.0)
                } else {
                    render_elements_from_surface_tree(renderer, surface.wl_surface(), at, 1.0, 1.0, Kind::Unspecified)
                };
                elements.extend(surfaces.into_iter().map(OutputRenderElements::Surface));
            }
        }
        elements
    }

    /// The popups (tooltips, menus) of the layer surfaces of `output` on `layers`, front first.
    fn layer_popup_elements<R>(&self, renderer: &mut R, output: &Output, layers: &[Layer]) -> Vec<OutputRenderElements<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + Send + 'static,
    {
        let map = layer_map_for_output(output);
        let mut elements = Vec::new();
        for layer in layers {
            for surface in map.layers_on(*layer).rev() {
                let Some(geo) = map.layer_geometry(surface) else { continue };
                for (popup, offset) in PopupManager::popups_for_surface(surface.wl_surface()) {
                    let at = (geo.loc + offset - popup.geometry().loc).to_physical(1);
                    let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                        render_elements_from_surface_tree(renderer, popup.wl_surface(), at, 1.0, 1.0, Kind::Unspecified);
                    elements.extend(surfaces.into_iter().map(OutputRenderElements::Surface));
                }
            }
        }
        elements
    }

    /// After a frame on `output`: let its clients draw the next one, and tidy up.
    pub fn post_render(&mut self, output: &Output) {
        let time = self.start_time.elapsed();
        self.space.elements().for_each(|window| {
            window.send_frame(output, time, Some(Duration::ZERO), |_, _| Some(output.clone()))
        });
        for layer in layer_map_for_output(output).layers() {
            layer.send_frame(output, time, Some(Duration::ZERO), |_, _| Some(output.clone()));
        }
        if let CursorImageStatus::Surface(surface) = &self.cursor_status {
            smithay::desktop::utils::send_frames_surface_tree(surface, output, time, Some(Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
        self.send_lock_frames(output, time);
        self.space.refresh();
        // Inhibiting surfaces may have died or been hidden since.
        self.refresh_idle_inhibit();
        self.popups.cleanup();
        let _ = self.display_handle.flush_clients();
    }
}

/// The four edges, `b` pixels thick, of a border around `rect` (a tile's: inside the gap).
fn border_edges(rect: Rect, b: i32) -> [(i32, i32, i32, i32); 4] {
    [
        (rect.x - b, rect.y - b, rect.w + 2 * b, b),
        (rect.x - b, rect.y + rect.h, rect.w + 2 * b, b),
        (rect.x - b, rect.y, b, rect.h),
        (rect.x + rect.w, rect.y, b, rect.h),
    ]
}
