//! Which tiles play sound, so a docked one can show it (a speaker on its bar). A thread
//! follows `pactl subscribe` and, on every change of the playback streams, lists them
//! (`pactl list sink-inputs`): the processes of the streams playing (not paused, not
//! muted) are sent to the event loop, which marks the tiles they belong to. A browser
//! plays through one process for all its windows: when several tiles share it, only those
//! whose title holds the title of what plays (MPRIS, followed with `playerctl`) count.

use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    time::Duration,
};

use smithay::reexports::calloop::{LoopHandle, channel};

use crate::{Dune, layout::TileId};

/// Wait before following `pactl subscribe` again when it ends (the sound server restarted).
const RETRY: Duration = Duration::from_secs(5);

enum Change {
    /// Processes of the streams playing now.
    Pids(HashSet<u32>),
    /// Titles of what the media players play now.
    Titles(Vec<String>),
}

pub fn watch(loop_handle: &LoopHandle<'static, Dune>) {
    let (tx, rx) = channel::channel();
    let watched = loop_handle.insert_source(rx, |event, _, state| match event {
        channel::Event::Msg(Change::Pids(pids)) => {
            state.sound_pids = pids;
            state.update_sounding();
        }
        channel::Event::Msg(Change::Titles(titles)) => state.sound_titles = titles,
        channel::Event::Closed => {}
    });
    if let Err(err) = watched {
        return tracing::warn!("docked tiles will not show sound: {err}");
    }
    let players = tx.clone();
    let spawned = std::thread::Builder::new().name("dune-sound".into()).spawn(move || follow(&tx));
    if let Err(err) = spawned {
        tracing::warn!("docked tiles will not show sound: {err}");
    }
    let spawned = std::thread::Builder::new().name("dune-players".into()).spawn(move || follow_players(&players));
    if let Err(err) = spawned {
        tracing::warn!("browser tiles will not tell which one plays: {err}");
    }
}

/// Send the playing pids now and after every change of the streams, until dune exits.
fn follow(tx: &channel::Sender<Change>) {
    loop {
        let child = Command::new("pactl").arg("subscribe").env("LC_ALL", "C").stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(err) => return tracing::warn!("docked tiles will not show sound: no pactl ({err})"),
        };
        if tx.send(Change::Pids(playing())).is_err() {
            let _ = child.kill();
            return;
        }
        let lines = BufReader::new(child.stdout.take().unwrap()).lines();
        for line in lines.map_while(Result::ok) {
            if line.contains("sink-input") && tx.send(Change::Pids(playing())).is_err() {
                let _ = child.kill();
                return;
            }
        }
        let _ = child.wait();
        std::thread::sleep(RETRY);
    }
}

/// `playerctl` lists every player's status and title on one line.
const PLAYER_FORMAT: &str = "{{status}}\t{{xesam:title}}";

/// Send the titles of what plays now and after every change of a player, until dune exits.
fn follow_players(tx: &channel::Sender<Change>) {
    loop {
        let child = Command::new("playerctl")
            .args(["--all-players", "--follow", "metadata", "--format", PLAYER_FORMAT])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(err) => return tracing::warn!("browser tiles will not tell which one plays: no playerctl ({err})"),
        };
        if tx.send(Change::Titles(playing_titles())).is_err() {
            let _ = child.kill();
            return;
        }
        // `--follow` prints only the player that changed: list them all again.
        let lines = BufReader::new(child.stdout.take().unwrap()).lines();
        for _ in lines.map_while(Result::ok) {
            if tx.send(Change::Titles(playing_titles())).is_err() {
                let _ = child.kill();
                return;
            }
        }
        let _ = child.wait();
        std::thread::sleep(RETRY);
    }
}

/// Titles of what the media players play now.
fn playing_titles() -> Vec<String> {
    Command::new("playerctl")
        .args(["--all-players", "metadata", "--format", PLAYER_FORMAT])
        .stderr(Stdio::null())
        .output()
        .map(|out| titles_playing(&String::from_utf8_lossy(&out.stdout)))
        .unwrap_or_default()
}

/// The titles of the `Playing` lines of `playerctl` output in `PLAYER_FORMAT`, lowercase.
fn titles_playing(listing: &str) -> Vec<String> {
    listing
        .lines()
        .filter_map(|line| line.strip_prefix("Playing\t"))
        .map(|title| title.trim().to_lowercase())
        .filter(|title| !title.is_empty())
        .collect()
}

/// Processes of the playback streams that play now.
fn playing() -> HashSet<u32> {
    Command::new("pactl")
        .args(["list", "sink-inputs"])
        .env("LC_ALL", "C")
        .stderr(Stdio::null())
        .output()
        .map(|out| playing_pids(&String::from_utf8_lossy(&out.stdout)))
        .unwrap_or_default()
}

/// The `application.process.id` of each stream in `pactl list sink-inputs` output that
/// is neither corked (paused) nor muted.
fn playing_pids(listing: &str) -> HashSet<u32> {
    let mut pids = HashSet::new();
    for stream in listing.split("Sink Input #").skip(1) {
        let mut silent = false;
        let mut pid: Option<u32> = None;
        for line in stream.lines().map(str::trim) {
            if line == "Corked: yes" || line == "Mute: yes" {
                silent = true;
            } else if let Some(value) = line.strip_prefix("application.process.id = ") {
                pid = value.trim_matches('"').parse().ok();
            }
        }
        pids.extend(pid.filter(|_| !silent));
    }
    pids
}

impl Dune {
    /// Mark the tiles whose window's process, or one it started, plays sound. Tiles that
    /// share that process with others (a browser's windows) only might: see `plays_sound`.
    pub fn update_sounding(&mut self) {
        let mut owners: HashMap<u32, Vec<TileId>> = HashMap::new();
        if !self.sound_pids.is_empty() {
            let ids: Vec<TileId> = self.tiles.keys().copied().collect();
            for id in ids {
                let Some(owner) = self.tile_pid(id) else { continue };
                if self.sound_pids.iter().any(|pid| crate::tiling::is_descendant(*pid, owner)) {
                    owners.entry(owner).or_default().push(id);
                }
            }
        }
        self.sounding.clear();
        self.sounding_shared.clear();
        for tiles in owners.into_values() {
            match tiles[..] {
                [one] => {
                    self.sounding.insert(one);
                }
                _ => self.sounding_shared.extend(tiles),
            }
        }
    }

    /// Whether the tile plays sound: its process does, alone, or its window's title holds
    /// the title of what a media player plays (the title changes with the browser's tab).
    pub fn plays_sound(&self, id: TileId) -> bool {
        self.sounding.contains(&id)
            || self.sounding_shared.contains(&id) && {
                let title = self.tile_title(id).to_lowercase();
                self.sound_titles.iter().any(|t| title.contains(t.as_str()))
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_streams_that_play_count() {
        let listing = "Sink Input #10\n\tDriver: PipeWire\n\tCorked: no\n\tMute: no\n\tProperties:\n\t\tapplication.name = \"Firefox\"\n\t\tapplication.process.id = \"1234\"\n\n\
                       Sink Input #11\n\tCorked: yes\n\tMute: no\n\tProperties:\n\t\tapplication.process.id = \"55\"\n\n\
                       Sink Input #12\n\tCorked: no\n\tMute: yes\n\tProperties:\n\t\tapplication.process.id = \"66\"\n\n\
                       Sink Input #13\n\tCorked: no\n\tMute: no\n\tProperties:\n\t\tapplication.name = \"mpv\"\n";
        assert_eq!(playing_pids(listing), HashSet::from([1234]));
        assert!(playing_pids("").is_empty());
    }

    #[test]
    fn only_titles_that_play_count() {
        let listing = "Playing\tEL LADO OSCURO\nPaused\tOther song\nPlaying\t\nStopped\t\n";
        assert_eq!(titles_playing(listing), vec!["el lado oscuro".to_string()]);
        assert!(titles_playing("").is_empty());
    }
}
