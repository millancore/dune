//! The window (Wayland, via smithay-client-toolkit) and the terminal behind it
//! (alacritty_terminal, its PTY read on a separate thread).

use std::{
    io::{Read, Write},
    os::fd::AsRawFd,
    sync::Arc,
    time::{Duration, Instant},
};

use dune_common::{
    at::{self, AtLayer},
    picker,
};

use alacritty_terminal::{
    event::{Event as TermEvent, EventListener, WindowSize},
    event_loop::{EventLoop as PtyLoop, EventLoopSender, Msg},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point, Side},
    selection::{Selection, SelectionType},
    sync::FairMutex,
    term::{ClipboardType, Config as TermConfig, Term, TermMode, test::TermSize},
    tty,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    data_device_manager::{
        DataDeviceManagerState, WritePipe,
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        data_source::{CopyPasteSource, DataSourceHandler},
    },
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            EventLoop, LoopHandle,
            channel::{self, Channel},
            timer::{TimeoutAction, Timer},
        },
        calloop_wayland_source::WaylandSource,
        client::{
            Connection, QueueHandle,
            globals::registry_queue_init,
            protocol::{
                wl_data_device::WlDataDevice, wl_data_device_manager::DndAction, wl_data_source::WlDataSource,
                wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface,
            },
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
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{
        Shm, ShmHandler,
        slot::{Buffer, SlotPool},
    },
};

use crate::{
    config::Config,
    font::{Font, points_to_pixels},
    keys::{self, Shortcut},
    render::{self, Canvas, PADDING},
};

const DEFAULT_COLS: u32 = 80;
const DEFAULT_LINES: u32 = 24;
const TEXT_MIMES: [&str; 3] = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"];
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;
const MULTI_CLICK: Duration = Duration::from_millis(400);
/// Lines per wheel step.
const WHEEL_LINES: i32 = 3;

/// Forwards terminal events from the PTY thread to the main loop.
#[derive(Clone)]
pub struct Proxy(channel::Sender<TermEvent>);

impl EventListener for Proxy {
    fn send_event(&self, event: TermEvent) {
        let _ = self.0.send(event);
    }
}

/// Where pasted text goes once the clipboard owner has sent it.
enum PasteTarget {
    /// Typed into the terminal (Ctrl+Shift+V, middle click).
    Input,
    /// Answer to an application's OSC 52 clipboard query.
    Application(Arc<dyn Fn(&str) -> String + Sync + Send + 'static>),
}

pub struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    data_device_manager: DataDeviceManagerState,
    loop_handle: LoopHandle<'static, App>,
    qh: QueueHandle<App>,

    window: Window,
    pool: SlotPool,
    buffer: Option<Buffer>,
    width: u32,
    height: u32,
    configured: bool,
    /// Content changed since the last frame.
    dirty: bool,
    /// A frame callback is pending: draw when it fires, not before.
    frame_pending: bool,
    exit: bool,

    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    data_device: Option<DataDevice>,
    modifiers: Modifiers,
    focused: bool,
    /// Serial of the last input event, needed to set the clipboard.
    last_serial: u32,
    pointer_pos: (f64, f64),
    selecting: bool,
    /// Time, cell and count of the last left click (double/triple click selection).
    last_click: Option<(Instant, Point, u8)>,
    /// Wheel movement not yet worth a whole line.
    scroll_rest: f64,
    /// What we offer while we own the clipboard.
    clipboard: Option<(CopyPasteSource, String)>,
    pastes: channel::Sender<(String, bool)>,
    paste_target: Option<PasteTarget>,

    config: Config,
    font: Font,
    term: Arc<FairMutex<Term<Proxy>>>,
    pty: EventLoopSender,
    grid: (usize, usize),

    /// dune's `@` layer, when running as a dune tile.
    at: Option<AtLayer>,
    /// The PTY (a duplicate of its descriptor) and the shell, to tell whether the
    /// shell is at its prompt.
    pty_file: Option<std::fs::File>,
    shell_pid: u32,
    /// Where the command line being typed at the prompt starts, as (line counted
    /// from the top of the scrollback, column), so it survives scrolling.
    line_start: Option<(i64, usize)>,
    /// An Enter held until the shell has drawn the line, and keys typed meanwhile.
    pending_enter: Option<Instant>,
    held_keys: Vec<Vec<u8>>,
    /// When we last wrote to the shell, and when it last printed.
    last_typed: Option<Instant>,
    last_output: Option<Instant>,
}

/// How long the shell must be quiet after echoing before a held Enter reads the line,
/// and the longest an Enter is held.
const ECHO_QUIET: Duration = Duration::from_millis(15);
const ECHO_TIMEOUT: Duration = Duration::from_millis(300);

/// Run the terminal; `command` (if any) replaces the shell.
pub fn run(command: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    WaylandSource::new(conn.clone(), queue)
        .insert(event_loop.handle())
        .map_err(|e| e.to_string())?;

    let compositor = CompositorState::bind(&globals, &qh)?;
    let xdg_shell = XdgShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;
    let data_device_manager = DataDeviceManagerState::bind(&globals, &qh)?;

    let config = crate::config::load();
    let font = Font::load(&config.font_family, points_to_pixels(config.font_size))?;
    let width = DEFAULT_COLS * font.cell_width + 2 * PADDING;
    let height = DEFAULT_LINES * font.cell_height + 2 * PADDING;

    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_title("dune-term");
    window.set_app_id("dune-term");
    window.commit();

    // The terminal and its PTY. Events from the PTY thread arrive through a channel.
    tty::setup_env();
    let (sender, receiver): (channel::Sender<TermEvent>, Channel<TermEvent>) = channel::channel();
    let proxy = Proxy(sender);
    let grid = render::grid_size(&font, width, height);
    let term = Term::new(TermConfig::default(), &TermSize::new(grid.0, grid.1), proxy.clone());
    let term = Arc::new(FairMutex::new(term));

    let mut options = tty::Options::default();
    // Inside dune, `$SHELL` is dune's wrapper doing the `@` layer for any terminal;
    // dune-term does that layer itself, so it starts the user's shell directly.
    let mut at_layer = None;
    if let Some((program, args)) = command.split_first() {
        options.shell = Some(tty::Shell::new(program.clone(), args.to_vec()));
    } else if let Some(shell) = at::wrapped_user_shell() {
        options.shell = Some(tty::Shell::new(shell.to_string_lossy().into_owned(), Vec::new()));
        // Read before starting the shell, which must not see it.
        let run_first = std::env::var("DUNE_RUN").ok();
        unsafe {
            std::env::remove_var("DUNE_RUN");
            std::env::set_var("SHELL", &shell);
        }
        at_layer = AtLayer::enabled().then_some(run_first);
    }
    let pty = tty::new(&options, window_size(&font, grid), 0)?;
    let shell_pid = pty.child().id();
    let pty_file = pty.file().try_clone().ok();
    let at = at_layer.map(|run_first| AtLayer::for_tile(shell_pid, run_first));
    let pty_loop = PtyLoop::new(term.clone(), proxy, pty, false, false)?;
    let pty_sender = pty_loop.channel();
    let _reader = pty_loop.spawn();

    event_loop
        .handle()
        .insert_source(receiver, |event, _, app| {
            if let channel::Event::Msg(event) = event {
                app.on_term_event(event);
            }
        })
        .map_err(|e| e.to_string())?;

    // Clipboard contents are read on a thread (the owner may be this very terminal,
    // which must keep serving the request meanwhile) and come back here.
    let (pastes, paste_receiver) = channel::channel::<(String, bool)>();
    event_loop
        .handle()
        .insert_source(paste_receiver, |event, _, app| {
            if let channel::Event::Msg((text, ok)) = event {
                app.on_clipboard_text(text, ok);
            }
        })
        .map_err(|e| e.to_string())?;

    // Notice prompts coming back (end of an `@prog`) and run the tile's first command.
    event_loop
        .handle()
        .insert_source(Timer::from_duration(Duration::from_millis(200)), |_, _, app| {
            app.tick_at();
            app.try_finish_enter();
            let next = if app.pending_enter.is_some() {
                5
            } else if app.at.as_ref().is_some_and(|a| a.waiting()) {
                50
            } else {
                200
            };
            TimeoutAction::ToDuration(Duration::from_millis(next))
        })
        .map_err(|e| e.to_string())?;

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new((width * height * 4) as usize, &shm)?,
        shm,
        data_device_manager,
        loop_handle: event_loop.handle(),
        qh: qh.clone(),
        window,
        buffer: None,
        width,
        height,
        configured: false,
        dirty: true,
        frame_pending: false,
        exit: false,
        keyboard: None,
        pointer: None,
        data_device: None,
        modifiers: Modifiers::default(),
        focused: false,
        last_serial: 0,
        pointer_pos: (0.0, 0.0),
        selecting: false,
        last_click: None,
        scroll_rest: 0.0,
        clipboard: None,
        pastes,
        paste_target: None,
        config,
        font,
        term,
        pty: pty_sender,
        grid,
        at,
        pty_file,
        shell_pid,
        line_start: None,
        pending_enter: None,
        held_keys: Vec::new(),
        last_typed: None,
        last_output: None,
    };

    while !app.exit {
        event_loop.dispatch(None, &mut app)?;
    }
    let _ = app.pty.send(Msg::Shutdown);
    Ok(())
}

/// Trace of the `@` layer on stderr, with `DUNE_DEBUG=1` (for the end-to-end tests).
fn debug(args: std::fmt::Arguments) {
    if std::env::var("DUNE_DEBUG").as_deref() == Ok("1") {
        eprintln!("dune-term[{}]: {args}", std::env::var("DUNE_TILE").unwrap_or_default());
    }
}

fn window_size(font: &Font, (cols, lines): (usize, usize)) -> WindowSize {
    WindowSize {
        num_cols: cols as u16,
        num_lines: lines as u16,
        cell_width: font.cell_width as u16,
        cell_height: font.cell_height as u16,
    }
}

impl App {
    fn on_term_event(&mut self, event: TermEvent) {
        match event {
            TermEvent::Wakeup => {
                self.last_output = Some(Instant::now());
                if let Some(at) = &mut self.at {
                    at.output_seen();
                }
                self.request_redraw()
            }
            TermEvent::CursorBlinkingChange | TermEvent::MouseCursorDirty => self.request_redraw(),
            TermEvent::Title(title) => self.window.set_title(title),
            TermEvent::ResetTitle => self.window.set_title("dune-term"),
            TermEvent::PtyWrite(text) => self.write(text.into_bytes()),
            TermEvent::ColorRequest(index, format) => {
                let color = {
                    let term = self.term.lock();
                    term.colors()[index].unwrap_or_else(|| {
                        crate::palette::resolve(
                            alacritty_terminal::vte::ansi::Color::Indexed(index.min(255) as u8),
                            term.colors(),
                        )
                    })
                };
                self.write(format(color).into_bytes());
            }
            TermEvent::TextAreaSizeRequest(format) => {
                let size = window_size(&self.font, self.grid);
                self.write(format(size).into_bytes());
            }
            // OSC 52: applications reading and writing the clipboard.
            TermEvent::ClipboardStore(ClipboardType::Clipboard, text) => self.set_clipboard(text),
            TermEvent::ClipboardLoad(ClipboardType::Clipboard, format) => {
                self.request_clipboard(PasteTarget::Application(format))
            }
            TermEvent::ChildExit(_) | TermEvent::Exit => self.exit = true,
            TermEvent::ClipboardStore(..) | TermEvent::ClipboardLoad(..) | TermEvent::Bell => {}
        }
    }

    fn write(&mut self, bytes: Vec<u8>) {
        self.last_typed = Some(Instant::now());
        let _ = self.pty.send(Msg::Input(bytes.into()));
    }

    fn request_redraw(&mut self) {
        self.dirty = true;
        if self.configured && !self.frame_pending {
            self.draw();
        }
    }

    fn on_key(&mut self, event: KeyEvent) {
        if let Some(shortcut) = keys::shortcut(event.keysym, &self.modifiers) {
            return self.run_shortcut(shortcut);
        }
        let app_cursor = self.term.lock().mode().contains(TermMode::APP_CURSOR);
        if let Some(bytes) = keys::encode(event.keysym, event.utf8.as_deref(), &self.modifiers, app_cursor) {
            // Typing jumps back to the bottom of the scrollback and drops the selection.
            {
                let mut term = self.term.lock();
                term.scroll_display(Scroll::Bottom);
                term.selection = None;
            }
            self.send_typed(bytes);
        }
    }

    /// Typed bytes on their way to the shell, through the `@` layer when in dune.
    fn send_typed(&mut self, bytes: Vec<u8>) {
        // Keys typed while an Enter waits for the shell go after it, in order.
        if self.pending_enter.is_some() {
            return self.held_keys.push(bytes);
        }
        let Some(at) = self.at.as_ref() else {
            return self.write(bytes);
        };
        let at_prompt = self.at_prompt();
        if at_prompt && at.line_is_empty() {
            // This key starts a new command line, right after the prompt.
            self.line_start = Some(self.cursor_position());
        }
        // Enter on a typed command line: read the line once the shell has drawn all of
        // it (a busy shell can lag behind fast typing), then decide where it runs.
        if bytes == b"\r" && at_prompt && at.picker().is_none() && !at.line_is_empty() {
            self.pending_enter = Some(Instant::now());
            return self.try_finish_enter();
        }

        let typed = bytes.clone();
        let bytes = self.at.as_mut().unwrap().input(&bytes, at_prompt);
        debug(format_args!(
            "key {:?} at_prompt={at_prompt} picker={} -> shell {:?}",
            String::from_utf8_lossy(&typed),
            self.at.as_ref().unwrap().picker().map_or("-".into(), |p| format!("@{}", p.input())),
            String::from_utf8_lossy(&bytes)
        ));
        self.request_redraw(); // the picker may have opened, changed or closed
        if !bytes.is_empty() {
            self.write(bytes);
        }
    }

    /// Finish a held Enter once the shell has echoed what was typed and gone quiet
    /// (or after `ECHO_TIMEOUT`): `cmd:v args` / `cmd:h args` or the tile's direction
    /// indicator send the line to a new tile (the shell only gets it cleared);
    /// otherwise Enter goes to the shell.
    fn try_finish_enter(&mut self) {
        let Some(since) = self.pending_enter else { return };
        let echoed = self
            .last_output
            .is_some_and(|out| self.last_typed.is_none_or(|typed| out > typed) && out.elapsed() >= ECHO_QUIET);
        if !echoed && since.elapsed() < ECHO_TIMEOUT {
            return; // the timer tries again
        }
        self.pending_enter = None;

        let text = self.typed_line().map(|t| t.trim().to_string()).unwrap_or_default();
        if let Some(line) = picker::parse_split_command(&text) {
            debug(format_args!("split command {line:?}"));
            self.at.as_mut().unwrap().split_command(line);
            self.line_start = None;
            self.write(b"\x15".to_vec()); // Ctrl-U: kill the line
        } else if let Some(side) = (!text.is_empty()).then(|| self.at.as_ref().unwrap().take_direction()).flatten() {
            debug(format_args!("direction {side:?}: {text:?}"));
            self.at.as_mut().unwrap().open_beside(side, text);
            self.line_start = None;
            self.write(b"\x15".to_vec());
        } else {
            let at_prompt = self.at_prompt();
            let bytes = self.at.as_mut().unwrap().input(b"\r", at_prompt);
            debug(format_args!("enter -> shell {:?} ({text:?})", String::from_utf8_lossy(&bytes)));
            self.write(bytes);
        }

        for keys in std::mem::take(&mut self.held_keys) {
            self.send_typed(keys);
        }
    }

    /// The cursor as (line counted from the top of the scrollback, column).
    fn cursor_position(&self) -> (i64, usize) {
        let term = self.term.lock();
        let point = term.grid().cursor.point;
        (term.grid().history_size() as i64 + point.line.0 as i64, point.column.0)
    }

    /// The command line typed since the prompt: the screen text from where typing
    /// started up to the cursor (so a right-side prompt or a dimmed autosuggestion
    /// after the cursor is not part of it).
    fn typed_line(&self) -> Option<String> {
        let (start_line, start_col) = self.line_start?;
        let term = self.term.lock();
        let grid = term.grid();
        let history = grid.history_size() as i64;
        let cursor = grid.cursor.point;
        let (end_line, end_col) = (cursor.line.0 as i64, cursor.column.0);
        let start_line = start_line - history;
        if start_line < -history || (start_line, start_col) > (end_line, end_col) {
            return None;
        }
        let mut text = String::new();
        for line in start_line..=end_line {
            let row = &grid[Line(line as i32)];
            let from = if line == start_line { start_col } else { 0 };
            let to = if line == end_line { end_col } else { grid.columns() };
            for col in from..to.min(grid.columns()) {
                let cell = &row[Column(col)];
                if !cell.flags.contains(alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER) {
                    text.push(cell.c);
                }
            }
        }
        Some(text)
    }

    /// The shell is at its prompt: it is the foreground process group of the PTY.
    fn at_prompt(&self) -> bool {
        self.pty_file.as_ref().is_some_and(|f| {
            let pgrp = unsafe { libc::tcgetpgrp(f.as_raw_fd()) };
            pgrp > 0 && pgrp as u32 == self.shell_pid
        })
    }

    fn tick_at(&mut self) {
        if self.at.is_none() {
            return;
        }
        let at_prompt = self.at_prompt();
        let bytes = self.at.as_mut().unwrap().tick(at_prompt);
        if !bytes.is_empty() {
            self.write(bytes);
        }
    }

    fn run_shortcut(&mut self, shortcut: Shortcut) {
        match shortcut {
            Shortcut::Copy => {
                let text = self.term.lock().selection_to_string();
                if let Some(text) = text {
                    self.set_clipboard(text);
                }
            }
            Shortcut::Paste => self.request_clipboard(PasteTarget::Input),
            Shortcut::ScrollPageUp => self.term.lock().scroll_display(Scroll::PageUp),
            Shortcut::ScrollPageDown => self.term.lock().scroll_display(Scroll::PageDown),
            Shortcut::ZoomIn => self.set_font_size(self.font.size() + 2.0),
            Shortcut::ZoomOut => self.set_font_size(self.font.size() - 2.0),
            Shortcut::ZoomReset => self.set_font_size(points_to_pixels(self.config.font_size)),
        }
        self.request_redraw();
    }

    fn set_font_size(&mut self, size: f32) {
        let size = size.clamp(6.0, 128.0);
        if let Ok(font) = Font::load(self.font.family(), size) {
            self.font = font;
            self.apply_grid();
        }
    }

    // ---- clipboard ----

    fn set_clipboard(&mut self, text: String) {
        let Some(device) = &self.data_device else { return };
        let source = self.data_device_manager.create_copy_paste_source(&self.qh, TEXT_MIMES);
        source.set_selection(device, self.last_serial);
        self.clipboard = Some((source, text));
    }

    fn request_clipboard(&mut self, target: PasteTarget) {
        // Our own selection: no round trip through the compositor.
        if let Some((_, text)) = &self.clipboard {
            let text = text.clone();
            self.paste_target = Some(target);
            return self.on_clipboard_text(text, true);
        }
        let Some(offer) = self.data_device.as_ref().and_then(|d| d.data().selection_offer()) else { return };
        let Some(mime) = offer.with_mime_types(|mimes| {
            TEXT_MIMES.iter().find(|m| mimes.iter().any(|x| x == *m)).map(|m| m.to_string())
        }) else {
            return;
        };
        let Ok(mut pipe) = offer.receive(mime) else { return };
        self.paste_target = Some(target);
        let sender = self.pastes.clone();
        std::thread::spawn(move || {
            let mut data = Vec::new();
            let ok = pipe.read_to_end(&mut data).is_ok();
            let _ = sender.send((String::from_utf8_lossy(&data).into_owned(), ok));
        });
    }

    fn on_clipboard_text(&mut self, text: String, ok: bool) {
        match self.paste_target.take() {
            Some(PasteTarget::Input) if ok && !text.is_empty() => self.paste(&text),
            Some(PasteTarget::Application(format)) => self.write(format(&text).into_bytes()),
            _ => {}
        }
    }

    /// Insert text as typed, bracketed when the application asked for it so a shell
    /// doesn't run pasted lines on its own.
    fn paste(&mut self, text: &str) {
        if let Some(at) = self.at.as_mut().filter(|a| a.picker().is_some()) {
            let bytes = at.input(text.replace(['\r', '\n'], " ").as_bytes(), true);
            self.request_redraw();
            return self.write(bytes);
        }
        let bracketed = self.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
        let bytes = if bracketed {
            // A paste must not be able to end the bracket itself.
            format!("\x1b[200~{}\x1b[201~", text.replace("\x1b[201~", "")).into_bytes()
        } else {
            text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
        };
        self.term.lock().scroll_display(Scroll::Bottom);
        self.write(bytes);
    }

    // ---- mouse ----

    /// The grid cell under a window position, and which half of it.
    fn cell_at(&self, (x, y): (f64, f64)) -> (Point, Side) {
        let (cw, ch) = (self.font.cell_width as f64, self.font.cell_height as f64);
        let col_f = ((x - PADDING as f64) / cw).max(0.0);
        let col = (col_f as usize).min(self.grid.0 - 1);
        let row = (((y - PADDING as f64) / ch).max(0.0) as usize).min(self.grid.1 - 1);
        let side = if col_f.fract() < 0.5 { Side::Left } else { Side::Right };
        let offset = self.term.lock().grid().display_offset() as i32;
        (Point::new(Line(row as i32 - offset), Column(col)), side)
    }

    /// Applications that asked for mouse events get them, unless Shift is held.
    fn mouse_reporting(&self) -> bool {
        !self.modifiers.shift && self.term.lock().mode().intersects(TermMode::MOUSE_MODE)
    }

    fn report_mouse(&mut self, button: u8, pressed: bool) {
        // cell_at takes the terminal lock itself, and it is not reentrant: holding it
        // here froze the whole tile on the first click in a mouse-reporting program.
        let (point, _) = self.cell_at(self.pointer_pos);
        let term = self.term.lock();
        if !term.mode().contains(TermMode::SGR_MOUSE) {
            return; // Only the SGR encoding is supported.
        }
        let line = (point.line.0 + term.grid().display_offset() as i32).max(0) as usize;
        drop(term);
        self.write(keys::mouse_sgr(button, point.column.0, line, pressed));
    }

    fn on_pointer(&mut self, event: &PointerEvent) {
        self.pointer_pos = event.position;
        match event.kind {
            PointerEventKind::Press { button, serial, .. } => {
                self.last_serial = serial;
                if self.mouse_reporting() {
                    let code = match button {
                        BTN_LEFT => 0,
                        BTN_MIDDLE => 1,
                        BTN_RIGHT => 2,
                        _ => return,
                    };
                    return self.report_mouse(code, true);
                }
                match button {
                    BTN_LEFT => self.start_selection(),
                    BTN_MIDDLE => self.request_clipboard(PasteTarget::Input),
                    _ => {}
                }
            }
            PointerEventKind::Release { button, serial, .. } => {
                self.last_serial = serial;
                if self.mouse_reporting() {
                    if let Some(code) = [BTN_LEFT, BTN_MIDDLE, BTN_RIGHT].iter().position(|b| *b == button) {
                        self.report_mouse(code as u8, false);
                    }
                    return;
                }
                if button == BTN_LEFT {
                    self.selecting = false;
                }
            }
            PointerEventKind::Motion { .. } if self.selecting => {
                let (point, side) = self.cell_at(self.pointer_pos);
                if let Some(selection) = self.term.lock().selection.as_mut() {
                    selection.update(point, side);
                }
                self.request_redraw();
            }
            PointerEventKind::Axis { vertical, .. } => {
                // Wayland: positive is down. Wheels report steps; touchpads only pixels.
                let lines = if vertical.value120 != 0 {
                    vertical.value120 as f64 / 120.0 * WHEEL_LINES as f64
                } else if vertical.discrete != 0 {
                    (vertical.discrete * WHEEL_LINES) as f64
                } else {
                    vertical.absolute / self.font.cell_height as f64
                };
                self.scroll_rest += lines;
                let whole = self.scroll_rest.trunc() as i32;
                self.scroll_rest -= whole as f64;
                if whole != 0 {
                    self.scroll(whole);
                }
            }
            _ => {}
        }
    }

    fn start_selection(&mut self) {
        let (point, side) = self.cell_at(self.pointer_pos);
        let count = match self.last_click {
            Some((at, p, n)) if at.elapsed() < MULTI_CLICK && p == point => n % 3 + 1,
            _ => 1,
        };
        self.last_click = Some((Instant::now(), point, count));
        let kind = match count {
            1 => SelectionType::Simple,
            2 => SelectionType::Semantic,
            _ => SelectionType::Lines,
        };
        self.term.lock().selection = Some(Selection::new(kind, point, side));
        self.selecting = true;
        self.request_redraw();
    }

    /// Wheel: scrollback normally; arrow keys in full-screen programs (less, man);
    /// wheel reports to programs that asked for the mouse.
    fn scroll(&mut self, lines: i32) {
        if self.mouse_reporting() {
            let button = if lines < 0 { 64 } else { 65 };
            for _ in 0..lines.abs() {
                self.report_mouse(button, true);
            }
            return;
        }
        let mode = *self.term.lock().mode();
        if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            let key: &[u8] = match (lines < 0, mode.contains(TermMode::APP_CURSOR)) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            self.write(key.repeat(lines.unsigned_abs() as usize));
        } else {
            self.term.lock().scroll_display(Scroll::Delta(-lines));
            self.request_redraw();
        }
    }

    // ---- size and drawing ----

    fn resize(&mut self, width: u32, height: u32) {
        if (width, height) == (self.width, self.height) {
            return;
        }
        self.width = width;
        self.height = height;
        self.buffer = None;
        self.apply_grid();
    }

    /// Fit the grid to the window after a size or font change.
    fn apply_grid(&mut self) {
        let grid = render::grid_size(&self.font, self.width, self.height);
        if grid != self.grid {
            self.grid = grid;
            self.term.lock().resize(TermSize::new(grid.0, grid.1));
            let _ = self.pty.send(Msg::Resize(window_size(&self.font, grid)));
        }
        self.dirty = true;
    }

    fn draw(&mut self) {
        let (width, height) = (self.width, self.height);
        let stride = width as i32 * 4;
        let buffer = self.buffer.get_or_insert_with(|| {
            self.pool
                .create_buffer(width as i32, height as i32, stride, wl_shm::Format::Argb8888)
                .expect("create buffer")
                .0
        });
        let pixels = match self.pool.canvas(buffer) {
            Some(pixels) => pixels,
            None => {
                // The compositor still holds the previous buffer: use a second one.
                let (second, pixels) = self
                    .pool
                    .create_buffer(width as i32, height as i32, stride, wl_shm::Format::Argb8888)
                    .expect("create buffer");
                *buffer = second;
                pixels
            }
        };

        {
            let term = self.term.lock();
            let mut canvas = Canvas { pixels, width, height };
            let picker = self.at.as_ref().and_then(|a| a.picker());
            render::draw(&mut canvas, &term, &mut self.font, self.focused, picker);
        }

        let surface = self.window.wl_surface();
        surface.damage_buffer(0, 0, width as i32, height as i32);
        surface.frame(&self.qh, FrameCallbackData(surface.clone()));
        buffer.attach_to(surface).expect("buffer attach");
        self.window.commit();
        self.frame_pending = true;
        self.dirty = false;
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

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        if self.dirty {
            self.draw();
        }
    }

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

impl WindowHandler for App {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window, configure: WindowConfigure, _: u32) {
        let width = configure.new_size.0.map_or(self.width, |w| w.get());
        let height = configure.new_size.1.map_or(self.height, |h| h.get());
        self.resize(width, height);
        self.configured = true;
        if !self.frame_pending {
            self.draw();
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        if self.data_device.is_none() {
            self.data_device = Some(self.data_device_manager.get_data_device(qh, &seat));
        }
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
        surface: &wl_surface::WlSurface,
        serial: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        if self.window.wl_surface() == surface {
            self.last_serial = serial;
            self.focused = true;
            self.request_redraw();
        }
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32) {
        if self.window.wl_surface() == surface {
            self.focused = false;
            self.request_redraw();
        }
    }

    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, serial: u32, event: KeyEvent) {
        self.last_serial = serial;
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
            if &event.surface == self.window.wl_surface() {
                self.on_pointer(event);
            }
        }
    }
}

impl DataDeviceHandler for App {
    // Drag and drop is not supported; only the clipboard.
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64, _: &wl_surface::WlSurface) {}

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}

    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64) {}

    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}

    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
}

impl DataOfferHandler for App {
    fn source_actions(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}

    fn selected_action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}
}

impl DataSourceHandler for App {
    fn accept_mime(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: Option<String>) {}

    fn send_request(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource, _: String, pipe: WritePipe) {
        if let Some((ours, text)) = &self.clipboard
            && ours.inner() == source
        {
            // Small writes; a pipe that blocks forever is the reader's problem, so do it off-thread.
            let text = text.clone();
            std::thread::spawn(move || {
                let mut file = std::fs::File::from(std::os::fd::OwnedFd::from(pipe));
                let _ = file.write_all(text.as_bytes());
            });
        }
    }

    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        // Someone else owns the clipboard now.
        if self.clipboard.as_ref().is_some_and(|(ours, _)| ours.inner() == source) {
            self.clipboard = None;
        }
        source.destroy();
    }

    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}

    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}

    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}
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
