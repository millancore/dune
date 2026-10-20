use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent, InputTime, KeyState,
        PointerMotionEvent,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    },
    input::{
        keyboard::{FilterResult, Keycode, Keysym, ModifiersState},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
};

use crate::{
    config::Config,
    layout::{Direction, Layout},
    state::{Dune, MoveDrag},
};

/// Compositor keybindings, all behind the modifier chosen with `DUNE_MOD`.
#[derive(Debug, Clone, Copy)]
enum Action {
    NewTerminal,
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
    Direction(Direction),
    /// Mod+d, or the `launcher` key: show the launcher (again: hide it).
    Launcher,
    /// A key pressed while the launcher is open, with the character it types.
    LauncherKey(ModifiersState, Keysym, Option<char>),
    /// Keys released while the launcher is open go nowhere.
    Swallowed,
    /// Mod+1..9: show that desktop on the active monitor.
    Desk(u8),
    /// Mod+Shift+1..9: send the focused tile to that desktop.
    SendToDesk(u8),
    /// Ctrl+Alt+F1..F12 on a real session.
    SwitchVt(i32),
}

/// How much one press of the resize keys changes a width or height.
const RESIZE_STEP: f64 = 0.05;
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
/// How far the pointer may move during a Mod+click before it counts as a drag.
const CLICK_SLOP: f64 = 4.0;
/// Scroll distance of one wheel notch (as smithay reports it).
const SCROLL_NOTCH: f64 = 15.0;

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
    // Direction indicator keys first: they may use the same modifier as the rest.
    if let Some((side, _)) = config.direction_keys.iter().find(|(_, key)| key.matches(mods, sym)) {
        return Some(Action::Direction(*side));
    }
    if config.close_key.is_some_and(|key| key.matches(mods, sym)) {
        return Some(Action::Close);
    }
    if config.launcher_key.is_some_and(|key| key.matches(mods, sym)) {
        return Some(Action::Launcher);
    }
    let modifier = config.modifier.as_str();
    if !modifier_held(modifier, mods) {
        return None;
    }
    let dir = match sym {
        Keysym::h | Keysym::Left => Some(Direction::Left),
        Keysym::j | Keysym::Down => Some(Direction::Down),
        Keysym::k | Keysym::Up => Some(Direction::Up),
        Keysym::l | Keysym::Right => Some(Direction::Right),
        _ => None,
    };
    if let Some(dir) = dir {
        return Some(if mods.shift { Action::Swap(dir) } else { Action::Focus(dir) });
    }
    // The key itself, unshifted: Mod+Shift+1 is `1` with shift held on any layout.
    let digit = sym.raw().wrapping_sub(Keysym::_1.raw());
    if digit < 9 {
        let n = digit as u8 + 1;
        return Some(if mods.shift { Action::SendToDesk(n) } else { Action::Desk(n) });
    }
    let row_mod = mods.ctrl && modifier != "ctrl+alt";
    match sym {
        Keysym::Return => Some(Action::NewTerminal),
        // Ctrl: the tile's width inside its row (unless Ctrl is part of the modifier).
        Keysym::equal | Keysym::plus if row_mod => Some(Action::RowWidth(RESIZE_STEP)),
        Keysym::minus if row_mod => Some(Action::RowWidth(-RESIZE_STEP)),
        Keysym::equal | Keysym::plus if mods.shift => Some(Action::Height(RESIZE_STEP)),
        Keysym::minus | Keysym::underscore if mods.shift => Some(Action::Height(-RESIZE_STEP)),
        Keysym::equal | Keysym::plus => Some(Action::Width(RESIZE_STEP)),
        Keysym::minus => Some(Action::Width(-RESIZE_STEP)),
        Keysym::d if config.launcher_key.is_none() => Some(Action::Launcher),
        Keysym::r => Some(Action::CycleWidth),
        Keysym::f => Some(Action::FullWidth),
        Keysym::space if mods.shift => Some(Action::ToggleFloat),
        Keysym::space => Some(Action::Center),
        Keysym::q => Some(Action::Close),
        Keysym::e if mods.shift => Some(Action::Quit),
        _ => None,
    }
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
    fn scroll_resize_steps() {
        assert_eq!(scroll_resize_delta(-15.0), RESIZE_STEP); // one notch up grows
        assert_eq!(scroll_resize_delta(30.0), -2.0 * RESIZE_STEP);
    }

    fn test_config(modifier: &str) -> Config {
        crate::config::Config {
            keyboard_rules: String::new(),
            keyboard_model: String::new(),
            keyboard_layout: String::new(),
            keyboard_variant: String::new(),
            keyboard_options: String::new(),
            repeat_delay: 500,
            repeat_rate: 25,
            terminal: String::new(),
            modifier: modifier.into(),
            direction_keys: Vec::new(),
            close_key: None,
            launcher_key: None,
            follow: false,
            xwayland: String::new(),
            outputs: Default::default(),
            input: Default::default(),
            font_family: String::new(),
            icon_theme: String::new(),
            exec: Vec::new(),
        }
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

impl Dune {
    /// The closest point to `pos` that is on some output.
    fn clamp_to_outputs(&self, pos: Point<f64, Logical>) -> Point<f64, Logical> {
        let geos: Vec<_> = self.space.outputs().filter_map(|o| self.space.output_geometry(o)).collect();
        clamp_to(&geos, pos)
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
            Action::NewTerminal => {
                self.spawn_terminal(None, None, None, None);
            }
            Action::Launcher => self.toggle_launcher(),
            Action::LauncherKey(mods, sym, text) => self.launcher_key(mods, sym, text),
            Action::Swallowed => {}
            Action::Focus(dir) => self.focus_direction(dir),
            Action::Swap(dir) => self.swap_direction(dir),
            Action::Width(delta) => self.change_focused(|l, id| l.resize_width(id, delta)),
            Action::Height(delta) => self.change_focused(|l, id| l.resize_height(id, delta)),
            Action::RowWidth(delta) => self.change_focused(|l, id| l.resize_row_width(id, delta)),
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
            Action::Direction(side) => self.toggle_direction(side),
            Action::Desk(n) => self.show_desk(n),
            Action::SendToDesk(n) => self.send_to_desk(n),
            Action::SwitchVt(vt) => self.change_vt(vt),
        }
    }

    /// A key press or release: compositor keybindings first, then the focused client.
    pub fn handle_key(&mut self, keycode: Keycode, state: KeyState, time: InputTime) {
        let serial = SERIAL_COUNTER.next_serial();
        let pressed = state == KeyState::Pressed;
        let config = self.config.clone();

        let action = self.seat.get_keyboard().unwrap().input::<Action, _>(
            self,
            keycode,
            state,
            serial,
            time,
            |state, mods, handle| {
                if state.launcher.is_open() {
                    // The launcher takes the keyboard; its own key still closes it.
                    let sym = handle.modified_sym();
                    let action = match action_for(&config, mods, handle.raw_latin_sym_or_raw_current_sym().unwrap_or(sym)) {
                        Some(Action::Launcher) if pressed => Action::Launcher,
                        _ if pressed => Action::LauncherKey(*mods, sym, sym.key_char()),
                        _ => Action::Swallowed,
                    };
                    return FilterResult::Intercept(action);
                }
                match handle
                    .raw_latin_sym_or_raw_current_sym()
                    .and_then(|sym| action_for(&config, mods, sym))
                {
                    Some(action) if pressed => FilterResult::Intercept(action),
                    _ => FilterResult::Forward,
                }
            },
        );
        if let Some(action) = action {
            self.run_action(action);
        }
    }

    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event, .. } => {
                self.handle_key(event.key_code(), event.state(), Event::time(&event));
            }
            InputEvent::PointerMotion { event, .. } => {
                // Relative motion (mice, touchpads on a real session), kept on the screens.
                let pointer = self.seat.get_pointer().unwrap();
                let pos = self.clamp_to_outputs(pointer.current_location() + event.delta());
                self.pointer_at(pos);
                self.drag_resize(pos);
                self.drag_move(pos);
                let under = self.surface_under(pos);
                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time(),
                    },
                );
                pointer.frame(self);
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
            }
            InputEvent::PointerButton { event, .. } => {
                let pointer = self.seat.get_pointer().unwrap();
                let serial = SERIAL_COUNTER.next_serial();
                let button_state = event.state();

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
                // on this monitor or another one; a float just goes where it is dragged.
                // A Mod+click without moving floats the tile, or puts a float back.
                if event.button_code() == BTN_LEFT {
                    let pos = pointer.current_location();
                    if button_state == ButtonState::Released
                        && let Some(drag) = self.move_drag.take()
                    {
                        if !drag.moved {
                            self.toggle_floating(drag.tile);
                        } else if drag.from.is_none() {
                            self.drop_tile(drag.tile, pos);
                        }
                        return;
                    }
                    if button_state == ButtonState::Pressed
                        && modifier_held(&self.config.modifier, &mods)
                        && let Some(id) = self.tile_at(pos.x, pos.y)
                    {
                        self.focus_tile(id);
                        let from = self.float_rect(id);
                        self.move_drag = Some(MoveDrag { tile: id, start: pos, moved: false, from });
                        return;
                    }
                }

                // Click to focus.
                if ButtonState::Pressed == button_state && !pointer.is_grabbed() {
                    let pos = pointer.current_location();
                    if let Some(id) = self.tile_at(pos.x, pos.y) {
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
                // Shift its height. Always Super, whatever the bindings modifier.
                let mods = self.seat.get_keyboard().unwrap().modifier_state();
                if mods.logo && vertical_amount != 0.0 {
                    let pos = self.seat.get_pointer().unwrap().current_location();
                    if let Some(id) = self.tile_at(pos.x, pos.y).or(self.focused) {
                        let delta = scroll_resize_delta(vertical_amount);
                        if mods.shift {
                            self.change_tile(id, |l, id| l.resize_height(id, delta));
                        } else {
                            self.change_tile(id, |l, id| l.resize_width(id, delta));
                        }
                    }
                    return;
                }

                // Alt+scroll over a tile with neighbours side by side resizes it inside its
                // row; anywhere else the scroll goes to the window.
                if mods.alt && !mods.ctrl && vertical_amount != 0.0 {
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
