//! The tile options mode (Mod+/): what can be done to the focused tile — make it private
//! (✓ when it is), float, center, full width, to a tab, send to a desktop, close — each
//! with its key when it has one. dune gives the rows (a `tile_action` per desktop to send
//! to, shown as one "Send to desktop #" row whose desktops come up when it is chosen: a
//! digit sends there); without a focused tile there are none and the finder does not
//! open. Typing narrows them, fuzzy; Enter or a click does one (a `tile_action` request).

use dune_common::protocol::{TileAction, TileOption};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};

use crate::panel::{ROWS, Row, View};

const HINT: &str = "Enter: do it · ↑↓ Tab: choose · Esc: close";
const DESK_HINT: &str = "1-9 Enter: send · ↑↓ Tab: choose · Esc: back";

/// What a row does.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Does {
    Action(TileAction),
    /// Show the desktops to send the tile to.
    SendTo,
}

/// A row: what it does and what it says.
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    does: Does,
    /// Its label, `✓` after a switch that is on.
    text: String,
    keys: String,
}

impl Entry {
    fn new(option: TileOption) -> Self {
        let mut text = option.action.label();
        if option.on == Some(true) {
            text.push_str(" ✓");
        }
        Entry { does: Does::Action(option.action), text, keys: option.keys }
    }
}

/// What a key did in the tile options.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Close,
    Do(TileAction),
}

pub struct TileOptions {
    entries: Vec<Entry>,
    /// The desktops to send the tile to, one row each.
    desks: Vec<Entry>,
    /// The options, while `entries` are the desktops.
    menu: Option<Vec<Entry>>,
    query: String,
    /// Entries matching `query`, best first.
    matches: Vec<usize>,
    selected: usize,
    matcher: Matcher,
}

impl TileOptions {
    /// `options`, in the order dune gives them; those sending to a desktop in one row,
    /// where the first of them is.
    pub fn new(options: Vec<TileOption>) -> Self {
        let mut entries = Vec::new();
        let mut desks = Vec::new();
        for option in options {
            let TileAction::SendToDesk(n) = option.action else {
                entries.push(Entry::new(option));
                continue;
            };
            if desks.is_empty() {
                entries.push(Entry { does: Does::SendTo, text: "Send to desktop #".into(), keys: any_desk(&option.keys) });
            }
            desks.push(Entry { does: Does::Action(option.action), text: format!("Desktop {n}"), keys: option.keys });
        }
        TileOptions {
            matches: (0..entries.len()).collect(),
            entries,
            desks,
            menu: None,
            query: String::new(),
            selected: 0,
            matcher: Matcher::new(Config::DEFAULT),
        }
    }

    /// A key pressed: `sym` with `mods`, typing `text`.
    pub fn feed(&mut self, mods: &Modifiers, sym: Keysym, text: Option<char>) -> Step {
        let n = self.matches.len().max(1);
        match sym {
            Keysym::Escape if self.menu.is_some() => self.back(),
            Keysym::Escape => return Step::Close,
            Keysym::Return | Keysym::KP_Enter => {
                return match self.matches.get(self.selected) {
                    Some(&e) => self.choose(e),
                    None if self.menu.is_some() => Step::Continue,
                    None => Step::Close,
                };
            }
            Keysym::Down | Keysym::Tab => self.selected = (self.selected + 1) % n,
            Keysym::n if mods.ctrl => self.selected = (self.selected + 1) % n,
            Keysym::Up | Keysym::ISO_Left_Tab => self.selected = (self.selected + n - 1) % n,
            Keysym::p if mods.ctrl => self.selected = (self.selected + n - 1) % n,
            Keysym::BackSpace if self.query.is_empty() && self.menu.is_some() => self.back(),
            Keysym::BackSpace => {
                self.query.pop();
                self.refilter();
            }
            Keysym::u if mods.ctrl => {
                self.query.clear();
                self.refilter();
            }
            _ => match text {
                // A desktop's number sends the tile there.
                Some(c @ '1'..='9') if self.menu.is_some() => {
                    let n = c as u8 - b'0';
                    if let Some(e) = self.entries.iter().position(|e| e.does == Does::Action(TileAction::SendToDesk(n))) {
                        return self.choose(e);
                    }
                }
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
        match self.matches.get(first_shown(self.selected) + row) {
            Some(&e) => self.choose(e),
            None => Step::Continue,
        }
    }

    /// Entry `e` chosen: do it, or show the desktops to send to.
    fn choose(&mut self, e: usize) -> Step {
        match self.entries[e].does {
            Does::Action(action) => Step::Do(action),
            Does::SendTo => {
                self.menu = Some(std::mem::replace(&mut self.entries, self.desks.clone()));
                self.query.clear();
                self.refilter();
                Step::Continue
            }
        }
    }

    /// From the desktops back to the options, on "Send to desktop #".
    fn back(&mut self) {
        let Some(menu) = self.menu.take() else { return };
        self.entries = menu;
        self.query.clear();
        self.refilter();
        self.selected = self.matches.iter().position(|&e| self.entries[e].does == Does::SendTo).unwrap_or(0);
    }

    /// The entries matching the query again, fuzzy over label and keys, best first (ties
    /// keep dune's order).
    fn refilter(&mut self) {
        self.selected = 0;
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let mut scored: Vec<(u32, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let haystack = format!("{} {}", e.text, e.keys);
                pattern.score(Utf32Str::new(&haystack, &mut buf), &mut self.matcher).map(|score| (score, i))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        self.matches = scored.into_iter().map(|(_, i)| i).collect();
    }

    /// What the panel shows: each option, its keys dim at the right.
    pub fn view(&self) -> View<'_> {
        let first = first_shown(self.selected);
        let rows = self
            .matches
            .iter()
            .enumerate()
            .skip(first)
            .take(ROWS)
            .map(|(i, e)| {
                let entry = &self.entries[*e];
                Row {
                    icon: None,
                    text: &entry.text,
                    detail: Some(entry.keys.clone()).filter(|k| !k.is_empty()),
                    sub: None,
                    selected: i == self.selected,
                }
            })
            .collect();
        let (prompt, what, hint) = match self.menu {
            Some(_) => ("send to desktop", "desktops", DESK_HINT),
            None => ("tile", "options", HINT),
        };
        View {
            prompt: format!("{prompt} {}▏", self.query),
            rows,
            status: format!("{} / {} {what}", self.matches.len(), self.entries.len()),
            hint,
        }
    }
}

/// The keys sending to desktop 1 (`super+shift+1, alt+1`) as the keys sending to any:
/// `super+shift+#`, ... Empty when they are not keys ending in its number.
fn any_desk(keys: &str) -> String {
    let keys: Option<Vec<String>> = keys
        .split(", ")
        .filter(|k| !k.is_empty())
        .map(|k| k.strip_suffix('1').map(|k| format!("{k}#")))
        .collect();
    keys.map(|k| k.join(", ")).unwrap_or_default()
}

/// First of the `ROWS` matches shown, so the selected one is among them.
fn first_shown(selected: usize) -> usize {
    selected.saturating_sub(ROWS - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> TileOptions {
        let option = |action, on, keys: &str| TileOption { action, on, keys: keys.into() };
        TileOptions::new(vec![
            option(TileAction::PrivateTile, Some(true), ""),
            option(TileAction::Center, None, ""),
            option(TileAction::Float, None, "super+shift+space"),
            option(TileAction::SendToDesk(1), None, "super+shift+1"),
            option(TileAction::SendToDesk(2), None, "super+shift+2"),
            option(TileAction::SendToDesk(3), None, "super+shift+3"),
            option(TileAction::Close, None, "super+q"),
        ])
    }

    fn none() -> Modifiers {
        Modifiers::default()
    }

    fn typed(o: &mut TileOptions, text: &str) {
        for c in text.chars() {
            assert_eq!(o.feed(&none(), Keysym::a, Some(c)), Step::Continue);
        }
    }

    fn listed(o: &TileOptions) -> Vec<Does> {
        o.matches.iter().map(|i| o.entries[*i].does).collect()
    }

    #[test]
    fn switches_that_are_on_are_checked() {
        let o = options();
        assert_eq!(o.entries[0].text, "Private tile ✓");
        assert_eq!(o.entries[1].text, "Center");
        let view = o.view();
        assert_eq!(view.rows[2].detail.as_deref(), Some("super+shift+space"));
        assert_eq!(view.rows[0].detail, None);
        assert!(view.rows[0].selected);
    }

    #[test]
    fn typing_narrows_and_enter_does_the_first() {
        let mut o = options();
        assert_eq!(o.feed(&none(), Keysym::Return, None), Step::Do(TileAction::PrivateTile));
        typed(&mut o, "close");
        assert_eq!(listed(&o), [Does::Action(TileAction::Close)]);
        o.feed(&none(), Keysym::Down, None);
        assert_eq!(o.feed(&none(), Keysym::Return, None), Step::Do(TileAction::Close));
        // Back to all with nothing typed; nothing matching: Enter closes.
        for _ in 0..5 {
            o.feed(&none(), Keysym::BackSpace, None);
        }
        assert_eq!(listed(&o).len(), 5);
        assert_eq!(o.feed(&none(), Keysym::BackSpace, None), Step::Continue);
        typed(&mut o, "zzz");
        assert!(listed(&o).is_empty());
        assert_eq!(o.feed(&none(), Keysym::Return, None), Step::Close);
    }

    #[test]
    fn one_row_sends_to_a_desktop_its_number_chooses() {
        let mut o = options();
        assert_eq!(o.entries.len(), 5);
        assert_eq!((o.entries[3].text.as_str(), o.entries[3].keys.as_str()), ("Send to desktop #", "super+shift+#"));
        typed(&mut o, "send");
        assert_eq!(listed(&o), [Does::SendTo]);
        // Enter shows the desktops, a digit sends there.
        assert_eq!(o.feed(&none(), Keysym::Return, None), Step::Continue);
        assert_eq!(o.view().rows[1].text, "Desktop 2");
        assert_eq!(o.view().status, "3 / 3 desktops");
        assert_eq!(o.feed(&none(), Keysym::_2, Some('2')), Step::Do(TileAction::SendToDesk(2)));
        // One that is not there does nothing; Esc goes back to the options, on the row.
        assert_eq!(o.feed(&none(), Keysym::_7, Some('7')), Step::Continue);
        assert_eq!(o.feed(&none(), Keysym::Escape, None), Step::Continue);
        assert_eq!(o.entries.len(), 5);
        assert_eq!(o.feed(&none(), Keysym::Return, None), Step::Continue);
        // A click on a desktop sends there too.
        assert_eq!(o.click(2), Step::Do(TileAction::SendToDesk(3)));
    }

    #[test]
    fn keys_for_any_desktop() {
        assert_eq!(any_desk("super+shift+1, alt+1"), "super+shift+#, alt+#");
        assert_eq!(any_desk(""), "");
        assert_eq!(any_desk("super+F2"), "");
    }

    #[test]
    fn a_click_does_the_row_and_escape_closes() {
        let mut o = options();
        assert_eq!(o.click(4), Step::Do(TileAction::Close));
        assert_eq!(o.click(5), Step::Continue);
        o.feed(&none(), Keysym::Up, None);
        assert_eq!(o.feed(&none(), Keysym::Return, None), Step::Do(TileAction::Close));
        assert_eq!(o.feed(&none(), Keysym::Escape, None), Step::Close);
    }
}
