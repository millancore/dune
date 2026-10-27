#![allow(irrefutable_let_patterns)]

mod config;
mod cursor;
mod desktops;
mod dock;
mod floating;
mod gamma;
mod handlers;
mod input;
mod ipc;
mod launcher;
mod layout;
mod lock;
mod privacy;
mod reload;
mod rounded;
mod render;
mod screencopy;
mod scratchpad;
mod screenshot;
mod shell;
mod sound;
mod state;
mod tab_title;
mod tiling;
mod udev;
mod windows;
mod winit;
mod wlr_screencopy;
mod workspaces;
mod xwayland;

use std::path::{Path, PathBuf};

use smithay::reexports::{calloop::EventLoop, wayland_server::Display};
pub use state::Dune;

const USAGE: &str = "\
usage:
  dune [--tty|--nested]                  start the compositor: on a TTY as the session, inside
                                         another session as a window (--tty/--nested to force)
  dune open [--split v|h] [--from TILE] [--cwd DIR] [--] COMMAND...
                                         open COMMAND in a new terminal tile
  dune scratchpad show|watch             show the next scratchpad window (or hide the focused
                                         one) / print a waybar JSON line on every change
  dune dock show|watch                   show the dock of the bar's monitor ($WAYBAR_OUTPUT_NAME)
                                         / print a waybar JSON line on every change
  dune recording stop|watch              stop the recording running / print a waybar JSON line
                                         on every change (with the time recorded)
  dune shell                             run the dune layer around $DUNE_USER_SHELL (what tiles run)";

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
        None => run(None).map_err(|e| e.to_string()),
        Some("--tty") => run(Some(Mode::Tty)).map_err(|e| e.to_string()),
        Some("--nested") => run(Some(Mode::Nested)).map_err(|e| e.to_string()),
        Some("open") => ipc::open_command(&args[1..]),
        Some("scratchpad") => ipc::scratchpad_command(&args[1..]),
        Some("dock") => ipc::dock_command(&args[1..]),
        Some("recording") => ipc::recording_command(&args[1..]),
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A real session: dune drives the screens and input devices.
    Tty,
    /// A window inside another Wayland or X11 session.
    Nested,
}

fn run(mode: Option<Mode>) -> Result<(), Box<dyn std::error::Error>> {
    // Inside another session there is a display to connect to; on a TTY there is not.
    let inside_session = std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some();
    let mode = mode.unwrap_or(if inside_session { Mode::Nested } else { Mode::Tty });

    init_logging();

    let mut event_loop: EventLoop<Dune> = EventLoop::try_new()?;
    let display: Display<Dune> = Display::new()?;
    let mut state = Dune::new(&mut event_loop, display);

    match mode {
        Mode::Nested => crate::winit::init_winit(&mut event_loop, &mut state)?,
        Mode::Tty => crate::udev::init_udev(&mut event_loop, &mut state)?,
    }

    let name = state.socket_name.to_string_lossy().into_owned();
    let socket = ipc::socket_path(&name);
    ipc::listen(&mut event_loop, &socket)?;
    reload::watch(&event_loop.handle());
    sound::watch(&event_loop.handle());

    state.user_shell = user_shell();
    state.shell_wrapper = match install_shell_link(&socket.with_extension("shell")) {
        Ok(link) => Some(link),
        Err(err) => {
            tracing::error!("cannot set up the : layer ({err}); tiles will run the plain shell");
            None
        }
    };

    // Children connect to us, not to the host compositor, and find the control socket.
    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);
        std::env::set_var("DUNE_SOCKET", &socket);
        std::env::remove_var("DISPLAY");
        // `dune ...` from a bar or a script finds this binary, even when its folder
        // (e.g. ~/.cargo/bin) is only on the PATH of interactive shells.
        if let Some(path) = path_with_own_folder() {
            std::env::set_var("PATH", path);
        }
        // Clients pick their own cursor size; without these, X11 apps (libXcursor)
        // derive it from the screen size and draw a huge pointer.
        if std::env::var_os("XCURSOR_THEME").is_none() {
            std::env::set_var("XCURSOR_THEME", "default");
        }
        if std::env::var_os("XCURSOR_SIZE").is_none() {
            std::env::set_var("XCURSOR_SIZE", "24");
        }
    }
    tracing::info!("listening on {name:?}, control socket {socket:?}, shell {:?}", state.user_shell);

    // X11 applications go through xwayland-satellite, like in niri.
    // satellite starts with the first X11 client (and again if it exits).
    let xwayland = match state.config.xwayland.as_str() {
        "off" => None,
        _ => xwayland::start(&event_loop.handle()),
    };
    if let Some(x_display) = &xwayland {
        unsafe { std::env::set_var("DISPLAY", x_display) };
        tracing::info!("X11 apps on DISPLAY={x_display} (xwayland-satellite, on demand)");
    }

    // As the session, tell systemd and D-Bus where the desktop is, so services they
    // start on demand (notifications, portals) connect to dune. Never when nested:
    // that would point the host session's services at us.
    if mode == Mode::Tty {
        unsafe {
            std::env::set_var("XDG_CURRENT_DESKTOP", "dune");
            std::env::set_var("XDG_SESSION_TYPE", "wayland");
        }
        let vars = ["WAYLAND_DISPLAY", "DISPLAY", "XDG_CURRENT_DESKTOP", "XDG_SESSION_TYPE", "DUNE_SOCKET", "XCURSOR_THEME", "XCURSOR_SIZE"];
        match std::process::Command::new("dbus-update-activation-environment").arg("--systemd").args(vars).status() {
            Ok(status) if status.success() => {}
            other => tracing::warn!("could not export the session environment to systemd/D-Bus: {other:?}"),
        }
        // Portals left from another session would keep its environment and backends
        // (screen sharing for niri, sway...): stopped, D-Bus starts them again for dune.
        let stop = ["--user", "stop", "xdg-desktop-portal.service", "xdg-desktop-portal-wlr.service", "xdg-desktop-portal-gnome.service"];
        match std::process::Command::new("systemctl").args(stop).spawn() {
            // Waited for on the side: stopping may take a moment, startup does not wait.
            Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
            Err(err) => tracing::warn!("could not restart the portals: {err}"),
        }
    }

    // `exec` lines of the config: bars, notification daemons...
    for command in &state.config.exec {
        autostart(command);
    }

    event_loop.run(None, &mut state, |_| {})?;

    let _ = std::fs::remove_file(&socket);
    if let Some(link) = &state.shell_wrapper {
        let _ = std::fs::remove_dir_all(link.parent().unwrap());
    }
    Ok(())
}

/// Run a command of the config (an `exec` line, `lock_command`) through the shell, on its own.
pub fn autostart(command: &str) {
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.arg("-c").arg(command).env_remove("DUNE_TILE").env_remove("DUNE_RUN").env_remove("DUNE_LAUNCH");
    if let Some(home) = std::env::var_os("HOME") {
        cmd.current_dir(home);
    }
    match cmd.spawn() {
        Ok(mut child) => {
            tracing::info!("started `{command}`");
            // Reap it when it exits so it doesn't linger as a zombie.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(err) => tracing::warn!("cannot start `{command}`: {err}"),
    }
}

/// `PATH` with the folder of the running binary first, when it is not on it already.
fn path_with_own_folder() -> Option<std::ffi::OsString> {
    let folder = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let path = std::env::var_os("PATH").unwrap_or_default();
    if std::env::split_paths(&path).any(|p| p == folder) {
        return None;
    }
    std::env::join_paths(std::iter::once(folder).chain(std::env::split_paths(&path))).ok()
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

/// The path of the dune binary. Reinstalled while it runs, Linux names the running one
/// `<path> (deleted)`: the new one at `<path>` is meant.
pub fn own_binary() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(match exe.to_str().and_then(|p| p.strip_suffix(" (deleted)")) {
        Some(path) => PathBuf::from(path),
        None => exe,
    })
}

/// Create `<dir>/dune-shell` pointing at this binary.
fn install_shell_link(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let link = dir.join(shell::ARGV0);
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(own_binary()?, &link)?;
    Ok(link)
}

/// The log goes to stderr: the session script keeps it in session.log, and stdout of a
/// TTY session goes nowhere.
fn init_logging() {
    use std::io::IsTerminal;
    let fmt = tracing_subscriber::fmt().with_writer(std::io::stderr).with_ansi(std::io::stderr().is_terminal());
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        fmt.with_env_filter(env_filter).init();
    } else {
        fmt.init();
    }
}
