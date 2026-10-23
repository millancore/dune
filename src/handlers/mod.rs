mod activation;
mod compositor;
mod layer_shell;
mod xdg_shell;

use crate::Dune;

//
// Wl Seat
//

use smithay::input::dnd::{DnDGrab, DndGrabHandler, GrabType, Source};
use smithay::input::pointer::Focus;
use smithay::input::tablet::TabletSeatHandler;
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::Serial;
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::selection::data_device::{
    DataDeviceHandler, DataDeviceState, WaylandDndGrabHandler, set_data_device_focus,
};
use smithay::wayland::selection::primary_selection::{
    PrimarySelectionHandler, PrimarySelectionState, set_primary_focus,
};

impl SeatHandler for Dune {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Dune> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: smithay::input::pointer::CursorImageStatus) {
        use smithay::input::pointer::CursorImageStatus::*;
        let kind = |c: &smithay::input::pointer::CursorImageStatus| match c {
            Hidden => "hidden",
            Named(_) => "named",
            Surface(_) => "surface",
        };
        // Which kind of cursor clients ask for (a game may hide it): for pointer bugs.
        if kind(&image) != kind(&self.cursor_status) {
            tracing::debug!(cursor = kind(&image), "cursor image");
        }
        self.cursor_status = image;
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client.clone());
        set_primary_focus(dh, seat, client);
    }
}

//
// Wl Data Device
//

impl SelectionHandler for Dune {
    type SelectionUserData = ();
}

impl DataDeviceHandler for Dune {
    fn data_device_state(&mut self) -> &mut DataDeviceState {
        &mut self.data_device_state
    }
}

/// Primary selection: selected text, pasted with the middle button.
impl PrimarySelectionHandler for Dune {
    fn primary_selection_state(&mut self) -> &mut PrimarySelectionState {
        &mut self.primary_selection_state
    }
}

/// Data control: `wl-copy`, `wl-paste` and clipboard managers (ext and wlr protocols).
impl smithay::wayland::selection::ext_data_control::DataControlHandler for Dune {
    fn data_control_state(&mut self) -> &mut smithay::wayland::selection::ext_data_control::DataControlState {
        &mut self.ext_data_control_state
    }
}

impl smithay::wayland::selection::wlr_data_control::DataControlHandler for Dune {
    fn data_control_state(&mut self) -> &mut smithay::wayland::selection::wlr_data_control::DataControlState {
        &mut self.wlr_data_control_state
    }
}

impl DndGrabHandler for Dune {}
impl WaylandDndGrabHandler for Dune {
    fn dnd_requested<S: Source>(
        &mut self,
        source: S,
        _icon: Option<WlSurface>,
        seat: Seat<Self>,
        serial: Serial,
        type_: GrabType,
    ) {
        match type_ {
            GrabType::Pointer => {
                let ptr = seat.get_pointer().unwrap();
                let start_data = ptr.grab_start_data().unwrap();
                let grab = DnDGrab::new_pointer(&self.display_handle, start_data, source, seat);
                ptr.set_grab(self, grab, serial, Focus::Keep);
            }
            GrabType::Touch => source.cancel(),
        }
    }
}

//
// Wl Output & Xdg Output
//

impl OutputHandler for Dune {}

//
// linux-dmabuf: GPU buffers from clients
//

use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::renderer::ImportDma;
use smithay::wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier};

impl DmabufHandler for Dune {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    /// Accept a buffer only if our renderer can actually use it.
    fn dmabuf_imported(&mut self, _global: &DmabufGlobal, dmabuf: Dmabuf, notifier: ImportNotifier) {
        let imported = self
            .backend
            .renderer()
            .is_some_and(|renderer| renderer.import_dmabuf(&dmabuf, None).is_ok());
        if imported {
            tracing::debug!("dmabuf imported");
            let _ = notifier.successful::<Dune>();
        } else {
            tracing::warn!("rejected a dmabuf the renderer cannot import");
            notifier.failed();
        }
    }
}

//
// Pointer constraints: a game locks or confines the pointer to its window
//

use smithay::input::pointer::PointerHandle;
use smithay::wayland::pointer_constraints::{PointerConstraintsHandler, with_pointer_constraint};

impl PointerConstraintsHandler for Dune {
    /// A constraint on the surface under the pointer starts right away; others when the
    /// pointer gets there (see the pointer motion handling).
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        let on_it = pointer.current_focus().as_ref() == Some(surface);
        with_pointer_constraint(surface, pointer, |constraint| {
            let kind = constraint.map(|c| match &*c {
                smithay::wayland::pointer_constraints::PointerConstraint::Locked(_) => "locked",
                smithay::wayland::pointer_constraints::PointerConstraint::Confined(_) => "confined",
            });
            tracing::debug!(?kind, on_it, "pointer constraint");
        });
        if on_it {
            with_pointer_constraint(surface, pointer, |constraint| {
                if let Some(constraint) = constraint {
                    constraint.activate();
                }
            });
        }
    }
}

//
// Idle: swayidle hears when input stops (ext-idle-notify); a video or a game holds it off
// (idle-inhibit)
//

use smithay::desktop::{WindowSurfaceType, layer_map_for_output};
use smithay::wayland::compositor::get_parent;
use smithay::wayland::idle_inhibit::IdleInhibitHandler;
use smithay::wayland::idle_notify::{IdleNotifierHandler, IdleNotifierState};

impl IdleNotifierHandler for Dune {
    fn idle_notifier_state(&mut self) -> &mut IdleNotifierState<Self> {
        &mut self.idle_notifier_state
    }
}

impl IdleInhibitHandler for Dune {
    fn inhibit(&mut self, surface: WlSurface) {
        if !self.idle_inhibitors.contains(&surface) {
            self.idle_inhibitors.push(surface);
        }
        self.refresh_idle_inhibit();
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitors.retain(|s| *s != surface);
        self.refresh_idle_inhibit();
    }
}

impl Dune {
    /// Idle is held off while a surface that asked for it is alive and shown: on a window
    /// mapped on screen (not a hidden tab or desktop, nor under another window) or a layer. Called
    /// after every frame too, as surfaces die or get hidden without telling.
    pub fn refresh_idle_inhibit(&mut self) {
        self.idle_inhibitors.retain(|s| s.is_alive());
        let inhibited = self.idle_inhibitors.iter().any(|s| self.is_shown_surface(s));
        self.idle_notifier_state.set_is_inhibited(inhibited);
    }

    /// Whether `surface` (or the surface it is a subsurface of) is a shown window or a layer.
    fn is_shown_surface(&self, surface: &WlSurface) -> bool {
        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }
        self.space.elements().any(|w| w.toplevel().is_some_and(|t| *t.wl_surface() == root))
            || self
                .space
                .outputs()
                .any(|o| layer_map_for_output(o).layer_for_surface(&root, WindowSurfaceType::TOPLEVEL).is_some())
    }
}

// For cursor-shape (tablet tools are not offered).
impl TabletSeatHandler for Dune {
    type ToolFocus = WlSurface;
}

smithay::delegate_dispatch2!(Dune);
