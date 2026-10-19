//! The shell wrapper: dune's `@` layer for terminals other than dune-term.
//!
//! Terminal tiles are started with `SHELL` pointing at dune (as `dune-shell`), so any
//! terminal emulator runs this instead of the user's shell. It starts the real shell
//! (`DUNE_USER_SHELL`) on a new PTY and copies bytes both ways, passing what is typed
//! through the shared `@` layer (`dune_common::at`) and drawing its picker with escape
//! codes. dune-term does the same natively and runs the shell itself.

use std::{
    ffi::{CString, OsString},
    io::Write,
    os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use dune_common::{
    at::AtLayer,
    picker::{MAX_ROWS, Picker},
};
use nix::{
    errno::Errno,
    poll::{PollFd, PollFlags, PollTimeout, poll},
    pty::{ForkptyResult, Winsize, forkpty},
    sys::{
        signal::{SigSet, Signal},
        signalfd::{SfdFlags, SignalFd},
        termios::{self, SetArg},
        wait::{WaitPidFlag, WaitStatus, waitpid},
    },
    unistd::{Pid, execv, read, tcgetpgrp, write},
};

/// Name the wrapper is started under (a symlink to the dune binary).
pub const ARGV0: &str = "dune-shell";

/// Run as the terminal's shell. Never returns.
pub fn run(argv0: &str, args: Vec<OsString>) -> ! {
    let real_shell = real_shell();
    let login = argv0.starts_with('-');
    let shell_argv0 = format!(
        "{}{}",
        if login { "-" } else { "" },
        Path::new(&real_shell).file_name().map_or("sh".into(), |n| n.to_string_lossy())
    );

    let stdin = std::io::stdin();
    let Ok(saved) = termios::tcgetattr(stdin.as_fd()) else {
        // Not on a terminal: nothing to intercept.
        exec_shell(&real_shell, &shell_argv0, &args);
    };

    // The layer reads DUNE_RUN; the shell must not see it, and must see its own SHELL.
    let winsize = get_winsize(stdin.as_fd());
    let (child, master) = match unsafe { forkpty(Some(&winsize), Some(&saved)) } {
        Ok(ForkptyResult::Child) => {
            unsafe {
                std::env::remove_var("DUNE_RUN");
                std::env::set_var("SHELL", &real_shell);
            }
            exec_shell(&real_shell, &shell_argv0, &args)
        }
        Ok(ForkptyResult::Parent { child, master }) => (child, master),
        Err(err) => {
            eprintln!("dune-shell: cannot create a pty ({err}), starting the shell directly");
            exec_shell(&real_shell, &shell_argv0, &args);
        }
    };

    let mut raw = saved.clone();
    termios::cfmakeraw(&mut raw);
    let _ = termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &raw);

    let at = AtLayer::from_env(child.as_raw() as u32);
    let code = Wrapper { child, master, at, drawn: false }.run();

    let _ = termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &saved);
    std::process::exit(code);
}

/// The shell to start: `DUNE_USER_SHELL`, unless missing or itself a wrapper.
pub fn real_shell() -> PathBuf {
    std::env::var_os("DUNE_USER_SHELL")
        .map(PathBuf::from)
        .filter(|s| s.file_name().is_some_and(|n| n != ARGV0) && s.exists())
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

fn exec_shell(shell: &Path, argv0: &str, args: &[OsString]) -> ! {
    let path = CString::new(shell.as_os_str().as_bytes()).unwrap();
    let mut argv = vec![CString::new(argv0).unwrap()];
    argv.extend(args.iter().filter_map(|a| CString::new(a.as_bytes()).ok()));
    let err = execv(&path, &argv).unwrap_err();
    eprintln!("dune-shell: cannot run {}: {err}", shell.display());
    std::process::exit(127);
}

fn get_winsize(fd: BorrowedFd) -> Winsize {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    unsafe { libc::ioctl(fd.as_raw_fd(), libc::TIOCGWINSZ, &mut ws) };
    Winsize {
        ws_row: ws.ws_row,
        ws_col: ws.ws_col,
        ws_xpixel: ws.ws_xpixel,
        ws_ypixel: ws.ws_ypixel,
    }
}

fn set_winsize(fd: BorrowedFd, ws: &Winsize) {
    let ws = libc::winsize {
        ws_row: ws.ws_row,
        ws_col: ws.ws_col,
        ws_xpixel: ws.ws_xpixel,
        ws_ypixel: ws.ws_ypixel,
    };
    unsafe { libc::ioctl(fd.as_raw_fd(), libc::TIOCSWINSZ, &ws) };
}

fn write_all(fd: BorrowedFd, mut data: &[u8]) {
    while !data.is_empty() {
        match write(fd, data) {
            Ok(n) => data = &data[n..],
            Err(Errno::EINTR) | Err(Errno::EAGAIN) => continue,
            Err(_) => return,
        }
    }
}

struct Wrapper {
    child: Pid,
    master: OwnedFd,
    at: AtLayer,
    /// The picker is on screen (rows reserved below the prompt).
    drawn: bool,
}

impl Wrapper {
    /// Main loop; returns the shell's exit code.
    fn run(&mut self) -> i32 {
        let mut signals = SigSet::empty();
        signals.add(Signal::SIGWINCH);
        signals.add(Signal::SIGCHLD);
        let _ = signals.thread_block();
        let sigfd = SignalFd::with_flags(&signals, SfdFlags::SFD_NONBLOCK).ok();

        let stdin = std::io::stdin();
        let mut buf = [0u8; 8192];
        loop {
            let to_shell = self.at.tick(self.at_prompt());
            write_all(self.master.as_fd(), &to_shell);

            let mut fds = vec![
                PollFd::new(stdin.as_fd(), PollFlags::POLLIN),
                PollFd::new(self.master.as_fd(), PollFlags::POLLIN),
            ];
            if let Some(sigfd) = &sigfd {
                fds.push(PollFd::new(sigfd.as_fd(), PollFlags::POLLIN));
            }
            let timeout: u16 = if self.at.waiting() { 50 } else { 500 };
            match poll(&mut fds, PollTimeout::from(timeout)) {
                Ok(_) | Err(Errno::EINTR) => {}
                Err(_) => break,
            }
            let ready = |fd: &PollFd| fd.revents().is_some_and(|r| !r.is_empty());
            let (input_ready, output_ready) = (ready(&fds[0]), ready(&fds[1]));
            let signal_ready = fds.get(2).is_some_and(ready);
            drop(fds);

            if output_ready {
                match read(self.master.as_fd(), &mut buf) {
                    Ok(0) | Err(Errno::EIO) => break,
                    Ok(n) => {
                        self.at.output_seen();
                        let mut out = std::io::stdout().lock();
                        let _ = out.write_all(&buf[..n]);
                        let _ = out.flush();
                    }
                    Err(_) => {}
                }
            }
            if input_ready {
                match read(stdin.as_fd(), &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let at_prompt = self.at_prompt();
                        let to_shell = self.at.input(&buf[..n], at_prompt);
                        self.redraw_picker();
                        write_all(self.master.as_fd(), &to_shell);
                    }
                    Err(_) => {}
                }
            }
            if signal_ready && let Some(sigfd) = &sigfd {
                while let Ok(Some(_)) = sigfd.read_signal() {}
                set_winsize(self.master.as_fd(), &get_winsize(stdin.as_fd()));
                self.redraw_picker();
                if let Ok(WaitStatus::Exited(..) | WaitStatus::Signaled(..)) =
                    waitpid(self.child, Some(WaitPidFlag::WNOHANG))
                {
                    // Flush what the shell printed last, then leave.
                    while let Ok(n @ 1..) = read(self.master.as_fd(), &mut buf) {
                        let _ = std::io::stdout().write_all(&buf[..n]);
                    }
                    let _ = std::io::stdout().flush();
                    return 0;
                }
            }
        }
        match waitpid(self.child, None) {
            Ok(WaitStatus::Exited(_, code)) => code,
            _ => 0,
        }
    }

    fn at_prompt(&self) -> bool {
        tcgetpgrp(self.master.as_fd()).is_ok_and(|pgrp| pgrp == self.child)
    }

    /// Bring the screen in line with the picker: open, update or clear it.
    fn redraw_picker(&mut self) {
        let mut out = std::io::stdout().lock();
        match self.at.picker() {
            Some(picker) => {
                if !self.drawn {
                    // Make room below the prompt (scrolling if needed), then remember the spot.
                    let rows = MAX_ROWS + 1;
                    let _ = write!(out, "{}\x1b[{rows}A\x1b7", "\n".repeat(rows));
                    self.drawn = true;
                }
                let cols = (get_winsize(std::io::stdin().as_fd()).ws_col as usize).max(20);
                let _ = out.write_all(draw(picker, cols).as_bytes());
            }
            None if self.drawn => {
                let _ = out.write_all(b"\x1b8\x1b[J");
                self.drawn = false;
            }
            None => {}
        }
        let _ = out.flush();
    }
}

/// The picker as escape codes, drawn from the saved cursor position (the prompt).
fn draw(picker: &Picker, cols: usize) -> String {
    let fit = |s: &str, width: usize| s.chars().take(width).collect::<String>();

    let mut s = String::from("\x1b8\x1b[J");
    let line = format!("@{}", picker.input());
    s += &format!("\x1b[1m{line}\x1b[0m");
    let (first, rows) = picker.window();
    for (i, c) in rows.iter().enumerate() {
        let label = fit(&c.label, cols.saturating_sub(4));
        if first + i == picker.selected() {
            s += &format!("\r\n  \x1b[7m {label} \x1b[0m");
        } else {
            s += &format!("\r\n   {label}");
        }
    }
    s += &format!("\r\n\x1b[2m  {}\x1b[0m", fit(dune_common::picker::HINT, cols.saturating_sub(2)));
    // Back to the end of the @ line.
    s += "\x1b8";
    let width = line.chars().count();
    if width > 0 {
        s += &format!("\x1b[{width}C");
    }
    s
}
