//! Window-system adapter only. All tree, raster and target state belongs to the session.
#[cfg(feature = "vulkan")]
mod feedback;
mod geometry;
#[cfg(feature = "opengl")]
mod gl;
#[cfg(any(feature = "opengl", feature = "vulkan"))]
mod gpu;
#[cfg(any(feature = "opengl", feature = "vulkan"))]
#[path = "../../backend/wayland/handles.rs"]
mod handles;
mod pointer;
mod shm;
#[cfg(feature = "vulkan")]
mod vulkan;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Requested {
    Raster,
    #[cfg(feature = "opengl")]
    Gl,
    #[cfg(feature = "vulkan")]
    Vk,
}
impl Requested {
    pub(super) fn parse(kind: &str, present: &str) -> Result<Self, String> {
        match (kind, present) {
            ("raster", "auto" | "cpu") => Ok(Self::Raster),
            #[cfg(feature = "opengl")]
            ("opengl", "auto") => Ok(Self::Gl),
            #[cfg(feature = "vulkan")]
            ("vulkan", "auto") => Ok(Self::Vk),
            _ => Err(format!(
                "unsupported Wayland renderer {kind}/{present}: check compiled features and explicit renderer options"
            )),
        }
    }
}
use super::{Owner, Ready, Wake};
use crate::{input::InputEvent, lock};
use geometry::{Geometry, Mapping, Protocols, Scale};
use rustler::OwnedEnv;
pub(super) use sctk::reexports::calloop::ping::Ping;
use sctk::{
    compositor::{CompositorHandler, CompositorState},
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, ping::make_ping},
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    seat::{SeatState, pointer::ThemedPointer},
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler},
};
use shm::Slots;
use smithay_client_toolkit as sctk;
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_seat, wl_surface},
};
pub(super) use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel as Toplevel;
rustler::atoms! {emerge_skia_close, window_close_requested}

struct WindowApp {
    owner: Arc<Owner>,
    env: OwnedEnv,
    protocols: Protocols,
    window: Window,
    registry: RegistryState,
    compositor: CompositorState,
    outputs: OutputState,
    seats: SeatState,
    shm: Shm,
    pointer: Option<(wl_seat::WlSeat, pointer::Pointer)>,
    keyboard: Option<(wl_seat::WlSeat, wl_keyboard::WlKeyboard)>,
    mods: u8,
    configured: bool,
    geometry: Geometry,
    // A failed preparation is not installed; retry on an edit or different
    // configure, not on every pointer motion or release of an unrelated slot.
    attempted: Option<Geometry>,
    installed: Option<Mapping>,
    frame_pending: bool,
    buffers: Slots,
    #[cfg(any(feature = "opengl", feature = "vulkan"))]
    gpu: Option<gpu::Output>,
    #[cfg(any(feature = "opengl", feature = "vulkan"))]
    callback_armed: bool,
    #[cfg(feature = "vulkan")]
    feedback: feedback::Feedback,
    allow_cursor_theme: bool,
    cursor_shapes: Option<sctk::seat::pointer::cursor_shape::CursorShapeManager>,
}
fn err(e: impl std::fmt::Display) -> String {
    format!("Wayland: {e}")
}
pub(super) fn startup_socket(conn: &Connection) -> Result<std::os::unix::net::UnixStream, String> {
    use std::os::fd::AsFd;
    Ok(conn.as_fd().try_clone_to_owned().map_err(err)?.into())
}
pub(super) fn run(
    owner: &Arc<Owner>,
    ready: &Ready,
    conn: Connection,
    title: String,
    size: (u32, u32),
    api: Requested,
) -> Result<(), String> {
    let (globals, queue) = registry_queue_init(&conn).map_err(err)?;
    let qh = queue.handle();
    let mut events = EventLoop::<WindowApp>::try_new().map_err(err)?;
    WaylandSource::new(conn.clone(), queue)
        .insert(events.handle())
        .map_err(err)?;
    let (ping, source) = make_ping().map_err(err)?;
    events
        .handle()
        .insert_source(source, |_, _, _| {})
        .map_err(err)?;
    let compositor = CompositorState::bind(&globals, &qh).map_err(err)?;
    let shell = XdgShell::bind(&globals, &qh).map_err(err)?;
    let shm = Shm::bind(&globals, &qh).map_err(err)?;
    let window = shell.create_window(
        compositor.create_surface(&qh),
        WindowDecorations::RequestServer,
        &qh,
    );
    window.set_title(title);
    window.set_app_id("org.emerge.viewport");
    let protocols = Protocols::bind(&globals, &qh, window.wl_surface());
    window.commit(); // Empty initial commit solicits configure; no hits or routes yet.
    conn.flush().map_err(err)?;
    lock(&owner.live)?
        .as_mut()
        .ok_or("renderer stopped")?
        .toplevel = Some(window.xdg_toplevel().clone());
    let mut app = WindowApp {
        owner: owner.clone(),
        env: OwnedEnv::new(),
        protocols,
        #[cfg(feature = "vulkan")]
        feedback: feedback::Feedback::new(&globals, &qh, &window, api)?,
        allow_cursor_theme: api == Requested::Raster,
        cursor_shapes: (api != Requested::Raster)
            .then(|| {
                sctk::seat::pointer::cursor_shape::CursorShapeManager::bind(&globals, &qh).ok()
            })
            .flatten(),
        #[cfg(any(feature = "opengl", feature = "vulkan"))]
        gpu: None,
        #[cfg(any(feature = "opengl", feature = "vulkan"))]
        callback_armed: false,
        window,
        registry: RegistryState::new(&globals),
        compositor,
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        shm,
        pointer: None,
        keyboard: None,
        mods: 0,
        configured: false,
        geometry: Geometry {
            logical: size,
            scale: Scale::Integer(1),
        },
        attempted: None,
        installed: None,
        frame_pending: false,
        buffers: Slots::default(),
    };
    #[cfg(any(feature = "opengl", feature = "vulkan"))]
    if api != Requested::Raster {
        while !app.configured || {
            #[cfg(feature = "vulkan")]
            {
                app.feedback.pending()
            }
            #[cfg(not(feature = "vulkan"))]
            {
                false
            }
        } {
            if owner.lifecycle.stop.load(Ordering::Acquire) {
                return Err("Wayland startup cancelled".into());
            }
            events.dispatch(None, &mut app).map_err(err)?;
        }
        app.init_device(&conn, api)?;
        app.allow_cursor_theme = app.gpu.is_none();
    }
    ready
        .send(Ok(Wake::Window(ping)))
        .map_err(|_| "Wayland startup cancelled")?;
    while !owner.lifecycle.stop.load(Ordering::Acquire) {
        let wait = app.present(&conn, &qh)?;
        events.dispatch(wait, &mut app).map_err(err)?;
    }
    Ok(())
}
impl WindowApp {
    fn present(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
    ) -> Result<Option<Duration>, String> {
        #[cfg(any(feature = "opengl", feature = "vulkan"))]
        let work = self.gpu.as_mut().is_some_and(|gpu| {
            gpu.readback();
            gpu.pending()
        });
        #[cfg(not(any(feature = "opengl", feature = "vulkan")))]
        let work = false;
        let changed_surface = self.installed.is_none_or(|m| m.geometry != self.geometry);
        if !self.configured || (self.frame_pending && !changed_surface) {
            return Ok(None);
        }
        let mut guard = lock(&self.owner.live)?;
        let live = guard.as_mut().ok_or("renderer stopped")?;
        if !work && !live.request && self.attempted.is_none_or(|g| g == self.geometry) {
            return Ok(None);
        }
        if let Some(deadline) = live.last.zip(live.interval).map(|(t, d)| t + d)
            && let Some(wait) = deadline.checked_duration_since(Instant::now())
        {
            return Ok(Some(wait));
        }
        let mapping = match self.geometry.mapping() {
            Ok(mapping) => mapping,
            Err(why) => {
                self.attempted = Some(self.geometry);
                live.request = false;
                live.log_frame_error(&mut self.env, "wayland", why);
                return Ok(None);
            }
        };
        let size = mapping.buffer;
        #[cfg(any(feature = "opengl", feature = "vulkan"))]
        if self.gpu.is_some() {
            drop(guard);
            self.attempted = Some(self.geometry);
            return self.present_gpu(conn, qh, mapping, changed_surface);
        }
        // No available slot: don't mutate the installed raster or sample input
        // geometry. A wl_buffer.release wakes this same event loop.
        let Some(i) = self.buffers.available(size) else {
            return Ok(None);
        };
        self.attempted = Some(self.geometry);
        let presented = live.present(
            &mut self.env,
            size,
            self.geometry.nominal(),
            "wayland",
            |_, pixels, reuse| {
                if reuse && !changed_surface {
                    return Ok(false);
                }
                let surface = self.window.wl_surface();
                let buffer = self.buffers.write(i, &self.shm, size, pixels)?;
                self.protocols.apply(surface, mapping.geometry);
                if surface.version() >= 4 {
                    surface.damage_buffer(0, 0, size.0 as i32, size.1 as i32)
                } else {
                    surface.damage(
                        0,
                        0,
                        self.geometry.logical.0 as i32,
                        self.geometry.logical.1 as i32,
                    )
                }
                buffer.attach_to(surface).map_err(err)?;
                if !self.frame_pending {
                    surface.frame(qh, surface.clone());
                    self.frame_pending = true;
                }
                self.window.commit();
                conn.flush().map(|_| true).map_err(err)
            },
        )?;
        if presented {
            self.installed = Some(mapping);
        }
        Ok(None)
    }
    fn input(&mut self, event: InputEvent) {
        if let Ok(mut guard) = self.owner.live.lock()
            && let Some(live) = guard.as_mut()
        {
            self.env
                .run(|env| live.input.dispatch(env, &live.raster, event));
            self.env.clear();
        }
    }
    fn output_interval(&self, o: &wl_output::WlOutput) {
        if let Some(mode) = self
            .outputs
            .info(o)
            .and_then(|o| o.modes.into_iter().find(|m| m.current))
            && let Ok(mut guard) = self.owner.live.lock()
            && let Some(stats) = guard.as_mut().and_then(|s| s.stats.as_mut())
        {
            stats.output_mode(mode.refresh_rate);
        }
    }
    fn set_scale(&mut self, next: Scale) {
        if self.geometry.scale != next {
            self.geometry.scale = next;
            self.resized();
        }
    }
    fn resized(&mut self) {
        if let Ok((width, height)) = self.geometry.buffer() {
            self.input(InputEvent::Resized {
                width,
                height,
                scale_factor: self.geometry.nominal(),
            });
        }
    }
}
impl WindowHandler for WindowApp {
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        c: WindowConfigure,
        _: u32,
    ) {
        let old = self.geometry;
        self.geometry.logical = (
            c.new_size.0.map_or(old.logical.0, |n| n.get()),
            c.new_size.1.map_or(old.logical.1, |n| n.get()),
        );
        if !self.configured || old != self.geometry {
            self.resized();
        }
        self.configured = true; // SCTK acknowledges the serial before this handler.
    }
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        if let Ok(guard) = self.owner.live.lock()
            && let Some(target) = guard.as_ref().and_then(|live| live.input.target)
        {
            let _ = self
                .env
                .send_and_clear(&target, |_| (emerge_skia_close(), window_close_requested()));
        }
        self.owner.lifecycle.request_stop();
    }
}
impl CompositorHandler for WindowApp {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        s: &wl_surface::WlSurface,
        n: i32,
    ) {
        if s == self.window.wl_surface()
            && s.version() >= 3
            && matches!(self.geometry.scale, Scale::Integer(_))
        {
            self.set_scale(Scale::Integer(n.max(1) as u32));
        }
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, s: &wl_surface::WlSurface, _: u32) {
        if s == self.window.wl_surface() {
            #[cfg(any(feature = "opengl", feature = "vulkan"))]
            {
                self.callback_armed = false;
            }
            self.frame_pending = false;
        }
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        s: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        if s == self.window.wl_surface() {
            self.output_interval(output);
        }
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}
impl OutputHandler for WindowApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self
            .window
            .wl_surface()
            .data::<sctk::compositor::SurfaceData>()
            .is_some_and(|s| s.outputs().any(|o| o == output))
        {
            self.output_interval(&output);
        }
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}
impl ShmHandler for WindowApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}
impl ProvidesRegistryState for WindowApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    sctk::registry_handlers!(OutputState, SeatState);
}
sctk::delegate_registry!(WindowApp);
sctk::delegate_output!(WindowApp);
sctk::delegate_compositor!(WindowApp);
sctk::delegate_shm!(WindowApp);
sctk::delegate_xdg_shell!(WindowApp);
sctk::delegate_xdg_window!(WindowApp);
