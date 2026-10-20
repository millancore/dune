//! Application icons for dune-finder: looked up in the freedesktop icon theme
//! (with the themes it inherits, then hicolor and `/usr/share/pixmaps`) and drawn
//! with resvg, SVG or PNG, to a square of the size asked.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use resvg::{tiny_skia, usvg};

/// An icon drawn to `size` × `size` pixels, RGBA premultiplied by alpha.
pub struct Icon {
    pub size: u32,
    pub pixels: Vec<u8>,
}

/// A directory of a theme and the icon sizes it holds (`[48x48/apps]` in `index.theme`).
#[derive(Debug, Clone, PartialEq)]
struct Dir {
    path: String,
    size: u32,
    min: u32,
    max: u32,
    scalable: bool,
}

impl Dir {
    /// How far its icons are from `size` pixels: 0 when they fit it.
    fn distance(&self, size: u32) -> u32 {
        self.min.saturating_sub(size).max(size.saturating_sub(self.max))
    }
}

/// A theme's folders (one per base directory that has it) and its directories.
struct Theme {
    roots: Vec<PathBuf>,
    dirs: Vec<Dir>,
}

pub struct Icons {
    /// The theme, then the ones it inherits, hicolor last.
    themes: Vec<Theme>,
    pixmaps: Vec<PathBuf>,
    /// Drawn icons by `Icon=` value and size; None when there is none.
    cache: HashMap<(String, u32), Option<Icon>>,
}

impl Icons {
    /// Icons of `theme` (empty: the desktop's icon theme, as GTK apps use).
    pub fn new(theme: &str) -> Self {
        let bases = base_dirs();
        let theme = Some(theme.to_string()).filter(|t| !t.is_empty()).or_else(desktop_theme).unwrap_or_else(|| "hicolor".into());
        let mut names = vec![theme];
        let mut themes = Vec::new();
        let mut i = 0;
        while i < names.len() {
            let name = names[i].clone();
            i += 1;
            let roots: Vec<PathBuf> = bases.iter().map(|b| b.join(&name)).filter(|r| r.is_dir()).collect();
            let Some(index) = roots.iter().find_map(|r| std::fs::read_to_string(r.join("index.theme")).ok()) else { continue };
            let (dirs, inherits) = parse_index(&index);
            for parent in inherits {
                if !names.contains(&parent) {
                    names.push(parent);
                }
            }
            themes.push(Theme { roots, dirs });
            if i == names.len() && !names.iter().any(|n| n == "hicolor") {
                names.push("hicolor".into());
            }
        }
        let pixmaps = data_dirs().into_iter().map(|d| d.join("pixmaps")).collect();
        Icons { themes, pixmaps, cache: HashMap::new() }
    }

    /// The icon named `name` (an `Icon=` value) at `size` pixels, if there is one.
    pub fn get(&mut self, name: &str, size: u32) -> Option<&Icon> {
        let key = (name.to_string(), size);
        if !self.cache.contains_key(&key) {
            let icon = self.find(name, size).and_then(|file| draw(&file, size));
            self.cache.insert(key.clone(), icon);
        }
        self.cache.get(&key)?.as_ref()
    }

    /// The icon `get` drew for `name` at `size`, without drawing it now.
    pub fn cached(&self, name: &str, size: u32) -> Option<&Icon> {
        self.cache.get(&(name.to_string(), size))?.as_ref()
    }

    fn find(&self, name: &str, size: u32) -> Option<PathBuf> {
        if name.starts_with('/') {
            return Some(PathBuf::from(name)).filter(|p| p.is_file());
        }
        for theme in &self.themes {
            let mut best: Option<(u32, PathBuf)> = None;
            for dir in &theme.dirs {
                let distance = dir.distance(size);
                if best.as_ref().is_some_and(|(d, _)| *d <= distance) {
                    continue;
                }
                if let Some(file) = theme.roots.iter().find_map(|root| icon_file(&root.join(&dir.path), name)) {
                    best = Some((distance, file));
                    if distance == 0 {
                        break;
                    }
                }
            }
            if let Some((_, file)) = best {
                return Some(file);
            }
        }
        self.pixmaps.iter().find_map(|dir| icon_file(dir, name))
    }
}

/// `name.png` or `name.svg` in `dir`.
fn icon_file(dir: &Path, name: &str) -> Option<PathBuf> {
    ["png", "svg"].into_iter().map(|ext| dir.join(format!("{name}.{ext}"))).find(|f| f.is_file())
}

/// The directories with icons of an `index.theme`, and the themes it inherits.
fn parse_index(text: &str) -> (Vec<Dir>, Vec<String>) {
    let mut groups: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut group = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            group = name.to_string();
        } else if let Some((key, value)) = line.split_once('=') {
            groups.entry(group.clone()).or_default().insert(key.trim().into(), value.trim().into());
        }
    }
    let theme = groups.get("Icon Theme").cloned().unwrap_or_default();
    let list = |key: &str| theme.get(key).map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>()).unwrap_or_default();
    let mut dirs: Vec<Dir> = list("Directories")
        .into_iter()
        .chain(list("ScaledDirectories"))
        .filter_map(|path| {
            let keys = groups.get(&path)?;
            let num = |key: &str| keys.get(key).and_then(|v| v.parse::<u32>().ok());
            // Icons for a bigger scale are drawn at the size times the scale.
            let size = num("Size")? * num("Scale").unwrap_or(1);
            let (min, max) = match keys.get("Type").map(String::as_str) {
                Some("Fixed") => (size, size),
                Some("Scalable") => (num("MinSize").unwrap_or(size), num("MaxSize").unwrap_or(size)),
                _ => {
                    let threshold = num("Threshold").unwrap_or(2);
                    (size.saturating_sub(threshold), size + threshold)
                }
            };
            Some(Dir { scalable: keys.get("Type").is_some_and(|t| t == "Scalable"), path, size, min, max })
        })
        .collect();
    // Fixed sizes first, so that an exact one wins over a scalable icon.
    dirs.sort_by_key(|d| (d.scalable, d.size));
    (dirs, list("Inherits"))
}

/// Draw the PNG or SVG `file` to fit `size` × `size`, centered.
fn draw(file: &Path, size: u32) -> Option<Icon> {
    let data = std::fs::read(file).ok()?;
    let mut canvas = tiny_skia::Pixmap::new(size, size)?;
    if file.extension().is_some_and(|e| e == "svg") {
        let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).ok()?;
        let (w, h) = (tree.size().width(), tree.size().height());
        let scale = size as f32 / w.max(h);
        let offset = ((size as f32 - w * scale) / 2.0, (size as f32 - h * scale) / 2.0);
        let transform = tiny_skia::Transform::from_scale(scale, scale).post_translate(offset.0, offset.1);
        resvg::render(&tree, transform, &mut canvas.as_mut());
    } else {
        let image = tiny_skia::Pixmap::decode_png(&data).ok()?;
        let (w, h) = (image.width() as f32, image.height() as f32);
        let scale = size as f32 / w.max(h);
        let offset = ((size as f32 - w * scale) / 2.0, (size as f32 - h * scale) / 2.0);
        let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..Default::default() };
        let transform = tiny_skia::Transform::from_scale(scale, scale).post_translate(offset.0, offset.1);
        canvas.draw_pixmap(0, 0, image.as_ref(), &paint, transform, None);
    }
    Some(Icon { size, pixels: canvas.take() })
}

/// Directories with application data, most important first (XDG base directories).
fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local/share")));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    data_home.into_iter().chain(data_dirs.split(':').map(PathBuf::from)).collect()
}

/// Where icon themes live: `~/.icons`, then `icons/` in each data directory.
fn base_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".icons"));
    home.into_iter().chain(data_dirs().into_iter().map(|d| d.join("icons"))).collect()
}

/// The icon theme the desktop set for GTK apps: GNOME's setting, else GTK's settings.ini.
fn desktop_theme() -> Option<String> {
    let gsettings = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().trim_matches('\'').to_string())
        .filter(|t| !t.is_empty());
    gsettings.or_else(|| {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        ["gtk-4.0", "gtk-3.0"].into_iter().find_map(|gtk| {
            let text = std::fs::read_to_string(config.join(gtk).join("settings.ini")).ok()?;
            text.lines().find_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "gtk-icon-theme-name").then(|| value.trim().trim_matches('"').to_string())
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = "[Icon Theme]\nName=Test\nInherits=Parent,hicolor\n\
        Directories=48x48/apps,16x16/apps,scalable/apps,24x24@2x/apps\n\n\
        [48x48/apps]\nSize=48\nType=Fixed\n\n[16x16/apps]\nSize=16\n\n\
        [scalable/apps]\nSize=64\nMinSize=8\nMaxSize=512\nType=Scalable\n\n\
        [24x24@2x/apps]\nSize=24\nScale=2\nType=Fixed\n";

    #[test]
    fn reads_index_theme() {
        let (dirs, inherits) = parse_index(INDEX);
        // Fixed sizes first, smallest first, then the scalable ones.
        let paths: Vec<&str> = dirs.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, ["16x16/apps", "48x48/apps", "24x24@2x/apps", "scalable/apps"]);
        assert_eq!((dirs[0].min, dirs[0].max), (14, 18)); // Threshold 2 by default
        assert_eq!(dirs[2].size, 48); // 24 at scale 2
        assert_eq!((dirs[3].min, dirs[3].max), (8, 512));
        assert_eq!(inherits, ["Parent", "hicolor"]);
    }

    #[test]
    fn distances() {
        let fixed = Dir { path: "48".into(), size: 48, min: 48, max: 48, scalable: false };
        assert_eq!((fixed.distance(48), fixed.distance(32), fixed.distance(64)), (0, 16, 16));
        let threshold = Dir { path: "16".into(), size: 16, min: 14, max: 18, scalable: false };
        assert_eq!((threshold.distance(17), threshold.distance(24)), (0, 6));
    }

    #[test]
    fn finds_and_draws_icons() {
        let root = std::env::temp_dir().join(format!("dune-icons-{}", std::process::id()));
        let theme = root.join("Test");
        std::fs::create_dir_all(theme.join("16x16/apps")).unwrap();
        std::fs::create_dir_all(theme.join("scalable/apps")).unwrap();
        std::fs::write(theme.join("index.theme"), INDEX).unwrap();
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="red"/></svg>"#;
        std::fs::write(theme.join("scalable/apps/app.svg"), svg).unwrap();
        std::fs::write(theme.join("16x16/apps/app.svg"), svg).unwrap();
        let (dirs, _) = parse_index(INDEX);
        let icons = Icons { themes: vec![Theme { roots: vec![theme.clone()], dirs }], pixmaps: vec![], cache: HashMap::new() };
        // The exact size wins; else the closest.
        assert_eq!(icons.find("app", 16).unwrap(), theme.join("16x16/apps/app.svg"));
        assert_eq!(icons.find("app", 32).unwrap(), theme.join("scalable/apps/app.svg"));
        assert!(icons.find("missing", 32).is_none());

        let icon = draw(&theme.join("scalable/apps/app.svg"), 24).unwrap();
        assert_eq!((icon.size, icon.pixels.len()), (24, 24 * 24 * 4));
        assert_eq!(&icon.pixels[..4], &[255, 0, 0, 255]);
        std::fs::remove_dir_all(&root).unwrap();
    }

}
