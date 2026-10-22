//! X11 apps through xwayland-satellite, started on demand as in niri: dune reserves an X
//! display (its lock file and sockets) and hands the sockets to satellite (`-listenfd`) when
//! the first X11 client connects. When satellite exits or crashes, the next X11 client
//! starts it again, on the same DISPLAY.

use std::{
    cell::RefCell,
    fs::{File, OpenOptions, Permissions},
    io::{ErrorKind, Write},
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::{
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
            net::UnixListener,
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
};

use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction, RegistrationToken, channel, generic::Generic};

use crate::Dune;

const SATELLITE: &str = "xwayland-satellite";

/// A file removed when dropped: the display's lock file and socket.
struct Unlink(PathBuf);

impl Drop for Unlink {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// An X display reserved by dune: its listening sockets (the path one, then on Linux the
/// abstract one) and, removed with it, its lock file and socket file.
struct Display {
    name: String,
    sockets: Vec<OwnedFd>,
    _files: [Unlink; 2],
}

struct Satellite {
    display: Display,
    /// satellite's stderr (and Xwayland's), appended to on every start.
    log: Option<PathBuf>,
    /// The watches on `display.sockets`, by index, while satellite is not running.
    watches: Vec<Option<RegistrationToken>>,
    /// Told when satellite exits: watch the sockets again.
    exited: channel::Sender<()>,
}

/// Reserve an X display for X11 apps, if xwayland-satellite can be started on demand:
/// its name (`:1`) for DISPLAY.
pub fn start(handle: &LoopHandle<'static, Dune>) -> Option<String> {
    if !supports_listenfd() {
        tracing::warn!("X11 apps unavailable: {SATELLITE} is missing or older than 0.7 (no -listenfd)");
        return None;
    }
    let display = match reserve(Path::new("/tmp")) {
        Ok(display) => display,
        Err(err) => {
            tracing::warn!("X11 apps unavailable: cannot reserve an X display ({err})");
            return None;
        }
    };
    let name = display.name.clone();
    // Its log goes to a file of its own, next to the control socket.
    let log = std::env::var_os("XDG_RUNTIME_DIR")
        .map(|d| Path::new(&d).join(format!("dune-xwayland{name}.log")))
        .filter(|p| File::create(p).is_ok());

    let (exited, rx) = channel::channel();
    let satellite = Rc::new(RefCell::new(Satellite { watches: vec![None; display.sockets.len()], display, log, exited }));
    let (h, s) = (handle.clone(), satellite.clone());
    if let Err(err) = handle.insert_source(rx, move |event, _, _| {
        if let channel::Event::Msg(()) = event {
            watch(&h, &s);
        }
    }) {
        tracing::warn!("X11 apps unavailable: {err}");
        return None;
    }
    watch(handle, &satellite);
    Some(name)
}

/// Whether satellite takes its sockets from dune (`-listenfd`, since 0.7).
fn supports_listenfd() -> bool {
    Command::new(SATELLITE)
        .args([":0", "--test-listenfd-support"])
        .env_remove("DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The first free X display under `root` (`/tmp`): its lock file `.X<n>-lock` (with our
/// pid) taken atomically, then its sockets bound.
fn reserve(root: &Path) -> std::io::Result<Display> {
    let dir = root.join(".X11-unix");
    ensure_x11_dir(root, &dir)?;
    for n in 0..64 {
        let lock_path = root.join(format!(".X{n}-lock"));
        let Ok(mut file) = OpenOptions::new().write(true).create_new(true).mode(0o444).open(&lock_path) else {
            continue;
        };
        let lock = Unlink(lock_path);
        file.write_all(format!("{:>10}\n", std::process::id()).as_bytes())?;
        drop(file);
        match bind(&dir, n) {
            Ok((sockets, socket)) => return Ok(Display { name: format!(":{n}"), sockets, _files: [lock, socket] }),
            Err(err) => tracing::debug!("X display :{n} taken: {err}"),
        }
    }
    Err(std::io::Error::other("no free X display"))
}

/// `/tmp/.X11-unix` exists, writable by all with the sticky bit, owned by root (as `/tmp`)
/// or by us. Adapted from niri (itself from Mutter).
fn ensure_x11_dir(root: &Path, dir: &Path) -> std::io::Result<()> {
    match std::fs::create_dir(dir) {
        Ok(()) => return std::fs::set_permissions(dir, Permissions::from_mode(0o1777)),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err),
    }
    let meta = std::fs::symlink_metadata(dir)?;
    let owner_ok = meta.uid() == std::fs::symlink_metadata(root)?.uid() || meta.uid() == unsafe { libc::getuid() };
    if !owner_ok || meta.mode() & 0o022 != 0o022 || meta.mode() & 0o1000 == 0 {
        return Err(std::io::Error::other(format!("{} has the wrong owner or permissions", dir.display())));
    }
    Ok(())
}

/// The sockets of display `n`: `<dir>/X<n>`, and on Linux the abstract one of that name.
fn bind(dir: &Path, n: u32) -> std::io::Result<(Vec<OwnedFd>, Unlink)> {
    let path = dir.join(format!("X{n}"));
    #[cfg(target_os = "linux")]
    let abstract_socket = {
        use std::os::{linux::net::SocketAddrExt, unix::ffi::OsStrExt, unix::net::SocketAddr};
        let addr = SocketAddr::from_abstract_name(path.as_os_str().as_bytes())?;
        UnixListener::bind_addr(&addr)?
    };
    // The lock is ours: a socket file left there belongs to no running server.
    let _ = std::fs::remove_file(&path);
    let socket = UnixListener::bind(&path)?;
    let unlink = Unlink(path);
    let mut sockets = vec![OwnedFd::from(socket)];
    #[cfg(target_os = "linux")]
    sockets.push(OwnedFd::from(abstract_socket));
    Ok((sockets, unlink))
}

/// Wait for an X11 client on any of the sockets.
fn watch(handle: &LoopHandle<'static, Dune>, satellite: &Rc<RefCell<Satellite>>) {
    let mut s = satellite.borrow_mut();
    for i in 0..s.display.sockets.len() {
        if let Some(token) = s.watches[i].take() {
            handle.remove(token);
        }
        let fd = match s.display.sockets[i].try_clone() {
            Ok(fd) => drop_pending(fd),
            Err(err) => {
                tracing::warn!("cannot watch the X11 socket: {err}");
                continue;
            }
        };
        let (h, sat) = (handle.clone(), satellite.clone());
        match handle.insert_source(Generic::new(fd, Interest::READ, Mode::Level), move |_, _, _| {
            spawn(&h, &sat, i);
            Ok(PostAction::Remove)
        }) {
            Ok(token) => s.watches[i] = Some(token),
            Err(err) => tracing::warn!("cannot watch the X11 socket: {err}"),
        }
    }
}

/// Accept and drop the connections waiting on `fd`. When satellite failed to start, the
/// client that triggered it is still queued, and would start it again and again.
fn drop_pending(fd: OwnedFd) -> OwnedFd {
    let listener = UnixListener::from(fd);
    if listener.set_nonblocking(true).is_ok() {
        while listener.accept().is_ok() {}
        let _ = listener.set_nonblocking(false);
    }
    OwnedFd::from(listener)
}

/// An X11 client connected on socket `fired`: start satellite with the sockets (it accepts
/// that client), from a thread that waits for it to exit.
fn spawn(handle: &LoopHandle<'static, Dune>, satellite: &Rc<RefCell<Satellite>>, fired: usize) {
    let mut s = satellite.borrow_mut();
    // The source that fired removes itself (`PostAction::Remove`).
    for (i, token) in s.watches.iter_mut().enumerate() {
        if let Some(token) = token.take()
            && i != fired
        {
            handle.remove(token);
        }
    }
    let exited = s.exited.clone();
    let fds: Vec<OwnedFd> = match s.display.sockets.iter().map(OwnedFd::try_clone).collect() {
        Ok(fds) => fds,
        Err(err) => {
            tracing::warn!("cannot start {SATELLITE}: {err}");
            let _ = exited.send(());
            return;
        }
    };
    let mut command = Command::new(SATELLITE);
    command
        .arg(&s.display.name)
        .env_remove("DISPLAY")
        .env_remove("RUST_BACKTRACE")
        .env_remove("RUST_LIB_BACKTRACE")
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    match s.log.as_ref().and_then(|p| OpenOptions::new().append(true).create(true).open(p).ok()) {
        Some(log) => command.stderr(log),
        None => command.stderr(Stdio::null()),
    };
    let raw: Vec<i32> = fds.iter().map(|fd| fd.as_raw_fd()).collect();
    for fd in &raw {
        command.arg("-listenfd").arg(fd.to_string());
    }
    // Our copies are close-on-exec: the ones satellite gets must not be.
    unsafe {
        command.pre_exec(move || {
            for fd in &raw {
                if libc::fcntl(*fd, libc::F_SETFD, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    tracing::info!("X11 client on {}: starting {SATELLITE}", s.display.name);
    let started = std::thread::Builder::new().name("xwayland-satellite".into()).spawn(move || {
        match command.spawn() {
            Ok(mut child) => {
                drop(fds);
                match child.wait() {
                    Ok(status) => tracing::warn!("{SATELLITE} exited ({status}); the next X11 app starts it again"),
                    Err(err) => tracing::warn!("cannot wait for {SATELLITE}: {err}"),
                }
            }
            Err(err) => tracing::warn!("cannot start {SATELLITE}: {err}"),
        }
        let _ = exited.send(());
    });
    if let Err(err) = started {
        tracing::warn!("cannot start {SATELLITE}: {err}");
        let _ = s.exited.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_the_first_free_display_and_frees_it() {
        let root = std::env::temp_dir().join(format!("dune-x11-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let a = reserve(&root).unwrap();
        let b = reserve(&root).unwrap();
        assert_eq!((a.name.as_str(), b.name.as_str()), (":0", ":1"));
        assert!(root.join(".X0-lock").exists() && root.join(".X11-unix/X0").exists());
        let pid: u32 = std::fs::read_to_string(root.join(".X0-lock")).unwrap().trim().parse().unwrap();
        assert_eq!(pid, std::process::id());
        assert_eq!(std::fs::metadata(root.join(".X11-unix")).unwrap().mode() & 0o7777, 0o1777);

        drop(a);
        assert!(!root.join(".X0-lock").exists() && !root.join(".X11-unix/X0").exists());
        // A lock someone else holds is skipped. (Not retaking :0 here: a process another
        // test forks may hold its abstract socket until it execs.)
        std::fs::write(root.join(".X0-lock"), "1\n").unwrap();
        assert_eq!(reserve(&root).unwrap().name, ":2");
        drop(b);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
