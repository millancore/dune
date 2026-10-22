//! The launcher's line editor.
//!
//! Pure logic: it turns keys into an edited line plus a list of suggestions; the
//! launcher of dune-finder draws them.

use std::{
    cmp::Reverse,
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::desktop::App;

/// Suggestions shown at once; the list scrolls to keep the selection visible.
pub const MAX_ROWS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,
    Enter,
    Tab,
    BackTab,
    Up,
    Down,
    Esc,
    CtrlC,
    CtrlU,
    CtrlW,
    Other,
}

/// Decode raw terminal input into keys.
pub fn parse_keys(bytes: &[u8]) -> Vec<Key> {
    let text = String::from_utf8_lossy(bytes);
    let mut chars = text.chars().peekable();
    let mut keys = Vec::new();
    while let Some(c) = chars.next() {
        keys.push(match c {
            '\x1b' => match chars.peek() {
                None => Key::Esc,
                Some('[') | Some('O') => {
                    chars.next();
                    // Parameters, then a final byte in @..~
                    let mut fin = None;
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            fin = Some(c);
                            break;
                        }
                    }
                    match fin {
                        Some('A') => Key::Up,
                        Some('B') => Key::Down,
                        Some('Z') => Key::BackTab,
                        _ => Key::Other,
                    }
                }
                Some(_) => {
                    chars.next(); // Alt+key
                    Key::Other
                }
            },
            '\r' | '\n' => Key::Enter,
            '\t' => Key::Tab,
            '\x7f' | '\x08' => Key::Backspace,
            '\x03' => Key::CtrlC,
            '\x15' => Key::CtrlU,
            '\x17' => Key::CtrlW,
            c if c >= ' ' => Key::Char(c),
            _ => Key::Other,
        });
    }
    keys
}

/// A finished line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Replaces the token being completed.
    pub insert: String,
    pub label: String,
    /// The app's icon (`App::icon`), for suggestions that are apps.
    pub icon: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    Cancel,
    Run(Line),
}

pub struct Picker {
    input: String,
    selected: usize,
    candidates: Vec<Candidate>,
    apps: Vec<App>,
    cwd: PathBuf,
    /// With nothing typed, list every app (dune-finder's launcher) instead of nothing.
    browse: bool,
    /// Times each app (by id) was launched: the most used come first.
    uses: HashMap<String, u32>,
    /// The app the last Enter ran, if it ran one.
    chosen: Option<String>,
    /// Browsing with nothing typed, this app (by id) heads the list: the user's terminal.
    first: Option<String>,
}

impl Picker {
    /// A picker over desktop applications (`crate::desktop::apps()`).
    pub fn new(apps: Vec<App>, cwd: PathBuf) -> Self {
        Picker {
            input: String::new(),
            selected: 0,
            candidates: Vec::new(),
            apps,
            cwd,
            browse: false,
            uses: HashMap::new(),
            chosen: None,
            first: None,
        }
    }

    /// Like `new`, listing every app before anything is typed, the most used first
    /// (`uses`: launches by app id, see `crate::desktop::launches`).
    pub fn browser(apps: Vec<App>, cwd: PathBuf, uses: HashMap<String, u32>) -> Self {
        let mut picker = Picker { browse: true, uses, ..Picker::new(apps, cwd) };
        picker.refresh();
        picker
    }

    /// Put the app `id` at the head of the list shown with nothing typed.
    pub fn pin_first(&mut self, id: &str) {
        self.first = Some(id.to_string());
        self.refresh();
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    #[cfg(test)]
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Highlight suggestion `i` (one clicked); out of the list it changes nothing.
    pub fn select(&mut self, i: usize) {
        if i < self.candidates.len() {
            self.selected = i;
        }
    }

    /// Id of the app the last Enter ran; None when it ran a typed command.
    pub fn chosen(&self) -> Option<&str> {
        self.chosen.as_deref()
    }

    pub fn feed(&mut self, key: Key) -> Outcome {
        match key {
            Key::Char(c) => self.edit(|input| input.push(c)),
            Key::Backspace if self.input.is_empty() && !self.browse => return Outcome::Cancel,
            Key::Backspace => self.edit(|input| {
                input.pop();
            }),
            Key::CtrlU => self.edit(String::clear),
            Key::CtrlW => self.edit(|input| {
                let trimmed = input.trim_end().len();
                let start = input[..trimmed].rfind(char::is_whitespace).map_or(0, |i| i + 1);
                input.truncate(start);
            }),
            Key::Esc | Key::CtrlC => return Outcome::Cancel,
            Key::Down => self.step(1),
            Key::Up | Key::BackTab => self.step(-1),
            Key::Tab => self.apart(|p| p.accept_selection(true)),
            Key::Enter => {
                let (line, chosen) = self.apart(|p| {
                    let (line, app) = p.resolve();
                    (line, app.map(|a| a.id.clone()))
                });
                self.chosen = chosen;
                return if line.command.is_empty() {
                    Outcome::Cancel
                } else {
                    Outcome::Run(line)
                };
            }
            Key::Other => {}
        }
        Outcome::Continue
    }

    /// Browsing (dune-finder's launcher), a last word starting with `:` says where
    /// the app opens: `firefox :d2`. It is not part of the command.
    pub fn dest(&self) -> Option<&str> {
        self.dest_start().map(|i| self.input[i..].trim_start())
    }

    /// Where the destination word starts, with the spaces before it.
    fn dest_start(&self) -> Option<usize> {
        if !self.browse {
            return None;
        }
        let word = self.input.rfind(' ').map_or(0, |i| i + 1);
        let start = self.input[..word].trim_end().len();
        self.input[word..].starts_with(':').then_some(start)
    }

    /// Run `f` with the destination word put aside, so that completion and the command
    /// only see what comes before it.
    fn apart<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let Some(start) = self.dest_start() else { return f(self) };
        let dest = self.input.split_off(start);
        let out = f(self);
        self.input.push_str(&dest);
        out
    }

    fn edit(&mut self, f: impl FnOnce(&mut String)) {
        f(&mut self.input);
        self.refresh();
    }

    fn step(&mut self, delta: isize) {
        let n = self.candidates.len();
        if n > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(n as isize) as usize;
        }
    }

    /// Put the highlighted suggestion in place of the token being typed.
    /// With `cycle`, pressing Tab again on a completed token moves to the next one.
    fn accept_selection(&mut self, cycle: bool) {
        let Some(candidate) = self.candidates.get(self.selected).cloned() else {
            return;
        };
        let start = self.token_start();
        if cycle && self.input[start..] == candidate.insert && self.candidates.len() > 1 {
            self.step(1);
            let next = self.candidates[self.selected].insert.clone();
            self.input.replace_range(start.., &next);
            return;
        }
        self.input.replace_range(start.., &candidate.insert);
        if self.candidates.len() == 1 {
            self.refresh();
        }
    }

    fn in_first_word(&self) -> bool {
        !self.input.contains(char::is_whitespace)
    }

    fn app(&self, id: &str) -> Option<&App> {
        self.apps.iter().find(|a| a.id == id)
    }

    fn uses(&self, app: &App) -> Reverse<u32> {
        Reverse(self.uses.get(&app.id).copied().unwrap_or(0))
    }

    /// Apps matching `query`, best first: how well they match, then the most used.
    fn matching(&self, query: &str) -> Vec<&App> {
        let mut found: Vec<(u8, &App)> = self.apps.iter().filter_map(|a| Some((a.rank(query)?, a))).collect();
        found.sort_by_key(|(rank, a)| (*rank, self.uses(a), a.name.len(), a.name.to_lowercase()));
        found.into_iter().map(|(_, a)| a).collect()
    }

    /// The finished line: the app (named by id, or the best match for a partial
    /// name) turned into its command, with the typed arguments. A name that matches
    /// no app runs as typed. Also the app it runs.
    fn resolve(&self) -> (Line, Option<&App>) {
        let word_end = self.input.find(char::is_whitespace).unwrap_or(self.input.len());
        let (name, args) = self.input.split_at(word_end);
        let app = self.app(name).or_else(|| {
            // The highlighted suggestion while typing the name, else the best match.
            let highlighted = self.candidates.get(self.selected).and_then(|c| self.app(&c.insert));
            highlighted.or_else(|| self.matching(name).first().copied())
        });
        match app {
            // Browsing, Enter with nothing typed runs the highlighted app.
            Some(app) if !name.is_empty() || self.browse => (Line { command: app.command(args) }, Some(app)),
            _ => (Line { command: format!("{name}{args}").trim().to_string() }, None),
        }
    }

    /// Byte offset where the token under completion starts.
    fn token_start(&self) -> usize {
        if self.in_first_word() {
            0
        } else {
            self.input.rfind(char::is_whitespace).map_or(0, |i| i + 1)
        }
    }

    fn refresh(&mut self) {
        self.apart(Self::refresh_candidates);
    }

    fn refresh_candidates(&mut self) {
        self.selected = 0;
        let token = &self.input[self.token_start()..];
        self.candidates = if self.in_first_word() {
            if token.is_empty() && !self.browse {
                Vec::new()
            } else {
                let mut apps = self.matching(token);
                if token.is_empty() {
                    apps = self.apps.iter().collect();
                    apps.sort_by_key(|a| (self.first.as_ref() != Some(&a.id), self.uses(a), a.name.to_lowercase()));
                }
                apps.into_iter()
                    .map(|a| Candidate { insert: a.id.clone(), label: a.name.clone(), icon: a.icon.clone() })
                    .collect()
            }
        } else {
            complete_path(&self.cwd, token)
        };
    }

    /// The visible slice of the list: (first index, rows).
    pub fn window(&self) -> (usize, &[Candidate]) {
        let n = self.candidates.len();
        let first = if self.selected < MAX_ROWS { 0 } else { self.selected + 1 - MAX_ROWS };
        (first, &self.candidates[first..(first + MAX_ROWS).min(n)])
    }
}

/// Files and directories in `cwd` matching `token` (which may contain a directory part).
fn complete_path(cwd: &Path, token: &str) -> Vec<Candidate> {
    let (dir_part, prefix) = match token.rfind('/') {
        Some(i) => (&token[..=i], &token[i + 1..]),
        None => ("", token),
    };
    let dir = if dir_part.starts_with('/') {
        PathBuf::from(dir_part)
    } else if let Some(rest) = dir_part.strip_prefix("~/") {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(rest)
    } else {
        cwd.join(dir_part)
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<Candidate> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if !name.starts_with(prefix) || (name.starts_with('.') && !prefix.starts_with('.')) {
                return None;
            }
            let slash = if e.path().is_dir() { "/" } else { "" };
            Some(Candidate {
                insert: format!("{dir_part}{name}{slash}"),
                label: format!("{name}{slash}"),
                icon: None,
            })
        })
        .collect();
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str, exec: &str) -> App {
        App { id: id.into(), name: name.into(), exec: exec.into(), ..Default::default() }
    }

    fn picker() -> Picker {
        let apps = vec![
            app("firefox", "Firefox", "/usr/lib/firefox/firefox %u"),
            app("org.gnome.Nautilus", "Files", "nautilus --new-window %U"),
            app("nvim", "Neovim", "nvim %F"),
            app("code", "Visual Studio Code", "/usr/bin/code %F"),
        ];
        Picker::new(apps, std::env::temp_dir())
    }

    fn typed(p: &mut Picker, text: &str) -> Outcome {
        let mut last = Outcome::Continue;
        for key in parse_keys(text.as_bytes()) {
            last = p.feed(key);
        }
        last
    }

    fn labels(p: &Picker) -> Vec<&str> {
        p.candidates().iter().map(|c| c.label.as_str()).collect()
    }

    #[test]
    fn browser_lists_every_app_before_typing() {
        let mut p = Picker::browser(picker().apps, std::env::temp_dir(), HashMap::new());
        assert_eq!(labels(&p), ["Files", "Firefox", "Neovim", "Visual Studio Code"]);
        // Backspace on nothing keeps it open; Enter runs the highlighted app.
        assert_eq!(p.feed(Key::Backspace), Outcome::Continue);
        p.feed(Key::Down);
        assert_eq!(p.feed(Key::Enter), Outcome::Run(Line { command: "/usr/lib/firefox/firefox".into() }));
        assert_eq!(p.chosen(), Some("firefox"));
        // Typing filters as usual.
        typed(&mut p, "neo");
        assert_eq!(labels(&p), ["Neovim"]);
    }

    #[test]
    fn a_lone_v_is_the_start_of_an_app_name() {
        let mut p = Picker::browser(picker().apps, std::env::temp_dir(), HashMap::new());
        assert_eq!(typed(&mut p, "v\r"), Outcome::Run(Line { command: "/usr/bin/code".into() }));
        assert_eq!(p.chosen(), Some("code"));
    }

    #[test]
    fn most_used_apps_come_first() {
        let uses = HashMap::from([("code".to_string(), 3), ("firefox".to_string(), 1)]);
        let mut p = Picker::browser(picker().apps, std::env::temp_dir(), uses);
        assert_eq!(labels(&p), ["Visual Studio Code", "Firefox", "Files", "Neovim"]);
        // Only among apps that match as well: "fi" starts both names.
        typed(&mut p, "fi");
        assert_eq!(labels(&p), ["Firefox", "Files"]);
        // A better match still wins over use.
        typed(&mut p, "\x15code");
        assert_eq!(labels(&p)[0], "Visual Studio Code");
        typed(&mut p, "\x15vim");
        assert_eq!(labels(&p), ["Neovim"]);
        // A typed command is not an app.
        typed(&mut p, "\x15true\r");
        assert_eq!(p.chosen(), None);
    }

    #[test]
    fn browser_sets_the_destination_aside() {
        let mut p = Picker::browser(picker().apps, std::env::temp_dir(), HashMap::new());
        typed(&mut p, "fire :d2");
        assert_eq!(p.dest(), Some(":d2"));
        // Suggestions and Tab are about the app, not the destination.
        assert_eq!(labels(&p), ["Firefox"]);
        p.feed(Key::Tab);
        assert_eq!(p.input(), "firefox :d2");
        assert_eq!(p.feed(Key::Enter), Outcome::Run(Line { command: "/usr/lib/firefox/firefox".into() }));

        // Only a destination: the highlighted app.
        let mut p = Picker::browser(picker().apps, std::env::temp_dir(), HashMap::new());
        typed(&mut p, ":s");
        assert_eq!(p.dest(), Some(":s"));
        assert!(matches!(p.feed(Key::Enter), Outcome::Run(_)));

        // `@` is typed like any other character.
        let mut p = Picker::browser(picker().apps, std::env::temp_dir(), HashMap::new());
        typed(&mut p, "fire @2");
        assert_eq!(p.dest(), None);

        // Not browsing: no destinations.
        let mut p = picker();
        typed(&mut p, "fire :d2");
        assert_eq!(p.dest(), None);
    }

    #[test]
    fn parses_keys() {
        assert_eq!(
            parse_keys(b"a\x1b[A\x1b[B\x1b[Z\t\r\x7f\x03\x1b"),
            vec![Key::Char('a'), Key::Up, Key::Down, Key::BackTab, Key::Tab, Key::Enter, Key::Backspace, Key::CtrlC, Key::Esc]
        );
        assert_eq!(parse_keys("ñ".as_bytes()), vec![Key::Char('ñ')]);
    }

    #[test]
    fn lists_desktop_apps_while_typing() {
        let mut p = picker();
        assert!(labels(&p).is_empty());
        typed(&mut p, "fi");
        assert_eq!(labels(&p), vec!["Files", "Firefox"]);
        let mut p = picker();
        typed(&mut p, "stud");
        assert_eq!(labels(&p), vec!["Visual Studio Code"]);
        let mut p = picker();
        typed(&mut p, "ff");
        assert_eq!(labels(&p), vec!["Firefox"]);
    }

    #[test]
    fn tab_completes_and_cycles() {
        let mut p = picker();
        typed(&mut p, "fi\t");
        assert_eq!(p.input(), "org.gnome.Nautilus");
        typed(&mut p, "\t");
        assert_eq!(p.input(), "firefox");
        let mut p = picker();
        typed(&mut p, "nv\t");
        assert_eq!(p.input(), "nvim");
    }

    #[test]
    fn enter_runs_the_app_command() {
        let run = |text: &str| typed(&mut picker(), text);
        let line = |command: &str| Outcome::Run(Line { command: command.into() });
        // A partial name runs the highlighted app, through its Exec line.
        assert_eq!(run("fire\r"), line("/usr/lib/firefox/firefox"));
        assert_eq!(run("naut\r"), line("nautilus --new-window"));
        // Arguments go where the Exec line wants them.
        assert_eq!(run("firefox https://a\r"), line("/usr/lib/firefox/firefox https://a"));
        assert_eq!(run("code .\r"), line("/usr/bin/code ."));
        // Down picks the second suggestion.
        assert_eq!(run("fi\x1b[B\r"), line("/usr/lib/firefox/firefox"));
        // Unknown names run as typed.
        assert_eq!(run("zzz --x\r"), line("zzz --x"));
    }

    #[test]
    fn cancel_keys() {
        assert_eq!(typed(&mut picker(), "\x7f"), Outcome::Cancel);
        assert_eq!(typed(&mut picker(), "nv\x1b"), Outcome::Cancel);
        assert_eq!(typed(&mut picker(), "nv\x03"), Outcome::Cancel);
        assert_eq!(typed(&mut picker(), "\r"), Outcome::Cancel);
    }

    #[test]
    fn completes_files_after_program() {
        let dir = std::env::temp_dir().join(format!("dune-picker-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("index.php"), "").unwrap();
        std::fs::write(dir.join(".hidden"), "").unwrap();
        let mut p = Picker::new(vec![app("nvim", "Neovim", "nvim %F")], dir.clone());
        typed(&mut p, "nvim ");
        assert_eq!(labels(&p), vec!["index.php", "src/"]);
        typed(&mut p, "in\t");
        assert_eq!(p.input(), "nvim index.php");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
