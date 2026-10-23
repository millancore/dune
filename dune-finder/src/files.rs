//! The files mode (`#` typed first in the apps): the folders and files of a folder, the
//! home folder first. Folders come first, hidden entries only when the query starts with
//! `.`; typing narrows them, fuzzy. Enter on a folder goes into it, on a file opens it
//! (`xdg-open`); `..` (or Backspace with nothing typed) goes up, and at the home folder
//! back to the apps. A path is typed as in a shell: `~` first goes home, `/` first to the
//! root, a `/` after a name into the highlighted folder (names need not be whole). Mod+Enter
//! opens the terminal file manager (`file_manager`, yazi) at the highlighted folder, or with
//! the highlighted file selected; Ctrl+Enter a terminal in the highlighted folder (on `..`
//! or a file: the one shown). dune hands Mod+Enter to dune-finder while it is open; Super or Alt,
//! whichever Mod is, counts.
//!
//! A folder is read with one `read_dir`, entries told apart by their file type (no `stat`
//! of each, which a network folder would make slow); only symlinks are followed to see
//! whether they lead to a folder.

use std::path::{Path, PathBuf};

use dune_common::protocol::Side;
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};

use crate::{
    icons::Icons,
    panel::{ROWS, Row, View},
};

const FOLDER_ICON: &str = "folder";
const FILE_ICON: &str = "text-x-generic";
const UP_ICON: &str = "go-up";

/// What a key did in the files mode.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Close,
    /// Open this file with its app.
    Open(PathBuf),
    /// Open the file manager at this folder, or with this file selected.
    Manage(PathBuf),
    /// Open a terminal in this folder.
    Terminal(PathBuf),
    /// Backspace with nothing typed at the first folder: back to the apps.
    Apps,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    name: String,
    dir: bool,
}

/// A line of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    /// `..`: the folder above.
    Up,
    Entry(usize),
}

pub struct Files {
    /// Where it started: Backspace there goes back to the apps.
    start: PathBuf,
    dir: PathBuf,
    /// The folder's entries, folders first, then by name.
    entries: Vec<Entry>,
    query: String,
    items: Vec<Item>,
    selected: usize,
    matcher: Matcher,
    /// What the hint calls the file manager.
    hint: String,
    /// A folder was just gone to by typing (`~`, `/`): a `/` next is the path's own, not
    /// the root.
    typed_into: bool,
    home: Option<PathBuf>,
}

impl Files {
    /// Nothing typed: `?` there lists the keys.
    pub fn nothing_typed(&self) -> bool {
        self.query.is_empty()
    }

    /// The files of `start`; `file_manager` is named in the hint.
    pub fn new(start: PathBuf, file_manager: &str) -> Self {
        let name = file_manager
            .split_whitespace()
            .next()
            .unwrap_or(file_manager);
        let mut files = Files {
            dir: start.clone(),
            start,
            entries: Vec::new(),
            query: String::new(),
            items: Vec::new(),
            selected: 0,
            matcher: Matcher::new(Config::DEFAULT),
            hint: format!("Enter: open · Mod+Enter: {name} · Ctrl+Enter: terminal · ⌫: up · Esc: close"),
            typed_into: false,
            home: std::env::var_os("HOME").map(PathBuf::from),
        };
        files.read();
        files
    }

    /// The files of `folder`, gone into by typing `/` after it (in the jump mode): a `/`
    /// next is the path's own, not the root.
    pub fn into(folder: PathBuf, file_manager: &str) -> Self {
        let mut files = Files::new(folder, file_manager);
        files.typed_into = true;
        files
    }

    /// Show the folder `dir`, with nothing typed.
    fn go(&mut self, dir: PathBuf) {
        self.dir = dir;
        self.query.clear();
        self.read();
    }

    fn read(&mut self) {
        self.entries = read_folder(&self.dir);
        self.refilter();
    }

    /// The items matching the query again, best first; ties: names starting with the
    /// query, then folders, then the shorter name (`du`: `dune` before `duenote`).
    fn refilter(&mut self) {
        self.selected = 0;
        let hidden = self.query.starts_with('.');
        let shown = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| hidden || !e.name.starts_with('.'));
        self.items = if self.query.is_empty() {
            let up = self.dir.parent().map(|_| Item::Up);
            up.into_iter()
                .chain(shown.map(|(i, _)| Item::Entry(i)))
                .collect()
        } else {
            let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
            let mut buf = Vec::new();
            let mut scored: Vec<(u32, usize)> = shown
                .filter_map(|(i, e)| {
                    pattern
                        .score(Utf32Str::new(&e.name, &mut buf), &mut self.matcher)
                        .map(|s| (s, i))
                })
                .collect();
            let query = self.query.to_lowercase();
            let entries = &self.entries;
            scored.sort_by_cached_key(|(score, i)| {
                let e = &entries[*i];
                (std::cmp::Reverse(*score), !e.name.to_lowercase().starts_with(&query), !e.dir, e.name.chars().count())
            });
            scored.into_iter().map(|(_, i)| Item::Entry(i)).collect()
        };
    }

    /// The path of `item`.
    fn path(&self, item: Item) -> PathBuf {
        match item {
            Item::Up => self
                .dir
                .parent()
                .map_or_else(|| self.dir.clone(), Path::to_path_buf),
            Item::Entry(i) => self.dir.join(&self.entries[i].name),
        }
    }

    /// Enter on `item`: into a folder, up, or open a file.
    fn choose(&mut self, item: Item) -> Step {
        match item {
            Item::Entry(i) if !self.entries[i].dir => Step::Open(self.path(item)),
            _ => {
                let dir = self.path(item);
                self.go(dir);
                Step::Continue
            }
        }
    }

    /// A key pressed: `sym` with `mods`, typing `text`.
    pub fn feed(&mut self, mods: &Modifiers, sym: Keysym, text: Option<char>) -> Step {
        let typed_into = std::mem::take(&mut self.typed_into);
        let n = self.items.len().max(1);
        let item = self.items.get(self.selected).copied();
        match sym {
            Keysym::Escape => return Step::Close,
            Keysym::Return | Keysym::KP_Enter if mods.ctrl && !mods.logo && !mods.alt => {
                let folder = match item {
                    Some(item @ Item::Entry(i)) if self.entries[i].dir => self.path(item),
                    // `..`, a file or nothing: the folder shown.
                    _ => self.dir.clone(),
                };
                return Step::Terminal(folder);
            }
            Keysym::Return | Keysym::KP_Enter if mods.logo || mods.alt => {
                // `..` (highlighted on entering a folder) or nothing: the folder shown.
                return Step::Manage(match item {
                    Some(item @ Item::Entry(_)) => self.path(item),
                    _ => self.dir.clone(),
                });
            }
            Keysym::Return | Keysym::KP_Enter => {
                return item.map_or(Step::Continue, |item| self.choose(item));
            }
            Keysym::Down | Keysym::Tab => self.selected = (self.selected + 1) % n,
            Keysym::n if mods.ctrl => self.selected = (self.selected + 1) % n,
            Keysym::Up | Keysym::ISO_Left_Tab => self.selected = (self.selected + n - 1) % n,
            Keysym::p if mods.ctrl => self.selected = (self.selected + n - 1) % n,
            Keysym::BackSpace if self.query.is_empty() => {
                if self.dir == self.start || self.dir.parent().is_none() {
                    return Step::Apps;
                }
                return self.choose(Item::Up);
            }
            Keysym::BackSpace => {
                self.query.pop();
                self.refilter();
            }
            Keysym::u if mods.ctrl => {
                self.query.clear();
                self.refilter();
            }
            Keysym::w if mods.ctrl => {
                let kept = self.query.trim_end().rfind(' ').map_or(0, |i| i + 1);
                self.query.truncate(kept);
                self.refilter();
            }
            _ => match text {
                // A slash after a folder's name (or part of it) goes into the highlighted
                // folder, as in a shell.
                Some('/') if !self.query.is_empty() => {
                    if let Some(item @ Item::Entry(i)) = item
                        && self.entries[i].dir
                    {
                        self.typed_into = true;
                        return self.choose(item);
                    }
                }
                // Right after `~` or a folder gone into: the path goes on.
                Some('/') if typed_into => {}
                // A path typed from the start: `/…` from the root, `~/…` from home.
                Some('/') if self.query.is_empty() => {
                    self.typed_into = true;
                    self.go(PathBuf::from("/"));
                }
                Some('~') if self.query.is_empty() => {
                    if let Some(home) = self.home.clone() {
                        self.typed_into = true;
                        self.go(home);
                    }
                }
                Some(c) if !c.is_control() && !mods.ctrl && !mods.alt && !mods.logo => {
                    self.query.push(c);
                    self.refilter();
                }
                _ => {}
            },
        }
        Step::Continue
    }

    /// Row `row` of the list was clicked: as Enter on it.
    pub fn click(&mut self, row: usize) -> Step {
        match self.items.get(first_shown(self.selected) + row).copied() {
            Some(item) => self.choose(item),
            None => Step::Continue,
        }
    }

    /// `~/…` for a path in the home folder.
    fn display(&self, path: &Path) -> String {
        match self
            .home
            .as_deref()
            .and_then(|home| path.strip_prefix(home).ok())
        {
            Some(rest) if rest.as_os_str().is_empty() => "~".into(),
            Some(rest) => format!("~/{}", rest.display()),
            None => path.display().to_string(),
        }
    }

    /// What the panel shows; icons of `icon_size` pixels. `note` and `side`: where things
    /// open (`View`).
    pub fn view<'a>(&'a self, icons: &'a mut Icons, icon_size: u32, note: String, side: Option<Side>) -> View<'a> {
        for name in [FOLDER_ICON, FILE_ICON, UP_ICON] {
            icons.get(name, icon_size);
        }
        let icons: &Icons = icons;
        let first = first_shown(self.selected);
        let rows = self
            .items
            .iter()
            .enumerate()
            .skip(first)
            .take(ROWS)
            .map(|(i, item)| {
                let (icon, text) = match item {
                    Item::Up => (UP_ICON, ".."),
                    Item::Entry(e) if self.entries[*e].dir => {
                        (FOLDER_ICON, self.entries[*e].name.as_str())
                    }
                    Item::Entry(e) => (FILE_ICON, self.entries[*e].name.as_str()),
                };
                Row {
                    icon: icons.cached(icon, icon_size),
                    text,
                    detail: None,
                    sub: None,
                    selected: i == self.selected,
                }
            })
            .collect();
        let dir = self.display(&self.dir);
        let slash = if dir.ends_with('/') { "" } else { "/" };
        View {
            prompt: format!("# {dir}{slash}{}▏", self.query),
            rows,
            placeholder: "",
            note,
            side,
            hint: &self.hint,
        }
    }
}

/// First of the `ROWS` items shown, so the selected one is among them.
fn first_shown(selected: usize) -> usize {
    selected.saturating_sub(ROWS - 1)
}

/// The entries of `dir`, folders first, then by name (case aside); none when it cannot be read.
fn read_folder(dir: &Path) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let kind = e.file_type().ok()?;
            let dir = kind.is_dir() || (kind.is_symlink() && e.path().is_dir());
            Some(Entry { name, dir })
        })
        .collect();
    entries.sort_by_cached_key(|e| (!e.dir, e.name.to_lowercase()));
    entries
}

/// `path` as one word for `/bin/sh -c`.
pub fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder for one test: `docs/`, `src/`, `.git/`, `a.txt`, `B.md`, `.env`.
    fn tree(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("dune-finder-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["docs/inner", "src", ".git"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["a.txt", "B.md", ".env", "docs/readme"] {
            std::fs::write(root.join(file), "").unwrap();
        }
        root
    }

    fn none() -> Modifiers {
        Modifiers::default()
    }

    fn typed(f: &mut Files, text: &str) {
        for c in text.chars() {
            f.feed(&none(), Keysym::a, Some(c));
        }
    }

    fn names(f: &Files) -> Vec<String> {
        f.items
            .iter()
            .map(|item| match item {
                Item::Up => "..".to_string(),
                Item::Entry(i) => f.entries[*i].name.clone(),
            })
            .collect()
    }

    #[test]
    fn paths_are_typed_from_home_or_the_root() {
        let root = tree("typed");
        let mut f = Files::new(root.join("docs"), "yazi");
        f.home = Some(root.clone());
        // `~/` goes home, the slash after it is the path's own.
        typed(&mut f, "~/sr");
        assert_eq!(f.dir, root);
        assert_eq!(names(&f)[0], "src");
        let mut f = Files::new(root.clone(), "yazi");
        typed(&mut f, "/");
        assert_eq!(f.dir, PathBuf::from("/"));
        // A part of a name and a slash: into the highlighted folder, and on.
        let mut f = Files::new(root.clone(), "yazi");
        typed(&mut f, "do/");
        assert_eq!(f.dir, root.join("docs"));
        typed(&mut f, "/");
        assert_eq!(f.dir, root.join("docs"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ties_go_to_the_name_that_starts_with_it_then_the_shorter() {
        let root =
            std::env::temp_dir().join(format!("dune-finder-files-ties-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["duenote", "duenote-feeder", "dune", "my-dune"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let mut f = Files::new(root.clone(), "yazi");
        typed(&mut f, "du");
        assert_eq!(names(&f)[0], "dune");
        typed(&mut f, "ne");
        assert_eq!(names(&f)[0], "dune");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn folders_come_first_and_hidden_ones_only_with_a_dot() {
        let root = tree("order");
        let mut f = Files::new(root.clone(), "yazi");
        assert_eq!(names(&f), ["..", "docs", "src", "a.txt", "B.md"]);
        typed(&mut f, ".");
        assert!(names(&f).contains(&".git".to_string()) && names(&f).contains(&".env".to_string()));
        f.feed(&none(), Keysym::BackSpace, None);
        typed(&mut f, "md");
        assert_eq!(names(&f), ["B.md"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn folders_are_entered_and_left() {
        let root = tree("walk");
        let mut f = Files::new(root.clone(), "yazi");
        typed(&mut f, "doc");
        assert_eq!(f.feed(&none(), Keysym::Return, None), Step::Continue);
        assert_eq!(f.dir, root.join("docs"));
        assert_eq!(names(&f), ["..", "inner", "readme"]);
        // Backspace with nothing typed goes up; at the first folder, back to the apps.
        assert_eq!(f.feed(&none(), Keysym::BackSpace, None), Step::Continue);
        assert_eq!(f.dir, root);
        assert_eq!(f.feed(&none(), Keysym::BackSpace, None), Step::Apps);
        // `..` goes up too, past the first folder.
        assert_eq!(f.feed(&none(), Keysym::Return, None), Step::Continue);
        assert_eq!(f.dir, root.parent().unwrap());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn files_are_opened_and_a_slash_enters_a_folder() {
        let root = tree("open");
        let mut f = Files::new(root.clone(), "yazi");
        typed(&mut f, "a.t");
        assert_eq!(
            f.feed(&none(), Keysym::Return, None),
            Step::Open(root.join("a.txt"))
        );
        let mut f = Files::new(root.clone(), "yazi");
        typed(&mut f, "src/");
        assert_eq!(f.dir, root.join("src"));
        // A click is Enter on the row: row 3 is `a.txt` (after `..`, docs, src).
        let mut f = Files::new(root.clone(), "yazi");
        assert_eq!(f.click(3), Step::Open(root.join("a.txt")));
        assert_eq!(f.click(1), Step::Continue);
        assert_eq!(f.dir, root.join("docs"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn mod_enter_hands_the_highlighted_entry_to_the_file_manager() {
        let root = tree("manage");
        let ctrl = Modifiers {
            logo: true,
            ..Default::default()
        };
        let mut f = Files::new(root.clone(), "yazi");
        // `..`, highlighted first: the folder shown, not its parent.
        assert_eq!(f.feed(&ctrl, Keysym::Return, None), Step::Manage(root.clone()));
        f.feed(&none(), Keysym::Down, None); // docs
        assert_eq!(
            f.feed(&ctrl, Keysym::Return, None),
            Step::Manage(root.join("docs"))
        );
        typed(&mut f, "B.");
        assert_eq!(
            f.feed(&ctrl, Keysym::Return, None),
            Step::Manage(root.join("B.md"))
        );
        typed(&mut f, "zzz");
        assert_eq!(
            f.feed(&ctrl, Keysym::Return, None),
            Step::Manage(root.clone())
        );
        // Mod+Enter too, Mod being Super or Alt.
        for m in [Modifiers { logo: true, ..Default::default() }, Modifiers { alt: true, ..Default::default() }] {
            assert_eq!(f.feed(&m, Keysym::Return, None), Step::Manage(root.clone()));
        }
        assert_eq!(f.feed(&none(), Keysym::Escape, None), Step::Close);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ctrl_enter_opens_a_terminal_in_the_highlighted_folder() {
        let root = tree("terminal");
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let mut f = Files::new(root.clone(), "yazi");
        // `..`, highlighted first: the folder shown, not its parent.
        assert_eq!(f.feed(&ctrl, Keysym::Return, None), Step::Terminal(root.clone()));
        f.feed(&none(), Keysym::Down, None); // docs
        assert_eq!(f.feed(&ctrl, Keysym::Return, None), Step::Terminal(root.join("docs")));
        // A file's: the folder shown; nothing matching: that folder too.
        typed(&mut f, "B.");
        assert_eq!(f.feed(&ctrl, Keysym::Return, None), Step::Terminal(root.clone()));
        typed(&mut f, "zzz");
        assert_eq!(f.feed(&ctrl, Keysym::Return, None), Step::Terminal(root.clone()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn paths_are_quoted_for_the_shell() {
        assert_eq!(quote(Path::new("/home/a b/c")), "'/home/a b/c'");
        assert_eq!(quote(Path::new("/tmp/it's")), r"'/tmp/it'\''s'");
    }

    #[test]
    fn an_unreadable_folder_is_empty() {
        let f = Files::new(PathBuf::from("/nonexistent/dune-finder"), "yazi");
        assert_eq!(names(&f), [".."]);
        assert!(f.hint.contains("Mod+Enter: yazi"));
    }
}
