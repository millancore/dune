//! The jump mode (`>` typed first in the apps), like zoxide's `z`: the folders terminals
//! worked in and the finder opened, by frecency (see `dune_common::jumps`). Typing narrows
//! them, fuzzy over the path, keeping the frecency order; Enter opens a terminal in the
//! folder, where the apps' `:` word and Ctrl keys say. Mod+Enter (or Ctrl+Enter) opens the file
//! manager there instead.

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
            hint: format!("Enter: terminal there · Mod+Enter: {name} · ⌫: apps · Esc: close"),
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
        let i = first_shown(self.selected) + row;
        match self.matches.get(i) {
            Some(f) => Step::Go(self.folders[*f].clone()),
            None => Step::Continue,
        }
    }

    /// The folders matching the query again, in frecency order (as `z`: the query picks,
    /// the frecency ranks).
    fn refilter(&mut self) {
        self.selected = 0;
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let matcher = &mut self.matcher;
        self.matches = (0..self.folders.len())
            .filter(|i| pattern.score(Utf32Str::new(&self.names[*i], &mut buf), matcher).is_some())
            .collect();
    }

    /// What the panel shows; icons of `icon_size` pixels. Each folder: its path, then (dim,
    /// at the right) its name.
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
            .take(ROWS)
            .map(|(i, f)| Row {
                icon,
                text: &self.names[*f],
                detail: self.folders[*f].file_name().map(|n| n.to_string_lossy().into_owned()),
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

/// First of the `ROWS` matches shown, so the selected one is among them.
fn first_shown(selected: usize) -> usize {
    selected.saturating_sub(ROWS - 1)
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
        assert_eq!(j.click(3), Step::Go("/home/me".into()));
        assert_eq!(j.click(4), Step::Continue);
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
