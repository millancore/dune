use crate::{Dune, state::ClientState};
use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    reexports::wayland_server::{
        Client,
        protocol::{wl_buffer, wl_surface::WlSurface},
    },
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, get_parent, is_sync_subsurface,
        },
        shm::{ShmHandler, ShmState},
    },
};

use super::xdg_shell;

impl CompositorHandler for Dune {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self
                .space
                .elements()
                .find(|w| w.toplevel().unwrap().wl_surface() == &root)
            {
                window.on_commit();
            }
        };

        // A dialog floats before its first configure, so its first size is its float size.
        let toplevel = self.space.elements().find_map(|w| w.toplevel().filter(|t| t.wl_surface() == surface).cloned());
        if let Some(toplevel) = toplevel {
            self.float_if_dialog(&toplevel);
            self.size_committed(&toplevel);
        }
        xdg_shell::handle_commit(&mut self.popups, &self.space, surface);
        self.layer_commit(surface);
    }
}

impl BufferHandler for Dune {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for Dune {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}
