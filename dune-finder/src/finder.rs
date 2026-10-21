//! The finder: one panel that is the apps (the launcher), the windows (the switcher), the
//! files or the folders to jump to, and turns from one into the other: `/` typed first in
//! the apps shows the windows, `#` the files, `>` the jumps, Backspace with nothing typed
//! there goes back. Started on the tile options (Mod+/), it stays on them. Keys, clicks and
//! the wheel come in; what to do with dune comes out (`Action`).

use std::path::{Path, PathBuf};

use dune_common::{
    desktop::App,
    protocol::{Dest, TileAction, TileId, TileOption, WindowInfo},
};
use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};

use crate::{
    apps::{self, Apps},
    files::{self, Files, quote},
    icons::Icons,
    jump::{self, Jump},
    panel::View,
    tile_options::{self, TileOptions},
    windows::{self, Entry, Windows},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Apps,
    Windows,
    Files,
    Jump,
    /// The focused tile's options.
    TileOptions,
}

/// What the finder wants done.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Continue,
    Close,
    /// Start `command` (see `apps::Step::Launch`), then close. `folder`: the folder it
    /// is about, counted as a visit for the jumps (and where a terminal starts).
    Launch {
        dest: Dest,
        command: String,
        terminal: bool,
        app: Option<String>,
        folder: Option<PathBuf>,
    },
    /// Show that window and focus it, then close.
    GoTo(TileId),
    /// Unlock the desktop of that window and go to it (`:l`), then close.
    Unlock(TileId),
    /// Do that to the tile the finder was opened for, then close.
    TileAction(TileAction),
}

pub struct Finder {
    mode: Mode,
    apps: Apps,
    /// The windows, asked for when first shown.
    windows: Option<Windows>,
    /// Desktop apps, for the icons of windows.
    desktop_apps: Vec<App>,
    /// Asks dune for its windows: those of locked desktops with `true` (`:l`), else the rest.
    load_windows: Box<dyn FnMut(bool) -> Vec<WindowInfo>>,
    /// The files, from `home` when first shown.
    files: Option<Files>,
    /// The folders to jump to, asked for when first shown.
    jump: Option<Jump>,
    /// The tile options, given by `show_tile_options`.
    tile_options: Option<TileOptions>,
    /// The recorded folders, the highest frecency first.
    load_jumps: Box<dyn FnMut() -> Vec<PathBuf>>,
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
        load_windows: Box<dyn FnMut(bool) -> Vec<WindowInfo>>,
        load_jumps: Box<dyn FnMut() -> Vec<PathBuf>>,
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
            jump: None,
            tile_options: None,
            load_jumps,
            home,
            file_manager,
            file_manager_terminal: None,
        };
        match mode {
            Mode::Windows => finder.show_windows(),
            Mode::Files => finder.show_files(),
            Mode::Jump => finder.show_jump(),
            Mode::TileOptions => finder.show_tile_options(Vec::new()),
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
        let entries = (self.load_windows)(false)
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

    /// The folders to jump to, listed afresh each time they are shown; gone ones left out.
    fn show_jump(&mut self) {
        let folders = (self.load_jumps)().into_iter().filter(|f| f.is_dir()).collect();
        self.jump = Some(Jump::new(folders, &self.home, &self.file_manager));
        self.mode = Mode::Jump;
    }

    /// The tile options, the rows dune gave (`options`).
    pub fn show_tile_options(&mut self, options: Vec<TileOption>) {
        self.tile_options = Some(TileOptions::new(options));
        self.mode = Mode::TileOptions;
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
            Mode::Jump if self.apps.choose_place(mods, sym) => Action::Continue,
            Mode::Jump => {
                let step = self.jump.as_mut().map_or(jump::Step::Apps, |j| j.feed(mods, sym, text));
                self.jump_step(step)
            }
            Mode::TileOptions => {
                let step = self
                    .tile_options
                    .as_mut()
                    .map_or(tile_options::Step::Close, |o| o.feed(mods, sym, text));
                tile_options_step(step)
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
            Mode::Jump => {
                let step = self.jump.as_mut().map_or(jump::Step::Continue, |j| j.click(row));
                self.jump_step(step)
            }
            Mode::TileOptions => {
                let step = self
                    .tile_options
                    .as_mut()
                    .map_or(tile_options::Step::Continue, |o| o.click(row));
                tile_options_step(step)
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
                folder: None,
            },
            apps::Step::Windows => {
                self.show_windows();
                Action::Continue
            }
            apps::Step::Files => {
                self.show_files();
                Action::Continue
            }
            apps::Step::Jump => {
                self.show_jump();
                Action::Continue
            }
        }
    }

    fn jump_step(&mut self, step: jump::Step) -> Action {
        match step {
            jump::Step::Continue => Action::Continue,
            jump::Step::Close => Action::Close,
            jump::Step::Go(folder) => Action::Launch {
                dest: self.apps.destination(),
                command: String::new(),
                terminal: true,
                app: None,
                folder: Some(folder),
            },
            jump::Step::Manage(folder) => self.manage(folder.clone(), folder),
            jump::Step::Apps => {
                self.mode = Mode::Apps;
                Action::Continue
            }
        }
    }

    /// The file manager at `path` (a folder, or a file to select), `folder` its folder.
    fn manage(&self, path: PathBuf, folder: PathBuf) -> Action {
        let dest = self.apps.destination();
        let command = manage_command(&self.file_manager, &path);
        let folder = Some(folder);
        match &self.file_manager_terminal {
            // A terminal of its own is a graphical app, started as one.
            Some(terminal) => Action::Launch {
                dest,
                command: format!("{terminal} {command}"),
                terminal: false,
                app: None,
                folder,
            },
            None => Action::Launch {
                dest,
                command,
                terminal: true,
                app: None,
                folder,
            },
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
                folder: path.parent().map(Path::to_path_buf),
            },
            files::Step::Manage(path) => {
                let folder = if path.is_dir() { path.clone() } else { path.parent().map_or_else(|| path.clone(), Path::to_path_buf) };
                self.manage(path, folder)
            }
            files::Step::Apps => {
                self.mode = Mode::Apps;
                Action::Continue
            }
        }
    }

    fn windows_step(&mut self, step: windows::Step) -> Action {
        match step {
            windows::Step::Continue => {
                // `:l` typed: the windows of locked desktops, asked for now.
                if let Some(windows) = self.windows.as_mut().filter(|w| w.wants_locked()) {
                    let locked = (self.load_windows)(true).into_iter().map(|w| Entry::new(w, &self.desktop_apps)).collect();
                    windows.set_locked(locked);
                }
                Action::Continue
            }
            windows::Step::Close => Action::Close,
            windows::Step::Go(tile) => Action::GoTo(tile),
            windows::Step::Unlock(tile) => Action::Unlock(tile),
            windows::Step::Apps => {
                self.mode = Mode::Apps;
                Action::Continue
            }
        }
    }

    /// What the panel shows; icons of `icon_size` pixels.
    pub fn view<'a>(&'a self, icons: &'a mut Icons, icon_size: u32) -> View<'a> {
        if let (Mode::Jump, Some(jump)) = (self.mode, &self.jump) {
            return jump.view(icons, icon_size);
        }
        if let (Mode::TileOptions, Some(options)) = (self.mode, &self.tile_options) {
            return options.view();
        }
        match (self.mode, &self.windows, &self.files) {
            (Mode::Windows, Some(windows), _) => windows.view(icons, icon_size),
            (Mode::Files, _, Some(files)) => files.view(icons, icon_size, self.apps.status()),
            _ => self.apps.view(icons, icon_size),
        }
    }
}

fn tile_options_step(step: tile_options::Step) -> Action {
    match step {
        tile_options::Step::Continue => Action::Continue,
        tile_options::Step::Close => Action::Close,
        tile_options::Step::Do(action) => Action::TileAction(action),
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
        let windows = |locked: bool| {
            vec![WindowInfo {
                tile: if locked { 5 } else { 4 },
                title: if locked { "Slack".into() } else { "Mozilla Firefox".into() },
                app: "firefox".into(),
                place: "desk 1".into(),
            }]
        };
        let home = std::env::temp_dir().join(format!("dune-finder-home-{}", std::process::id()));
        std::fs::create_dir_all(home.join("Music")).unwrap();
        std::fs::write(home.join("notes.txt"), "").unwrap();
        let jump_dir = home.join("Music");
        Finder::new(
            mode,
            Apps::new(apps.clone(), HashMap::new(), false),
            apps,
            Box::new(windows),
            Box::new(move || vec![jump_dir.clone(), "/no/such/folder".into()]),
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
                app: None,
                folder: Some(home.clone()),
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
    fn a_gt_shows_the_jumps_and_enter_opens_a_terminal_there() {
        let mut f = finder(Mode::Apps);
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        f.feed(&ctrl, Keysym::f, None); // floating, chosen in the apps
        assert_eq!(f.feed(&Modifiers::default(), Keysym::greater, Some('>')), Action::Continue);
        assert_eq!(f.mode(), Mode::Jump);
        let music = f.home.join("Music");
        // The folder that is gone is not listed: Enter takes the one there is.
        assert_eq!(
            key(&mut f, Keysym::Return),
            Action::Launch {
                dest: Dest {
                    place: dune_common::protocol::Place::Float,
                    ..Dest::default()
                },
                command: String::new(),
                terminal: true,
                app: None,
                folder: Some(music.clone()),
            }
        );
        let mut f = finder(Mode::Jump);
        assert!(matches!(
            f.feed(&ctrl, Keysym::Return, None),
            Action::Launch { command, folder: Some(folder), .. } if command == manage_command("yazi", &music) && folder == music
        ));
        let mut f = finder(Mode::Jump);
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
    fn tile_options_choose_an_action_and_stay_there() {
        let mut f = finder(Mode::TileOptions);
        let option = |action, on| TileOption { action, on, keys: String::new() };
        f.show_tile_options(vec![
            option(TileAction::PrivateTile, Some(false)),
            option(TileAction::Float, None),
            option(TileAction::Close, None),
        ]);
        assert_eq!(f.mode(), Mode::TileOptions);
        // `/` is typed as a filter here, not a way to the windows.
        assert_eq!(key(&mut f, Keysym::slash), Action::Continue);
        assert_eq!(f.mode(), Mode::TileOptions);
        assert_eq!(key(&mut f, Keysym::BackSpace), Action::Continue);
        assert_eq!(f.click(Some(2)), Action::TileAction(TileAction::Close));
        assert_eq!(f.feed(&Modifiers::default(), Keysym::f, Some('f')), Action::Continue);
        assert_eq!(key(&mut f, Keysym::Return), Action::TileAction(TileAction::Float));
        assert_eq!(key(&mut f, Keysym::Escape), Action::Close);
    }

    #[test]
    fn colon_l_asks_for_the_locked_windows_and_unlocks_the_one_chosen() {
        let mut f = finder(Mode::Windows);
        for c in ":l".chars() {
            assert_eq!(f.feed(&Modifiers::default(), Keysym::l, Some(c)), Action::Continue);
        }
        assert_eq!(key(&mut f, Keysym::Return), Action::Unlock(5));
    }

    #[test]
    fn the_wheel_moves_the_selection() {
        let mut f = finder(Mode::Windows);
        assert_eq!(f.scroll(true), Action::Continue);
        assert_eq!(key(&mut f, Keysym::Return), Action::GoTo(4)); // one window: round to it
    }
}
