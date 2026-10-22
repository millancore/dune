//! Settings from `~/.config/dune/config` (or `$XDG_CONFIG_HOME/dune/config`).
//!
//! One `key = value` per line, `#` starts a comment:
//!
//! ```text
//! keyboard_layout = us
//! keyboard_variant = dvorak
//! repeat_delay = 500      # ms a held key waits before repeating
//! repeat_rate = 25        # repeats per second while held
//! terminal = wezterm start --always-new-process  # or foot, alacritty, ...
//! modifier = alt          # alt, super or ctrl+alt: `mod` in the keys below, and Mod+mouse
//! lock_command = swaylock -f   # what the `lock` key runs; default swaylock -f
//! follow = true          # go to the desktop an app is launched on (`firefox :d3`); default false
//! focus_follows_mouse = true  # the tile under the pointer takes focus (when all of it is on screen); default false
//! xwayland = satellite    # X11 apps through xwayland-satellite, or `off`
//! font_family = JetBrains Mono   # tab titles and dune-finder
//! tab_min_width = 200     # px a tab shrinks to before its bar scrolls; default 200
//! tab_max_width = 220     # px a tab grows to while there is room; default 220
//! icon_theme = Papirus-Dark      # dune-finder icons; default: the desktop's (gsettings / GTK)
//! file_manager = yazi     # terminal file manager dune-finder's `#` opens with Mod+Enter; default yazi
//! file_manager_terminal = foot  # terminal it runs in (foot shows yazi's image previews); default `terminal`
//! border_color = none     # border of tiles without focus: RRGGBB or RRGGBBAA (no `#`), or none; default ffffff
//! fold_click = mod+right  # fold the tile under the pointer into the accordion (on a folded one: open it); default none
//! exec = waybar           # run when dune starts; one line per program, as many as needed
//! screenshot_dir = ~/shots       # where the `screenshot` keys save; default $XDG_PICTURES_DIR/Screenshots, else ~/Pictures/Screenshots
//! recording_dir = ~/videos       # where `record` saves; default $XDG_VIDEOS_DIR, else ~/Videos
//!
//! # Mice and touchpads (on the real session, not nested)
//! input.left_handed = true      # swap the left and right buttons
//! input.natural_scroll = false  # content follows the fingers
//! input.tap = true              # tap to click on touchpads
//! input.accel_speed = 0.0       # pointer acceleration, -1.0 (slow) to 1.0 (fast)
//!
//! # Monitors, by connector name (dune logs them as `monitor HDMI-A-1: ...`)
//! output.HDMI-A-1.mode = 2560x1080@60   # WIDTHxHEIGHT[@HZ]; default: the monitor's preferred mode
//! output.HDMI-A-1.position = 0,0        # top-left corner; unset ones go to the right of the rest
//! output.DP-1.off = true                # leave this monitor dark
//! ```
//!
//! Keys: every compositor key has a name, set like `close = super+q, super+'`. A value is
//! one or more keys separated by commas and replaces that key's defaults; `none` turns it
//! off; one that is not a key (a typo) is warned about and the defaults stay. `mod` stands
//! for `modifier`, the rest are `super`, `alt`, `ctrl` and `shift`, then a key name as
//! xkb writes it (`Return`, `slash`, `comma`, `Escape`) or one character (`q`, `'`). Keys
//! are the unshifted ones, so Mod+Shift+1 is `mod+shift+1`, and `mod+E` is `mod+e`. A key
//! two names share goes to the one listed first here (a warning tells), so with
//! `modifier = ctrl+alt` the Ctrl keys (`move_tab_*`, `row_*`) give way. Ctrl+Alt+F1..F12
//! switch VTs on a real session, always. The defaults:
//!
//! ```text
//! focus_left = mod+h, mod+Left        # focus the tile that way
//! focus_down = mod+j, mod+Down
//! focus_up = mod+k, mod+Up
//! focus_right = mod+l, mod+Right
//! swap_left = mod+shift+h, mod+shift+Left        # swap the tile with the one that way
//! swap_down = mod+shift+j, mod+shift+Down
//! swap_up = mod+shift+k, mod+shift+Up
//! swap_right = mod+shift+l, mod+shift+Right
//! move_tab_left = mod+ctrl+h, mod+ctrl+Left      # join the column that way as a tab, or leave a tabbed one
//! move_tab_down = mod+ctrl+j, mod+ctrl+Down
//! move_tab_up = mod+ctrl+k, mod+ctrl+Up
//! move_tab_right = mod+ctrl+l, mod+ctrl+Right
//! focus_monitor_left = none           # focus the monitor that way (monitors stacked: mod+Up / mod+Down,
//! focus_monitor_down = none           # with those keys taken out of focus_up / focus_down)
//! focus_monitor_up = none
//! focus_monitor_right = none
//! desk_1 = mod+1                      # show that desktop (desk_1 .. desk_9: mod+1 .. mod+9)
//! send_to_desk_1 = mod+shift+1        # send the tile there (send_to_desk_1 .. _9: mod+shift+1 .. 9)
//! empty_container = mod+Return        # an empty container, dune-finder to fill it
//! empty_tab = mod+shift+t             # one as the next tab of the focused tile's container
//! next_tab = mod+Tab
//! prev_tab = mod+shift+Tab, mod+shift+ISO_Left_Tab
//! wider = mod+equal, mod+plus         # the column's width
//! narrower = mod+minus
//! taller = mod+shift+equal, mod+shift+plus       # the tile's height
//! shorter = mod+shift+minus, mod+shift+underscore
//! row_wider = mod+ctrl+equal, mod+ctrl+plus      # the tile's width inside its row
//! row_narrower = mod+ctrl+minus
//! cycle_width = mod+r
//! full_width = mod+f
//! center = mod+space                  # center the column (again: stop)
//! float = mod+shift+space             # float the tile (again: back into the strip)
//! send_to_scratchpad = mod+shift+grave  # the tile off every desktop, into the scratchpad
//! scratchpad = mod+grave              # the next scratchpad window floats here (on it again: back)
//! close = mod+q
//! launcher = mod+d                    # dune-finder on the apps
//! tile_options = mod+slash            # dune-finder on the focused tile's options (private, float, ...)
//! switcher = none                     # dune-finder on the windows (`/` in it gets there too)
//! private_tile = none                 # block the focused tile on screen and in captures (again: show it)
//! fold = none                         # fold the focused tile into the accordion (on a folded one: open it)
//! terminal_vertical = mod+v           # a terminal right of the tile, in its folder
//! terminal_horizontal = mod+s         # one below it
//! terminal_tab = none                 # one as a new tab of its container
//! stack_to_tabs = mod+t
//! quit = mod+shift+e
//! lock = mod+Escape                   # runs `lock_command`
//! screenshot = Print                  # of a region chosen with the mouse (Esc: none)
//! screenshot_tile = ctrl+Print        # of the focused tile
//! screenshot_screen = shift+Print     # of the active monitor
//! record = mod+shift+r                # record a region chosen with the mouse (any record key again: stop)
//! record_tile = mod+ctrl+r            # record the focused tile
//! record_screen = mod+ctrl+shift+r    # record the active monitor
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
//! `file_manager` at it. `>` typed first jumps, like zoxide's `z`: the folders terminals
//! worked in (and the finder opened), the most used and recent first; Enter opens a
//! terminal there, Mod+Enter the `file_manager`.
//!
//! With Mod held, the pointer near an edge of a tile lights a band along that edge;
//! a Mod+click there opens an empty container on that side with a compact dune-finder (no
//! list until something is typed). A Mod+click elsewhere floats the tile.
//!
//! Tabs belong to columns (each tab with its own splits) and have keys (their only settings
//! are `tab_min_width` and `tab_max_width`):
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
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
};

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

/// `fold_click`: a click, or `none` (the default); a value that is not one is warned about.
fn fold_click(file: &HashMap<String, String>, modifier: &str) -> Option<ClickSpec> {
    let value = file.get("fold_click").filter(|v| !v.eq_ignore_ascii_case("none"))?;
    let click = ClickSpec::parse(value, modifier);
    if click.is_none() {
        tracing::warn!("ignoring `fold_click = {value}`: not modifiers and left, right or middle, nor none");
    }
    click
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

/// Keys separated by commas, or `none`; `None` if one of them is not a key.
fn key_list(value: &str, modifier: &str) -> Option<Vec<KeySpec>> {
    if value.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    value.split(',').map(|spec| KeySpec::parse(spec, modifier)).collect()
}

/// The keys of every binding, in table order: the file's in place of the defaults.
fn bindings(file: &HashMap<String, String>, modifier: &str) -> Vec<(Binding, KeySpec)> {
    // With the name and whether the file set it, for the warnings.
    let mut taken: Vec<(String, Binding, KeySpec, bool)> = Vec::new();
    for (name, binding, defaults) in binding_table() {
        let set = file.get(&name).and_then(|value| {
            let keys = key_list(value, modifier);
            if keys.is_none() {
                tracing::warn!("ignoring `{name} = {value}`: not keys like mod+q, super+shift+t or none");
            }
            keys
        });
        let from_file = set.is_some();
        let keys = set.unwrap_or_else(|| key_list(&defaults, modifier).unwrap_or_default());
        for key in keys {
            if let Some((other, other_binding, _, other_from_file)) = taken.iter().find(|(_, _, k, _)| *k == key) {
                // Defaults collapsing under `modifier = ctrl+alt` are expected, not news.
                if *other_binding != binding && (from_file || *other_from_file) {
                    tracing::warn!("`{name}`: {key} already does `{other}`, it stays there");
                }
                continue;
            }
            taken.push((name.clone(), binding, key, from_file));
        }
    }
    taken.into_iter().map(|(_, binding, key, _)| (binding, key)).collect()
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

/// Mice and touchpads (`input.<field>` in the file); unset ones keep libinput's defaults.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InputConfig {
    pub left_handed: Option<bool>,
    pub natural_scroll: Option<bool>,
    pub tap: Option<bool>,
    /// -1.0 to 1.0.
    pub accel_speed: Option<f64>,
}

/// Settings of one monitor (`output.<NAME>.<field>` in the file).
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
    /// Icon theme for dune-finder; empty: the one GTK apps use.
    pub icon_theme: String,
    /// The terminal file manager dune-finder opens a folder or file in (`yazi PATH`).
    pub file_manager: String,
    /// The terminal `file_manager` runs in (`foot yazi PATH`); empty: `terminal`.
    pub file_manager_terminal: String,
    /// The border of tiles without focus (premultiplied RGBA); `None`: not drawn.
    pub border_color: Option<[f32; 4]>,
    /// Shell commands run when dune starts (`exec` lines, in order).
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

/// The config a file with `text` gives, without the environment.
#[cfg(test)]
pub fn from_text(text: &str) -> Config {
    resolve(&parse(text), &|_| None)
}

pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("dune").join("config"))
}

pub fn load() -> Config {
    let text = path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    Config { exec: execs(&text), ..resolve(&parse(&text), &|key| std::env::var(key).ok()) }
}

/// The `exec` lines: unlike other keys, each one counts.
fn execs(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split('#').next().unwrap_or("").split_once('=')?;
            (key.trim() == "exec" && !value.trim().is_empty()).then(|| value.trim().to_string())
        })
        .collect()
}

fn parse(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let line = line.split('#').next().unwrap_or("").trim();
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn resolve(file: &HashMap<String, String>, env: &dyn Fn(&str) -> Option<String>) -> Config {
    let get = |env_key: &str, file_key: &str, default: &str| {
        env(env_key)
            .filter(|v| !v.is_empty())
            .or_else(|| file.get(file_key).cloned())
            .unwrap_or_else(|| default.to_string())
    };
    // Nested inside another compositor Super is usually taken by the host.
    let modifier = get("DUNE_MOD", "modifier", "alt");
    Config {
        keyboard_rules: get("XKB_DEFAULT_RULES", "keyboard_rules", ""),
        keyboard_model: get("XKB_DEFAULT_MODEL", "keyboard_model", ""),
        keyboard_layout: get("XKB_DEFAULT_LAYOUT", "keyboard_layout", ""),
        keyboard_variant: get("XKB_DEFAULT_VARIANT", "keyboard_variant", ""),
        keyboard_options: get("XKB_DEFAULT_OPTIONS", "keyboard_options", ""),
        repeat_delay: number(file, "repeat_delay", 500, 100..=2000),
        repeat_rate: number(file, "repeat_rate", 25, 1..=100),
        // A new process per window: wezterm's single instance would open every window
        // from the first one, whose `DUNE_TILE` and pid no longer find the new tile.
        terminal: get("DUNE_TERMINAL", "terminal", "wezterm start --always-new-process"),
        modifier: modifier.clone(),
        xwayland: get("DUNE_XWAYLAND", "xwayland", "satellite"),
        bindings: bindings(file, &modifier),
        lock_command: file
            .get("lock_command")
            .cloned()
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "swaylock -f".into()),
        follow: flag(file, "follow"),
        focus_follows_mouse: flag(file, "focus_follows_mouse"),
        fold_click: fold_click(file, &modifier),
        outputs: outputs(file),
        input: input(file),
        font_family: file.get("font_family").cloned().unwrap_or_else(|| "monospace".into()),
        tab_widths: tab_widths(file),
        icon_theme: file.get("icon_theme").cloned().unwrap_or_default(),
        file_manager: file
            .get("file_manager")
            .cloned()
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| "yazi".into()),
        file_manager_terminal: file
            .get("file_manager_terminal")
            .cloned()
            .unwrap_or_default(),
        border_color: border_color(file),
        exec: Vec::new(),
        screenshot_dir: file.get("screenshot_dir").cloned().unwrap_or_default(),
        recording_dir: file.get("recording_dir").cloned().unwrap_or_default(),
    }
}

/// `border_color`: `RRGGBB`, `RRGGBBAA` or `none`; white when missing or not one.
fn border_color(file: &HashMap<String, String>) -> Option<[f32; 4]> {
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let Some(value) = file.get("border_color") else { return Some(WHITE) };
    if value.eq_ignore_ascii_case("none") {
        return None;
    }
    let hex = value.trim_start_matches("0x");
    let channel = |i: usize| hex.get(i..i + 2).and_then(|c| u8::from_str_radix(c, 16).ok()).map(|c| c as f32 / 255.0);
    let rgba = match hex.len() {
        6 => (0..3).map(|i| channel(i * 2)).chain([Some(1.0)]).collect::<Option<Vec<_>>>(),
        8 => (0..4).map(|i| channel(i * 2)).collect(),
        _ => None,
    };
    let Some([r, g, b, a]) = rgba.and_then(|v| <[f32; 4]>::try_from(v).ok()) else {
        tracing::warn!("ignoring `border_color = {value}`: not RRGGBB, RRGGBBAA or none");
        return Some(WHITE);
    };
    // Solid colour buffers take premultiplied alpha.
    Some([r * a, g * a, b * a, a])
}

/// `tab_min_width` and `tab_max_width`; a maximum under the minimum is raised to it.
fn tab_widths(file: &HashMap<String, String>) -> TabWidths {
    let default = TabWidths::default();
    let min = number(file, "tab_min_width", default.min, 40..=2000);
    let max = number(file, "tab_max_width", default.max, 40..=2000);
    if max < min {
        tracing::warn!("`tab_max_width = {max}` is under `tab_min_width = {min}`, using {min} for both");
    }
    TabWidths { min, max: max.max(min) }
}

fn number(file: &HashMap<String, String>, key: &str, default: i32, range: std::ops::RangeInclusive<i32>) -> i32 {
    let Some(value) = file.get(key) else { return default };
    value.parse().ok().filter(|v| range.contains(v)).unwrap_or_else(|| {
        tracing::warn!("ignoring `{key} = {value}`: not a number from {} to {}", range.start(), range.end());
        default
    })
}

/// A `true`/`false` setting, false when missing or not one.
fn flag(file: &HashMap<String, String>, key: &str) -> bool {
    file.get(key).is_some_and(|value| {
        let parsed = bool_value(value);
        if parsed.is_none() {
            tracing::warn!("ignoring `{key} = {value}`: not true or false");
        }
        parsed.unwrap_or(false)
    })
}

fn bool_value(value: &str) -> Option<bool> {
    match value {
        "true" | "yes" => Some(true),
        "false" | "no" => Some(false),
        _ => None,
    }
}

fn input(file: &HashMap<String, String>) -> InputConfig {
    let mut input = InputConfig::default();
    for (key, value) in file {
        let Some(field) = key.strip_prefix("input.") else { continue };
        let ok = match field {
            "left_handed" => bool_value(value).map(|v| input.left_handed = Some(v)).is_some(),
            "natural_scroll" => bool_value(value).map(|v| input.natural_scroll = Some(v)).is_some(),
            "tap" => bool_value(value).map(|v| input.tap = Some(v)).is_some(),
            "accel_speed" => value
                .parse::<f64>()
                .ok()
                .filter(|v| (-1.0..=1.0).contains(v))
                .map(|v| input.accel_speed = Some(v))
                .is_some(),
            _ => false,
        };
        if !ok {
            tracing::warn!("ignoring `{key} = {value}`");
        }
    }
    input
}

fn outputs(file: &HashMap<String, String>) -> BTreeMap<String, OutputConfig> {
    let mut outputs = BTreeMap::<String, OutputConfig>::new();
    for (key, value) in file {
        let Some((name, field)) = key.strip_prefix("output.").and_then(|k| k.rsplit_once('.')) else { continue };
        let output = outputs.entry(name.to_string()).or_default();
        let ok = match field {
            "mode" => OutputMode::parse(value).map(|m| output.mode = Some(m)).is_some(),
            "position" => value
                .split_once(',')
                .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                .map(|p| output.position = Some(p))
                .is_some(),
            "off" => bool_value(value).map(|v| output.off = v).is_some(),
            _ => false,
        };
        if !ok {
            tracing::warn!("ignoring `{key} = {value}`");
        }
    }
    outputs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_values_and_defaults() {
        let file = parse("# my setup\nkeyboard_layout = us\nkeyboard_variant=dvorak  # comment\n\nnonsense line\n");
        let c = resolve(&file, &|_| None);
        assert_eq!(c.keyboard_layout, "us");
        assert_eq!(c.keyboard_variant, "dvorak");
        assert_eq!(c.keyboard_options, "");
        assert_eq!(c.terminal, "wezterm start --always-new-process");
        assert_eq!(c.modifier, "alt");
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
            let c = resolve(&parse(&format!("modifier = {modifier}\n")), &|_| None);
            assert_eq!(keys_of(&c, Binding::Close), [spec(&format!("{held}+q"))], "{modifier}");
            assert_eq!(keys_of(&c, Binding::Focus(Direction::Left)), [spec(&format!("{held}+h")), spec(&format!("{held}+Left"))]);
            assert_eq!(keys_of(&c, Binding::SendToDesk(9)), [spec(&format!("{held}+shift+9"))]);
            assert_eq!(keys_of(&c, Binding::TileOptions), [spec(&format!("{held}+slash"))]);
        }
        // `DUNE_MOD` wins over the file, for the keys too.
        let env = |k: &str| (k == "DUNE_MOD").then(|| "super".to_string());
        let c = resolve(&parse("modifier = alt\n"), &env);
        assert_eq!(keys_of(&c, Binding::Close), [spec("super+q")]);
        // Every binding has a key by default, but the windows (`/` in dune-finder), the
        // private switches (in the tile options), folding (`fold_click`), a terminal tab (Mod+Shift+t opens
        // an empty one) and the monitors (their keys depend on how they are placed).
        let c = resolve(&parse(""), &|_| None);
        let unbound = [Binding::Switcher, Binding::PrivateTile, Binding::Fold, Binding::Terminal(TerminalHere::Tab)];
        for (name, binding, defaults) in binding_table() {
            let none = unbound.contains(&binding) || matches!(binding, Binding::FocusMonitor(_));
            assert_eq!(keys_of(&c, binding).is_empty(), none, "{name}");
            assert!(key_list(&defaults, "alt").is_some(), "{name} = {defaults}");
        }
    }

    #[test]
    fn fold_click_is_a_button_with_modifiers() {
        let c = resolve(&parse("modifier = super\nfold_click = mod+right\n"), &|_| None);
        let click = c.fold_click.unwrap();
        assert_eq!((click.logo, click.alt, click.ctrl, click.shift, click.button), (true, false, false, false, 0x111));
        assert_eq!(ClickSpec::parse("ctrl+shift+Middle", "alt").map(|c| (c.ctrl, c.shift, c.button)), Some((true, true, 0x112)));
        // None by default; `none`, a key or an unknown modifier: none.
        for text in ["", "fold_click = none\n", "fold_click = mod+q\n", "fold_click = hyper+left\n"] {
            assert_eq!(resolve(&parse(text), &|_| None).fold_click, None, "{text}");
        }
    }

    #[test]
    fn monitor_focus_takes_keys_given_up_by_tile_focus() {
        let c = resolve(&parse("focus_up = mod+k
focus_monitor_up = mod+Up
"), &|_| None);
        assert_eq!(keys_of(&c, Binding::Focus(Direction::Up)), [spec("alt+k")]);
        assert_eq!(keys_of(&c, Binding::FocusMonitor(Direction::Up)), [spec("alt+Up")]);
    }

    #[test]
    fn a_value_replaces_the_defaults() {
        let c = resolve(&parse("modifier = super\nclose = super+q, super+'\nlauncher = mod+e\n"), &|_| None);
        assert_eq!(keys_of(&c, Binding::Close), [spec("super+q"), spec("super+apostrophe")]);
        assert_eq!(keys_of(&c, Binding::Launcher), [spec("super+e")]);
        // The others keep theirs.
        assert_eq!(keys_of(&c, Binding::TileOptions), [spec("super+slash")]);
        let c = resolve(&parse("terminal_vertical = super+ctrl+v\n"), &|_| None);
        assert_eq!(keys_of(&c, Binding::Terminal(TerminalHere::Right)), [spec("super+ctrl+v")]);
        assert_eq!(keys_of(&c, Binding::Terminal(TerminalHere::Below)), [spec("alt+s")]);
    }

    #[test]
    fn none_turns_a_key_off() {
        let c = resolve(&parse("quit = none\nlock = None\n"), &|_| None);
        assert!(keys_of(&c, Binding::Quit).is_empty());
        assert!(keys_of(&c, Binding::Lock).is_empty());
        assert!(!c.bindings.iter().any(|(_, k)| *k == spec("alt+shift+e")));
    }

    #[test]
    fn keys_are_unshifted_and_lowercase() {
        let c = resolve(&parse("lock = mod+E\nquit = super+shift+Q\n"), &|_| None);
        assert_eq!(keys_of(&c, Binding::Lock), [spec("alt+e")]);
        assert_eq!(keys_of(&c, Binding::Quit), [spec("super+shift+q")]);
        assert_eq!(spec("alt+e").key, Keysym::e);
    }

    #[test]
    fn a_bad_value_keeps_the_defaults() {
        for value in ["hyper+q", "mod+q, mod+NotAKey", "", "mod+"] {
            let c = resolve(&parse(&format!("close = {value}\n")), &|_| None);
            assert_eq!(keys_of(&c, Binding::Close), [spec("alt+q")], "{value:?}");
        }
    }

    #[test]
    fn ctrl_keys_give_way_under_ctrl_alt() {
        let c = resolve(&parse("modifier = ctrl+alt\n"), &|_| None);
        let ctrl_alt_left = spec("ctrl+alt+Left");
        assert_eq!(c.bindings.iter().find(|(_, k)| *k == ctrl_alt_left).map(|(b, _)| *b), Some(Binding::Focus(Direction::Left)));
        assert!(keys_of(&c, Binding::MoveTab(Direction::Left)).is_empty());
        assert!(keys_of(&c, Binding::RowWider).is_empty());
        assert_eq!(keys_of(&c, Binding::Wider), [spec("ctrl+alt+equal"), spec("ctrl+alt+plus")]);
    }

    #[test]
    fn scratchpad_keys() {
        let c = resolve(&parse(""), &|_| None);
        assert_eq!(keys_of(&c, Binding::Scratchpad), [spec("alt+grave")]);
        assert_eq!(keys_of(&c, Binding::SendToScratchpad), [spec("alt+shift+grave")]);
        let c = resolve(&parse("scratchpad = none
"), &|_| None);
        assert!(keys_of(&c, Binding::Scratchpad).is_empty());
    }

    #[test]
    fn tile_options_and_private_keys() {
        let c = resolve(&parse(""), &|_| None);
        assert_eq!(keys_of(&c, Binding::TileOptions), [spec("alt+slash")]);
        assert!(keys_of(&c, Binding::Switcher).is_empty());
        assert!(keys_of(&c, Binding::PrivateTile).is_empty());
        // Still settable; a `switcher` set to Mod+/ gives way to `tile_options`, listed first.
        let c = resolve(&parse("switcher = mod+w\nprivate_tile = mod+p\n"), &|_| None);
        assert_eq!(keys_of(&c, Binding::Switcher), [spec("alt+w")]);
        assert_eq!(keys_of(&c, Binding::PrivateTile), [spec("alt+p")]);
        // `private_desktop` is gone (desktops lock from the bar): an unknown key, ignored.
        assert!(!binding_table().iter().any(|(name, ..)| name == "private_desktop"));
        let c = resolve(&parse("private_desktop = mod+shift+p\n"), &|_| None);
        assert!(!c.bindings.iter().any(|(_, k)| *k == spec("alt+shift+p")));
        let c = resolve(&parse("switcher = mod+slash\n"), &|_| None);
        assert_eq!(keys_of(&c, Binding::TileOptions), [spec("alt+slash")]);
        assert!(keys_of(&c, Binding::Switcher).is_empty());
    }

    #[test]
    fn key_labels() {
        let c = resolve(&parse("modifier = super\nclose = mod+q, super+'\n"), &|_| None);
        assert_eq!(c.key_label(Binding::Close), "super+q, super+apostrophe");
        assert_eq!(c.key_label(Binding::PrivateTile), "");
    }

    #[test]
    fn lock_key_and_command() {
        let c = resolve(&parse(""), &|_| None);
        assert_eq!(keys_of(&c, Binding::Lock), [spec("alt+Escape")]);
        assert_eq!(keys_of(&c, Binding::Lock)[0].key, Keysym::Escape);
        assert_eq!(c.lock_command, "swaylock -f");
        let c = resolve(&parse("lock_command = swaylock -f -c 000000\n"), &|_| None);
        assert_eq!(c.lock_command, "swaylock -f -c 000000");
    }

    #[test]
    fn capture_keys_and_folders() {
        let c = resolve(&parse(""), &|_| None);
        assert_eq!(keys_of(&c, Binding::Screenshot), [spec("Print")]);
        assert_eq!(keys_of(&c, Binding::ScreenshotScreen), [spec("shift+Print")]);
        assert_eq!(keys_of(&c, Binding::Record), [spec("alt+shift+r")]);
        assert_eq!(keys_of(&c, Binding::ScreenshotTile), [spec("ctrl+Print")]);
        assert_eq!(keys_of(&c, Binding::RecordTile), [spec("alt+ctrl+r")]);
        assert_eq!(keys_of(&c, Binding::RecordScreen), [spec("alt+ctrl+shift+r")]);
        assert_eq!(keys_of(&c, Binding::CycleWidth), [spec("alt+r")]);
        assert_eq!((c.screenshot_dir.as_str(), c.recording_dir.as_str()), ("", ""));
        let c = resolve(&parse("modifier = super
screenshot = super+p
record = none
"), &|_| None);
        assert_eq!(keys_of(&c, Binding::Screenshot), [spec("super+p")]);
        assert!(keys_of(&c, Binding::Record).is_empty());
        assert_eq!(keys_of(&c, Binding::ScreenshotScreen), [spec("shift+Print")]);
        let c = resolve(&parse("screenshot_dir = ~/shots
recording_dir = /data/videos
"), &|_| None);
        assert_eq!(c.screenshot_dir, "~/shots");
        assert_eq!(c.recording_dir, "/data/videos");
    }

    #[test]
    fn a_shared_key_stays_with_the_first() {
        // `close` comes before `lock`: alt+q stays a close key, lock is left with none.
        let c = resolve(&parse("lock = mod+q\n"), &|_| None);
        assert_eq!(keys_of(&c, Binding::Close), [spec("alt+q")]);
        assert!(keys_of(&c, Binding::Lock).is_empty());
        // Set on the earlier one, it takes the key from the later one's defaults.
        let c = resolve(&parse("focus_left = mod+t\n"), &|_| None);
        assert_eq!(keys_of(&c, Binding::Focus(Direction::Left)), [spec("alt+t")]);
        assert!(keys_of(&c, Binding::StackToTabs).is_empty());
        // Each key once.
        let keys: Vec<_> = c.bindings.iter().map(|(_, k)| *k).collect();
        assert!(keys.iter().enumerate().all(|(i, k)| !keys[..i].contains(k)));
    }

    #[test]
    fn every_exec_line_counts() {
        let text = "exec = waybar -c ~/.config/waybar/dune.jsonc\nterminal = foot\nexec = mako  # notifications\nexec =\n";
        assert_eq!(execs(text), ["waybar -c ~/.config/waybar/dune.jsonc", "mako"]);
        assert!(execs("").is_empty());
    }

    #[test]
    fn key_repeat() {
        let config = resolve(&parse(""), &|_| None);
        assert_eq!((config.repeat_delay, config.repeat_rate), (500, 25));
        let config = resolve(&parse("repeat_delay = 600\nrepeat_rate = 30\n"), &|_| None);
        assert_eq!((config.repeat_delay, config.repeat_rate), (600, 30));
        let config = resolve(&parse("repeat_delay = 5\nrepeat_rate = fast\n"), &|_| None);
        assert_eq!((config.repeat_delay, config.repeat_rate), (500, 25));
    }

    #[test]
    fn follow() {
        assert!(!resolve(&parse(""), &|_| None).follow);
        assert!(resolve(&parse("follow = true\n"), &|_| None).follow);
        assert!(!resolve(&parse("follow = maybe\n"), &|_| None).follow);
        assert!(!resolve(&parse(""), &|_| None).focus_follows_mouse);
        assert!(resolve(&parse("focus_follows_mouse = true\n"), &|_| None).focus_follows_mouse);
    }

    #[test]
    fn tab_widths_come_from_the_file() {
        assert_eq!(resolve(&parse(""), &|_| None).tab_widths, TabWidths::default());
        let config = resolve(&parse("tab_min_width = 120\ntab_max_width = 300\n"), &|_| None);
        assert_eq!(config.tab_widths, TabWidths { min: 120, max: 300 });
        // Out of range: the default; a maximum under the minimum: the minimum.
        let config = resolve(&parse("tab_min_width = 5\ntab_max_width = 100\n"), &|_| None);
        assert_eq!(config.tab_widths, TabWidths { min: TabWidths::default().min, max: TabWidths::default().min });
    }

    #[test]
    fn border_color() {
        assert_eq!(resolve(&parse(""), &|_| None).border_color, Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(resolve(&parse("border_color = none\n"), &|_| None).border_color, None);
        assert_eq!(resolve(&parse("border_color = ff0000\n"), &|_| None).border_color, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(resolve(&parse("border_color = ffffff00\n"), &|_| None).border_color, Some([0.0, 0.0, 0.0, 0.0]));
        // `#` starts a comment, so `#333333` reads as no value: still white.
        assert_eq!(resolve(&parse("border_color = #333333\n"), &|_| None).border_color, Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(resolve(&parse("border_color = blue\n"), &|_| None).border_color, Some([1.0, 1.0, 1.0, 1.0]));
    }

    #[test]
    fn file_manager() {
        assert_eq!(resolve(&parse(""), &|_| None).file_manager, "yazi");
        assert_eq!(
            resolve(&parse("file_manager = lf\n"), &|_| None).file_manager,
            "lf"
        );
        assert_eq!(resolve(&parse(""), &|_| None).file_manager_terminal, "");
        assert_eq!(
            resolve(&parse("file_manager_terminal = foot\n"), &|_| None).file_manager_terminal,
            "foot"
        );
    }

    #[test]
    fn outputs() {
        let c = resolve(
            &parse(
                "output.HDMI-A-1.mode = 2560x1080@59.94\noutput.HDMI-A-1.position = 0, 0\n\
                 output.HDMI-A-2.mode = 1920x1080\noutput.HDMI-A-2.position = 311,1080\n\
                 output.DP-1.off = true\noutput.DP-2.mode = big\n",
            ),
            &|_| None,
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
        let c = resolve(
            &parse("input.left_handed = true\ninput.natural_scroll = no\ninput.accel_speed = 2\ninput.tap = maybe\n"),
            &|_| None,
        );
        assert_eq!(c.input.left_handed, Some(true));
        assert_eq!(c.input.natural_scroll, Some(false));
        assert_eq!(c.input.accel_speed, None); // out of range: libinput's default
        assert_eq!(c.input.tap, None);
        assert_eq!(resolve(&parse(""), &|_| None).input, InputConfig::default());
    }

    #[test]
    fn restart_needed() {
        let old = resolve(&parse("xwayland = off\noutput.A.mode = 1920x1080\noutput.A.position = 0,0\n"), &|_| None);
        let new = resolve(
            &parse("xwayland = off\noutput.A.mode = 2560x1080\noutput.A.position = 10,0\noutput.B.off = true\nmodifier = super\n"),
            &|_| None,
        );
        assert_eq!(new.restart_needed(&old), ["output.A.mode", "output.B.off"]);
        assert!(old.restart_needed(&old).is_empty());
    }

    #[test]
    fn environment_wins() {
        let file = parse("keyboard_variant = dvorak\nterminal = alacritty\n");
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
