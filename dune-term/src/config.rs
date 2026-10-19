//! dune-term settings, read from dune's config file (`~/.config/dune/config`):
//!
//! ```text
//! font_family = JetBrains Mono
//! font_size = 17          # points
//! ```

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub font_family: String,
    /// In points.
    pub font_size: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            font_family: "monospace".into(),
            font_size: 12.0,
        }
    }
}

pub fn load() -> Config {
    let path = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|base| base.join("dune").join("config"));
    let text = path.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    parse(&text)
}

fn parse(text: &str) -> Config {
    let mut config = Config::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim();
        match key.trim() {
            "font_family" if !value.is_empty() => config.font_family = value.to_string(),
            "font_size" => {
                if let Ok(size) = value.parse::<f32>() {
                    config.font_size = size.clamp(4.0, 96.0);
                }
            }
            _ => {}
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_font_settings_and_ignores_the_rest() {
        let c = parse("keyboard_variant = dvorak\nfont_family = JetBrains Mono  # nice\nfont_size = 17\n");
        assert_eq!(c, Config { font_family: "JetBrains Mono".into(), font_size: 17.0 });
        assert_eq!(parse("font_size = huge\n"), Config::default());
        assert_eq!(parse("font_size = 500\n").font_size, 96.0);
    }
}
