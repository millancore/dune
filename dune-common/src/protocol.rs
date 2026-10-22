//! The control socket protocol between dune and its clients (`dune open`, the shell
//! wrapper, dune-finder).
//!
//! A request is one message, terminated by closing the write side:
//! - `open\t<l|r|u|d|->\t<from tile or empty>\t<cwd>\t<command>` — open a terminal tile
//!   on that side of the tile (`v` and `h` are accepted for `r` and `d`); the command is
//!   last so it may contain anything, tabs and newlines included;
//! - `swallow\t<tile>\t<pid>` — the windows of that process and its children go to the tile,
//!   on top of what it shows, until the process is gone (`dune-open`); answered `ok`;
//! - `keys\t<code>:<p|r> ...` — testing aid, honoured only with `DUNE_DEBUG=1`;
//! - `windows` — answered with every window, one per line (`WindowInfo::encode_all`),
//!   but those on locked desktops;
//! - `locked_windows` — answered with the windows of locked desktops only, the same way;
//! - `goto\t<tile>` — show that window and focus it (not on a locked desktop); answered `ok`;
//! - `unlock\t<tile>` — unlock the desktop of that window, then go to it; answered `ok`;
//! - `launch\t<dest>\t<target tile or empty>\t<slot tile or empty>\t<terminal 0|1>\t<cwd>\t<command>`
//!   — start `command` where `dest` says, next to `target`, or into the empty container
//!   `slot`; in a terminal with `terminal` 1, started in `cwd` when given. Answered `ok`;
//! - `tile_options\t<tile or empty>` — answered with the rows of the tile options menu for
//!   that tile, one per line (`TileOption::encode_all`);
//! - `tile_action\t<tile or empty>\t<action>` — do `action` (`TileAction::code`) on that
//!   tile; nothing without one. Answered `ok`;
//! - `quit\t` — end the session (dune-finder's Log out); answered `ok`;
//! - `scratchpad_show\t` — what the `scratchpad` key does (`dune scratchpad show`); answered `ok`;
//! - `watch_scratchpad\t` — the connection stays open: dune writes a waybar JSON line with the
//!   scratchpad's windows now and on every change (`dune scratchpad watch`);
//! - `recording\tstarted|ended` — the recording's shell: wf-recorder started / ended;
//! - `recording_stop\t` — stop the recording running (`dune recording stop`); answered `ok`;
//! - `watch_recording\t` — the connection stays open: dune writes `on SECONDS` (when the
//!   recording started, since the epoch) or `off` now and on every change.

use std::{
    io::{Read, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

pub type TileId = u32;

/// How a tile is divided when a new tile is opened next to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    /// Vertical divider: tiles side by side.
    Vertical,
    /// Horizontal divider: tiles stacked top/bottom.
    Horizontal,
}

/// A side of a tile: where a new tile goes, or where focus moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
    Up,
    Down,
}

impl Side {
    /// How the tile is divided to put a new one on this side.
    pub fn split(self) -> Split {
        match self {
            Side::Left | Side::Right => Split::Vertical,
            Side::Up | Side::Down => Split::Horizontal,
        }
    }

    /// The new tile goes before (left of / above) the existing one.
    pub fn before(self) -> bool {
        matches!(self, Side::Left | Side::Up)
    }

    fn code(side: Option<Side>) -> &'static str {
        match side {
            Some(Side::Left) => "l",
            Some(Side::Right) => "r",
            Some(Side::Up) => "u",
            Some(Side::Down) => "d",
            None => "-",
        }
    }

    pub fn parse(s: &str) -> Option<Side> {
        match s {
            "l" => Some(Side::Left),
            "r" | "v" => Some(Side::Right),
            "u" => Some(Side::Up),
            "d" | "h" => Some(Side::Down),
            _ => None,
        }
    }
}

impl From<Split> for Side {
    /// Side by side opens to the right, stacked below.
    fn from(split: Split) -> Side {
        match split {
            Split::Vertical => Side::Right,
            Split::Horizontal => Side::Down,
        }
    }
}

/// Where an app started by dune-finder goes.
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

    fn code(self) -> &'static str {
        match self {
            Place::Column => "c",
            Place::Side(side) => Side::code(Some(side)),
            Place::Float => "f",
        }
    }

    fn parse(s: &str) -> Option<Place> {
        match s {
            "c" => Some(Place::Column),
            "f" => Some(Place::Float),
            s => Side::parse(s).map(Place::Side),
        }
    }
}

/// Where a launched app goes, chosen in dune-finder with Ctrl keys and the `:` word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dest {
    pub place: Place,
    /// A tab of the focused tile's container.
    pub tab: bool,
    /// Monitor number, left to right, from 1.
    pub monitor: Option<u8>,
    pub desk: Option<u8>,
}

impl Default for Dest {
    fn default() -> Self {
        Dest {
            place: Place::Column,
            tab: false,
            monitor: None,
            desk: None,
        }
    }
}

impl Dest {
    /// `self` changed by a `:` word, its letters in any order: `d` and a digit (or a digit
    /// alone) is a desktop, `m` and a digit a monitor, `t` a tab of the current tile's
    /// container, `n` a new tile, `f` floating. `:d3`, `:3`, `:m2`, `:m2d3` (monitor 2,
    /// desktop 3), `:t`, `:d2f`. None when the word is not one.
    pub fn with(mut self, word: &str) -> Option<Dest> {
        let mut chars = word.strip_prefix(':')?.chars();
        let digit = |c: Option<char>| {
            c.and_then(|c| c.to_digit(10))
                .filter(|d| *d >= 1)
                .map(|d| d as u8)
        };
        while let Some(c) = chars.next() {
            match c.to_ascii_lowercase() {
                'm' => self.monitor = Some(digit(chars.next())?),
                'd' => self.desk = Some(digit(chars.next())?),
                't' => self.tab = true,
                'n' => self.tab = false,
                'f' => self.place = Place::Float,
                c => self.desk = Some(digit(Some(c))?),
            }
        }
        Some(self)
    }

    pub fn label(&self) -> String {
        if self.tab {
            return "opens as a tab of this tile".into();
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

    /// `<place>,<tab 0|1>,<monitor>,<desk>`, the numbers empty when unset.
    fn encode(&self) -> String {
        let num = |n: Option<u8>| n.map(|n| n.to_string()).unwrap_or_default();
        format!(
            "{},{},{},{}",
            self.place.code(),
            self.tab as u8,
            num(self.monitor),
            num(self.desk)
        )
    }

    fn decode(s: &str) -> Option<Dest> {
        let mut parts = s.split(',');
        let place = Place::parse(parts.next()?)?;
        let tab = parts.next()? == "1";
        let num = |s: &str| {
            if s.is_empty() {
                Some(None)
            } else {
                s.parse().ok().map(Some)
            }
        };
        let monitor = num(parts.next()?)?;
        let desk = num(parts.next()?)?;
        Some(Dest {
            place,
            tab,
            monitor,
            desk,
        })
    }
}

/// A window, as dune answers a `windows` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub tile: TileId,
    pub title: String,
    pub app: String,
    /// Where it is: `desk 2`, `desk 2 · m1` with more than one monitor.
    pub place: String,
}

impl WindowInfo {
    /// The desktop in `place`.
    pub fn desk(&self) -> Option<u8> {
        self.place.strip_prefix("desk ")?.split(' ').next()?.parse().ok()
    }

    /// The monitor in `place`: 1 when it names none (there is one).
    pub fn monitor(&self) -> u8 {
        self.place
            .rsplit_once(" · m")
            .and_then(|(_, m)| m.parse().ok())
            .unwrap_or(1)
    }

    /// The answer to `windows`: one window per line, its fields separated by tabs (tabs
    /// and line breaks inside them become spaces).
    pub fn encode_all(windows: &[WindowInfo]) -> String {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        windows
            .iter()
            .map(|w| {
                format!(
                    "{}\t{}\t{}\t{}\n",
                    w.tile,
                    clean(&w.title),
                    clean(&w.app),
                    clean(&w.place)
                )
            })
            .collect()
    }

    pub fn decode_all(answer: &str) -> Vec<WindowInfo> {
        answer
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(4, '\t');
                Some(WindowInfo {
                    tile: parts.next()?.parse().ok()?,
                    title: parts.next()?.to_string(),
                    app: parts.next()?.to_string(),
                    place: parts.next()?.to_string(),
                })
            })
            .collect()
    }
}

/// What the tile options menu (Mod+/) does to its tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TileAction {
    /// Block the tile on screen and in captures (again: show it).
    PrivateTile,
    Float,
    Center,
    FullWidth,
    /// Out of its split into a tab of its own (`stack_to_tabs`).
    ToTab,
    SendToDesk(u8),
    /// Off every desktop, into the scratchpad.
    Scratchpad,
    Close,
}

impl TileAction {
    /// Every action, in the menu's order.
    pub fn all() -> Vec<TileAction> {
        use TileAction::*;
        let mut all = vec![PrivateTile, Float, Center, FullWidth, ToTab];
        all.extend((1..=9).map(SendToDesk));
        all.extend([Scratchpad, Close]);
        all
    }

    /// Its name in the protocol: the name of its key in dune's config.
    pub fn code(self) -> String {
        match self {
            TileAction::PrivateTile => "private_tile".into(),
            TileAction::Float => "float".into(),
            TileAction::Center => "center".into(),
            TileAction::FullWidth => "full_width".into(),
            TileAction::ToTab => "stack_to_tabs".into(),
            TileAction::SendToDesk(n) => format!("send_to_desk_{n}"),
            TileAction::Scratchpad => "send_to_scratchpad".into(),
            TileAction::Close => "close".into(),
        }
    }

    pub fn parse(code: &str) -> Option<TileAction> {
        TileAction::all().into_iter().find(|a| a.code() == code)
    }

    /// What the menu calls it.
    pub fn label(self) -> String {
        match self {
            TileAction::PrivateTile => "Private tile".into(),
            TileAction::Float => "Float".into(),
            TileAction::Center => "Center".into(),
            TileAction::FullWidth => "Full width".into(),
            TileAction::ToTab => "To tab".into(),
            TileAction::SendToDesk(n) => format!("Send to desktop {n}"),
            TileAction::Scratchpad => "Send to scratchpad".into(),
            TileAction::Close => "Close".into(),
        }
    }
}

/// A row of the tile options menu, as dune answers a `tile_options` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileOption {
    pub action: TileAction,
    /// Whether a switch (private tile) is on; `None` for the rest.
    pub on: Option<bool>,
    /// The keys that do it too, as the config writes them (`super+f`); empty: none.
    pub keys: String,
}

impl TileOption {
    /// The answer to `tile_options`: `<action>\t<on 0|1|->\t<keys>` per line.
    pub fn encode_all(options: &[TileOption]) -> String {
        options
            .iter()
            .map(|o| {
                let on = match o.on {
                    Some(true) => "1",
                    Some(false) => "0",
                    None => "-",
                };
                format!("{}\t{on}\t{}\n", o.action.code(), o.keys.replace(['\t', '\n', '\r'], " "))
            })
            .collect()
    }

    pub fn decode_all(answer: &str) -> Vec<TileOption> {
        answer
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(3, '\t');
                let action = TileAction::parse(parts.next()?)?;
                let on = match parts.next()? {
                    "1" => Some(true),
                    "0" => Some(false),
                    _ => None,
                };
                Some(TileOption { action, on, keys: parts.next()?.to_string() })
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Open {
        side: Option<Side>,
        from: Option<TileId>,
        cwd: Option<PathBuf>,
        command: String,
    },
    Swallow {
        tile: TileId,
        pid: u32,
    },
    /// (xkb keycode, pressed) pairs fed in as if typed on the keyboard.
    Keys(Vec<(u32, bool)>),
    Windows,
    /// The windows of locked desktops, which `Windows` leaves out.
    LockedWindows,
    GoTo {
        tile: TileId,
    },
    /// Unlock the desktop `tile` is on and go to it.
    Unlock {
        tile: TileId,
    },
    Launch {
        dest: Dest,
        target: Option<TileId>,
        /// The empty container (Mod+Enter) it goes into.
        slot: Option<TileId>,
        terminal: bool,
        /// The folder a terminal starts in (the jump mode's).
        cwd: Option<PathBuf>,
        command: String,
    },
    /// The rows of the tile options menu for `tile` (none focused: none).
    TileOptions {
        tile: Option<TileId>,
    },
    /// A row chosen in the tile options menu.
    TileAction {
        tile: Option<TileId>,
        action: TileAction,
    },
    /// End the session.
    Quit,
    /// Hide the shown scratchpad window focused, or show the next one.
    ScratchpadShow,
    /// Keep the connection, writing the scratchpad's state on every change.
    WatchScratchpad,
    /// The recording's shell: wf-recorder started (`true`) or ended; `area`: the rectangle
    /// it records (`x, y, w, h`), when not a whole monitor.
    Recording { started: bool, area: Option<(i32, i32, i32, i32)> },
    /// Stop the recording running.
    RecordingStop,
    /// Keep the connection, writing whether a recording runs on every change.
    WatchRecording,
}

impl Request {
    pub fn encode(&self) -> String {
        match self {
            Request::Open { side, from, cwd, command } => {
                let split = Side::code(*side);
                let from = from.map(|t| t.to_string()).unwrap_or_default();
                let cwd = cwd.as_deref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                format!("open\t{split}\t{from}\t{cwd}\t{command}")
            }
            Request::Swallow { tile, pid } => format!("swallow\t{tile}\t{pid}"),
            Request::Keys(keys) => {
                let keys: Vec<String> = keys
                    .iter()
                    .map(|(code, pressed)| format!("{code}:{}", if *pressed { "p" } else { "r" }))
                    .collect();
                format!("keys\t{}", keys.join(" "))
            }
            Request::Windows => "windows\t".into(),
            Request::LockedWindows => "locked_windows\t".into(),
            Request::GoTo { tile } => format!("goto\t{tile}"),
            Request::Unlock { tile } => format!("unlock\t{tile}"),
            Request::Launch {
                dest,
                target,
                slot,
                terminal,
                cwd,
                command,
            } => {
                let tile = |t: &Option<TileId>| t.map(|t| t.to_string()).unwrap_or_default();
                let cwd = cwd.as_deref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                format!(
                    "launch\t{}\t{}\t{}\t{}\t{cwd}\t{command}",
                    dest.encode(),
                    tile(target),
                    tile(slot),
                    *terminal as u8
                )
            }
            Request::TileOptions { tile } => format!("tile_options\t{}", tile.map(|t| t.to_string()).unwrap_or_default()),
            Request::TileAction { tile, action } => {
                format!("tile_action\t{}\t{}", tile.map(|t| t.to_string()).unwrap_or_default(), action.code())
            }
            Request::Quit => "quit\t".into(),
            Request::ScratchpadShow => "scratchpad_show\t".into(),
            Request::WatchScratchpad => "watch_scratchpad\t".into(),
            Request::Recording { started, area } => {
                let area = area.map(|(x, y, w, h)| format!("\t{x},{y} {w}x{h}")).unwrap_or_default();
                format!("recording\t{}{area}", if *started { "started" } else { "ended" })
            }
            Request::RecordingStop => "recording_stop\t".into(),
            Request::WatchRecording => "watch_recording\t".into(),
        }
    }

    pub fn decode(msg: &str) -> Option<Self> {
        let (kind, rest) = msg.split_once('\t')?;
        match kind {
            "open" => {
                let mut parts = rest.splitn(4, '\t');
                let side = Side::parse(parts.next()?);
                let from = parts.next()?.parse().ok();
                let cwd = Some(parts.next()?).filter(|c| !c.is_empty()).map(PathBuf::from);
                let command = parts.next()?.to_string();
                Some(Request::Open { side, from, cwd, command })
            }
            "swallow" => {
                let (tile, pid) = rest.split_once('\t')?;
                Some(Request::Swallow { tile: tile.parse().ok()?, pid: pid.parse().ok()? })
            }
            "keys" => rest
                .split_whitespace()
                .map(|k| {
                    let (code, state) = k.split_once(':')?;
                    Some((code.parse().ok()?, state == "p"))
                })
                .collect::<Option<Vec<_>>>()
                .map(Request::Keys),
            "windows" => Some(Request::Windows),
            "locked_windows" => Some(Request::LockedWindows),
            "unlock" => Some(Request::Unlock { tile: rest.parse().ok()? }),
            "goto" => Some(Request::GoTo {
                tile: rest.parse().ok()?,
            }),
            "launch" => {
                let mut parts = rest.splitn(6, '\t');
                let dest = Dest::decode(parts.next()?)?;
                let target = parts.next()?.parse().ok();
                let slot = parts.next()?.parse().ok();
                let terminal = parts.next()? == "1";
                let cwd = Some(parts.next()?).filter(|c| !c.is_empty()).map(PathBuf::from);
                let command = parts.next()?.to_string();
                Some(Request::Launch {
                    dest,
                    target,
                    slot,
                    terminal,
                    cwd,
                    command,
                })
            }
            "tile_options" => Some(Request::TileOptions { tile: rest.parse().ok() }),
            "tile_action" => {
                let (tile, action) = rest.split_once('\t')?;
                Some(Request::TileAction { tile: tile.parse().ok(), action: TileAction::parse(action)? })
            }
            "quit" => Some(Request::Quit),
            "scratchpad_show" => Some(Request::ScratchpadShow),
            "watch_scratchpad" => Some(Request::WatchScratchpad),
            "recording" => {
                let (state, area) = rest.split_once('\t').unwrap_or((rest, ""));
                let started = match state {
                    "started" => true,
                    "ended" => false,
                    _ => return None,
                };
                Some(Request::Recording { started, area: parse_geometry(area) })
            }
            "recording_stop" => Some(Request::RecordingStop),
            "watch_recording" => Some(Request::WatchRecording),
            _ => None,
        }
    }
}

/// The control socket of the compositor named by `wayland_socket`.
pub fn socket_path(wayland_socket: &str) -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    runtime.join(format!("dune-{wayland_socket}.sock"))
}

fn connect_and_send(req: &Request) -> Result<UnixStream, String> {
    let path = std::env::var_os("DUNE_SOCKET").ok_or("DUNE_SOCKET is not set: not running inside dune?")?;
    let mut stream = UnixStream::connect(&path).map_err(|e| format!("cannot connect to {path:?}: {e}"))?;
    stream.write_all(req.encode().as_bytes()).map_err(|e| e.to_string())?;
    Ok(stream)
}

/// Send a request to the compositor named by `$DUNE_SOCKET`.
pub fn send(req: &Request) -> Result<(), String> {
    connect_and_send(req).map(|_| ())
}

/// Send a request whose answer does not end (`watch_scratchpad`): the stream to read it from.
pub fn watch(req: &Request) -> Result<UnixStream, String> {
    let stream = connect_and_send(req)?;
    stream.shutdown(Shutdown::Write).map_err(|e| e.to_string())?;
    Ok(stream)
}

/// Send a request and wait up to `timeout` for dune's answer.
pub fn ask(req: &Request, timeout: Duration) -> Result<String, String> {
    let mut stream = connect_and_send(req)?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .map_err(|e| e.to_string())?;
    Ok(answer)
}

/// A geometry as slurp, grim and wf-recorder write it: `X,Y WxH`, as `(x, y, w, h)`.
pub fn parse_geometry(text: &str) -> Option<(i32, i32, i32, i32)> {
    let (at, size) = text.trim().split_once(' ')?;
    let (x, y) = at.split_once(',')?;
    let (w, h) = size.split_once('x')?;
    Some((x.parse().ok()?, y.parse().ok()?, w.parse().ok()?, h.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip() {
        let reqs = [
            Request::Open {
                side: Some(Side::Left),
                from: Some(3),
                cwd: Some(PathBuf::from("/srv/app")),
                command: "vim index.php\tweird\nname".into(),
            },
            Request::Open { side: None, from: None, cwd: None, command: String::new() },
            Request::Swallow { tile: 2, pid: 4242 },
            Request::Keys(vec![(38, true), (38, false)]),
            Request::Windows,
            Request::GoTo { tile: 9 },
            Request::LockedWindows,
            Request::Unlock { tile: 9 },
            Request::Launch {
                dest: Dest::default(),
                target: None,
                slot: Some(7),
                terminal: true,
                cwd: Some(PathBuf::from("/srv/a b")),
                command: String::new(),
            },
            Request::Launch {
                dest: Dest {
                    place: Place::Side(Side::Up),
                    tab: true,
                    monitor: Some(2),
                    desk: Some(3),
                },
                target: Some(3),
                slot: None,
                terminal: false,
                cwd: None,
                command: "firefox --new-window\thttps://a".into(),
            },
            Request::Launch {
                dest: Dest {
                    place: Place::Float,
                    ..Dest::default()
                },
                target: Some(1),
                slot: None,
                terminal: false,
                cwd: None,
                command: "x".into(),
            },
            Request::TileOptions { tile: Some(4) },
            Request::TileOptions { tile: None },
            Request::TileAction { tile: Some(4), action: TileAction::PrivateTile },
            Request::TileAction { tile: None, action: TileAction::Close },
            Request::TileAction { tile: Some(2), action: TileAction::SendToDesk(9) },
            Request::TileAction { tile: Some(2), action: TileAction::Scratchpad },
            Request::Quit,
            Request::ScratchpadShow,
            Request::WatchScratchpad,
            Request::Recording { started: true, area: None },
            Request::Recording { started: true, area: Some((-10, 20, 300, 40)) },
            Request::Recording { started: false, area: None },
            Request::RecordingStop,
            Request::WatchRecording,
        ];
        for req in reqs {
            assert_eq!(Request::decode(&req.encode()), Some(req));
        }
    }

    #[test]
    fn geometries_as_slurp_writes_them() {
        assert_eq!(parse_geometry("10,-20 300x40"), Some((10, -20, 300, 40)));
        assert_eq!(parse_geometry(" 0,0 1x1\n"), Some((0, 0, 1, 1)));
        assert_eq!(parse_geometry(""), None);
        assert_eq!(parse_geometry("10,20"), None);
    }

    #[test]
    fn sides() {
        assert_eq!(Side::from(Split::Vertical), Side::Right);
        assert_eq!(Side::parse("h"), Some(Side::Down)); // old `open` messages
        assert!(Side::Up.before() && !Side::Down.before());
        assert_eq!(Side::Left.split(), Split::Vertical);
    }

    #[test]
    fn windows_answer_roundtrip() {
        let windows = vec![
            WindowInfo {
                tile: 1,
                title: "a\tb\nc".into(),
                app: "foot".into(),
                place: "desk 1".into(),
            },
            WindowInfo {
                tile: 2,
                title: String::new(),
                app: String::new(),
                place: "desk 2 · m2".into(),
            },
        ];
        let decoded = WindowInfo::decode_all(&WindowInfo::encode_all(&windows));
        assert_eq!(decoded[0].title, "a b c");
        assert_eq!(decoded[1], windows[1]);
        assert_eq!(WindowInfo::decode_all(""), Vec::new());
        assert_eq!((windows[0].desk(), windows[0].monitor()), (Some(1), 1));
        assert_eq!((windows[1].desk(), windows[1].monitor()), (Some(2), 2));
    }

    #[test]
    fn colon_words_choose_monitor_desktop_and_tab() {
        let r = Dest::default();
        assert_eq!(r.with(":3"), Some(Dest { desk: Some(3), ..r }));
        assert_eq!(
            r.with(":m2"),
            Some(Dest {
                monitor: Some(2),
                ..r
            })
        );
        assert_eq!(
            r.with(":m23"),
            Some(Dest {
                monitor: Some(2),
                desk: Some(3),
                ..r
            })
        );
        assert_eq!(r.with(":3m2"), r.with(":m23"));
        assert_eq!(r.with(":d3"), r.with(":3"));
        assert_eq!(r.with(":m2d3"), r.with(":m23"));
        assert_eq!(r.with("@3"), None);
        assert_eq!(
            r.with(":2f"),
            Some(Dest {
                place: Place::Float,
                desk: Some(2),
                ..r
            })
        );
        assert_eq!(r.with(":t"), Some(Dest { tab: true, ..r }));
        assert_eq!(r.with(":s"), None); // stacking is not chosen here
        assert_eq!(Dest { tab: true, ..r }.with(":n"), Some(r));
        assert_eq!(r.with(":"), Some(r)); // still typing
        assert_eq!(r.with(":0"), None);
        assert_eq!(r.with(":m"), None);
        assert_eq!(r.with(":d"), None);
        assert_eq!(r.with(":d0"), None);
        assert_eq!(r.with(":x"), None);
        assert_eq!(
            Dest { desk: Some(2), ..r }.label(),
            "opens in a new column · desktop 2"
        );
    }

    #[test]
    fn rejects_unknown_requests() {
        assert_eq!(Request::decode("close\t-\t\t\t"), None);
        assert_eq!(Request::decode("swallow\tx\t1"), None);
        assert_eq!(Request::decode("unswallow\t2"), None);
        assert_eq!(Request::decode("launch\tz,0,,\t\t\t0\tx"), None);
        assert_eq!(Request::decode("goto\t"), None);
        assert_eq!(Request::decode("unlock\t"), None);
        assert_eq!(Request::decode("tile_action\t3\tprivate_desktop"), None);
        assert_eq!(Request::decode("tile_action\t3\tsend_to_desk_0"), None);
        assert_eq!(Request::decode("tile_action\t3"), None);
    }

    #[test]
    fn tile_actions_have_their_config_names() {
        let all = TileAction::all();
        assert_eq!(all.len(), 16);
        for action in all {
            assert_eq!(TileAction::parse(&action.code()), Some(action));
        }
        assert_eq!(TileAction::ToTab.code(), "stack_to_tabs");
        assert_eq!(TileAction::SendToDesk(3).label(), "Send to desktop 3");
        assert_eq!(TileAction::Scratchpad.code(), "send_to_scratchpad");
        assert_eq!(TileAction::parse("bogus"), None);
    }

    #[test]
    fn tile_options_answer_roundtrip() {
        let options = vec![
            TileOption { action: TileAction::PrivateTile, on: Some(true), keys: String::new() },
            TileOption { action: TileAction::Float, on: None, keys: "super+p".into() },
            TileOption { action: TileAction::Close, on: None, keys: "super+q, super+apostrophe".into() },
        ];
        assert_eq!(TileOption::decode_all(&TileOption::encode_all(&options)), options);
        assert_eq!(TileOption::decode_all("nonsense\n"), Vec::new());
    }
}
