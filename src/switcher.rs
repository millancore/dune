//! The switcher (Mod+/): a panel like the launcher's listing every window, on every
//! monitor and desktop, in every tab and folded row, the most recently focused first (the
//! focused one last: Enter right away goes back to the window before). Typing narrows it,
//! fuzzy, by title, app and desktop; Enter shows the window's desktop and focuses it.

use dune_common::font::Font;
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    input::keyboard::{Keysym, ModifiersState},
    utils::Transform,
};

use crate::{
    Dune,
    icons::Icons,
    launcher::{
        ACCENT, BACKGROUND, COLUMNS, Canvas, DIM, FONT_SIZE, FRAME, FRAME_COLOR, ICON_MARGIN, PADDING, SELECTED, TEXT,
        line_height,
    },
    layout::TileId,
};

/// Lines of windows shown at once.
const ROWS: usize = 10;
const HINT: &str = "Enter: go · ↑↓ Tab: choose · Esc: close";

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

/// What a key did to the switcher.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Close,
    Go(TileId),
}

struct Session {
    entries: Vec<Entry>,
    query: String,
    /// Entries matching `query`, best first.
    matches: Vec<usize>,
    selected: usize,
    output: String,
}

pub struct Switcher {
    /// Loaded on first use: fontconfig runs a process.
    font: Option<Option<Font>>,
    family: String,
    icons: Option<Icons>,
    icon_theme: String,
    matcher: Matcher,
    session: Option<Session>,
    /// The panel as last drawn, and its size; dropped whenever it changes.
    image: Option<(MemoryRenderBuffer, (i32, i32))>,
}

impl Switcher {
    pub fn new(family: &str, icon_theme: &str) -> Self {
        Switcher {
            font: None,
            family: family.to_string(),
            icons: None,
            icon_theme: icon_theme.to_string(),
            matcher: Matcher::new(Config::DEFAULT),
            session: None,
            image: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.session.is_some()
    }

    /// The output the panel is shown on.
    pub fn output(&self) -> Option<&str> {
        self.session.as_ref().map(|s| s.output.as_str())
    }

    /// Show the panel on `output` with `entries`, in the order to list them.
    pub fn open(&mut self, entries: Vec<Entry>, output: String) {
        let matches = (0..entries.len()).collect();
        self.session = Some(Session { entries, query: String::new(), matches, selected: 0, output });
        self.image = None;
    }

    pub fn close(&mut self) {
        self.session = None;
        self.image = None;
    }

    /// A key pressed while open: `sym` with `mods`, typing `text`.
    pub fn feed(&mut self, mods: &ModifiersState, sym: Keysym, text: Option<char>) -> Step {
        let Some(session) = self.session.as_mut() else { return Step::Close };
        self.image = None;
        let n = session.matches.len().max(1);
        match sym {
            Keysym::Escape => {
                self.close();
                return Step::Close;
            }
            Keysym::Return | Keysym::KP_Enter => {
                let go = session.matches.get(session.selected).map(|i| session.entries[*i].tile);
                self.close();
                return go.map_or(Step::Close, Step::Go);
            }
            Keysym::Down | Keysym::Tab => session.selected = (session.selected + 1) % n,
            Keysym::n if mods.ctrl => session.selected = (session.selected + 1) % n,
            Keysym::Up | Keysym::ISO_Left_Tab => session.selected = (session.selected + n - 1) % n,
            Keysym::p if mods.ctrl => session.selected = (session.selected + n - 1) % n,
            Keysym::BackSpace => {
                session.query.pop();
                self.refilter();
            }
            Keysym::u if mods.ctrl => {
                session.query.clear();
                self.refilter();
            }
            Keysym::w if mods.ctrl => {
                let kept = session.query.trim_end().rfind(' ').map_or(0, |i| i + 1);
                session.query.truncate(kept);
                self.refilter();
            }
            _ => match text {
                Some(c) if !c.is_control() && !mods.ctrl && !mods.alt && !mods.logo => {
                    session.query.push(c);
                    self.refilter();
                }
                _ => {}
            },
        }
        Step::Continue
    }

    /// The entries matching the query again, best first (ties keep the listing order).
    fn refilter(&mut self) {
        let Some(session) = self.session.as_mut() else { return };
        session.selected = 0;
        let pattern = Pattern::parse(&session.query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let mut scored: Vec<(u32, usize)> = session
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let haystack = format!("{} {} {}", e.title, e.app, e.place);
                pattern.score(Utf32Str::new(&haystack, &mut buf), &mut self.matcher).map(|score| (score, i))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        session.matches = scored.into_iter().map(|(_, i)| i).collect();
    }

    /// The panel, at most `max_width` pixels wide: its pixels and size.
    pub fn image(&mut self, max_width: i32) -> Option<(&MemoryRenderBuffer, (i32, i32))> {
        let session = self.session.as_ref()?;
        let family = &self.family;
        let font = self.font.get_or_insert_with(|| {
            Font::load(family, FONT_SIZE)
                .or_else(|_| Font::load("monospace", FONT_SIZE))
                .inspect_err(|err| tracing::warn!("switcher without text: {err}"))
                .ok()
        });
        let size = panel_size(font.as_ref(), max_width);
        if self.image.as_ref().is_none_or(|(_, drawn)| *drawn != size) {
            let icons = self.icons.get_or_insert_with(|| Icons::new(&self.icon_theme));
            let pixels = draw(font.as_mut(), icons, session, size);
            let buffer = MemoryRenderBuffer::from_slice(&pixels, Fourcc::Argb8888, size, 1, Transform::Normal, None);
            self.image = Some((buffer, size));
        }
        self.image.as_ref().map(|(buffer, size)| (buffer, *size))
    }
}

/// The query line, the windows, a gap, then how many match and the hint.
fn panel_size(font: Option<&Font>, max_width: i32) -> (i32, i32) {
    let cell = font.map_or(10, |f| f.cell_width as i32);
    let w = (COLUMNS as i32 * cell + 2 * PADDING).min(max_width).max(2 * PADDING + 1);
    let h = (ROWS as i32 + 4) * line_height(font) + 2 * PADDING;
    (w, h)
}

/// First of the `ROWS` matches shown, so the selected one is among them.
fn first_shown(selected: usize) -> usize {
    selected.saturating_sub(ROWS - 1)
}

/// ARGB8888 pixels (B, G, R, A in memory) of the panel.
fn draw(font: Option<&mut Font>, icons: &mut Icons, session: &Session, (w, h): (i32, i32)) -> Vec<u8> {
    let mut canvas = Canvas { pixels: [BACKGROUND[2], BACKGROUND[1], BACKGROUND[0], 0xff].repeat((w * h) as usize), w, h };
    canvas.fill(0, 0, w, FRAME, FRAME_COLOR);
    canvas.fill(0, h - FRAME, w, FRAME, FRAME_COLOR);
    canvas.fill(0, 0, FRAME, h, FRAME_COLOR);
    canvas.fill(w - FRAME, 0, FRAME, h, FRAME_COLOR);
    let Some(font) = font else { return canvas.pixels };

    let line = line_height(Some(font));
    let cell = font.cell_width.max(1) as i32;
    let cols = ((w - 2 * PADDING) / cell).max(1) as usize;
    let row_y = |row: i32| PADDING + row * line;

    canvas.text(font, PADDING, row_y(0), line, &format!("/ {}▏", session.query), TEXT, cols);
    // Each window: its app's icon in a square as tall as the line, its title, then (dim,
    // at the right) its app and where it is.
    let icon_size = (line - 2 * ICON_MARGIN).max(1) as u32;
    let text_x = PADDING + line + ICON_MARGIN;
    let text_cols = cols.saturating_sub(((line + ICON_MARGIN) / cell) as usize).max(1);
    let first = first_shown(session.selected);
    for (i, &e) in session.matches.iter().skip(first).take(ROWS).enumerate() {
        let entry = &session.entries[e];
        let y = row_y(1 + i as i32);
        if first + i == session.selected {
            canvas.fill(FRAME, y, w - 2 * FRAME, line, SELECTED);
        }
        if let Some(icon) = entry.icon.as_deref().and_then(|name| icons.get(name, icon_size)) {
            canvas.image(PADDING + ICON_MARGIN, y + ICON_MARGIN, icon);
        }
        let detail = format!("{} · {}", entry.app, entry.place);
        let detail_cols = detail.chars().count().min(text_cols / 2);
        canvas.text(font, text_x, y, line, &entry.title, TEXT, text_cols.saturating_sub(detail_cols + 2).max(1));
        let detail_x = text_x + (text_cols - detail_cols) as i32 * cell;
        canvas.text(font, detail_x, y, line, &detail, DIM, detail_cols);
    }
    let bottom = ROWS as i32 + 2;
    let count = format!("{} / {} windows", session.matches.len(), session.entries.len());
    canvas.text(font, PADDING, row_y(bottom), line, &count, ACCENT, cols);
    canvas.text(font, PADDING, row_y(bottom + 1), line, HINT, DIM, cols);
    canvas.pixels
}

impl Dune {
    /// Mod+/: show the switcher on the active output, or hide it.
    pub fn toggle_switcher(&mut self) {
        if self.switcher.is_open() {
            return self.switcher.close();
        }
        let Some(output) = self.active_output() else { return };
        let entries = self.switcher_entries();
        self.switcher.open(entries, output.name());
    }

    /// A key pressed while the switcher is open; Enter goes to the window chosen.
    pub fn switcher_key(&mut self, mods: ModifiersState, sym: Keysym, text: Option<char>) {
        if let Step::Go(id) = self.switcher.feed(&mods, sym, text) {
            self.go_to(id);
        }
    }

    /// Show the desktop `id` is on and focus it: its tab shows, its row opens and the view
    /// slides to it (see `arrange`).
    pub fn go_to(&mut self, id: TileId) {
        if !self.tiles.contains_key(&id) {
            return;
        }
        if let Some((output, desk)) = self.hidden_desk(id) {
            self.show_desk_on(&output, desk);
        }
        self.focus_tile(id);
    }

    /// Every tile with a window, on every monitor and desktop: the most recently focused
    /// first, then the rest monitor by monitor, desktop by desktop; the focused one last.
    fn switcher_entries(&self) -> Vec<Entry> {
        let outputs = self.numbered_outputs();
        let mut placed: Vec<(TileId, String)> = Vec::new();
        for (m, output) in outputs.iter().enumerate() {
            let name = output.name();
            let monitor = if outputs.len() > 1 { format!(" · m{}", m + 1) } else { String::new() };
            let shown = self.desk_of(&name);
            for desk in 1..=crate::desktops::DESKS {
                let place = format!("desk {desk}{monitor}");
                let tiles = if desk == shown {
                    let floats = self.floating.get(&name).into_iter().flatten().map(|(t, _)| *t);
                    floats.chain(self.layouts.get(&name).map(|l| l.tiles()).unwrap_or_default()).collect()
                } else {
                    self.hidden.get(&(name.clone(), desk)).map(|d| d.tiles()).unwrap_or_default()
                };
                placed.extend(tiles.into_iter().map(|t| (t, place.clone())));
            }
        }
        placed.retain(|(t, _)| self.tiles.get(t).is_some_and(|tile| tile.top().is_some()));
        let rank = |t: TileId| match self.recent.iter().position(|r| *r == t) {
            Some(_) if self.focused == Some(t) => usize::MAX,
            Some(i) => i,
            None => usize::MAX - 1,
        };
        placed.sort_by_key(|(t, _)| rank(*t));

        let apps = dune_common::desktop::apps();
        placed
            .into_iter()
            .map(|(tile, place)| {
                let app = self.tile_app_id(tile).unwrap_or_default();
                let icon = app_icon(&apps, &app);
                Entry { tile, title: self.tile_title(tile), app, place, icon }
            })
            .collect()
    }
}

/// The icon of the desktop app whose id is `app_id` (or ends in it: `org.gnome.Nautilus`
/// for `nautilus`), else the app id itself as an icon name.
fn app_icon(apps: &[dune_common::desktop::App], app_id: &str) -> Option<String> {
    if app_id.is_empty() {
        return None;
    }
    let named = |a: &&dune_common::desktop::App| {
        a.id.eq_ignore_ascii_case(app_id) || a.id.rsplit('.').next().is_some_and(|last| last.eq_ignore_ascii_case(app_id))
    };
    apps.iter().find(named).and_then(|a| a.icon.clone()).or_else(|| Some(app_id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(tile: TileId, title: &str, app: &str, place: &str) -> Entry {
        Entry { tile, title: title.into(), app: app.into(), place: place.into(), icon: None }
    }

    fn switcher() -> Switcher {
        let mut s = Switcher::new("monospace", "");
        s.open(
            vec![
                entry(1, "dune — nvim", "dune-term", "desk 1"),
                entry(2, "Mozilla Firefox", "firefox", "desk 2"),
                entry(3, "Files", "org.gnome.Nautilus", "desk 3 · m2"),
            ],
            "out".into(),
        );
        s
    }

    fn none() -> ModifiersState {
        ModifiersState::default()
    }

    fn typed(s: &mut Switcher, text: &str) {
        for c in text.chars() {
            assert_eq!(s.feed(&none(), Keysym::a, Some(c)), Step::Continue);
        }
    }

    fn listed(s: &Switcher) -> Vec<TileId> {
        let session = s.session.as_ref().unwrap();
        session.matches.iter().map(|i| session.entries[*i].tile).collect()
    }

    #[test]
    fn enter_right_away_goes_to_the_first_window() {
        let mut s = switcher();
        assert_eq!(listed(&s), vec![1, 2, 3]);
        assert_eq!(s.feed(&none(), Keysym::Return, None), Step::Go(1));
        assert!(!s.is_open());
    }

    #[test]
    fn typing_narrows_fuzzy_over_title_app_and_place() {
        let mut s = switcher();
        typed(&mut s, "ffx");
        assert_eq!(listed(&s), vec![2]);
        s.feed(&none(), Keysym::BackSpace, None);
        s.feed(&none(), Keysym::BackSpace, None);
        s.feed(&none(), Keysym::BackSpace, None);
        // Fuzzy: "m2" fits Mozilla on desk 2 too, but the monitor fits best.
        typed(&mut s, "m2");
        assert_eq!(listed(&s), vec![3, 2]);
        let ctrl = ModifiersState { ctrl: true, ..Default::default() };
        s.feed(&ctrl, Keysym::u, None);
        assert_eq!(listed(&s).len(), 3);
        typed(&mut s, "zzz");
        assert_eq!(listed(&s), Vec::<TileId>::new());
        assert_eq!(s.feed(&none(), Keysym::Return, None), Step::Close);
    }

    #[test]
    fn moving_through_the_list_goes_round() {
        let mut s = switcher();
        s.feed(&none(), Keysym::Up, None);
        assert_eq!(s.session.as_ref().unwrap().selected, 2);
        s.feed(&none(), Keysym::Tab, None);
        s.feed(&none(), Keysym::Down, None);
        assert_eq!(s.feed(&none(), Keysym::Return, None), Step::Go(2));
    }

    #[test]
    fn escape_closes() {
        let mut s = switcher();
        assert_eq!(s.feed(&none(), Keysym::Escape, None), Step::Close);
        assert!(!s.is_open());
    }

    #[test]
    fn the_selected_window_stays_on_screen() {
        assert_eq!(first_shown(0), 0);
        assert_eq!(first_shown(ROWS - 1), 0);
        assert_eq!(first_shown(ROWS + 2), 3);
    }

    #[test]
    fn icons_come_from_the_desktop_app() {
        let app = |id: &str, icon: &str| dune_common::desktop::App {
            id: id.into(),
            name: id.into(),
            exec: String::new(),
            terminal: false,
            icon: Some(icon.into()),
            generic_name: None,
            keywords: Vec::new(),
        };
        let apps = vec![app("org.gnome.Nautilus", "folder"), app("firefox", "firefox-icon")];
        assert_eq!(app_icon(&apps, "firefox").as_deref(), Some("firefox-icon"));
        assert_eq!(app_icon(&apps, "nautilus").as_deref(), Some("folder"));
        assert_eq!(app_icon(&apps, "foot").as_deref(), Some("foot"));
        assert_eq!(app_icon(&apps, ""), None);
    }
}
