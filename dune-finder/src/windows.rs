//! The windows mode (Mod+/, or `/` typed first in the apps): every window dune has, on
//! every monitor and desktop, in every tab and folded row, the most recently focused first
//! (the focused one last: Enter right away goes back to the window before). Typing narrows
//! it, fuzzy, by title, app and desktop; Enter shows the window's desktop and focuses it.

use dune_common::{
    desktop::App,
    protocol::{TileId, WindowInfo},
};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};

use crate::{
    icons::Icons,
    panel::{ROWS, Row, View},
};

const HINT: &str = "Enter: go · ↑↓ Tab: choose · ⌫: apps · Esc: close";

/// A window to go to.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub tile: TileId,
    pub title: String,
    pub app: String,
    /// Where it is: `desk 2`, `desk 2 · m1` with more than one monitor.
    pub place: String,
    /// Theme icon name (or path) of its app.
    pub icon: Option<String>,
}

impl Entry {
    /// `window`, with the icon of its app among `apps`.
    pub fn new(window: WindowInfo, apps: &[App]) -> Self {
        let icon = app_icon(apps, &window.app);
        Entry {
            tile: window.tile,
            title: window.title,
            app: window.app,
            place: window.place,
            icon,
        }
    }
}

/// What a key did in the windows mode.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Close,
    Go(TileId),
    /// Backspace with nothing typed: back to the apps.
    Apps,
}

pub struct Windows {
    entries: Vec<Entry>,
    query: String,
    /// Entries matching `query`, best first.
    matches: Vec<usize>,
    selected: usize,
    matcher: Matcher,
}

impl Windows {
    /// `entries`, in the order to list them.
    pub fn new(entries: Vec<Entry>) -> Self {
        let matches = (0..entries.len()).collect();
        Windows {
            entries,
            query: String::new(),
            matches,
            selected: 0,
            matcher: Matcher::new(Config::DEFAULT),
        }
    }

    /// A key pressed: `sym` with `mods`, typing `text`.
    pub fn feed(&mut self, mods: &Modifiers, sym: Keysym, text: Option<char>) -> Step {
        let n = self.matches.len().max(1);
        match sym {
            Keysym::Escape => return Step::Close,
            Keysym::Return | Keysym::KP_Enter => {
                return self
                    .matches
                    .get(self.selected)
                    .map_or(Step::Close, |i| Step::Go(self.entries[*i].tile));
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

    /// Row `row` of the list was clicked: go to its window.
    pub fn click(&mut self, row: usize) -> Step {
        let i = first_shown(self.selected) + row;
        match self.matches.get(i) {
            Some(e) => Step::Go(self.entries[*e].tile),
            None => Step::Continue,
        }
    }

    /// The entries matching the query again, best first (ties keep the listing order).
    fn refilter(&mut self) {
        self.selected = 0;
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let mut scored: Vec<(u32, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let haystack = format!("{} {} {}", e.title, e.app, e.place);
                pattern.score(Utf32Str::new(&haystack, &mut buf), &mut self.matcher).map(|score| (score, i))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        self.matches = scored.into_iter().map(|(_, i)| i).collect();
    }

    /// What the panel shows; icons of `icon_size` pixels. Each window: its app's icon, its
    /// title, then (dim, at the right) its app and where it is.
    pub fn view<'a>(&'a self, icons: &'a mut Icons, icon_size: u32) -> View<'a> {
        let first = first_shown(self.selected);
        let shown: Vec<(usize, &Entry)> = self
            .matches
            .iter()
            .skip(first)
            .take(ROWS)
            .enumerate()
            .map(|(i, e)| (first + i, &self.entries[*e]))
            .collect();
        for name in shown.iter().filter_map(|(_, e)| e.icon.as_deref()) {
            icons.get(name, icon_size);
        }
        let icons: &Icons = icons;
        let rows = shown
            .iter()
            .map(|(i, e)| Row {
                icon: e
                    .icon
                    .as_deref()
                    .and_then(|name| icons.cached(name, icon_size)),
                text: &e.title,
                detail: Some(format!("{} · {}", e.app, e.place)),
                selected: *i == self.selected,
            })
            .collect();
        View {
            prompt: format!("/ {}▏", self.query),
            rows,
            status: format!("{} / {} windows", self.matches.len(), self.entries.len()),
            hint: HINT,
        }
    }
}

/// First of the `ROWS` matches shown, so the selected one is among them.
fn first_shown(selected: usize) -> usize {
    selected.saturating_sub(ROWS - 1)
}

/// The icon of the desktop app whose id is `app_id` (or ends in it: `org.gnome.Nautilus`
/// for `nautilus`), else the app id itself as an icon name.
fn app_icon(apps: &[App], app_id: &str) -> Option<String> {
    if app_id.is_empty() {
        return None;
    }
    let named = |a: &&App| {
        a.id.eq_ignore_ascii_case(app_id)
            || a.id
                .rsplit('.')
                .next()
                .is_some_and(|last| last.eq_ignore_ascii_case(app_id))
    };
    apps.iter().find(named).and_then(|a| a.icon.clone()).or_else(|| Some(app_id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(tile: TileId, title: &str, app: &str, place: &str) -> Entry {
        Entry { tile, title: title.into(), app: app.into(), place: place.into(), icon: None }
    }

    fn windows() -> Windows {
        Windows::new(vec![
            entry(1, "dune — nvim", "dune-term", "desk 1"),
            entry(2, "Mozilla Firefox", "firefox", "desk 2"),
            entry(3, "Files", "org.gnome.Nautilus", "desk 3 · m2"),
        ])
    }

    fn none() -> Modifiers {
        Modifiers::default()
    }

    fn typed(w: &mut Windows, text: &str) {
        for c in text.chars() {
            assert_eq!(w.feed(&none(), Keysym::a, Some(c)), Step::Continue);
        }
    }

    fn listed(w: &Windows) -> Vec<TileId> {
        w.matches.iter().map(|i| w.entries[*i].tile).collect()
    }

    #[test]
    fn enter_right_away_goes_to_the_first_window() {
        let mut w = windows();
        assert_eq!(listed(&w), vec![1, 2, 3]);
        assert_eq!(w.feed(&none(), Keysym::Return, None), Step::Go(1));
    }

    #[test]
    fn typing_narrows_fuzzy_over_title_app_and_place() {
        let mut w = windows();
        typed(&mut w, "ffx");
        assert_eq!(listed(&w), vec![2]);
        w.feed(&none(), Keysym::BackSpace, None);
        w.feed(&none(), Keysym::BackSpace, None);
        w.feed(&none(), Keysym::BackSpace, None);
        // Fuzzy: "m2" fits Mozilla on desk 2 too, but the monitor fits best.
        typed(&mut w, "m2");
        assert_eq!(listed(&w), vec![3, 2]);
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        w.feed(&ctrl, Keysym::u, None);
        assert_eq!(listed(&w).len(), 3);
        typed(&mut w, "zzz");
        assert_eq!(listed(&w), Vec::<TileId>::new());
        assert_eq!(w.feed(&none(), Keysym::Return, None), Step::Close);
    }

    #[test]
    fn moving_through_the_list_goes_round() {
        let mut w = windows();
        w.feed(&none(), Keysym::Up, None);
        assert_eq!(w.selected, 2);
        w.feed(&none(), Keysym::Tab, None);
        w.feed(&none(), Keysym::Down, None);
        assert_eq!(w.feed(&none(), Keysym::Return, None), Step::Go(2));
    }

    #[test]
    fn escape_closes_and_backspace_on_nothing_goes_to_the_apps() {
        let mut w = windows();
        assert_eq!(w.feed(&none(), Keysym::Escape, None), Step::Close);
        typed(&mut w, "f");
        assert_eq!(w.feed(&none(), Keysym::BackSpace, None), Step::Continue);
        assert_eq!(w.feed(&none(), Keysym::BackSpace, None), Step::Apps);
    }

    #[test]
    fn a_click_goes_to_the_row() {
        let mut w = windows();
        assert_eq!(w.click(2), Step::Go(3));
        assert_eq!(w.click(3), Step::Continue);
    }

    #[test]
    fn the_selected_window_stays_on_screen() {
        assert_eq!(first_shown(0), 0);
        assert_eq!(first_shown(ROWS - 1), 0);
        assert_eq!(first_shown(ROWS + 2), 3);
    }

    #[test]
    fn icons_come_from_the_desktop_app() {
        let app = |id: &str, icon: &str| App {
            id: id.into(),
            name: id.into(),
            icon: Some(icon.into()),
            ..Default::default()
        };
        let apps = vec![app("org.gnome.Nautilus", "folder"), app("firefox", "firefox-icon")];
        assert_eq!(app_icon(&apps, "firefox").as_deref(), Some("firefox-icon"));
        assert_eq!(app_icon(&apps, "nautilus").as_deref(), Some("folder"));
        assert_eq!(app_icon(&apps, "foot").as_deref(), Some("foot"));
        assert_eq!(app_icon(&apps, ""), None);
    }
}
