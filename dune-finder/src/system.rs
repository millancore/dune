//! The system mode (Mod+Tab from the last of the other modes): power off, reboot, log out
//! (end the dune session) or stay awake (dune holds off idle, so no lock nor suspend, until
//! chosen again). Typing narrows them, fuzzy; Enter or a click does one.

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};

use crate::panel::{Row, View};

const HINT: &str = "Enter: do it · ↑↓ Tab: choose · Mod+Tab: next mode · Esc: close";

/// What a row does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Power {
    Shutdown,
    Reboot,
    LogOut,
    /// Turn dune's Stay awake on or off.
    StayAwake,
}

impl Power {
    const ALL: [Power; 4] = [Power::Shutdown, Power::Reboot, Power::LogOut, Power::StayAwake];

    fn label(self) -> &'static str {
        match self {
            Power::Shutdown => "Power off",
            Power::Reboot => "Reboot",
            Power::LogOut => "Log out",
            Power::StayAwake => "Stay awake",
        }
    }

    /// Other words it is found by.
    fn also(self) -> &'static str {
        match self {
            Power::Shutdown => "shutdown apagar",
            Power::Reboot => "restart reiniciar",
            Power::LogOut => "exit quit session salir",
            Power::StayAwake => "wake up awake caffeine inhibit sleep suspend despierto",
        }
    }

    /// The command that does it; log out and stay awake are asked of dune instead (`None`).
    pub fn command(self) -> Option<&'static str> {
        match self {
            Power::Shutdown => Some("systemctl poweroff"),
            Power::Reboot => Some("systemctl reboot"),
            Power::LogOut | Power::StayAwake => None,
        }
    }
}

/// What a key did in the system mode.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Close,
    Do(Power),
    /// Backspace with nothing typed: back to the apps.
    Apps,
}

pub struct System {
    query: String,
    /// Rows matching `query`, best first.
    matches: Vec<Power>,
    selected: usize,
    matcher: Matcher,
    /// Stay awake is on: its row says so.
    awake: bool,
}

impl System {
    pub fn new(awake: bool) -> Self {
        System {
            query: String::new(),
            matches: Power::ALL.to_vec(),
            selected: 0,
            matcher: Matcher::new(Config::DEFAULT),
            awake,
        }
    }

    /// Nothing typed: `?` there lists the keys.
    pub fn nothing_typed(&self) -> bool {
        self.query.is_empty()
    }

    /// A key pressed: `sym` with `mods`, typing `text`.
    pub fn feed(&mut self, mods: &Modifiers, sym: Keysym, text: Option<char>) -> Step {
        let n = self.matches.len().max(1);
        match sym {
            Keysym::Escape => return Step::Close,
            Keysym::Return | Keysym::KP_Enter => {
                return self.matches.get(self.selected).map_or(Step::Continue, |p| Step::Do(*p));
            }
            Keysym::Down | Keysym::Tab => self.selected = (self.selected + 1) % n,
            Keysym::n if mods.ctrl => self.selected = (self.selected + 1) % n,
            Keysym::Up | Keysym::ISO_Left_Tab => self.selected = (self.selected + n - 1) % n,
            Keysym::p if mods.ctrl => self.selected = (self.selected + n - 1) % n,
            Keysym::BackSpace if self.query.is_empty() => return Step::Apps,
            Keysym::BackSpace => {
                self.query.pop();
                self.refilter();
            }
            Keysym::u if mods.ctrl => {
                self.query.clear();
                self.refilter();
            }
            _ => match text {
                Some(c) if !c.is_control() && !mods.ctrl && !mods.alt && !mods.logo => {
                    self.query.push(c);
                    self.refilter();
                }
                _ => {}
            },
        }
        Step::Continue
    }

    /// Row `row` of the list was clicked: do it.
    pub fn click(&mut self, row: usize) -> Step {
        self.matches.get(row).map_or(Step::Continue, |p| Step::Do(*p))
    }

    /// The rows matching the query again, fuzzy over label and other words, best first.
    fn refilter(&mut self) {
        self.selected = 0;
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let mut scored: Vec<(u32, Power)> = Power::ALL
            .into_iter()
            .filter_map(|p| {
                let haystack = format!("{} {}", p.label(), p.also());
                pattern.score(Utf32Str::new(&haystack, &mut buf), &mut self.matcher).map(|score| (score, p))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        self.matches = scored.into_iter().map(|(_, p)| p).collect();
    }

    /// What the panel shows.
    pub fn view(&self) -> View<'_> {
        let rows = self
            .matches
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let detail = (*p == Power::StayAwake && self.awake).then(|| "on".to_string());
                Row { icon: None, text: p.label(), detail, sub: None, selected: i == self.selected }
            })
            .collect();
        View {
            prompt: format!("system {}▏", self.query),
            rows,
            placeholder: "",
            note: String::new(),
            side: None,
            hint: HINT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> Modifiers {
        Modifiers::default()
    }

    #[test]
    fn enter_does_the_row_chosen() {
        let mut s = System::new(false);
        assert_eq!(s.feed(&none(), Keysym::Down, None), Step::Continue);
        assert_eq!(s.feed(&none(), Keysym::Return, None), Step::Do(Power::Reboot));
        assert_eq!(s.click(2), Step::Do(Power::LogOut));
        assert_eq!(s.click(5), Step::Continue);
        assert_eq!(s.feed(&none(), Keysym::Escape, None), Step::Close);
    }

    #[test]
    fn typing_narrows_them_in_either_language() {
        let mut s = System::new(false);
        for c in "salir".chars() {
            s.feed(&none(), Keysym::NoSymbol, Some(c));
        }
        assert_eq!(s.feed(&none(), Keysym::Return, None), Step::Do(Power::LogOut));
        let mut s = System::new(false);
        for c in "zzz".chars() {
            s.feed(&none(), Keysym::NoSymbol, Some(c));
        }
        assert_eq!(s.feed(&none(), Keysym::Return, None), Step::Continue);
        assert_eq!(Power::LogOut.command(), None);
        assert_eq!(Power::Shutdown.command(), Some("systemctl poweroff"));
    }

    #[test]
    fn stay_awake_is_found_by_wake_and_says_when_on() {
        let mut s = System::new(false);
        for c in "wake".chars() {
            s.feed(&none(), Keysym::NoSymbol, Some(c));
        }
        assert_eq!(s.view().rows[0].text, "Stay awake");
        assert_eq!(s.view().rows[0].detail, None);
        assert_eq!(s.feed(&none(), Keysym::Return, None), Step::Do(Power::StayAwake));
        assert_eq!(Power::StayAwake.command(), None);
        let s = System::new(true);
        assert_eq!(s.view().rows[3].detail.as_deref(), Some("on"));
    }
}
