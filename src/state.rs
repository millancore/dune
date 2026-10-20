use std::{collections::HashMap, ffi::OsString, path::PathBuf, sync::Arc};

use smithay::{
    backend::{renderer::gles::GlesRenderer, winit::WinitGraphicsBackend},
    desktop::{PopupManager, Space, Window, WindowSurfaceType, layer_map_for_output},
    input::{Seat, SeatState, pointer::CursorImageStatus},
    reexports::{
        calloop::{EventLoop, Interest, LoopHandle, LoopSignal, Mode, PostAction, generic::Generic},
        wayland_server::{
            Display, DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Logical, Point},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        dmabuf::DmabufState,
        viewporter::ViewporterState,
        shell::wlr_layer::{Layer, WlrLayerShellState},
        output::OutputManagerState,
        selection::{data_device::DataDeviceState, primary_selection::PrimarySelectionState},
        shell::xdg::{XdgShellState, decoration::XdgDecorationState},
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};

use crate::layout::{Layout, Rect, Side, TileId};

/// One slot of the layout. `stack[0]` is the tile's terminal; programs started from it
/// with `@prog` are pushed on top and "swallow" it until they close. The last window
/// is the one shown.
#[derive(Debug, Default)]
pub struct Tile {
    pub stack: Vec<Window>,
    /// An empty container (Mod+Enter): it takes its place before any window is in it.
    pub reserved: bool,
}

impl Tile {
    pub fn top(&self) -> Option<&Window> {
        self.stack.last()
    }
}

/// A Mod+left button drag of a tile.
#[derive(Clone, Copy, Debug)]
pub struct MoveDrag {
    pub tile: TileId,
    /// Where the button went down.
    pub start: Point<f64, Logical>,
    /// The pointer went far enough to make it a drag rather than a click.
    pub moved: bool,
    /// Where a floating tile was when the drag started.
    pub from: Option<Rect>,
    /// Pressed near this edge of the tile (`split_edge`): a click there opens a split.
    pub split: Option<Side>,
}

pub struct Dune {
    pub start_time: std::time::Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,
    pub loop_handle: LoopHandle<'static, Dune>,

    pub space: Space<Window>,
    pub loop_signal: LoopSignal,

    // Tiling
    /// One layout per output, by output name.
    pub layouts: HashMap<String, Layout>,
    /// The output new tiles go to when they have no target: the last one the pointer
    /// entered or a tile was focused on.
    pub active_output: Option<String>,
    pub tiles: HashMap<TileId, Tile>,
    pub focused: Option<TileId>,
    pub next_tile: TileId,
    /// Geometries from the last `arrange`, used for borders, focus and hit testing.
    pub geometries: Vec<(TileId, Rect)>,
    /// The part of each tile that is on its output; tiles scrolled away are missing.
    pub shown: Vec<(TileId, Rect)>,
    /// Tiles folded into a tab: their bar is drawn, their window is not.
    pub tabs: Vec<TileId>,
    /// Bars of the tabbed containers on the outputs (shown ones only), from the last `arrange`.
    pub tab_bars: Vec<crate::layout::TabBar>,
    /// By output: how many tiles of its strip are past its left and its right edge.
    pub edge_counts: HashMap<String, (usize, usize)>,
    /// Mod+right button drag resizing a tile: the tile and the last pointer position.
    pub resize_drag: Option<(TileId, Point<f64, Logical>)>,
    /// Mod+left button drag moving a tile: it is dropped where the button is released.
    pub move_drag: Option<MoveDrag>,
    /// Floating tiles of each output, by output name, back to front.
    pub floating: HashMap<String, Vec<(TileId, Rect)>>,
    /// Where floating tiles came from in the strip, with the layout's output, to go back.
    pub float_homes: HashMap<TileId, (String, crate::layout::Home)>,
    /// Where tiles sent to a tab of their own by Mod+t were in their column, with the
    /// layout's output, to go back on the next Mod+t.
    pub tab_homes: HashMap<TileId, (String, crate::layout::Home)>,
    /// The desktop each output shows (1 when missing); `layouts` and `floating` hold it.
    pub desks: HashMap<String, u8>,
    /// Desktops not shown, by output name and number.
    pub hidden: HashMap<(String, u8), crate::desktops::Desk>,
    /// Tiles waiting for the window of an `@prog`, with the pid of the shell that started it.
    pub swallows: HashMap<TileId, u32>,
    /// Apps the launcher started on their own, by their `DUNE_LAUNCH` number: where their
    /// first window goes, and when they started.
    pub launches: HashMap<u64, crate::launcher::Launch>,
    pub next_launch: u64,
    /// Per-tile direction indicator: where the next command typed there opens.
    pub directions: HashMap<TileId, crate::layout::Side>,
    /// `dune-shell` link given to terminals as `$SHELL`, and the shell it starts.
    pub shell_wrapper: Option<PathBuf>,
    pub user_shell: PathBuf,

    // Smithay State
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Dune>,
    pub data_device_state: DataDeviceState,
    pub primary_selection_state: PrimarySelectionState,
    pub popups: PopupManager,

    pub seat: Seat<Self>,
    pub config: crate::config::Config,

    pub layer_shell_state: WlrLayerShellState,
    pub viewporter_state: ViewporterState,
    /// A layer (dune-finder, a launcher) that asked for the keyboard; it has it until it closes.
    pub layer_focus: Option<WlSurface>,

    /// GPU buffers from clients (linux-dmabuf); the global is created with the backend.
    pub dmabuf_state: DmabufState,
    /// Screen capture (see `crate::screencopy`): monitors as capture sources, the
    /// sessions clients hold, and frames waiting for something new on their monitor.
    pub output_capture_source_state: smithay::wayland::image_capture_source::OutputCaptureSourceState,
    pub image_copy_capture_state: smithay::wayland::image_copy_capture::ImageCopyCaptureState,
    /// xdg-activation: windows brought forward by a token (a clicked notification).
    pub activation_state: smithay::wayland::xdg_activation::XdgActivationState,
    pub capture_sessions: Vec<smithay::wayland::image_copy_capture::Session>,
    pub pending_captures: Vec<crate::screencopy::PendingCapture>,
    /// Where we draw: a window in another session, or the real hardware.
    pub backend: Backend,
    pub decorations: crate::render::Decorations,
    pub tab_titles: crate::tab_title::TabTitles,
    /// Bars told about the desktops (ext-workspace-v1).
    pub workspaces: crate::workspaces::WorkspaceState,
    /// Night light: gamma ramps clients set per monitor (wlr-gamma-control).
    pub gamma: crate::gamma::GammaState,
    /// dune-finder (Mod+d, Mod+/, Mod+Enter) while it runs.
    pub finder: Option<crate::launcher::FinderRun>,
    /// Tiles focused, the latest first: dune-finder lists windows in this order.
    pub recent: Vec<TileId>,
    /// How far the pointer has pushed against a screen edge (see `input::edge_slide`).
    pub edge_push: f64,
    /// The pointer image: clients choose it; dune draws it on a real session.
    pub cursor_status: CursorImageStatus,
    pub cursor: crate::cursor::Cursor,
}

pub enum Backend {
    /// Not set up yet.
    None,
    /// Nested in another compositor (or X11).
    Winit(WinitGraphicsBackend<GlesRenderer>),
    /// A real session on a TTY.
    Udev(crate::udev::UdevData),
}

impl Dune {
    /// Run `f` with the renderer lent out of the backend, so it can build a scene from
    /// the rest of the state. None before there is one.
    pub fn with_renderer<T>(&mut self, f: impl FnOnce(&mut Self, &mut GlesRenderer) -> T) -> Option<T> {
        let mut backend = std::mem::replace(&mut self.backend, Backend::None);
        let result = backend.renderer().map(|renderer| f(self, renderer));
        self.backend = backend;
        result
    }

    /// The GPU's render node, for clients allocating buffers on it (real sessions).
    pub fn render_node(&self) -> Option<smithay::backend::drm::DrmNode> {
        match &self.backend {
            Backend::Udev(udev) => udev.render_node(),
            _ => None,
        }
    }
}

impl Backend {
    /// The renderer of the primary GPU.
    pub fn renderer(&mut self) -> Option<&mut GlesRenderer> {
        match self {
            Backend::None => None,
            Backend::Winit(backend) => Some(backend.renderer()),
            Backend::Udev(udev) => udev.renderer.as_mut(),
        }
    }
}

impl Dune {
    pub fn new(event_loop: &mut EventLoop<'static, Self>, display: Display<Self>) -> Self {
        let start_time = std::time::Instant::now();

        let dh = display.handle();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let layer_shell_state = WlrLayerShellState::new::<Self>(&dh);
        // Clients scaling their buffers (xwayland-satellite requires it).
        let viewporter_state = ViewporterState::new::<Self>(&dh);
        let workspaces = crate::workspaces::WorkspaceState::new(&dh);
        let gamma = crate::gamma::GammaState::new(&dh);
        let popups = PopupManager::default();
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let primary_selection_state = PrimarySelectionState::new::<Self>(&dh);
        // Games (through xwayland-satellite too) read raw mouse motion and lock or confine
        // the pointer while playing.
        smithay::wayland::relative_pointer::RelativePointerManagerState::new::<Self>(&dh);
        smithay::wayland::pointer_constraints::PointerConstraintsState::new::<Self>(&dh);
        // Screen capture: grim, and screen sharing through xdg-desktop-portal-wlr.
        let output_capture_source_state = smithay::wayland::image_capture_source::OutputCaptureSourceState::new::<Self>(&dh);
        let image_copy_capture_state = smithay::wayland::image_copy_capture::ImageCopyCaptureState::new::<Self>(&dh);
        let activation_state = smithay::wayland::xdg_activation::XdgActivationState::new::<Self>(&dh);

        let mut seat_state = SeatState::new();
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "seat0");
        // Keys arrive as raw keycodes (also when nested), so the layout must be ours.
        let config = crate::config::load();
        let (delay, rate) = (config.repeat_delay, config.repeat_rate);
        if let Err(err) = seat.add_keyboard(config.xkb(), delay, rate) {
            tracing::error!("invalid keyboard layout in the dune config ({err:?}), falling back to the default");
            seat.add_keyboard(Default::default(), delay, rate).unwrap();
        }
        seat.add_pointer();

        let socket_name = Self::init_wayland_listener(display, event_loop);
        let loop_signal = event_loop.get_signal();

        Self {
            start_time,
            display_handle: dh,
            loop_handle: event_loop.handle(),

            space: Space::default(),
            loop_signal,
            socket_name,

            layouts: HashMap::new(),
            active_output: None,
            tiles: HashMap::new(),
            focused: None,
            next_tile: 1,
            geometries: Vec::new(),
            shown: Vec::new(),
            tabs: Vec::new(),
            tab_bars: Vec::new(),
            edge_counts: HashMap::new(),
            resize_drag: None,
            move_drag: None,
            floating: HashMap::new(),
            float_homes: HashMap::new(),
            tab_homes: HashMap::new(),
            desks: HashMap::new(),
            hidden: HashMap::new(),
            swallows: HashMap::new(),
            launches: HashMap::new(),
            next_launch: 1,
            directions: HashMap::new(),
            shell_wrapper: None,
            user_shell: PathBuf::from("/bin/sh"),

            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            primary_selection_state,
            popups,
            seat,
            tab_titles: crate::tab_title::TabTitles::new(&config.font_family, config.tab_widths),
            finder: None,
            recent: Vec::new(),
            edge_push: 0.0,
            workspaces,
            gamma,
            config,
            layer_shell_state,
            viewporter_state,
            layer_focus: None,
            dmabuf_state: DmabufState::new(),
            output_capture_source_state,
            image_copy_capture_state,
            activation_state,
            capture_sessions: Vec::new(),
            pending_captures: Vec::new(),
            backend: Backend::None,
            decorations: Default::default(),
            cursor_status: CursorImageStatus::default_named(),
            cursor: crate::cursor::Cursor::load(),
        }
    }

    fn init_wayland_listener(display: Display<Dune>, event_loop: &mut EventLoop<Self>) -> OsString {
        let listening_socket = ListeningSocketSource::new_auto().unwrap();
        let socket_name = listening_socket.socket_name().to_os_string();

        let loop_handle = event_loop.handle();

        loop_handle
            .insert_source(listening_socket, move |client_stream, _, state| {
                state
                    .display_handle
                    .insert_client(client_stream, Arc::new(ClientState::default()))
                    .unwrap();
            })
            .expect("Failed to init the wayland event source.");

        loop_handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, state| {
                    // Safety: we don't drop the display
                    unsafe {
                        display.get_mut().dispatch_clients(state).unwrap();
                    }
                    Ok(PostAction::Continue)
                },
            )
            .unwrap();

        socket_name
    }

    /// The surface under the pointer: overlay/top layers, then tiles, then bottom/background layers.
    pub fn surface_under(&self, pos: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        let output = self.space.outputs().find(|o| self.space.output_geometry(o).is_some_and(|g| g.to_f64().contains(pos)))?;
        let output_loc = self.space.output_geometry(output)?.loc;
        let layers = layer_map_for_output(output);
        let layer_surface_under = |layer: Layer, kind: WindowSurfaceType| {
            let local = pos - output_loc.to_f64();
            layers.layer_under(layer, local).and_then(|l| {
                let geo = layers.layer_geometry(l)?;
                l.surface_under(local - geo.loc.to_f64(), kind)
                    .map(|(s, p)| (s, (p + geo.loc + output_loc).to_f64()))
            })
        };
        // The popups of a bar under the windows are drawn over them (see `output_elements`).
        layer_surface_under(Layer::Overlay, WindowSurfaceType::ALL)
            .or_else(|| layer_surface_under(Layer::Top, WindowSurfaceType::ALL))
            .or_else(|| layer_surface_under(Layer::Bottom, WindowSurfaceType::POPUP))
            .or_else(|| layer_surface_under(Layer::Background, WindowSurfaceType::POPUP))
            .or_else(|| {
                self.space.element_under(pos).and_then(|(window, location)| {
                    window
                        .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                        .map(|(s, p)| (s, (p + location).to_f64()))
                })
            })
            .or_else(|| layer_surface_under(Layer::Bottom, WindowSurfaceType::ALL))
            .or_else(|| layer_surface_under(Layer::Background, WindowSurfaceType::ALL))
    }
}

/// Data associated with a wayland client that connects to Dune.
#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
