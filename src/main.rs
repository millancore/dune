#![allow(irrefutable_let_patterns)]

mod config;
mod handlers;
mod input;
mod ipc;
mod layout;
mod shell;
mod state;
mod tiling;
mod winit;

use std::path::{Path, PathBuf};

use smithay::reexports::{calloop::EventLoop, wayland_server::Display};
pub use state::Dune;

const USAGE: &str = "\
usage:
  dune                                   start the compositor (nested in the current session)
  dune open [--split v|h] [--from TILE] [--cwd DIR] [--] COMMAND...
                                         open COMMAND in a new terminal tile
  dune shell                             run the @ layer around $DUNE_USER_SHELL (what tiles run)";

fn main() {
    let mut args = std::env::args_os();
    let argv0 = args.next().map(|a| a.to_string_lossy().into_owned()).unwrap_or_default();
    let args: Vec<_> = args.collect();

    // Terminals run us as their shell through the `dune-shell` link (`-dune-shell` for login shells).
    let name = Path::new(&argv0).file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
    if name.trim_start_matches('-') == shell::ARGV0 {
        shell::run(&name, args);
    }

    let args: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let result = match args.first().map(String::as_str) {
        None => run().map_err(|e| e.to_string()),
        Some("open") => ipc::open_command(&args[1..]),
        Some("shell") => shell::run(shell::ARGV0, args[1..].iter().map(Into::into).collect()),
        Some("-h") | Some("--help") => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.to_string()),
    };
    if let Err(err) = result {
        eprintln!("dune: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let mut event_loop: EventLoop<Dune> = EventLoop::try_new()?;
    let display: Display<Dune> = Display::new()?;
    let mut state = Dune::new(&mut event_loop, display);

    crate::winit::init_winit(&mut event_loop, &mut state)?;

    let name = state.socket_name.to_string_lossy().into_owned();
    let socket = ipc::socket_path(&name);
    ipc::listen(&mut event_loop, &socket)?;

    state.user_shell = user_shell();
    state.shell_wrapper = match install_shell_link(&socket.with_extension("shell")) {
        Ok(link) => Some(link),
        Err(err) => {
            tracing::error!("cannot set up the @ layer ({err}); tiles will run the plain shell");
            None
        }
    };

    // Children connect to us, not to the host compositor, and find the control socket.
    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);
        std::env::set_var("DUNE_SOCKET", &socket);
        std::env::remove_var("DISPLAY");
    }
    tracing::info!("listening on {name:?}, control socket {socket:?}, shell {:?}", state.user_shell);

    // X11 applications go through xwayland-satellite, like in niri.
    let mut xwayland = match state.config.xwayland.as_str() {
        "off" => None,
        _ => start_xwayland_satellite(),
    };
    if let Some((x_display, _)) = &xwayland {
        unsafe { std::env::set_var("DISPLAY", x_display) };
        tracing::info!("X11 apps on DISPLAY={x_display} (xwayland-satellite)");
    }

    // The basic unit: start with one terminal.
    state.spawn_terminal(None, None, None, None);

    event_loop.run(None, &mut state, |_| {})?;

    if let Some((_, child)) = xwayland.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = std::fs::remove_file(&socket);
    if let Some(link) = &state.shell_wrapper {
        let _ = std::fs::remove_dir_all(link.parent().unwrap());
    }
    Ok(())
}

/// Start xwayland-satellite on the first free X display (`:0` is often taken by the
/// host session when nested). Returns the display name and the process.
fn start_xwayland_satellite() -> Option<(String, std::process::Child)> {
    let free = (0..64).find(|n| {
        !Path::new(&format!("/tmp/.X11-unix/X{n}")).exists() && !Path::new(&format!("/tmp/.X{n}-lock")).exists()
    })?;
    let display = format!(":{free}");
    // Its log (and Xwayland's) goes to a file of its own, next to the control socket.
    let log = std::env::var_os("XDG_RUNTIME_DIR")
        .map(|d| Path::new(&d).join(format!("dune-xwayland{display}.log")))
        .and_then(|p| std::fs::File::create(p).ok());
    let mut command = std::process::Command::new("xwayland-satellite");
    command.arg(&display).stdout(std::process::Stdio::null());
    if let Some(log) = log {
        command.stderr(log);
    }
    match command.spawn() {
        Ok(child) => Some((display, child)),
        Err(err) => {
            tracing::warn!("X11 apps unavailable: cannot start xwayland-satellite ({err})");
            None
        }
    }
}

/// The shell the user normally gets: `$SHELL`, unless that is already a dune wrapper
/// (dune started from inside a dune tile), then the one that wrapper runs.
fn user_shell() -> PathBuf {
    let is_wrapper = |p: &Path| p.file_name().is_some_and(|n| n == shell::ARGV0);
    std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|s| !is_wrapper(s))
        .or_else(|| std::env::var_os("DUNE_USER_SHELL").map(PathBuf::from))
        .filter(|s| !is_wrapper(s) && s.exists())
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

/// Create `<dir>/dune-shell` pointing at this binary.
fn install_shell_link(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let link = dir.join(shell::ARGV0);
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(std::env::current_exe()?, &link)?;
    Ok(link)
}

fn init_logging() {
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    } else {
        tracing_subscriber::fmt().init();
    }
}
