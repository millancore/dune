//! The compositor's end of the control socket (protocol in `dune_common::protocol`),
//! and the `dune open` command.

use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    time::Duration,
};

use dune_common::protocol::{Request, Side, send};
use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction, generic::Generic};

use crate::Dune;

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
