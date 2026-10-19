use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker,
            ImportDma,
            gles::GlesRenderer,
        },
        egl::EGLDevice,
        winit::{self, WinitEvent},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Rectangle, Transform},
    wayland::dmabuf::DmabufFeedbackBuilder,
};

use crate::{Dune, render::BACKGROUND, state::Backend};


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
    state.backend = Backend::Winit(backend);
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
                // The backend is lent out while the scene is built from the rest of the state.
                let Backend::Winit(mut backend) = std::mem::replace(&mut state.backend, Backend::None) else { return };
                let damage = Rectangle::from_size(backend.window_size());
                {
                    let (renderer, mut framebuffer) = backend.bind().unwrap();
                    // The host draws the pointer.
                    let elements = state.output_elements(renderer, &output, false);
                    damage_tracker.render_output(renderer, &mut framebuffer, 0, &elements, BACKGROUND).unwrap();
                }
                backend.submit(Some(&[damage])).unwrap();
                backend.window().request_redraw();
                state.backend = Backend::Winit(backend);
                state.post_render(&output);
            }
            WinitEvent::CloseRequested => {
                state.loop_signal.stop();
            }
            _ => (),
        };
    })?;

    Ok(())
}
