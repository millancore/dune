//! dune-finder's settings, from `~/.config/dune/finder.toml` (next to dune's `dune.toml`),
//! read each time it starts, so a change shows the next time it opens. Every setting is
//! optional:
//!
//! ```toml
//! font = "JetBrains Mono"        # default monospace
//! icon_theme = "Papirus-Dark"    # default: the desktop's (gsettings / GTK)
//! file_manager = "yazi"          # terminal file manager Mod+Enter opens a file or folder in; default yazi
//! file_manager_terminal = "foot" # terminal it runs in (foot shows yazi's image previews); default dune's
//! ```
//!
//! A setting it does not know is warned about; a file that is not TOML, or a value of the
//! wrong type, is reported with its line and the defaults are used.

use serde::Deserialize;

use crate::app::Options;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct File {
    font: Option<String>,
    icon_theme: Option<String>,
    file_manager: Option<String>,
    file_manager_terminal: Option<String>,
}

/// The default options with the settings of `finder.toml` in them.
pub fn load() -> Options {
    let Some(path) = dune_common::config_dir().map(|d| d.join("finder.toml")) else { return Options::default() };
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    match parse(&text) {
        Ok(file) => apply(file),
        Err(err) => {
            eprintln!("dune-finder: {}: {err}", path.display());
            Options::default()
        }
    }
}

fn parse(text: &str) -> Result<File, toml::de::Error> {
    let deserializer = toml::Deserializer::parse(text)?;
    serde_ignored::deserialize(deserializer, |setting| {
        eprintln!("dune-finder: ignoring `{setting}` in finder.toml: no setting by that name");
    })
}

fn apply(file: File) -> Options {
    let mut options = Options::default();
    let set = |value: Option<String>| value.filter(|v| !v.trim().is_empty());
    if let Some(font) = set(file.font) {
        options.font = font;
    }
    if let Some(theme) = set(file.icon_theme) {
        options.icon_theme = theme;
    }
    if let Some(file_manager) = set(file.file_manager) {
        options.file_manager = file_manager;
    }
    options.file_manager_terminal = set(file.file_manager_terminal);
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_and_defaults() {
        let o = apply(parse("font = \"Mono\"\nfile_manager_terminal = \"foot\"\n").unwrap());
        assert_eq!((o.font.as_str(), o.file_manager.as_str()), ("Mono", "yazi"));
        assert_eq!(o.file_manager_terminal.as_deref(), Some("foot"));
        let o = apply(parse("").unwrap());
        assert_eq!((o.font.as_str(), o.icon_theme.as_str(), o.file_manager_terminal), ("monospace", "", None));
        // Empty: the default.
        assert_eq!(apply(parse("file_manager = \"\"\n").unwrap()).file_manager, "yazi");
    }

    #[test]
    fn not_toml_is_an_error_with_its_line() {
        assert!(parse("font = Mono\n").unwrap_err().to_string().contains("line 1"));
        assert!(parse("font = 3\n").is_err());
        assert!(parse("bogus = 1\n").is_ok());
    }
}
