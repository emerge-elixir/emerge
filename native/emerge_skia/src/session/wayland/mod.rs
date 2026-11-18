//! Window-system adapter only. All tree, raster and target state belongs to the session.
mod pointer;
mod shm;
use super::{Owner, Ready, Wake};
use crate::{input::InputEvent, lock};
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
use shm::{Geometry, Slots};
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
    window: Window,
    registry: RegistryState,
    compositor: CompositorState,
    outputs: OutputState,
    seats: SeatState,
    shm: Shm,
    pointer: Option<(wl_seat::WlSeat, ThemedPointer)>,
    keyboard: Option<(wl_seat::WlSeat, wl_keyboard::WlKeyboard)>,
    mods: u8,
    configured: bool,
    geometry: Geometry,
    // A failed preparation is not installed; retry on an edit or different
    // configure, not on every pointer motion or release of an unrelated slot.
    attempted: Option<Geometry>,
    installed: Option<Geometry>,
    frame_pending: bool,
    buffers: Slots,
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
    window.commit(); // Empty initial commit solicits configure; no hits or routes yet.
    conn.flush().map_err(err)?;
    lock(&owner.live)?
        .as_mut()
        .ok_or("renderer stopped")?
        .toplevel = Some(window.xdg_toplevel().clone());
    let mut app = WindowApp {
        owner: owner.clone(),
        env: OwnedEnv::new(),
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
            scale: 1,
        },
        attempted: None,
        installed: None,
        frame_pending: false,
        buffers: Slots::default(),
    };
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
        let changed_surface = self.installed != Some(self.geometry);
        if !self.configured || (self.frame_pending && !changed_surface) {
            return Ok(None);
        }
        let mut guard = lock(&self.owner.live)?;
        let live = guard.as_mut().ok_or("renderer stopped")?;
        if !live.request && self.attempted.is_none_or(|g| g == self.geometry) {
            return Ok(None);
        }
        if let Some(deadline) = live.last.zip(live.interval).map(|(t, d)| t + d)
            && let Some(wait) = deadline.checked_duration_since(Instant::now())
        {
            return Ok(Some(wait));
        }
        let size = match self.geometry.buffer() {
            Ok(size) => size,
            Err(why) => {
                self.attempted = Some(self.geometry);
                live.request = false;
                live.log_frame_error(&mut self.env, "wayland", why);
                return Ok(None);
            }
        };
        // No available slot: don't mutate the installed raster or sample input
        // geometry. A wl_buffer.release wakes this same event loop.
        let Some(i) = self.buffers.available(size) else {
            return Ok(None);
        };
        self.attempted = Some(self.geometry);
        let presented = live.present(
            &mut self.env,
            size,
            self.geometry.scale as f32,
            "wayland",
            |_, pixels, reuse| {
                if reuse && !changed_surface {
                    return Ok(());
                }
                let surface = self.window.wl_surface();
                let buffer = self.buffers.write(i, &self.shm, size, pixels)?;
                if surface.version() >= 3 {
                    surface.set_buffer_scale(self.geometry.scale as i32);
                }
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
                conn.flush().map_err(err)
            },
        )?;
        if presented {
            self.installed = Some(self.geometry);
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
    fn resized(&mut self) {
        if let Ok((width, height)) = self.geometry.buffer() {
            self.input(InputEvent::Resized {
                width,
                height,
                scale_factor: self.geometry.scale as f32,
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
        if s == self.window.wl_surface() && s.version() >= 3 {
            self.geometry.scale = n.max(1) as u32;
            self.resized();
        }
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, s: &wl_surface::WlSurface, _: u32) {
        if s == self.window.wl_surface() {
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
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
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
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
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
