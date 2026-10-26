//! The scratchpad, like sway's: tiles put away off every desktop (`send_to_scratchpad`,
//! Mod+Shift+`), called back one at a time as a float on the desktop shown on the active
//! monitor (`scratchpad`, Mod+`). The same key on a shown one that is focused puts it back,
//! at the end of the queue, so pressing it again goes through them. A shown one made tiled
//! again, sent to a desktop or closed is no longer one.
//!
//! Bars see how many wait there: `dune scratchpad watch` keeps a control socket connection
//! (`watch_scratchpad`) to which dune writes a waybar JSON line now and on every change
//! (`status_line`); `dune scratchpad show` is the key, for a click on it.

use std::{io::Write, os::unix::net::UnixStream};

use crate::{
    Dune,
    floating::{centered, keep_inside, start_size},
    layout::{Rect, TileId},
};

/// The line bars get for the scratchpad holding windows with `titles`: their count (empty
/// with none, which waybar hides), the titles as tooltip, class `empty` or `filled`.
pub fn status_line(titles: &[String]) -> String {
    if titles.is_empty() {
        return r#"{"text":"","tooltip":"","class":"empty"}"#.into();
    }
    let tooltip = std::iter::once("Scratchpad".to_string()).chain(titles.iter().cloned()).collect::<Vec<_>>().join("\n");
    format!(r#"{{"text":"{}","tooltip":"{}","class":"filled"}}"#, titles.len(), json_escape(&tooltip))
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

impl Dune {
    /// Whether `id` waits in the scratchpad, not shown.
    pub fn in_scratchpad(&self, id: TileId) -> bool {
        self.scratchpad.iter().any(|(t, _)| *t == id)
    }

    /// Whether `id` is off the screen: on a desktop not shown, or in the scratchpad.
    pub fn put_away(&self, id: TileId) -> bool {
        self.hidden_desk(id).is_some() || self.in_scratchpad(id)
    }

    /// `id` leaves wherever it is for the end of the scratchpad, keeping its float's size
    /// and place. Focus goes to a neighbour; from a float (it has none in the strip), back
    /// to the tile focused before it.
    pub fn send_to_scratchpad(&mut self, id: TileId) {
        if !self.tiles.contains_key(&id) || self.in_scratchpad(id) {
            return;
        }
        let rect = self.float_rect(id);
        let before = rect.and_then(|_| previous_focus(&self.recent, id, |t| self.tiles.contains_key(&t) && !self.put_away(t)));
        let next = before.or_else(|| self.neighbor_of(id));
        self.remove_from_layouts(id);
        self.hide_windows(&[id]);
        self.scratchpad.push((id, rect));
        tracing::info!(tile = id, "into the scratchpad");
        if self.focused == Some(id) {
            self.focused = next;
        }
        self.arrange();
        self.update_focus();
    }

    /// The `scratchpad` key: the shown scratchpad tile focused goes back in, else the first
    /// one waiting comes out.
    pub fn toggle_scratchpad(&mut self) {
        if let Some(id) = self.focused.filter(|f| self.scratch_shown.contains(f) && self.is_floating(*f)) {
            return self.send_to_scratchpad(id);
        }
        if let Some(&(id, _)) = self.scratchpad.first() {
            self.show_scratchpad(id);
        }
    }

    /// `id` out of the scratchpad, floating on the desktop shown on the active monitor
    /// where it floated before (kept on that monitor), else in the middle; focused.
    pub fn show_scratchpad(&mut self, id: TileId) {
        let Some(i) = self.scratchpad.iter().position(|(t, _)| *t == id) else { return };
        let Some(output) = self.active_output() else { return };
        let Some(area) = self.output_rect(&output) else { return };
        let (_, rect) = self.scratchpad.remove(i);
        self.float_tile(id, &output.name(), back_at(rect, area));
        self.scratch_shown.insert(id);
        tracing::info!(tile = id, output = output.name(), "out of the scratchpad");
        self.focus_tile(id);
    }

    /// The titles of the windows waiting in the scratchpad, in its order.
    fn scratchpad_titles(&self) -> Vec<String> {
        self.scratchpad.iter().map(|(t, _)| *t).filter(|t| self.tiles.get(t).is_some_and(|tile| tile.top().is_some())).map(|t| self.shown_title(t)).collect()
    }

    /// A `watch_scratchpad` connection: told the state now, and on every change.
    pub fn watch_scratchpad(&mut self, mut stream: UnixStream) {
        let line = status_line(&self.scratchpad_titles());
        if writeln!(stream, "{line}").is_ok() && stream.set_nonblocking(true).is_ok() {
            self.scratchpad_watchers.push(stream);
        }
    }

    /// After every `arrange`: the watchers get the new state when it changed; those gone
    /// (or not reading) are dropped.
    pub fn refresh_scratchpad(&mut self) {
        let line = status_line(&self.scratchpad_titles());
        if line == self.scratchpad_status {
            return;
        }
        self.scratchpad_watchers.retain_mut(|w| writeln!(w, "{line}").is_ok());
        self.scratchpad_status = line;
    }
}

/// The tile focused last before `id` (`recent`, most recent first) that is still `there`.
fn previous_focus(recent: &[TileId], id: TileId, there: impl Fn(TileId) -> bool) -> Option<TileId> {
    recent.iter().copied().find(|t| *t != id && there(*t))
}

/// Where a tile out of the scratchpad floats on `area`: where it floated before (`rect`),
/// kept inside it, else in the middle.
fn back_at(rect: Option<Rect>, area: Rect) -> Rect {
    rect.map(|r| keep_inside(r, area)).unwrap_or_else(|| {
        let (w, h) = start_size(area);
        centered(area, w, h)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_line_counts_the_windows() {
        assert_eq!(status_line(&[]), r#"{"text":"","tooltip":"","class":"empty"}"#);
        let line = status_line(&["vim \"a\"".into(), "foot\\x".into()]);
        assert_eq!(line, r#"{"text":"2","tooltip":"Scratchpad\nvim \"a\"\nfoot\\x","class":"filled"}"#);
        assert_eq!(json_escape("a\tb"), "a\\u0009b");
    }

    #[test]
    fn focus_goes_back_to_the_tile_before() {
        // 9 is the float going away; 4 was focused before it, 7 is gone or put away.
        assert_eq!(previous_focus(&[9, 7, 4, 2], 9, |t| t != 7), Some(4));
        assert_eq!(previous_focus(&[9], 9, |_| true), None);
    }

    #[test]
    fn it_comes_back_where_it_floated_on_this_monitor() {
        let area = Rect::new(0, 0, 1000, 800);
        assert_eq!(back_at(Some(Rect::new(100, 100, 300, 200)), area), Rect::new(100, 100, 300, 200));
        // From a monitor further right: moved onto this one.
        assert_eq!(back_at(Some(Rect::new(1500, 100, 300, 200)), area), Rect::new(700, 100, 300, 200));
        let r = back_at(None, area);
        assert_eq!((r.x + r.w / 2, r.y + r.h / 2), (500, 400));
    }
}
