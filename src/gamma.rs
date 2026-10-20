//! Night light (wlsunset, the waybar module): the `wlr-gamma-control-unstable-v1` protocol.
//!
//! A client takes a monitor's gamma ramps and sets them from a file of `u16`s: the red
//! ramp, then green, then blue. One control per monitor at a time; when it goes away the
//! monitor gets a linear ramp back. Only a real session has ramps (the CRTC's); nested,
//! every control fails.

use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::fd::OwnedFd,
    sync::Mutex,
};

use smithay::{
    output::Output,
    reexports::{
        rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl},
        wayland_protocols_wlr::gamma_control::v1::server::{
            zwlr_gamma_control_manager_v1::{self, ZwlrGammaControlManagerV1},
            zwlr_gamma_control_v1::{self, ZwlrGammaControlV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, backend::ClientId,
        },
    },
};

use crate::{Dune, state::Backend};

/// The control each monitor (by name) has.
#[derive(Default)]
pub struct GammaState {
    controls: HashMap<String, ZwlrGammaControlV1>,
}

/// Data of a control: the monitor it sets, None once it failed or let go.
pub struct ControlData {
    output: Mutex<Option<Output>>,
}

impl GammaState {
    pub fn new(dh: &DisplayHandle) -> Self {
        dh.create_global::<Dune, ZwlrGammaControlManagerV1, ()>(1, ());
        GammaState::default()
    }
}

/// The ramps in `bytes`: exactly `3 * size` native-endian `u16`s, else None.
pub fn parse_ramps(bytes: &[u8], size: u32) -> Option<Vec<u16>> {
    if size == 0 || bytes.len() != 3 * size as usize * 2 {
        return None;
    }
    Some(bytes.chunks_exact(2).map(|b| u16::from_ne_bytes([b[0], b[1]])).collect())
}

/// Red, green and blue ramps of `size` entries that change nothing.
pub fn linear_ramps(size: usize) -> Vec<u16> {
    let last = size.saturating_sub(1).max(1);
    let ramp = (0..size).map(|i| (i * 65535 / last) as u16);
    ramp.clone().chain(ramp.clone()).chain(ramp).collect()
}

/// Read the ramps a client sent; None if the file has not exactly their size.
fn read_ramps(fd: OwnedFd, size: u32) -> Option<Vec<u16>> {
    // A pipe whose writer is still open must not hang dune.
    if let Ok(flags) = fcntl_getfl(&fd) {
        let _ = fcntl_setfl(&fd, flags | OFlags::NONBLOCK);
    }
    let mut file = File::from(fd);
    let _ = file.seek(SeekFrom::Start(0)); // pipes cannot seek
    let mut bytes = Vec::new();
    // One byte more than needed tells a file that is too long.
    if let Err(err) = file.take(3 * u64::from(size) * 2 + 1).read_to_end(&mut bytes) {
        tracing::warn!("cannot read gamma ramps: {err}");
        return None;
    }
    parse_ramps(&bytes, size)
}

impl Dune {
    /// Entries in each gamma ramp of `output`; None where its gamma cannot be set.
    pub fn gamma_size(&self, output: &Output) -> Option<u32> {
        match &self.backend {
            Backend::Udev(udev) => udev.gamma_size(output).filter(|size| *size > 0),
            _ => None,
        }
    }

    /// Set the gamma ramps of `output` (red, green and blue one after another), or a
    /// linear ramp with None.
    pub fn set_gamma(&mut self, output: &Output, ramps: Option<&[u16]>) -> Result<(), String> {
        match &mut self.backend {
            Backend::Udev(udev) => udev.set_gamma(output, ramps),
            _ => Err("no gamma ramps when nested".into()),
        }
    }

    /// A monitor went away: its control fails.
    pub fn gamma_output_removed(&mut self, output: &Output) {
        if let Some(control) = self.gamma.controls.remove(&output.name()) {
            if let Some(data) = control.data::<ControlData>() {
                data.output.lock().unwrap().take();
            }
            control.failed();
        }
    }

    /// The control lets go of its monitor, which gets a linear ramp back.
    fn release_gamma(&mut self, control: &ZwlrGammaControlV1, data: &ControlData) {
        let Some(output) = data.output.lock().unwrap().take() else { return };
        let name = output.name();
        if self.gamma.controls.get(&name) == Some(control) {
            self.gamma.controls.remove(&name);
        }
        if let Err(err) = self.set_gamma(&output, None) {
            tracing::warn!("cannot reset the gamma of {name}: {err}");
        }
    }
}

impl GlobalDispatch<ZwlrGammaControlManagerV1, ()> for Dune {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrGammaControlManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrGammaControlManagerV1, ()> for Dune {
    fn request(
        state: &mut Self,
        _client: &Client,
        _manager: &ZwlrGammaControlManagerV1,
        request: zwlr_gamma_control_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let zwlr_gamma_control_manager_v1::Request::GetGammaControl { id, output } = request {
            let output = Output::from_resource(&output);
            let control = data_init.init(id, ControlData { output: Mutex::new(None) });
            let Some(output) = output else { return control.failed() };
            let name = output.name();
            let taken = state.gamma.controls.get(&name).is_some_and(|c| c.is_alive());
            match state.gamma_size(&output) {
                Some(size) if !taken => {
                    *control.data::<ControlData>().unwrap().output.lock().unwrap() = Some(output);
                    state.gamma.controls.insert(name, control.clone());
                    control.gamma_size(size);
                }
                _ => control.failed(),
            }
        }
    }
}

impl Dispatch<ZwlrGammaControlV1, ControlData> for Dune {
    fn request(
        state: &mut Self,
        _client: &Client,
        control: &ZwlrGammaControlV1,
        request: zwlr_gamma_control_v1::Request,
        data: &ControlData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        // Destroy is handled by `destroyed`.
        let zwlr_gamma_control_v1::Request::SetGamma { fd } = request else { return };
        let Some(output) = data.output.lock().unwrap().clone() else { return };
        let result = state
            .gamma_size(&output)
            .and_then(|size| read_ramps(fd, size))
            .ok_or_else(|| "gamma ramps of the wrong size".to_string())
            .and_then(|ramps| state.set_gamma(&output, Some(&ramps)));
        if let Err(err) = result {
            tracing::warn!("cannot set the gamma of {}: {err}", output.name());
            control.failed();
            state.release_gamma(control, data);
        }
    }

    fn destroyed(state: &mut Self, _client: ClientId, control: &ZwlrGammaControlV1, data: &ControlData) {
        state.release_gamma(control, data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    #[test]
    fn ramps_of_the_right_size() {
        let values = [0, 100, 200, 300, 400, 500];
        assert_eq!(parse_ramps(&bytes(&values), 2), Some(values.to_vec()));
    }

    #[test]
    fn ramps_too_short_or_too_long() {
        assert_eq!(parse_ramps(&bytes(&[1, 2, 3, 4, 5]), 2), None);
        assert_eq!(parse_ramps(&bytes(&[1, 2, 3, 4, 5, 6, 7]), 2), None);
        assert_eq!(parse_ramps(&bytes(&[1, 2, 3, 4, 5, 6])[..11], 2), None); // half a value
        assert_eq!(parse_ramps(&[], 0), None);
    }

    #[test]
    fn linear() {
        assert_eq!(linear_ramps(3), [0, 32767, 65535, 0, 32767, 65535, 0, 32767, 65535]);
        assert_eq!(linear_ramps(256)[255], 65535);
        assert_eq!(linear_ramps(1), [0, 0, 0]);
    }

    #[test]
    fn read_from_a_file() {
        use std::io::Write;
        let mut file = memfd();
        file.write_all(&bytes(&[1, 2, 3, 4, 5, 6])).unwrap();
        // Read from the start, though the writer left the offset at the end.
        assert_eq!(read_ramps(file.into(), 2), Some(vec![1, 2, 3, 4, 5, 6]));
    }

    /// An unnamed file, like the memfd wlsunset sends.
    fn memfd() -> File {
        use smithay::reexports::rustix::fs::{MemfdFlags, memfd_create};
        File::from(memfd_create("dune-gamma-test", MemfdFlags::CLOEXEC).unwrap())
    }
}
