//! Screen capture for the older tools (`wlr-screencopy-unstable-v1`): wf-recorder records
//! with it, and grim falls back to it. A frame is a monitor, or a region of it, copied
//! once into a client buffer; drawn like `crate::screencopy` (private tiles blocked),
//! but turned like the monitor's own buffer (its transform), which clients undo.
//!
//! `copy` draws right away; `copy_with_damage` (recorders, frame after frame) waits for
//! the monitor to show something new, like the ext frames after the first.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use smithay::{
    backend::allocator::{Buffer, Fourcc},
    output::{Output, WeakOutput},
    reexports::{
        wayland_protocols_wlr::screencopy::v1::server::{
            zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
            zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
            protocol::{wl_buffer::WlBuffer, wl_shm},
        },
    },
    utils::{Clock, Logical, Monotonic, Physical, Rectangle, Size, Transform},
    wayland::{dmabuf::get_dmabuf, shm::with_buffer_contents},
};

use crate::Dune;

/// Frames waiting for their monitor's next frame with changes.
#[derive(Default)]
pub struct WlrScreencopy {
    pending: Vec<PendingCopy>,
}

impl WlrScreencopy {
    pub fn new(dh: &DisplayHandle) -> Self {
        dh.create_global::<Dune, ZwlrScreencopyManagerV1, ()>(3, ());
        WlrScreencopy::default()
    }
}

/// Data of a frame: what it captures, in pixels of the monitor as shown, and how the
/// monitor turns it.
pub struct FrameData {
    output: WeakOutput,
    region: Rectangle<i32, Physical>,
    transform: Transform,
    cursor: bool,
    /// A frame is copied once.
    used: AtomicBool,
}

/// A frame to draw into `buffer`; `damage` for `copy_with_damage`.
struct PendingCopy {
    frame: ZwlrScreencopyFrameV1,
    buffer: WlBuffer,
    damage: bool,
}

/// The pixels of a monitor `size` pixels big, at `scale`, that the region `x, y, w, h`
/// (in the monitor's logical coordinates) covers; None when nothing of it is on the
/// monitor. Checked before it is a `Rectangle`, which cannot be negative.
fn region_pixels(size: Size<i32, Physical>, scale: f64, (x, y, w, h): (i32, i32, i32, i32)) -> Option<Rectangle<i32, Physical>> {
    if w <= 0 || h <= 0 {
        return None;
    }
    let logical = Rectangle::<i32, Logical>::new((x, y).into(), (w, h).into());
    logical.to_physical_precise_round(scale).intersection(Rectangle::from_size(size))
}

/// Whether `buffer` takes a frame of `size`: shm Argb8888 or Xrgb8888 of that size, or
/// a dmabuf of that size.
fn buffer_fits(buffer: &WlBuffer, size: Size<i32, Physical>) -> bool {
    if let Ok(dmabuf) = get_dmabuf(buffer) {
        let dmabuf_size = dmabuf.size();
        return (dmabuf_size.w, dmabuf_size.h) == (size.w, size.h);
    }
    with_buffer_contents(buffer, |_, len, data| {
        matches!(data.format, wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888)
            && (data.width, data.height) == (size.w, size.h)
            && data.stride >= size.w * 4
            && data.offset as usize + data.stride as usize * data.height as usize <= len
    })
    .unwrap_or(false)
}

impl Dune {
    /// `output` showed something new: the `copy_with_damage` frames waiting for it get it.
    pub fn wlr_capture_frames(&mut self, output: &Output) {
        let (ready, waiting) = std::mem::take(&mut self.wlr_screencopy.pending)
            .into_iter()
            .filter(|c| c.frame.is_alive())
            .partition(|c| c.frame.data::<FrameData>().is_none_or(|d| d.output.upgrade().is_none_or(|o| o == *output)));
        self.wlr_screencopy.pending = waiting;
        for copy in ready {
            self.wlr_copy(copy);
        }
    }

    /// Draw the frame's monitor (its region) into the buffer and tell the client.
    fn wlr_copy(&mut self, copy: PendingCopy) {
        let PendingCopy { frame, buffer, damage } = copy;
        let Some(data) = frame.data::<FrameData>() else { return };
        let Some(output) = data.output.upgrade() else { return frame.failed() };
        let size = data.transform.transform_size(data.region.size);
        match self.draw_output(&output, data.cursor, data.region, data.transform, &buffer) {
            Ok(()) => {
                frame.flags(zwlr_screencopy_frame_v1::Flags::empty());
                if damage {
                    frame.damage(0, 0, size.w as u32, size.h as u32);
                }
                let now: Duration = Clock::<Monotonic>::new().now().into();
                let secs = now.as_secs();
                frame.ready((secs >> 32) as u32, secs as u32, now.subsec_nanos());
            }
            Err(err) => {
                tracing::warn!("wlr screen capture of {} failed: {err}", output.name());
                frame.failed();
            }
        }
    }

    /// A new frame of `output` (all of it, or `region` in its logical coordinates): the
    /// buffers it can be copied into, or `failed`.
    fn new_wlr_frame(
        &mut self,
        id: New<ZwlrScreencopyFrameV1>,
        output: Option<Output>,
        region: Option<(i32, i32, i32, i32)>,
        cursor: bool,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let pixels = output.as_ref().and_then(|output| {
            // The monitor as shown: turned 90 or 270 degrees, it is as wide as the mode is tall.
            let size = output.current_transform().transform_size(output.current_mode()?.size);
            match region {
                Some(region) => region_pixels(size, output.current_scale().fractional_scale(), region),
                None => Some(Rectangle::from_size(size)),
            }
        });
        let transform = output.as_ref().map_or(Transform::Normal, Output::current_transform);
        let data = FrameData {
            output: output.as_ref().map(Output::downgrade).unwrap_or_default(),
            region: pixels.unwrap_or_default(),
            transform,
            cursor,
            used: AtomicBool::new(false),
        };
        let frame = data_init.init(id, data);
        let Some(region) = pixels.filter(|_| self.backend.renderer().is_some()) else {
            return frame.failed();
        };
        let size = transform.transform_size(region.size);
        let (w, h) = (size.w as u32, size.h as u32);
        // One shm format: wf-recorder makes a buffer and copies for each one offered.
        frame.buffer(wl_shm::Format::Xrgb8888, w, h, w * 4);
        if frame.version() >= 3 {
            // One format to allocate in; Xrgb8888 when the GPU draws into it.
            let formats = self.dmabuf_constraints().map(|c| c.formats).unwrap_or_default();
            let format = formats.iter().map(|(code, _)| *code).find(|code| *code == Fourcc::Xrgb8888).or(formats.first().map(|(code, _)| *code));
            if let Some(format) = format {
                frame.linux_dmabuf(format as u32, w, h);
            }
            frame.buffer_done();
        }
    }
}

impl GlobalDispatch<ZwlrScreencopyManagerV1, ()> for Dune {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrScreencopyManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for Dune {
    fn request(
        state: &mut Self,
        _client: &Client,
        _manager: &ZwlrScreencopyManagerV1,
        request: zwlr_screencopy_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_screencopy_manager_v1::Request::CaptureOutput { frame, overlay_cursor, output } => {
                state.new_wlr_frame(frame, Output::from_resource(&output), None, overlay_cursor != 0, data_init);
            }
            zwlr_screencopy_manager_v1::Request::CaptureOutputRegion { frame, overlay_cursor, output, x, y, width, height } => {
                let region = Some((x, y, width, height));
                state.new_wlr_frame(frame, Output::from_resource(&output), region, overlay_cursor != 0, data_init);
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, FrameData> for Dune {
    fn request(
        state: &mut Self,
        _client: &Client,
        frame: &ZwlrScreencopyFrameV1,
        request: zwlr_screencopy_frame_v1::Request,
        data: &FrameData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        let (buffer, damage) = match request {
            zwlr_screencopy_frame_v1::Request::Copy { buffer } => (buffer, false),
            zwlr_screencopy_frame_v1::Request::CopyWithDamage { buffer } => (buffer, true),
            _ => return, // destroy
        };
        if data.used.swap(true, Ordering::Relaxed) {
            return frame.post_error(zwlr_screencopy_frame_v1::Error::AlreadyUsed, "the frame was already copied");
        }
        if data.output.upgrade().is_none() {
            return frame.failed();
        }
        if !buffer_fits(&buffer, data.transform.transform_size(data.region.size)) {
            return frame.post_error(zwlr_screencopy_frame_v1::Error::InvalidBuffer, "the buffer does not fit the frame");
        }
        let copy = PendingCopy { frame: frame.clone(), buffer, damage };
        if damage {
            state.wlr_screencopy.pending.push(copy);
        } else {
            state.wlr_copy(copy);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> (i32, i32, i32, i32) {
        (x, y, w, h)
    }

    #[test]
    fn region_on_the_monitor() {
        let size = Size::from((1920, 1080));
        let pixels = |x, y, w, h| Some(Rectangle::<i32, Physical>::new((x, y).into(), (w, h).into()));
        assert_eq!(region_pixels(size, 1.0, rect(10, 10, 200, 100)), pixels(10, 10, 200, 100));
        assert_eq!(region_pixels(size, 2.0, rect(10, 10, 200, 100)), pixels(20, 20, 400, 200));
        // Cut to the monitor.
        assert_eq!(region_pixels(size, 1.0, rect(1800, 1000, 400, 300)), pixels(1800, 1000, 120, 80));
        assert_eq!(region_pixels(size, 1.0, rect(-10, -10, 20, 20)), pixels(0, 0, 10, 10));
    }

    #[test]
    fn region_off_the_monitor_or_empty() {
        let size = Size::from((1920, 1080));
        assert_eq!(region_pixels(size, 1.0, rect(2000, 0, 100, 100)), None);
        assert_eq!(region_pixels(size, 1.0, rect(0, 0, 0, 100)), None);
        assert_eq!(region_pixels(size, 1.0, rect(0, 0, 100, -5)), None);
    }
}
