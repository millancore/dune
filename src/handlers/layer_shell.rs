//! wlr-layer-shell: bars, notifications, launchers and wallpapers.
//!
//! Layers live on an output's layer map (smithay renders them: background/bottom below
//! the tiles, top/overlay above). An exclusive zone (a bar) shrinks the area the tiles
//! get; a layer that wants the keyboard (a launcher) gets it while it is open.

use smithay::{
    desktop::{LayerSurface, WindowSurfaceType, layer_map_for_output},
    output::Output,
    reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface},
    wayland::{
        compositor::with_states,
        shell::wlr_layer::{
            KeyboardInteractivity, Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler,
            WlrLayerShellState,
        },
    },
};

use crate::Dune;

impl WlrLayerShellHandler for Dune {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(&mut self, surface: WlrLayerSurface, output: Option<WlOutput>, _layer: Layer, namespace: String) {
        let Some(output) = output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.space.outputs().next().cloned())
        else {
            return;
        };
        tracing::info!(namespace, "new layer surface");
        let mut map = layer_map_for_output(&output);
        if let Err(err) = map.map_layer(&LayerSurface::new(surface, namespace)) {
            tracing::warn!("cannot map layer surface: {err}");
        }
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        for output in outputs {
            let mut map = layer_map_for_output(&output);
            let layer = map.layers().find(|l| l.layer_surface() == &surface).cloned();
            if let Some(layer) = layer {
                map.unmap_layer(&layer);
            }
        }
        if self.layer_focus.as_ref() == Some(surface.wl_surface()) {
            self.layer_focus = None;
        }
        // Its exclusive zone is gone: the tiles get the space back.
        self.arrange();
        self.update_focus();
    }
}

impl Dune {
    /// Should be called on every commit: configures new layers and rearranges.
    pub fn layer_commit(&mut self, surface: &WlSurface) {
        let Some(output) = self
            .space
            .outputs()
            .find(|o| layer_map_for_output(o).layer_for_surface(surface, WindowSurfaceType::TOPLEVEL).is_some())
            .cloned()
        else {
            return;
        };

        let initial_configure_sent = with_states(surface, |states| {
            states
                .data_map
                .get::<LayerSurfaceData>()
                .unwrap()
                .lock()
                .unwrap()
                .initial_configure_sent
        });
        let wants_keyboard = {
            let mut map = layer_map_for_output(&output);
            // Arrange before the initial configure to honour the size the client asked for.
            map.arrange();
            let layer = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL).unwrap();
            if !initial_configure_sent {
                layer.layer_surface().send_configure();
            }
            let state = layer.cached_state();
            matches!(state.layer, Layer::Top | Layer::Overlay)
                && state.keyboard_interactivity == KeyboardInteractivity::Exclusive
        };

        // A launcher or lock-like layer takes the keyboard until it closes.
        if wants_keyboard && self.layer_focus.as_ref() != Some(surface) {
            tracing::info!("layer takes the keyboard");
            self.layer_focus = Some(surface.clone());
            self.update_focus();
        } else if !wants_keyboard && self.layer_focus.as_ref() == Some(surface) {
            self.layer_focus = None;
            self.update_focus();
        }
        // Exclusive zones may have changed.
        self.arrange();
    }
}
