use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent, InputTime, KeyState,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    },
    input::{
        keyboard::{FilterResult, Keycode, Keysym, ModifiersState},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    utils::SERIAL_COUNTER,
};

use crate::{config::Config, layout::Direction, state::Dune};

/// Compositor keybindings, all behind the modifier chosen with `DUNE_MOD`.
#[derive(Debug, Clone, Copy)]
enum Action {
    NewTerminal,
    Focus(Direction),
    Swap(Direction),
    Grow,
    Shrink,
    Close,
    Quit,
    Direction(Direction),
}

fn modifier_held(modifier: &str, mods: &ModifiersState) -> bool {
    match modifier {
        "super" | "logo" => mods.logo,
        "ctrl+alt" => mods.ctrl && mods.alt,
        _ => mods.alt,
    }
}

fn action_for(config: &Config, mods: &ModifiersState, sym: Keysym) -> Option<Action> {
    // Direction indicator keys first: they may use the same modifier as the rest.
    if let Some((side, _)) = config.direction_keys.iter().find(|(_, key)| key.matches(mods, sym)) {
        return Some(Action::Direction(*side));
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
    match sym {
        Keysym::Return => Some(Action::NewTerminal),
        Keysym::equal | Keysym::plus => Some(Action::Grow),
        Keysym::minus => Some(Action::Shrink),
        Keysym::q => Some(Action::Close),
        Keysym::e if mods.shift => Some(Action::Quit),
        _ => None,
    }
}

impl Dune {
    fn run_action(&mut self, action: Action) {
        match action {
            Action::NewTerminal => self.spawn_terminal(None, None, None, None),
            Action::Focus(dir) => self.focus_direction(dir),
            Action::Swap(dir) => self.swap_direction(dir),
            Action::Grow => self.resize_focused(0.05),
            Action::Shrink => self.resize_focused(-0.05),
            Action::Close => self.close_focused(),
            Action::Quit => self.loop_signal.stop(),
            Action::Direction(side) => self.toggle_direction(side),
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
            |_, mods, handle| {
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
            InputEvent::PointerMotion { .. } => {}
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let output = self.space.outputs().next().unwrap();
                let output_geo = self.space.output_geometry(output).unwrap();
                let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();

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

                let pointer = self.seat.get_pointer().unwrap();
                pointer.axis(self, frame);
                pointer.frame(self);
            }
            _ => {}
        }
    }
}
