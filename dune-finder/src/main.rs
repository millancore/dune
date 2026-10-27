//! dune-finder: the launcher (apps), window switcher (`/`), files (`#`), jumps (`>`) and
//! tile options of the dune compositor, as a panel of its own (a wlr-layer-shell surface)
//! that dune starts for Mod+d, Mod+/ (the tile options) and Mod+Enter. It asks dune for the windows and to start or show what was chosen over the
//! control socket (`dune_common::protocol`, `$DUNE_SOCKET`).
//!
//! `dune-finder [--mode apps|windows|files|jump|tile-options|system] [--output NAME] [--target TILE] [--slot TILE
//! --at X,Y,W,H] [--compact] [--font FAMILY] [--icon-theme THEME] [--file-manager CMD]
//! [--file-manager-terminal CMD] [--terminal CMD]` (the font, icon theme and file manager
//! come from `finder.toml`, see `settings`; the flags win over it): `--target` is
//! the tile apps open next to (and the tile options are for); `--slot` the empty container they go into, `--at` where it is
//! on the output; `--compact` shows no list until something is typed or browsed; `--file-manager` what Mod+Enter (or Ctrl+Enter) opens files in (default `yazi`),
//! `--file-manager-terminal` the terminal it runs in (default: dune's), `--terminal` dune's
//! terminal, whose app heads the apps before anything is typed.

mod app;
use dune_common::icons;
mod apps;
mod files;
mod finder;
mod jump;
mod panel;
mod settings;
mod system;
mod tabs;
mod tile_options;
mod windows;

use app::Options;
use finder::Mode;

fn main() {
    let options = match parse(settings::load(), std::env::args().skip(1)) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("dune-finder: {err}");
            std::process::exit(2);
        }
    };
    if let Err(err) = app::run(options) {
        eprintln!("dune-finder: {err}");
        std::process::exit(1);
    }
}

/// The command line over `options` (finder.toml's settings).
fn parse(mut options: Options, mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--mode" => {
                options.mode = match value()?.as_str() {
                    "apps" => Mode::Apps,
                    "windows" => Mode::Windows,
                    "files" => Mode::Files,
                    "jump" => Mode::Jump,
                    "tile-options" => Mode::TileOptions,
                    "system" => Mode::System,
                    other => return Err(format!("unknown mode `{other}`")),
                }
            }
            "--output" => options.output = Some(value()?),
            "--target" => options.target = value()?.parse().ok(),
            "--slot" => options.slot = value()?.parse().ok(),
            "--at" => options.at = parse_rect(&value()?),
            "--compact" => options.compact = true,
            "--font" => options.font = value()?,
            "--icon-theme" => options.icon_theme = value()?,
            "--file-manager" => options.file_manager = value()?,
            "--file-manager-terminal" => options.file_manager_terminal = Some(value()?),
            "--terminal" => options.terminal = value()?,
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(options)
}

/// `x,y,w,h`.
fn parse_rect(s: &str) -> Option<(i32, i32, i32, i32)> {
    let n: Vec<i32> = s
        .split(',')
        .map(|n| n.trim().parse().ok())
        .collect::<Option<_>>()?;
    match n[..] {
        [x, y, w, h] => Some((x, y, w, h)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> impl Iterator<Item = String> {
        s.split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn arguments() {
        let o = parse(Options::default(), args("--mode windows --output DP-1 --target 3 --slot 7 --at 10,20,300,400 --font Mono --file-manager lf")).unwrap();
        assert_eq!(o.mode, Mode::Windows);
        assert_eq!(o.output.as_deref(), Some("DP-1"));
        assert_eq!(
            (o.target, o.slot, o.at),
            (Some(3), Some(7), Some((10, 20, 300, 400)))
        );
        assert_eq!((o.font.as_str(), o.file_manager.as_str()), ("Mono", "lf"));
        assert_eq!(parse(Options::default(), args("")).unwrap().file_manager, "yazi");
        assert_eq!(
            parse(Options::default(), args("--file-manager-terminal foot"))
                .unwrap()
                .file_manager_terminal
                .as_deref(),
            Some("foot")
        );
        assert_eq!(parse(Options::default(), args("")).unwrap().file_manager_terminal, None);
        assert_eq!(parse(Options::default(), args("")).unwrap().mode, Mode::Apps);
        assert_eq!(parse(Options::default(), args("--mode jump")).unwrap().mode, Mode::Jump);
        assert_eq!(parse(Options::default(), args("--mode tile-options")).unwrap().mode, Mode::TileOptions);
        assert!(parse(Options::default(), args("--compact")).unwrap().compact);
        assert!(!parse(Options::default(), args("")).unwrap().compact);
        assert!(parse(Options::default(), args("--mode x")).is_err());
        assert!(parse(Options::default(), args("--slot")).is_err());
        assert!(parse(Options::default(), args("--bogus")).is_err());
        assert_eq!(parse_rect("1,2,3"), None);
    }
}
