//! The dune layer: what happens between the keyboard and the shell of a terminal tile.
//!
//! Typing goes to the shell; this layer runs `DUNE_RUN` once the shell is ready, and
//! tells the compositor which tile the windows of what it ran belong to (swallow).
//! "At the prompt" means the shell itself is the foreground process group of its
//! terminal (no program running) — the caller checks that.
//!
//! The layer only decides; the caller does the I/O: it writes the returned bytes to
//! the shell. Applications are started from the compositor's launcher.

use std::time::{Duration, Instant};

use crate::protocol::{Request, TileId};

/// Quiet time after the shell's first prompt before a `DUNE_RUN` command is typed
/// (startup files may run programs before that).
const SETTLE: Duration = Duration::from_millis(300);

pub struct AtLayer {
    tile: Option<TileId>,
    shell_pid: u32,
    send: Box<dyn Fn(&Request) -> bool>,
    /// The shell was at its prompt the last time we looked.
    was_idle: bool,
    /// A program was started in place and the compositor is waiting for its window.
    swallowing: bool,
    /// Command to run once the shell is ready (`DUNE_RUN`: what the launcher runs in a terminal).
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
        Self::new(tile, shell_pid, run_first, send)
    }

    pub fn new(
        tile: Option<TileId>,
        shell_pid: u32,
        run_first: Option<String>,
        send: Box<dyn Fn(&Request) -> bool>,
    ) -> Self {
        AtLayer {
            tile,
            shell_pid,
            send,
            was_idle: true,
            swallowing: false,
            run_first: run_first.filter(|r| !r.trim().is_empty()),
            last_output: None,
        }
    }

    /// Whether the layer applies: only inside dune.
    pub fn enabled() -> bool {
        std::env::var_os("DUNE_SOCKET").is_some()
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
            self.end_swallow();
        }
        self.was_idle = at_prompt;

        if self.run_first.is_some() && at_prompt && self.last_output.is_some_and(|t| t.elapsed() >= SETTLE) {
            let command = self.run_first.take().unwrap();
            return self.run_in_place(&command);
        }
        Vec::new()
    }

    /// Input typed by the user (already encoded as terminal bytes); returns what goes
    /// to the shell (all of it: the layer only watches).
    pub fn input(&mut self, bytes: &[u8], at_prompt: bool) -> Vec<u8> {
        if !at_prompt {
            return bytes.to_vec();
        }

        let mut i = 0;
        while i < bytes.len() {
            // Escape sequences (keys, terminal replies) may carry anything; skip them.
            if bytes[i] == 0x1b {
                i += escape_len(&bytes[i..]);
                continue;
            }
            if matches!(bytes[i], b'\r' | b'\n') {
                // A builtin run in place (no program ran) ends here.
                self.end_swallow();
            }
            i += 1;
        }
        bytes.to_vec()
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

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    /// A layer for tile 7 whose requests are recorded.
    fn layer(run_first: Option<&str>) -> (AtLayer, Rc<RefCell<Vec<Request>>>) {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let log = sent.clone();
        let send = Box::new(move |req: &Request| {
            log.borrow_mut().push(req.clone());
            true
        });
        (AtLayer::new(Some(7), 4242, run_first.map(String::from), send), sent)
    }

    #[test]
    fn ordinary_typing_passes_through() {
        let (mut at, sent) = layer(None);
        assert_eq!(at.input(b"echo a:b\r", true), b"echo a:b\r");
        assert!(sent.borrow().is_empty());
    }

    #[test]
    fn enter_at_the_prompt_ends_a_swallow() {
        let (mut at, sent) = layer(Some("cd /tmp"));
        at.output_seen();
        std::thread::sleep(SETTLE + Duration::from_millis(20));
        assert_eq!(at.tick(true), b"cd /tmp\r");
        // A terminal reply carrying a newline is not Enter.
        at.input(b"\x1b]11;a\nb\x07", true);
        assert_eq!(sent.borrow().len(), 1);
        at.input(b"\r", true);
        assert_eq!(sent.borrow()[1], Request::Unswallow { tile: 7 });
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
    }
}
