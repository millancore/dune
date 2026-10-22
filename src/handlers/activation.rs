//! xdg-activation: a client asks for one of its windows to be brought forward, with a token
//! another client (or itself) got from us. A notification clicked in mako hands the app
//! such a token, so its window is gone to: its desktop shows, its tab, and it takes focus.

use smithay::input::Seat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::Serial;
use smithay::wayland::xdg_activation::{XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData};

use crate::Dune;
use crate::layout::TileId;

/// Tokens older than this activate nothing.
const TOKEN_LIFETIME_SECS: u64 = 10;

/// A token made from the input event `serial` may steal focus: that event came after the
/// focused window got the keyboard (`last_enter`), or nothing has it.
fn fresh(serial: Serial, last_enter: Option<Serial>) -> bool {
    last_enter.is_none_or(|enter| serial.is_no_older_than(&enter))
}

impl Dune {
    /// The tile with `surface` among its windows, on any monitor or desktop.
    fn tile_of_surface(&self, surface: &WlSurface) -> Option<TileId> {
        self.tiles
            .iter()
            .find(|(_, tile)| tile.stack.iter().any(|w| w.toplevel().is_some_and(|t| t.wl_surface() == surface)))
            .map(|(id, _)| *id)
    }
}

impl XdgActivationHandler for Dune {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.activation_state
    }

    /// Only tokens born of an input event on our seat, after the focused window got the
    /// keyboard: a click on a notification, a key in a terminal opening a link.
    fn token_created(&mut self, _token: XdgActivationToken, data: XdgActivationTokenData) -> bool {
        let app = data.app_id.clone().unwrap_or_default();
        let Some((serial, seat)) = data.serial else {
            tracing::info!(app, "activation token refused: no input event");
            return false;
        };
        let Some(seat) = Seat::<Dune>::from_resource(&seat) else { return false };
        let last_enter = seat.get_keyboard().and_then(|k| k.last_enter());
        let ok = fresh(serial, last_enter);
        tracing::info!(app, ok, "activation token asked for");
        ok
    }

    fn request_activation(&mut self, token: XdgActivationToken, data: XdgActivationTokenData, surface: WlSurface) {
        self.activation_state.remove_token(&token);
        if data.timestamp.elapsed().as_secs() >= TOKEN_LIFETIME_SECS {
            return tracing::info!("activation with an expired token");
        }
        match self.tile_of_surface(&surface) {
            Some(id) => {
                tracing::info!(tile = id, "activation: going to the window");
                self.go_to(id)
            }
            None => tracing::info!("activation of a surface that is no tile's window"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_input_after_the_focus_entered_may_activate() {
        assert!(fresh(Serial::from(5), None));
        assert!(fresh(Serial::from(7), Some(Serial::from(5))));
        assert!(fresh(Serial::from(5), Some(Serial::from(5))));
        assert!(!fresh(Serial::from(3), Some(Serial::from(5))));
    }
}
