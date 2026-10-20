//! The finder: one panel that is the apps (the launcher), the windows (the switcher) or
//! the files, and turns from one into the other: `/` typed first in the apps shows the
//! windows, `#` the files, Backspace with nothing typed there goes back. Keys, clicks and
//! the wheel come in; what to do with dune comes out (`Action`).

use std::path::{Path, PathBuf};

use dune_common::{
    desktop::App,
    protocol::{Dest, TileId, WindowInfo},
};
use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};

use crate::{
    apps::{self, Apps},
    files::{self, Files, quote},
    icons::Icons,
    panel::View,
    windows::{self, Entry, Windows},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Apps,
    Windows,
    Files,
}

/// What the finder wants done.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Continue,
    Close,
    /// Start `command` (see `apps::Step::Launch`), then close.
    Launch {
        dest: Dest,
        command: String,
        terminal: bool,
        app: Option<String>,
    },
    /// Show that window and focus it, then close.
    GoTo(TileId),
}

pub struct Finder {
    mode: Mode,
    apps: Apps,
    /// The windows, asked for when first shown.
    windows: Option<Windows>,
    /// Desktop apps, for the icons of windows.
    desktop_apps: Vec<App>,
    /// Asks dune for its windows.
    load_windows: Box<dyn FnMut() -> Vec<WindowInfo>>,
    /// The files, from `home` when first shown.
    files: Option<Files>,
    home: PathBuf,
    /// The terminal file manager Mod+Enter (or Ctrl+Enter) opens files in (`yazi`).
    file_manager: String,
    /// The terminal `file_manager` runs in (`foot`); `None`: dune's.
    pub file_manager_terminal: Option<String>,
}

impl Finder {
    pub fn new(
        mode: Mode,
        apps: Apps,
        desktop_apps: Vec<App>,
        load_windows: Box<dyn FnMut() -> Vec<WindowInfo>>,
        home: PathBuf,
        file_manager: String,
    ) -> Self {
        let mut finder = Finder {
            mode: Mode::Apps,
            apps,
            windows: None,
            desktop_apps,
            load_windows,
            files: None,
            home,
            file_manager,
            file_manager_terminal: None,
        };
        match mode {
            Mode::Windows => finder.show_windows(),
            Mode::Files => finder.show_files(),
            Mode::Apps => {}
        }
        finder
    }

    #[cfg(test)]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Still on the apps with nothing typed or browsed: a compact panel stays compact.
    pub fn untouched(&self) -> bool {
        self.mode == Mode::Apps && self.apps.untouched()
    }

    /// The windows, listed afresh each time they are shown.
    fn show_windows(&mut self) {
        let entries = (self.load_windows)()
            .into_iter()
            .map(|w| Entry::new(w, &self.desktop_apps))
            .collect();
        self.windows = Some(Windows::new(entries));
        self.mode = Mode::Windows;
    }

    /// The files, from the home folder each time they are shown.
    fn show_files(&mut self) {
        self.files = Some(Files::new(self.home.clone(), &self.file_manager));
        self.mode = Mode::Files;
    }

    /// A key pressed: `sym` with `mods`, typing `text`.
    pub fn feed(&mut self, mods: &Modifiers, sym: Keysym, text: Option<char>) -> Action {
        match self.mode {
            Mode::Apps => {
                let step = self.apps.feed(mods, sym, text);
                self.apps_step(step)
            }
            Mode::Windows => {
                let step = self
                    .windows
                    .as_mut()
                    .map_or(windows::Step::Apps, |w| w.feed(mods, sym, text));
                self.windows_step(step)
            }
            // The Ctrl keys still choose where what is opened goes.
            Mode::Files if self.apps.choose_place(mods, sym) => Action::Continue,
            Mode::Files => {
                let step = self
                    .files
                    .as_mut()
                    .map_or(files::Step::Apps, |f| f.feed(mods, sym, text));
                self.files_step(step)
            }
        }
    }

    /// A click on row `row` of the list (`panel::row_at`); elsewhere on the panel, `None`.
    pub fn click(&mut self, row: Option<usize>) -> Action {
        let Some(row) = row else {
            return Action::Continue;
        };
        match self.mode {
            Mode::Apps => {
                let step = self.apps.click(row);
                self.apps_step(step)
            }
            Mode::Windows => {
                let step = self
                    .windows
                    .as_mut()
                    .map_or(windows::Step::Continue, |w| w.click(row));
                self.windows_step(step)
            }
            Mode::Files => {
                let step = self
                    .files
                    .as_mut()
                    .map_or(files::Step::Continue, |f| f.click(row));
                self.files_step(step)
            }
        }
    }

    /// The wheel turned over the panel: the selection moves down (or up).
    pub fn scroll(&mut self, down: bool) -> Action {
        let sym = if down { Keysym::Down } else { Keysym::Up };
        self.feed(&Modifiers::default(), sym, None)
    }

    fn apps_step(&mut self, step: apps::Step) -> Action {
        match step {
            apps::Step::Continue => Action::Continue,
            apps::Step::Close => Action::Close,
            apps::Step::Launch {
                dest,
                command,
                terminal,
                app,
            } => Action::Launch {
                dest,
                command,
                terminal,
                app,
            },
            apps::Step::Windows => {
                self.show_windows();
                Action::Continue
            }
            apps::Step::Files => {
                self.show_files();
                Action::Continue
            }
        }
    }

    fn files_step(&mut self, step: files::Step) -> Action {
        let dest = self.apps.destination();
        match step {
            files::Step::Continue => Action::Continue,
            files::Step::Close => Action::Close,
            files::Step::Open(path) => Action::Launch {
                dest,
                command: open_command(&path),
                terminal: false,
                app: None,
            },
            files::Step::Manage(path) => {
                let command = manage_command(&self.file_manager, &path);
                match &self.file_manager_terminal {
                    // A terminal of its own is a graphical app, started as one.
                    Some(terminal) => Action::Launch {
                        dest,
                        command: format!("{terminal} {command}"),
                        terminal: false,
                        app: None,
                    },
                    None => Action::Launch {
                        dest,
                        command,
                        terminal: true,
                        app: None,
                    },
                }
            }
            files::Step::Apps => {
                self.mode = Mode::Apps;
                Action::Continue
            }
        }
    }

    fn windows_step(&mut self, step: windows::Step) -> Action {
        match step {
            windows::Step::Continue => Action::Continue,
            windows::Step::Close => Action::Close,
            windows::Step::Go(tile) => Action::GoTo(tile),
            windows::Step::Apps => {
                self.mode = Mode::Apps;
                Action::Continue
            }
        }
    }

    /// What the panel shows; icons of `icon_size` pixels.
    pub fn view<'a>(&'a self, icons: &'a mut Icons, icon_size: u32) -> View<'a> {
        match (self.mode, &self.windows, &self.files) {
            (Mode::Windows, Some(windows), _) => windows.view(icons, icon_size),
            (Mode::Files, _, Some(files)) => files.view(icons, icon_size, self.apps.status()),
            _ => self.apps.view(icons, icon_size),
        }
    }
}

/// Opens `path` with the app for its type.
fn open_command(path: &Path) -> String {
    format!("xdg-open {}", quote(path))
}

/// `file_manager` at the folder `path`, or with the file `path` selected (yazi does both).
fn manage_command(file_manager: &str, path: &Path) -> String {
    format!("{file_manager} {}", quote(path))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn finder(mode: Mode) -> Finder {
        let apps = vec![App {
            id: "firefox".into(),
            name: "Firefox".into(),
            exec: "firefox %u".into(),
            ..Default::default()
        }];
        let windows = || {
            vec![WindowInfo {
                tile: 4,
                title: "Mozilla Firefox".into(),
                app: "firefox".into(),
                place: "desk 1".into(),
            }]
        };
        let home = std::env::temp_dir().join(format!("dune-finder-home-{}", std::process::id()));
        std::fs::create_dir_all(home.join("Music")).unwrap();
        std::fs::write(home.join("notes.txt"), "").unwrap();
        Finder::new(
            mode,
            Apps::new(apps.clone(), HashMap::new(), false),
            apps,
            Box::new(windows),
            home,
            "yazi".into(),
        )
    }

    fn key(f: &mut Finder, sym: Keysym) -> Action {
        f.feed(&Modifiers::default(), sym, sym.key_char())
    }

    #[test]
    fn a_slash_turns_the_apps_into_the_windows_and_backspace_back() {
        let mut f = finder(Mode::Apps);
        assert_eq!(key(&mut f, Keysym::slash), Action::Continue);
        assert_eq!(f.mode(), Mode::Windows);
        assert_eq!(key(&mut f, Keysym::BackSpace), Action::Continue);
        assert_eq!(f.mode(), Mode::Apps);
        assert!(
            matches!(key(&mut f, Keysym::Return), Action::Launch { command, .. } if command == "firefox")
        );
    }

    #[test]
    fn windows_first_goes_to_a_window() {
        let mut f = finder(Mode::Windows);
        assert_eq!(f.mode(), Mode::Windows);
        assert_eq!(f.click(Some(0)), Action::GoTo(4));
        assert_eq!(f.click(None), Action::Continue);
        assert_eq!(key(&mut f, Keysym::Escape), Action::Close);
    }

    #[test]
    fn a_hash_shows_the_files_and_opens_them_where_the_apps_would() {
        let mut f = finder(Mode::Apps);
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        f.feed(&ctrl, Keysym::Left, None); // to the left, chosen in the apps
        assert_eq!(
            f.feed(&Modifiers::default(), Keysym::numbersign, Some('#')),
            Action::Continue
        );
        assert_eq!(f.mode(), Mode::Files);
        f.feed(&ctrl, Keysym::f, None); // floating, chosen in the files
        let home = f.home.clone();
        // Rows: `..`, Music, notes.txt.
        assert_eq!(
            f.click(Some(2)),
            Action::Launch {
                dest: Dest {
                    place: dune_common::protocol::Place::Float,
                    ..Dest::default()
                },
                command: open_command(&home.join("notes.txt")),
                terminal: false,
                app: None
            }
        );
        let mut f = finder(Mode::Files);
        key(&mut f, Keysym::Down); // Music
        assert!(matches!(
            f.feed(&ctrl, Keysym::Return, None),
            Action::Launch { command, terminal: true, .. } if command == manage_command("yazi", &home.join("Music"))
        ));
        // In a terminal of its own: started as a graphical app.
        let mut f = finder(Mode::Files);
        f.file_manager_terminal = Some("foot".into());
        key(&mut f, Keysym::Down); // Music
        assert!(matches!(
            f.feed(&ctrl, Keysym::Return, None),
            Action::Launch { command, terminal: false, .. } if command == format!("foot {}", manage_command("yazi", &home.join("Music")))
        ));
        let mut f = finder(Mode::Files);
        assert_eq!(key(&mut f, Keysym::BackSpace), Action::Continue);
        assert_eq!(f.mode(), Mode::Apps);
    }

    #[test]
    fn file_commands_quote_the_path() {
        assert_eq!(
            open_command(Path::new("/home/me/a b.pdf")),
            "xdg-open '/home/me/a b.pdf'"
        );
        assert_eq!(
            manage_command("yazi", Path::new("/home/me/Music")),
            "yazi '/home/me/Music'"
        );
        assert_eq!(
            manage_command("yazi", Path::new("/home/me/it's.txt")),
            r"yazi '/home/me/it'\''s.txt'"
        );
    }

    #[test]
    fn the_wheel_moves_the_selection() {
        let mut f = finder(Mode::Windows);
        assert_eq!(f.scroll(true), Action::Continue);
        assert_eq!(key(&mut f, Keysym::Return), Action::GoTo(4)); // one window: round to it
    }
}
