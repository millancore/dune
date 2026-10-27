//! Code shared by the dune compositor, its shell wrapper and dune-finder.

pub mod at;
pub mod desktop;
pub mod font;
pub mod icons;
pub mod jumps;
pub mod picker;
pub mod protocol;

/// `~/.config/dune` (or `$XDG_CONFIG_HOME/dune`): `dune.toml` and `finder.toml`.
pub fn config_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("dune"))
}
