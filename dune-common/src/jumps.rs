//! The folders dune-finder's jump mode (`>`) goes to, ranked by frecency as zoxide does:
//! each visit adds one to a folder's rank, and a rank counts more the more recent its last
//! visit. The compositor records the folders terminals work in, dune-finder the ones it
//! opens and jumps to. Kept in `$XDG_DATA_HOME/dune/jumps`, a `rank\tlast\tpath` line per
//! folder (`last`: seconds since the epoch).

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

/// The recorded folders, the highest frecency at `now` first.
pub fn ranked(now: u64) -> Vec<Jump> {
    let mut jumps = jumps_file().and_then(|f| std::fs::read_to_string(f).ok()).map_or_else(Vec::new, |t| parse(&t));
    jumps.sort_by(|a, b| b.frecency(now).total_cmp(&a.frecency(now)));
    jumps
}

/// One more visit to the folder `path`, now.
pub fn record(path: &Path) -> std::io::Result<()> {
    let Some(file) = jumps_file() else { return Ok(()) };
    let mut jumps = std::fs::read_to_string(&file).map_or_else(|_| Vec::new(), |t| parse(&t));
    visit(&mut jumps, path, now());
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Written aside and moved in: the compositor and dune-finder both record.
    let tmp = file.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, format(&jumps))?;
    std::fs::rename(tmp, file)
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
    fn broken_lines_are_skipped() {
        let jumps = parse("2\t5\t/a b/c\nnope\n1\tx\t/d\n3\t9\t\n");
        assert_eq!(jumps, vec![Jump { path: "/a b/c".into(), rank: 2.0, last: 5 }]);
    }
}
