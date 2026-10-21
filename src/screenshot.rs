//! Screenshots and screen recordings by keys, with the usual tools: slurp chooses a
//! region, grim takes it (`crate::screencopy`), wl-copy puts it on the clipboard,
//! wf-recorder records it (`crate::wlr_screencopy`) and notify-send tells where the file
//! went. They run in a shell of their own, so dune never waits on them; Esc in slurp
//! cancels, and nothing happens.

use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use nix::{sys::signal, unistd::Pid};

use crate::Dune;

/// The folder a capture goes in: `$1` (from the config, `~/` for the home folder), else
/// `$XDG_<kind>_DIR` (`xdg-user-dir`, which says `$HOME` for one not set up) with `$3`
/// after it, else `~/$2`. `$4` is the file's extension; `$file` is a name in it, with
/// the time, and the folder is made.
const FILE: &str = r#"
dir=$1
case $dir in "~/"*) dir=$HOME/${dir#"~/"} ;; esac
if [ -z "$dir" ]; then
    eval "base=\${XDG_$(echo "$2" | tr a-z A-Z)_DIR:-}"
    [ -n "$base" ] || base=$(xdg-user-dir "$(echo "$2" | tr a-z A-Z)" 2>/dev/null)
    [ -n "$base" ] && [ "$base" != "$HOME" ] || base=$HOME/$2
    dir=$base$3
fi
mkdir -p "$dir" || exit 1
file=$dir/dune-$(date +%Y-%m-%d_%H-%M-%S).$4
"#;

/// A screenshot: of the region chosen with slurp, or of the monitor `$5` when given.
const SCREENSHOT: &str = r#"
if [ -n "$5" ]; then
    set -- "$1" "$2" "$3" "$4" -o "$5"
else
    region=$(slurp) || exit 0
    set -- "$1" "$2" "$3" "$4" -g "$region"
fi
#FILE
shift 4
if grim "$@" "$file"; then
    wl-copy --type image/png < "$file"
    notify-send -a dune -i "$file" "Screenshot saved" "$file"
else
    notify-send -a dune -u critical "Screenshot failed"
fi
"#;

/// A recording of the region chosen with slurp, until SIGINT reaches the process group
/// (the `record` key again): wf-recorder finishes the file, then the shell tells.
const RECORD: &str = r#"
trap 'stopped=1' INT
region=$(slurp) || exit 0
[ -z "$stopped" ] || exit 0
#FILE
notify-send -a dune "Recording" "$file"
wf-recorder -g "$region" -f "$file" > /dev/null 2>&1 < /dev/null
if [ -s "$file" ]; then
    notify-send -a dune "Recording saved" "$file"
else
    notify-send -a dune -u critical "Recording failed"
fi
"#;

/// The recording while its shell runs (`record` key).
pub struct Recording {
    /// The shell, which leads its process group (slurp, wf-recorder).
    pid: u32,
    /// The shell exited (and was reaped).
    done: Arc<AtomicBool>,
}

/// `/bin/sh` running `script` (with `FILE` where it says `#FILE`) with `args` as `$1`...,
/// in a process group of its own, from the home folder, without the variables of a tile.
fn capture_command(script: &str, args: &[&str]) -> Command {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(script.replace("#FILE", FILE)).arg("dune-capture").args(args);
    cmd.env_remove("DUNE_TILE").env_remove("DUNE_RUN").env_remove("DUNE_LAUNCH");
    cmd.stdin(Stdio::null()).process_group(0);
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from).filter(|d| d.is_dir()) {
        cmd.current_dir(home);
    }
    cmd
}

/// Start `cmd`, reaped when it exits; `done` is set then.
fn start(mut cmd: Command, what: &str, done: Arc<AtomicBool>) -> Option<u32> {
    match cmd.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            std::thread::spawn(move || {
                let _ = child.wait();
                done.store(true, Ordering::Relaxed);
            });
            Some(pid)
        }
        Err(err) => {
            tracing::error!("cannot start the {what}: {err}");
            None
        }
    }
}

impl Dune {
    /// `screenshot`: a region chosen with the mouse; `screenshot_screen` (`whole`): the
    /// active monitor. Saved in `screenshot_dir` and put on the clipboard.
    pub fn screenshot(&mut self, whole: bool) {
        let output = match whole {
            true => match self.active_output() {
                Some(output) => output.name(),
                None => return tracing::warn!("no monitor to take a screenshot of"),
            },
            false => String::new(),
        };
        let dir = self.config.screenshot_dir.clone();
        let cmd = capture_command(SCREENSHOT, &[&dir, "Pictures", "/Screenshots", "png", &output]);
        start(cmd, "screenshot", Arc::default());
    }

    /// `record`: record a region chosen with the mouse into `recording_dir`; pressed
    /// again (while choosing too), stop. One recording at a time.
    pub fn toggle_recording(&mut self) {
        if let Some(recording) = self.recording.take_if(|r| !r.done.load(Ordering::Relaxed)) {
            let _ = signal::killpg(Pid::from_raw(recording.pid as i32), signal::Signal::SIGINT);
            return;
        }
        let dir = self.config.recording_dir.clone();
        let done = Arc::new(AtomicBool::new(false));
        let cmd = capture_command(RECORD, &[&dir, "Videos", "", "mp4"]);
        self.recording = start(cmd, "recording", done.clone()).map(|pid| Recording { pid, done });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path, time::Duration};

    /// A folder for one test, with `bin/` holding fake tools that log their arguments.
    fn sandbox(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dune-screenshot-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).unwrap();
        dir
    }

    fn tool(dir: &Path, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("bin").join(name);
        fs::write(&path, format!("#!/bin/sh\necho \"{name} $*\" >> \"{}/log\"\n{body}\n", dir.display())).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn run(dir: &Path, script: &str, args: &[&str]) -> Command {
        let mut cmd = capture_command(script, args);
        let path = format!("{}/bin:/usr/bin:/bin", dir.display());
        cmd.env("PATH", path).env("HOME", dir).env_remove("XDG_PICTURES_DIR").env_remove("XDG_VIDEOS_DIR");
        cmd.current_dir(dir);
        cmd
    }

    fn log(dir: &Path) -> String {
        fs::read_to_string(dir.join("log")).unwrap_or_default()
    }

    /// The one file in `dir`, which must be named like `dune-2026-10-21_09-30-00.EXT`.
    fn saved(dir: &Path, ext: &str) -> PathBuf {
        let files: Vec<PathBuf> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(files.len(), 1, "{files:?}");
        let name = files[0].file_name().unwrap().to_str().unwrap().to_string();
        let stamp = name.strip_prefix("dune-").and_then(|n| n.strip_suffix(&format!(".{ext}"))).unwrap();
        assert!(stamp.len() == 19 && stamp.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '_'), "{name}");
        files[0].clone()
    }

    #[test]
    fn screenshot_of_a_region() {
        let dir = sandbox("region");
        tool(&dir, "slurp", "echo '10,10 200x100'");
        tool(&dir, "grim", "for f; do :; done; echo png > \"$f\"");
        tool(&dir, "wl-copy", "cat > /dev/null");
        tool(&dir, "notify-send", "");
        // xdg-user-dir says $HOME for a folder that is not set up: ~/Pictures then.
        tool(&dir, "xdg-user-dir", "echo \"$HOME\"");
        let status = run(&dir, SCREENSHOT, &["", "Pictures", "/Screenshots", "png", ""]).status().unwrap();
        assert!(status.success());
        let file = saved(&dir.join("Pictures/Screenshots"), "png");
        let log = log(&dir);
        assert!(log.contains(&format!("grim -g 10,10 200x100 {}", file.display())), "{log}");
        assert!(log.contains("wl-copy --type image/png"), "{log}");
        assert!(log.contains(&format!("notify-send -a dune -i {} Screenshot saved", file.display())), "{log}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn screenshot_of_a_monitor_in_the_set_folder() {
        let dir = sandbox("monitor");
        tool(&dir, "slurp", "exit 1");
        tool(&dir, "grim", "for f; do :; done; echo png > \"$f\"");
        tool(&dir, "wl-copy", "cat > /dev/null");
        tool(&dir, "notify-send", "");
        let status = run(&dir, SCREENSHOT, &["~/shots", "Pictures", "/Screenshots", "png", "HDMI-A-1"]).status().unwrap();
        assert!(status.success());
        let file = saved(&dir.join("shots"), "png");
        let log = log(&dir);
        assert!(log.contains(&format!("grim -o HDMI-A-1 {}", file.display())), "{log}");
        assert!(!log.contains("slurp"), "{log}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn escape_in_slurp_cancels() {
        let dir = sandbox("cancel");
        tool(&dir, "slurp", "echo 'selection cancelled' >&2; exit 1");
        tool(&dir, "grim", "");
        tool(&dir, "notify-send", "");
        for (script, kind) in [(SCREENSHOT, "Pictures"), (RECORD, "Videos")] {
            let status = run(&dir, script, &["", kind, "", "x", ""]).status().unwrap();
            assert!(status.success());
        }
        assert_eq!(log(&dir), "slurp \nslurp \n");
        assert!(!dir.join("Pictures").exists() && !dir.join("Videos").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn xdg_folders() {
        let dir = sandbox("xdg");
        tool(&dir, "slurp", "echo '0,0 10x10'");
        tool(&dir, "grim", "for f; do :; done; echo png > \"$f\"");
        tool(&dir, "wl-copy", "cat > /dev/null");
        tool(&dir, "notify-send", "");
        tool(&dir, "xdg-user-dir", "echo \"$HOME/Bilder\"");
        assert!(run(&dir, SCREENSHOT, &["", "Pictures", "/Screenshots", "png", ""]).status().unwrap().success());
        saved(&dir.join("Bilder/Screenshots"), "png");
        // The variable wins over xdg-user-dir.
        let mut cmd = run(&dir, SCREENSHOT, &["", "Pictures", "/Screenshots", "png", ""]);
        cmd.env("XDG_PICTURES_DIR", dir.join("Fotos"));
        assert!(cmd.status().unwrap().success());
        saved(&dir.join("Fotos/Screenshots"), "png");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recording_stops_on_sigint() {
        let dir = sandbox("record");
        tool(&dir, "slurp", "echo '0,0 400x300'");
        tool(&dir, "notify-send", "");
        tool(&dir, "xdg-user-dir", "echo \"$HOME\"");
        // Like wf-recorder: records until SIGINT, then finishes the file.
        tool(
            &dir,
            "wf-recorder",
            &format!("trap 'echo video > \"$4\"; echo stopped >> \"{}/log\"; exit 0' INT\nwhile :; do sleep 0.05; done", dir.display()),
        );
        let done = Arc::new(AtomicBool::new(false));
        let pid = start(run(&dir, RECORD, &["", "Videos", "", "mp4"]), "recording", done.clone()).unwrap();
        let wait_for = |what: &dyn Fn() -> bool| {
            for _ in 0..100 {
                if what() {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            false
        };
        assert!(wait_for(&|| log(&dir).contains("wf-recorder")), "{}", log(&dir));
        assert!(!done.load(Ordering::Relaxed));
        signal::killpg(Pid::from_raw(pid as i32), signal::Signal::SIGINT).unwrap();
        assert!(wait_for(&|| done.load(Ordering::Relaxed)));
        let file = saved(&dir.join("Videos"), "mp4");
        let log = log(&dir);
        assert!(log.contains(&format!("wf-recorder -g 0,0 400x300 -f {}", file.display())), "{log}");
        assert!(log.contains(&format!("notify-send -a dune Recording {}", file.display())), "{log}");
        assert!(log.contains(&format!("stopped\nnotify-send -a dune Recording saved {}", file.display())), "{log}");
        let _ = fs::remove_dir_all(&dir);
    }
}
