//! What the status bar shows: the working directory of the program in the
//! terminal and, inside a git repository, its branch.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Status {
    pub cwd: PathBuf,
    pub branch: Option<String>,
}

/// The status of process `pid` (the PTY's foreground process group, or the shell).
pub fn read(pid: u32) -> Option<Status> {
    let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok()?;
    let branch = git_branch(&cwd);
    Some(Status { cwd, branch })
}

/// The branch checked out in the repository holding `dir`, read from its `HEAD`;
/// a detached `HEAD` gives the commit's short hash.
pub fn git_branch(dir: &Path) -> Option<String> {
    let git_dir = dir.ancestors().find_map(|d| {
        let dot_git = d.join(".git");
        if dot_git.is_dir() {
            return Some(dot_git);
        }
        // Worktrees and submodules: a `.git` file pointing at the real directory.
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let target = text.strip_prefix("gitdir:")?.trim();
        Some(d.join(target))
    })?;
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref:") {
        Some(reference) => {
            let reference = reference.trim();
            Some(reference.strip_prefix("refs/heads/").unwrap_or(reference).to_string())
        }
        None => Some(head.chars().take(7).collect()),
    }
}

/// `path` with the home directory written `~`.
pub fn display_path(path: &Path, home: Option<&Path>) -> String {
    if let Some(rest) = home.and_then(|h| path.strip_prefix(h).ok()) {
        if rest.as_os_str().is_empty() {
            return "~".into();
        }
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

/// `text` cut from the left to at most `max` characters, starting with `…` when cut.
pub fn cut_left(text: &str, max: usize) -> String {
    let len = text.chars().count();
    if len <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let kept: String = text.chars().skip(len - (max - 1)).collect();
    format!("…{kept}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dune-term-status-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn branch_of_a_repository_from_a_subdirectory() {
        let repo = scratch("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src/deep")).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/feature/bar\n").unwrap();
        assert_eq!(git_branch(&repo.join("src/deep")).as_deref(), Some("feature/bar"));

        std::fs::write(repo.join(".git/HEAD"), "6d12a91f00ba2\n").unwrap();
        assert_eq!(git_branch(&repo).as_deref(), Some("6d12a91"));
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn branch_of_a_worktree() {
        let root = scratch("worktree");
        std::fs::create_dir_all(root.join("main/.git/worktrees/wt")).unwrap();
        std::fs::write(root.join("main/.git/worktrees/wt/HEAD"), "ref: refs/heads/wt-branch\n").unwrap();
        std::fs::create_dir_all(root.join("wt")).unwrap();
        std::fs::write(root.join("wt/.git"), "gitdir: ../main/.git/worktrees/wt\n").unwrap();
        assert_eq!(git_branch(&root.join("wt")).as_deref(), Some("wt-branch"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_branch_outside_a_repository() {
        let dir = scratch("plain");
        // The temporary directory itself is not inside a repository.
        assert_eq!(git_branch(&dir), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn home_is_written_as_tilde() {
        let home = Path::new("/home/me");
        assert_eq!(display_path(Path::new("/home/me"), Some(home)), "~");
        assert_eq!(display_path(Path::new("/home/me/src/dune"), Some(home)), "~/src/dune");
        assert_eq!(display_path(Path::new("/home/meow"), Some(home)), "/home/meow");
        assert_eq!(display_path(Path::new("/etc"), None), "/etc");
    }

    #[test]
    fn long_paths_are_cut_from_the_left() {
        assert_eq!(cut_left("~/src", 10), "~/src");
        assert_eq!(cut_left("~/src/dune", 6), "…/dune");
        assert_eq!(cut_left("abc", 0), "");
    }
}
