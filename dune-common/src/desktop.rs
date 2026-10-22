//! Desktop applications (freedesktop `.desktop` entries): what the launcher lists.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct App {
    /// Desktop file id: `firefox`, `org.gnome.Nautilus`.
    pub id: String,
    /// Display name, localized when possible: `Firefox`, `Files`.
    pub name: String,
    /// The `Exec` line, field codes still in it.
    pub exec: String,
    /// `Terminal=true`: a console program, it needs a terminal to show.
    pub terminal: bool,
    /// The `Icon` key: a theme icon name (`firefox`) or an absolute path.
    pub icon: Option<String>,
    /// What kind of app it is, localized: `Web Browser`.
    pub generic_name: Option<String>,
    /// Search terms the app gives besides its name, localized and lowercase.
    pub keywords: Vec<String>,
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
    pub fn program(&self) -> &str {
        // Skip an `env VAR=value ...` prefix.
        let first = self.exec.split_whitespace().find(|w| *w != "env" && !w.contains('=')).unwrap_or("");
        first.rsplit('/').next().unwrap_or(first).trim_matches('"')
    }

    /// How well `query` matches, lower is better, like fuzzel: the name starting with it,
    /// a word of the name starting with it, the name containing it, the name containing
    /// its letters in order (`ff` → Firefox), then the id, generic name, keywords or program.
    pub fn rank(&self, query: &str) -> Option<u8> {
        let q = query.to_lowercase();
        if q.is_empty() {
            return None;
        }
        let name = self.name.to_lowercase();
        let id = self.id.to_lowercase();
        let short_id = id.rsplit('.').next().unwrap_or(&id); // org.gnome.Nautilus → nautilus
        let words = |text: &str| text.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(&q));
        if name.starts_with(&q) {
            Some(0)
        } else if words(&name) {
            Some(1)
        } else if name.contains(&q) {
            Some(2)
        } else if subsequence(&q, &name) {
            Some(3)
        } else if id.starts_with(&q)
            || short_id.starts_with(&q)
            || self.generic_name.as_deref().is_some_and(|g| words(&g.to_lowercase()))
            || self.keywords.iter().any(|k| k.starts_with(&q))
            || self.program().to_lowercase().starts_with(&q)
        {
            Some(4)
        } else {
            None
        }
    }
}

/// Whether the letters of `needle` appear in `haystack` in order.
fn subsequence(needle: &str, haystack: &str) -> bool {
    let mut rest = haystack.chars();
    needle.chars().all(|c| rest.any(|h| h == c))
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
    let desktops: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(str::to_lowercase)
        .collect();
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
            let entry = std::fs::read_to_string(&file).ok().and_then(|text| parse(&id, &text, &lang, &desktops));
            found.insert(id, entry);
        }
    }
    let mut apps: Vec<App> = found.into_values().flatten().filter(App::installed).collect();
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
    // The same app installed twice (a package and a copy in ~/.local) shows once.
    apps.dedup_by(|a, b| a.name == b.name && a.exec == b.exec);
    apps
}

impl App {
    /// Whether the program it runs is there: an absolute path that exists, else assumed
    /// (a name is looked up in PATH when it runs). Stale entries of removed AppImages go.
    fn installed(&self) -> bool {
        let first = self.exec.split_whitespace().find(|w| *w != "env" && !w.contains('=')).unwrap_or("");
        let first = first.trim_matches('"');
        !first.starts_with('/') || Path::new(first).exists()
    }
}

/// Whether `program` (a `TryExec` value) can run: an existing path, or a file in PATH.
fn on_path(program: &str) -> bool {
    if program.contains('/') {
        return Path::new(program).exists();
    }
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// Where launches are counted, like fuzzel's cache: `$XDG_CACHE_HOME/dune/launcher`.
fn launches_file() -> Option<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(cache.join("dune").join("launcher"))
}

/// Times each app (by id) was launched from the launcher.
pub fn launches() -> HashMap<String, u32> {
    launches_file().and_then(|f| std::fs::read_to_string(f).ok()).map_or_else(HashMap::new, |t| parse_launches(&t))
}

/// Count one more launch of app `id`.
pub fn record_launch(id: &str) -> std::io::Result<()> {
    let Some(file) = launches_file() else { return Ok(()) };
    let mut counts = launches();
    *counts.entry(id.to_string()).or_insert(0) += 1;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&file, format_launches(&counts))
}

/// `count id` per line (ids may contain spaces, counts cannot).
fn parse_launches(text: &str) -> HashMap<String, u32> {
    text.lines()
        .filter_map(|line| {
            let (count, id) = line.split_once(' ')?;
            Some((id.to_string(), count.parse().ok()?))
        })
        .collect()
}

fn format_launches(counts: &HashMap<String, u32>) -> String {
    let mut lines: Vec<_> = counts.iter().map(|(id, n)| format!("{n} {id}\n")).collect();
    lines.sort();
    lines.concat()
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

/// The `[Desktop Entry]` group of a desktop file, if it is an application meant to
/// show here. `lang` like `es_ES.UTF-8` picks `Name[es_ES]` or `Name[es]` over `Name`;
/// `desktops` (lowercase `XDG_CURRENT_DESKTOP` names) decide `OnlyShowIn` / `NotShowIn`.
fn parse(id: &str, text: &str, lang: &str, desktops: &[String]) -> Option<App> {
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
    let list = |key: &str| fields.get(key).map(|v| v.split(';').filter(|d| !d.is_empty()).map(str::to_lowercase).collect::<Vec<_>>());
    let here = |list: Vec<String>| list.iter().any(|d| desktops.contains(d));
    if list("OnlyShowIn").is_some_and(|l| !here(l)) || list("NotShowIn").is_some_and(here) {
        return None;
    }
    if fields.get("TryExec").is_some_and(|program| !on_path(program)) {
        return None;
    }
    let localized = |key: &str| {
        let get = |l: &str| (!l.is_empty()).then(|| fields.get(format!("{key}[{l}]").as_str()).copied()).flatten();
        get(lang).or_else(|| get(short)).or_else(|| fields.get(key).copied()).filter(|v| !v.is_empty())
    };
    let name = localized("Name")?;
    let exec = fields.get("Exec").copied().filter(|e| !e.is_empty())?;
    Some(App {
        id: id.to_string(),
        name: name.to_string(),
        exec: exec.to_string(),
        terminal: flag("Terminal"),
        icon: fields.get("Icon").filter(|i| !i.is_empty()).map(|i| i.to_string()),
        generic_name: localized("GenericName").map(str::to_string),
        keywords: localized("Keywords")
            .map(|k| k.split(';').filter(|w| !w.is_empty()).map(str::to_lowercase).collect())
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX: &str = "[Desktop Entry]\nType=Application\nName=Firefox\nName[es]=Navegador Firefox\nIcon=firefox\n\
        Exec=/usr/lib/firefox/firefox %u\n\n[Desktop Action new-window]\nName=New Window\n\
        Exec=/usr/lib/firefox/firefox --new-window %u\n";

    fn app(id: &str, name: &str, exec: &str) -> App {
        App { id: id.into(), name: name.into(), exec: exec.into(), ..Default::default() }
    }

    #[test]
    fn parses_the_main_group_only() {
        let a = parse("firefox", FIREFOX, "en_US.UTF-8", &[]).unwrap();
        assert_eq!(a, App { icon: Some("firefox".into()), ..app("firefox", "Firefox", "/usr/lib/firefox/firefox %u") });
        let es = parse("firefox", FIREFOX, "es_ES.UTF-8", &[]).unwrap();
        assert_eq!(es.name, "Navegador Firefox");
        let htop = parse("htop", "[Desktop Entry]\nType=Application\nName=Htop\nExec=htop\nTerminal=true\n", "", &[]);
        assert!(htop.unwrap().terminal);
        let files = "[Desktop Entry]\nType=Application\nName=Files\nExec=nautilus\nGenericName=File Manager\n\
            GenericName[es]=Gestor de archivos\nKeywords=folder;explorer;\nKeywords[es]=carpeta;\n";
        let en = parse("nautilus", files, "", &[]).unwrap();
        assert_eq!((en.generic_name.as_deref(), en.keywords.clone()), (Some("File Manager"), vec!["folder".into(), "explorer".into()]));
        let es = parse("nautilus", files, "es_ES.UTF-8", &[]).unwrap();
        assert_eq!((es.generic_name.as_deref(), es.keywords), (Some("Gestor de archivos"), vec!["carpeta".into()]));
    }

    #[test]
    fn skips_hidden_and_non_applications() {
        assert!(parse("x", "[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\n", "", &[]).is_none());
        assert!(parse("x", "[Desktop Entry]\nType=Link\nName=X\nURL=http://x\n", "", &[]).is_none());
        assert!(parse("x", "[Desktop Entry]\nType=Application\nName=X\n", "", &[]).is_none());
    }

    #[test]
    fn other_desktops_and_missing_programs() {
        let dune = ["dune".to_string()];
        let entry = |extra: &str| format!("[Desktop Entry]\nType=Application\nName=X\nExec=x\n{extra}\n");
        assert!(parse("x", &entry("OnlyShowIn=GNOME;XFCE;"), "", &dune).is_none());
        assert!(parse("x", &entry("OnlyShowIn=GNOME;Dune;"), "", &dune).is_some());
        assert!(parse("x", &entry("NotShowIn=KDE;"), "", &dune).is_some());
        assert!(parse("x", &entry("NotShowIn=dune;"), "", &dune).is_none());
        assert!(parse("x", &entry("TryExec=sh"), "", &dune).is_some());
        assert!(parse("x", &entry("TryExec=/nonexistent/dune-test"), "", &dune).is_none());
        assert!(parse("x", &entry("TryExec=dune-no-such-program"), "", &dune).is_none());
        assert!(!app("x", "X", "/nonexistent/X.AppImage %U").installed());
        assert!(app("x", "X", "env A=1 sh -c true").installed());
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
        let files = App {
            generic_name: Some("File Manager".into()),
            keywords: vec!["folder".into()],
            ..app("org.gnome.Nautilus", "Files", "nautilus --new-window %U")
        };
        assert_eq!(files.rank("fil"), Some(0)); // the name
        assert_eq!(files.rank("ile"), Some(2)); // inside the name
        assert_eq!(files.rank("fs"), Some(3)); // its letters in order
        assert_eq!(files.rank("naut"), Some(4)); // last part of the id
        assert_eq!(files.rank("manager"), Some(4)); // the generic name
        assert_eq!(files.rank("fold"), Some(4)); // a keyword
        let code = app("code", "Visual Studio Code", "/usr/bin/code --unity-launch %F");
        assert_eq!(code.rank("stu"), Some(1)); // any word of the name
        assert_eq!(code.rank("vsc"), Some(3));
        assert_eq!(code.rank("xyz"), None);
        let env = app("thing", "Thing", "env FOO=1 /opt/thing/bin/thingy");
        assert_eq!(env.rank("thingy"), Some(4)); // the program, past `env FOO=1`
        assert_eq!(app("firefox", "Firefox", "firefox").rank("ff"), Some(3));
    }

    #[test]
    fn launch_counts() {
        let counts = parse_launches("3 firefox\n1 Crusader Kings II\nbad\nx y\n");
        assert_eq!(counts, HashMap::from([("firefox".into(), 3), ("Crusader Kings II".into(), 1)]));
        assert_eq!(parse_launches(&format_launches(&counts)), counts);
    }

    #[test]
    fn desktop_ids() {
        let base = Path::new("/usr/share/applications");
        assert_eq!(desktop_id(base, &base.join("firefox.desktop")).unwrap(), "firefox");
        assert_eq!(desktop_id(base, &base.join("kde/org.kde.foo.desktop")).unwrap(), "kde-org.kde.foo");
    }
}

