//! The launcher (Mod+d): a panel dune draws over everything, like rofi. It searches the
//! desktop applications with the picker and lets the user choose, before Enter,
//! where the app opens: on a side of the focused tile, floating, stacked on it, on
//! another monitor or desktop (`firefox @2`, see `Dest::with`). Graphical apps start on
//! their own, like rofi does; console programs get a terminal.

use std::{
    collections::HashMap,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

use dune_common::{
    desktop::{self, App},
    font::{Font, Style},
    picker::{Key, MAX_ROWS, Outcome, Picker},
};
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    input::keyboard::{Keysym, ModifiersState},
    utils::Transform,
};

use crate::{
    Dune,
    icons::{Icon, Icons},
    layout::{Side, TileId},
};

/// Text size, in pixels.
const FONT_SIZE: f32 = 18.0;
/// Characters across the panel.
const COLUMNS: usize = 60;
const PADDING: i32 = 16;
/// Space around an icon, inside its line.
const ICON_MARGIN: i32 = 2;
const FRAME: i32 = 2;
const BACKGROUND: [u8; 3] = [0x1e, 0x20, 0x30];
const FRAME_COLOR: [u8; 3] = [0x5c, 0x9e, 0xf2];
const SELECTED: [u8; 3] = [0x3b, 0x42, 0x61];
const TEXT: [u8; 3] = [0xd8, 0xdc, 0xe6];
const DIM: [u8; 3] = [0x7f, 0x84, 0x9c];
const ACCENT: [u8; 3] = [0xfa, 0x9e, 0x38];
const HINT: &str = "@3 desk · @m2 monitor · @s stack · @f float · Ctrl+arrows";
const DEST_HELP: &str = "@: 1-9 desktop · m1-m9 monitor · s stacked · n new · f float";

/// Where the launched app goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// A column of its own, right of the focused tile's.
    Column,
    /// On a side of the focused tile, inside its column.
    Side(Side),
    Float,
}

impl Place {
    fn label(self) -> &'static str {
        match self {
            Place::Column => "opens in a new column",
            Place::Side(Side::Left) => "opens ← left",
            Place::Side(Side::Right) => "opens → right",
            Place::Side(Side::Up) => "opens ↑ above",
            Place::Side(Side::Down) => "opens ↓ below",
            Place::Float => "opens floating",
        }
    }
}

/// Where the launched app goes, chosen with Ctrl keys and the `@` word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dest {
    pub place: Place,
    /// On top of the focused tile, which shows again when the app closes.
    pub stack: bool,
    /// Monitor number, left to right, from 1.
    pub monitor: Option<u8>,
    pub desk: Option<u8>,
}

impl Dest {
    /// `self` changed by an `@` word, its letters in any order: a digit is a desktop,
    /// `m` and a digit a monitor, `s` stacked on the current tile, `n` a new tile,
    /// `f` floating. `@3`, `@m2`, `@m23` (monitor 2, desktop 3), `@s`, `@2f`.
    /// None when the word is not one.
    fn with(mut self, word: &str) -> Option<Dest> {
        let mut chars = word.strip_prefix('@')?.chars();
        let digit = |c: Option<char>| c.and_then(|c| c.to_digit(10)).filter(|d| *d >= 1).map(|d| d as u8);
        while let Some(c) = chars.next() {
            match c.to_ascii_lowercase() {
                'm' => self.monitor = Some(digit(chars.next())?),
                's' => self.stack = true,
                'n' => self.stack = false,
                'f' => self.place = Place::Float,
                c => self.desk = Some(digit(Some(c))?),
            }
        }
        Some(self)
    }

    fn label(&self) -> String {
        if self.stack {
            return "opens stacked on this tile".into();
        }
        let mut label = self.place.label().to_string();
        if let Some(m) = self.monitor {
            label.push_str(&format!(" · monitor {m}"));
        }
        if let Some(d) = self.desk {
            label.push_str(&format!(" · desktop {d}"));
        }
        label
    }
}

/// Where a launched app's window goes, worked out when Enter was pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spot {
    /// On top of this tile.
    Stack(TileId),
    /// A new tile on desktop `desk` of `output`, next to `target` (at the end without one).
    /// `side` None: a column of its own.
    New { output: String, desk: u8, target: Option<TileId>, side: Option<Side>, float: bool },
}

/// An app the launcher started, until its first window shows up.
pub struct Launch {
    pub spot: Spot,
    pub started: Instant,
    /// The process started for a graphical app: its window is matched by pid too, as
    /// Chromium-based browsers overwrite their environment (`DUNE_LAUNCH` with it).
    pub pid: Option<u32>,
}

/// A launch whose window has not shown up by then is forgotten (the app opened
/// none, or an already running instance opened it).
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);

/// What a key did to the launcher.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Close,
    /// Run `command` at `dest`, next to `target` (the tile focused when it opened),
    /// in a terminal when `terminal` (a console program). `app`: the id of the app
    /// it runs, None for a typed command.
    Open { dest: Dest, target: Option<TileId>, command: String, terminal: bool, app: Option<String> },
}

struct Session {
    picker: Picker,
    stack: bool,
    /// Graphical apps (no `Terminal=true`): what they run starts without a terminal.
    gui: Vec<App>,
    place: Place,
    output: String,
    target: Option<TileId>,
}

impl Session {
    /// Where the Ctrl keys say the app goes.
    fn keys_dest(&self) -> Dest {
        Dest { place: self.place, stack: self.stack, monitor: None, desk: None }
    }

    /// The Ctrl keys' choice changed by the `@` word typed; None when that word is wrong.
    fn dest(&self) -> Option<Dest> {
        match self.picker.dest() {
            Some(word) => self.keys_dest().with(word),
            None => Some(self.keys_dest()),
        }
    }
}

pub struct Launcher {
    /// Loaded on first use: fontconfig runs a process.
    font: Option<Option<Font>>,
    family: String,
    /// Loaded on first use, from this theme (empty: the desktop's).
    icons: Option<Icons>,
    icon_theme: String,
    session: Option<Session>,
    /// The panel as last drawn, and its size; dropped whenever it changes.
    image: Option<(MemoryRenderBuffer, (i32, i32))>,
}

impl Launcher {
    pub fn new(family: &str, icon_theme: &str) -> Self {
        Launcher { font: None, family: family.to_string(), icons: None, icon_theme: icon_theme.to_string(), session: None, image: None }
    }

    pub fn is_open(&self) -> bool {
        self.session.is_some()
    }

    /// The output the panel is shown on.
    pub fn output(&self) -> Option<&str> {
        self.session.as_ref().map(|s| s.output.as_str())
    }

    /// Show the launcher on `output`; apps open next to `target`. `uses`: launches
    /// by app id, the most used are listed first.
    pub fn open(&mut self, apps: Vec<App>, uses: HashMap<String, u32>, output: String, target: Option<TileId>) {
        let home = std::env::var_os("HOME").map(Into::into).unwrap_or_else(|| "/".into());
        let gui = apps.iter().filter(|a| !a.terminal).cloned().collect();
        self.session = Some(Session {
            picker: Picker::browser(apps, home, uses),
            gui,
            place: Place::Column,
            stack: false,
            output,
            target,
        });
        self.image = None;
    }

    pub fn close(&mut self) {
        self.session = None;
        self.image = None;
    }

    /// A key pressed while open: `sym` with `mods`, typing `text`.
    pub fn feed(&mut self, mods: &ModifiersState, sym: Keysym, text: Option<char>) -> Step {
        let Some(session) = self.session.as_mut() else { return Step::Close };
        self.image = None;
        if mods.ctrl {
            let place = match sym {
                Keysym::Left => Some(Place::Side(Side::Left)),
                Keysym::Right => Some(Place::Side(Side::Right)),
                Keysym::Up => Some(Place::Side(Side::Up)),
                Keysym::Down => Some(Place::Side(Side::Down)),
                Keysym::f | Keysym::F if session.place == Place::Float => Some(Place::Column),
                Keysym::f | Keysym::F => Some(Place::Float),
                Keysym::s | Keysym::S => {
                    session.stack = !session.stack;
                    return Step::Continue;
                }
                _ => None,
            };
            if let Some(place) = place {
                session.place = place;
                return Step::Continue;
            }
        }
        let key = match sym {
            Keysym::Escape => Key::Esc,
            Keysym::Return | Keysym::KP_Enter => Key::Enter,
            Keysym::BackSpace => Key::Backspace,
            Keysym::Tab => Key::Tab,
            Keysym::ISO_Left_Tab => Key::BackTab,
            Keysym::Up => Key::Up,
            Keysym::Down => Key::Down,
            Keysym::c if mods.ctrl => Key::CtrlC,
            Keysym::u if mods.ctrl => Key::CtrlU,
            Keysym::w if mods.ctrl => Key::CtrlW,
            _ => match text {
                Some(c) if !c.is_control() && !mods.ctrl && !mods.alt && !mods.logo => Key::Char(c),
                _ => Key::Other,
            },
        };
        match session.picker.feed(key) {
            Outcome::Continue => Step::Continue,
            Outcome::Cancel => {
                self.close();
                Step::Close
            }
            Outcome::Run(line) => {
                let mut dest = session.dest().unwrap_or(session.keys_dest());
                // `app:v` / `app:h` still name the side themselves.
                if let Some(split) = line.split {
                    dest.place = Place::Side(Side::from(split));
                }
                let target = session.target;
                let terminal = !runs_gui_app(&session.gui, &line.command);
                let app = session.picker.chosen().map(str::to_string);
                self.close();
                Step::Open { dest, target, command: line.command, terminal, app }
            }
        }
    }

    /// The panel, at most `max_width` pixels wide: its pixels and size.
    pub fn image(&mut self, max_width: i32) -> Option<(&MemoryRenderBuffer, (i32, i32))> {
        let session = self.session.as_ref()?;
        let family = &self.family;
        let font = self.font.get_or_insert_with(|| {
            Font::load(family, FONT_SIZE)
                .or_else(|_| Font::load("monospace", FONT_SIZE))
                .inspect_err(|err| tracing::warn!("launcher without text: {err}"))
                .ok()
        });
        let size = panel_size(font.as_ref(), max_width);
        if self.image.as_ref().is_none_or(|(_, drawn)| *drawn != size) {
            let icons = self.icons.get_or_insert_with(|| Icons::new(&self.icon_theme));
            let pixels = draw(font.as_mut(), icons, session, size);
            let buffer = MemoryRenderBuffer::from_slice(&pixels, Fourcc::Argb8888, size, 1, Transform::Normal, None);
            self.image = Some((buffer, size));
        }
        self.image.as_ref().map(|(buffer, size)| (buffer, *size))
    }
}

impl Dune {
    /// Mod+d: show the launcher on the active output, or hide it.
    pub fn toggle_launcher(&mut self) {
        if self.launcher.is_open() {
            return self.launcher.close();
        }
        let Some(output) = self.active_output() else { return };
        self.launcher.open(desktop::apps(), desktop::launches(), output.name(), self.focused);
    }

    /// A key pressed while the launcher is open; Enter opens the app where it was asked.
    pub fn launcher_key(&mut self, mods: ModifiersState, sym: Keysym, text: Option<char>) {
        let Step::Open { dest, target, command, terminal, app } = self.launcher.feed(&mods, sym, text) else { return };
        let Some(spot) = self.spot(dest, target) else {
            return tracing::warn!("no output to launch `{command}` on");
        };
        if let Some(app) = app
            && let Err(err) = desktop::record_launch(&app)
        {
            tracing::warn!("cannot count the launch of {app}: {err}");
        }
        tracing::info!(?spot, terminal, "launching `{command}`");
        // With `follow`, the desktop it opens on is shown first, so its window maps and takes focus.
        let follow = match &spot {
            Spot::New { output, desk, .. } if self.config.follow => Some((output.clone(), *desk)),
            _ => None,
        };
        if let Some((output, desk)) = &follow
            && !terminal
        {
            self.show_desk_on(output, *desk);
        }
        self.launch(command, spot, terminal);
        // A terminal's tile is placed by `launch`; showing it afterwards brings it along.
        if let Some((output, desk)) = follow
            && terminal
        {
            self.show_desk_on(&output, desk);
        }
    }

    /// Where `dest` puts an app launched while `target` was focused.
    fn spot(&self, dest: Dest, target: Option<TileId>) -> Option<Spot> {
        let target = target.filter(|t| self.tiles.contains_key(t));
        if dest.stack && let Some(t) = target {
            return Some(Spot::Stack(t));
        }
        let here = target.and_then(|t| self.output_of(t)).or_else(|| self.active_output().map(|o| o.name()))?;
        let output = dest
            .monitor
            .and_then(|m| self.numbered_outputs().get(m as usize - 1).map(|o| o.name()))
            .unwrap_or(here.clone());
        let desk = dest.desk.unwrap_or(self.desk_of(&output));
        // Next to the focused tile when it is there, else after the last tile there.
        let target = target.filter(|_| output == here && desk == self.desk_of(&output));
        let (side, float) = match dest.place {
            Place::Column => (None, false),
            Place::Side(side) => (Some(side), false),
            Place::Float => (None, true),
        };
        Some(Spot::New { output, desk, target, side, float })
    }

    /// Start `command` for `spot`. Graphical apps start on their own, like rofi; console
    /// programs in a terminal. The process carries `DUNE_LAUNCH`, so `place_toplevel`
    /// knows where its first window goes.
    fn launch(&mut self, command: String, spot: Spot, terminal: bool) {
        let n = self.next_launch;
        self.next_launch += 1;
        self.launches.retain(|_, l| l.started.elapsed() < LAUNCH_TIMEOUT);

        if terminal {
            // The terminal belongs to its tile: a new one stacked on the target or placed now,
            // or the floating target it stacks inside of.
            let stacked = match &spot {
                Spot::Stack(t) => self.stack_tile(*t),
                Spot::New { .. } => None,
            };
            let tile = match (&spot, stacked) {
                (_, Some(id)) => id,
                (Spot::Stack(t), None) => *t,
                (Spot::New { output, desk, target, side, float }, None) => {
                    let id = self.next_tile;
                    self.next_tile += 1;
                    self.tiles.insert(id, Default::default());
                    self.place_on_desk(id, output, *desk, *target, *side, *float);
                    id
                }
            };
            let stack = matches!(spot, Spot::Stack(_)) && stacked.is_none();
            if self.start_terminal(tile, None, Some(command), stack.then_some(n)) {
                if stack {
                    self.launches.insert(n, Launch { spot, started: Instant::now(), pid: None });
                }
            } else if !stack {
                self.remove_from_layouts(tile);
                self.tiles.remove(&tile);
            }
            return;
        }

        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(format!("exec {command}"));
        cmd.env("DUNE_LAUNCH", n.to_string()).env_remove("DUNE_TILE").env_remove("DUNE_RUN");
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from).filter(|d| d.is_dir()) {
            cmd.current_dir(home);
        }
        match cmd.spawn() {
            Ok(mut child) => {
                self.launches.insert(n, Launch { spot, started: Instant::now(), pid: Some(child.id()) });
                // Reap the process when it exits so it doesn't linger as a zombie.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(err) => tracing::error!("failed to launch `{command}`: {err}"),
        }
    }
}

/// Whether `command` is what one of the `gui` apps runs (with or without arguments).
/// Anything else, typed by hand, may be a console program and gets a terminal.
fn runs_gui_app(gui: &[App], command: &str) -> bool {
    gui.iter().any(|app| {
        let base = app.command("");
        !base.is_empty() && command.strip_prefix(&base).is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
    })
}

/// Height of one line of text.
fn line_height(font: Option<&Font>) -> i32 {
    font.map_or(24, |f| f.cell_height as i32 + 6)
}

/// The input line, the suggestions, a gap, then where the app opens and the hint.
fn panel_size(font: Option<&Font>, max_width: i32) -> (i32, i32) {
    let cell = font.map_or(10, |f| f.cell_width as i32);
    let w = (COLUMNS as i32 * cell + 2 * PADDING).min(max_width).max(2 * PADDING + 1);
    let h = (MAX_ROWS as i32 + 4) * line_height(font) + 2 * PADDING;
    (w, h)
}

/// ARGB8888 pixels (B, G, R, A in memory) of the panel.
fn draw(font: Option<&mut Font>, icons: &mut Icons, session: &Session, (w, h): (i32, i32)) -> Vec<u8> {
    let mut canvas = Canvas { pixels: [BACKGROUND[2], BACKGROUND[1], BACKGROUND[0], 0xff].repeat((w * h) as usize), w, h };
    canvas.fill(0, 0, w, FRAME, FRAME_COLOR);
    canvas.fill(0, h - FRAME, w, FRAME, FRAME_COLOR);
    canvas.fill(0, 0, FRAME, h, FRAME_COLOR);
    canvas.fill(w - FRAME, 0, FRAME, h, FRAME_COLOR);
    let Some(font) = font else { return canvas.pixels };

    let line = line_height(Some(font));
    let cols = ((w - 2 * PADDING) / font.cell_width.max(1) as i32).max(1) as usize;
    let row_y = |row: i32| PADDING + row * line;

    let picker = &session.picker;
    canvas.text(font, PADDING, row_y(0), line, &format!("> {}▏", picker.input()), TEXT, cols);
    // Each suggestion: its icon in a square as tall as the line, then its name.
    let icon_size = (line - 2 * ICON_MARGIN).max(1) as u32;
    let text_x = PADDING + line + ICON_MARGIN;
    let text_cols = cols.saturating_sub(((line + ICON_MARGIN) / font.cell_width.max(1) as i32) as usize).max(1);
    let (first, rows) = picker.window();
    for (i, candidate) in rows.iter().enumerate() {
        let y = row_y(1 + i as i32);
        if first + i == picker.selected() {
            canvas.fill(FRAME, y, w - 2 * FRAME, line, SELECTED);
        }
        if let Some(icon) = candidate.icon.as_deref().and_then(|name| icons.get(name, icon_size)) {
            canvas.image(PADDING + ICON_MARGIN, y + ICON_MARGIN, icon);
        }
        canvas.text(font, text_x, y, line, &candidate.label, TEXT, text_cols);
    }
    let bottom = MAX_ROWS as i32 + 2;
    let dest = session.dest().map_or_else(|| DEST_HELP.to_string(), |d| d.label());
    canvas.text(font, PADDING, row_y(bottom), line, &dest, ACCENT, cols);
    canvas.text(font, PADDING, row_y(bottom + 1), line, HINT, DIM, cols);
    canvas.pixels
}

struct Canvas {
    pixels: Vec<u8>,
    w: i32,
    h: i32,
}

impl Canvas {
    fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, rgb: [u8; 3]) {
        for py in y.max(0)..(y + h).min(self.h) {
            for px in x.max(0)..(x + w).min(self.w) {
                let at = ((py * self.w + px) * 4) as usize;
                self.pixels[at..at + 3].copy_from_slice(&[rgb[2], rgb[1], rgb[0]]);
            }
        }
    }

    /// Blend `icon` (RGBA premultiplied) with its top-left corner at (x, y).
    fn image(&mut self, x: i32, y: i32, icon: &Icon) {
        let size = icon.size as i32;
        for iy in 0..size {
            for ix in 0..size {
                let (px, py) = (x + ix, y + iy);
                if px < 0 || py < 0 || px >= self.w || py >= self.h {
                    continue;
                }
                let src = &icon.pixels[((iy * size + ix) * 4) as usize..][..4];
                let alpha = src[3] as u32;
                if alpha == 0 {
                    continue;
                }
                let at = ((py * self.w + px) * 4) as usize;
                for (channel, value) in [(2, src[0]), (1, src[1]), (0, src[2])] {
                    let under = self.pixels[at + channel] as u32;
                    self.pixels[at + channel] = (value as u32 + under * (255 - alpha) / 255).min(255) as u8;
                }
            }
        }
    }

    /// `text` in a line of `height` pixels starting at (x, y), cut with `…` past `cols`.
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, font: &mut Font, x: i32, y: i32, height: i32, text: &str, rgb: [u8; 3], cols: usize) {
        let mut chars: Vec<char> = text.chars().collect();
        if chars.len() > cols {
            chars.truncate(cols.saturating_sub(1));
            chars.push('…');
        }
        let cell = font.cell_width as i32;
        let baseline = y + (height - font.cell_height as i32) / 2 + font.baseline;
        for (i, c) in chars.into_iter().enumerate() {
            let Some(glyph) = font.glyph(c, Style::Regular) else { continue };
            let (left, top) = (x + i as i32 * cell + glyph.left, baseline - glyph.top);
            for gy in 0..glyph.height as i32 {
                for gx in 0..glyph.width as i32 {
                    let (px, py) = (left + gx, top + gy);
                    if px < 0 || py < 0 || px >= self.w || py >= self.h {
                        continue;
                    }
                    let g = (gy * glyph.width as i32 + gx) as usize;
                    let (color, alpha) = if glyph.color {
                        let p = &glyph.data[g * 4..g * 4 + 4];
                        ([p[0], p[1], p[2]], p[3])
                    } else {
                        (rgb, glyph.data[g])
                    };
                    let at = ((py * self.w + px) * 4) as usize;
                    for (channel, value) in [(2, color[0]), (1, color[1]), (0, color[2])] {
                        let under = self.pixels[at + channel] as u32;
                        self.pixels[at + channel] = ((value as u32 * alpha as u32 + under * (255 - alpha as u32)) / 255) as u8;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launcher() -> Launcher {
        let mut launcher = Launcher::new("monospace", "hicolor");
        let apps = vec![
            App { id: "firefox".into(), name: "Firefox".into(), exec: "firefox %u".into(), icon: Some("firefox".into()), ..Default::default() },
            App { id: "nvim".into(), name: "Neovim".into(), exec: "nvim %F".into(), terminal: true, ..Default::default() },
        ];
        launcher.open(apps, HashMap::new(), "HDMI-A-1".into(), Some(3));
        launcher
    }

    fn column() -> Dest {
        Dest { place: Place::Column, stack: false, monitor: None, desk: None }
    }

    fn press(l: &mut Launcher, sym: Keysym, ctrl: bool) -> Step {
        let mods = ModifiersState { ctrl, ..Default::default() };
        l.feed(&mods, sym, sym.key_char())
    }

    fn typed(l: &mut Launcher, text: &str) {
        for c in text.chars() {
            l.feed(&ModifiersState::default(), Keysym::from_char(c), Some(c));
        }
    }

    #[test]
    fn opens_in_a_new_column_by_default() {
        let mut l = launcher();
        typed(&mut l, "fire");
        assert_eq!(
            press(&mut l, Keysym::Return, false),
            Step::Open { dest: column(), target: Some(3), command: "firefox".into(), terminal: false, app: Some("firefox".into()) }
        );
        assert!(!l.is_open());
    }

    #[test]
    fn only_console_programs_get_a_terminal() {
        let mut l = launcher();
        typed(&mut l, "nv");
        assert!(matches!(press(&mut l, Keysym::Return, false), Step::Open { terminal: true, .. }));

        let gui = [App { id: "firefox".into(), name: "Firefox".into(), exec: "firefox %u".into(), ..Default::default() }];
        assert!(runs_gui_app(&gui, "firefox"));
        assert!(runs_gui_app(&gui, "firefox https://a"));
        assert!(!runs_gui_app(&gui, "firefoxy"));
        assert!(!runs_gui_app(&gui, "htop"));
    }

    #[test]
    fn ctrl_keys_choose_the_place() {
        let mut l = launcher();
        press(&mut l, Keysym::Up, true);
        typed(&mut l, "neo");
        assert_eq!(l.session.as_ref().unwrap().picker.input(), "neo");
        assert!(matches!(press(&mut l, Keysym::Return, false), Step::Open { dest: Dest { place: Place::Side(Side::Up), .. }, .. }));

        let mut l = launcher();
        press(&mut l, Keysym::f, true);
        assert!(matches!(press(&mut l, Keysym::Return, false), Step::Open { dest: Dest { place: Place::Float, .. }, .. }));
        // Ctrl+f again goes back to a column.
        let mut l = launcher();
        press(&mut l, Keysym::f, true);
        press(&mut l, Keysym::f, true);
        assert!(matches!(press(&mut l, Keysym::Return, false), Step::Open { dest: Dest { place: Place::Column, .. }, .. }));
    }

    #[test]
    fn typing_and_erasing_keeps_working() {
        let mut l = launcher();
        typed(&mut l, "xyz");
        for _ in 0..5 {
            assert_eq!(press(&mut l, Keysym::BackSpace, false), Step::Continue);
        }
        typed(&mut l, "nv");
        assert!(matches!(press(&mut l, Keysym::Return, false), Step::Open { command, .. } if command == "nvim"));
    }

    #[test]
    fn at_words_choose_monitor_desktop_and_stack() {
        let r = column();
        assert_eq!(r.with("@3"), Some(Dest { desk: Some(3), ..r }));
        assert_eq!(r.with("@m2"), Some(Dest { monitor: Some(2), ..r }));
        assert_eq!(r.with("@m23"), Some(Dest { monitor: Some(2), desk: Some(3), ..r }));
        assert_eq!(r.with("@3m2"), r.with("@m23"));
        assert_eq!(r.with("@s"), Some(Dest { stack: true, ..r }));
        assert_eq!(r.with("@2f"), Some(Dest { place: Place::Float, desk: Some(2), ..r }));
        assert_eq!(Dest { stack: true, ..r }.with("@n"), Some(r));
        assert_eq!(r.with("@"), Some(r)); // still typing
        assert_eq!(r.with("@0"), None);
        assert_eq!(r.with("@m"), None);
        assert_eq!(r.with("@x"), None);

        let mut l = launcher();
        typed(&mut l, "fire @2");
        assert_eq!(
            press(&mut l, Keysym::Return, false),
            Step::Open {
                dest: Dest { desk: Some(2), ..r },
                target: Some(3),
                command: "firefox".into(),
                terminal: false,
                app: Some("firefox".into())
            }
        );
        // Ctrl+s stacks; `@n` undoes it.
        let mut l = launcher();
        press(&mut l, Keysym::s, true);
        typed(&mut l, "fire");
        assert!(matches!(press(&mut l, Keysym::Return, false), Step::Open { dest: Dest { stack: true, .. }, .. }));
        let mut l = launcher();
        press(&mut l, Keysym::s, true);
        typed(&mut l, "fire @n");
        assert!(matches!(press(&mut l, Keysym::Return, false), Step::Open { dest: Dest { stack: false, .. }, .. }));
    }

    #[test]
    fn typed_commands_are_not_apps() {
        let mut l = launcher();
        typed(&mut l, "true");
        assert!(matches!(press(&mut l, Keysym::Return, false), Step::Open { app: None, .. }));
    }

    #[test]
    fn icons_are_blended() {
        let mut canvas = Canvas { pixels: [0, 0, 0, 255].repeat(4), w: 2, h: 2 };
        // Opaque red, then half-transparent green (premultiplied).
        let icon = Icon { size: 1, pixels: vec![255, 0, 0, 255] };
        canvas.image(0, 0, &icon);
        canvas.image(1, 1, &Icon { size: 1, pixels: vec![0, 128, 0, 128] });
        canvas.image(5, 5, &icon); // off the canvas
        assert_eq!(&canvas.pixels[0..4], &[0, 0, 255, 255]); // B, G, R, A
        assert_eq!(&canvas.pixels[12..16], &[0, 128, 0, 255]);
    }

    #[test]
    fn escape_closes() {
        let mut l = launcher();
        assert_eq!(press(&mut l, Keysym::Escape, false), Step::Close);
        assert!(!l.is_open());
    }

    #[test]
    fn panel_is_drawn() {
        let mut l = launcher();
        let (_, (w, h)) = l.image(4000).expect("an image while open");
        assert!(w > 2 * PADDING && h > 2 * PADDING);
        let (_, (w, _)) = l.image(300).unwrap();
        assert_eq!(w, 300);
    }
}
