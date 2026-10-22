//! Settings from `~/.config/dune/dune.toml` (or `$XDG_CONFIG_HOME/dune/dune.toml`), in
//! TOML; dune-finder's are in `finder.toml` next to it (see dune-finder). Every setting is
//! optional:
//!
//! ```toml
//! terminal = "wezterm start --always-new-process"  # or foot, alacritty, ...
//! modifier = "alt"            # alt, super or ctrl+alt: `mod` in the keys below, and Mod+mouse
//! lock_command = "swaylock -f"  # what the `lock` key runs; default swaylock -f
//! follow = true               # go to the desktop an app is launched on (`firefox :d3`); default false
//! focus_follows_mouse = true  # the tile under the pointer takes focus (when all of it is on screen); default false
//! xwayland = "satellite"      # X11 apps through xwayland-satellite, or "off"
//! font = "JetBrains Mono"     # tab titles; default monospace
//! border_color = "#ffffff"    # border of tiles without focus: #RRGGBB or #RRGGBBAA, or "none"; default white
//! exec = ["waybar", "mako"]   # run when dune starts, in order
//!
//! [keyboard]
//! layout = "us"
//! variant = "dvorak"
//! options = "caps:escape"     # also `model` and `rules`
//! repeat_delay = 500          # ms a held key waits before repeating (100-2000)
//! repeat_rate = 25            # repeats per second while held (1-100)
//!
//! [tabs]
//! min_width = 200             # px a tab shrinks to before its bar scrolls; default 200
//! max_width = 220             # px a tab grows to while there is room; default 220
//!
//! [capture]
//! screenshot_dir = "~/shots"  # where the `screenshot` keys save; default $XDG_PICTURES_DIR/Screenshots, else ~/Pictures/Screenshots
//! recording_dir = "~/videos"  # where `record` saves; default $XDG_VIDEOS_DIR, else ~/Videos
//!
//! [mouse]
//! fold_click = "mod+right"    # fold the tile under the pointer into the accordion (on a folded one: open it); default none
//!
//! # Mice and touchpads (on the real session, not nested)
//! [input]
//! left_handed = true          # swap the left and right buttons
//! natural_scroll = false      # content follows the fingers
//! tap = true                  # tap to click on touchpads
//! accel_speed = 0.0           # pointer acceleration, -1.0 (slow) to 1.0 (fast)
//!
//! # Monitors, by connector name (dune logs them as `monitor HDMI-A-1: ...`)
//! [output.HDMI-A-1]
//! mode = "2560x1080@60"       # WIDTHxHEIGHT[@HZ]; default: the monitor's preferred mode
//! position = [0, 0]           # top-left corner; unset ones go to the right of the rest
//! [output.DP-1]
//! off = true                  # leave this monitor dark
//! ```
//!
//! A setting dune does not know, or a value that is not one (a key that is not a key, a
//! number out of range), is warned about in the log and the default stays. A file that is
//! not TOML, or a value of the wrong type (`follow = "yes"`), is reported with its line;
//! dune then starts with the defaults, or keeps the settings it has when the file is saved.
//!
//! Keys, in `[keys]`: every compositor key has a name, set like `close = ["super+q",
//! "super+'"]` (or one key, `close = "super+q"`). A value replaces that key's defaults;
//! `"none"` (or `[]`) turns it off. `mod` stands for `modifier`, the rest are `super`, `alt`,
//! `ctrl` and `shift`, then a key name as xkb writes it (`Return`, `slash`, `comma`,
//! `Escape`) or one character (`q`, `'`). Keys are the unshifted ones, so Mod+Shift+1 is
//! `mod+shift+1`, and `mod+E` is `mod+e`. A key two names share goes to the one listed
//! first here (a warning tells), so with `modifier = "ctrl+alt"` the Ctrl keys (`move_tab_*`,
//! `row_*`) give way. Ctrl+Alt+F1..F12 switch VTs on a real session, always. The defaults:
//!
//! ```toml
//! [keys]
//! focus_left = ["mod+h", "mod+Left"]  # focus the tile that way
//! focus_down = ["mod+j", "mod+Down"]
//! focus_up = ["mod+k", "mod+Up"]
//! focus_right = ["mod+l", "mod+Right"]
//! swap_left = ["mod+shift+h", "mod+shift+Left"]  # swap the tile with the one that way
//! swap_down = ["mod+shift+j", "mod+shift+Down"]
//! swap_up = ["mod+shift+k", "mod+shift+Up"]
//! swap_right = ["mod+shift+l", "mod+shift+Right"]
//! move_tab_left = ["mod+ctrl+h", "mod+ctrl+Left"]  # join the column that way as a tab, or leave a tabbed one
//! move_tab_down = ["mod+ctrl+j", "mod+ctrl+Down"]
//! move_tab_up = ["mod+ctrl+k", "mod+ctrl+Up"]
//! move_tab_right = ["mod+ctrl+l", "mod+ctrl+Right"]
//! focus_monitor_left = "none"         # focus the monitor that way (monitors stacked: mod+Up / mod+Down,
//! focus_monitor_down = "none"         # with those keys taken out of focus_up / focus_down)
//! focus_monitor_up = "none"
//! focus_monitor_right = "none"
//! desk_1 = "mod+1"                    # show that desktop (desk_1 .. desk_9: mod+1 .. mod+9)
//! send_to_desk_1 = "mod+shift+1"      # send the tile there (send_to_desk_1 .. _9: mod+shift+1 .. 9)
//! empty_container = "mod+Return"      # an empty container, dune-finder to fill it
//! empty_tab = "mod+shift+t"           # one as the next tab of the focused tile's container
//! next_tab = "mod+Tab"
//! prev_tab = ["mod+shift+Tab", "mod+shift+ISO_Left_Tab"]
//! wider = ["mod+equal", "mod+plus"]   # the column's width
//! narrower = "mod+minus"
//! taller = ["mod+shift+equal", "mod+shift+plus"]  # the tile's height
//! shorter = ["mod+shift+minus", "mod+shift+underscore"]
//! row_wider = ["mod+ctrl+equal", "mod+ctrl+plus"]  # the tile's width inside its row
//! row_narrower = "mod+ctrl+minus"
//! cycle_width = "mod+r"
//! full_width = "mod+f"
//! fullscreen = "mod+shift+f"          # the tile over its whole monitor, bar too (again: back)
//! center = "mod+space"                # center the column (again: stop)
//! float = "mod+shift+space"           # float the tile (again: back into the strip)
//! send_to_scratchpad = "mod+shift+grave"  # the tile off every desktop, into the scratchpad
//! scratchpad = "mod+grave"            # the next scratchpad window floats here (on it again: back)
//! close = "mod+q"
//! launcher = "mod+d"                  # dune-finder on the apps
//! tile_options = "mod+slash"          # dune-finder on the focused tile's options (private, float, ...)
//! switcher = "none"                   # dune-finder on the windows (`/` in it gets there too)
//! private_tile = "none"               # block the focused tile on screen and in captures (again: show it)
//! fold = "none"                       # fold the focused tile into the accordion (on a folded one: open it)
//! terminal_vertical = "mod+v"         # a terminal right of the tile, in its folder
//! terminal_horizontal = "mod+s"       # one below it
//! terminal_tab = "none"               # one as a new tab of its container
//! stack_to_tabs = "mod+t"
//! quit = "mod+shift+e"
//! lock = "mod+Escape"                 # runs `lock_command`
//! screenshot = "Print"                # of a region chosen with the mouse (Esc: none)
//! screenshot_tile = "ctrl+Print"      # of the focused tile
//! screenshot_screen = "shift+Print"   # of the active monitor
//! record = "mod+shift+r"              # record a region chosen with the mouse (any record key again: stop)
//! record_tile = "mod+ctrl+r"          # record the focused tile
//! record_screen = "mod+ctrl+shift+r"  # record the active monitor
//! ```
//!
//! The pointer pushing on against the left or right edge of the screens slides the strip
//! that way; it stays there until focus moves. A mark at the middle of an edge (`‹3`, `2›`)
//! tells how many tiles are past it.
//!
//! dune-finder (a program of its own, installed next to dune) is the launcher and the
//! window list: Mod+d shows the apps, `/` typed first in them turns it into every window
//! to go to, Backspace with nothing typed turns it back (`:l` typed first lists the
//! windows of locked desktops instead; choosing one unlocks its desktop). Mod+/ shows the
//! options of the focused tile: private tile, float, center, full width, to tab, send to
//! a desktop, close, each with its key; typing narrows them, Enter does one. A click
//! chooses a row, the wheel moves through them, a click away closes it. Mod+Enter opens an
//! empty container with dune-finder for it: Enter right away puts a terminal (`terminal`)
//! there, a program chosen or typed goes there instead. `#` typed first shows the files of
//! the home folder: Enter opens a folder or a file (`xdg-open`), Mod+Enter the
//! `file_manager` (finder.toml) at it. `>` typed first jumps, like zoxide's `z`: the folders terminals
//! worked in (and the finder opened), the most used and recent first; Enter opens a
//! terminal there, Mod+Enter the `file_manager`.
//!
//! With Mod held, the pointer near an edge of a tile lights a band along that edge;
//! a Mod+click there opens an empty container on that side with a compact dune-finder (no
//! list until something is typed). A Mod+click elsewhere floats the tile.
//!
//! Tabs belong to columns (each tab with its own splits) and have keys (their only settings
//! are `[tabs]` `min_width` and `max_width`):
//! Mod+Tab / Mod+Shift+Tab show the next / previous tab, Mod+Ctrl+direction joins the
//! column that way as a tab (or leaves a tabbed one), Mod+t sends a tile out of its split
//! into a tab of its own (a lone tile: a bar with it as the one tab), a click on a tab
//! shows it, a Mod+drag let go on a tab bar adds the tile there as a tab, a tab Mod+dragged
//! along its own bar moves to the place it is let go, and dune-finder opens an app as a tab with
//! Ctrl+T or `:t`. The tab bar sits at the top left of its column (see
//! `layout::split_tab_bar`; its placement may become a setting).
//!
//! A right click on a desktop in the bar (waybar `ext/workspaces` with `"on-click-right":
//! "deactivate"`) locks it, "🔒 n": it is not shown (Mod+N, a click on it and the window
//! list leave it alone; locking the one shown switches to another) until another right
//! click, or a window chosen with `:l`, unlocks it. Tiles can still be sent there.
//!
//! Screenshots and recordings (see `screenshot`) need grim, slurp, wf-recorder,
//! wl-clipboard and libnotify (`notify-send`). A screenshot is saved as
//! `dune-YYYY-MM-DD_HH-MM-SS.png`, put on the clipboard and announced with its path; a
//! recording (`.mp4`, no sound) is announced when it starts and when it is saved. Bars
//! show a recording running with `dune recording watch` (waybar `custom` module,
//! `return-type: json`; `on-click: dune recording stop`).
//!
//! Night light has no settings here: programs like wlsunset (the waybar night-light
//! module runs it) set the monitors' gamma through wlr-gamma-control (see `gamma`), on
//! the real session only.
//!
//! Saving the file applies it live (see `reload`), except monitor modes, `off`, `xwayland`
//! and `exec` (programs start once, with dune).
//!
//! Environment variables win over the file: `XKB_DEFAULT_LAYOUT`, `XKB_DEFAULT_VARIANT`,
//! `XKB_DEFAULT_OPTIONS`, `XKB_DEFAULT_MODEL`, `XKB_DEFAULT_RULES`, `DUNE_TERMINAL`, `DUNE_MOD`.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use serde::Deserialize;
use smithay::input::keyboard::{Keysym, ModifiersState, XkbConfig, xkb};

use crate::layout::{Direction, TabWidths};


/// A key with its modifiers, written like `super+Right` or `ctrl+alt+h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySpec {
    pub logo: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub key: Keysym,
}

impl KeySpec {
    /// `mod` in `spec` stands for `modifier` (`alt`, `super` or `ctrl+alt`).
    pub fn parse(spec: &str, modifier: &str) -> Option<KeySpec> {
        let mut parts: Vec<&str> = spec.split('+').map(str::trim).collect();
        let name = parts.pop().filter(|n| !n.is_empty())?;
        let mut key = xkb::keysym_from_name(name, xkb::KEYSYM_CASE_INSENSITIVE);
        // A single character stands for its own key, like `'` for apostrophe.
        let mut chars = name.chars();
        if let (Some(c), None, Keysym::NoSymbol) = (chars.next(), chars.next(), key) {
            key = xkb::utf32_to_keysym(c as u32);
        }
        if key == Keysym::NoSymbol {
            return None;
        }
        // Keys come unshifted (Mod+Shift+e is `e` with shift held), so `E` is the `e` key.
        if let Some(c) = key.key_char() {
            let mut lower = c.to_lowercase();
            if let (Some(l), None) = (lower.next(), lower.next())
                && l != c
            {
                key = Keysym::from_char(l);
            }
        }
        let (logo, alt, ctrl, shift) = modifiers(&parts, modifier)?;
        Some(KeySpec { logo, alt, ctrl, shift, key })
    }

    /// Exactly these modifiers (so `super+Right` is not taken by `super+shift+Right`).
    pub fn matches(&self, mods: &ModifiersState, key: Keysym) -> bool {
        key == self.key && mods.logo == self.logo && mods.alt == self.alt && mods.ctrl == self.ctrl && mods.shift == self.shift
    }
}

/// The modifiers of a key or click (`super`, `alt`, `ctrl`, `shift`, `mod` for
/// `modifier`), as (logo, alt, ctrl, shift); `None` if one is not a modifier.
fn modifiers(parts: &[&str], modifier: &str) -> Option<(bool, bool, bool, bool)> {
    let (mut logo, mut alt, mut ctrl, mut shift) = (false, false, false, false);
    for m in parts {
        match m.to_lowercase().as_str() {
            "mod" => match modifier {
                "super" | "logo" => logo = true,
                "ctrl+alt" => (ctrl, alt) = (true, true),
                _ => alt = true,
            },
            "super" | "logo" | "mod4" => logo = true,
            "alt" | "mod1" => alt = true,
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            _ => return None,
        }
    }
    Some((logo, alt, ctrl, shift))
}

/// A mouse button with its modifiers, written like `mod+right` or `ctrl+middle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClickSpec {
    pub logo: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    /// The evdev code of the button (`BTN_LEFT`, ...).
    pub button: u32,
}

impl ClickSpec {
    /// The button is `left`, `right` or `middle`; `mod` stands for `modifier`.
    pub fn parse(spec: &str, modifier: &str) -> Option<ClickSpec> {
        let mut parts: Vec<&str> = spec.split('+').map(str::trim).collect();
        let button = match parts.pop()?.to_lowercase().as_str() {
            "left" => 0x110,
            "right" => 0x111,
            "middle" => 0x112,
            _ => return None,
        };
        let (logo, alt, ctrl, shift) = modifiers(&parts, modifier)?;
        Some(ClickSpec { logo, alt, ctrl, shift, button })
    }

    /// This button with exactly these modifiers.
    pub fn matches(&self, mods: &ModifiersState, button: u32) -> bool {
        button == self.button && mods.logo == self.logo && mods.alt == self.alt && mods.ctrl == self.ctrl && mods.shift == self.shift
    }
}


impl std::fmt::Display for KeySpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (held, name) in [(self.logo, "super+"), (self.ctrl, "ctrl+"), (self.alt, "alt+"), (self.shift, "shift+")] {
            if held {
                f.write_str(name)?;
            }
        }
        f.write_str(&xkb::keysym_get_name(self.key))
    }
}

/// What a compositor key does; each one has a name in the file (see the table up top).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    Focus(Direction),
    Swap(Direction),
    MoveTab(Direction),
    FocusMonitor(Direction),
    Desk(u8),
    SendToDesk(u8),
    EmptyContainer,
    EmptyTab,
    NextTab,
    PrevTab,
    Wider,
    Narrower,
    Taller,
    Shorter,
    RowWider,
    RowNarrower,
    CycleWidth,
    FullWidth,
    Fullscreen,
    Center,
    Float,
    SendToScratchpad,
    Scratchpad,
    Close,
    Launcher,
    TileOptions,
    Switcher,
    PrivateTile,
    Fold,
    Terminal(TerminalHere),
    StackToTabs,
    Quit,
    Lock,
    Screenshot,
    ScreenshotTile,
    ScreenshotScreen,
    Record,
    RecordTile,
    RecordScreen,
}

/// Every binding: its name in the file, what it does and its default keys. A key two of
/// them share goes to the earlier one.
fn binding_table() -> Vec<(String, Binding, String)> {
    use Binding::*;
    let sides = [
        ("left", Direction::Left, "h", "Left"),
        ("down", Direction::Down, "j", "Down"),
        ("up", Direction::Up, "k", "Up"),
        ("right", Direction::Right, "l", "Right"),
    ];
    let mut table = Vec::new();
    for (prefix, mods, binding) in [
        ("focus", "mod", Focus as fn(Direction) -> Binding),
        ("swap", "mod+shift", Swap),
        ("move_tab", "mod+ctrl", MoveTab),
    ] {
        for (side, dir, letter, arrow) in sides {
            table.push((format!("{prefix}_{side}"), binding(dir), format!("{mods}+{letter}, {mods}+{arrow}")));
        }
    }
    table.extend(sides.map(|(side, dir, _, _)| (format!("focus_monitor_{side}"), FocusMonitor(dir), "none".to_string())));
    table.extend((1..=9).map(|n| (format!("desk_{n}"), Desk(n), format!("mod+{n}"))));
    table.extend((1..=9).map(|n| (format!("send_to_desk_{n}"), SendToDesk(n), format!("mod+shift+{n}"))));
    let rest = [
        ("empty_container", EmptyContainer, "mod+Return"),
        ("empty_tab", EmptyTab, "mod+shift+t"),
        ("next_tab", NextTab, "mod+Tab"),
        ("prev_tab", PrevTab, "mod+shift+Tab, mod+shift+ISO_Left_Tab"),
        ("wider", Wider, "mod+equal, mod+plus"),
        ("narrower", Narrower, "mod+minus"),
        ("taller", Taller, "mod+shift+equal, mod+shift+plus"),
        ("shorter", Shorter, "mod+shift+minus, mod+shift+underscore"),
        ("row_wider", RowWider, "mod+ctrl+equal, mod+ctrl+plus"),
        ("row_narrower", RowNarrower, "mod+ctrl+minus"),
        ("cycle_width", CycleWidth, "mod+r"),
        ("full_width", FullWidth, "mod+f"),
        ("fullscreen", Fullscreen, "mod+shift+f"),
        ("center", Center, "mod+space"),
        ("float", Float, "mod+shift+space"),
        ("send_to_scratchpad", SendToScratchpad, "mod+shift+grave"),
        ("scratchpad", Scratchpad, "mod+grave"),
        ("close", Close, "mod+q"),
        ("launcher", Launcher, "mod+d"),
        ("tile_options", TileOptions, "mod+slash"),
        ("switcher", Switcher, "none"),
        ("private_tile", PrivateTile, "none"),
        ("fold", Fold, "none"),
        ("terminal_vertical", Terminal(TerminalHere::Right), "mod+v"),
        ("terminal_horizontal", Terminal(TerminalHere::Below), "mod+s"),
        ("terminal_tab", Terminal(TerminalHere::Tab), "none"),
        ("stack_to_tabs", StackToTabs, "mod+t"),
        ("quit", Quit, "mod+shift+e"),
        ("lock", Lock, "mod+Escape"),
        ("screenshot", Screenshot, "Print"),
        ("screenshot_tile", ScreenshotTile, "ctrl+Print"),
        ("screenshot_screen", ScreenshotScreen, "shift+Print"),
        ("record", Record, "mod+shift+r"),
        ("record_tile", RecordTile, "mod+ctrl+r"),
        ("record_screen", RecordScreen, "mod+ctrl+shift+r"),
    ];
    table.extend(rest.map(|(name, binding, keys)| (name.to_string(), binding, keys.to_string())));
    table
}

/// The keys of a name in `[keys]`: one, or a list.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Keys {
    One(String),
    Many(Vec<String>),
}

/// Keys separated by commas, or `none`; `None` if one of them is not a key.
fn key_list(value: &str, modifier: &str) -> Option<Vec<KeySpec>> {
    keys_of_specs(&value.split(',').collect::<Vec<_>>(), modifier)
}

/// Keys written one by one; `["none"]` or none at all: no key. `None` if one is not a key.
fn keys_of_specs(specs: &[&str], modifier: &str) -> Option<Vec<KeySpec>> {
    if specs.is_empty() || matches!(specs, [one] if one.trim().eq_ignore_ascii_case("none")) {
        return Some(Vec::new());
    }
    specs.iter().map(|spec| KeySpec::parse(spec, modifier)).collect()
}

/// The keys of every binding, in table order: the file's in place of the defaults.
fn bindings(file: &BTreeMap<String, Keys>, modifier: &str) -> Vec<(Binding, KeySpec)> {
    let table = binding_table();
    for name in file.keys().filter(|name| !table.iter().any(|(n, ..)| n == *name)) {
        tracing::warn!("ignoring `keys.{name}`: dune has no key by that name");
    }
    // With the name and whether the file set it, for the warnings.
    let mut taken: Vec<(String, Binding, KeySpec, bool)> = Vec::new();
    for (name, binding, defaults) in table {
        let set = file.get(&name).and_then(|value| {
            let specs: Vec<&str> = match value {
                Keys::One(key) => vec![key],
                Keys::Many(keys) => keys.iter().map(String::as_str).collect(),
            };
            let keys = keys_of_specs(&specs, modifier);
            if keys.is_none() {
                tracing::warn!("ignoring `keys.{name} = {specs:?}`: not keys like mod+q, super+shift+t or none");
            }
            keys
        });
        let from_file = set.is_some();
        let keys = set.unwrap_or_else(|| key_list(&defaults, modifier).unwrap_or_default());
        for key in keys {
            if let Some((other, other_binding, _, other_from_file)) = taken.iter().find(|(_, _, k, _)| *k == key) {
                // Defaults collapsing under `modifier = ctrl+alt` are expected, not news.
                if *other_binding != binding && (from_file || *other_from_file) {
                    tracing::warn!("`keys.{name}`: {key} already does `{other}`, it stays there");
                }
                continue;
            }
            taken.push((name.clone(), binding, key, from_file));
        }
    }
    taken.into_iter().map(|(_, binding, key, _)| (binding, key)).collect()
}

/// `mouse.fold_click`: a click, or none (the default); a value that is not one is warned about.
fn fold_click(value: Option<&str>, modifier: &str) -> Option<ClickSpec> {
    let value = value.filter(|v| !v.eq_ignore_ascii_case("none"))?;
    let click = ClickSpec::parse(value, modifier);
    if click.is_none() {
        tracing::warn!("ignoring `mouse.fold_click = {value}`: not modifiers and left, right or middle, nor none");
    }
    click
}

/// Where the terminal keys (`terminal_vertical`, `_horizontal`, `_tab`) open a terminal,
/// next to the focused tile and in the folder its shell is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalHere {
    Right,
    Below,
    Tab,
}

/// A monitor mode written like `2560x1080@60` (the refresh rate is optional).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputMode {
    pub width: u16,
    pub height: u16,
    /// Hz, rounded.
    pub refresh: Option<u32>,
}

impl OutputMode {
    pub fn parse(text: &str) -> Option<OutputMode> {
        let (size, refresh) = match text.split_once('@') {
            Some((size, hz)) => (size, Some(hz.trim().parse::<f64>().ok()?.round() as u32)),
            None => (text, None),
        };
        let (w, h) = size.split_once('x')?;
        Some(OutputMode { width: w.trim().parse().ok()?, height: h.trim().parse().ok()?, refresh })
    }
}

/// Mice and touchpads (`[input]`); unset ones keep libinput's defaults.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    pub left_handed: Option<bool>,
    pub natural_scroll: Option<bool>,
    pub tap: Option<bool>,
    /// -1.0 to 1.0.
    pub accel_speed: Option<f64>,
}

/// Settings of one monitor (`[output.NAME]`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputConfig {
    pub mode: Option<OutputMode>,
    pub position: Option<(i32, i32)>,
    pub off: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub keyboard_rules: String,
    pub keyboard_model: String,
    pub keyboard_layout: String,
    pub keyboard_variant: String,
    pub keyboard_options: String,
    /// Milliseconds a held key waits before it starts repeating.
    pub repeat_delay: i32,
    /// Repeats per second while a key stays held.
    pub repeat_rate: i32,
    pub terminal: String,
    pub modifier: String,
    /// Every compositor key and what it does, `mod` already read as `modifier`; a key is
    /// in it once.
    pub bindings: Vec<(Binding, KeySpec)>,
    /// What the `lock` key runs, through the shell.
    pub lock_command: String,
    /// Show the desktop the launcher opens an app on, instead of staying here.
    pub follow: bool,
    /// The tile under the pointer takes focus as the pointer moves.
    pub focus_follows_mouse: bool,
    /// The click that folds the tile under the pointer into the accordion (opens a folded one).
    pub fold_click: Option<ClickSpec>,
    /// `satellite` (default) or `off`.
    pub xwayland: String,
    /// Monitors by connector name, like `HDMI-A-1`.
    pub outputs: BTreeMap<String, OutputConfig>,
    pub input: InputConfig,
    /// As fontconfig names it; tab titles are written with it.
    pub font_family: String,
    /// How narrow and how wide tabs get in their bars.
    pub tab_widths: TabWidths,
    /// The border of tiles without focus (premultiplied RGBA); `None`: not drawn.
    pub border_color: Option<[f32; 4]>,
    /// Shell commands run when dune starts (`exec`, in order).
    pub exec: Vec<String>,
    /// Where screenshots go (`~/` for the home folder); empty: `$XDG_PICTURES_DIR/Screenshots`.
    pub screenshot_dir: String,
    /// Where recordings go; empty: `$XDG_VIDEOS_DIR`.
    pub recording_dir: String,
}

impl Config {
    pub fn xkb(&self) -> XkbConfig<'_> {
        XkbConfig {
            rules: &self.keyboard_rules,
            model: &self.keyboard_model,
            layout: &self.keyboard_layout,
            variant: &self.keyboard_variant,
            options: Some(self.keyboard_options.clone()).filter(|o| !o.is_empty()),
        }
    }

    /// The keys of `binding`, as the file writes them (`super+f, super+F11`); empty: none.
    pub fn key_label(&self, binding: Binding) -> String {
        let keys: Vec<String> = self.bindings.iter().filter(|(b, _)| *b == binding).map(|(_, k)| k.to_string()).collect();
        keys.join(", ")
    }

    /// Settings that differ from `old` but only take effect when dune starts again.
    pub fn restart_needed(&self, old: &Config) -> Vec<String> {
        let mut changed = Vec::new();
        if self.xwayland != old.xwayland {
            changed.push("xwayland".to_string());
        }
        if self.exec != old.exec {
            changed.push("exec".to_string());
        }
        let names = self.outputs.keys().chain(old.outputs.keys()).collect::<BTreeSet<_>>();
        for name in names {
            let new = self.outputs.get(name).cloned().unwrap_or_default();
            let old = old.outputs.get(name).cloned().unwrap_or_default();
            if new.mode != old.mode {
                changed.push(format!("output.{name}.mode"));
            }
            if new.off != old.off {
                changed.push(format!("output.{name}.off"));
            }
        }
        changed
    }
}

/// `dune.toml` as written: what is missing is `None` (or empty), for `resolve` to fill in.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct File {
    terminal: Option<String>,
    modifier: Option<String>,
    lock_command: Option<String>,
    follow: bool,
    focus_follows_mouse: bool,
    xwayland: Option<String>,
    font: Option<String>,
    border_color: Option<String>,
    exec: Vec<String>,
    keyboard: KeyboardFile,
    tabs: TabsFile,
    capture: CaptureFile,
    mouse: MouseFile,
    input: InputConfig,
    output: BTreeMap<String, OutputFile>,
    keys: BTreeMap<String, Keys>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct KeyboardFile {
    rules: Option<String>,
    model: Option<String>,
    layout: Option<String>,
    variant: Option<String>,
    options: Option<String>,
    repeat_delay: Option<i64>,
    repeat_rate: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TabsFile {
    min_width: Option<i64>,
    max_width: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CaptureFile {
    screenshot_dir: String,
    recording_dir: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MouseFile {
    fold_click: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OutputFile {
    mode: Option<String>,
    position: Option<[i32; 2]>,
    off: bool,
}

/// The config a file with `text` gives, without the environment.
#[cfg(test)]
pub fn from_text(text: &str) -> Config {
    resolve(&parse(text).unwrap(), &|_| None)
}

/// `~/.config/dune/dune.toml`.
pub fn path() -> Option<PathBuf> {
    Some(dune_common::config_dir()?.join("dune.toml"))
}

/// Every setting at its default (the environment still counts): no file, or one that is
/// not TOML when dune starts.
pub fn defaults() -> Config {
    resolve(&File::default(), &|key| std::env::var(key).ok())
}

/// The settings of `dune.toml`; `None` (reported) when it is not TOML or a value has the
/// wrong type. No file: the defaults.
pub fn load() -> Option<Config> {
    let path = path()?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    match parse(&text) {
        Ok(file) => Some(resolve(&file, &|key| std::env::var(key).ok())),
        Err(err) => {
            tracing::error!("{}: {err}", path.display());
            None
        }
    }
}

/// The file as TOML; a setting dune does not know is warned about.
fn parse(text: &str) -> Result<File, toml::de::Error> {
    let deserializer = toml::Deserializer::parse(text)?;
    serde_ignored::deserialize(deserializer, |setting| {
        tracing::warn!("ignoring `{setting}`: dune has no setting by that name");
    })
}

fn resolve(file: &File, env: &dyn Fn(&str) -> Option<String>) -> Config {
    let get = |env_key: &str, value: &Option<String>, default: &str| {
        env(env_key)
            .filter(|v| !v.is_empty())
            .or_else(|| value.clone())
            .unwrap_or_else(|| default.to_string())
    };
    let keyboard = &file.keyboard;
    // Nested inside another compositor Super is usually taken by the host.
    let modifier = get("DUNE_MOD", &file.modifier, "alt");
    Config {
        keyboard_rules: get("XKB_DEFAULT_RULES", &keyboard.rules, ""),
        keyboard_model: get("XKB_DEFAULT_MODEL", &keyboard.model, ""),
        keyboard_layout: get("XKB_DEFAULT_LAYOUT", &keyboard.layout, ""),
        keyboard_variant: get("XKB_DEFAULT_VARIANT", &keyboard.variant, ""),
        keyboard_options: get("XKB_DEFAULT_OPTIONS", &keyboard.options, ""),
        repeat_delay: number("keyboard.repeat_delay", keyboard.repeat_delay, 500, 100..=2000),
        repeat_rate: number("keyboard.repeat_rate", keyboard.repeat_rate, 25, 1..=100),
        // A new process per window: wezterm's single instance would open every window
        // from the first one, whose `DUNE_TILE` and pid no longer find the new tile.
        terminal: get("DUNE_TERMINAL", &file.terminal, "wezterm start --always-new-process"),
        modifier: modifier.clone(),
        xwayland: get("DUNE_XWAYLAND", &file.xwayland, "satellite"),
        bindings: bindings(&file.keys, &modifier),
        lock_command: file.lock_command.clone().filter(|c| !c.is_empty()).unwrap_or_else(|| "swaylock -f".into()),
        follow: file.follow,
        focus_follows_mouse: file.focus_follows_mouse,
        fold_click: fold_click(file.mouse.fold_click.as_deref(), &modifier),
        outputs: outputs(&file.output),
        input: input(&file.input),
        font_family: file.font.clone().filter(|f| !f.is_empty()).unwrap_or_else(|| "monospace".into()),
        tab_widths: tab_widths(&file.tabs),
        border_color: border_color(file.border_color.as_deref()),
        exec: file.exec.iter().filter(|c| !c.trim().is_empty()).cloned().collect(),
        screenshot_dir: file.capture.screenshot_dir.clone(),
        recording_dir: file.capture.recording_dir.clone(),
    }
}

/// `border_color`: `#RRGGBB`, `#RRGGBBAA` (the `#` optional) or `none`; white when missing
/// or not one.
fn border_color(value: Option<&str>) -> Option<[f32; 4]> {
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let Some(value) = value else { return Some(WHITE) };
    if value.eq_ignore_ascii_case("none") {
        return None;
    }
    let hex = value.trim_start_matches('#').trim_start_matches("0x");
    let channel = |i: usize| hex.get(i..i + 2).and_then(|c| u8::from_str_radix(c, 16).ok()).map(|c| c as f32 / 255.0);
    let rgba = match hex.len() {
        6 => (0..3).map(|i| channel(i * 2)).chain([Some(1.0)]).collect::<Option<Vec<_>>>(),
        8 => (0..4).map(|i| channel(i * 2)).collect(),
        _ => None,
    };
    let Some([r, g, b, a]) = rgba.and_then(|v| <[f32; 4]>::try_from(v).ok()) else {
        tracing::warn!("ignoring `border_color = {value}`: not #RRGGBB, #RRGGBBAA or none");
        return Some(WHITE);
    };
    // Solid colour buffers take premultiplied alpha.
    Some([r * a, g * a, b * a, a])
}

/// `tabs.min_width` and `tabs.max_width`; a maximum under the minimum is raised to it.
fn tab_widths(tabs: &TabsFile) -> TabWidths {
    let default = TabWidths::default();
    let min = number("tabs.min_width", tabs.min_width, default.min, 40..=2000);
    let max = number("tabs.max_width", tabs.max_width, default.max, 40..=2000);
    if max < min {
        tracing::warn!("`tabs.max_width = {max}` is under `tabs.min_width = {min}`, using {min} for both");
    }
    TabWidths { min, max: max.max(min) }
}

fn number(key: &str, value: Option<i64>, default: i32, range: std::ops::RangeInclusive<i32>) -> i32 {
    let Some(value) = value else { return default };
    i32::try_from(value).ok().filter(|v| range.contains(v)).unwrap_or_else(|| {
        tracing::warn!("ignoring `{key} = {value}`: not a number from {} to {}", range.start(), range.end());
        default
    })
}

fn input(input: &InputConfig) -> InputConfig {
    let mut input = input.clone();
    if let Some(speed) = input.accel_speed.filter(|v| !(-1.0..=1.0).contains(v)) {
        tracing::warn!("ignoring `input.accel_speed = {speed}`: not from -1.0 to 1.0");
        input.accel_speed = None;
    }
    input
}

fn outputs(file: &BTreeMap<String, OutputFile>) -> BTreeMap<String, OutputConfig> {
    file.iter()
        .map(|(name, output)| {
            let mode = output.mode.as_deref().and_then(|text| {
                let mode = OutputMode::parse(text);
                if mode.is_none() {
                    tracing::warn!("ignoring `output.{name}.mode = {text}`: not WIDTHxHEIGHT[@HZ]");
                }
                mode
            });
            let config = OutputConfig { mode, position: output.position.map(|[x, y]| (x, y)), off: output.off };
            (name.clone(), config)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> Config {
        from_text(text)
    }

    #[test]
    fn file_values_and_defaults() {
        let c = config("# my setup\n[keyboard]\nlayout = \"us\"\nvariant = \"dvorak\"  # comment\n");
        assert_eq!(c.keyboard_layout, "us");
        assert_eq!(c.keyboard_variant, "dvorak");
        assert_eq!(c.keyboard_options, "");
        assert_eq!(c.terminal, "wezterm start --always-new-process");
        assert_eq!(c.modifier, "alt");
    }

    #[test]
    fn not_toml_or_a_wrong_type_is_an_error_with_its_line() {
        let err = parse("terminal = foot\n").unwrap_err();
        assert!(err.to_string().contains("line 1"), "{err}");
        let err = parse("modifier = \"super\"\nfollow = \"yes\"\n").unwrap_err();
        assert!(err.to_string().contains("line 2"), "{err}");
        // Unknown settings are not errors (they are warned about).
        assert!(parse("nonsense = 1\n[keyboard]\nlayot = \"us\"\n").is_ok());
    }

    #[test]
    fn key_specs() {
        let spec = KeySpec::parse("super+Right", "alt").unwrap();
        assert!(spec.logo && !spec.alt && spec.key == Keysym::Right);
        assert_eq!(KeySpec::parse("Ctrl + Alt + h", "alt").unwrap().key, Keysym::h);
        assert!(KeySpec::parse("hyper+Right", "alt").is_none());
        assert!(KeySpec::parse("super+NotAKey", "alt").is_none());
        assert_eq!(KeySpec::parse("super+'", "alt").unwrap().key, Keysym::apostrophe);
        let mods = |logo, shift| ModifiersState { logo, shift, ..Default::default() };
        assert!(spec.matches(&mods(true, false), Keysym::Right));
        assert!(!spec.matches(&mods(true, true), Keysym::Right));
        assert!(!spec.matches(&mods(false, false), Keysym::Right));
    }

    /// The keys `config` gives `binding`.
    fn keys_of(config: &Config, binding: Binding) -> Vec<KeySpec> {
        config.bindings.iter().filter(|(b, _)| *b == binding).map(|(_, k)| *k).collect()
    }

    fn spec(text: &str) -> KeySpec {
        KeySpec::parse(text, "alt").unwrap()
    }

    #[test]
    fn default_keys_follow_the_modifier() {
        for (modifier, held) in [("alt", "alt"), ("super", "super"), ("logo", "super"), ("ctrl+alt", "ctrl+alt")] {
            let c = config(&format!("modifier = \"{modifier}\"\n"));
            assert_eq!(keys_of(&c, Binding::Close), [spec(&format!("{held}+q"))], "{modifier}");
            assert_eq!(keys_of(&c, Binding::Focus(Direction::Left)), [spec(&format!("{held}+h")), spec(&format!("{held}+Left"))]);
            assert_eq!(keys_of(&c, Binding::SendToDesk(9)), [spec(&format!("{held}+shift+9"))]);
            assert_eq!(keys_of(&c, Binding::TileOptions), [spec(&format!("{held}+slash"))]);
            assert_eq!(keys_of(&c, Binding::Fullscreen), [spec(&format!("{held}+shift+f"))]);
        }
        // `DUNE_MOD` wins over the file, for the keys too.
        let env = |k: &str| (k == "DUNE_MOD").then(|| "super".to_string());
        let c = resolve(&parse("modifier = \"alt\"\n").unwrap(), &env);
        assert_eq!(keys_of(&c, Binding::Close), [spec("super+q")]);
        // Every binding has a key by default, but the windows (`/` in dune-finder), the
        // private switches (in the tile options), folding (`fold_click`), a terminal tab (Mod+Shift+t opens
        // an empty one) and the monitors (their keys depend on how they are placed).
        let c = config("");
        let unbound = [Binding::Switcher, Binding::PrivateTile, Binding::Fold, Binding::Terminal(TerminalHere::Tab)];
        for (name, binding, defaults) in binding_table() {
            let none = unbound.contains(&binding) || matches!(binding, Binding::FocusMonitor(_));
            assert_eq!(keys_of(&c, binding).is_empty(), none, "{name}");
            assert!(key_list(&defaults, "alt").is_some(), "{name} = {defaults}");
        }
    }

    #[test]
    fn fold_click_is_a_button_with_modifiers() {
        let c = config("modifier = \"super\"\n[mouse]\nfold_click = \"mod+right\"\n");
        let click = c.fold_click.unwrap();
        assert_eq!((click.logo, click.alt, click.ctrl, click.shift, click.button), (true, false, false, false, 0x111));
        assert_eq!(ClickSpec::parse("ctrl+shift+Middle", "alt").map(|c| (c.ctrl, c.shift, c.button)), Some((true, true, 0x112)));
        // None by default; `none`, a key or an unknown modifier: none.
        for value in ["none", "mod+q", "hyper+left"] {
            assert_eq!(config(&format!("[mouse]\nfold_click = \"{value}\"\n")).fold_click, None, "{value}");
        }
        assert_eq!(config("").fold_click, None);
    }

    #[test]
    fn monitor_focus_takes_keys_given_up_by_tile_focus() {
        let c = config("[keys]\nfocus_up = \"mod+k\"\nfocus_monitor_up = \"mod+Up\"\n");
        assert_eq!(keys_of(&c, Binding::Focus(Direction::Up)), [spec("alt+k")]);
        assert_eq!(keys_of(&c, Binding::FocusMonitor(Direction::Up)), [spec("alt+Up")]);
    }

    #[test]
    fn a_value_replaces_the_defaults() {
        let c = config("modifier = \"super\"\n[keys]\nclose = [\"super+q\", \"super+'\"]\nlauncher = \"mod+e\"\n");
        assert_eq!(keys_of(&c, Binding::Close), [spec("super+q"), spec("super+apostrophe")]);
        assert_eq!(keys_of(&c, Binding::Launcher), [spec("super+e")]);
        // The others keep theirs.
        assert_eq!(keys_of(&c, Binding::TileOptions), [spec("super+slash")]);
        let c = config("[keys]\nterminal_vertical = \"super+ctrl+v\"\n");
        assert_eq!(keys_of(&c, Binding::Terminal(TerminalHere::Right)), [spec("super+ctrl+v")]);
        assert_eq!(keys_of(&c, Binding::Terminal(TerminalHere::Below)), [spec("alt+s")]);
    }

    #[test]
    fn none_turns_a_key_off() {
        let c = config("[keys]\nquit = \"none\"\nlock = \"None\"\nclose = []\n");
        assert!(keys_of(&c, Binding::Quit).is_empty());
        assert!(keys_of(&c, Binding::Lock).is_empty());
        assert!(keys_of(&c, Binding::Close).is_empty());
        assert!(!c.bindings.iter().any(|(_, k)| *k == spec("alt+shift+e")));
    }

    #[test]
    fn keys_are_unshifted_and_lowercase() {
        let c = config("[keys]\nlock = \"mod+E\"\nquit = \"super+shift+Q\"\n");
        assert_eq!(keys_of(&c, Binding::Lock), [spec("alt+e")]);
        assert_eq!(keys_of(&c, Binding::Quit), [spec("super+shift+q")]);
        assert_eq!(spec("alt+e").key, Keysym::e);
    }

    #[test]
    fn a_bad_value_keeps_the_defaults() {
        for value in ["\"hyper+q\"", "[\"mod+q\", \"mod+NotAKey\"]", "\"\"", "\"mod+\""] {
            let c = config(&format!("[keys]\nclose = {value}\n"));
            assert_eq!(keys_of(&c, Binding::Close), [spec("alt+q")], "{value:?}");
        }
    }

    #[test]
    fn ctrl_keys_give_way_under_ctrl_alt() {
        let c = config("modifier = \"ctrl+alt\"\n");
        let ctrl_alt_left = spec("ctrl+alt+Left");
        assert_eq!(c.bindings.iter().find(|(_, k)| *k == ctrl_alt_left).map(|(b, _)| *b), Some(Binding::Focus(Direction::Left)));
        assert!(keys_of(&c, Binding::MoveTab(Direction::Left)).is_empty());
        assert!(keys_of(&c, Binding::RowWider).is_empty());
        assert_eq!(keys_of(&c, Binding::Wider), [spec("ctrl+alt+equal"), spec("ctrl+alt+plus")]);
    }

    #[test]
    fn scratchpad_keys() {
        let c = config("");
        assert_eq!(keys_of(&c, Binding::Scratchpad), [spec("alt+grave")]);
        assert_eq!(keys_of(&c, Binding::SendToScratchpad), [spec("alt+shift+grave")]);
        let c = config("[keys]\nscratchpad = \"none\"\n");
        assert!(keys_of(&c, Binding::Scratchpad).is_empty());
    }

    #[test]
    fn tile_options_and_private_keys() {
        let c = config("");
        assert_eq!(keys_of(&c, Binding::TileOptions), [spec("alt+slash")]);
        assert!(keys_of(&c, Binding::Switcher).is_empty());
        assert!(keys_of(&c, Binding::PrivateTile).is_empty());
        // Still settable; a `switcher` set to Mod+/ gives way to `tile_options`, listed first.
        let c = config("[keys]\nswitcher = \"mod+w\"\nprivate_tile = \"mod+p\"\n");
        assert_eq!(keys_of(&c, Binding::Switcher), [spec("alt+w")]);
        assert_eq!(keys_of(&c, Binding::PrivateTile), [spec("alt+p")]);
        // `private_desktop` is gone (desktops lock from the bar): an unknown key, ignored.
        assert!(!binding_table().iter().any(|(name, ..)| name == "private_desktop"));
        let c = config("[keys]\nprivate_desktop = \"mod+shift+p\"\n");
        assert!(!c.bindings.iter().any(|(_, k)| *k == spec("alt+shift+p")));
        let c = config("[keys]\nswitcher = \"mod+slash\"\n");
        assert_eq!(keys_of(&c, Binding::TileOptions), [spec("alt+slash")]);
        assert!(keys_of(&c, Binding::Switcher).is_empty());
    }

    #[test]
    fn key_labels() {
        let c = config("modifier = \"super\"\n[keys]\nclose = [\"mod+q\", \"super+'\"]\n");
        assert_eq!(c.key_label(Binding::Close), "super+q, super+apostrophe");
        assert_eq!(c.key_label(Binding::PrivateTile), "");
    }

    #[test]
    fn lock_key_and_command() {
        let c = config("");
        assert_eq!(keys_of(&c, Binding::Lock), [spec("alt+Escape")]);
        assert_eq!(keys_of(&c, Binding::Lock)[0].key, Keysym::Escape);
        assert_eq!(c.lock_command, "swaylock -f");
        let c = config("lock_command = \"swaylock -f -c 000000\"\n");
        assert_eq!(c.lock_command, "swaylock -f -c 000000");
    }

    #[test]
    fn capture_keys_and_folders() {
        let c = config("");
        assert_eq!(keys_of(&c, Binding::Screenshot), [spec("Print")]);
        assert_eq!(keys_of(&c, Binding::ScreenshotScreen), [spec("shift+Print")]);
        assert_eq!(keys_of(&c, Binding::Record), [spec("alt+shift+r")]);
        assert_eq!(keys_of(&c, Binding::ScreenshotTile), [spec("ctrl+Print")]);
        assert_eq!(keys_of(&c, Binding::RecordTile), [spec("alt+ctrl+r")]);
        assert_eq!(keys_of(&c, Binding::RecordScreen), [spec("alt+ctrl+shift+r")]);
        assert_eq!(keys_of(&c, Binding::CycleWidth), [spec("alt+r")]);
        assert_eq!((c.screenshot_dir.as_str(), c.recording_dir.as_str()), ("", ""));
        let c = config("modifier = \"super\"\n[keys]\nscreenshot = \"super+p\"\nrecord = \"none\"\n");
        assert_eq!(keys_of(&c, Binding::Screenshot), [spec("super+p")]);
        assert!(keys_of(&c, Binding::Record).is_empty());
        assert_eq!(keys_of(&c, Binding::ScreenshotScreen), [spec("shift+Print")]);
        let c = config("[capture]\nscreenshot_dir = \"~/shots\"\nrecording_dir = \"/data/videos\"\n");
        assert_eq!(c.screenshot_dir, "~/shots");
        assert_eq!(c.recording_dir, "/data/videos");
    }

    #[test]
    fn a_shared_key_stays_with_the_first() {
        // `close` comes before `lock`: alt+q stays a close key, lock is left with none.
        let c = config("[keys]\nlock = \"mod+q\"\n");
        assert_eq!(keys_of(&c, Binding::Close), [spec("alt+q")]);
        assert!(keys_of(&c, Binding::Lock).is_empty());
        // Set on the earlier one, it takes the key from the later one's defaults.
        let c = config("[keys]\nfocus_left = \"mod+t\"\n");
        assert_eq!(keys_of(&c, Binding::Focus(Direction::Left)), [spec("alt+t")]);
        assert!(keys_of(&c, Binding::StackToTabs).is_empty());
        // Each key once.
        let keys: Vec<_> = c.bindings.iter().map(|(_, k)| *k).collect();
        assert!(keys.iter().enumerate().all(|(i, k)| !keys[..i].contains(k)));
    }

    #[test]
    fn exec_is_a_list() {
        let c = config("exec = [\"waybar -c ~/.config/waybar/dune.jsonc\", \"mako\", \"\"]\n");
        assert_eq!(c.exec, ["waybar -c ~/.config/waybar/dune.jsonc", "mako"]);
        assert!(config("").exec.is_empty());
    }

    #[test]
    fn key_repeat() {
        let c = config("");
        assert_eq!((c.repeat_delay, c.repeat_rate), (500, 25));
        let c = config("[keyboard]\nrepeat_delay = 600\nrepeat_rate = 30\n");
        assert_eq!((c.repeat_delay, c.repeat_rate), (600, 30));
        let c = config("[keyboard]\nrepeat_delay = 5\nrepeat_rate = 99999999999\n");
        assert_eq!((c.repeat_delay, c.repeat_rate), (500, 25));
    }

    #[test]
    fn follow() {
        assert!(!config("").follow);
        assert!(config("follow = true\n").follow);
        assert!(!config("").focus_follows_mouse);
        assert!(config("focus_follows_mouse = true\n").focus_follows_mouse);
    }

    #[test]
    fn font_and_tab_widths() {
        let c = config("");
        assert_eq!((c.font_family.as_str(), c.tab_widths), ("monospace", TabWidths::default()));
        let c = config("font = \"JetBrains Mono\"\n[tabs]\nmin_width = 120\nmax_width = 300\n");
        assert_eq!((c.font_family.as_str(), c.tab_widths), ("JetBrains Mono", TabWidths { min: 120, max: 300 }));
        // Out of range: the default; a maximum under the minimum: the minimum.
        let c = config("[tabs]\nmin_width = 5\nmax_width = 100\n");
        assert_eq!(c.tab_widths, TabWidths { min: TabWidths::default().min, max: TabWidths::default().min });
    }

    #[test]
    fn border_color() {
        assert_eq!(config("").border_color, Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(config("border_color = \"none\"\n").border_color, None);
        assert_eq!(config("border_color = \"#ff0000\"\n").border_color, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(config("border_color = \"ff0000\"\n").border_color, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(config("border_color = \"#ffffff00\"\n").border_color, Some([0.0, 0.0, 0.0, 0.0]));
        assert_eq!(config("border_color = \"blue\"\n").border_color, Some([1.0, 1.0, 1.0, 1.0]));
    }

    #[test]
    fn outputs() {
        let c = config(
            "[output.HDMI-A-1]\nmode = \"2560x1080@59.94\"\nposition = [0, 0]\n\
             [output.HDMI-A-2]\nmode = \"1920x1080\"\nposition = [311, 1080]\n\
             [output.DP-1]\noff = true\n[output.DP-2]\nmode = \"big\"\n",
        );
        let a = &c.outputs["HDMI-A-1"];
        assert_eq!(a.mode, Some(OutputMode { width: 2560, height: 1080, refresh: Some(60) }));
        assert_eq!(a.position, Some((0, 0)));
        let b = &c.outputs["HDMI-A-2"];
        assert_eq!(b.mode, Some(OutputMode { width: 1920, height: 1080, refresh: None }));
        assert_eq!(b.position, Some((311, 1080)));
        assert!(c.outputs["DP-1"].off && !a.off);
        assert_eq!(c.outputs["DP-2"].mode, None); // invalid: the monitor's preferred mode
    }

    #[test]
    fn input_settings() {
        let c = config("[input]\nleft_handed = true\nnatural_scroll = false\naccel_speed = 2.0\n");
        assert_eq!(c.input.left_handed, Some(true));
        assert_eq!(c.input.natural_scroll, Some(false));
        assert_eq!(c.input.accel_speed, None); // out of range: libinput's default
        assert_eq!(c.input.tap, None);
        assert_eq!(config("").input, InputConfig::default());
    }

    #[test]
    fn restart_needed() {
        let old = config("xwayland = \"off\"\n[output.A]\nmode = \"1920x1080\"\nposition = [0, 0]\n");
        let new = config(
            "xwayland = \"off\"\nmodifier = \"super\"\n[output.A]\nmode = \"2560x1080\"\nposition = [10, 0]\n[output.B]\noff = true\n",
        );
        assert_eq!(new.restart_needed(&old), ["output.A.mode", "output.B.off"]);
        assert!(old.restart_needed(&old).is_empty());
    }

    #[test]
    fn environment_wins() {
        let file = parse("terminal = \"alacritty\"\n[keyboard]\nvariant = \"dvorak\"\n").unwrap();
        let env = |k: &str| match k {
            "XKB_DEFAULT_VARIANT" => Some("colemak".to_string()),
            "DUNE_TERMINAL" => Some(String::new()), // empty means unset
            _ => None,
        };
        let c = resolve(&file, &env);
        assert_eq!(c.keyboard_variant, "colemak");
        assert_eq!(c.terminal, "alacritty");
    }
}
