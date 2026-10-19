//! Desktop applications (freedesktop `.desktop` entries): what `@` launches.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// Desktop file id: `firefox`, `org.gnome.Nautilus`.
    pub id: String,
    /// Display name, localized when possible: `Firefox`, `Files`.
    pub name: String,
    /// The `Exec` line, field codes still in it.
    pub exec: String,
}

impl App {
    /// The shell command to run the app with `args` (as typed, shell syntax).
    /// `%f %u` take the first argument, `%F %U` all of them; with neither, the
    /// arguments are appended. Other field codes are dropped.
    pub fn command(&self, args: &str) -> String {
        let args = args.trim();
        let first = args.split_whitespace().next().unwrap_or("");
        let mut out = String::new();
        let mut used = false;
        let mut chars = self.exec.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('%') => out.push('%'),
                Some('f' | 'u') => {
                    out.push_str(first);
                    used = true;
                }
                Some('F' | 'U') => {
                    out.push_str(args);
                    used = true;
                }
                _ => {} // %i %c %k and deprecated codes
            }
        }
        let mut command = out.split_whitespace().collect::<Vec<_>>().join(" ");
        if !used && !args.is_empty() {
            command.push(' ');
            command.push_str(args);
        }
        command
    }

    /// Name of the program the app runs: `firefox` for `/usr/lib/firefox/firefox %u`.
    fn program(&self) -> &str {
        // Skip an `env VAR=value ...` prefix.
        let first = self.exec.split_whitespace().find(|w| *w != "env" && !w.contains('=')).unwrap_or("");
        first.rsplit('/').next().unwrap_or(first).trim_matches('"')
    }

    /// How well `query` matches: by id, a word of the name, or the program; lower is better.
    pub fn rank(&self, query: &str) -> Option<u8> {
        let q = query.to_lowercase();
        if q.is_empty() {
            return None;
        }
        let id = self.id.to_lowercase();
        let short_id = id.rsplit('.').next().unwrap_or(&id); // org.gnome.Nautilus → nautilus
        if id.starts_with(&q) || short_id.starts_with(&q) {
            Some(0)
        } else if self.name.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(&q)) {
            Some(1)
        } else if self.program().to_lowercase().starts_with(&q) {
            Some(2)
        } else {
            None
        }
    }
}

/// Directories holding `applications/`, most important first (XDG base directories).
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

/// All visible desktop applications, sorted by name. An id found in an earlier
/// directory hides the same id in later ones.
pub fn apps() -> Vec<App> {
    let lang = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();
    let mut found: HashMap<String, Option<App>> = HashMap::new();
    for dir in data_dirs() {
        let apps_dir = dir.join("applications");
        let mut files = Vec::new();
        collect(&apps_dir, &mut files);
        for file in files {
            let Some(id) = desktop_id(&apps_dir, &file) else { continue };
            if found.contains_key(&id) {
                continue;
            }
            let entry = std::fs::read_to_string(&file).ok().and_then(|text| parse(&id, &text, &lang));
            found.insert(id, entry);
        }
    }
    let mut apps: Vec<App> = found.into_values().flatten().collect();
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
    apps
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "desktop") {
            out.push(path);
        }
    }
}

/// `applications/kde/org.kde.foo.desktop` → `kde-org.kde.foo`.
fn desktop_id(base: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(base).ok()?.to_str()?;
    Some(rel.trim_end_matches(".desktop").replace('/', "-"))
}

/// The `[Desktop Entry]` group of a desktop file, if it is a visible application.
/// `lang` like `es_ES.UTF-8` picks `Name[es_ES]` or `Name[es]` over `Name`.
fn parse(id: &str, text: &str, lang: &str) -> Option<App> {
    let lang = lang.split(['.', '@']).next().unwrap_or("");
    let short = lang.split('_').next().unwrap_or("");
    let mut in_entry = false;
    let mut fields: HashMap<&str, &str> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            fields.entry(key.trim()).or_insert(value.trim());
        }
    }
    let flag = |key: &str| fields.get(key).is_some_and(|v| *v == "true");
    if fields.get("Type").copied() != Some("Application") || flag("NoDisplay") || flag("Hidden") {
        return None;
    }
    let localized = |l: &str| (!l.is_empty()).then(|| fields.get(format!("Name[{l}]").as_str()).copied()).flatten();
    let name = localized(lang).or_else(|| localized(short)).or_else(|| fields.get("Name").copied())?;
    let exec = fields.get("Exec").copied().filter(|e| !e.is_empty())?;
    Some(App { id: id.to_string(), name: name.to_string(), exec: exec.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX: &str = "[Desktop Entry]\nType=Application\nName=Firefox\nName[es]=Navegador Firefox\n\
        Exec=/usr/lib/firefox/firefox %u\n\n[Desktop Action new-window]\nName=New Window\n\
        Exec=/usr/lib/firefox/firefox --new-window %u\n";

    fn app(id: &str, name: &str, exec: &str) -> App {
        App { id: id.into(), name: name.into(), exec: exec.into() }
    }

    #[test]
    fn parses_the_main_group_only() {
        let a = parse("firefox", FIREFOX, "en_US.UTF-8").unwrap();
        assert_eq!(a, app("firefox", "Firefox", "/usr/lib/firefox/firefox %u"));
        assert_eq!(parse("firefox", FIREFOX, "es_ES.UTF-8").unwrap().name, "Navegador Firefox");
    }

    #[test]
    fn skips_hidden_and_non_applications() {
        assert!(parse("x", "[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\n", "").is_none());
        assert!(parse("x", "[Desktop Entry]\nType=Link\nName=X\nURL=http://x\n", "").is_none());
        assert!(parse("x", "[Desktop Entry]\nType=Application\nName=X\n", "").is_none());
    }

    #[test]
    fn field_codes() {
        let ff = app("firefox", "Firefox", "/usr/lib/firefox/firefox %u");
        assert_eq!(ff.command(""), "/usr/lib/firefox/firefox");
        assert_eq!(ff.command("https://a https://b"), "/usr/lib/firefox/firefox https://a");
        let gimp = app("gimp", "GIMP", "gimp-2.10 %U");
        assert_eq!(gimp.command("a.png b.png"), "gimp-2.10 a.png b.png");
        let plain = app("htop", "Htop", "htop");
        assert_eq!(plain.command("-d 10"), "htop -d 10");
        let icon = app("x", "X", "x --icon %i --name %c %% %f");
        assert_eq!(icon.command("f.txt"), "x --icon --name % f.txt");
    }

    #[test]
    fn matching() {
        let files = app("org.gnome.Nautilus", "Files", "nautilus --new-window %U");
        assert_eq!(files.rank("naut"), Some(0)); // last part of the id
        assert_eq!(files.rank("fil"), Some(1)); // the name
        let code = app("code", "Visual Studio Code", "/usr/bin/code --unity-launch %F");
        assert_eq!(code.rank("stu"), Some(1)); // any word of the name
        assert_eq!(code.rank("vsc"), None);
        let env = app("thing", "Thing", "env FOO=1 /opt/thing/bin/thingy");
        assert_eq!(env.rank("thingy"), Some(2)); // the program, past `env FOO=1`
        assert_eq!(env.rank("hing"), None);
    }

    #[test]
    fn desktop_ids() {
        let base = Path::new("/usr/share/applications");
        assert_eq!(desktop_id(base, &base.join("firefox.desktop")).unwrap(), "firefox");
        assert_eq!(desktop_id(base, &base.join("kde/org.kde.foo.desktop")).unwrap(), "kde-org.kde.foo");
    }
}

