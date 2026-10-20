//! Screen capture (ext-image-copy-capture-v1 with output sources): grim takes
//! screenshots and xdg-desktop-portal-wlr shares the screen (browsers, video calls)
//! by asking dune to draw a monitor into their buffers.
//!
//! A session's first frame is drawn right away; later ones wait for the monitor to
//! show something new, so a screencast of a still screen costs nothing.

use std::time::Duration;

use smithay::{
    backend::{
        allocator::{Fourcc, dmabuf::Dmabuf},
        renderer::{Bind, ExportMem, Offscreen, damage::OutputDamageTracker, gles::{GlesRenderer, GlesTexture}},
    },
    output::{Output, WeakOutput},
    reexports::wayland_server::protocol::wl_shm,
    utils::{Clock, Monotonic, Rectangle, Size, Transform},
    wayland::{
        dmabuf::get_dmabuf,
        image_capture_source::{ImageCaptureSource, ImageCaptureSourceHandler, OutputCaptureSourceHandler, OutputCaptureSourceState},
        image_copy_capture::{
            BufferConstraints, CaptureFailureReason, DmabufConstraints, Frame, ImageCopyCaptureHandler, ImageCopyCaptureState,
            Session, SessionRef,
        },
        shm::with_buffer_contents_mut,
    },
};

use crate::{render::BACKGROUND, state::Dune};

/// Pixel formats offered for dmabuf captures (all the renderer can draw into, of these).
const DMABUF_FORMATS: [Fourcc; 4] = [Fourcc::Argb8888, Fourcc::Xrgb8888, Fourcc::Abgr8888, Fourcc::Xbgr8888];

/// A frame waiting for its monitor's next frame with changes.
pub struct PendingCapture {
    output: WeakOutput,
    cursor: bool,
    frame: Frame,
}

/// A session that got its first frame: the next ones wait for changes.
struct Started;

impl ImageCaptureSourceHandler for Dune {}

impl OutputCaptureSourceHandler for Dune {
    fn output_capture_source_state(&mut self) -> &mut OutputCaptureSourceState {
        &mut self.output_capture_source_state
    }

    fn output_source_created(&mut self, source: ImageCaptureSource, output: &Output) {
        source.user_data().insert_if_missing(|| output.downgrade());
    }
}

impl ImageCopyCaptureHandler for Dune {
    fn image_copy_capture_state(&mut self) -> &mut ImageCopyCaptureState {
        &mut self.image_copy_capture_state
    }

    /// The monitor's size in pixels; shm in the two formats every client knows, and
    /// dmabufs on the GPU dune renders with (real sessions).
    fn capture_constraints(&mut self, source: &ImageCaptureSource) -> Option<BufferConstraints> {
        let output = source.user_data().get::<WeakOutput>()?.upgrade()?;
        let mode = output.current_mode()?;
        let node = self.render_node();
        let renderer = self.backend.renderer()?;
        let dma = node.map(|node| {
            let formats = renderer.egl_context().dmabuf_render_formats();
            let formats = DMABUF_FORMATS
                .into_iter()
                .map(|code| (code, formats.iter().filter(|f| f.code == code).map(|f| f.modifier).collect::<Vec<_>>()))
                .filter(|(_, modifiers)| !modifiers.is_empty())
                .collect();
            DmabufConstraints { node, formats }
        });
        Some(BufferConstraints {
            size: (mode.size.w, mode.size.h).into(),
            shm: vec![wl_shm::Format::Argb8888, wl_shm::Format::Xrgb8888],
            dma,
        })
    }

    fn new_session(&mut self, session: Session) {
        // Dropping a `Session` stops it: keep it until the client is done with it.
        self.capture_sessions.push(session);
    }

    fn session_destroyed(&mut self, session: SessionRef) {
        self.capture_sessions.retain(|s| s.as_ref() != session);
    }

    fn frame(&mut self, session: &SessionRef, frame: Frame) {
        let Some(output) = session.source().user_data().get::<WeakOutput>().cloned() else {
            return frame.fail(CaptureFailureReason::Unknown);
        };
        let capture = PendingCapture { output, cursor: session.draw_cursor(), frame };
        if session.user_data().get::<Started>().is_some() {
            self.pending_captures.push(capture);
        } else {
            session.user_data().insert_if_missing(|| Started);
            self.capture(capture);
        }
    }
}

impl Dune {
    /// `output` showed something new: the frames waiting for it get it.
    pub fn capture_frames(&mut self, output: &Output) {
        let (ready, waiting) = std::mem::take(&mut self.pending_captures)
            .into_iter()
            .partition(|c| c.output.upgrade().is_none_or(|o| o == *output));
        self.pending_captures = waiting;
        for capture in ready {
            self.capture(capture);
        }
    }

    /// Draw the capture's monitor into the frame's buffer.
    fn capture(&mut self, capture: PendingCapture) {
        let PendingCapture { output, cursor, frame } = capture;
        let Some(output) = output.upgrade() else {
            return frame.fail(CaptureFailureReason::Stopped);
        };
        match self.draw_into(&output, cursor, &frame) {
            Ok(()) => {
                let now: Duration = Clock::<Monotonic>::new().now().into();
                frame.success(Transform::Normal, None, now);
            }
            Err(err) => {
                tracing::warn!("screen capture of {} failed: {err}", output.name());
                frame.fail(CaptureFailureReason::Unknown);
            }
        }
    }

    fn draw_into(&mut self, output: &Output, cursor: bool, frame: &Frame) -> Result<(), String> {
        let mode = output.current_mode().ok_or("monitor without a mode")?;
        let buffer = frame.buffer();
        let mut damage = OutputDamageTracker::new(mode.size, 1.0, Transform::Normal);
        let size = Size::from((mode.size.w, mode.size.h));
        self.with_renderer(|state, renderer| {
            let elements = state.output_elements(renderer, output, cursor);
            if let Ok(dmabuf) = get_dmabuf(&buffer) {
                let mut dmabuf: Dmabuf = dmabuf.clone();
                let mut target = renderer.bind(&mut dmabuf).map_err(|e| e.to_string())?;
                let result = damage.render_output(renderer, &mut target, 0, &elements, BACKGROUND).map_err(|e| format!("{e:?}"))?;
                // The client reads the buffer as soon as it hears the frame is ready.
                result.sync.wait().map_err(|_| "interrupted while drawing".to_string())
            } else {
                render_to_shm(renderer, &mut damage, &elements, size, &buffer)
            }
        })
        .ok_or("no renderer")?
    }
}

/// Draw `elements` offscreen and copy the pixels into the shm `buffer` (Argb8888 or
/// Xrgb8888: B, G, R, A in memory).
fn render_to_shm(
    renderer: &mut GlesRenderer,
    damage: &mut OutputDamageTracker,
    elements: &[crate::render::OutputRenderElements<GlesRenderer>],
    size: Size<i32, smithay::utils::Buffer>,
    buffer: &smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer,
) -> Result<(), String> {
    let mut texture: GlesTexture = renderer.create_buffer(Fourcc::Abgr8888, size).map_err(|e| e.to_string())?;
    let mapping = {
        let mut target = renderer.bind(&mut texture).map_err(|e| e.to_string())?;
        damage.render_output(renderer, &mut target, 0, elements, BACKGROUND).map_err(|e| format!("{e:?}"))?;
        renderer.copy_framebuffer(&target, Rectangle::from_size(size), Fourcc::Abgr8888).map_err(|e| e.to_string())?
    };
    let pixels = renderer.map_texture(&mapping).map_err(|e| e.to_string())?;
    with_buffer_contents_mut(buffer, |ptr, len, data| {
        let (w, h) = (size.w as usize, size.h as usize);
        let stride = data.stride as usize;
        let offset = data.offset as usize;
        if data.width as usize != w || data.height as usize != h || offset + stride * h > len {
            return Err("the buffer does not fit the monitor".to_string());
        }
        let memory = unsafe { std::slice::from_raw_parts_mut(ptr.add(offset), stride * h) };
        for (y, row) in pixels.chunks_exact(w * 4).take(h).enumerate() {
            rgba_to_bgra(row, &mut memory[y * stride..y * stride + w * 4]);
        }
        Ok(())
    })
    .map_err(|e| format!("{e:?}"))?
}

/// R, G, B, A bytes (GL's RGBA) to B, G, R, A (wl_shm's Argb8888).
fn rgba_to_bgra(from: &[u8], to: &mut [u8]) {
    for (src, dst) in from.chunks_exact(4).zip(to.chunks_exact_mut(4)) {
        dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_swap_red_and_blue() {
        let mut out = [0u8; 8];
        rgba_to_bgra(&[1, 2, 3, 4, 5, 6, 7, 8], &mut out);
        assert_eq!(out, [3, 2, 1, 4, 7, 6, 5, 8]);
    }
}
