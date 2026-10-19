//! The `:` layer: what happens between the keyboard and the shell of a terminal tile.
//!
//! `:` typed at an empty prompt opens the picker; everything else goes to the shell.
//! "At the prompt" means the shell itself is the foreground process group of its
//! terminal (no program running) — the caller checks that — and nothing was typed
//! since the last Enter, which this layer tracks.
//!
//! The layer only decides; the caller does the I/O: it writes the returned bytes to
//! the shell and draws `picker()` however it can (escape codes in the PTY wrapper,
//! natively in dune-term).

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::{
    desktop::App,
    picker::{self, Line, Outcome, Picker},
    protocol::{Request, Side, TileId},
};

/// Quiet time after the shell's first prompt before a `DUNE_RUN` command is typed
/// (startup files may run programs before that).
const SETTLE: Duration = Duration::from_millis(300);

pub struct AtLayer {
    tile: Option<TileId>,
    shell_pid: u32,
    send: Box<dyn Fn(&Request) -> bool>,
    /// The applications `:` offers (read when the picker opens, so new ones show up).
    apps: Box<dyn Fn() -> Vec<App>>,
    /// Takes (reads and clears) the tile's direction indicator, kept by dune.
    take_direction: Box<dyn Fn() -> Option<Side>>,
    /// Something was typed at the prompt since the last Enter.
    typed: bool,
    /// The shell was at its prompt the last time we looked.
    was_idle: bool,
    /// A `:prog` was started and the compositor is waiting for its window.
    swallowing: bool,
    picker: Option<Picker>,
    /// Command to run once the shell is ready (tiles opened with `:prog:v`).
    run_first: Option<String>,
    last_output: Option<Instant>,
}

impl AtLayer {
    /// For a shell started inside dune: tile and first command from the environment
    /// (`DUNE_TILE`, `DUNE_RUN`), requests sent to `$DUNE_SOCKET`.
    pub fn from_env(shell_pid: u32) -> Self {
        Self::for_tile(shell_pid, std::env::var("DUNE_RUN").ok())
    }

    /// Like `from_env`, with the first command given (callers that must remove
    /// `DUNE_RUN` from the environment before starting the shell).
    pub fn for_tile(shell_pid: u32, run_first: Option<String>) -> Self {
        let tile = std::env::var("DUNE_TILE").ok().and_then(|t| t.parse().ok());
        let send = Box::new(|req: &Request| crate::protocol::send(req).is_ok());
        let take_direction = Box::new(move || tile.and_then(crate::protocol::take_direction));
        Self::new(tile, shell_pid, run_first, send, Box::new(crate::desktop::apps), take_direction)
    }

    pub fn new(
        tile: Option<TileId>,
        shell_pid: u32,
        run_first: Option<String>,
        send: Box<dyn Fn(&Request) -> bool>,
        apps: Box<dyn Fn() -> Vec<App>>,
        take_direction: Box<dyn Fn() -> Option<Side>>,
    ) -> Self {
        AtLayer {
            tile,
            shell_pid,
            send,
            apps,
            take_direction,
            typed: false,
            was_idle: true,
            swallowing: false,
            picker: None,
            run_first: run_first.filter(|r| !r.trim().is_empty()),
            last_output: None,
        }
    }

    /// Whether the `:` layer applies: only inside dune.
    pub fn enabled() -> bool {
        std::env::var_os("DUNE_SOCKET").is_some()
    }

    pub fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
    }

    /// The shell printed something.
    pub fn output_seen(&mut self) {
        self.last_output = Some(Instant::now());
    }

    /// Whether `tick` should be called soon (a `DUNE_RUN` command is waiting).
    pub fn waiting(&self) -> bool {
        self.run_first.is_some()
    }

    /// Call regularly with whether the shell is at its prompt; returns bytes to send
    /// to the shell (the `DUNE_RUN` command once startup is over).
    pub fn tick(&mut self, at_prompt: bool) -> Vec<u8> {
        if at_prompt && !self.was_idle {
            // A program handed the terminal back to the shell.
            self.typed = false;
            self.end_swallow();
        }
        self.was_idle = at_prompt;

        if self.run_first.is_some() && at_prompt && self.last_output.is_some_and(|t| t.elapsed() >= SETTLE) {
            let command = self.run_first.take().unwrap();
            return self.run_in_place(&command);
        }
        Vec::new()
    }

    /// Nothing was typed at the prompt since the last Enter: the next key starts a
    /// new command line.
    pub fn line_is_empty(&self) -> bool {
        !self.typed && self.picker.is_none()
    }

    /// A `cmd:v args` / `cmd:h args` line typed at the prompt (see
    /// `picker::parse_split_command`): ask the compositor for the tile. The caller
    /// clears the shell's line instead of sending Enter.
    pub fn split_command(&mut self, line: Line) {
        self.typed = false;
        self.run_line(line);
    }

    /// Enter on a typed command line: the side its tile's direction indicator
    /// points to, if any (taking it clears it).
    pub fn take_direction(&self) -> Option<Side> {
        (self.take_direction)()
    }

    /// Open `command` in a new tile on `side` of this one (the caller clears the
    /// shell's line instead of sending Enter).
    pub fn open_beside(&mut self, side: Side, command: String) {
        self.typed = false;
        self.open(side, command);
    }

    /// Input typed by the user (already encoded as terminal bytes); returns what goes
    /// to the shell. May open, drive or close the picker.
    pub fn input(&mut self, bytes: &[u8], at_prompt: bool) -> Vec<u8> {
        if self.picker.is_some() {
            return self.feed_picker(bytes);
        }
        if !at_prompt {
            self.typed = false;
            return bytes.to_vec();
        }

        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == picker::TRIGGER as u8 && !self.typed {
                let mut out = bytes[..i].to_vec();
                self.picker = Some(Picker::new((self.apps)(), self.shell_cwd()));
                out.extend(self.feed_picker(&bytes[i + 1..]));
                return out;
            }
            if b == 0x1b {
                // Replies from the terminal (cursor reports, focus events, OSC/DCS
                // answers) are not typing; arrow keys and the like are.
                let len = escape_len(&bytes[i..]);
                if !is_terminal_reply(&bytes[i..i + len]) {
                    self.typed = true;
                }
                i += len;
                continue;
            }
            match b {
                b'\r' | b'\n' => {
                    self.typed = false;
                    // A builtin started with `:` (no program ran) ends here.
                    self.end_swallow();
                }
                0x03 | 0x15 => self.typed = false, // Ctrl-C / Ctrl-U clear the line
                _ => self.typed = true,
            }
            i += 1;
        }
        bytes.to_vec()
    }

    fn feed_picker(&mut self, bytes: &[u8]) -> Vec<u8> {
        for key in picker::parse_keys(bytes) {
            let Some(picker) = self.picker.as_mut() else { break };
            match picker.feed(key) {
                Outcome::Continue => {}
                Outcome::Cancel => {
                    self.picker = None;
                }
                Outcome::Run(line) => {
                    self.picker = None;
                    return self.run_line(line);
                }
            }
        }
        Vec::new()
    }

    /// Run a finished `:` line: on the side it names (`:v`/`:h`), else on the side the
    /// direction indicator points to, else in place.
    fn run_line(&mut self, line: Line) -> Vec<u8> {
        match line.split.map(Side::from).or_else(|| (self.take_direction)()) {
            Some(side) => {
                self.open(side, line.command);
                Vec::new()
            }
            None => self.run_in_place(&line.command),
        }
    }

    fn open(&mut self, side: Side, command: String) {
        (self.send)(&Request::Open {
            side: Some(side),
            from: self.tile,
            cwd: Some(self.shell_cwd()),
            command,
        });
    }

    /// The command typed into the shell, as if by the user, after telling the
    /// compositor that windows of this shell's children belong to this tile.
    fn run_in_place(&mut self, command: &str) -> Vec<u8> {
        if let Some(tile) = self.tile
            && (self.send)(&Request::Swallow { tile, shell_pid: self.shell_pid })
        {
            self.swallowing = true;
        }
        format!("{command}\r").into_bytes()
    }

    fn end_swallow(&mut self) {
        if self.swallowing {
            self.swallowing = false;
            if let Some(tile) = self.tile {
                (self.send)(&Request::Unswallow { tile });
            }
        }
    }

    fn shell_cwd(&self) -> PathBuf {
        std::fs::read_link(format!("/proc/{}/cwd", self.shell_pid))
            .or_else(|_| std::env::current_dir())
            .unwrap_or_else(|_| PathBuf::from("/"))
    }
}

/// Name of dune's shell wrapper; terminals inside dune get it as `$SHELL`.
pub const WRAPPER_NAME: &str = "dune-shell";

/// When `$SHELL` is dune's wrapper, the user's real shell (`DUNE_USER_SHELL`, or
/// `/bin/sh`): a terminal that does the `:` layer itself starts that directly.
pub fn wrapped_user_shell() -> Option<PathBuf> {
    let shell = PathBuf::from(std::env::var_os("SHELL")?);
    if shell.file_name()? != WRAPPER_NAME {
        return None;
    }
    Some(
        std::env::var_os("DUNE_USER_SHELL")
            .map(PathBuf::from)
            .filter(|s| s.file_name().is_some_and(|n| n != WRAPPER_NAME) && s.exists())
            .unwrap_or_else(|| PathBuf::from("/bin/sh")),
    )
}

/// Length of the escape sequence at the start of `bytes` (which starts with ESC).
fn escape_len(bytes: &[u8]) -> usize {
    match bytes.get(1) {
        Some(b'[') => bytes[2..]
            .iter()
            .position(|b| (0x40..=0x7e).contains(b))
            .map_or(bytes.len(), |p| p + 3),
        // OSC / DCS / APC: until BEL or ESC \
        Some(b']') | Some(b'P') | Some(b'_') => {
            let mut j = 2;
            while j < bytes.len() {
                if bytes[j] == 0x07 {
                    return j + 1;
                }
                if bytes[j] == 0x1b && bytes.get(j + 1) == Some(&b'\\') {
                    return j + 2;
                }
                j += 1;
            }
            bytes.len()
        }
        Some(b'O') => 3.min(bytes.len()),
        Some(_) => 2,
        None => 1,
    }
}

fn is_terminal_reply(seq: &[u8]) -> bool {
    match seq.get(1) {
        Some(b']') | Some(b'P') | Some(b'_') => true,
        // Cursor position, device attributes/status, focus in/out, window reports.
        Some(b'[') => matches!(seq.last(), Some(b'R' | b'c' | b'n' | b'I' | b'O' | b't' | b'y')),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use crate::protocol::Side;

    /// A layer for tile 7 whose requests are recorded.
    fn layer(run_first: Option<&str>) -> (AtLayer, Rc<RefCell<Vec<Request>>>) {
        layer_pointing(run_first, None)
    }

    /// The same, with the tile's direction indicator pointing to `side` (once).
    fn layer_pointing(run_first: Option<&str>, side: Option<Side>) -> (AtLayer, Rc<RefCell<Vec<Request>>>) {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let log = sent.clone();
        let send = Box::new(move |req: &Request| {
            log.borrow_mut().push(match req {
                Request::Open { side, from, command, .. } => {
                    Request::Open { side: *side, from: *from, cwd: None, command: command.clone() }
                }
                Request::Direction { tile } => Request::Direction { tile: *tile },
                Request::Swallow { tile, shell_pid } => Request::Swallow { tile: *tile, shell_pid: *shell_pid },
                Request::Unswallow { tile } => Request::Unswallow { tile: *tile },
                Request::Keys(k) => Request::Keys(k.clone()),
            });
            true
        });
        let apps = Box::new(|| vec![App { id: "firefox".into(), name: "Firefox".into(), exec: "firefox %u".into() }]);
        let indicator = std::cell::Cell::new(side);
        let take_direction = Box::new(move || indicator.take());
        (AtLayer::new(Some(7), 4242, run_first.map(String::from), send, apps, take_direction), sent)
    }

    #[test]
    fn ordinary_typing_passes_through() {
        let (mut at, sent) = layer(None);
        assert_eq!(at.input(b"echo a:b\r", true), b"echo a:b\r");
        assert!(at.picker().is_none());
        assert!(sent.borrow().is_empty());
    }

    #[test]
    fn at_while_a_program_runs_is_for_the_program() {
        let (mut at, _) = layer(None);
        assert_eq!(at.input(b":", false), b":");
        assert!(at.picker().is_none());
    }

    #[test]
    fn at_on_an_empty_prompt_opens_the_picker() {
        let (mut at, _) = layer(None);
        assert_eq!(at.input(b":ls", true), b"");
        assert_eq!(at.picker().unwrap().input(), "ls");
        // Escape closes it and the prompt stays empty.
        assert_eq!(at.input(b"\x1b", true), b"");
        assert!(at.picker().is_none());
    }

    #[test]
    fn terminal_replies_do_not_count_as_typing() {
        let (mut at, _) = layer(None);
        at.input(b"\x1b[12;40R\x1b[I", true);
        at.input(b":", true);
        assert!(at.picker().is_some());
        // But an arrow key (history) does.
        let (mut at, _) = layer(None);
        at.input(b"\x1b[A", true);
        at.input(b":", true);
        assert!(at.picker().is_none());
    }

    #[test]
    fn in_place_run_swallows_until_the_prompt_returns() {
        let (mut at, sent) = layer(None);
        assert_eq!(at.input(b":sleep 1\r", true), b"sleep 1\r");
        assert_eq!(sent.borrow()[0], Request::Swallow { tile: 7, shell_pid: 4242 });
        at.tick(false); // the program runs
        at.tick(true); // and ends
        assert_eq!(sent.borrow()[1], Request::Unswallow { tile: 7 });
        assert_eq!(sent.borrow().len(), 2);
    }

    #[test]
    fn split_asks_the_compositor() {
        let (mut at, sent) = layer(None);
        assert_eq!(at.input(b":vim:v notes.md\r", true), b"");
        assert_eq!(
            sent.borrow()[0],
            Request::Open { side: Some(Side::Right), from: Some(7), cwd: None, command: "vim notes.md".into() }
        );
    }

    #[test]
    fn at_launches_desktop_apps() {
        let (mut at, sent) = layer(None);
        assert_eq!(at.input(b":fire https://a\r", true), b"firefox https://a\r");
        assert_eq!(sent.borrow()[0], Request::Swallow { tile: 7, shell_pid: 4242 });
    }

    #[test]
    fn the_direction_indicator_sends_at_apps_aside() {
        let (mut at, sent) = layer_pointing(None, Some(Side::Left));
        assert_eq!(at.input(b":fire\r", true), b""); // nothing runs here
        assert_eq!(
            sent.borrow()[0],
            Request::Open { side: Some(Side::Left), from: Some(7), cwd: None, command: "firefox".into() }
        );
        // Used once: the next one runs in place.
        assert_eq!(at.input(b":fire\r", true), b"firefox\r");
    }

    #[test]
    fn open_beside_a_typed_command() {
        let (mut at, sent) = layer_pointing(None, Some(Side::Up));
        at.input(b"htop", true);
        let side = at.take_direction().unwrap();
        at.open_beside(side, "htop".into());
        assert!(at.line_is_empty());
        assert_eq!(sent.borrow()[0], Request::Open { side: Some(Side::Up), from: Some(7), cwd: None, command: "htop".into() });
        assert_eq!(at.take_direction(), None);
    }

    #[test]
    fn split_command_without_at() {
        let (mut at, sent) = layer(None);
        at.input(b"vim:h test.txt", true);
        assert!(!at.line_is_empty());
        at.split_command(picker::parse_split_command("vim:h test.txt").unwrap());
        assert!(at.line_is_empty());
        assert_eq!(
            sent.borrow()[0],
            Request::Open { side: Some(Side::Down), from: Some(7), cwd: None, command: "vim test.txt".into() }
        );
    }

    #[test]
    fn dune_run_waits_for_the_shell_to_settle() {
        let (mut at, sent) = layer(Some("htop"));
        assert!(at.waiting());
        assert_eq!(at.tick(true), b""); // nothing printed yet
        at.output_seen();
        assert_eq!(at.tick(true), b""); // printed just now
        std::thread::sleep(SETTLE + Duration::from_millis(20));
        assert_eq!(at.tick(false), b""); // a startup program is running
        assert_eq!(at.tick(true), b"htop\r");
        assert!(!at.waiting());
        assert_eq!(sent.borrow()[0], Request::Swallow { tile: 7, shell_pid: 4242 });
    }

    #[test]
    fn escape_sequences() {
        assert_eq!(escape_len(b"\x1b[A rest"), 3);
        assert_eq!(escape_len(b"\x1b[12;40R"), 8);
        assert_eq!(escape_len(b"\x1b]11;rgb:0/0/0\x07x"), 15);
        assert_eq!(escape_len(b"\x1bOA"), 3);
        assert!(is_terminal_reply(b"\x1b[12;40R"));
        assert!(!is_terminal_reply(b"\x1b[3~"));
    }
}
