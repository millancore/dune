//! The dune layer: what happens between the keyboard and the shell of a terminal tile.
//!
//! Typing goes to the shell untouched; this layer only types `DUNE_RUN` (what `dune open`
//! and the launcher run in a terminal) once the shell is ready. "At the prompt" means the
//! shell itself is the foreground process group of its terminal (no program running) —
//! the caller checks that.
//!
//! The layer only decides; the caller does the I/O: it writes the returned bytes to
//! the shell. Applications are started from the compositor's launcher.

use std::time::{Duration, Instant};

/// Quiet time after the shell's first prompt before a `DUNE_RUN` command is typed
/// (startup files may run programs before that).
const SETTLE: Duration = Duration::from_millis(300);

pub struct AtLayer {
    /// Command to run once the shell is ready (`DUNE_RUN`: what the launcher runs in a terminal).
    run_first: Option<String>,
    last_output: Option<Instant>,
}

impl AtLayer {
    /// For a shell started inside dune: the first command from the environment (`DUNE_RUN`).
    pub fn from_env() -> Self {
        Self::new(std::env::var("DUNE_RUN").ok())
    }

    pub fn new(run_first: Option<String>) -> Self {
        AtLayer { run_first: run_first.filter(|r| !r.trim().is_empty()), last_output: None }
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
    /// to the shell (the `DUNE_RUN` command once startup is over), as if typed.
    pub fn tick(&mut self, at_prompt: bool) -> Vec<u8> {
        if at_prompt && self.last_output.is_some_and(|t| t.elapsed() >= SETTLE)
            && let Some(command) = self.run_first.take()
        {
            return format!("{command}\r").into_bytes();
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dune_run_waits_for_the_shell_to_settle() {
        let mut at = AtLayer::new(Some("htop".into()));
        assert!(at.waiting());
        assert_eq!(at.tick(true), b""); // nothing printed yet
        at.output_seen();
        assert_eq!(at.tick(true), b""); // printed just now
        std::thread::sleep(SETTLE + Duration::from_millis(20));
        assert_eq!(at.tick(false), b""); // a startup program is running
        assert_eq!(at.tick(true), b"htop\r");
        assert!(!at.waiting());
        assert_eq!(at.tick(true), b"");
    }

    #[test]
    fn no_command_types_nothing() {
        let mut at = AtLayer::new(Some("  ".into()));
        at.output_seen();
        std::thread::sleep(SETTLE + Duration::from_millis(20));
        assert!(!at.waiting());
        assert_eq!(at.tick(true), b"");
    }
}
