mod compositor;
mod layer_shell;
mod xdg_shell;

use crate::Dune;

//
// Wl Seat
//

use smithay::input::dnd::{DnDGrab, DndGrabHandler, GrabType, Source};
use smithay::input::pointer::Focus;
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
        if pointer.current_focus().as_ref() == Some(surface) {
            with_pointer_constraint(surface, pointer, |constraint| {
                if let Some(constraint) = constraint {
                    constraint.activate();
                }
            });
        }
    }
}

smithay::delegate_dispatch2!(Dune);
