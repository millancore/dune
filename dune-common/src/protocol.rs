//! The control socket protocol between dune and its clients (`dune open`, the `@` layer).
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
//! - `keys\t<code>:<p|r> ...` — testing aid, honoured only with `DUNE_DEBUG=1`.

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

#[derive(Debug, PartialEq, Eq)]
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

/// The direction indicator of `tile` (cleared by asking); `None` when unset or
/// when dune does not answer in time.
pub fn take_direction(tile: TileId) -> Option<Side> {
    let mut stream = connect_and_send(&Request::Direction { tile }).ok()?;
    stream.shutdown(Shutdown::Write).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(300))).ok()?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).ok()?;
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
    fn rejects_unknown_requests() {
        assert_eq!(Request::decode("close\t-\t\t\t"), None);
        assert_eq!(Request::decode("swallow\tx\t1"), None);
    }
}
