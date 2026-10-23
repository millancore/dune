//! The compositor's end of the control socket (protocol in `dune_common::protocol`),
//! and the `dune open`, `dune scratchpad`, `dune recording`, `dune desktops` and `dune bar`
//! commands.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    time::Duration,
};

use dune_common::protocol::{Request, Side, ask, send, watch};
use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction, generic::Generic};

use crate::{
    Dune,
    desktops::DESKS,
    workspaces::{DeskState, parse_desktops_line},
};

pub use dune_common::protocol::socket_path;

pub fn listen(event_loop: &mut EventLoop<'static, Dune>, path: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    listener.set_nonblocking(true)?;

    event_loop
        .handle()
        .insert_source(Generic::new(listener, Interest::READ, Mode::Level), |_, listener, state| {
            while let Ok((mut stream, _)) = listener.accept() {
                match read_request(&mut stream) {
                    Some(Request::WatchScratchpad) => state.watch_scratchpad(stream),
                    Some(Request::WatchRecording) => state.watch_recording(stream),
                    Some(Request::WatchDesktops { output }) => state.watch_desktops(output, stream),
                    Some(req) => {
                        if let Some(answer) = state.handle_request(req) {
                            let _ = stream.write_all(answer.as_bytes());
                        }
                    }
                    None => tracing::warn!("ignoring malformed IPC request"),
                }
            }
            Ok(PostAction::Continue)
        })
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}

fn read_request(stream: &mut UnixStream) -> Option<Request> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(200))).ok()?;
    let mut msg = String::new();
    stream.read_to_string(&mut msg).ok()?;
    Request::decode(&msg)
}

/// `dune scratchpad show`: what the `scratchpad` key does. `dune scratchpad watch`: a line
/// for waybar (`return-type: json`) now and on every change of the scratchpad, until dune
/// goes away.
pub fn scratchpad_command(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("show") => ask(&Request::ScratchpadShow, Duration::from_secs(1)).map(|_| ()),
        Some("watch") => {
            let stream = watch(&Request::WatchScratchpad)?;
            let mut out = std::io::stdout().lock();
            for line in BufReader::new(stream).lines() {
                let line = line.map_err(|e| e.to_string())?;
                writeln!(out, "{line}").and_then(|_| out.flush()).map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        _ => Err("usage: dune scratchpad show|watch".into()),
    }
}

/// The string `field` of one of the waybar JSON lines dune writes, unescaped; empty
/// without one.
fn json_field(line: &str, field: &str) -> String {
    let key = format!(r#""{field}":""#);
    let Some(start) = line.find(&key) else { return String::new() };
    let mut text = String::new();
    let mut chars = line[start + key.len()..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                Some('n') => text.push('\n'),
                Some(c) => text.push(c),
                None => break,
            },
            c => text.push(c),
        }
    }
    text
}

/// `dune recording stop`: stop the recording running. `dune recording watch`: a waybar
/// JSON line (`return-type: json`) now and on every change, the time recorded going on each
/// second while one runs, until dune goes away. `started` / `ended` are what the
/// recording's shell tells dune (`crate::screenshot`).
pub fn recording_command(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("stop") => ask(&Request::RecordingStop, Duration::from_secs(1)).map(|_| ()),
        Some(state @ ("started" | "ended")) => {
            let area = args.get(1).and_then(|g| dune_common::protocol::parse_geometry(g));
            send(&Request::Recording { started: state == "started", area })
        }
        Some("watch") => {
            let stream = watch(&Request::WatchRecording)?;
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines() {
                    if line.ok().and_then(|l| tx.send(l).ok()).is_none() {
                        return;
                    }
                }
            });
            let mut out = std::io::stdout().lock();
            let mut since: Option<u64> = None;
            let mut last = String::new();
            loop {
                match rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(line) => since = recording_since(&line),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                }
                let line = recording_line(elapsed(since));
                if line != last {
                    writeln!(out, "{line}").and_then(|_| out.flush()).map_err(|e| e.to_string())?;
                    last = line;
                }
            }
        }
        _ => Err("usage: dune recording stop|watch".into()),
    }
}

/// When the recording dune tells of in `line` (`on <unix secs>` / `off`) started.
fn recording_since(line: &str) -> Option<u64> {
    line.strip_prefix("on ").and_then(|s| s.trim().parse().ok())
}

/// The seconds since `since`, if any.
fn elapsed(since: Option<u64>) -> Option<u64> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    since.map(|s| now.saturating_sub(s))
}

/// What a bar shows for a recording going on for `elapsed` seconds (`● mm:ss`), or none
/// (empty).
fn recording_text(elapsed: Option<u64>) -> String {
    elapsed.map_or_else(String::new, |s| format!("● {:02}:{:02}", s / 60, s % 60))
}

/// The waybar line for a recording going on for `elapsed` seconds, or none (empty, which
/// waybar hides).
fn recording_line(elapsed: Option<u64>) -> String {
    match elapsed {
        Some(_) => format!(
            r#"{{"text":"{}","tooltip":"Recording · click to stop","class":"recording"}}"#,
            recording_text(elapsed)
        ),
        None => r#"{"text":"","tooltip":"","class":"idle"}"#.into(),
    }
}

/// `dune desktops show N [OUTPUT]`: show desktop N on that monitor (none: the active one).
/// `dune desktops lock N [OUTPUT]`: lock it, or unlock it.
pub fn desktops_command(args: &[String]) -> Result<(), String> {
    const USAGE: &str = "usage: dune desktops show|lock N [OUTPUT]";
    let desk = || args.get(1).and_then(|n| n.parse().ok()).filter(|n| (1..=DESKS).contains(n)).ok_or(USAGE);
    let output = || args.get(2).cloned();
    match args.first().map(String::as_str) {
        Some("show") => ask(&Request::DeskShow { output: output(), desk: desk()? }, Duration::from_secs(1)).map(|_| ()),
        Some("lock") => ask(&Request::DeskLock { output: output(), desk: desk()? }, Duration::from_secs(1)).map(|_| ()),
        _ => Err(USAGE.into()),
    }
}

/// A line from one of the watches `dune bar watch` keeps.
enum BarEvent {
    Desktops(String),
    Scratchpad(String),
    Recording(String),
}

/// `dune bar watch OUTPUT`: keep Ironbar's variables up to date, until dune goes away:
/// `dune_<OUTPUT>_<n>` (`ironbar_key`) the labels of that monitor's desktops
/// (`desk_label`), `dune_scratchpad` the count of scratchpad windows (empty with none) and
/// `dune_scratchpad_tooltip` their titles, `dune_recording` the time recorded (`● mm:ss`,
/// empty with none). Only what changed goes to Ironbar; what it did not take is sent again
/// on the next change or second.
pub fn bar_command(args: &[String]) -> Result<(), String> {
    const USAGE: &str = "usage: dune bar watch OUTPUT";
    let (Some("watch"), Some(output)) = (args.first().map(String::as_str), args.get(1)) else {
        return Err(USAGE.into());
    };
    let (tx, rx) = std::sync::mpsc::channel();
    for (req, event) in [
        (Request::WatchDesktops { output: output.clone() }, BarEvent::Desktops as fn(String) -> BarEvent),
        (Request::WatchScratchpad, BarEvent::Scratchpad),
        (Request::WatchRecording, BarEvent::Recording),
    ] {
        let stream = watch(&req)?;
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                if line.ok().and_then(|l| tx.send(event(l)).ok()).is_none() {
                    return;
                }
            }
        });
    }
    drop(tx);
    let socket = ironbar_socket().ok_or("dune: XDG_RUNTIME_DIR is not set")?;
    let mut want: HashMap<String, String> = HashMap::new();
    let mut sent: HashMap<String, String> = HashMap::new();
    let mut since = None;
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(BarEvent::Desktops(line)) => {
                for (n, (state, name)) in (1..).zip(parse_desktops_line(&line)) {
                    want.insert(ironbar_key(output, n), desk_label(state, &name));
                }
            }
            Ok(BarEvent::Scratchpad(line)) => {
                want.insert("dune_scratchpad".into(), json_field(&line, "text"));
                want.insert("dune_scratchpad_tooltip".into(), json_field(&line, "tooltip"));
            }
            Ok(BarEvent::Recording(line)) => since = recording_since(&line),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
        want.insert("dune_recording".into(), recording_text(elapsed(since)));
        for (key, value) in &want {
            if sent.get(key) == Some(value) {
                continue;
            }
            match ironbar_set(&socket, key, value) {
                Ok(()) => {
                    sent.insert(key.clone(), value.clone());
                }
                Err(e) => eprintln!("dune: ironbar var set {key}: {e}"),
            }
        }
    }
}

/// Ironbar's IPC socket.
fn ironbar_socket() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join("ironbar-ipc.sock"))
}

/// What `ironbar var set KEY VALUE` sends: one JSON line.
fn ironbar_set_request(key: &str, value: &str) -> String {
    use crate::scratchpad::json_escape;
    format!(
        r#"{{"command":"var","subcommand":"set","key":"{}","value":"{}"}}"#,
        json_escape(key),
        json_escape(value)
    ) + "\n"
}

/// Set Ironbar's variable `key` to `value` through its socket, as `ironbar var set` does:
/// one request per connection, answered `{"type":"ok"}`.
fn ironbar_set(socket: &Path, key: &str, value: &str) -> Result<(), String> {
    let mut stream = UnixStream::connect(socket).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(1))).map_err(|e| e.to_string())?;
    stream.write_all(ironbar_set_request(key, value).as_bytes()).map_err(|e| e.to_string())?;
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).map_err(|e| e.to_string())?;
    if answer.contains(r#""type":"ok""#) { Ok(()) } else { Err(answer.trim().to_string()) }
}

/// Ironbar's variable for desktop `n` of `output`: `dune_<output>_<n>`, what is not
/// alphanumeric, `-` or `_` in the name made `_`.
fn ironbar_key(output: &str, n: u8) -> String {
    let output: String = output.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("dune_{output}_{n}")
}

/// The Pango markup a bar shows for a desktop: empty when hidden (the button goes), the
/// shown one bold and underlined.
fn desk_label(state: DeskState, name: &str) -> String {
    let name = name.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    match state {
        DeskState::Hidden => String::new(),
        DeskState::Shown => name,
        // Ironbar escapes markup that has no `<span`.
        DeskState::Active => format!("<span weight=\"bold\" underline=\"single\">{name}</span>"),
    }
}

/// `dune open [--split l|r|u|d] [--from TILE] [--cwd DIR] [--] COMMAND...`
/// (`v` and `h` are accepted for `r` and `d`).
pub fn open_command(args: &[String]) -> Result<(), String> {
    let mut side = None;
    let mut from = std::env::var("DUNE_TILE").ok().and_then(|t| t.parse().ok());
    let mut cwd = std::env::current_dir().ok();
    let mut args = args.iter();
    let mut command = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--split" => {
                let v = args.next().ok_or("--split needs a value")?;
                side = Some(Side::parse(v).ok_or("--split must be one of l, r, u, d (or v, h)")?);
            }
            "--from" => from = args.next().and_then(|t| t.parse().ok()),
            "--cwd" => cwd = args.next().map(PathBuf::from),
            "--" => command.extend(args.by_ref().cloned()),
            _ => command.push(arg.clone()),
        }
    }
    send(&Request::Open {
        side,
        from,
        cwd,
        command: command.join(" "),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recording_line_counts_the_time() {
        assert_eq!(recording_line(None), r#"{"text":"","tooltip":"","class":"idle"}"#);
        assert_eq!(
            recording_line(Some(83)),
            r#"{"text":"● 01:23","tooltip":"Recording · click to stop","class":"recording"}"#
        );
    }

    #[test]
    fn the_fields_of_a_bar_line() {
        let scratchpad = crate::scratchpad::status_line(&["a".into(), "b".into()]);
        assert_eq!(json_field(&scratchpad, "text"), "2");
        assert_eq!(json_field(&scratchpad, "tooltip"), "Scratchpad\na\nb");
        assert_eq!(json_field(&recording_line(None), "text"), "");
        assert_eq!(json_field(r#"{"text":"a \"b\" \\ c","class":"x"}"#, "text"), r#"a "b" \ c"#);
        assert_eq!(json_field("off", "text"), "");
    }

    #[test]
    fn the_recording_text() {
        assert_eq!(recording_text(None), "");
        assert_eq!(recording_text(Some(83)), "● 01:23");
        assert_eq!(recording_since("on 1700000000"), Some(1700000000));
        assert_eq!(recording_since("off"), None);
    }

    #[test]
    fn ironbar_set_sends_one_json_line() {
        assert_eq!(
            ironbar_set_request("dune_scratchpad_tooltip", "Scratchpad\n\"a\""),
            "{\"command\":\"var\",\"subcommand\":\"set\",\"key\":\"dune_scratchpad_tooltip\",\"value\":\"Scratchpad\\n\\\"a\\\"\"}\n"
        );
        let path = std::env::temp_dir().join(format!("dune-ironbar-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let bar = std::thread::spawn(move || {
            let mut asked = Vec::new();
            for answer in [r#"{"type":"ok"}"#, r#"{"type":"error","message":"no"}"#] {
                let (stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                asked.push(line);
                writeln!(&stream, "{answer}").unwrap();
            }
            asked
        });
        assert_eq!(ironbar_set(&path, "dune_recording", "● 00:01"), Ok(()));
        assert!(ironbar_set(&path, "dune_recording", "").is_err());
        let asked = bar.join().unwrap();
        assert_eq!(asked[0], ironbar_set_request("dune_recording", "● 00:01"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ironbar_keys_keep_only_what_it_allows() {
        assert_eq!(ironbar_key("HDMI-A-1", 3), "dune_HDMI-A-1_3");
        assert_eq!(ironbar_key("Virtual.1 x", 9), "dune_Virtual_1_x_9");
    }

    #[test]
    fn desk_labels_as_markup() {
        assert_eq!(desk_label(DeskState::Hidden, "3"), "");
        assert_eq!(desk_label(DeskState::Shown, "🔒 2"), "🔒 2");
        assert_eq!(desk_label(DeskState::Active, "1"), "<span weight=\"bold\" underline=\"single\">1</span>");
        assert_eq!(desk_label(DeskState::Shown, "a<&>"), "a&lt;&amp;&gt;");
    }
}
