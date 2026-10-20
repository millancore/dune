//! What dune draws besides the clients — tile borders, direction indicator bars, tab
//! bars and, on a real session, the pointer — shared by both backends.

use std::{collections::HashMap, time::Duration};

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer,
        element::{
            AsRenderElements, Kind,
            memory::MemoryRenderBufferRenderElement,
            render_elements,
            solid::{SolidColorBuffer, SolidColorRenderElement},
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        },
    },
    desktop::layer_map_for_output,
    input::pointer::{CursorImageAttributes, CursorImageStatus},
    output::Output,
    reexports::wayland_server::Resource,
    utils::{Physical, Point, Scale},
    wayland::shell::wlr_layer::Layer,
    wayland::compositor::with_states,
};

use crate::{
    Dune,
    layout::{Rect, Side, TileId},
    tiling::{BORDER, GAP},
};

pub const BACKGROUND: [f32; 4] = [0.08, 0.08, 0.1, 1.0];
const FOCUS_COLOR: [f32; 4] = [0.36, 0.62, 0.95, 1.0];
/// The direction indicator: a bar on the side where the next command will open.
const DIRECTION_COLOR: [f32; 4] = [0.98, 0.62, 0.22, 1.0];
/// Where a dragged tile will land (premultiplied, translucent).
const DROP_COLOR: [f32; 4] = [0.36 * 0.35, 0.62 * 0.35, 0.95 * 0.35, 0.35];
/// How far the marks of tiles past a screen edge float in from it, in pixels.
const EDGE_MARK_INSET: i32 = 4;
/// Darkens the tile being dragged (premultiplied, translucent).
const DRAGGED_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.45];

render_elements! {
    /// Everything on an output, front first: pointer, decorations, then clients.
    pub OutputRenderElements<R> where R: ImportAll + ImportMem;
    Solid=SolidColorRenderElement,
    /// Clients: windows, layers and pointer surfaces.
    Surface=WaylandSurfaceRenderElement<R>,
    /// Pointer images and tab bars.
    Image=MemoryRenderBufferRenderElement<R>,
}

/// Buffers behind the decorations, kept between frames so unchanged ones cause no damage.
#[derive(Default)]
pub struct Decorations {
    /// Top, bottom, left, right edges of each tile's border.
    borders: HashMap<TileId, [SolidColorBuffer; 4]>,
    direction_bars: HashMap<TileId, SolidColorBuffer>,
    /// While a tile is dragged: the spot it will land on, and the tile itself.
    drop: SolidColorBuffer,
    dragged: SolidColorBuffer,
}

impl Dune {
    /// The rectangle of `tile` when it is shown on `output`.
    fn shown_rect(&self, tile: TileId, output: &Output) -> Option<Rect> {
        self.on_output(tile, output).then_some(())?;
        self.shown.iter().any(|(t, _)| *t == tile).then_some(())?;
        self.geometries.iter().find(|(t, _)| *t == tile).map(|(_, r)| *r)
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
    /// (A dragged float just follows the pointer.)
    fn drag_elements(&mut self, output: &Output) -> Vec<SolidColorRenderElement> {
        let Some(origin) = self.space.output_geometry(output).map(|g| g.loc) else { return Vec::new() };
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

    /// Borders and direction bars of the tiles picked by `only` that fall on `output`,
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
        // Indicator bars first: elements earlier in the list are drawn on top.
        decorations.direction_bars.retain(|tile, _| self.directions.contains_key(tile));
        for (tile, side) in &self.directions {
            let Some(rect) = tile_rect(tile) else { continue };
            let (x, y, w, h) = match side {
                Side::Left => (rect.x - GAP, rect.y, GAP, rect.h),
                Side::Right => (rect.x + rect.w, rect.y, GAP, rect.h),
                Side::Up => (rect.x, rect.y - GAP, rect.w, GAP),
                Side::Down => (rect.x, rect.y + rect.h, rect.w, GAP),
            };
            let buffer = decorations.direction_bars.entry(*tile).or_default();
            buffer.update((w, h), DIRECTION_COLOR);
            elements.push(SolidColorRenderElement::from_buffer(buffer, place(x, y), 1.0, 1.0, Kind::Unspecified));
        }
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
            for (buffer, (x, y, w, h)) in buffers.iter_mut().zip(border_edges(rect)) {
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
            let Some((_, rect)) = self.geometries.iter().find(|(t, _)| *t == tile).copied() else { continue };
            if !self.on_output(tile, output) || !self.shown.iter().any(|(t, _)| *t == tile) {
                continue;
            }
            let title = self.tile_title(tile);
            let buffer = self.tab_titles.bar(tile, &title, (rect.w, rect.h));
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
            let titles = bar.tabs.iter().map(|t| self.tile_title(*t)).collect();
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
    fn pointer_elements<R>(&mut self, renderer: &mut R, output: &Output) -> Vec<OutputRenderElements<R>>
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
        let mut elements = if with_pointer { self.pointer_elements(renderer, output) } else { Vec::new() };
        elements.extend(self.launcher_elements(renderer, output));
        elements.extend(self.switcher_elements(renderer, output));
        elements.extend(self.drag_elements(output).into_iter().map(OutputRenderElements::Solid));
        elements.extend(self.layer_elements(renderer, output, &[Layer::Overlay, Layer::Top]));
        elements.extend(self.edge_mark_elements(renderer, output));
        // Floats, front to back, each with its border over its window, above the strip.
        let floats: Vec<TileId> = self.floating.get(&output.name()).into_iter().flatten().rev().map(|(t, _)| *t).collect();
        for float in &floats {
            elements.extend(self.decoration_elements(output, &|t| t == *float).into_iter().map(OutputRenderElements::Solid));
            elements.extend(self.window_elements(renderer, output, &|t| t == *float));
        }
        let tiled = |t: TileId| !floats.contains(&t);
        elements.extend(self.decoration_elements(output, &tiled).into_iter().map(OutputRenderElements::Solid));
        let tabs = self.tab_elements(renderer, output);
        elements.extend(tabs);
        elements.extend(self.window_elements(renderer, output, &tiled));
        elements.extend(self.layer_elements(renderer, output, &[Layer::Bottom, Layer::Background]));
        elements
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

    /// The launcher panel, centered across `output` a quarter of the way down, or in the
    /// middle of the empty container it was opened in.
    fn launcher_elements<R>(&mut self, renderer: &mut R, output: &Output) -> Vec<OutputRenderElements<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + Send + 'static,
    {
        if self.launcher.output() != Some(output.name().as_str()) {
            return Vec::new();
        }
        let Some(geo) = self.space.output_geometry(output) else { return Vec::new() };
        // Opened in an empty container: in the middle of it, as long as it is on screen.
        let slot = self.launcher.slot().and_then(|(slot, _)| self.shown.iter().find(|(t, _)| *t == slot)).map(|(_, r)| *r);
        let width = slot.map_or(geo.size.w, |r| r.w);
        let Some((buffer, (w, h))) = self.launcher.image(width - 2 * GAP) else { return Vec::new() };
        let (x, y) = match slot {
            Some(r) => (r.x + (r.w - w) / 2, r.y + ((r.h - h) / 2).max(0)),
            None => (geo.loc.x + (geo.size.w - w) / 2, geo.loc.y + ((geo.size.h - h) / 4).max(0)),
        };
        let at = Point::<f64, Physical>::from(((x - geo.loc.x) as f64, (y - geo.loc.y) as f64));
        MemoryRenderBufferRenderElement::from_buffer(renderer, at, buffer, None, None, None, Kind::Unspecified)
            .map(|e| vec![OutputRenderElements::Image(e)])
            .unwrap_or_default()
    }

    /// The switcher panel, centered across `output` a quarter of the way down.
    fn switcher_elements<R>(&mut self, renderer: &mut R, output: &Output) -> Vec<OutputRenderElements<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + Send + 'static,
    {
        if self.switcher.output() != Some(output.name().as_str()) {
            return Vec::new();
        }
        let Some(geo) = self.space.output_geometry(output) else { return Vec::new() };
        let Some((buffer, (w, h))) = self.switcher.image(geo.size.w - 2 * GAP) else { return Vec::new() };
        let at = Point::<f64, Physical>::from((((geo.size.w - w) / 2) as f64, ((geo.size.h - h) / 4).max(0) as f64));
        MemoryRenderBufferRenderElement::from_buffer(renderer, at, buffer, None, None, None, Kind::Unspecified)
            .map(|e| vec![OutputRenderElements::Image(e)])
            .unwrap_or_default()
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
            let surfaces: Vec<WaylandSurfaceRenderElement<R>> = window.render_elements(renderer, at, Scale::from(1.0), 1.0);
            elements.extend(surfaces.into_iter().map(OutputRenderElements::Surface));
        }
        elements
    }

    /// Layer surfaces (bars, launchers, wallpapers) of `output` on `layers`, front first.
    fn layer_elements<R>(&self, renderer: &mut R, output: &Output, layers: &[Layer]) -> Vec<OutputRenderElements<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + Send + 'static,
    {
        let map = layer_map_for_output(output);
        let mut elements = Vec::new();
        for layer in layers {
            for surface in map.layers_on(*layer).rev() {
                let Some(geo) = map.layer_geometry(surface) else { continue };
                let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                    surface.render_elements(renderer, geo.loc.to_physical(1), Scale::from(1.0), 1.0);
                elements.extend(surfaces.into_iter().map(OutputRenderElements::Surface));
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
        self.space.refresh();
        self.popups.cleanup();
        let _ = self.display_handle.flush_clients();
    }
}

/// The four edges of the border around a tile, inside the gap.
fn border_edges(rect: Rect) -> [(i32, i32, i32, i32); 4] {
    let b = BORDER.min(GAP);
    [
        (rect.x - b, rect.y - b, rect.w + 2 * b, b),
        (rect.x - b, rect.y + rect.h, rect.w + 2 * b, b),
        (rect.x - b, rect.y, b, rect.h),
        (rect.x + rect.w, rect.y, b, rect.h),
    ]
}
