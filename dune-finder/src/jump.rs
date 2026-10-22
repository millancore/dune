//! The jump mode (`>` typed first in the apps), like zoxide's `z`: the folders terminals
//! worked in and the finder opened, by frecency (see `dune_common::jumps`). Typing narrows
//! them, fuzzy over the path, keeping the frecency order; Enter opens a terminal in the
//! folder, where the apps' `:` word and Ctrl keys say. Mod+Enter (or Ctrl+Enter) opens the file
//! manager there instead. `/` goes into the highlighted folder, in the files mode: its
//! folders and files narrowed by typing (Backspace there with nothing typed comes back).

use std::path::{Path, PathBuf};

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

/// What a key did in the jump mode.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Close,
    /// Open a terminal in this folder.
    Go(PathBuf),
    /// Open the file manager at this folder.
    Manage(PathBuf),
    /// `/`: show this folder's files.
    Into(PathBuf),
    /// Backspace with nothing typed: back to the apps.
    Apps,
}

pub struct Jump {
    folders: Vec<PathBuf>,
    /// How each folder is shown and matched: `~` for the home folder.
    names: Vec<String>,
    query: String,
    /// Folders matching `query`, in frecency order.
    matches: Vec<usize>,
    selected: usize,
    matcher: Matcher,
    hint: String,
}

impl Jump {
    /// Nothing typed: `?` there lists the keys.
    pub fn nothing_typed(&self) -> bool {
        self.query.is_empty()
    }

    /// `folders`, the highest frecency first; `home` is shown as `~`; `file_manager` is
    /// named in the hint.
    pub fn new(folders: Vec<PathBuf>, home: &Path, file_manager: &str) -> Self {
        let names = folders.iter().map(|f| tilde(f, home)).collect();
        let name = file_manager.split_whitespace().next().unwrap_or(file_manager);
        Jump {
            matches: (0..folders.len()).collect(),
            folders,
            names,
            query: String::new(),
            selected: 0,
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
            hint: format!("Enter: terminal there · Mod+Enter: {name} · /: into it · ⌫: apps · Esc: close"),
        }
    }

    /// A key pressed: `sym` with `mods`, typing `text`.
    pub fn feed(&mut self, mods: &Modifiers, sym: Keysym, text: Option<char>) -> Step {
        let n = self.matches.len().max(1);
        match sym {
            Keysym::Escape => return Step::Close,
            Keysym::Return | Keysym::KP_Enter => {
                let Some(folder) = self.matches.get(self.selected).map(|i| self.folders[*i].clone()) else {
                    return Step::Close;
                };
                return if mods.logo || mods.ctrl || mods.alt { Step::Manage(folder) } else { Step::Go(folder) };
            }
            Keysym::Down | Keysym::Tab => self.selected = (self.selected + 1) % n,
            Keysym::n if mods.ctrl => self.selected = (self.selected + 1) % n,
            Keysym::Up | Keysym::ISO_Left_Tab => self.selected = (self.selected + n - 1) % n,
            Keysym::p if mods.ctrl => self.selected = (self.selected + n - 1) % n,
            Keysym::BackSpace if self.query.is_empty() => return Step::Apps,
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
                Some('/') => {
                    if let Some(i) = self.matches.get(self.selected) {
                        return Step::Into(self.folders[*i].clone());
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

    /// Row `row` of the list was clicked: a terminal in its folder.
    pub fn click(&mut self, row: usize) -> Step {
        let i = first_shown(self.selected) + row / 2;
        match self.matches.get(i) {
            Some(f) => Step::Go(self.folders[*f].clone()),
            None => Step::Continue,
        }
    }

    /// The folders matching the query again, in frecency order (as `z`: the query picks,
    /// the frecency ranks); those whose own name matches first (`proj`: the `projects`
    /// folders before the ones in them).
    fn refilter(&mut self) {
        self.selected = 0;
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let matcher = &mut self.matcher;
        let matches: Vec<usize> = (0..self.folders.len())
            .filter(|i| pattern.score(Utf32Str::new(&self.names[*i], &mut buf), matcher).is_some())
            .collect();
        if self.query.is_empty() {
            self.matches = matches;
            return;
        }
        let (named, inside): (Vec<usize>, Vec<usize>) = matches.into_iter().partition(|i| {
            let name = self.folders[*i].file_name().and_then(|n| n.to_str()).unwrap_or(&self.names[*i]);
            pattern.score(Utf32Str::new(name, &mut buf), matcher).is_some()
        });
        self.matches = named.into_iter().chain(inside).collect();
    }

    /// What the panel shows; icons of `icon_size` pixels. Each folder on two lines: its
    /// name, then (dim) its path.
    pub fn view<'a>(&'a self, icons: &'a mut Icons, icon_size: u32) -> View<'a> {
        icons.get(FOLDER_ICON, icon_size);
        let icons: &Icons = icons;
        let icon = icons.cached(FOLDER_ICON, icon_size);
        let first = first_shown(self.selected);
        let rows = self
            .matches
            .iter()
            .enumerate()
            .skip(first)
            .take(SHOWN)
            .map(|(i, f)| Row {
                icon,
                text: self.folders[*f].file_name().and_then(|n| n.to_str()).unwrap_or(&self.names[*f]),
                detail: None,
                sub: Some(self.names[*f].clone()),
                selected: i == self.selected,
            })
            .collect();
        View {
            prompt: format!("> {}▏", self.query),
            rows,
            status: format!("{} / {} folders", self.matches.len(), self.folders.len()),
            hint: &self.hint,
        }
    }
}

/// `path` with the home folder written `~`.
fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// Folders shown at once, two lines each.
const SHOWN: usize = ROWS / 2;

/// First of the `SHOWN` matches, so the selected one is among them.
fn first_shown(selected: usize) -> usize {
    selected.saturating_sub(SHOWN - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jump() -> Jump {
        let folders = ["/home/me/work/dune", "/srv/app", "/home/me/work/duenote", "/home/me"];
        Jump::new(folders.iter().map(PathBuf::from).collect(), Path::new("/home/me"), "yazi")
    }

    fn none() -> Modifiers {
        Modifiers::default()
    }

    fn typed(j: &mut Jump, text: &str) {
        for c in text.chars() {
            assert_eq!(j.feed(&none(), Keysym::a, Some(c)), Step::Continue);
        }
    }

    fn listed(j: &Jump) -> Vec<&str> {
        j.matches.iter().map(|i| j.names[*i].as_str()).collect()
    }

    #[test]
    fn folders_are_listed_by_frecency_with_home_as_tilde() {
        let j = jump();
        assert_eq!(listed(&j), vec!["~/work/dune", "/srv/app", "~/work/duenote", "~"]);
    }

    #[test]
    fn typing_narrows_and_the_frecency_order_stays() {
        let mut j = jump();
        typed(&mut j, "dune");
        assert_eq!(listed(&j), vec!["~/work/dune", "~/work/duenote"]);
        let mut j = jump();
        typed(&mut j, "dnote");
        assert_eq!(listed(&j), vec!["~/work/duenote"]);
        assert_eq!(j.feed(&none(), Keysym::Return, None), Step::Go("/home/me/work/duenote".into()));
        typed(&mut j, "zzz");
        assert_eq!(j.feed(&none(), Keysym::Return, None), Step::Close);
    }

    #[test]
    fn enter_opens_a_terminal_and_mod_enter_the_file_manager() {
        let mut j = jump();
        j.feed(&none(), Keysym::Down, None);
        assert_eq!(j.feed(&none(), Keysym::Return, None), Step::Go("/srv/app".into()));
        let logo = Modifiers { logo: true, ..Default::default() };
        assert_eq!(j.feed(&logo, Keysym::Return, None), Step::Manage("/srv/app".into()));
        // Two lines a folder: either one is it.
        assert_eq!(j.click(6), Step::Go("/home/me".into()));
        assert_eq!(j.click(7), Step::Go("/home/me".into()));
        assert_eq!(j.click(8), Step::Continue);
    }

    #[test]
    fn each_folder_shows_its_name_and_under_it_its_path() {
        let j = jump();
        let mut icons = Icons::new("");
        let view = j.view(&mut icons, 16);
        let shown: Vec<(&str, Option<&str>)> = view.rows.iter().map(|r| (r.text, r.sub.as_deref())).collect();
        assert_eq!(
            shown,
            [("dune", Some("~/work/dune")), ("app", Some("/srv/app")), ("duenote", Some("~/work/duenote")), ("me", Some("~"))]
        );
    }

    #[test]
    fn folders_named_like_the_query_come_before_the_ones_in_them() {
        let folders = ["/home/me/work/projects/canatec", "/srv/app", "/home/me/work/projects", "/home/me/work"];
        let mut j = Jump::new(folders.iter().map(PathBuf::from).collect(), Path::new("/home/me"), "yazi");
        typed(&mut j, "proj");
        assert_eq!(listed(&j), vec!["~/work/projects", "~/work/projects/canatec"]);
        // `/` goes into the highlighted one, the first or the one chosen.
        assert_eq!(j.feed(&none(), Keysym::slash, Some('/')), Step::Into("/home/me/work/projects".into()));
        j.feed(&none(), Keysym::Down, None);
        assert_eq!(j.feed(&none(), Keysym::slash, Some('/')), Step::Into("/home/me/work/projects/canatec".into()));
    }

    #[test]
    fn a_slash_goes_into_the_highlighted_folder() {
        let mut j = jump();
        typed(&mut j, "app");
        assert_eq!(j.feed(&none(), Keysym::slash, Some('/')), Step::Into("/srv/app".into()));
        // Nothing highlighted: nothing to go into.
        typed(&mut j, "zzz");
        assert_eq!(j.feed(&none(), Keysym::slash, Some('/')), Step::Continue);
    }

    #[test]
    fn escape_closes_and_backspace_on_nothing_goes_to_the_apps() {
        let mut j = jump();
        assert_eq!(j.feed(&none(), Keysym::Escape, None), Step::Close);
        typed(&mut j, "s");
        assert_eq!(j.feed(&none(), Keysym::BackSpace, None), Step::Continue);
        assert_eq!(j.feed(&none(), Keysym::BackSpace, None), Step::Apps);
    }
}
