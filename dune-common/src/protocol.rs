//! The control socket protocol between dune and its clients (`dune open`, the `@` layer,
//! dune-finder).
//!
//! A request is one message, terminated by closing the write side:
//! - `open\t<l|r|u|d|->\t<from tile or empty>\t<cwd>\t<command>` — open a terminal tile
//!   on that side of the tile (`v` and `h` are accepted for `r` and `d`); the command is
//!   last so it may contain anything, tabs and newlines included;
//! - `direction\t<tile>` — answered with the tile's direction indicator (`l|r|u|d|-`),
//!   which is then cleared;
//! - `swallow\t<tile>\t<shell pid>` — the next windows of processes started by that
//!   shell take the tile's place (`@prog`);
//! - `unswallow\t<tile>` — that `@prog` finished;
//! - `keys\t<code>:<p|r> ...` — testing aid, honoured only with `DUNE_DEBUG=1`;
//! - `windows` — answered with every window, one per line (`WindowInfo::encode_all`);
//! - `goto\t<tile>` — show that window and focus it; answered `ok`;
//! - `launch\t<dest>\t<target tile or empty>\t<slot tile or empty>\t<terminal 0|1>\t<command>`
//!   — start `command` where `dest` says, next to `target`, or into the empty container
//!   `slot`; in a terminal with `terminal` 1. Answered `ok`.

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
    /// Vertical divider: tiles side by side (`@prog:v`, like vim's `:vsplit`).
    Vertical,
    /// Horizontal divider: tiles stacked top/bottom (`@prog:h`).
    Horizontal,
}

impl Split {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "v" => Some(Split::Vertical),
            "h" => Some(Split::Horizontal),
            _ => None,
        }
    }
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
    /// `:v` opens to the right, `:h` below.
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

/// Where a launched app goes, chosen in dune-finder with Ctrl keys and the `@` word.
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
    /// `self` changed by an `@` word, its letters in any order: a digit is a desktop,
    /// `m` and a digit a monitor, `t` a tab of the current tile's container, `n` a new
    /// tile, `f` floating. `@3`, `@m2`, `@m23` (monitor 2, desktop 3), `@t`, `@2f`.
    /// None when the word is not one.
    pub fn with(mut self, word: &str) -> Option<Dest> {
        let mut chars = word.strip_prefix('@')?.chars();
        let digit = |c: Option<char>| {
            c.and_then(|c| c.to_digit(10))
                .filter(|d| *d >= 1)
                .map(|d| d as u8)
        };
        while let Some(c) = chars.next() {
            match c.to_ascii_lowercase() {
                'm' => self.monitor = Some(digit(chars.next())?),
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
        shell_pid: u32,
    },
    Unswallow {
        tile: TileId,
    },
    Direction {
        tile: TileId,
    },
    /// (xkb keycode, pressed) pairs fed in as if typed on the keyboard.
    Keys(Vec<(u32, bool)>),
    Windows,
    GoTo {
        tile: TileId,
    },
    Launch {
        dest: Dest,
        target: Option<TileId>,
        /// The empty container (Mod+Enter) it goes into.
        slot: Option<TileId>,
        terminal: bool,
        command: String,
    },
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
            Request::Swallow { tile, shell_pid } => format!("swallow\t{tile}\t{shell_pid}"),
            Request::Unswallow { tile } => format!("unswallow\t{tile}"),
            Request::Direction { tile } => format!("direction\t{tile}"),
            Request::Keys(keys) => {
                let keys: Vec<String> = keys
                    .iter()
                    .map(|(code, pressed)| format!("{code}:{}", if *pressed { "p" } else { "r" }))
                    .collect();
                format!("keys\t{}", keys.join(" "))
            }
            Request::Windows => "windows\t".into(),
            Request::GoTo { tile } => format!("goto\t{tile}"),
            Request::Launch {
                dest,
                target,
                slot,
                terminal,
                command,
            } => {
                let tile = |t: &Option<TileId>| t.map(|t| t.to_string()).unwrap_or_default();
                format!(
                    "launch\t{}\t{}\t{}\t{}\t{command}",
                    dest.encode(),
                    tile(target),
                    tile(slot),
                    *terminal as u8
                )
            }
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
                Some(Request::Swallow {
                    tile: tile.parse().ok()?,
                    shell_pid: pid.parse().ok()?,
                })
            }
            "unswallow" => Some(Request::Unswallow { tile: rest.parse().ok()? }),
            "direction" => Some(Request::Direction { tile: rest.parse().ok()? }),
            "keys" => rest
                .split_whitespace()
                .map(|k| {
                    let (code, state) = k.split_once(':')?;
                    Some((code.parse().ok()?, state == "p"))
                })
                .collect::<Option<Vec<_>>>()
                .map(Request::Keys),
            "windows" => Some(Request::Windows),
            "goto" => Some(Request::GoTo {
                tile: rest.parse().ok()?,
            }),
            "launch" => {
                let mut parts = rest.splitn(5, '\t');
                let dest = Dest::decode(parts.next()?)?;
                let target = parts.next()?.parse().ok();
                let slot = parts.next()?.parse().ok();
                let terminal = parts.next()? == "1";
                let command = parts.next()?.to_string();
                Some(Request::Launch {
                    dest,
                    target,
                    slot,
                    terminal,
                    command,
                })
            }
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

/// The direction indicator of `tile` (cleared by asking); `None` when unset or
/// when dune does not answer in time.
pub fn take_direction(tile: TileId) -> Option<Side> {
    let answer = ask(&Request::Direction { tile }, Duration::from_millis(300)).ok()?;
    Side::parse(answer.trim())
}

/// The answer to a `direction` request.
pub fn direction_answer(side: Option<Side>) -> String {
    Side::code(side).to_string()
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
            Request::Direction { tile: 4 },
            Request::Swallow { tile: 2, shell_pid: 4242 },
            Request::Unswallow { tile: 2 },
            Request::Keys(vec![(38, true), (38, false)]),
            Request::Windows,
            Request::GoTo { tile: 9 },
            Request::Launch {
                dest: Dest::default(),
                target: None,
                slot: Some(7),
                terminal: true,
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
                command: "x".into(),
            },
        ];
        for req in reqs {
            assert_eq!(Request::decode(&req.encode()), Some(req));
        }
    }

    #[test]
    fn sides() {
        assert_eq!(Side::from(Split::Vertical), Side::Right);
        assert_eq!(Side::parse("h"), Some(Side::Down)); // old `open` messages
        assert!(Side::Up.before() && !Side::Down.before());
        assert_eq!(Side::Left.split(), Split::Vertical);
        assert_eq!(direction_answer(None), "-");
        assert_eq!(Side::parse(&direction_answer(Some(Side::Up))), Some(Side::Up));
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
    }

    #[test]
    fn at_words_choose_monitor_desktop_and_tab() {
        let r = Dest::default();
        assert_eq!(r.with("@3"), Some(Dest { desk: Some(3), ..r }));
        assert_eq!(
            r.with("@m2"),
            Some(Dest {
                monitor: Some(2),
                ..r
            })
        );
        assert_eq!(
            r.with("@m23"),
            Some(Dest {
                monitor: Some(2),
                desk: Some(3),
                ..r
            })
        );
        assert_eq!(r.with("@3m2"), r.with("@m23"));
        assert_eq!(
            r.with("@2f"),
            Some(Dest {
                place: Place::Float,
                desk: Some(2),
                ..r
            })
        );
        assert_eq!(r.with("@t"), Some(Dest { tab: true, ..r }));
        assert_eq!(r.with("@s"), None); // stacking is not chosen here
        assert_eq!(Dest { tab: true, ..r }.with("@n"), Some(r));
        assert_eq!(r.with("@"), Some(r)); // still typing
        assert_eq!(r.with("@0"), None);
        assert_eq!(r.with("@m"), None);
        assert_eq!(r.with("@x"), None);
        assert_eq!(
            Dest { desk: Some(2), ..r }.label(),
            "opens in a new column · desktop 2"
        );
    }

    #[test]
    fn rejects_unknown_requests() {
        assert_eq!(Request::decode("close\t-\t\t\t"), None);
        assert_eq!(Request::decode("swallow\tx\t1"), None);
        assert_eq!(Request::decode("launch\tz,0,,\t\t\t0\tx"), None);
        assert_eq!(Request::decode("goto\t"), None);
    }
}
