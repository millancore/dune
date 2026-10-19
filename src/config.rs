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
//! ```
//!
//! Environment variables win over the file: `XKB_DEFAULT_LAYOUT`, `XKB_DEFAULT_VARIANT`,
//! `XKB_DEFAULT_OPTIONS`, `XKB_DEFAULT_MODEL`, `XKB_DEFAULT_RULES`, `DUNE_TERMINAL`, `DUNE_MOD`.

use std::{collections::HashMap, path::PathBuf};

use smithay::input::keyboard::{Keysym, ModifiersState, xkb};

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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    }
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
