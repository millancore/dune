//! The windows mode (Mod+/, or `/` typed first in the apps): every window dune has, on
//! every monitor and desktop, in every tab and folded row, the most recently focused first
//! (the focused one last: Enter right away goes back to the window before). Typing narrows
//! it, fuzzy, by title, app and desktop; `:d2` keeps desktop 2 only and `:m1` monitor 1
//! (`:d2 :m1 slack`). Enter shows the window's desktop and focuses it.
//!
//! Windows of locked desktops are not among them: `:l` typed first lists those instead
//! (asked of dune then, marked 🔒), the rest of the query narrowing them as usual; Enter
//! unlocks the window's desktop and goes to it.
//!
//! A browser that lets dune-finder see its tabs (`tabs`) has its windows listed by their
//! tabs, the one shown first: typing finds a tab by its title or address, Enter brings it to
//! the front and goes to its window.

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
    tabs::{self, Tab},
};

const HINT: &str = ":d1-9: only that desktop · :m1-9: only that monitor · :l: on locked desktops \
    · Enter: go · ↑↓ Tab: choose · ⌫: apps · Esc: close";

/// A window to go to.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub tile: TileId,
    pub title: String,
    pub app: String,
    /// Where it is: `desk 2`, `desk 2 · m1` with more than one monitor.
    pub place: String,
    pub desk: Option<u8>,
    pub monitor: u8,
    /// Theme icon name (or path) of its app.
    pub icon: Option<String>,
    /// A tab of the browser window: this entry is that tab, `title` its title.
    pub tab: Option<Tab>,
}

impl Entry {
    /// `window`, with the icon of its app among `apps`.
    pub fn new(window: WindowInfo, apps: &[App]) -> Self {
        let icon = app_icon(apps, &window.app);
        Entry {
            tile: window.tile,
            desk: window.desk(),
            monitor: window.monitor(),
            title: window.title,
            app: window.app,
            place: window.place,
            icon,
            tab: None,
        }
    }
}

/// `entries` with each browser window among them replaced by its `tabs` (see
/// `tabs::by_window`), the one it shows first.
pub fn with_tabs(entries: Vec<Entry>, tabs: Vec<Tab>) -> Vec<Entry> {
    if tabs.is_empty() {
        return entries;
    }
    let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
    let mut owned = tabs::by_window(&titles, tabs);
    entries
        .into_iter()
        .enumerate()
        .flat_map(|(i, window)| match owned.remove(&i) {
            Some(mut tabs) => {
                tabs.sort_by_key(|t| !tabs::shows(&window.title, &t.title));
                tabs.into_iter()
                    .map(|tab| Entry { title: tabs::clean(&tab.title).to_string(), tab: Some(tab), ..window.clone() })
                    .collect()
            }
            None => vec![window],
        })
        .collect()
}

/// What a key did in the windows mode.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Close,
    Go(TileId),
    /// A browser tab: bring it to the front, then go to its window (`tile`, or the one
    /// titled after it by then).
    Tab { tile: TileId, id: String, title: String },
    /// A window of a locked desktop (`:l`): unlock it and go there.
    Unlock(TileId),
    /// Backspace with nothing typed: back to the apps.
    Apps,
}

pub struct Windows {
    entries: Vec<Entry>,
    /// The windows of locked desktops, given once `:l` is typed (`wants_locked`).
    locked: Option<Vec<Entry>>,
    query: String,
    /// Entries matching `query`, best first.
    matches: Vec<usize>,
    selected: usize,
    matcher: Matcher,
}

impl Windows {
    /// Nothing typed: `?` there lists the keys.
    pub fn nothing_typed(&self) -> bool {
        self.query.is_empty()
    }

    /// `entries`, in the order to list them.
    pub fn new(entries: Vec<Entry>) -> Self {
        let matches = (0..entries.len()).collect();
        Windows {
            entries,
            locked: None,
            query: String::new(),
            matches,
            selected: 0,
            matcher: Matcher::new(Config::DEFAULT),
        }
    }

    /// Whether `:l` is typed and the windows of locked desktops are needed (`set_locked`).
    pub fn wants_locked(&self) -> bool {
        self.query.starts_with(":l") && self.locked.is_none()
    }

    /// The windows of locked desktops, each marked 🔒.
    pub fn set_locked(&mut self, entries: Vec<Entry>) {
        let mark = |mut e: Entry| {
            if !e.title.starts_with('🔒') {
                e.title = format!("🔒 {}", e.title);
            }
            e
        };
        self.locked = Some(entries.into_iter().map(mark).collect());
        self.refilter();
    }

    /// The entries listed (see `listing`).
    fn listing(&self) -> (&[Entry], &str, bool) {
        listing(&self.query, &self.entries, self.locked.as_deref())
    }

    /// What choosing the `i`th listed entry does.
    fn choose(&self, i: usize) -> Step {
        let (entries, _, locked) = self.listing();
        let entry = &entries[i];
        match &entry.tab {
            _ if locked => Step::Unlock(entry.tile),
            Some(tab) => Step::Tab { tile: entry.tile, id: tab.id.clone(), title: entry.title.clone() },
            None => Step::Go(entry.tile),
        }
    }

    /// A key pressed: `sym` with `mods`, typing `text`.
    pub fn feed(&mut self, mods: &Modifiers, sym: Keysym, text: Option<char>) -> Step {
        let n = self.matches.len().max(1);
        match sym {
            Keysym::Escape => return Step::Close,
            Keysym::Return | Keysym::KP_Enter => {
                return self.matches.get(self.selected).map_or(Step::Close, |i| self.choose(*i));
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
            Some(e) => self.choose(*e),
            None => Step::Continue,
        }
    }

    /// The entries matching the query again, best first (ties keep the listing order). Its
    /// `:dN` / `:mN` words keep that desktop / monitor only; other `:` words (`:`, `:d`
    /// while typing) count for nothing. `:l` first: the locked ones (`listing`).
    fn refilter(&mut self) {
        self.selected = 0;
        let (mut desk, mut monitor) = (None, None);
        let mut fuzzy = Vec::new();
        let (entries, query, _) = listing(&self.query, &self.entries, self.locked.as_deref());
        for word in query.split_whitespace() {
            let Some(filter) = word.strip_prefix(':') else {
                fuzzy.push(word);
                continue;
            };
            let number = |prefix| filter.strip_prefix(prefix).and_then(|n| n.parse::<u8>().ok());
            desk = number('d').or(desk);
            monitor = number('m').or(monitor);
        }
        let pattern = Pattern::parse(&fuzzy.join(" "), CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let mut scored: Vec<(u32, usize)> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| desk.is_none_or(|d| e.desk == Some(d)) && monitor.is_none_or(|m| e.monitor == m))
            .filter_map(|(i, e)| {
                let url = e.tab.as_ref().map_or("", |t| t.url.as_str());
                let haystack = format!("{} {} {} {url}", e.title, e.app, e.place);
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
        let (entries, _, locked) = self.listing();
        let shown: Vec<(usize, &Entry)> = self
            .matches
            .iter()
            .skip(first)
            .take(ROWS)
            .enumerate()
            .map(|(i, e)| (first + i, &entries[*e]))
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
                sub: None,
                selected: *i == self.selected,
            })
            .collect();
        let kind = if locked { "locked windows" } else { "windows" };
        View {
            prompt: format!("/ {}▏", self.query),
            rows,
            status: format!("{} / {} {kind}", self.matches.len(), entries.len()),
            hint: HINT,
        }
    }
}

/// The entries `query` lists and the query narrowing them: with `:l` first, the `locked`
/// ones and the rest of the query, else `entries` and all of it; whether they are the
/// locked ones.
fn listing<'a>(query: &'a str, entries: &'a [Entry], locked: Option<&'a [Entry]>) -> (&'a [Entry], &'a str, bool) {
    match query.strip_prefix(":l") {
        Some(rest) => (locked.unwrap_or_default(), rest, true),
        None => (entries, query, false),
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
        let window = WindowInfo { tile, title: title.into(), app: app.into(), place: place.into() };
        Entry::new(window, &[])
    }

    fn windows() -> Windows {
        Windows::new(vec![
            entry(1, "dune — nvim", "wezterm", "desk 1"),
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
        let (entries, ..) = w.listing();
        w.matches.iter().map(|i| entries[*i].tile).collect()
    }

    #[test]
    fn a_browser_window_is_listed_by_its_tabs_the_shown_one_first() {
        let tab = |id: &str, title: &str, url: &str| Tab { id: id.into(), title: title.into(), url: url.into(), window: Some(7) };
        let entries = vec![
            entry(1, "dune — nvim", "wezterm", "desk 1"),
            entry(2, "(2) Inbox - Vivaldi", "vivaldi-stable", "desk 2"),
            entry(3, "Files", "org.gnome.Nautilus", "desk 3 · m2"),
        ];
        let tabs = vec![tab("a", "Rust docs", "https://doc.rust-lang.org"), tab("b", "(2) Inbox", "https://mail.example")];
        let mut w = Windows::new(with_tabs(entries, tabs));
        let titles: Vec<&str> = w.entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["dune — nvim", "Inbox", "Rust docs", "Files"]);
        assert_eq!(listed(&w), vec![1, 2, 2, 3]);
        // Found by its address too; Enter brings it to the front.
        typed(&mut w, "rust-lang");
        assert_eq!(
            w.feed(&none(), Keysym::Return, None),
            Step::Tab { tile: 2, id: "a".into(), title: "Rust docs".into() }
        );
        // No browser to ask: the windows as they are.
        assert_eq!(with_tabs(vec![entry(2, "Inbox - Vivaldi", "vivaldi-stable", "desk 2")], Vec::new())[0].title, "Inbox - Vivaldi");
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
    fn colon_words_keep_a_desktop_or_a_monitor() {
        let mut w = windows();
        typed(&mut w, ":d2");
        assert_eq!(listed(&w), vec![2]);
        let ctrl = Modifiers { ctrl: true, ..Default::default() };
        w.feed(&ctrl, Keysym::u, None);
        // No `mN`: the one monitor, m1.
        typed(&mut w, ":m1");
        assert_eq!(listed(&w), vec![1, 2]);
        typed(&mut w, " fire");
        assert_eq!(listed(&w), vec![2]);
        w.feed(&ctrl, Keysym::u, None);
        typed(&mut w, ":m2 files");
        assert_eq!(listed(&w), vec![3]);
        w.feed(&ctrl, Keysym::u, None);
        typed(&mut w, ":d3 :m1");
        assert_eq!(listed(&w), Vec::<TileId>::new());
        // While typing, `:` and `:d` keep everything, in the listing order.
        w.feed(&ctrl, Keysym::u, None);
        typed(&mut w, ":");
        assert_eq!(listed(&w), vec![1, 2, 3]);
        typed(&mut w, "d");
        assert_eq!(listed(&w), vec![1, 2, 3]);
        w.feed(&ctrl, Keysym::u, None);
        typed(&mut w, ":x nvim");
        assert_eq!(listed(&w), vec![1]);
    }

    #[test]
    fn colon_l_lists_the_windows_of_locked_desktops_only() {
        let mut w = windows();
        typed(&mut w, ":");
        assert!(!w.wants_locked());
        typed(&mut w, "l");
        // dune is asked for them once.
        assert!(w.wants_locked());
        assert_eq!(listed(&w), Vec::<TileId>::new());
        w.set_locked(vec![entry(7, "Slack | client", "slack", "desk 4"), entry(8, "🔒 foot", "foot", "desk 4")]);
        assert!(!w.wants_locked());
        assert_eq!(listed(&w), vec![7, 8]);
        // Marked, once.
        assert_eq!(w.locked.as_ref().unwrap()[0].title, "🔒 Slack | client");
        assert_eq!(w.locked.as_ref().unwrap()[1].title, "🔒 foot");
        // Choosing one unlocks its desktop.
        assert_eq!(w.feed(&none(), Keysym::Return, None), Step::Unlock(7));
        assert_eq!(w.click(1), Step::Unlock(8));
        // The rest of the query narrows them as usual.
        typed(&mut w, " slack");
        assert_eq!(listed(&w), vec![7]);
        let ctrl = Modifiers { ctrl: true, ..Default::default() };
        w.feed(&ctrl, Keysym::u, None);
        typed(&mut w, ":lfoot");
        assert_eq!(listed(&w), vec![8]);
        // Without `:l`, the unlocked ones again, gone to.
        w.feed(&ctrl, Keysym::u, None);
        assert_eq!(listed(&w), vec![1, 2, 3]);
        assert_eq!(w.feed(&none(), Keysym::Return, None), Step::Go(1));
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
