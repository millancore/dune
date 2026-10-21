use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent, InputTime, KeyState,
        PointerMotionEvent,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    },
    input::{
        keyboard::{FilterResult, Keycode, Keysym, ModifiersState},
        pointer::{AxisFrame, ButtonEvent, MotionEvent, PointerHandle, RelativeMotionEvent},
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
    wayland::{
        compositor::RegionAttributes,
        pointer_constraints::{PointerConstraint, with_pointer_constraint},
    },
};

use dune_common::protocol::TileAction;

use crate::{
    config::{Binding, Config, TerminalHere},
    launcher::FinderMode,
    layout::{Direction, Layout, Rect, Side, TileId, split_band, split_edge},
    state::{Dune, MoveDrag},
};

/// What compositor keys do: the config's bindings (`Binding`), and VT switching.
#[derive(Debug, Clone, Copy)]
enum Action {
    /// Mod+Enter: an empty container, dune-finder to choose what goes in it.
    EmptyContainer,
    Focus(Direction),
    Swap(Direction),
    /// Column width / tile height, as a share of the output.
    Width(f64),
    Height(f64),
    /// Width of a tile inside its row, as a share of the column.
    RowWidth(f64),
    CycleWidth,
    FullWidth,
    /// Center the focused column on its monitor (again: stop centering it).
    Center,
    /// Float the focused tile in the middle of its monitor (again: back into the strip).
    ToggleFloat,
    Close,
    Quit,
    /// Mod+d: show dune-finder on the apps (again: hide it).
    Launcher,
    /// dune-finder on the windows (again: hide it); no key by default (`/` in the finder).
    Switcher,
    /// Mod+/: dune-finder on the focused tile's options (again: hide it).
    TileOptions,
    /// Block the focused tile on screen and in captures (again: show it).
    PrivateTile,
    /// Mod+v / Mod+s / Mod+Shift+t: a terminal to the right of the focused tile, below it
    /// or as a tab of its container, in its folder.
    Terminal(TerminalHere),
    /// Mod+Tab / Mod+Shift+Tab: the next / previous tab of the focused column.
    CycleTab(i32),
    /// Mod+Ctrl+direction: join the column that way as a tab, or leave a tabbed one.
    MoveTab(Direction),
    /// Mod+t: the focused tile leaves the tiles it shares a tab with for a tab of its own
    /// in its column; alone, the windows stacked in it become tabs of its column; a tab
    /// with none goes back where it was in its column, or out of the tabs.
    StackToTabs,
    /// Mod+1..9: show that desktop on the active monitor.
    Desk(u8),
    /// Mod+Shift+1..9: send the focused tile to that desktop.
    SendToDesk(u8),
    /// Mod+Escape: run `lock_command` (swaylock).
    Lock,
    /// Ctrl+Alt+F1..F12 on a real session.
    SwitchVt(i32),
}

impl From<Binding> for Action {
    fn from(binding: Binding) -> Action {
        match binding {
            Binding::Focus(dir) => Action::Focus(dir),
            Binding::Swap(dir) => Action::Swap(dir),
            Binding::MoveTab(dir) => Action::MoveTab(dir),
            Binding::Desk(n) => Action::Desk(n),
            Binding::SendToDesk(n) => Action::SendToDesk(n),
            Binding::EmptyContainer => Action::EmptyContainer,
            Binding::NextTab => Action::CycleTab(1),
            Binding::PrevTab => Action::CycleTab(-1),
            Binding::Wider => Action::Width(RESIZE_STEP),
            Binding::Narrower => Action::Width(-RESIZE_STEP),
            Binding::Taller => Action::Height(RESIZE_STEP),
            Binding::Shorter => Action::Height(-RESIZE_STEP),
            Binding::RowWider => Action::RowWidth(RESIZE_STEP),
            Binding::RowNarrower => Action::RowWidth(-RESIZE_STEP),
            Binding::CycleWidth => Action::CycleWidth,
            Binding::FullWidth => Action::FullWidth,
            Binding::Center => Action::Center,
            Binding::Float => Action::ToggleFloat,
            Binding::Close => Action::Close,
            Binding::Launcher => Action::Launcher,
            Binding::Switcher => Action::Switcher,
            Binding::TileOptions => Action::TileOptions,
            Binding::PrivateTile => Action::PrivateTile,
            Binding::Terminal(here) => Action::Terminal(here),
            Binding::StackToTabs => Action::StackToTabs,
            Binding::Quit => Action::Quit,
            Binding::Lock => Action::Lock,
        }
    }
}

/// How much one press of the resize keys changes a width or height.
const RESIZE_STEP: f64 = 0.05;
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
/// How far the pointer may move during a Mod+click before it counts as a drag.
const CLICK_SLOP: f64 = 4.0;
/// Scroll distance of one wheel notch (as smithay reports it).
const SCROLL_NOTCH: f64 = 15.0;

/// Pixels the pointer pushes against a screen edge before the strip starts sliding.
const EDGE_RESISTANCE: f64 = 50.0;
/// Pixels the strip slides per pixel the pointer pushes past the resistance.
const EDGE_SPEED: f64 = 2.0;

/// The pointer, already at a left/right screen edge (`at_edge`), went `over` pixels past it
/// (right: positive; 0: not pushing): what has been pushed so far goes in `pushed`, and once
/// past `EDGE_RESISTANCE` the pixels to slide the strip. Leaving the edge starts over.
fn edge_slide(pushed: &mut f64, at_edge: bool, over: f64) -> i32 {
    if !at_edge || over == 0.0 {
        *pushed = 0.0;
        return 0;
    }
    let before = *pushed;
    *pushed += over.abs();
    let past = *pushed - before.max(EDGE_RESISTANCE);
    if past <= 0.0 {
        return 0;
    }
    (past * EDGE_SPEED * over.signum()).round() as i32
}

/// Width/height change for a Super+scroll: a wheel notch is one resize step, up grows.
fn scroll_resize_delta(vertical_scroll: f64) -> f64 {
    -vertical_scroll / SCROLL_NOTCH * RESIZE_STEP
}

fn modifier_held(modifier: &str, mods: &ModifiersState) -> bool {
    match modifier {
        "super" | "logo" => mods.logo,
        "ctrl+alt" => mods.ctrl && mods.alt,
        _ => mods.alt,
    }
}

fn action_for(config: &Config, mods: &ModifiersState, sym: Keysym) -> Option<Action> {
    // The keymap turns Ctrl+Alt+Fn into XF86Switch_VT_n.
    let vt = sym.raw().wrapping_sub(Keysym::XF86_Switch_VT_1.raw());
    if vt < 12 {
        return Some(Action::SwitchVt(vt as i32 + 1));
    }
    let (binding, _) = config.bindings.iter().find(|(_, key)| key.matches(mods, sym))?;
    Some((*binding).into())
}

/// While the session is locked, only VT switching stays a compositor key; the rest go to
/// the lock surface.
fn allowed_while_locked(action: &Action) -> bool {
    matches!(action, Action::SwitchVt(_))
}

/// The closest point to `pos` inside one of `outputs` (pointer motion stays on the screens).
fn clamp_to(outputs: &[Rectangle<i32, Logical>], pos: Point<f64, Logical>) -> Point<f64, Logical> {
    if outputs.iter().any(|g| g.to_f64().contains(pos)) {
        return pos;
    }
    outputs
        .iter()
        .map(|g| {
            let g = g.to_f64();
            let x = pos.x.clamp(g.loc.x, g.loc.x + g.size.w - 1.0);
            let y = pos.y.clamp(g.loc.y, g.loc.y + g.size.h - 1.0);
            Point::from((x, y))
        })
        .min_by(|a, b| {
            let d = |p: &Point<f64, Logical>| (p.x - pos.x).powi(2) + (p.y - pos.y).powi(2);
            d(a).total_cmp(&d(b))
        })
        .unwrap_or(pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_stays_on_the_screens() {
        // Two monitors side by side, of different heights (like 2560x1080 + 1920x1080, shorter).
        let outputs = [Rectangle::new((0, 0).into(), (2560, 1080).into()), Rectangle::new((2560, 0).into(), (1920, 900).into())];
        let p = |x: f64, y: f64| Point::<f64, Logical>::from((x, y));
        assert_eq!(clamp_to(&outputs, p(100.0, 100.0)), p(100.0, 100.0));
        assert_eq!(clamp_to(&outputs, p(-50.0, 100.0)), p(0.0, 100.0));
        assert_eq!(clamp_to(&outputs, p(3000.0, 1000.0)), p(3000.0, 899.0)); // below the short monitor
        assert_eq!(clamp_to(&outputs, p(9000.0, -5.0)), p(4479.0, 0.0));
    }

    #[test]
    fn pushing_a_screen_edge_slides_past_the_resistance() {
        let mut pushed = 0.0;
        // Arriving at the edge does not count.
        assert_eq!(edge_slide(&mut pushed, false, 30.0), 0);
        assert_eq!(edge_slide(&mut pushed, true, 30.0), 0);
        assert_eq!(edge_slide(&mut pushed, true, 30.0), (10.0 * EDGE_SPEED) as i32);
        assert_eq!(edge_slide(&mut pushed, true, 5.0), (5.0 * EDGE_SPEED) as i32);
        // Moving off the edge starts over; the left edge slides the other way.
        assert_eq!(edge_slide(&mut pushed, true, 0.0), 0);
        assert_eq!(edge_slide(&mut pushed, true, -60.0), -(10.0 * EDGE_SPEED) as i32);
    }

    #[test]
    fn scroll_resize_steps() {
        assert_eq!(scroll_resize_delta(-15.0), RESIZE_STEP); // one notch up grows
        assert_eq!(scroll_resize_delta(30.0), -2.0 * RESIZE_STEP);
    }

    fn test_config(modifier: &str) -> Config {
        crate::config::from_text(&format!("modifier = {modifier}\n"))
    }

    #[test]
    fn vt_switch_keys() {
        let config = test_config("alt");
        let mods = ModifiersState { ctrl: true, alt: true, ..Default::default() };
        assert!(matches!(action_for(&config, &mods, Keysym::XF86_Switch_VT_3), Some(Action::SwitchVt(3))));
        assert!(matches!(action_for(&config, &mods, Keysym::XF86_Switch_VT_12), Some(Action::SwitchVt(12))));
        assert!(action_for(&config, &mods, Keysym::F3).is_none());
    }

    #[test]
    fn only_vt_switching_while_locked() {
        let config = test_config("alt");
        let alt = ModifiersState { alt: true, ..Default::default() };
        let ctrl_alt = ModifiersState { ctrl: true, alt: true, ..Default::default() };
        let allowed = |mods: &ModifiersState, sym| action_for(&config, mods, sym).is_some_and(|a| allowed_while_locked(&a));
        assert!(allowed(&ctrl_alt, Keysym::XF86_Switch_VT_2));
        for sym in [Keysym::Return, Keysym::d, Keysym::slash, Keysym::q, Keysym::_1, Keysym::Left, Keysym::E, Keysym::Escape] {
            assert!(action_for(&config, &alt, sym).is_none_or(|a| !allowed_while_locked(&a)), "{sym:?}");
        }
        // Mod+Shift+e (quit) stays off too.
        let alt_shift = ModifiersState { alt: true, shift: true, ..Default::default() };
        assert!(matches!(action_for(&config, &alt_shift, Keysym::e), Some(Action::Quit)));
        assert!(!allowed(&alt_shift, Keysym::e));
    }

    #[test]
    fn ctrl_resizes_inside_the_row() {
        let alt_ctrl = ModifiersState { alt: true, ctrl: true, ..Default::default() };
        let config = test_config("alt");
        assert!(matches!(action_for(&config, &alt_ctrl, Keysym::plus), Some(Action::RowWidth(d)) if d > 0.0));
        assert!(matches!(action_for(&config, &alt_ctrl, Keysym::minus), Some(Action::RowWidth(d)) if d < 0.0));
        // With Ctrl in the modifier, the plain keys stay the column's width.
        let config = test_config("ctrl+alt");
        assert!(matches!(action_for(&config, &alt_ctrl, Keysym::plus), Some(Action::Width(_))));
    }

    #[test]
    fn terminal_keys() {
        let config = test_config("super");
        let logo = ModifiersState { logo: true, ..Default::default() };
        assert!(matches!(action_for(&config, &logo, Keysym::v), Some(Action::Terminal(TerminalHere::Right))));
        assert!(matches!(action_for(&config, &logo, Keysym::s), Some(Action::Terminal(TerminalHere::Below))));
        // A key set for one replaces its Mod key; the others keep theirs.
        let config = crate::config::from_text("modifier = super\nterminal_vertical = mod+n\n");
        assert!(matches!(action_for(&config, &logo, Keysym::n), Some(Action::Terminal(TerminalHere::Right))));
        assert!(action_for(&config, &logo, Keysym::v).is_none());
        assert!(matches!(action_for(&config, &logo, Keysym::s), Some(Action::Terminal(TerminalHere::Below))));
    }

    #[test]
    fn tab_keys() {
        let config = test_config("alt");
        let alt = ModifiersState { alt: true, ..Default::default() };
        let alt_shift = ModifiersState { alt: true, shift: true, ..Default::default() };
        let alt_ctrl = ModifiersState { alt: true, ctrl: true, ..Default::default() };
        assert!(matches!(action_for(&config, &alt, Keysym::Tab), Some(Action::CycleTab(1))));
        assert!(matches!(action_for(&config, &alt_shift, Keysym::Tab), Some(Action::CycleTab(-1))));
        assert!(matches!(action_for(&config, &alt_shift, Keysym::ISO_Left_Tab), Some(Action::CycleTab(-1))));
        assert!(matches!(action_for(&config, &alt_ctrl, Keysym::Left), Some(Action::MoveTab(Direction::Left))));
        assert!(matches!(action_for(&config, &alt_ctrl, Keysym::j), Some(Action::MoveTab(Direction::Down))));
        assert!(matches!(action_for(&config, &alt, Keysym::t), Some(Action::StackToTabs)));
        assert!(matches!(action_for(&config, &alt_shift, Keysym::t), Some(Action::Terminal(TerminalHere::Tab))));
        assert!(matches!(action_for(&config, &alt, Keysym::slash), Some(Action::TileOptions)));
        assert!(matches!(action_for(&config, &alt_shift, Keysym::Left), Some(Action::Swap(Direction::Left))));
        // With Ctrl in the modifier, Ctrl+direction stays a focus move.
        let config = test_config("ctrl+alt");
        assert!(matches!(action_for(&config, &alt_ctrl, Keysym::Left), Some(Action::Focus(Direction::Left))));
    }

    #[test]
    fn escape_locks() {
        let alt = ModifiersState { alt: true, ..Default::default() };
        assert!(matches!(action_for(&test_config("alt"), &alt, Keysym::Escape), Some(Action::Lock)));
        assert!(action_for(&test_config("alt"), &ModifiersState::default(), Keysym::Escape).is_none());
        let logo = ModifiersState { logo: true, ..Default::default() };
        assert!(matches!(action_for(&test_config("super"), &logo, Keysym::Escape), Some(Action::Lock)));
        // Set off: Mod+Escape goes to the client.
        let config = crate::config::from_text("lock = none\n");
        assert!(action_for(&config, &alt, Keysym::Escape).is_none());
    }

    #[test]
    fn digits_pick_desktops() {
        let config = test_config("super");
        let logo = ModifiersState { logo: true, ..Default::default() };
        let shift = ModifiersState { logo: true, shift: true, ..Default::default() };
        assert!(matches!(action_for(&config, &logo, Keysym::_1), Some(Action::Desk(1))));
        assert!(matches!(action_for(&config, &logo, Keysym::_9), Some(Action::Desk(9))));
        assert!(matches!(action_for(&config, &shift, Keysym::_3), Some(Action::SendToDesk(3))));
        assert!(action_for(&config, &logo, Keysym::_0).is_none());
        assert!(action_for(&config, &ModifiersState::default(), Keysym::_2).is_none());
    }
}

/// What an active pointer constraint asks of the pointer.
enum Constraint {
    Locked,
    /// Confined to this region of the surface (all of it without one).
    Confined(Option<RegionAttributes>),
}

/// The active constraint of `surface`, when the pointer at `local` (on the surface) is in its region.
fn active_constraint(surface: &WlSurface, pointer: &PointerHandle<Dune>, local: Point<f64, Logical>) -> Option<Constraint> {
    with_pointer_constraint(surface, pointer, |constraint| {
        let constraint = constraint.filter(|c| c.is_active())?;
        if !constraint.region().is_none_or(|r| r.contains(local.to_i32_round())) {
            return None;
        }
        Some(match &*constraint {
            PointerConstraint::Locked(_) => Constraint::Locked,
            PointerConstraint::Confined(confined) => Constraint::Confined(confined.region().cloned()),
        })
    })
}

/// The pointer reached `surface` at `local`: a constraint waiting there starts.
fn activate_constraint(surface: &WlSurface, pointer: &PointerHandle<Dune>, local: Point<f64, Logical>) {
    with_pointer_constraint(surface, pointer, |constraint| {
        if let Some(constraint) = constraint.filter(|c| !c.is_active())
            && constraint.region().is_none_or(|r| r.contains(local.to_i32_round()))
        {
            constraint.activate();
        }
    });
}

impl Dune {
    /// The closest point to `pos` that is on some output.
    fn clamp_to_outputs(&self, pos: Point<f64, Logical>) -> Point<f64, Logical> {
        let geos: Vec<_> = self.space.outputs().filter_map(|o| self.space.output_geometry(o)).collect();
        clamp_to(&geos, pos)
    }

    /// Slide the strip of the output under `pos` by `dx` pixels (none: nothing).
    fn slide_strip(&mut self, pos: Point<f64, Logical>, dx: i32) {
        if dx == 0 {
            return;
        }
        let Some(name) = self.space.output_under(pos).next().map(|o| o.name()) else { return };
        let focus = self.focused;
        if let Some(layout) = self.layouts.get_mut(&name) {
            layout.slide_view(dx, focus);
            self.arrange();
        }
    }

    /// Mod+right drag: the tile under the pointer follows it, right/down growing it.
    fn drag_resize(&mut self, pos: Point<f64, Logical>) {
        let Some((id, last)) = self.resize_drag else { return };
        self.resize_drag = Some((id, pos));
        if self.is_floating(id) {
            return self.resize_float(id, pos.x - last.x, pos.y - last.y);
        }
        let output = self.space.outputs().find(|o| self.layouts.get(&o.name()).is_some_and(|l| l.contains(id)));
        let Some(area) = output.and_then(|o| self.space.output_geometry(o)) else { return };
        let (dx, dy) = ((pos.x - last.x) / area.size.w as f64, (pos.y - last.y) / area.size.h as f64);
        self.change_tile(id, |l, id| {
            l.resize_width(id, dx);
            l.resize_height(id, dy);
        });
    }

    /// A resize key or Super+scroll: a float grows by `dw`/`dh` around its middle, a tile in
    /// the strip takes `change`.
    fn resize_tile(&mut self, id: TileId, dw: f64, dh: f64, change: impl Fn(&mut Layout, TileId)) {
        if self.is_floating(id) {
            self.scale_float(id, dw, dh);
        } else {
            // Keys and wheel jump a whole step: the tiles slide there instead.
            self.animate_resize();
            self.change_tile(id, change);
        }
    }

    fn resize_focused(&mut self, dw: f64, dh: f64, change: impl Fn(&mut Layout, TileId)) {
        if let Some(id) = self.focused {
            self.resize_tile(id, dw, dh, change);
        }
    }

    /// `focus_follows_mouse`: the tile under the pointer takes focus, unless part of it is
    /// off screen (focusing it would slide the view under the pointer), a button or drag
    /// is going on, dune-finder is open or the session is locked.
    fn hover_focus(&mut self, pos: Point<f64, Logical>) {
        let busy = self.move_drag.is_some()
            || self.resize_drag.is_some()
            || self.finder.is_some()
            || self.session_lock.is_locked();
        if !self.config.focus_follows_mouse || busy || self.seat.get_pointer().unwrap().is_grabbed()
        {
            return;
        }
        let Some((id, shown)) = self.shown.iter().find(|(_, r)| r.contains(pos.x, pos.y)).copied() else { return };
        let whole = self.geometries.iter().any(|(t, r)| *t == id && *r == shown);
        if whole && self.focused != Some(id) {
            self.focus_tile(id);
        }
    }

    /// Mod held, no drag going on and the pointer near an edge of a tile of the strip: that
    /// tile, the side, and the band along that edge to light (`split_hint_at`).
    pub fn split_hint(&self) -> Option<(TileId, Side, Rect)> {
        if self.resize_drag.is_some()
            || self.finder.is_some()
            || self.move_drag.is_some_and(|d| d.moved)
            || self.session_lock.is_locked()
        {
            return None;
        }
        let mods = self.seat.get_keyboard()?.modifier_state();
        if !modifier_held(&self.config.modifier, &mods) {
            return None;
        }
        self.split_hint_at(self.seat.get_pointer()?.current_location())
    }

    /// The tile of the strip under `pos`, if `pos` is near one of its edges (`split_edge`).
    fn split_hint_at(&self, pos: Point<f64, Logical>) -> Option<(TileId, Side, Rect)> {
        let (id, rect) = self.shown.iter().find(|(_, r)| r.contains(pos.x, pos.y)).copied()?;
        if self.is_floating(id) {
            return None;
        }
        let side = split_edge(&rect, pos.x, pos.y)?;
        Some((id, side, split_band(&rect, side)))
    }

    /// Mod+left drag: past a few pixels it is a drag, not a click; a float follows the pointer.
    fn drag_move(&mut self, pos: Point<f64, Logical>) {
        let Some(drag) = self.move_drag.as_mut() else { return };
        if (pos.x - drag.start.x).hypot(pos.y - drag.start.y) > CLICK_SLOP {
            drag.moved = true;
        }
        let drag = *drag;
        if let (true, Some(from)) = (drag.moved, drag.from) {
            self.drag_float(drag.tile, from, drag.start, pos);
        }
    }

    fn run_action(&mut self, action: Action) {
        match action {
            Action::EmptyContainer => self.open_empty_container(),
            Action::Launcher => self.toggle_finder(FinderMode::Apps),
            Action::Switcher => self.toggle_finder(FinderMode::Windows),
            Action::TileOptions => self.toggle_finder(FinderMode::TileOptions),
            Action::PrivateTile => self.toggle_private_focused(),
            Action::Focus(dir) => self.focus_direction(dir),
            Action::Swap(dir) => self.swap_direction(dir),
            Action::Width(delta) => self.resize_focused(delta, 0.0, |l, id| l.resize_width(id, delta)),
            Action::Height(delta) => self.resize_focused(0.0, delta, |l, id| l.resize_height(id, delta)),
            Action::RowWidth(delta) => self.resize_focused(delta, 0.0, |l, id| l.resize_row_width(id, delta)),
            Action::CycleWidth => self.change_focused(Layout::cycle_width),
            Action::FullWidth => self.change_focused(Layout::toggle_full_width),
            Action::Center => self.change_focused(Layout::toggle_center),
            Action::ToggleFloat => {
                if let Some(id) = self.focused {
                    self.toggle_floating(id);
                }
            }
            Action::Close => self.close_focused(),
            Action::Quit => self.loop_signal.stop(),
            Action::CycleTab(delta) => self.cycle_tab(delta),
            Action::MoveTab(dir) => self.move_tab(dir),
            Action::StackToTabs => self.stack_to_tabs(),
            Action::Terminal(here) => self.terminal_here(here),
            Action::Desk(n) => self.show_desk(n),
            Action::SendToDesk(n) => self.send_to_desk(n),
            Action::Lock => crate::autostart(&self.config.lock_command),
            Action::SwitchVt(vt) => self.change_vt(vt),
        }
    }

    /// A row chosen in the tile options menu: `action` on `tile`, focused first (the actions
    /// work on the focused tile). Nothing without one, or with one on a locked desktop.
    pub fn run_tile_action(&mut self, tile: Option<TileId>, action: TileAction) {
        let Some(tile) = tile.filter(|t| self.tiles.contains_key(t) && !self.on_locked_desk(*t)) else { return };
        if action == TileAction::PrivateTile {
            return self.toggle_private_tile(tile);
        }
        if self.focused != Some(tile) {
            self.focus_tile(tile);
        }
        self.run_action(crate::privacy::binding_of(action).into());
    }

    /// A key press or release: compositor keybindings first, then the focused client.
    pub fn handle_key(&mut self, keycode: Keycode, state: KeyState, time: InputTime) {
        let serial = SERIAL_COUNTER.next_serial();
        let pressed = state == KeyState::Pressed;
        let config = self.config.clone();
        // While dune-finder is open, Mod+Enter is its own (files: the file manager).
        let finder_open = self.finder.is_some();
        let locked = self.session_lock.is_locked();

        let action = self.seat.get_keyboard().unwrap().input::<Action, _>(
            self,
            keycode,
            state,
            serial,
            time,
            |_, mods, handle| match handle
                .raw_latin_sym_or_raw_current_sym()
                .and_then(|sym| action_for(&config, mods, sym))
            {
                Some(Action::EmptyContainer) if finder_open => FilterResult::Forward,
                Some(action) if locked && !allowed_while_locked(&action) => FilterResult::Forward,
                Some(action) if pressed => FilterResult::Intercept(action),
                _ => FilterResult::Forward,
            },
        );
        if let Some(action) = action {
            self.run_action(action);
        }
    }

    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        // Any input but devices coming and going restarts swayidle's timers.
        if !matches!(event, InputEvent::DeviceAdded { .. } | InputEvent::DeviceRemoved { .. }) {
            self.idle_notifier_state.notify_activity(&self.seat);
        }
        let locked = self.session_lock.is_locked();
        match event {
            InputEvent::Keyboard { event, .. } => {
                self.handle_key(event.key_code(), event.state(), Event::time(&event));
            }
            InputEvent::PointerMotion { event, .. } => {
                // Relative motion (mice, touchpads on a real session), kept on the screens.
                let pointer = self.seat.get_pointer().unwrap();
                let current = pointer.current_location();
                let before = self.surface_under(current);
                let constraint = before.as_ref().and_then(|(surface, origin)| active_constraint(surface, &pointer, current - *origin));
                pointer.relative_motion(
                    self,
                    before.clone(),
                    &RelativeMotionEvent { delta: event.delta(), delta_unaccel: event.delta_unaccel(), time: event.time() },
                );
                // A locked pointer stays where it is: the game only gets the raw motion.
                if matches!(constraint, Some(Constraint::Locked)) {
                    return pointer.frame(self);
                }
                let wanted = current + event.delta();
                let mut pos = self.clamp_to_outputs(wanted);
                // Pushing on against a left/right screen edge slides the strip there (not
                // for a pointer a game confines, nor while locked).
                if constraint.is_none() && !locked {
                    let dx = edge_slide(&mut self.edge_push, (pos.x - current.x).abs() < 0.5, wanted.x - pos.x);
                    self.slide_strip(pos, dx);
                }
                // A confined one stays on its surface, inside the region.
                if let (Some(Constraint::Confined(region)), Some((surface, origin))) = (&constraint, &before) {
                    let inside = |p: Point<f64, Logical>| region.as_ref().is_none_or(|r| r.contains((p - *origin).to_i32_round()));
                    if !inside(Point::from((pos.x, current.y))) {
                        pos.x = current.x;
                    }
                    if !inside(Point::from((current.x, pos.y))) {
                        pos.y = current.y;
                    }
                    if self.surface_under(pos).is_none_or(|(s, _)| s != *surface) {
                        return pointer.frame(self);
                    }
                }
                self.pointer_at(pos);
                self.drag_resize(pos);
                self.drag_move(pos);
                let under = self.surface_under(pos);
                pointer.motion(
                    self,
                    under.clone(),
                    &MotionEvent {
                        location: pos,
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time(),
                    },
                );
                pointer.frame(self);
                if let Some((surface, origin)) = under {
                    activate_constraint(&surface, &pointer, pos - origin);
                }
                self.hover_focus(pos);
            }
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let output = self.space.outputs().next().unwrap();
                let output_geo = self.space.output_geometry(output).unwrap();
                let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();
                self.pointer_at(pos);
                self.drag_resize(pos);
                self.drag_move(pos);

                let serial = SERIAL_COUNTER.next_serial();
                let pointer = self.seat.get_pointer().unwrap();
                let under = self.surface_under(pos);

                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial,
                        time: event.time(),
                    },
                );
                pointer.frame(self);
                self.hover_focus(pos);
            }
            InputEvent::PointerButton { event, .. } => {
                let pointer = self.seat.get_pointer().unwrap();
                let serial = SERIAL_COUNTER.next_serial();
                let button_state = event.state();

                // Locked: the click only goes to the lock surface.
                if locked {
                    let button = ButtonEvent { button: event.button_code(), state: button_state, serial, time: event.time() };
                    pointer.button(self, &button);
                    return pointer.frame(self);
                }

                // A click away from dune-finder closes it, and goes no further.
                if button_state == ButtonState::Pressed
                    && self.finder_under(pointer.current_location()) == Some(false)
                {
                    return self.close_finder();
                }

                // Mod+right button drags resize the tile instead of reaching the client.
                let mods = self.seat.get_keyboard().unwrap().modifier_state();
                if event.button_code() == BTN_RIGHT {
                    if button_state == ButtonState::Released && self.resize_drag.take().is_some() {
                        return;
                    }
                    let pos = pointer.current_location();
                    if button_state == ButtonState::Pressed
                        && modifier_held(&self.config.modifier, &mods)
                        && let Some(id) = self.tile_at(pos.x, pos.y)
                    {
                        self.focus_tile(id);
                        self.resize_drag = Some((id, pos));
                        return;
                    }
                }

                // Mod+left button drags move the tile: it lands where the button is released,
                // on this monitor or another one; a float just goes where it is dragged. Let go
                // on a tab bar, either one joins that container as a tab. A tab grabbed in its
                // bar and let go there takes the place under the pointer.
                // A Mod+click without moving floats the tile, or puts a float back; near an
                // edge of a tile of the strip (`split_hint`), it opens an empty split there.
                if event.button_code() == BTN_LEFT {
                    let pos = pointer.current_location();
                    if button_state == ButtonState::Released
                        && let Some(drag) = self.move_drag.take()
                    {
                        if !drag.moved {
                            match drag.split {
                                Some(side) => self.open_empty_split(drag.tile, side),
                                None => self.toggle_floating(drag.tile),
                            }
                        } else if drag.from.is_none() {
                            self.drop_tile(drag.tile, pos);
                        } else if let Some(target) = self.drop_spot(drag.tile, pos).filter(|s| s.tab).and_then(|s| s.target) {
                            // A float let go on a tab bar becomes a tab there.
                            self.join_tab(drag.tile, target);
                        }
                        return;
                    }
                    if button_state == ButtonState::Pressed
                        && modifier_held(&self.config.modifier, &mods)
                        && let Some(id) = self.tile_at(pos.x, pos.y).or_else(|| self.tab_at(pos.x, pos.y))
                    {
                        let split = self.split_hint_at(pos).filter(|(t, ..)| *t == id).map(|(_, side, _)| side);
                        self.focus_tile(id);
                        let from = self.float_rect(id);
                        self.move_drag = Some(MoveDrag { tile: id, start: pos, moved: false, from, split });
                        return;
                    }
                }

                // A click on the "+" of a tab bar: a new empty tab there, with dune-finder.
                if event.button_code() == BTN_LEFT
                    && button_state == ButtonState::Pressed
                    && !pointer.is_grabbed()
                {
                    let pos = pointer.current_location();
                    if let Some(target) = self.plus_at(pos.x, pos.y) {
                        return self.open_empty_tab(target);
                    }
                }

                // Click to focus.
                if ButtonState::Pressed == button_state && !pointer.is_grabbed() {
                    let pos = pointer.current_location();
                    if let Some(id) = self.tab_at(pos.x, pos.y).or_else(|| self.tile_at(pos.x, pos.y)) {
                        if self.focused != Some(id) {
                            self.focus_tile(id);
                        }
                    }
                };

                pointer.button(
                    self,
                    &ButtonEvent {
                        button: event.button_code(),
                        state: button_state,
                        serial,
                        time: event.time(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerAxis { event, .. } => {
                let source = event.source();

                let horizontal_amount = event
                    .amount(Axis::Horizontal)
                    .unwrap_or_else(|| event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.);
                let vertical_amount = event
                    .amount(Axis::Vertical)
                    .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.);
                let horizontal_amount_discrete = event.amount_v120(Axis::Horizontal);
                let vertical_amount_discrete = event.amount_v120(Axis::Vertical);

                let mut frame = AxisFrame::new(event.time()).source(source);
                if horizontal_amount != 0.0 {
                    frame = frame.value(Axis::Horizontal, horizontal_amount);
                    if let Some(discrete) = horizontal_amount_discrete {
                        frame = frame.v120(Axis::Horizontal, discrete as i32);
                    }
                }
                if vertical_amount != 0.0 {
                    frame = frame.value(Axis::Vertical, vertical_amount);
                    if let Some(discrete) = vertical_amount_discrete {
                        frame = frame.v120(Axis::Vertical, discrete as i32);
                    }
                }

                if source == AxisSource::Finger {
                    if event.amount(Axis::Horizontal) == Some(0.0) {
                        frame = frame.stop(Axis::Horizontal);
                    }
                    if event.amount(Axis::Vertical) == Some(0.0) {
                        frame = frame.stop(Axis::Vertical);
                    }
                }

                // Super+scroll resizes the tile under the pointer: the column's width, or with
                // Shift its height. Always Super, whatever the bindings modifier. Not while locked.
                let mods = self.seat.get_keyboard().unwrap().modifier_state();
                if !locked && mods.logo && vertical_amount != 0.0 {
                    let pos = self.seat.get_pointer().unwrap().current_location();
                    if let Some(id) = self.tile_at(pos.x, pos.y).or(self.focused) {
                        let delta = scroll_resize_delta(vertical_amount);
                        if mods.shift {
                            self.resize_tile(id, 0.0, delta, |l, id| l.resize_height(id, delta));
                        } else {
                            self.resize_tile(id, delta, 0.0, |l, id| l.resize_width(id, delta));
                        }
                    }
                    return;
                }

                // Alt+scroll over a tile with neighbours side by side resizes it inside its
                // row; anywhere else the scroll goes to the window.
                if !locked && mods.alt && !mods.ctrl && vertical_amount != 0.0 {
                    let pos = self.seat.get_pointer().unwrap().current_location();
                    let tile = self.tile_at(pos.x, pos.y);
                    if let Some(id) = tile.filter(|id| self.layouts.values().any(|l| l.shares_row(*id))) {
                        let delta = scroll_resize_delta(vertical_amount);
                        self.change_tile(id, |l, id| l.resize_row_width(id, delta));
                        return;
                    }
                }

                let pointer = self.seat.get_pointer().unwrap();
                pointer.axis(self, frame);
                pointer.frame(self);
            }
            _ => {}
        }
    }
}
