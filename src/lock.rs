//! Screen lock (swaylock): the `ext-session-lock-v1` protocol.
//!
//! While locked, each output shows only its lock surface over black (and the pointer):
//! no tiles, layers, bars or decorations. The keyboard goes to the lock surface of the
//! active output, the pointer only reaches lock surfaces, and compositor keys are off
//! except VT switching. The locker hears `locked` once every output drew such a frame.
//! A locker that dies without unlocking leaves the screens black and locked; another one
//! may take over then.

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use smithay::{
    backend::{
        input::InputTime,
        renderer::{
            element::{
                Kind,
                solid::{SolidColorBuffer, SolidColorRenderElement},
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            },
            gles::GlesRenderer,
        },
    },
    desktop::{WindowSurfaceType, utils::under_from_surface_tree},
    input::pointer::{CursorImageStatus, MotionEvent},
    output::Output,
    reexports::{
        wayland_protocols::ext::session_lock::v1::server::ext_session_lock_v1::ExtSessionLockV1,
        wayland_server::{
            DisplayHandle, Resource,
            protocol::{wl_output::WlOutput, wl_surface::WlSurface},
        },
    },
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::session_lock::{LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker},
};

use crate::{Dune, render::OutputRenderElements};

/// What a locked output shows where its lock surface does not cover it (or has none).
const LOCKED_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

enum Phase {
    Unlocked,
    /// A lock was asked for: the screens are black already; the locker hears `locked` once
    /// all the outputs (by name) are in `drawn`.
    Locking { locker: SessionLocker, drawn: HashSet<String> },
    Locked,
}

pub struct SessionLock {
    manager: SessionLockManagerState,
    phase: Phase,
    /// The lock of the client holding it (locking or locked): a dead one may be replaced.
    holder: Option<ExtSessionLockV1>,
    /// The holder's lock surfaces, by output name.
    surfaces: HashMap<String, LockSurface>,
    /// The black under each output's lock surface, by output name.
    backdrops: HashMap<String, SolidColorBuffer>,
}

impl SessionLock {
    pub fn new(dh: &DisplayHandle) -> Self {
        SessionLock {
            manager: SessionLockManagerState::new::<Dune, _>(dh, |_| true),
            phase: Phase::Unlocked,
            holder: None,
            surfaces: HashMap::new(),
            backdrops: HashMap::new(),
        }
    }

    /// Locked, or on the way (the screens are black from the lock request on).
    pub fn is_locked(&self) -> bool {
        !matches!(self.phase, Phase::Unlocked)
    }

    /// The lock surface of output `name`, while its client keeps it.
    fn surface(&self, name: &str) -> Option<&LockSurface> {
        self.surfaces.get(name).filter(|s| s.alive())
    }
}

/// What a new lock request gets.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    /// Not locked: lock now.
    Lock,
    /// Already locked by a client that died without unlocking: the new one takes over.
    TakeOver,
    /// Already locked by a live client.
    Refuse,
}

fn answer(locked: bool, holder_alive: bool) -> Answer {
    match (locked, holder_alive) {
        (false, _) => Answer::Lock,
        (true, false) => Answer::TakeOver,
        (true, true) => Answer::Refuse,
    }
}

/// Whether every one of `outputs` drew a locked frame.
fn all_drawn(drawn: &HashSet<String>, outputs: &[String]) -> bool {
    outputs.iter().all(|o| drawn.contains(o))
}

impl SessionLockHandler for Dune {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock.manager
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        let lock = &mut self.session_lock;
        let holder_alive = lock.holder.as_ref().is_some_and(Resource::is_alive);
        match answer(lock.is_locked(), holder_alive) {
            // Dropping the locker tells its client the lock failed.
            Answer::Refuse => tracing::warn!("refusing a second session lock: the session is locked already"),
            Answer::TakeOver => {
                tracing::info!("a new locker takes over the session lock");
                lock.holder = Some(confirmation.ext_session_lock().clone());
                lock.surfaces.clear();
                match std::mem::replace(&mut lock.phase, Phase::Locked) {
                    // The screens are still on their way to black: the new locker waits for them too.
                    Phase::Locking { drawn, .. } => lock.phase = Phase::Locking { locker: confirmation, drawn },
                    // They are black already.
                    _ => confirmation.lock(),
                }
            }
            Answer::Lock => {
                tracing::info!("session locking");
                lock.holder = Some(confirmation.ext_session_lock().clone());
                lock.surfaces.clear();
                lock.phase = Phase::Locking { locker: confirmation, drawn: HashSet::new() };
                self.enter_lock();
                // Without outputs there is nothing to wait for.
                self.confirm_lock();
            }
        }
    }

    fn unlock(&mut self) {
        tracing::info!("session unlocked");
        let lock = &mut self.session_lock;
        lock.phase = Phase::Unlocked;
        lock.holder = None;
        lock.surfaces.clear();
        // Monitors may have changed while locked; the focus goes back where it was.
        self.arrange();
        self.update_focus();
        self.refresh_pointer_focus();
    }

    fn new_surface(&mut self, surface: LockSurface, output: WlOutput) {
        // Surfaces of a refused locker are not shown.
        if self.session_lock.holder.as_ref() != Some(surface.ext_session_lock()) {
            return;
        }
        let Some(output) = Output::from_resource(&output) else { return };
        let Some(geo) = self.space.output_geometry(&output) else { return };
        // smithay sends this configure right after.
        surface.with_pending_state(|state| state.size = Some((geo.size.w as u32, geo.size.h as u32).into()));
        self.session_lock.surfaces.insert(output.name(), surface);
        self.update_focus();
        self.refresh_pointer_focus();
    }
}

impl Dune {
    /// The lock starts: drags and grabs end, and keyboard and pointer leave the clients.
    fn enter_lock(&mut self) {
        self.move_drag = None;
        self.resize_drag = None;
        self.edge_push = 0.0;
        // The pointer image a window chose goes too.
        self.cursor_status = CursorImageStatus::default_named();
        let pointer = self.seat.get_pointer().unwrap();
        pointer.unset_grab(self, SERIAL_COUNTER.next_serial(), InputTime::now());
        self.seat.get_keyboard().unwrap().unset_grab(self);
        self.update_focus();
        self.refresh_pointer_focus();
    }

    /// Point the pointer focus at what is under it now (only lock surfaces while locked).
    fn refresh_pointer_focus(&mut self) {
        let pointer = self.seat.get_pointer().unwrap();
        let location = pointer.current_location();
        let under = self.surface_under(location);
        let event = MotionEvent { location, serial: SERIAL_COUNTER.next_serial(), time: InputTime::now() };
        pointer.motion(self, under, &event);
        pointer.frame(self);
    }

    /// While locked, where the keyboard goes: the lock surface of the active output, else
    /// any lock surface there is.
    pub fn lock_focus(&self) -> Option<WlSurface> {
        let lock = &self.session_lock;
        let active = self.active_output().map(|o| o.name());
        active
            .and_then(|name| lock.surface(&name))
            .or_else(|| lock.surfaces.values().find(|s| s.alive()))
            .map(|s| s.wl_surface().clone())
    }

    /// While locked, the surface under `pos`: the lock surface of the output there.
    pub fn lock_surface_under(&self, pos: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        let output = self.space.output_under(pos).next()?;
        let origin = self.space.output_geometry(output)?.loc;
        let surface = self.session_lock.surface(&output.name())?;
        under_from_surface_tree(surface.wl_surface(), pos, origin, WindowSurfaceType::ALL).map(|(s, p)| (s, p.to_f64()))
    }

    /// What a locked `output` shows, front first: the pointer, its lock surface, black.
    pub fn lock_elements(&mut self, renderer: &mut GlesRenderer, output: &Output, with_pointer: bool) -> Vec<OutputRenderElements> {
        let mut elements = if with_pointer { self.pointer_elements(renderer, output) } else { Vec::new() };
        let Some(geo) = self.space.output_geometry(output) else { return elements };
        let name = output.name();
        if let Some(surface) = self.session_lock.surface(&name) {
            // A monitor that changed size: the lock surface follows (nothing is sent otherwise).
            surface.with_pending_state(|state| state.size = Some((geo.size.w as u32, geo.size.h as u32).into()));
            surface.send_configure();
            let surfaces: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                render_elements_from_surface_tree(renderer, surface.wl_surface(), (0, 0), 1.0, 1.0, Kind::Unspecified);
            elements.extend(surfaces.into_iter().map(OutputRenderElements::Surface));
        }
        let backdrop = self.session_lock.backdrops.entry(name).or_default();
        backdrop.update(geo.size, LOCKED_COLOR);
        let black = SolidColorRenderElement::from_buffer(backdrop, (0, 0), 1.0, 1.0, Kind::Unspecified);
        elements.push(OutputRenderElements::Solid(black));
        elements
    }

    /// After a frame on `output`: its lock surface may draw the next one.
    pub fn send_lock_frames(&self, output: &Output, time: Duration) {
        if let Some(surface) = self.session_lock.surface(&output.name()) {
            smithay::desktop::utils::send_frames_surface_tree(surface.wl_surface(), output, time, Some(Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
    }

    /// The backends showed a frame of `output` drawn while locked (`lock_elements`).
    pub fn lock_frame_drawn(&mut self, output: &Output) {
        if let Phase::Locking { drawn, .. } = &mut self.session_lock.phase {
            drawn.insert(output.name());
            self.confirm_lock();
        }
    }

    /// Tell the locker the session is locked once every output drew a locked frame.
    fn confirm_lock(&mut self) {
        let outputs: Vec<String> = self.space.outputs().map(|o| o.name()).collect();
        let lock = &mut self.session_lock;
        if !matches!(&lock.phase, Phase::Locking { drawn, .. } if all_drawn(drawn, &outputs)) {
            return;
        }
        if let Phase::Locking { locker, .. } = std::mem::replace(&mut lock.phase, Phase::Locked) {
            tracing::info!("session locked on {} output(s)", outputs.len());
            locker.lock();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_once_every_output_drew() {
        let outputs = ["DP-1".to_string(), "HDMI-A-1".to_string()];
        let mut drawn = HashSet::new();
        assert!(!all_drawn(&drawn, &outputs));
        drawn.insert("DP-1".to_string());
        assert!(!all_drawn(&drawn, &outputs));
        drawn.insert("HDMI-A-1".to_string());
        assert!(all_drawn(&drawn, &outputs));
        // A monitor unplugged while locking is not waited for; without monitors, nothing is.
        assert!(all_drawn(&drawn, &outputs[..1]));
        assert!(all_drawn(&HashSet::new(), &[]));
    }

    #[test]
    fn a_dead_locker_can_be_replaced() {
        assert_eq!(answer(false, false), Answer::Lock);
        assert_eq!(answer(false, true), Answer::Lock);
        assert_eq!(answer(true, false), Answer::TakeOver);
        assert_eq!(answer(true, true), Answer::Refuse);
    }
}
