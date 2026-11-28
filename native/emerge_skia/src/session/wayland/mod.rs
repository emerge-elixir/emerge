//! Window-system adapter only. All tree, raster and target state belongs to the session.
#[cfg(feature = "vulkan")]
mod feedback;
mod geometry;
#[cfg(feature = "opengl")]
mod gl;
#[cfg(any(feature = "opengl", feature = "vulkan"))]
mod gpu;
#[cfg(any(feature = "opengl", feature = "vulkan"))]
mod textures;
#[cfg(any(feature = "opengl", feature = "vulkan"))]
pub(super) use textures::Lease;
#[cfg(any(feature = "opengl", feature = "vulkan"))]
#[path = "../../backend/wayland/handles.rs"]
mod handles;
mod pointer;
mod shm;
mod touch;
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
use super::{
    Completed, Cpu, Display, Owner, Pixels, Ready, Target, Wake, presentation::Frame, tree_actor,
};
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
pub(super) use shm::Slots;
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
    display: Display,
    shm_claim: Option<(Frame<Completed>, Mapping)>,
    #[cfg(feature = "stage-probes")]
    held_targets: Vec<Arc<dyn Send + Sync>>,
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
    key_focus: bool,
    loop_handle: sctk::reexports::calloop::LoopHandle<'static, WindowApp>,
    touch: touch::Contacts,
    mods: u8,
    configured: bool,
    geometry: Geometry,
    // A failed preparation is not installed; retry on an edit or different
    // configure, not on every pointer motion or release of an unrelated slot.
    attempted: Option<Geometry>,
    installed: Option<Mapping>,
    frame_pending: bool,

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
    mut display: Display,
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
    display.toplevel = Some(window.xdg_toplevel().clone());
    let mut app = WindowApp {
        owner: owner.clone(),
        display,
        shm_claim: None,
        #[cfg(feature = "stage-probes")]
        held_targets: Vec::with_capacity(4),
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
        key_focus: false,
        loop_handle: events.handle(),
        touch: Default::default(),
        mods: 0,
        configured: false,
        geometry: Geometry {
            logical: size,
            scale: Scale::Integer(1),
        },
        attempted: None,
        installed: None,
        frame_pending: false,
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
        app.init_device(&conn, api, &ping)?;
        app.allow_cursor_theme = app.gpu.is_none();
    }
    let wake = Wake::Window(ping);
    let cpu = matches!(owner.selected.get(), None | Some(super::Selected::Raster))
        .then(|| Cpu::Shm(app.shm.wl_shm().clone().into(), Box::default()));
    tree_actor::run(owner, &wake, cpu, || {
        ready
            .send(Ok(wake.clone()))
            .map_err(|_| "Wayland startup cancelled")?;
        while !owner.lifecycle.stop.load(Ordering::Acquire) {
            let wait = app.present(&conn, &qh)?;
            let until_report = app
                .display
                .report_due(owner)
                .into_iter()
                .chain(app.display.input_deadline(owner, &mut app.env))
                .min()
                .map(|at| at.saturating_duration_since(Instant::now()));
            events
                .dispatch(wait.into_iter().chain(until_report).min(), &mut app)
                .map_err(err)?;
        }
        Ok(())
    })
}
impl WindowApp {
    // Simulate delayed release, not delayed display feedback. Hold exactly
    // the submitted target lease, never the scene/route or a writable target.
    #[cfg(feature = "stage-probes")]
    fn pin_target(&mut self, bundle: &super::Bundle, target: Arc<dyn Send + Sync>) {
        use super::probes::Stage;
        if !self.owner.probes.open(Stage::RetainTargets)
            && !self.held_targets.iter().any(|p| Arc::ptr_eq(p, &target))
        {
            debug_assert!(self.held_targets.len() < 4);
            self.held_targets.push(target);
            self.owner
                .probes
                .emit(Stage::RetainTargets, bundle.id.serial, bundle.revision);
        }
    }
    fn present(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
    ) -> Result<Option<Duration>, String> {
        #[cfg(any(feature = "opengl", feature = "vulkan"))]
        self.device_report()?;
        self.display.controls(&self.owner, &mut self.env);
        #[cfg(feature = "stage-probes")]
        if self.owner.probes.open(super::probes::Stage::RetainTargets)
            && !self.held_targets.is_empty()
        {
            self.held_targets.clear();
            self.owner.render.notify();
        }
        if !self.configured {
            return Ok(None);
        }
        let mapping = match self.geometry.mapping() {
            Ok(m) => m,
            Err(reason) => {
                if self.attempted != Some(self.geometry) {
                    self.attempted = Some(self.geometry);
                    // Retire all candidates for the previous configure, even on failure.
                    self.owner.invalidate_view()?;
                    if let Some(pid) = *lock(&self.owner.log)? {
                        let _ = self.env.send_and_clear(&pid, |_| {
                            (super::emerge_skia_log(), super::error(), "wayland", reason)
                        });
                    }
                }
                return Ok(None);
            }
        };
        if self.attempted != Some(self.geometry) {
            self.attempted = Some(self.geometry);
            self.owner
                .geometry(mapping.buffer, self.geometry.nominal())?;
        }
        let changed_surface = self.installed != Some(mapping);
        #[cfg(any(feature = "opengl", feature = "vulkan"))]
        if self.gpu.is_some() {
            return self.present_gpu(qh, mapping);
        }
        let open = self
            .owner
            .opportunity(self.shm_claim.is_none() && (!self.frame_pending || changed_surface));
        {
            let mut p = lock(&self.owner.pipe)?;
            p.submit_open = open;
            if open && let Some(f) = p.completed.take() {
                #[cfg(feature = "stage-probes")]
                self.owner.probes.emit(
                    crate::session::probes::Stage::Claimed,
                    f.id.serial,
                    f.value.bundle.revision,
                );
                self.shm_claim = Some((f, mapping));
                p.submit_open = false;
            }
        }
        // wl_buffer.release arrives independently of frame callbacks.
        self.owner.render.notify();
        #[cfg(feature = "stage-probes")]
        if !self
            .owner
            .probes
            .open(crate::session::probes::Stage::Claimed)
        {
            return Ok(None);
        }
        let Some((frame, claimed_mapping)) = self.shm_claim.take() else {
            return Ok(None);
        };
        if mapping != claimed_mapping
            || lock(&self.owner.pipe)?
                .view
                .is_none_or(|v| v.generation != frame.id.generation)
        {
            return Ok(Some(Duration::ZERO));
        }
        #[cfg(feature = "stage-probes")]
        if self
            .owner
            .probes
            .reject(frame.id, frame.value.bundle.revision)
        {
            let loser = lock(&self.owner.pipe)?.completed.retry(frame);
            drop(loser);
            return Ok(Some(Duration::ZERO));
        }
        let Completed { bundle, target } = frame.value;
        let Target::Shm(buffer, rgba) = target else {
            return Err("unexpected shm target".into());
        };
        let current = self.display.installed.as_ref();
        let reuse = !changed_surface
            && current.is_some_and(|i| {
                i.bundle.view.generation == bundle.view.generation
                    && i.pixels.cpu().is_some_and(|old| Arc::ptr_eq(old, &rgba))
            });
        let elapsed = if reuse {
            None
        } else {
            let started = Instant::now();
            let surface = self.window.wl_surface();
            self.protocols.apply(surface, mapping.geometry);
            if surface.version() >= 4 {
                surface.damage_buffer(0, 0, mapping.buffer.0 as i32, mapping.buffer.1 as i32);
            } else {
                surface.damage(
                    0,
                    0,
                    self.geometry.logical.0 as i32,
                    self.geometry.logical.1 as i32,
                );
            }
            buffer.attach_to(surface).map_err(err)?;
            if !self.frame_pending {
                surface.frame(qh, surface.clone());
                self.display.callback = super::stats::Callback::arm(&bundle);
                self.frame_pending = true;
            }
            self.window.commit();
            conn.flush().map_err(err)?;
            let handed = Instant::now();
            #[cfg(feature = "stage-probes")]
            {
                self.owner.probes.confirm(bundle.id, bundle.revision)?;
                self.pin_target(&bundle, buffer);
            }
            Some(started..handed)
        };
        self.install_mapping(mapping);
        self.display.install(
            &self.owner,
            &mut self.env,
            bundle,
            Pixels::Cpu(rgba),
            elapsed,
        )?;
        lock(&self.owner.pipe)?.submit_open = self.owner.opportunity(!self.frame_pending);
        Ok(None)
    }
    fn input(&mut self, event: InputEvent) {
        self.input_at(event, Instant::now());
    }
    fn input_at(&mut self, event: InputEvent, at: Instant) {
        self.env
            .run(|env| self.display.dispatch(&self.owner, env, event, at));
        self.env.clear();
    }
    // Only a submitted buffer changes the coordinate system. A stationary
    // contact must not turn into a pan (or acquire velocity) on wl_touch.up.
    fn install_mapping(&mut self, next: Mapping) {
        if let Some(old) = self.installed.replace(next)
            && old != next
        {
            let (from, to) = (old.ratios(), next.ratios());
            if from != to {
                self.display.input.remap(
                    |p| std::array::from_fn(|i| (f64::from(p[i]) * (to[i] / from[i])) as f32),
                    |id| self.touch.position(id, next),
                );
            }
        }
    }
    fn output_interval(&mut self, o: &wl_output::WlOutput) {
        if let Some(mode) = self
            .outputs
            .info(o)
            .and_then(|o| o.modes.into_iter().find(|m| m.current))
        {
            self.owner.stats.interval(
                (mode.refresh_rate > 0)
                    .then(|| Duration::from_secs_f64(1000. / f64::from(mode.refresh_rate))),
            );
            if mode.refresh_rate > 0
                && let Ok(mut p) = self.owner.pipe.lock()
            {
                p.clock.cadence(Duration::from_secs_f64(
                    1000. / f64::from(mode.refresh_rate),
                ));
                self.owner.actor.notify();
            }
        }
    }
    fn set_scale(&mut self, next: Scale) {
        if self.geometry.scale != next {
            self.geometry.scale = next;
            self.resized();
        }
    }
    fn resized(&mut self) {
        // Don't assign feedback spanning a configure to either mapping.
        self.display.callback = None;
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
        if let Some(target) = self.display.input.target {
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
            let now = Instant::now();
            super::stats::Callback::received(&mut self.display.callback, now, &self.owner.stats);
            if let Ok(mut p) = self.owner.pipe.lock() {
                p.clock.feedback(now);
                self.owner.actor.notify();
            }
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
