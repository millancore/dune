//! The panel on screen (a wlr-layer-shell surface, via smithay-client-toolkit), its
//! keyboard and pointer, and what is asked of dune when something is chosen.

use std::time::Duration;

use dune_common::{
    desktop,
    font::Font,
    protocol::{self, Request, TileId, WindowInfo},
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, LoopHandle},
        calloop_wayland_source::WaylandSource,
        client::{
            Connection, QueueHandle,
            globals::registry_queue_init,
            protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};

use crate::{
    apps::Apps,
    finder::{Action, Finder, Mode},
    icons::Icons,
    panel::{self, FONT_SIZE},
};

const BTN_LEFT: u32 = 0x110;
/// Space kept between the panel and the edges of the output (or empty container).
const GAP: i32 = 6;
/// How long dune may take to answer.
const ANSWER: Duration = Duration::from_secs(2);

/// The command line (see `main`).
#[derive(Debug)]
pub struct Options {
    pub mode: Mode,
    pub output: Option<String>,
    pub target: Option<TileId>,
    pub slot: Option<TileId>,
    /// Where the empty container is on the output: x, y, width, height.
    pub at: Option<(i32, i32, i32, i32)>,
    /// Start without the list, until something is typed or browsed.
    pub compact: bool,
    pub font: String,
    pub icon_theme: String,
    pub file_manager: String,
    /// The terminal `file_manager` runs in; `None`: dune's.
    pub file_manager_terminal: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            mode: Mode::Apps,
            output: None,
            target: None,
            slot: None,
            at: None,
            compact: false,
            font: "monospace".into(),
            icon_theme: String::new(),
            file_manager: "yazi".into(),
            file_manager_terminal: None,
        }
    }
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    loop_handle: LoopHandle<'static, App>,
    layer: Option<LayerSurface>,
    size: (i32, i32),
    /// The size of the output the panel is on.
    output_size: (i32, i32),
    /// No list shown yet (`--compact`, nothing typed or browsed).
    compact: bool,
    configured: bool,
    exit: bool,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    modifiers: Modifiers,
    /// Wheel motion not yet turned into a step (touchpads scroll in pixels).
    wheel_rest: f64,
    font: Option<Font>,
    icons: Icons,
    finder: Finder,
    options: Options,
}

pub fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let event_loop: EventLoop<App> = EventLoop::try_new()?;

    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;

    let font = Font::load(&options.font, FONT_SIZE)
        .or_else(|_| Font::load("monospace", FONT_SIZE))
        .inspect_err(|err| eprintln!("dune-finder: no text: {err}"))
        .ok();
    let apps = desktop::apps();
    let picker = Apps::new(apps.clone(), desktop::launches(), options.slot.is_some());
    let home = std::env::var_os("HOME").map_or_else(|| "/".into(), std::path::PathBuf::from);
    let mut finder = Finder::new(
        options.mode,
        picker,
        apps,
        Box::new(windows),
        home,
        options.file_manager.clone(),
    );
    finder.file_manager_terminal = options.file_manager_terminal.clone();
    let (w, h) = panel::panel_size(font.as_ref(), i32::MAX, panel::ROWS);
    let compact = options.compact && finder.untouched();

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new((w * h * 4) as usize, &shm)?,
        shm,
        loop_handle: event_loop.handle(),
        layer: None,
        size: (w, h),
        output_size: (1920, 1080),
        compact,
        configured: false,
        exit: false,
        keyboard: None,
        pointer: None,
        modifiers: Modifiers::default(),
        wheel_rest: 0.0,
        font,
        icons: Icons::new(&options.icon_theme),
        finder,
        options,
    };
    // The outputs' names and sizes, to pick ours and fit it.
    queue.roundtrip(&mut app)?;
    queue.roundtrip(&mut app)?;
    app.create_layer(&compositor, &layer_shell, &qh);

    let mut event_loop = event_loop;
    WaylandSource::new(conn, queue)
        .insert(event_loop.handle())
        .map_err(|e| e.to_string())?;
    while !app.exit {
        event_loop.dispatch(None, &mut app)?;
    }
    Ok(())
}

/// The windows dune has, the most recently focused first.
fn windows() -> Vec<WindowInfo> {
    match protocol::ask(&Request::Windows, ANSWER) {
        Ok(answer) => WindowInfo::decode_all(&answer),
        Err(err) => {
            eprintln!("dune-finder: cannot list the windows: {err}");
            Vec::new()
        }
    }
}

/// Where the panel with `rows` rows goes on an output of `output` size: centered across it
/// a quarter of the way down, or in the middle of the empty container `at`. Its size, then
/// its top-left corner as the top and left margins.
fn placement(
    font: Option<&Font>,
    output: (i32, i32),
    at: Option<(i32, i32, i32, i32)>,
    rows: usize,
) -> ((i32, i32), (i32, i32)) {
    match at {
        Some((x, y, w, h)) => {
            let size = panel::panel_size(font, w - 2 * GAP, rows);
            (size, (y + ((h - size.1) / 2).max(0), x + (w - size.0) / 2))
        }
        None => {
            let size = panel::panel_size(font, output.0 - 2 * GAP, rows);
            (
                size,
                (((output.1 - size.1) / 4).max(0), (output.0 - size.0) / 2),
            )
        }
    }
}

impl App {
    fn create_layer(
        &mut self,
        compositor: &CompositorState,
        layer_shell: &LayerShell,
        qh: &QueueHandle<Self>,
    ) {
        let outputs: Vec<wl_output::WlOutput> = self.output_state.outputs().collect();
        let info = |o: &wl_output::WlOutput| self.output_state.info(o);
        let output = outputs
            .iter()
            .find(|o| {
                self.options.output.is_some() && info(o).and_then(|i| i.name) == self.options.output
            })
            .or(outputs.first());
        let output_size = output
            .and_then(info)
            .and_then(|i| {
                i.logical_size
                    .or_else(|| i.modes.iter().find(|m| m.current).map(|m| m.dimensions))
            })
            .unwrap_or((1920, 1080));
        self.output_size = output_size;
        let (size, (top, left)) = placement(self.font.as_ref(), output_size, self.options.at, self.rows());
        self.size = size;

        let surface = compositor.create_surface(qh);
        let layer = layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("dune-finder"),
            output,
        );
        layer.set_size(size.0 as u32, size.1 as u32);
        // Placed from the output's corner, over any bar.
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_margin(top, 0, 0, left);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.commit();
        self.layer = Some(layer);
    }

    /// Rows of the list shown: none while compact.
    fn rows(&self) -> usize {
        if self.compact { 0 } else { panel::ROWS }
    }

    /// A compact panel something was typed or browsed in grows to the whole panel.
    fn grow(&mut self) {
        if !self.compact || self.finder.untouched() {
            return;
        }
        self.compact = false;
        let Some(layer) = &self.layer else { return };
        let (size, (top, left)) = placement(self.font.as_ref(), self.output_size, self.options.at, self.rows());
        self.size = size;
        layer.set_size(size.0 as u32, size.1 as u32);
        layer.set_margin(top, 0, 0, left);
    }

    fn draw(&mut self) {
        let Some(layer) = &self.layer else { return };
        if !self.configured {
            return;
        }
        let (w, h) = self.size;
        let rows = self.rows();
        let icon_size = panel::icon_size(self.font.as_ref());
        let view = self.finder.view(&mut self.icons, icon_size);
        let pixels = panel::draw(self.font.as_mut(), &view, (w, h), rows);
        let Ok((buffer, canvas)) = self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
        else {
            return eprintln!("dune-finder: cannot get a buffer");
        };
        // The pool rounds slots up (to 64 bytes): the canvas can be longer than the pixels.
        canvas[..pixels.len()].copy_from_slice(&pixels);
        let surface = layer.wl_surface();
        surface.damage_buffer(0, 0, w, h);
        if buffer.attach_to(surface).is_ok() {
            layer.commit();
        }
    }

    /// Do what the finder asks, then show what changed.
    fn act(&mut self, action: Action) {
        match action {
            Action::Continue => {
                self.grow();
                return self.draw();
            }
            Action::Close => {}
            Action::Launch {
                dest,
                command,
                terminal,
                app,
            } => {
                if let Some(app) = app
                    && let Err(err) = desktop::record_launch(&app)
                {
                    eprintln!("dune-finder: cannot count the launch of {app}: {err}");
                }
                let (target, slot) = (self.options.target, self.options.slot);
                if let Err(err) = protocol::ask(
                    &Request::Launch {
                        dest,
                        target,
                        slot,
                        terminal,
                        command,
                    },
                    ANSWER,
                ) {
                    eprintln!("dune-finder: cannot launch: {err}");
                }
            }
            Action::GoTo(tile) => {
                if let Err(err) = protocol::ask(&Request::GoTo { tile }, ANSWER) {
                    eprintln!("dune-finder: cannot go to the window: {err}");
                }
            }
        }
        self.exit = true;
    }

    fn on_key(&mut self, event: KeyEvent) {
        let text = event.utf8.as_deref().and_then(|t| t.chars().next());
        let action = self.finder.feed(&self.modifiers, event.keysym, text);
        self.act(action);
    }

    fn on_pointer(&mut self, event: &PointerEvent) {
        match event.kind {
            PointerEventKind::Press {
                button: BTN_LEFT, ..
            } => {
                let row = panel::row_at(self.font.as_ref(), event.position.1, self.rows());
                let action = self.finder.click(row);
                self.act(action);
            }
            PointerEventKind::Axis { vertical, .. } => {
                let steps = if vertical.value120 != 0 {
                    vertical.value120 as f64 / 120.0
                } else if vertical.discrete != 0 {
                    vertical.discrete as f64
                } else {
                    vertical.absolute / panel::line_height(self.font.as_ref()) as f64
                };
                self.wheel_rest += steps;
                let whole = self.wheel_rest.trunc() as i32;
                self.wheel_rest -= whole as f64;
                for _ in 0..whole.abs() {
                    let action = self.finder.scroll(whole > 0);
                    self.act(action);
                }
            }
            _ => {}
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}

    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let (w, h) = configure.new_size;
        if w > 0 && h > 0 {
            self.size = (w as i32, h as i32);
        }
        self.configured = true;
        self.draw();
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let keyboard = self.seat_state.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                self.loop_handle.clone(),
                Box::new(|app, _, event| app.on_key(event)),
            );
            self.keyboard = keyboard.ok();
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
    }

    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Keyboard
            && let Some(keyboard) = self.keyboard.take()
        {
            keyboard.release();
        }
        if capability == Capability::Pointer
            && let Some(pointer) = self.pointer.take()
        {
            pointer.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.on_key(event);
    }

    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.on_key(event);
    }

    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for event in events {
            if self
                .layer
                .as_ref()
                .is_some_and(|l| &event.surface == l.wl_surface())
                && !self.exit
            {
                self.on_pointer(event);
            }
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(App);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_is_placed_on_the_output_or_in_the_container() {
        let ((w, h), (top, left)) = placement(None, (4000, 2000), None, panel::ROWS);
        assert_eq!(left, (4000 - w) / 2);
        assert_eq!(top, (2000 - h) / 4);
        // In a container narrower than the panel: it shrinks to fit inside.
        let ((w, h), (top, left)) = placement(None, (4000, 2000), Some((100, 50, 400, 2000)), panel::ROWS);
        assert_eq!(w, 400 - 2 * GAP);
        assert_eq!(left, 100 + GAP);
        assert_eq!(top, 50 + (2000 - h) / 2);
        // Compact: shorter, still in the container's middle.
        let ((_, compact), (top, _)) = placement(None, (4000, 2000), Some((100, 50, 400, 2000)), 0);
        assert!(compact < h);
        assert_eq!(top, 50 + (2000 - compact) / 2);
    }
}
