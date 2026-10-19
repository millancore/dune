//! Settings from `~/.config/dune/config` (or `$XDG_CONFIG_HOME/dune/config`).
//!
//! One `key = value` per line, `#` starts a comment:
//!
//! ```text
//! keyboard_layout = us
//! keyboard_variant = dvorak
//! terminal = dune-term  # or foot, alacritty, ...
//! modifier = alt          # alt, super or ctrl+alt
//! direction_right = super+Right   # direction indicator keys (also _left, _up, _down)
//! xwayland = satellite    # X11 apps through xwayland-satellite, or `off`
//! font_family = JetBrains Mono   # dune-term's font, also used for tab titles
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
//! Saving the file applies it live (see `reload`), except monitor modes, `off` and `xwayland`.
//!
//! Environment variables win over the file: `XKB_DEFAULT_LAYOUT`, `XKB_DEFAULT_VARIANT`,
//! `XKB_DEFAULT_OPTIONS`, `XKB_DEFAULT_MODEL`, `XKB_DEFAULT_RULES`, `DUNE_TERMINAL`, `DUNE_MOD`.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
};

use smithay::input::keyboard::{Keysym, ModifiersState, XkbConfig, xkb};

use crate::layout::Side;

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
        let key = xkb::keysym_from_name(name, xkb::KEYSYM_CASE_INSENSITIVE);
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
    pub terminal: String,
    pub modifier: String,
    /// Keys that point the focused tile's direction indicator at each side.
    pub direction_keys: Vec<(Side, KeySpec)>,
    /// `satellite` (default) or `off`.
    pub xwayland: String,
    /// Monitors by connector name, like `HDMI-A-1`.
    pub outputs: BTreeMap<String, OutputConfig>,
    pub input: InputConfig,
    /// As fontconfig names it; tab titles are written with it.
    pub font_family: String,
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
    resolve(&parse(&text), &|key| std::env::var(key).ok())
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
        terminal: get("DUNE_TERMINAL", "terminal", "dune-term"),
        // Nested inside another compositor Super is usually taken by the host.
        modifier: get("DUNE_MOD", "modifier", "alt"),
        xwayland: get("DUNE_XWAYLAND", "xwayland", "satellite"),
        direction_keys: [
            (Side::Left, "direction_left", "super+Left"),
            (Side::Right, "direction_right", "super+Right"),
            (Side::Up, "direction_up", "super+Up"),
            (Side::Down, "direction_down", "super+Down"),
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
        outputs: outputs(file),
        input: input(file),
        font_family: file.get("font_family").cloned().unwrap_or_else(|| "monospace".into()),
    }
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
        assert_eq!(key(Side::Right), KeySpec::parse("super+Right"));
        assert_eq!(key(Side::Up), None); // invalid: no binding rather than a wrong one
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
