//! The folders dune-finder's jump mode (`>`) goes to, ranked by frecency as zoxide does:
//! each visit adds one to a folder's rank, and a rank counts more the more recent its last
//! visit. The compositor records the folders terminals work in, dune-finder the ones it
//! opens and jumps to. Kept in `$XDG_DATA_HOME/dune/jumps`, a `rank\tlast\tpath` line per
//! folder (`last`: seconds since the epoch).
//!
//! A folder is kept by its real path (symlinks resolved), so it is one folder however it was
//! reached; the system's `/proc` and `/tmp` are not kept. The folders above a recorded one, in
//! the home folder, are listed too, under it.

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Past this sum of ranks, every rank is aged (×0.9) and the ones under 1 forgotten.
const MAX_TOTAL: f64 = 10_000.0;
const HOUR: u64 = 3600;
const DAY: u64 = 24 * HOUR;
const WEEK: u64 = 7 * DAY;

#[derive(Debug, Clone, PartialEq)]
pub struct Jump {
    pub path: PathBuf,
    pub rank: f64,
    /// Last visit, in seconds since the epoch.
    pub last: u64,
}

impl Jump {
    /// How high it is listed at `now` (seconds since the epoch).
    pub fn frecency(&self, now: u64) -> f64 {
        let age = now.saturating_sub(self.last);
        let weight = if age < HOUR {
            4.0
        } else if age < DAY {
            2.0
        } else if age < WEEK {
            0.5
        } else {
            0.25
        };
        self.rank * weight
    }
}

fn jumps_file() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("dune").join("jumps"))
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The recorded folders and the ones above them, the highest frecency at `now` first.
pub fn ranked(now: u64) -> Vec<Jump> {
    let jumps = jumps_file().and_then(|f| std::fs::read_to_string(f).ok()).map_or_else(Vec::new, |t| parse(&t));
    let jumps = merged(jumps, real_path);
    let mut jumps = match std::env::var_os("HOME") {
        Some(home) => with_parents(jumps, &real_path(Path::new(&home)), now),
        None => jumps,
    };
    jumps.sort_by(|a, b| b.frecency(now).total_cmp(&a.frecency(now)));
    jumps
}

/// One more visit to the folder `path`, now.
pub fn record(path: &Path) -> std::io::Result<()> {
    let Some(file) = jumps_file() else { return Ok(()) };
    let path = real_path(path);
    if !kept(&path) {
        return Ok(());
    }
    let jumps = std::fs::read_to_string(&file).map_or_else(|_| Vec::new(), |t| parse(&t));
    let mut jumps = merged(jumps, real_path);
    visit(&mut jumps, &path, now());
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Written aside and moved in: the compositor and dune-finder both record.
    let tmp = file.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, format(&jumps))?;
    std::fs::rename(tmp, file)
}

/// `path` with its symlinks resolved; as it is when that cannot be done (it is gone).
fn real_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Whether a folder is worth keeping: not one of the system's `/proc` or `/tmp`.
fn kept(path: &Path) -> bool {
    !path.starts_with("/proc") && !path.starts_with("/tmp")
}

/// `jumps` by their `real` path, one each (the ranks of a folder recorded under two paths
/// added, its latest visit kept), without the ones not `kept`.
fn merged(jumps: Vec<Jump>, real: impl Fn(&Path) -> PathBuf) -> Vec<Jump> {
    let mut out: Vec<Jump> = Vec::with_capacity(jumps.len());
    for jump in jumps {
        let path = real(&jump.path);
        if !kept(&path) {
            continue;
        }
        match out.iter_mut().find(|j| j.path == path) {
            Some(j) => {
                j.rank += jump.rank;
                j.last = j.last.max(jump.last);
            }
            None => out.push(Jump { path, ..jump }),
        }
    }
    out
}

/// `jumps` and the folders above them inside `home` (not `home` itself): such a folder
/// counts half its best one under it, or what it has itself when recorded and that is more.
fn with_parents(mut jumps: Vec<Jump>, home: &Path, now: u64) -> Vec<Jump> {
    let mut parents: Vec<Jump> = Vec::new();
    for jump in &jumps {
        let half = Jump { rank: jump.rank / 2.0, ..jump.clone() };
        for parent in jump.path.ancestors().skip(1).take_while(|p| p.starts_with(home) && *p != home) {
            match parents.iter_mut().find(|p| p.path == parent) {
                Some(p) if p.frecency(now) >= half.frecency(now) => {}
                Some(p) => *p = Jump { path: parent.to_path_buf(), ..half.clone() },
                None => parents.push(Jump { path: parent.to_path_buf(), ..half.clone() }),
            }
        }
    }
    for parent in parents {
        match jumps.iter_mut().find(|j| j.path == parent.path) {
            Some(j) if j.frecency(now) >= parent.frecency(now) => {}
            Some(j) => *j = parent,
            None => jumps.push(parent),
        }
    }
    jumps
}

/// Count a visit to `path` at `now` in `jumps`, ageing them all past `MAX_TOTAL`.
fn visit(jumps: &mut Vec<Jump>, path: &Path, now: u64) {
    match jumps.iter_mut().find(|j| j.path == path) {
        Some(j) => {
            j.rank += 1.0;
            j.last = now;
        }
        None => jumps.push(Jump { path: path.to_path_buf(), rank: 1.0, last: now }),
    }
    if jumps.iter().map(|j| j.rank).sum::<f64>() > MAX_TOTAL {
        for j in jumps.iter_mut() {
            j.rank *= 0.9;
        }
        jumps.retain(|j| j.rank >= 1.0);
    }
}

fn parse(text: &str) -> Vec<Jump> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let rank = parts.next()?.parse().ok()?;
            let last = parts.next()?.parse().ok()?;
            let path = PathBuf::from(parts.next().filter(|p| !p.is_empty())?);
            Some(Jump { path, rank, last })
        })
        .collect()
}

fn format(jumps: &[Jump]) -> String {
    jumps.iter().map(|j| format!("{}\t{}\t{}\n", j.rank, j.last, j.path.display())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_visit_ranks_a_folder_up_and_stamps_it() {
        let mut jumps = Vec::new();
        visit(&mut jumps, Path::new("/srv/app"), 100);
        visit(&mut jumps, Path::new("/tmp"), 200);
        visit(&mut jumps, Path::new("/srv/app"), 300);
        assert_eq!(jumps[0], Jump { path: "/srv/app".into(), rank: 2.0, last: 300 });
        assert_eq!(jumps[1].rank, 1.0);
        assert_eq!(parse(&format(&jumps)), jumps);
    }

    #[test]
    fn recent_visits_count_more() {
        let now = 10 * WEEK;
        let jump = |rank, last| Jump { path: "/a".into(), rank, last };
        assert_eq!(jump(1.0, now - 10).frecency(now), 4.0);
        assert_eq!(jump(1.0, now - 2 * HOUR).frecency(now), 2.0);
        assert_eq!(jump(1.0, now - 2 * DAY).frecency(now), 0.5);
        assert_eq!(jump(8.0, now - 2 * WEEK).frecency(now), 2.0);
    }

    #[test]
    fn ranks_age_past_the_total_and_small_ones_go() {
        let mut jumps = vec![
            Jump { path: "/big".into(), rank: MAX_TOTAL, last: 0 },
            Jump { path: "/small".into(), rank: 1.0, last: 0 },
        ];
        visit(&mut jumps, Path::new("/big"), 1);
        assert_eq!(jumps.len(), 1);
        assert!((jumps[0].rank - (MAX_TOTAL + 1.0) * 0.9).abs() < 1e-6);
    }

    #[test]
    fn a_folder_under_two_paths_is_one_and_the_system_ones_go() {
        // `/home/me/s` is a symlink to `/data/s`.
        let real = |p: &Path| match p.strip_prefix("/home/me/s") {
            Ok(rest) => Path::new("/data/s").join(rest),
            Err(_) => p.to_path_buf(),
        };
        let jumps = vec![
            Jump { path: "/home/me/s/projects".into(), rank: 2.0, last: 10 },
            Jump { path: "/proc/42/fdinfo".into(), rank: 1.0, last: 10 },
            Jump { path: "/data/s/projects".into(), rank: 3.0, last: 20 },
            Jump { path: "/tmp/scratch".into(), rank: 1.0, last: 10 },
        ];
        assert_eq!(merged(jumps, real), vec![Jump { path: "/data/s/projects".into(), rank: 5.0, last: 20 }]);
    }

    #[test]
    fn the_folders_above_one_in_home_are_listed_under_it() {
        let now = 10 * WEEK;
        let jumps = vec![
            Jump { path: "/home/me/data/s/projects/canatec".into(), rank: 4.0, last: now },
            Jump { path: "/home/me/data/s".into(), rank: 1.0, last: now },
            Jump { path: "/srv/app".into(), rank: 1.0, last: now },
        ];
        let mut all = with_parents(jumps, Path::new("/home/me"), now);
        all.sort_by(|a, b| b.frecency(now).total_cmp(&a.frecency(now)).then(a.path.cmp(&b.path)));
        let listed: Vec<(&str, f64)> = all.iter().map(|j| (j.path.to_str().unwrap(), j.rank)).collect();
        assert_eq!(
            listed,
            [
                ("/home/me/data/s/projects/canatec", 4.0),
                // Half the best under it, more than it had itself.
                ("/home/me/data", 2.0),
                ("/home/me/data/s", 2.0),
                ("/home/me/data/s/projects", 2.0),
                // Neither home nor anything out of it.
                ("/srv/app", 1.0),
            ]
        );
    }

    #[test]
    fn broken_lines_are_skipped() {
        let jumps = parse("2\t5\t/a b/c\nnope\n1\tx\t/d\n3\t9\t\n");
        assert_eq!(jumps, vec![Jump { path: "/a b/c".into(), rank: 2.0, last: 5 }]);
    }
}
