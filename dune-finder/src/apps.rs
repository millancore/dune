//! The apps mode (Mod+d): it searches the desktop applications with the picker and lets
//! the user choose, before Enter, where the app opens: on a side of the focused tile,
//! floating, as a tab of it, on another monitor or desktop (`firefox :d2`, see `Dest::with`).
//! Graphical apps start on their own, like rofi does; console programs get a terminal.
//!
//! Opened for an empty container (Mod+Enter, `slot`), Enter right away puts a terminal
//! there; anything chosen or typed goes there too.

use std::{collections::HashMap, path::PathBuf};

use dune_common::{
    desktop::App,
    picker::{Key, Outcome, Picker},
    protocol::{Dest, Place, Side},
};
use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};

use crate::{
    icons::Icons,
    panel::{Row, View},
};

const HINT: &str = ":d1-9: on that desktop · :m1-9: on that monitor · :t: as a tab of this tile · :f: floating \
    · Ctrl+arrows: on that side of this tile · Ctrl+F Ctrl+T: floating, as a tab · /: windows · #: files \
    · >: jump to a folder · Esc: close";
const DEST_HELP: &str = ":  d1-9 desktop · m1-m9 monitor · t tab · n new · f float";
const SLOT_HINT: &str = "Enter: a terminal here · ↑↓ or type: another program · /: windows · #: files \
    · >: jump to a folder · Esc: close";

/// What a key did in the apps mode.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Close,
    /// Run `command` at `dest` (into the empty container, when opened for one), in a
    /// terminal when `terminal` (a console program). `app`: the id of the app it runs,
    /// None for a typed command.
    Launch {
        dest: Dest,
        command: String,
        terminal: bool,
        app: Option<String>,
    },
    /// `/` typed first: go to the windows.
    Windows,
    /// `#` typed first: go to the files.
    Files,
    /// `>` typed first: go to the folders to jump to.
    Jump,
    /// `@` typed first: go to the system actions.
    System,
}

pub struct Apps {
    picker: Picker,
    tab: bool,
    /// Graphical apps (no `Terminal=true`): what they run starts without a terminal.
    gui: Vec<App>,
    place: Place,
    /// Opened for an empty container (Mod+Enter).
    slot: bool,
    /// The list was moved through: Enter with nothing typed runs the highlighted app,
    /// not a terminal.
    browsed: bool,
    /// The user's terminal heads the list (`with_terminal`): in an empty container it is
    /// highlighted from the start, as Enter gives a terminal.
    terminal_first: bool,
}

impl Apps {
    /// The apps mode over `apps`; `uses`: launches by app id, the most used are listed first.
    pub fn new(apps: Vec<App>, uses: HashMap<String, u32>, slot: bool) -> Self {
        let home: PathBuf = std::env::var_os("HOME")
            .map(Into::into)
            .unwrap_or_else(|| "/".into());
        let gui = apps.iter().filter(|a| !a.terminal).cloned().collect();
        Apps {
            picker: Picker::browser(apps, home, uses),
            tab: false,
            gui,
            place: Place::Column,
            slot,
            browsed: false,
            terminal_first: false,
        }
    }

    /// The app running `terminal` (dune's `terminal` command) heads the list shown with
    /// nothing typed.
    pub fn with_terminal(mut self, terminal: &str) -> Self {
        let first = terminal.split_whitespace().next().unwrap_or("");
        let program = first.rsplit('/').next().unwrap_or(first);
        // A terminal emulator is a graphical app.
        if let Some(id) = self.gui.iter().find(|a| !program.is_empty() && a.program() == program).map(|a| a.id.clone()) {
            self.picker.pin_first(&id);
            self.terminal_first = true;
        }
        self
    }

    /// In an empty container, Enter now gives a terminal.
    fn enter_is_terminal(&self) -> bool {
        self.slot && !self.browsed && self.picker.input().trim().is_empty()
    }

    /// Nothing typed: `?` there lists the keys.
    pub fn nothing_typed(&self) -> bool {
        self.picker.input().is_empty()
    }

    /// Nothing typed or browsed yet: a compact panel shows no list.
    pub fn untouched(&self) -> bool {
        !self.browsed && self.picker.input().is_empty()
    }

    /// Where the Ctrl keys say the app goes.
    fn keys_dest(&self) -> Dest {
        Dest { place: self.place, tab: self.tab, monitor: None, desk: None }
    }

    /// The Ctrl keys' choice changed by the `:` word typed; None when that word is wrong.
    fn dest(&self) -> Option<Dest> {
        match self.picker.dest() {
            Some(word) => self.keys_dest().with(word),
            None => Some(self.keys_dest()),
        }
    }

    /// Where what is chosen goes: the Ctrl keys' choice with the `:` word typed.
    pub fn destination(&self) -> Dest {
        self.dest().unwrap_or(self.keys_dest())
    }

    /// Where what is chosen opens, besides a side of this tile (`side`), at the prompt
    /// line's right: empty for a new column, or in the empty container it was opened for.
    pub fn note(&self) -> String {
        if self.slot {
            return String::new();
        }
        let Some(dest) = self.dest() else { return DEST_HELP.into() };
        if dest.tab {
            return "as a tab".into();
        }
        let mut parts = Vec::new();
        if dest.place == Place::Float {
            parts.push("floating".to_string());
        }
        parts.extend(dest.monitor.map(|m| format!("monitor {m}")));
        parts.extend(dest.desk.map(|d| format!("desktop {d}")));
        parts.join(" · ")
    }

    /// The side of the focused tile what is chosen opens on, if one.
    pub fn side(&self) -> Option<Side> {
        match self.dest()? {
            Dest { tab: false, place: Place::Side(side), .. } if !self.slot => Some(side),
            _ => None,
        }
    }

    /// Ctrl+arrows, Ctrl+F and Ctrl+T choose where things open; whether `sym` was one.
    /// For an empty container the place is that container: Ctrl keys choose none.
    pub fn choose_place(&mut self, mods: &Modifiers, sym: Keysym) -> bool {
        if !mods.ctrl || self.slot {
            return false;
        }
        let place = match sym {
            Keysym::Left => Place::Side(Side::Left),
            Keysym::Right => Place::Side(Side::Right),
            Keysym::Up => Place::Side(Side::Up),
            Keysym::Down => Place::Side(Side::Down),
            Keysym::f | Keysym::F if self.place == Place::Float => Place::Column,
            Keysym::f | Keysym::F => Place::Float,
            Keysym::t | Keysym::T => {
                self.tab = !self.tab;
                return true;
            }
            _ => return false,
        };
        self.place = place;
        true
    }

    /// A key pressed: `sym` with `mods`, typing `text`.
    pub fn feed(&mut self, mods: &Modifiers, sym: Keysym, text: Option<char>) -> Step {
        if self.choose_place(mods, sym) {
            return Step::Continue;
        }
        let key = match sym {
            Keysym::Escape => Key::Esc,
            Keysym::Return | Keysym::KP_Enter => Key::Enter,
            Keysym::BackSpace => Key::Backspace,
            Keysym::Tab => Key::Tab,
            Keysym::ISO_Left_Tab => Key::BackTab,
            Keysym::Up => Key::Up,
            Keysym::Down => Key::Down,
            Keysym::c if mods.ctrl => Key::CtrlC,
            Keysym::u if mods.ctrl => Key::CtrlU,
            Keysym::w if mods.ctrl => Key::CtrlW,
            _ => match text {
                Some(c) if !c.is_control() && !mods.ctrl && !mods.alt && !mods.logo => Key::Char(c),
                _ => Key::Other,
            },
        };
        match key {
            Key::Char('/') if self.picker.input().is_empty() => return Step::Windows,
            Key::Char('#') if self.picker.input().is_empty() => return Step::Files,
            Key::Char('>') if self.picker.input().is_empty() => return Step::Jump,
            Key::Char('@') if self.picker.input().is_empty() => return Step::System,
            _ => {}
        }
        // Nothing is highlighted yet: the first Down lands on the first app.
        let first_down = key == Key::Down && self.enter_is_terminal() && !self.terminal_first;
        if matches!(key, Key::Up | Key::Down | Key::Tab | Key::BackTab) {
            self.browsed = true;
        }
        if first_down {
            return Step::Continue;
        }
        if key == Key::Enter && self.enter_is_terminal() {
            return Step::Launch {
                dest: self.keys_dest(),
                command: String::new(),
                terminal: true,
                app: None,
            };
        }
        match self.picker.feed(key) {
            Outcome::Continue => Step::Continue,
            Outcome::Cancel => Step::Close,
            Outcome::Run(line) => {
                let dest = self.destination();
                let terminal = !runs_gui_app(&self.gui, &line.command);
                let app = self.picker.chosen().map(str::to_string);
                Step::Launch {
                    dest,
                    command: line.command,
                    terminal,
                    app,
                }
            }
        }
    }

    /// Row `row` of the list was clicked: run what it shows.
    pub fn click(&mut self, row: usize) -> Step {
        let (first, rows) = self.picker.window();
        if row >= rows.len() {
            return Step::Continue;
        }
        self.picker.select(first + row);
        self.browsed = true;
        self.feed(&Modifiers::default(), Keysym::Return, None)
    }

    /// What the panel shows; icons of `icon_size` pixels.
    pub fn view<'a>(&'a self, icons: &'a mut Icons, icon_size: u32) -> View<'a> {
        let (first, candidates) = self.picker.window();
        for name in candidates.iter().filter_map(|c| c.icon.as_deref()) {
            icons.get(name, icon_size);
        }
        let icons: &Icons = icons;
        let rows = candidates
            .iter()
            .enumerate()
            .map(|(i, c)| Row {
                icon: c
                    .icon
                    .as_deref()
                    .and_then(|name| icons.cached(name, icon_size)),
                text: &c.label,
                detail: None,
                sub: None,
                selected: first + i == self.picker.selected() && (self.terminal_first || !self.enter_is_terminal()),
            })
            .collect();
        let hint = if self.slot { SLOT_HINT } else { HINT };
        View {
            prompt: format!("{}▏", self.picker.input()),
            rows,
            placeholder: "",
            note: self.note(),
            side: self.side(),
            hint,
        }
    }
}

/// Whether `command` is what one of the `gui` apps runs (with or without arguments).
/// Anything else, typed by hand, may be a console program and gets a terminal.
fn runs_gui_app(gui: &[App], command: &str) -> bool {
    gui.iter().any(|app| {
        let base = app.command("");
        !base.is_empty() && command.strip_prefix(&base).is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apps() -> Vec<App> {
        vec![
            App {
                id: "firefox".into(),
                name: "Firefox".into(),
                exec: "firefox %u".into(),
                icon: Some("firefox".into()),
                ..Default::default()
            },
            App {
                id: "nvim".into(),
                name: "Neovim".into(),
                exec: "nvim %F".into(),
                terminal: true,
                ..Default::default()
            },
        ]
    }

    fn launcher() -> Apps {
        Apps::new(apps(), HashMap::new(), false)
    }

    /// The apps of Mod+Enter, for an empty container.
    fn in_slot() -> Apps {
        Apps::new(apps(), HashMap::new(), true)
    }

    fn column() -> Dest {
        Dest::default()
    }

    fn press(a: &mut Apps, sym: Keysym, ctrl: bool) -> Step {
        let mods = Modifiers {
            ctrl,
            ..Default::default()
        };
        a.feed(&mods, sym, sym.key_char())
    }

    fn typed(a: &mut Apps, text: &str) {
        for c in text.chars() {
            a.feed(&Modifiers::default(), Keysym::from_char(c), Some(c));
        }
    }

    fn launch(command: &str, terminal: bool, app: Option<&str>) -> Step {
        Step::Launch {
            dest: column(),
            command: command.into(),
            terminal,
            app: app.map(Into::into),
        }
    }

    #[test]
    fn typing_or_browsing_touches_it() {
        let mut a = in_slot();
        assert!(a.untouched());
        press(&mut a, Keysym::Down, false);
        assert!(!a.untouched());
        let mut a = in_slot();
        typed(&mut a, "f");
        assert!(!a.untouched());
    }

    #[test]
    fn enter_alone_in_an_empty_container_is_a_terminal() {
        let mut a = in_slot();
        assert_eq!(press(&mut a, Keysym::Return, false), launch("", true, None));
    }

    #[test]
    fn a_side_of_this_tile_shows_as_a_mini_tile() {
        let mut a = launcher();
        assert_eq!((a.side(), a.note()), (None, String::new()));
        press(&mut a, Keysym::Right, true);
        assert_eq!((a.side(), a.note()), (Some(Side::Right), String::new()));
        // As a tab it opens on no side.
        press(&mut a, Keysym::t, true);
        assert_eq!((a.side(), a.note()), (None, "as a tab".to_string()));
        // In the empty container it was opened for: nothing to say.
        let mut a = in_slot();
        press(&mut a, Keysym::Left, true);
        assert_eq!((a.side(), a.note()), (None, String::new()));
    }

    #[test]
    fn what_is_chosen_goes_into_the_empty_container() {
        let mut a = in_slot();
        // Ctrl keys choose no place there.
        press(&mut a, Keysym::Left, true);
        typed(&mut a, "fire");
        assert_eq!(
            press(&mut a, Keysym::Return, false),
            launch("firefox", false, Some("firefox"))
        );
        let mut a = in_slot();
        typed(&mut a, "htop");
        assert_eq!(
            press(&mut a, Keysym::Return, false),
            launch("htop", true, None)
        );
        // Typed and erased: Enter gives a terminal again.
        let mut a = in_slot();
        typed(&mut a, "x");
        press(&mut a, Keysym::BackSpace, false);
        assert_eq!(press(&mut a, Keysym::Return, false), launch("", true, None));
    }

    #[test]
    fn browsing_in_an_empty_container_runs_the_highlighted_app() {
        let mut a = in_slot();
        press(&mut a, Keysym::Down, false);
        press(&mut a, Keysym::Up, false);
        assert!(matches!(
            press(&mut a, Keysym::Return, false),
            Step::Launch { app: Some(_), .. }
        ));
        let mut a = in_slot();
        assert_eq!(press(&mut a, Keysym::Escape, false), Step::Close);
    }

    #[test]
    fn the_first_down_in_an_empty_container_highlights_the_first_app() {
        let mut a = in_slot();
        press(&mut a, Keysym::Down, false);
        assert_eq!(a.picker.selected(), 0);
        press(&mut a, Keysym::Down, false);
        assert_eq!(a.picker.selected(), 1);
    }

    #[test]
    fn in_an_empty_container_the_terminal_is_highlighted_from_the_start() {
        let mut list = apps();
        list.push(App {
            id: "org.wezfurlong.wezterm".into(),
            name: "WezTerm".into(),
            exec: "wezterm start --cwd .".into(),
            ..Default::default()
        });
        let mut a = Apps::new(list, HashMap::new(), true).with_terminal("wezterm start");
        let mut icons = Icons::new("");
        assert!(a.view(&mut icons, 16).rows[0].selected);
        // Enter: the container's own terminal; Down: the next app.
        assert_eq!(press(&mut a, Keysym::Return, false), launch("", true, None));
        press(&mut a, Keysym::Down, false);
        assert_eq!(a.picker.selected(), 1);
    }

    #[test]
    fn opens_in_a_new_column_by_default() {
        let mut a = launcher();
        typed(&mut a, "fire");
        assert_eq!(
            press(&mut a, Keysym::Return, false),
            launch("firefox", false, Some("firefox"))
        );
    }

    #[test]
    fn only_console_programs_get_a_terminal() {
        let mut a = launcher();
        typed(&mut a, "nv");
        assert!(matches!(
            press(&mut a, Keysym::Return, false),
            Step::Launch { terminal: true, .. }
        ));

        let gui = [App {
            id: "firefox".into(),
            name: "Firefox".into(),
            exec: "firefox %u".into(),
            ..Default::default()
        }];
        assert!(runs_gui_app(&gui, "firefox"));
        assert!(runs_gui_app(&gui, "firefox https://a"));
        assert!(!runs_gui_app(&gui, "firefoxy"));
        assert!(!runs_gui_app(&gui, "htop"));
    }

    #[test]
    fn ctrl_keys_choose_the_place() {
        let mut a = launcher();
        press(&mut a, Keysym::Up, true);
        typed(&mut a, "neo");
        assert_eq!(a.picker.input(), "neo");
        assert!(matches!(
            press(&mut a, Keysym::Return, false),
            Step::Launch {
                dest: Dest {
                    place: Place::Side(Side::Up),
                    ..
                },
                ..
            }
        ));

        let mut a = launcher();
        press(&mut a, Keysym::f, true);
        assert!(matches!(
            press(&mut a, Keysym::Return, false),
            Step::Launch {
                dest: Dest {
                    place: Place::Float,
                    ..
                },
                ..
            }
        ));
        // Ctrl+f again goes back to a column.
        let mut a = launcher();
        press(&mut a, Keysym::f, true);
        press(&mut a, Keysym::f, true);
        assert!(matches!(
            press(&mut a, Keysym::Return, false),
            Step::Launch {
                dest: Dest {
                    place: Place::Column,
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn typing_and_erasing_keeps_working() {
        let mut a = launcher();
        typed(&mut a, "xyz");
        for _ in 0..5 {
            assert_eq!(press(&mut a, Keysym::BackSpace, false), Step::Continue);
        }
        typed(&mut a, "nv");
        assert!(
            matches!(press(&mut a, Keysym::Return, false), Step::Launch { command, .. } if command == "nvim")
        );
    }

    #[test]
    fn colon_words_choose_the_destination() {
        let r = column();
        let mut a = launcher();
        typed(&mut a, "fire :d2");
        assert_eq!(
            press(&mut a, Keysym::Return, false),
            Step::Launch {
                dest: Dest { desk: Some(2), ..r },
                command: "firefox".into(),
                terminal: false,
                app: Some("firefox".into())
            }
        );
        // A wrong or unfinished `:` word: the help says what fits.
        let mut a = launcher();
        typed(&mut a, "fire :d");
        assert_eq!(a.note(), DEST_HELP);
        typed(&mut a, "3");
        assert_eq!(a.note(), "desktop 3");
        assert_eq!(a.side(), None);
        // Ctrl+t opens as a tab; `:n` undoes it.
        let mut a = launcher();
        press(&mut a, Keysym::t, true);
        typed(&mut a, "fire");
        assert!(matches!(
            press(&mut a, Keysym::Return, false),
            Step::Launch {
                dest: Dest { tab: true, .. },
                ..
            }
        ));
        let mut a = launcher();
        press(&mut a, Keysym::t, true);
        typed(&mut a, "fire :n");
        assert!(matches!(
            press(&mut a, Keysym::Return, false),
            Step::Launch {
                dest: Dest { tab: false, .. },
                ..
            }
        ));
    }

    #[test]
    fn typed_commands_are_not_apps() {
        let mut a = launcher();
        typed(&mut a, "true");
        assert!(matches!(
            press(&mut a, Keysym::Return, false),
            Step::Launch { app: None, .. }
        ));
    }

    #[test]
    fn escape_closes() {
        let mut a = launcher();
        assert_eq!(press(&mut a, Keysym::Escape, false), Step::Close);
    }

    #[test]
    fn a_slash_first_goes_to_the_windows_and_a_hash_to_the_files() {
        let mut a = launcher();
        assert_eq!(
            a.feed(&Modifiers::default(), Keysym::numbersign, Some('#')),
            Step::Files
        );
        assert_eq!(press(&mut a, Keysym::slash, false), Step::Windows);
        assert_eq!(a.feed(&Modifiers::default(), Keysym::at, Some('@')), Step::System);
        // Inside a command it is just typed (a path).
        typed(&mut a, "ls");
        assert_eq!(
            a.feed(&Modifiers::default(), Keysym::slash, Some('/')),
            Step::Continue
        );
        assert_eq!(a.picker.input(), "ls/");
    }

    #[test]
    fn a_click_runs_the_row() {
        let mut a = in_slot();
        // Firefox is listed first (by name), Neovim second.
        assert!(
            matches!(a.click(1), Step::Launch { command, terminal: true, .. } if command == "nvim")
        );
        let mut a = launcher();
        assert_eq!(a.click(5), Step::Continue);
        assert!(matches!(a.click(0), Step::Launch { app: Some(app), .. } if app == "firefox"));
    }

    #[test]
    fn the_terminal_heads_the_list_before_anything_is_typed() {
        let mut list = apps();
        list.push(App {
            id: "org.wezfurlong.wezterm".into(),
            name: "WezTerm".into(),
            exec: "wezterm start --cwd .".into(),
            ..Default::default()
        });
        let uses = HashMap::from([("firefox".to_string(), 9)]);
        let mut a = Apps::new(list, uses, false).with_terminal("wezterm start --always-new-process");
        assert_eq!(a.picker.window().1[0].insert, "org.wezfurlong.wezterm");
        assert!(matches!(
            press(&mut a, Keysym::Return, false),
            Step::Launch { app: Some(app), terminal: false, .. } if app == "org.wezfurlong.wezterm"
        ));
        // Typing filters as usual.
        let mut a = Apps::new(apps(), HashMap::new(), false).with_terminal("/usr/bin/wezterm");
        typed(&mut a, "fire");
        assert_eq!(a.picker.window().1[0].insert, "firefox");
    }

}
