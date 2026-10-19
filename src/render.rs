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
/// The border of every tile that does not have focus.
const BORDER_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// The direction indicator: a bar on the side where the next command will open.
const DIRECTION_COLOR: [f32; 4] = [0.98, 0.62, 0.22, 1.0];

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
}

impl Dune {
    /// Tile borders and direction bars that fall on `output`, front first, in the
    /// output's coordinates.
    pub fn decoration_elements(&mut self, output: &Output) -> Vec<SolidColorRenderElement> {
        let Some(origin) = self.space.output_geometry(output).map(|g| g.loc) else {
            return Vec::new();
        };
        // Only tiles that are on screen get decorations.
        let layout = self.layouts.get(&output.name());
        let tile_rect = |tile: &TileId| {
            layout.is_some_and(|l| l.contains(*tile)).then_some(())?;
            self.shown.iter().any(|(t, _)| t == tile).then_some(())?;
            self.geometries.iter().find(|(t, _)| t == tile).map(|(_, r)| *r)
        };
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
        for (tile, _) in &self.shown {
            let Some(rect) = tile_rect(tile) else { continue };
            let color = if self.focused == Some(*tile) { FOCUS_COLOR } else { BORDER_COLOR };
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
        elements.extend(self.decoration_elements(output).into_iter().map(OutputRenderElements::Solid));
        let tabs = self.tab_elements(renderer, output);
        elements.extend(tabs);
        elements.extend(self.layer_elements(renderer, output, &[Layer::Overlay, Layer::Top]));
        elements.extend(self.window_elements(renderer, output));
        elements.extend(self.layer_elements(renderer, output, &[Layer::Bottom, Layer::Background]));
        elements
    }

    /// Whether `tile` belongs to the layout of `output`.
    fn on_output(&self, tile: TileId, output: &Output) -> bool {
        self.layouts.get(&output.name()).is_some_and(|l| l.contains(tile))
    }

    /// The windows of the tiles of `output`. A column partly scrolled off its output
    /// reaches into the neighbouring one; it is only drawn on its own.
    fn window_elements<R>(&self, renderer: &mut R, output: &Output) -> Vec<OutputRenderElements<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + Send + 'static,
    {
        let Some(origin) = self.space.output_geometry(output).map(|g| g.loc) else { return Vec::new() };
        let mut elements = Vec::new();
        for (id, tile) in &self.tiles {
            let Some(window) = tile.top() else { continue };
            if !self.on_output(*id, output) {
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
