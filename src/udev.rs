//! A real session on a TTY: the seat through libseat, the GPU through DRM/GBM/EGL,
//! input through libinput. One GPU (the primary one); every connected monitor
//! becomes an output named like `HDMI-A-1`, with the mode and position from
//! `output.<NAME>.*` in the config (by default its preferred mode, side by side).

use std::{
    collections::HashMap,
    path::Path,
    time::{Duration, Instant},
};

use smithay::{
    backend::{
        allocator::{
            Fourcc,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmNode, NodeType,
            compositor::FrameFlags,
            exporter::gbm::GbmFramebufferExporter,
            output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements},
        },
        egl::{EGLContext, EGLDevice, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{ImportDma, ImportMemWl, gles::GlesRenderer},
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, UdevEvent, all_gpus, primary_gpu},
    },
    output::{Mode as WlMode, Output, PhysicalProperties},
    reexports::{
        calloop::{
            EventLoop, RegistrationToken,
            timer::{TimeoutAction, Timer},
        },
        drm::control::{ModeTypeFlags, connector, crtc},
        input::{Device as InputDevice, DeviceConfigError, Libinput},
        rustix::fs::OFlags,
        wayland_server::backend::GlobalId,
    },
    utils::DeviceFd,
    wayland::dmabuf::DmabufFeedbackBuilder,
};
use smithay_drm_extras::{
    display_info,
    drm_scanner::{DrmScanEvent, DrmScanner},
};

use crate::{
    Dune,
    config::{InputConfig, OutputMode},
    render::{BACKGROUND, OutputRenderElements},
    state::Backend,
};

type Allocator = GbmAllocator<DrmDeviceFd>;
type Exporter = GbmFramebufferExporter<DrmDeviceFd>;
type Elements = OutputRenderElements<GlesRenderer>;

/// How often outputs are checked for something new to show (frames with no damage
/// cost nothing).
const REDRAW_INTERVAL: Duration = Duration::from_millis(16);
/// A queued frame whose vblank has not come after this long is given up on, so a lost
/// vblank cannot freeze a monitor.
const VBLANK_TIMEOUT: Duration = Duration::from_millis(250);

pub struct UdevData {
    pub session: LibSeatSession,
    /// Renderer of the primary GPU (set once the device is opened).
    pub renderer: Option<GlesRenderer>,
    gpu: Option<Gpu>,
    /// Mice, touchpads and keyboards plugged in, to apply `input.*` again on a config reload.
    input_devices: Vec<InputDevice>,
}

struct Gpu {
    node: DrmNode,
    manager: DrmOutputManager<Allocator, Exporter, (), DrmDeviceFd>,
    scanner: DrmScanner,
    surfaces: HashMap<crtc::Handle, Surface>,
    _vblank: RegistrationToken,
}

struct Surface {
    output: Output,
    global: GlobalId,
    drm_output: DrmOutput<Allocator, Exporter, (), DrmDeviceFd>,
    /// A frame is on its way to the screen (since then); wait for its vblank before the next.
    queued: Option<Instant>,
}

pub fn init_udev(event_loop: &mut EventLoop<'static, Dune>, state: &mut Dune) -> Result<(), Box<dyn std::error::Error>> {
    let (session, notifier) = LibSeatSession::new().map_err(|e| format!("cannot open a seat session (run dune from a TTY): {e}"))?;
    let seat = session.seat();

    let primary = primary_gpu(&seat)?
        .and_then(|path| DrmNode::from_path(path).ok())
        .or_else(|| all_gpus(&seat).ok()?.into_iter().find_map(|path| DrmNode::from_path(path).ok()))
        .ok_or("no GPU found")?;
    tracing::info!("using {primary} as the GPU");

    state.backend = Backend::Udev(UdevData {
        session: session.clone(),
        renderer: None,
        gpu: None,
        input_devices: Vec::new(),
    });

    // The GPU, and monitors plugged in or out later.
    let udev = UdevBackend::new(&seat)?;
    let primary_path = udev
        .device_list()
        .find(|(id, _)| *id == primary.dev_id() || node_matches(*id, primary))
        .map(|(id, path)| (id, path.to_path_buf()))
        .ok_or("the GPU is not in the udev device list")?;
    state.gpu_added(DrmNode::from_dev_id(primary_path.0)?, &primary_path.1)?;
    event_loop
        .handle()
        .insert_source(udev, |event, _, state| match event {
            UdevEvent::Changed { device_id } => state.gpu_changed(device_id),
            UdevEvent::Added { .. } | UdevEvent::Removed { .. } => {
                tracing::info!("ignoring GPU hotplug: only the primary GPU is used");
            }
        })
        .map_err(|e| e.to_string())?;

    // Keyboards, mice, touchpads.
    let mut libinput = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput.udev_assign_seat(&seat).map_err(|()| "cannot assign libinput to the seat")?;
    event_loop
        .handle()
        .insert_source(LibinputInputBackend::new(libinput.clone()), |event, _, state| {
            if let Backend::Udev(udev) = &mut state.backend {
                match &event {
                    InputEvent::DeviceAdded { device } => {
                        configure_input_device(&mut device.clone(), &state.config.input);
                        udev.input_devices.push(device.clone());
                    }
                    InputEvent::DeviceRemoved { device } => udev.input_devices.retain(|d| d != device),
                    _ => {}
                }
            }
            state.process_input_event(event)
        })
        .map_err(|e| e.to_string())?;

    // Switching to another VT and back.
    event_loop
        .handle()
        .insert_source(notifier, move |event, _, state| match event {
            SessionEvent::PauseSession => {
                tracing::info!("session paused");
                libinput.suspend();
                if let Some(gpu) = state.gpu_mut() {
                    gpu.manager.pause();
                }
            }
            SessionEvent::ActivateSession => {
                tracing::info!("session resumed");
                if let Err(err) = libinput.resume() {
                    tracing::error!("cannot resume input: {err:?}");
                }
                if let Some(gpu) = state.gpu_mut() {
                    if let Err(err) = gpu.manager.lock().activate(false) {
                        tracing::error!("cannot resume the display: {err}");
                    }
                    for surface in gpu.surfaces.values_mut() {
                        surface.queued = None;
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;

    // Redraw loop: outputs without a frame in flight get one if anything changed.
    event_loop
        .handle()
        .insert_source(Timer::from_duration(REDRAW_INTERVAL), |_, _, state| {
            state.render_all();
            TimeoutAction::ToDuration(REDRAW_INTERVAL)
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// What `choose_mode` needs to know about a monitor mode.
struct ModeInfo {
    width: u16,
    height: u16,
    refresh: u32,
    preferred: bool,
}

/// Index of the mode to use: the configured size (at its refresh rate, else the
/// fastest one), falling back to the monitor's preferred mode, then its first.
fn choose_mode(modes: &[ModeInfo], wanted: Option<OutputMode>) -> Option<usize> {
    let configured = wanted.and_then(|w| {
        let same_size = || modes.iter().enumerate().filter(move |(_, m)| (m.width, m.height) == (w.width, w.height));
        same_size()
            .find(|(_, m)| Some(m.refresh) == w.refresh)
            .or_else(|| same_size().max_by_key(|(_, m)| m.refresh))
            .map(|(i, _)| i)
    });
    configured
        .or_else(|| modes.iter().position(|m| m.preferred))
        .or((!modes.is_empty()).then_some(0))
}

/// Top-left corner of each output, given its width and configured position: the
/// unconfigured ones are lined up to the right of everything else.
fn positions(outputs: &[(i32, Option<(i32, i32)>)]) -> Vec<(i32, i32)> {
    let mut x = outputs
        .iter()
        .filter_map(|(width, pos)| pos.map(|(x, _)| x + width))
        .max()
        .unwrap_or(0);
    outputs
        .iter()
        .map(|(width, pos)| {
            pos.unwrap_or_else(|| {
                x += width;
                (x - width, 0)
            })
        })
        .collect()
}

/// `dev_id` is another node (primary/render) of the same GPU as `node`.
fn node_matches(dev_id: u64, node: DrmNode) -> bool {
    DrmNode::from_dev_id(dev_id)
        .ok()
        .and_then(|n| n.node_with_type(NodeType::Render)?.ok())
        .is_some_and(|render| render == node)
}

/// Applies `input.*` from the config to a mouse or touchpad as libinput finds it.
fn configure_input_device(device: &mut InputDevice, config: &InputConfig) {
    let name = device.name().to_string();
    let apply = |what: &str, result: Result<(), DeviceConfigError>| {
        if let Err(err) = result {
            tracing::debug!("input {name}: {what} not applied: {err:?}");
        }
    };
    if let Some(on) = config.left_handed
        && device.config_left_handed_is_available()
    {
        apply("left_handed", device.config_left_handed_set(on));
    }
    if let Some(on) = config.natural_scroll
        && device.config_scroll_has_natural_scroll()
    {
        apply("natural_scroll", device.config_scroll_set_natural_scroll_enabled(on));
    }
    if let Some(on) = config.tap
        && device.config_tap_finger_count() > 0
    {
        apply("tap", device.config_tap_set_enabled(on));
    }
    if let Some(speed) = config.accel_speed
        && device.config_accel_is_available()
    {
        apply("accel_speed", device.config_accel_set_speed(speed));
    }
}

impl Dune {
    fn udev(&mut self) -> Option<&mut UdevData> {
        match &mut self.backend {
            Backend::Udev(udev) => Some(udev),
            _ => None,
        }
    }

    fn gpu_mut(&mut self) -> Option<&mut Gpu> {
        self.udev()?.gpu.as_mut()
    }

    /// Switch to another virtual terminal (Ctrl+Alt+F1..F12).
    pub fn change_vt(&mut self, vt: i32) {
        if let Some(udev) = self.udev()
            && let Err(err) = udev.session.change_vt(vt)
        {
            tracing::error!("cannot switch to VT {vt}: {err}");
        }
    }

    fn gpu_added(&mut self, node: DrmNode, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let udev = self.udev().unwrap();
        let fd = udev
            .session
            .open(path, OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK)
            .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let fd = DrmDeviceFd::new(DeviceFd::from(fd));
        let (drm, notifier) = DrmDevice::new(fd.clone(), true)?;
        let gbm = GbmDevice::new(fd)?;

        let display = unsafe { EGLDisplay::new(gbm.clone())? };
        let render_node = EGLDevice::device_for_display(&display)
            .ok()
            .and_then(|d| d.try_get_render_node().ok().flatten())
            .unwrap_or(node);
        let renderer = unsafe { GlesRenderer::new(EGLContext::new(&display)?)? };

        // Clients render on this GPU and share buffers with us (linux-dmabuf v4).
        let formats = renderer.dmabuf_formats();
        match DmabufFeedbackBuilder::new(render_node.dev_id(), formats.clone()).build() {
            Ok(feedback) => {
                self.dmabuf_state.create_global_with_default_feedback::<Dune>(&self.display_handle, &feedback);
            }
            Err(err) => {
                tracing::warn!("no dmabuf feedback ({err}), offering linux-dmabuf v3");
                self.dmabuf_state.create_global::<Dune>(&self.display_handle, formats);
            }
        }
        self.shm_state.update_formats(renderer.shm_formats());

        let render_formats = renderer.egl_context().dmabuf_render_formats().clone();
        let manager = DrmOutputManager::new(
            drm,
            GbmAllocator::new(gbm.clone(), GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT),
            GbmFramebufferExporter::new(gbm.clone(), Some(render_node).into()),
            Some(gbm),
            [Fourcc::Argb8888, Fourcc::Xrgb8888, Fourcc::Abgr8888, Fourcc::Xbgr8888],
            render_formats,
        );
        let vblank = self
            .loop_handle
            .insert_source(notifier, |event, _, state| match event {
                DrmEvent::VBlank(crtc) => state.frame_done(crtc),
                DrmEvent::Error(err) => tracing::error!("display error: {err}"),
            })
            .map_err(|e| e.to_string())?;

        let udev = self.udev().unwrap();
        udev.renderer = Some(renderer);
        udev.gpu = Some(Gpu {
            node,
            manager,
            scanner: DrmScanner::new(),
            surfaces: HashMap::new(),
            _vblank: vblank,
        });
        self.scan_connectors();
        Ok(())
    }

    fn gpu_changed(&mut self, dev_id: u64) {
        if self.gpu_mut().is_some_and(|gpu| gpu.node.dev_id() == dev_id || node_matches(dev_id, gpu.node)) {
            self.scan_connectors();
        }
    }

    /// Bring outputs in line with the monitors that are connected now.
    fn scan_connectors(&mut self) {
        let Some(gpu) = self.gpu_mut() else { return };
        let events: Vec<DrmScanEvent> = match gpu.scanner.scan_connectors(gpu.manager.device()) {
            Ok(result) => result.into_iter().collect(),
            Err(err) => return tracing::warn!("cannot scan monitors: {err}"),
        };
        for event in events {
            match event {
                DrmScanEvent::Connected { connector, crtc: Some(crtc) } => self.connector_connected(connector, crtc),
                DrmScanEvent::Disconnected { crtc: Some(crtc), .. } => self.connector_disconnected(crtc),
                _ => {}
            }
        }
        self.place_outputs();
        self.arrange();
    }

    /// `input.*` from the config again, on every device plugged in.
    pub fn configure_input_devices(&mut self) {
        let Backend::Udev(udev) = &mut self.backend else { return };
        for device in &mut udev.input_devices {
            configure_input_device(device, &self.config.input);
        }
    }

    fn connector_connected(&mut self, connector: connector::Info, crtc: crtc::Handle) {
        let name = format!("{}-{}", connector.interface().as_str(), connector.interface_id());
        let config = self.config.outputs.get(&name).cloned().unwrap_or_default();
        if config.off {
            return tracing::info!("monitor {name} is off in the config");
        }
        let modes: Vec<ModeInfo> = connector
            .modes()
            .iter()
            .map(|m| ModeInfo {
                width: m.size().0,
                height: m.size().1,
                refresh: m.vrefresh(),
                preferred: m.mode_type().contains(ModeTypeFlags::PREFERRED),
            })
            .collect();
        let Some(mode) = choose_mode(&modes, config.mode).map(|i| connector.modes()[i]) else {
            return tracing::warn!("monitor {name} has no modes");
        };
        if let Some(wanted) = config.mode
            && (mode.size().0, mode.size().1) != (wanted.width, wanted.height)
        {
            tracing::warn!("monitor {name} has no {}x{} mode, using its preferred one", wanted.width, wanted.height);
        }

        let Backend::Udev(udev) = &mut self.backend else { return };
        let (Some(gpu), Some(renderer)) = (udev.gpu.as_mut(), udev.renderer.as_mut()) else { return };
        let device = gpu.manager.device();
        let info = display_info::for_connector(device, connector.handle());
        let text = |f: Option<String>| f.unwrap_or_else(|| "Unknown".into());
        let (w_mm, h_mm) = connector.size().unwrap_or((0, 0));
        let output = Output::new(
            name.clone(),
            PhysicalProperties {
                size: (w_mm as i32, h_mm as i32).into(),
                subpixel: connector.subpixel().into(),
                make: text(info.as_ref().and_then(|i| i.make())),
                model: text(info.as_ref().and_then(|i| i.model())),
                serial_number: text(info.as_ref().and_then(|i| i.serial())),
            },
        );
        let wl_mode = WlMode::from(mode);
        output.set_preferred(wl_mode);
        output.change_current_state(Some(wl_mode), None, None, None);
        let global = output.create_global::<Dune>(&self.display_handle);

        let planes = device.planes(&crtc).ok();
        let drm_output = match gpu.manager.lock().initialize_output::<_, Elements>(
            crtc,
            mode,
            &[connector.handle()],
            &output,
            planes,
            renderer,
            &DrmOutputRenderElements::default(),
        ) {
            Ok(drm_output) => drm_output,
            Err(err) => {
                self.display_handle.remove_global::<Dune>(global);
                return tracing::error!("cannot drive monitor {name}: {err}");
            }
        };
        tracing::info!("monitor {name}: {}x{}@{}", mode.size().0, mode.size().1, mode.vrefresh());
        gpu.surfaces.insert(crtc, Surface { output: output.clone(), global, drm_output, queued: None });
        self.space.map_output(&output, (0, 0));
    }

    fn connector_disconnected(&mut self, crtc: crtc::Handle) {
        let Some(gpu) = self.gpu_mut() else { return };
        if let Some(surface) = gpu.surfaces.remove(&crtc) {
            tracing::info!("monitor {} disconnected", surface.output.name());
            self.space.unmap_output(&surface.output);
            self.display_handle.remove_global::<Dune>(surface.global);
        }
    }

    /// Outputs where the config puts them; the others side by side to their right,
    /// in connector order.
    pub fn place_outputs(&mut self) {
        let mut outputs: Vec<Output> = self.space.outputs().cloned().collect();
        outputs.sort_by_key(|o| o.name());
        let placed = positions(
            &outputs
                .iter()
                .map(|o| {
                    let width = o.current_mode().map_or(0, |m| m.size.w);
                    (width, self.config.outputs.get(&o.name()).and_then(|c| c.position))
                })
                .collect::<Vec<_>>(),
        );
        for (output, (x, y)) in outputs.into_iter().zip(placed) {
            output.change_current_state(None, None, None, Some((x, y).into()));
            self.space.map_output(&output, (x, y));
        }
    }

    fn render_all(&mut self) {
        let crtcs: Vec<crtc::Handle> = match self.gpu_mut() {
            Some(gpu) => gpu
                .surfaces
                .iter_mut()
                .filter_map(|(crtc, s)| match s.queued {
                    Some(since) if since.elapsed() >= VBLANK_TIMEOUT => {
                        tracing::warn!("no vblank on {} for {:?}, drawing again", s.output.name(), since.elapsed());
                        s.queued = None;
                        Some(*crtc)
                    }
                    Some(_) => None,
                    None => Some(*crtc),
                })
                .collect(),
            None => return,
        };
        for crtc in crtcs {
            self.render_surface(crtc);
        }
    }

    fn render_surface(&mut self, crtc: crtc::Handle) {
        let Some(udev) = self.udev() else { return };
        if !udev.session.is_active() {
            return;
        }
        let Some(output) = udev.gpu.as_ref().and_then(|g| g.surfaces.get(&crtc)).map(|s| s.output.clone()) else {
            return;
        };
        // The renderer is lent out while the scene is built from the rest of the state.
        let Some(mut renderer) = udev.renderer.take() else { return };
        let elements = self.output_elements(&mut renderer, &output, true);

        let Backend::Udev(udev) = &mut self.backend else { unreachable!() };
        let surface = udev.gpu.as_mut().and_then(|g| g.surfaces.get_mut(&crtc)).unwrap();
        // No overlay planes: a game scanned out on one covered the composited pointer
        // (niri leaves them off by default too).
        let flags = FrameFlags::ALLOW_PRIMARY_PLANE_SCANOUT | FrameFlags::ALLOW_CURSOR_PLANE_SCANOUT;
        match surface.drm_output.render_frame(&mut renderer, &elements, BACKGROUND, flags) {
            Ok(result) if !result.is_empty => match surface.drm_output.queue_frame(()) {
                Ok(()) => surface.queued = Some(Instant::now()),
                Err(err) => tracing::warn!("cannot show frame on {}: {err}", output.name()),
            },
            Ok(_) => {} // nothing changed
            Err(err) => tracing::warn!("cannot render {}: {err}", output.name()),
        }
        drop(elements);
        udev.renderer = Some(renderer);
        self.post_render(&output);
    }

    /// The monitor showed our frame: the next one may go.
    fn frame_done(&mut self, crtc: crtc::Handle) {
        let Some(surface) = self.gpu_mut().and_then(|g| g.surfaces.get_mut(&crtc)) else { return };
        if let Err(err) = surface.drm_output.frame_submitted() {
            tracing::warn!("frame error: {err}");
        }
        surface.queued = None;
        self.render_surface(crtc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_from_config_or_preferred() {
        let mode = |width, height, refresh, preferred| ModeInfo { width, height, refresh, preferred };
        let modes = [mode(2560, 1080, 60, true), mode(1920, 1080, 60, false), mode(1920, 1080, 75, false), mode(1280, 720, 60, false)];
        let wanted = |width, height, refresh| Some(OutputMode { width, height, refresh });
        assert_eq!(choose_mode(&modes, None), Some(0));
        assert_eq!(choose_mode(&modes, wanted(1920, 1080, Some(60))), Some(1));
        assert_eq!(choose_mode(&modes, wanted(1920, 1080, None)), Some(2)); // fastest
        assert_eq!(choose_mode(&modes, wanted(1920, 1080, Some(144))), Some(2));
        assert_eq!(choose_mode(&modes, wanted(800, 600, None)), Some(0)); // not offered
        assert_eq!(choose_mode(&modes[1..], None), Some(0));
        assert_eq!(choose_mode(&[], None), None);
    }

    #[test]
    fn output_positions() {
        // The user's niri setup: the small monitor under the wide one.
        assert_eq!(positions(&[(2560, Some((0, 0))), (1920, Some((311, 1080)))]), [(0, 0), (311, 1080)]);
        assert_eq!(positions(&[(2560, None), (1920, None)]), [(0, 0), (2560, 0)]);
        assert_eq!(positions(&[(1920, None), (2560, Some((0, 0)))]), [(2560, 0), (0, 0)]);
    }
}
