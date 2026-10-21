//! The tile options mode (Mod+/): what can be done to the focused tile — make it private
//! (✓ when it is), float, center, full width, to a tab, send to a desktop, close — each
//! with its key when it has one. dune gives the rows (a `tile_options` request); without
//! a focused tile there are none and the finder does not open. Typing narrows them,
//! fuzzy; Enter or a click does one (a `tile_action` request).

use dune_common::protocol::{TileAction, TileOption};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};

use crate::panel::{ROWS, Row, View};

const HINT: &str = "Enter: do it · ↑↓ Tab: choose · Esc: close";

/// A row: what it does and what it says.
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    action: TileAction,
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
        Entry { action: option.action, text, keys: option.keys }
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
    query: String,
    /// Entries matching `query`, best first.
    matches: Vec<usize>,
    selected: usize,
    matcher: Matcher,
}

impl TileOptions {
    /// `options`, in the order dune gives them.
    pub fn new(options: Vec<TileOption>) -> Self {
        let entries: Vec<Entry> = options.into_iter().map(Entry::new).collect();
        TileOptions {
            matches: (0..entries.len()).collect(),
            entries,
            query: String::new(),
            selected: 0,
            matcher: Matcher::new(Config::DEFAULT),
        }
    }

    /// A key pressed: `sym` with `mods`, typing `text`.
    pub fn feed(&mut self, mods: &Modifiers, sym: Keysym, text: Option<char>) -> Step {
        let n = self.matches.len().max(1);
        match sym {
            Keysym::Escape => return Step::Close,
            Keysym::Return | Keysym::KP_Enter => {
                return self
                    .matches
                    .get(self.selected)
                    .map_or(Step::Close, |i| Step::Do(self.entries[*i].action));
            }
            Keysym::Down | Keysym::Tab => self.selected = (self.selected + 1) % n,
            Keysym::n if mods.ctrl => self.selected = (self.selected + 1) % n,
            Keysym::Up | Keysym::ISO_Left_Tab => self.selected = (self.selected + n - 1) % n,
            Keysym::p if mods.ctrl => self.selected = (self.selected + n - 1) % n,
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
        match self.matches.get(first_shown(self.selected) + row) {
            Some(e) => Step::Do(self.entries[*e].action),
            None => Step::Continue,
        }
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
                    selected: i == self.selected,
                }
            })
            .collect();
        View {
            prompt: format!("tile {}▏", self.query),
            rows,
            status: format!("{} / {} options", self.matches.len(), self.entries.len()),
            hint: HINT,
        }
    }
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

    fn listed(o: &TileOptions) -> Vec<TileAction> {
        o.matches.iter().map(|i| o.entries[*i].action).collect()
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
        typed(&mut o, "desk");
        assert_eq!(listed(&o), [TileAction::SendToDesk(3)]);
        o.feed(&none(), Keysym::Down, None);
        assert_eq!(o.feed(&none(), Keysym::Return, None), Step::Do(TileAction::SendToDesk(3)));
        // Back to all with nothing typed; nothing matching: Enter closes.
        for _ in 0..4 {
            o.feed(&none(), Keysym::BackSpace, None);
        }
        assert_eq!(listed(&o).len(), 5);
        assert_eq!(o.feed(&none(), Keysym::BackSpace, None), Step::Continue);
        typed(&mut o, "zzz");
        assert!(listed(&o).is_empty());
        assert_eq!(o.feed(&none(), Keysym::Return, None), Step::Close);
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
