use std::time::Duration;

use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker,
            ImportDma,
            gles::GlesRenderer,
            element::{
                Kind,
                solid::{SolidColorBuffer, SolidColorRenderElement},
            },
        },
        egl::EGLDevice,
        winit::{self, WinitEvent},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Rectangle, Transform},
    desktop::layer_map_for_output,
    wayland::dmabuf::DmabufFeedbackBuilder,
};

use crate::{
    Dune,
    layout::Side,
    tiling::{BORDER, GAP},
};

const BACKGROUND: [f32; 4] = [0.08, 0.08, 0.1, 1.0];
const FOCUS_COLOR: [f32; 4] = [0.36, 0.62, 0.95, 1.0];
/// The direction indicator: a bar on the side where the next command will open.
const DIRECTION_COLOR: [f32; 4] = [0.98, 0.62, 0.22, 1.0];

pub fn init_winit(event_loop: &mut EventLoop<Dune>, state: &mut Dune) -> Result<(), Box<dyn std::error::Error>> {
    let (mut backend, winit) = winit::init::<GlesRenderer>()?;

    let mode = Mode {
        size: backend.window_size(),
        refresh: 60_000,
    };

    let output = Output::new(
        "winit".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Dune".into(),
            model: "Winit".into(),
            serial_number: "Unknown".into(),
        },
    );
    let _global = output.create_global::<Dune>(&state.display_handle);
    output.change_current_state(Some(mode), Some(Transform::Flipped180), None, Some((0, 0).into()));
    output.set_preferred(mode);

    state.space.map_output(&output, (0, 0));

    // Let clients render on the GPU and hand us the buffers (linux-dmabuf): without
    // it Mesa falls back to software rendering (llvmpipe). Version 4 also tells them
    // which GPU we render with.
    let formats = backend.renderer().dmabuf_formats();
    let render_node = EGLDevice::device_for_display(backend.renderer().egl_context().display())
        .ok()
        .and_then(|device| device.try_get_render_node().ok().flatten());
    let feedback = render_node.and_then(|node| DmabufFeedbackBuilder::new(node.dev_id(), formats.clone()).build().ok());
    match &feedback {
        Some(feedback) => {
            state.dmabuf_state.create_global_with_default_feedback::<Dune>(&state.display_handle, feedback);
        }
        None => {
            tracing::warn!("no render node found, offering linux-dmabuf v3");
            state.dmabuf_state.create_global::<Dune>(&state.display_handle, formats);
        }
    }

    let mut damage_tracker = OutputDamageTracker::from_output(&output);
    // Top, bottom, left, right edges of the focus border.
    let mut borders: [SolidColorBuffer; 4] = Default::default();
    let mut direction_bars: std::collections::HashMap<crate::layout::TileId, SolidColorBuffer> = Default::default();

    state.backend = Some(backend);
    let injected_input_only =
        std::env::var("DUNE_DEBUG").as_deref() == Ok("1") && std::env::var("DUNE_TEST_INPUT").as_deref() == Ok("injected");

    event_loop.handle().insert_source(winit, move |event, _, state| {
        match event {
            WinitEvent::Resized { size, .. } => {
                output.change_current_state(Some(Mode { size, refresh: 60_000 }), None, None, None);
                state.arrange();
            }
            // Tests drive dune with injected keys only: the host's real keyboard, which
            // follows focus on the host desktop, must not type into test shells.
            WinitEvent::Input(_) if injected_input_only => {}
            WinitEvent::Input(event) => state.process_input_event(event),
            WinitEvent::Redraw => {
                let backend = state.backend.as_mut().unwrap();
                let size = backend.window_size();
                let damage = Rectangle::from_size(size);

                let mut elements = Vec::new();
                // Indicator bars first: elements earlier in the list are drawn on top.
                direction_bars.retain(|tile, _| state.directions.contains_key(tile));
                for (tile, side) in &state.directions {
                    let Some(rect) = state.geometries.iter().find(|(t, _)| t == tile).map(|(_, r)| *r) else {
                        continue;
                    };
                    let (x, y, w, h) = match side {
                        Side::Left => (rect.x - GAP, rect.y, GAP, rect.h),
                        Side::Right => (rect.x + rect.w, rect.y, GAP, rect.h),
                        Side::Up => (rect.x, rect.y - GAP, rect.w, GAP),
                        Side::Down => (rect.x, rect.y + rect.h, rect.w, GAP),
                    };
                    let buffer = direction_bars.entry(*tile).or_default();
                    buffer.update((w, h), DIRECTION_COLOR);
                    elements.push(SolidColorRenderElement::from_buffer(buffer, (x, y), 1.0, 1.0, Kind::Unspecified));
                }
                if let Some(rect) = state
                    .focused
                    .and_then(|id| state.geometries.iter().find(|(t, _)| *t == id))
                    .map(|(_, r)| *r)
                {
                    let b = BORDER.min(GAP);
                    let edges = [
                        (rect.x - b, rect.y - b, rect.w + 2 * b, b),
                        (rect.x - b, rect.y + rect.h, rect.w + 2 * b, b),
                        (rect.x - b, rect.y, b, rect.h),
                        (rect.x + rect.w, rect.y, b, rect.h),
                    ];
                    for (buffer, (x, y, w, h)) in borders.iter_mut().zip(edges) {
                        buffer.update((w, h), FOCUS_COLOR);
                        elements.push(SolidColorRenderElement::from_buffer(
                            buffer,
                            (x, y),
                            1.0,
                            1.0,
                            Kind::Unspecified,
                        ));
                    }
                }

                {
                    let (renderer, mut framebuffer) = backend.bind().unwrap();
                    smithay::desktop::space::render_output(
                        &output,
                        renderer,
                        &mut framebuffer,
                        1.0,
                        0,
                        [&state.space],
                        &elements,
                        &mut damage_tracker,
                        BACKGROUND,
                    )
                    .unwrap();
                }
                backend.submit(Some(&[damage])).unwrap();

                state.space.elements().for_each(|window| {
                    window.send_frame(&output, state.start_time.elapsed(), Some(Duration::ZERO), |_, _| {
                        Some(output.clone())
                    })
                });
                for layer in layer_map_for_output(&output).layers() {
                    layer.send_frame(&output, state.start_time.elapsed(), Some(Duration::ZERO), |_, _| {
                        Some(output.clone())
                    });
                }

                state.space.refresh();
                state.popups.cleanup();
                let _ = state.display_handle.flush_clients();

                state.backend.as_ref().unwrap().window().request_redraw();
            }
            WinitEvent::CloseRequested => {
                state.loop_signal.stop();
            }
            _ => (),
        };
    })?;

    Ok(())
}
