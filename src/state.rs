use std::{collections::{HashMap, HashSet}, ffi::OsString, path::PathBuf, sync::Arc};

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

/// One slot of the layout: its windows, the first one put in it at the bottom. A program
/// started with `dune-open` in its terminal, or an app launched as a tab of a float, is
/// stacked on top until it closes; the last window is the one shown.
#[derive(Debug, Default)]
pub struct Tile {
    pub stack: Vec<Window>,
    /// An empty container (Mod+Enter): it takes its place before any window is in it.
    pub reserved: bool,
    /// Process of the window put in it last.
    pub pid: Option<u32>,
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
    /// It is the `fold_click`: a click folds the tile instead of floating it.
    pub fold: bool,
}

#[derive(Clone, Copy)]
pub struct ResizeDrag {
    pub tile: TileId,
    /// Where the button went down.
    pub start: Point<f64, Logical>,
    /// The pointer when last seen.
    pub last: Point<f64, Logical>,
    /// It is the `fold_click`: a click (no drag) folds the tile.
    pub fold: bool,
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
    /// A resize by keys or wheel sliding into place: tiles are drawn on their way there.
    pub resize_animation: Option<crate::tiling::ResizeAnimation>,
    /// Sizes sent to toplevels that have not drawn them yet, with when they were sent.
    pub configure_waits: HashMap<
        smithay::reexports::wayland_server::backend::ObjectId,
        (Option<smithay::utils::Size<i32, Logical>>, std::time::Instant),
    >,
    /// The part of each tile that is on its output; tiles scrolled away are missing.
    pub shown: Vec<(TileId, Rect)>,
    /// Tiles folded into a tab: their bar is drawn, their window is not.
    pub tabs: Vec<TileId>,
    /// Bars of the tabbed containers on the outputs (shown ones only), from the last `arrange`.
    pub tab_bars: Vec<crate::layout::TabBar>,
    /// Tiles of folded tabbed columns' bars on the outputs, with the column's tab count.
    pub fold_counts: Vec<(TileId, usize)>,
    /// Processes playing sound now, and the tiles they belong to (`crate::sound`): alone
    /// in their process, or sharing it (they play if their title says so), with the titles
    /// the media players play.
    pub sound_pids: HashSet<u32>,
    pub sounding: HashSet<TileId>,
    pub sounding_shared: HashSet<TileId>,
    pub sound_titles: Vec<String>,
    /// By output: how many tiles of its strip are past its left and its right edge.
    pub edge_counts: HashMap<String, (usize, usize)>,
    /// Tiles that want to be fullscreen (one per monitor), until focus goes to another
    /// tile of their monitor or they ask to stop.
    pub fullscreen: HashSet<TileId>,
    /// By output: the fullscreen tile covering it now (laid out on its shown desktop).
    pub covers: HashMap<String, TileId>,
    /// Mod+right button drag resizing a tile.
    pub resize_drag: Option<ResizeDrag>,
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
    /// The desktop each output showed before the one it shows: where locking that one goes.
    pub previous_desks: HashMap<String, u8>,
    /// Desktops not shown, by output name and number.
    pub hidden: HashMap<(String, u8), crate::desktops::Desk>,
    /// Tiles waiting in the scratchpad, in order, with where they floated (`crate::scratchpad`).
    pub scratchpad: Vec<(TileId, Option<Rect>)>,
    /// Scratchpad tiles shown, floating: the `scratchpad` key puts them back.
    pub scratch_shown: HashSet<TileId>,
    /// `dune scratchpad watch` connections, and the last line they were sent.
    pub scratchpad_watchers: Vec<std::os::unix::net::UnixStream>,
    pub scratchpad_status: String,
    /// Tiles whose next windows are those of a process (`dune-open`, a `swallow` request)
    /// and its children, with its pid, while it lives.
    pub swallows: HashMap<TileId, u32>,
    /// Apps the launcher started on their own, by their `DUNE_LAUNCH` number: where their
    /// first window goes, and when they started.
    pub launches: HashMap<u64, crate::launcher::Launch>,
    pub next_launch: u64,
    /// The latest window nothing claimed (a tile of its own), its process and when: it
    /// takes the place of a window of that process closing right after (see
    /// `REPLACE_WINDOW`).
    pub unclaimed_window: Option<(TileId, u32, std::time::Instant)>,
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
    /// Clipboard managers and `wl-copy`/`wl-paste` (data control, ext and wlr flavours).
    pub ext_data_control_state: smithay::wayland::selection::ext_data_control::DataControlState,
    pub wlr_data_control_state: smithay::wayland::selection::wlr_data_control::DataControlState,
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
    /// wlr-screencopy (wf-recorder): its frames waiting for something new, see
    /// `crate::wlr_screencopy`.
    pub wlr_screencopy: crate::wlr_screencopy::WlrScreencopy,
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
    /// The screen recording (`record` key) while it runs, see `crate::screenshot`.
    pub recording: Option<crate::screenshot::Recording>,
    /// When wf-recorder started, while it records (the recording's shell says).
    pub recording_since: Option<std::time::SystemTime>,
    /// What the recording takes, unless a whole monitor: framed on screen while it runs.
    pub recording_area: Option<crate::layout::Rect>,
    /// `dune recording watch` connections (bars), told when a recording starts and ends.
    pub recording_watchers: Vec<std::os::unix::net::UnixStream>,
    /// Tiles focused, the latest first: dune-finder lists windows in this order.
    pub recent: Vec<TileId>,
    /// How far the pointer has pushed against a screen edge (see `input::edge_slide`).
    pub edge_push: f64,
    /// The pointer image: clients choose it; dune draws it on a real session.
    pub cursor_status: CursorImageStatus,
    pub cursor: crate::cursor::Cursor,
    /// swayidle's timers (ext-idle-notify-v1), restarted by any input.
    pub idle_notifier_state: smithay::wayland::idle_notify::IdleNotifierState<Dune>,
    /// Surfaces asking to hold off idle (idle-inhibit: videos, games), while they are alive.
    pub idle_inhibitors: Vec<WlSurface>,
    /// Screen lock (swaylock, ext-session-lock-v1), see `crate::lock`.
    pub session_lock: crate::lock::SessionLock,
    /// Tiles and desktops blocked on screen and in captures, see `crate::privacy`.
    pub privacy: crate::privacy::Privacy,
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
        // `wl-copy`/`wl-paste` and clipboard managers set and read both selections without
        // a focused window.
        let ext_data_control_state = smithay::wayland::selection::ext_data_control::DataControlState::new::<Self, _>(
            &dh,
            Some(&primary_selection_state),
            |_| true,
        );
        let wlr_data_control_state = smithay::wayland::selection::wlr_data_control::DataControlState::new::<Self, _>(
            &dh,
            Some(&primary_selection_state),
            |_| true,
        );
        // Games (through xwayland-satellite too) read raw mouse motion and lock or confine
        // the pointer while playing.
        smithay::wayland::relative_pointer::RelativePointerManagerState::new::<Self>(&dh);
        smithay::wayland::pointer_constraints::PointerConstraintsState::new::<Self>(&dh);
        // Screen capture: grim, wf-recorder (wlr-screencopy), and screen sharing through
        // xdg-desktop-portal-wlr.
        let output_capture_source_state = smithay::wayland::image_capture_source::OutputCaptureSourceState::new::<Self>(&dh);
        let image_copy_capture_state = smithay::wayland::image_copy_capture::ImageCopyCaptureState::new::<Self>(&dh);
        let wlr_screencopy = crate::wlr_screencopy::WlrScreencopy::new(&dh);
        let activation_state = smithay::wayland::xdg_activation::XdgActivationState::new::<Self>(&dh);
        // swayidle: idle timeouts, held off by videos and games; swaylock: the screen lock.
        let idle_notifier_state = smithay::wayland::idle_notify::IdleNotifierState::new(&dh, event_loop.handle());
        smithay::wayland::idle_inhibit::IdleInhibitManagerState::new::<Self>(&dh);
        let session_lock = crate::lock::SessionLock::new(&dh);

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
            resize_animation: None,
            configure_waits: HashMap::new(),
            shown: Vec::new(),
            tabs: Vec::new(),
            tab_bars: Vec::new(),
            fold_counts: Vec::new(),
            sound_pids: HashSet::new(),
            sounding: HashSet::new(),
            sounding_shared: HashSet::new(),
            sound_titles: Vec::new(),
            edge_counts: HashMap::new(),
            fullscreen: HashSet::new(),
            covers: HashMap::new(),
            resize_drag: None,
            move_drag: None,
            floating: HashMap::new(),
            float_homes: HashMap::new(),
            tab_homes: HashMap::new(),
            desks: HashMap::new(),
            previous_desks: HashMap::new(),
            hidden: HashMap::new(),
            scratchpad: Vec::new(),
            scratch_shown: HashSet::new(),
            swallows: HashMap::new(),
            scratchpad_watchers: Vec::new(),
            scratchpad_status: String::new(),
            launches: HashMap::new(),
            next_launch: 1,
            unclaimed_window: None,
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
            ext_data_control_state,
            wlr_data_control_state,
            popups,
            seat,
            tab_titles: crate::tab_title::TabTitles::new(&config.font_family, config.tab_widths),
            finder: None,
            recording: None,
            recording_since: None,
            recording_area: None,
            recording_watchers: Vec::new(),
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
            wlr_screencopy,
            backend: Backend::None,
            decorations: Default::default(),
            cursor_status: CursorImageStatus::default_named(),
            cursor: crate::cursor::Cursor::load(),
            idle_notifier_state,
            idle_inhibitors: Vec::new(),
            session_lock,
            privacy: Default::default(),
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

    /// The surface under the pointer (while locked, see `lock_surface_under`): overlay/top layers, then tiles, then bottom/background layers.
    pub fn surface_under(&self, pos: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        // Locked: only the lock surfaces take the pointer.
        if self.session_lock.is_locked() {
            return self.lock_surface_under(pos);
        }
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
        let window_under = || {
            self.space.element_under(pos).and_then(|(window, location)| {
                window
                    .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                    .map(|(s, p)| (s, (p + location).to_f64()))
            })
        };
        // A fullscreen tile is under Overlay layers only (see `output_elements`).
        if self.covers.contains_key(&output.name()) {
            return layer_surface_under(Layer::Overlay, WindowSurfaceType::ALL).or_else(window_under);
        }
        // The popups of a bar under the windows are drawn over them (see `output_elements`).
        layer_surface_under(Layer::Overlay, WindowSurfaceType::ALL)
            .or_else(|| layer_surface_under(Layer::Top, WindowSurfaceType::ALL))
            .or_else(|| layer_surface_under(Layer::Bottom, WindowSurfaceType::POPUP))
            .or_else(|| layer_surface_under(Layer::Background, WindowSurfaceType::POPUP))
            .or_else(window_under)
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
