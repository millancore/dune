use std::{collections::HashMap, ffi::OsString, path::PathBuf, sync::Arc};

use smithay::{
    backend::{renderer::gles::GlesRenderer, winit::WinitGraphicsBackend},
    desktop::{PopupManager, Space, Window, WindowSurfaceType, layer_map_for_output},
    input::{Seat, SeatState, keyboard::XkbConfig},
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
        selection::data_device::DataDeviceState,
        shell::xdg::{XdgShellState, decoration::XdgDecorationState},
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};

use crate::layout::{Layout, Rect, TileId};

/// One slot of the layout. `stack[0]` is the tile's terminal; programs started from it
/// with `@prog` are pushed on top and "swallow" it until they close. The last window
/// is the one shown.
#[derive(Debug, Default)]
pub struct Tile {
    pub stack: Vec<Window>,
}

impl Tile {
    pub fn top(&self) -> Option<&Window> {
        self.stack.last()
    }
}

pub struct Dune {
    pub start_time: std::time::Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,
    pub loop_handle: LoopHandle<'static, Dune>,

    pub space: Space<Window>,
    pub loop_signal: LoopSignal,

    // Tiling
    pub layout: Layout,
    pub tiles: HashMap<TileId, Tile>,
    pub focused: Option<TileId>,
    pub next_tile: TileId,
    /// Geometries from the last `arrange`, used for borders, focus and hit testing.
    pub geometries: Vec<(TileId, Rect)>,
    /// Tiles waiting for the window of an `@prog`, with the pid of the shell that started it.
    pub swallows: HashMap<TileId, u32>,
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
    pub popups: PopupManager,

    pub seat: Seat<Self>,
    pub config: crate::config::Config,

    pub layer_shell_state: WlrLayerShellState,
    pub viewporter_state: ViewporterState,
    /// A layer (launcher) that asked for the keyboard; it has it until it closes.
    pub layer_focus: Option<WlSurface>,

    /// GPU buffers from clients (linux-dmabuf); the global is created with the backend.
    pub dmabuf_state: DmabufState,
    /// The window we render into (nested), set by `winit::init_winit`.
    pub backend: Option<WinitGraphicsBackend<GlesRenderer>>,
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
        let popups = PopupManager::default();
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let data_device_state = DataDeviceState::new::<Self>(&dh);

        let mut seat_state = SeatState::new();
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "seat0");
        // Keys arrive as raw keycodes (also when nested), so the layout must be ours.
        let config = crate::config::load();
        let xkb_config = XkbConfig {
            rules: &config.keyboard_rules,
            model: &config.keyboard_model,
            layout: &config.keyboard_layout,
            variant: &config.keyboard_variant,
            options: Some(config.keyboard_options.clone()).filter(|o| !o.is_empty()),
        };
        if let Err(err) = seat.add_keyboard(xkb_config, 200, 25) {
            tracing::error!("invalid keyboard layout in the dune config ({err:?}), falling back to the default");
            seat.add_keyboard(Default::default(), 200, 25).unwrap();
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

            layout: Layout::default(),
            tiles: HashMap::new(),
            focused: None,
            next_tile: 1,
            geometries: Vec::new(),
            swallows: HashMap::new(),
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
            popups,
            seat,
            config,
            layer_shell_state,
            viewporter_state,
            layer_focus: None,
            dmabuf_state: DmabufState::new(),
            backend: None,
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
        let layer_surface_under = |layer: Layer| {
            let local = pos - output_loc.to_f64();
            layers.layer_under(layer, local).and_then(|l| {
                let geo = layers.layer_geometry(l)?;
                l.surface_under(local - geo.loc.to_f64(), WindowSurfaceType::ALL)
                    .map(|(s, p)| (s, (p + geo.loc + output_loc).to_f64()))
            })
        };
        layer_surface_under(Layer::Overlay)
            .or_else(|| layer_surface_under(Layer::Top))
            .or_else(|| {
                self.space.element_under(pos).and_then(|(window, location)| {
                    window
                        .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                        .map(|(s, p)| (s, (p + location).to_f64()))
                })
            })
            .or_else(|| layer_surface_under(Layer::Bottom))
            .or_else(|| layer_surface_under(Layer::Background))
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
