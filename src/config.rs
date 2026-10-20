//! Settings from `~/.config/dune/config` (or `$XDG_CONFIG_HOME/dune/config`).
//!
//! One `key = value` per line, `#` starts a comment:
//!
//! ```text
//! keyboard_layout = us
//! keyboard_variant = dvorak
//! repeat_delay = 500      # ms a held key waits before repeating
//! repeat_rate = 25        # repeats per second while held
//! terminal = dune-term  # or foot, alacritty, ...
//! modifier = alt          # alt, super or ctrl+alt
//! direction_right = super+ctrl+Right   # direction indicator keys (also _left, _up, _down)
//! close = super+'         # extra key that closes the focused tile (Mod+q always does)
//! launcher = super+e      # key that toggles the launcher (default Mod+d)
//! switcher = super+g      # key that toggles the window switcher (default Mod+/)
//! follow = true           # go to the desktop an app is launched on (`firefox @3`); default false
//! focus_follows_mouse = true  # the tile under the pointer takes focus (when all of it is on screen); default false
//! xwayland = satellite    # X11 apps through xwayland-satellite, or `off`
//! font_family = JetBrains Mono   # tab titles and launcher (dune-term reads `~/.config/dune/term`)
//! tab_min_width = 200     # px a tab shrinks to before its bar scrolls; default 200
//! tab_max_width = 220     # px a tab grows to while there is room; default 220
//! icon_theme = Papirus-Dark      # launcher icons; default: the desktop's (gsettings / GTK)
//! border_color = none     # border of tiles without focus: RRGGBB or RRGGBBAA (no `#`), or none; default ffffff
//! exec = waybar           # run when dune starts; one line per program, as many as needed
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
//! The pointer pushing on against the left or right edge of the screens slides the strip
//! that way; it stays there until focus moves. A mark at the middle of an edge (`‹3`, `2›`)
//! tells how many tiles are past it. Mod+/ lists every window to go to.
//!
//! Mod+Enter opens an empty container with the launcher in it: Enter right away puts a
//! terminal (`terminal`) there, a program chosen or typed goes there instead. Mod+d is the
//! launcher on its own.
//!
//! Tabs belong to columns (each tab with its own splits) and have keys (their only settings
//! are `tab_min_width` and `tab_max_width`):
//! Mod+Tab / Mod+Shift+Tab show the next / previous tab, Mod+Ctrl+direction joins the
//! column that way as a tab (or leaves a tabbed one), Mod+t sends a tile out of its split
//! into a tab of its own (a lone tile: a bar with it as the one tab), a click on a tab
//! shows it, a Mod+drag let go on a tab bar adds the tile there as a tab, a tab Mod+dragged
//! along its own bar moves to the place it is let go, and the launcher opens an app as a tab with
//! Ctrl+T or `@t`. The tab bar sits at the top left of its column (see
//! `layout::split_tab_bar`; its placement may become a setting).
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

use crate::layout::{Side, TabWidths};

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
    pub fn parse(spec: &str) -> Option<KeySpec> {
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
        let mut spec = KeySpec { logo: false, alt: false, ctrl: false, shift: false, key };
        for m in parts {
            match m.to_lowercase().as_str() {
                "super" | "logo" | "mod4" => spec.logo = true,
                "alt" | "mod1" => spec.alt = true,
                "ctrl" | "control" => spec.ctrl = true,
                "shift" => spec.shift = true,
                _ => return None,
            }
        }
        Some(spec)
    }

    /// Exactly these modifiers (so `super+Right` is not taken by `super+shift+Right`).
    pub fn matches(&self, mods: &ModifiersState, key: Keysym) -> bool {
        key == self.key && mods.logo == self.logo && mods.alt == self.alt && mods.ctrl == self.ctrl && mods.shift == self.shift
    }
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
    /// Keys that point the focused tile's direction indicator at each side.
    pub direction_keys: Vec<(Side, KeySpec)>,
    /// An extra key that closes the focused tile, besides Mod+q.
    pub close_key: Option<KeySpec>,
    /// The key that toggles the launcher, instead of Mod+d.
    pub launcher_key: Option<KeySpec>,
    /// The key that toggles the window switcher, instead of Mod+/.
    pub switcher_key: Option<KeySpec>,
    /// Show the desktop the launcher opens an app on, instead of staying here.
    pub follow: bool,
    /// The tile under the pointer takes focus as the pointer moves.
    pub focus_follows_mouse: bool,
    /// `satellite` (default) or `off`.
    pub xwayland: String,
    /// Monitors by connector name, like `HDMI-A-1`.
    pub outputs: BTreeMap<String, OutputConfig>,
    pub input: InputConfig,
    /// As fontconfig names it; tab titles are written with it.
    pub font_family: String,
    /// How narrow and how wide tabs get in their bars.
    pub tab_widths: TabWidths,
    /// Icon theme for the launcher; empty: the one GTK apps use.
    pub icon_theme: String,
    /// The border of tiles without focus (premultiplied RGBA); `None`: not drawn.
    pub border_color: Option<[f32; 4]>,
    /// Shell commands run when dune starts (`exec` lines, in order).
    pub exec: Vec<String>,
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
    Config {
        keyboard_rules: get("XKB_DEFAULT_RULES", "keyboard_rules", ""),
        keyboard_model: get("XKB_DEFAULT_MODEL", "keyboard_model", ""),
        keyboard_layout: get("XKB_DEFAULT_LAYOUT", "keyboard_layout", ""),
        keyboard_variant: get("XKB_DEFAULT_VARIANT", "keyboard_variant", ""),
        keyboard_options: get("XKB_DEFAULT_OPTIONS", "keyboard_options", ""),
        repeat_delay: number(file, "repeat_delay", 500, 100..=2000),
        repeat_rate: number(file, "repeat_rate", 25, 1..=100),
        terminal: get("DUNE_TERMINAL", "terminal", "dune-term"),
        // Nested inside another compositor Super is usually taken by the host.
        modifier: get("DUNE_MOD", "modifier", "alt"),
        xwayland: get("DUNE_XWAYLAND", "xwayland", "satellite"),
        direction_keys: [
            (Side::Left, "direction_left", "super+ctrl+Left"),
            (Side::Right, "direction_right", "super+ctrl+Right"),
            (Side::Up, "direction_up", "super+ctrl+Up"),
            (Side::Down, "direction_down", "super+ctrl+Down"),
        ]
        .into_iter()
        .filter_map(|(side, key, default)| {
            let spec = file.get(key).map(String::as_str).unwrap_or(default);
            let parsed = KeySpec::parse(spec);
            if parsed.is_none() {
                tracing::warn!("ignoring `{key} = {spec}`: not a key like super+Right");
            }
            Some((side, parsed?))
        })
        .collect(),
        close_key: file.get("close").and_then(|spec| {
            let parsed = KeySpec::parse(spec);
            if parsed.is_none() {
                tracing::warn!("ignoring `close = {spec}`: not a key like super+q");
            }
            parsed
        }),
        launcher_key: file.get("launcher").and_then(|spec| {
            let parsed = KeySpec::parse(spec);
            if parsed.is_none() {
                tracing::warn!("ignoring `launcher = {spec}`: not a key like super+d");
            }
            parsed
        }),
        switcher_key: file.get("switcher").and_then(|spec| {
            let parsed = KeySpec::parse(spec);
            if parsed.is_none() {
                tracing::warn!("ignoring `switcher = {spec}`: not a key like super+g");
            }
            parsed
        }),
        follow: flag(file, "follow"),
        focus_follows_mouse: flag(file, "focus_follows_mouse"),
        outputs: outputs(file),
        input: input(file),
        font_family: file.get("font_family").cloned().unwrap_or_else(|| "monospace".into()),
        tab_widths: tab_widths(file),
        icon_theme: file.get("icon_theme").cloned().unwrap_or_default(),
        border_color: border_color(file),
        exec: Vec::new(),
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
        assert_eq!(c.terminal, "dune-term");
        assert_eq!(c.modifier, "alt");
    }

    #[test]
    fn key_specs() {
        let spec = KeySpec::parse("super+Right").unwrap();
        assert!(spec.logo && !spec.alt && spec.key == Keysym::Right);
        assert_eq!(KeySpec::parse("Ctrl + Alt + h").unwrap().key, Keysym::h);
        assert!(KeySpec::parse("hyper+Right").is_none());
        assert!(KeySpec::parse("super+NotAKey").is_none());
        assert_eq!(KeySpec::parse("super+'").unwrap().key, Keysym::apostrophe);
        let mods = |logo, shift| ModifiersState { logo, shift, ..Default::default() };
        assert!(spec.matches(&mods(true, false), Keysym::Right));
        assert!(!spec.matches(&mods(true, true), Keysym::Right));
        assert!(!spec.matches(&mods(false, false), Keysym::Right));
    }

    #[test]
    fn direction_keys_from_file_or_default() {
        let c = resolve(&parse("direction_left = ctrl+alt+Left\ndirection_up = nonsense+Up\n"), &|_| None);
        let key = |side| c.direction_keys.iter().find(|(s, _)| *s == side).map(|(_, k)| *k);
        assert_eq!(key(Side::Left), KeySpec::parse("ctrl+alt+Left"));
        assert_eq!(key(Side::Right), KeySpec::parse("super+ctrl+Right"));
        assert_eq!(key(Side::Up), None); // invalid: no binding rather than a wrong one
    }

    #[test]
    fn close_key() {
        assert_eq!(resolve(&parse(""), &|_| None).close_key, None);
        let c = resolve(&parse("close = super+'\n"), &|_| None);
        assert_eq!(c.close_key, KeySpec::parse("super+apostrophe"));
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
    fn launcher_key() {
        assert_eq!(resolve(&parse(""), &|_| None).launcher_key, None);
        let c = resolve(&parse("launcher = super+e\n"), &|_| None);
        assert_eq!(c.launcher_key, KeySpec::parse("super+e"));
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
