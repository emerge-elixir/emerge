//! Wayland backend built on smithay-client-toolkit.

#[path = "wayland/egl.rs"]
mod egl;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender as CrossbeamSender, TrySendError};
use glutin::prelude::GlSurface;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_registry, delegate_xdg_shell,
    delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{self, EventLoop},
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{Event as FractionalScaleEvent, WpFractionalScaleV1},
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

use crate::{
    actors::{EventMsg, RenderMsg, TreeMsg},
    backend::wake::{
        BackendWake, BackendWakeHandle, WindowBackendStartupInfo, WindowBackendStartupResult,
    },
    backend::wayland_config::WaylandConfig,
    input::InputEvent,
    renderer::RenderState,
    video::VideoRegistry,
};

use self::egl::{GlEnv, create_gl_env, resize_gl_env};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FrameCallbackState {
    #[default]
    None,
    Requested,
    Received,
}

#[derive(Clone, Debug)]
enum WakeAction {
    Stop,
    Redraw,
    VideoFrameAvailable,
}

#[derive(Clone)]
struct WaylandWake {
    tx: calloop::channel::Sender<WakeAction>,
}

impl BackendWake for WaylandWake {
    fn request_stop(&self) {
        let _ = self.tx.send(WakeAction::Stop);
    }

    fn request_redraw(&self) {
        let _ = self.tx.send(WakeAction::Redraw);
    }

    fn notify_video_frame(&self) {
        let _ = self.tx.send(WakeAction::VideoFrameAvailable);
    }
}

struct ProtocolHandles {
    _compositor_state: CompositorState,
    _viewporter: Option<WpViewporter>,
    viewport: Option<WpViewport>,
    _fractional_scale: Option<WpFractionalScaleV1>,
}

impl ProtocolHandles {
    fn new(
        globals: &wayland_client::globals::GlobalList,
        qh: &QueueHandle<WaylandApp>,
        compositor_state: CompositorState,
        window: &Window,
    ) -> Self {
        let viewporter = globals.bind(qh, 1..=1, ()).ok();
        let viewport = viewporter
            .as_ref()
            .map(|viewporter: &WpViewporter| viewporter.get_viewport(window.wl_surface(), qh, ()));
        let fractional_scale_manager = globals.bind(qh, 1..=1, ()).ok();
        let fractional_scale =
            fractional_scale_manager
                .as_ref()
                .and_then(|manager: &WpFractionalScaleManagerV1| {
                    viewport.as_ref().map(|_| {
                        manager.get_fractional_scale(
                            window.wl_surface(),
                            qh,
                            FractionalScaleData {
                                surface: window.wl_surface().clone(),
                            },
                        )
                    })
                });

        Self {
            _compositor_state: compositor_state,
            _viewporter: viewporter,
            viewport,
            _fractional_scale: fractional_scale,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SurfaceGeometry {
    logical_size: (u32, u32),
    buffer_size: (u32, u32),
    integer_scale_factor: u32,
    preferred_fractional_scale: Option<f32>,
    scale_factor: f32,
}

impl SurfaceGeometry {
    fn new(config: &WaylandConfig) -> Self {
        let logical_size = (config.width.max(1), config.height.max(1));

        Self {
            logical_size,
            buffer_size: logical_size,
            integer_scale_factor: 1,
            preferred_fractional_scale: None,
            scale_factor: 1.0,
        }
    }

    fn set_logical_size(&mut self, width: u32, height: u32) -> bool {
        let next_size = (width.max(1), height.max(1));

        if self.logical_size == next_size {
            return false;
        }

        self.logical_size = next_size;
        true
    }

    fn set_integer_scale_factor(&mut self, scale_factor: i32) {
        self.integer_scale_factor = scale_factor.max(1) as u32;
    }

    fn set_preferred_fractional_scale(&mut self, scale_factor: Option<f32>) {
        self.preferred_fractional_scale = scale_factor;
    }

    fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    fn apply_to_surface(&mut self, window: &Window, viewport: Option<&WpViewport>) {
        let scale_factor = self.current_scale_factor();
        let fractional_active = self.preferred_fractional_scale.is_some() && viewport.is_some();

        if fractional_active {
            let _ = window.set_buffer_scale(1);

            if let Some(viewport) = viewport {
                viewport.set_destination(self.logical_size.0 as i32, self.logical_size.1 as i32);
            }
        } else {
            let _ = window.set_buffer_scale(self.integer_scale_factor.max(1));

            if let Some(viewport) = viewport {
                viewport.set_destination(-1, -1);
            }
        }

        self.scale_factor = scale_factor;
        self.buffer_size = Self::buffer_dimensions(self.logical_size, scale_factor);
    }

    fn current_scale_factor(&self) -> f32 {
        self.preferred_fractional_scale
            .unwrap_or(self.integer_scale_factor.max(1) as f32)
            .max(1.0)
    }

    fn buffer_dimensions(logical_size: (u32, u32), scale_factor: f32) -> (u32, u32) {
        let width = ((logical_size.0.max(1) as f64) * scale_factor as f64).round() as u32;
        let height = ((logical_size.1.max(1) as f64) * scale_factor as f64).round() as u32;

        (width.max(1), height.max(1))
    }
}

#[derive(Clone, Copy, Debug)]
struct PresentState {
    configured: bool,
    redraw_requested: bool,
    frame_callback_state: FrameCallbackState,
    last_present_at: Option<Instant>,
    estimated_frame_interval: Duration,
}

impl Default for PresentState {
    fn default() -> Self {
        Self {
            configured: false,
            redraw_requested: false,
            frame_callback_state: FrameCallbackState::None,
            last_present_at: None,
            estimated_frame_interval: Duration::from_millis(16),
        }
    }
}

impl PresentState {
    fn queue_redraw(&mut self) {
        self.redraw_requested = true;
    }

    fn can_draw(&self, exit: bool) -> bool {
        !exit
            && self.configured
            && self.redraw_requested
            && self.frame_callback_state != FrameCallbackState::Requested
    }

    fn request_frame_callback(&mut self, window: &Window, qh: &QueueHandle<WaylandApp>) {
        match self.frame_callback_state {
            FrameCallbackState::None | FrameCallbackState::Received => {
                window.wl_surface().frame(qh, window.wl_surface().clone());
                self.frame_callback_state = FrameCallbackState::Requested;
            }
            FrameCallbackState::Requested => {}
        }
    }

    fn frame_callback_received(&mut self) {
        self.frame_callback_state = FrameCallbackState::Received;
    }

    fn finish_present(&mut self, video_needs_cleanup: bool) {
        self.redraw_requested = video_needs_cleanup;
    }

    fn observe_present(&mut self, presented_at: Instant) -> Instant {
        if let Some(last_present_at) = self.last_present_at {
            let observed = presented_at.saturating_duration_since(last_present_at);
            if observed >= Duration::from_millis(4) && observed <= Duration::from_millis(100) {
                self.estimated_frame_interval = observed;
            }
        }

        self.last_present_at = Some(presented_at);
        presented_at + self.estimated_frame_interval
    }
}

struct WaylandApp {
    registry_state: RegistryState,
    output_state: OutputState,
    qh: QueueHandle<Self>,
    window: Window,
    env: Option<GlEnv>,
    protocols: ProtocolHandles,
    geometry: SurfaceGeometry,
    present: PresentState,
    exit: bool,
    running_flag: Arc<AtomicBool>,
    tree_tx: CrossbeamSender<TreeMsg>,
    render_rx: Receiver<RenderMsg>,
    event_tx: crossbeam_channel::Sender<EventMsg>,
    video_registry: Arc<VideoRegistry>,
    render_state: RenderState,
}

#[derive(Clone)]
struct FractionalScaleData {
    surface: wl_surface::WlSurface,
}

impl WaylandApp {
    fn new(
        conn: &Connection,
        globals: &wayland_client::globals::GlobalList,
        qh: QueueHandle<Self>,
        running_flag: Arc<AtomicBool>,
        tree_tx: CrossbeamSender<TreeMsg>,
        event_tx: crossbeam_channel::Sender<EventMsg>,
        render_rx: Receiver<RenderMsg>,
        video_registry: Arc<VideoRegistry>,
        config: &WaylandConfig,
    ) -> Result<Self, String> {
        let compositor_state = CompositorState::bind(globals, &qh)
            .map_err(|err| format!("wl_compositor not available: {err}"))?;
        let xdg_shell = XdgShell::bind(globals, &qh)
            .map_err(|err| format!("xdg shell not available: {err}"))?;

        let surface = compositor_state.create_surface(&qh);
        let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
        window.set_title(&config.title);
        window.set_app_id("dev.emerge.emerge_skia");

        let protocols = ProtocolHandles::new(globals, &qh, compositor_state, &window);

        window.commit();

        let mut app = Self {
            registry_state: RegistryState::new(globals),
            output_state: OutputState::new(globals, &qh),
            qh,
            window,
            env: None,
            protocols,
            geometry: SurfaceGeometry::new(config),
            present: PresentState::default(),
            exit: false,
            running_flag,
            tree_tx,
            render_rx,
            event_tx,
            video_registry,
            render_state: RenderState::default(),
        };

        app.apply_surface_scale_state();

        if app.geometry.buffer_size != app.geometry.logical_size {
            app.reconfigure_surface_geometry(conn);
        }

        Ok(app)
    }

    fn handle_wake_action(&mut self, action: WakeAction) {
        match action {
            WakeAction::Stop => {
                self.running_flag.store(false, Ordering::Relaxed);
                self.exit = true;
            }
            WakeAction::Redraw => {
                self.flush_render_updates();
            }
            WakeAction::VideoFrameAvailable => {
                self.queue_redraw();
            }
        }
    }

    fn queue_redraw(&mut self) {
        self.present.queue_redraw();
    }

    fn flush_render_updates(&mut self) {
        if self.exit {
            return;
        }

        if self.drain_render_messages() {
            self.queue_redraw();
        }
    }

    fn drain_render_messages(&mut self) -> bool {
        let mut updated = false;

        while let Ok(msg) = self.render_rx.try_recv() {
            match msg {
                RenderMsg::Commands {
                    commands,
                    version,
                    animate,
                    ..
                } => {
                    self.render_state.commands = commands;
                    self.render_state.render_version = version;
                    self.render_state.animate = animate;
                    updated = true;
                }
                RenderMsg::CursorUpdate { .. } => {}
                RenderMsg::Stop => {
                    self.running_flag.store(false, Ordering::Relaxed);
                    self.exit = true;
                    return false;
                }
            }
        }

        updated
    }

    fn update_logical_size(&mut self, conn: &Connection, width: u32, height: u32) {
        if !self.geometry.set_logical_size(width, height) {
            return;
        }

        self.reconfigure_surface_geometry(conn);
    }

    fn maybe_draw(&mut self) {
        if !self.present.can_draw(self.exit) {
            return;
        }

        self.draw();
    }

    fn draw(&mut self) {
        self.present.request_frame_callback(&self.window, &self.qh);

        let Some(env) = self.env.as_mut() else {
            return;
        };

        let mut video_needs_cleanup = false;

        match env.renderer.sync_video_frames(&self.video_registry, None) {
            Ok(result) => video_needs_cleanup = result.needs_cleanup,
            Err(err) => eprintln!("video sync failed: {err}"),
        }

        env.renderer.render(&self.render_state);

        if let Err(err) = env.gl_surface.swap_buffers(&env.gl_context) {
            eprintln!("wayland egl swap_buffers failed: {err}");
            self.running_flag.store(false, Ordering::Relaxed);
            self.exit = true;
            return;
        }

        let presented_at = Instant::now();
        let predicted_next_present_at = self.present.observe_present(presented_at);

        if self.render_state.animate {
            self.send_animation_pulse(presented_at, predicted_next_present_at);
        }

        self.present.finish_present(video_needs_cleanup);
    }

    fn apply_surface_scale_state(&mut self) {
        self.geometry
            .apply_to_surface(&self.window, self.protocols.viewport.as_ref());
    }

    fn reconfigure_surface_geometry(&mut self, conn: &Connection) {
        let previous = self.geometry;

        self.apply_surface_scale_state();

        if !self.present.configured && self.env.is_none() {
            return;
        }

        if self.geometry.buffer_size.0 == 0 || self.geometry.buffer_size.1 == 0 {
            return;
        }

        let geometry_changed = previous != self.geometry;
        let buffer_changed = previous.buffer_size != self.geometry.buffer_size;

        if self.env.is_none() {
            match create_gl_env(conn, self.window.wl_surface(), self.geometry.buffer_size) {
                Ok(env) => self.env = Some(env),
                Err(err) => {
                    eprintln!("wayland egl setup failed: {err}");
                    self.running_flag.store(false, Ordering::Relaxed);
                    self.exit = true;
                    return;
                }
            }
        } else if buffer_changed && let Some(env) = self.env.as_mut() {
            resize_gl_env(env, self.geometry.buffer_size);
        }

        if geometry_changed {
            self.queue_redraw();

            let _ = self
                .event_tx
                .send(EventMsg::InputEvent(InputEvent::Resized {
                    width: self.geometry.buffer_size.0,
                    height: self.geometry.buffer_size.1,
                    scale_factor: self.geometry.scale_factor(),
                }));
        }
    }

    fn send_animation_pulse(&self, presented_at: Instant, predicted_next_present_at: Instant) {
        let msg = TreeMsg::AnimationPulse {
            presented_at,
            predicted_next_present_at,
        };

        match self.tree_tx.try_send(msg) {
            Ok(()) => {}
            Err(TrySendError::Full(msg)) => {
                let _ = self.tree_tx.send(msg);
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

impl CompositorHandler for WaylandApp {
    fn scale_factor_changed(
        &mut self,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        self.geometry.set_integer_scale_factor(new_factor);
        self.reconfigure_surface_geometry(conn);
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.present.frame_callback_received();
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl WindowHandler for WaylandApp {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &Window) {
        self.running_flag.store(false, Ordering::Relaxed);
        self.exit = true;
    }

    fn configure(
        &mut self,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
        _window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        let width = configure
            .new_size
            .0
            .map(|value| value.get())
            .unwrap_or(self.geometry.logical_size.0);
        let height = configure
            .new_size
            .1
            .map(|value| value.get())
            .unwrap_or(self.geometry.logical_size.1);

        self.present.configured = true;
        self.update_logical_size(conn, width, height);
    }
}

impl OutputHandler for WaylandApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

delegate_compositor!(WaylandApp);
delegate_output!(WaylandApp);
delegate_xdg_shell!(WaylandApp);
delegate_xdg_window!(WaylandApp);
delegate_registry!(WaylandApp);

impl ProvidesRegistryState for WaylandApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState];
}

impl Dispatch<WpViewporter, ()> for WaylandApp {
    fn event(
        _: &mut Self,
        _: &WpViewporter,
        _: <WpViewporter as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        unreachable!("wp_viewporter::Event is empty in version 1")
    }
}

impl Dispatch<WpViewport, ()> for WaylandApp {
    fn event(
        _: &mut Self,
        _: &WpViewport,
        _: <WpViewport as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        unreachable!("wp_viewport::Event is empty in version 1")
    }
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for WaylandApp {
    fn event(
        _: &mut Self,
        _: &WpFractionalScaleManagerV1,
        _: <WpFractionalScaleManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        unreachable!("wp_fractional_scale_manager_v1 has no events")
    }
}

impl Dispatch<WpFractionalScaleV1, FractionalScaleData> for WaylandApp {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: <WpFractionalScaleV1 as Proxy>::Event,
        data: &FractionalScaleData,
        conn: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if data.surface != *state.window.wl_surface() {
            return;
        }

        let FractionalScaleEvent::PreferredScale { scale } = event else {
            return;
        };

        state
            .geometry
            .set_preferred_fractional_scale(Some(scale as f32 / 120.0));
        state.reconfigure_surface_geometry(conn);
    }
}

fn fail_startup(
    proxy_tx: &Sender<WindowBackendStartupResult>,
    running_flag: &Arc<AtomicBool>,
    event_tx: &crossbeam_channel::Sender<EventMsg>,
    message: String,
) {
    let _ = proxy_tx.send(Err(message.clone()));
    eprintln!("{message}");
    running_flag.store(false, Ordering::Relaxed);
    let _ = event_tx.send(EventMsg::Stop);
}

pub fn run(
    config: WaylandConfig,
    running_flag: Arc<AtomicBool>,
    tree_tx: CrossbeamSender<TreeMsg>,
    event_tx: crossbeam_channel::Sender<EventMsg>,
    render_rx: Receiver<RenderMsg>,
    video_registry: Arc<VideoRegistry>,
    proxy_tx: Sender<WindowBackendStartupResult>,
) {
    let conn = match Connection::connect_to_env() {
        Ok(conn) => conn,
        Err(err) => {
            fail_startup(
                &proxy_tx,
                &running_flag,
                &event_tx,
                format!("failed to connect to wayland compositor: {err}"),
            );
            return;
        }
    };

    let (globals, event_queue) = match registry_queue_init(&conn) {
        Ok(values) => values,
        Err(err) => {
            fail_startup(
                &proxy_tx,
                &running_flag,
                &event_tx,
                format!("failed to initialize wayland registry: {err}"),
            );
            return;
        }
    };

    let qh = event_queue.handle();
    let mut event_loop: EventLoop<WaylandApp> = match EventLoop::try_new() {
        Ok(event_loop) => event_loop,
        Err(err) => {
            fail_startup(
                &proxy_tx,
                &running_flag,
                &event_tx,
                format!("failed to create wayland event loop: {err}"),
            );
            return;
        }
    };

    let loop_handle = event_loop.handle();
    if let Err(err) = WaylandSource::new(conn.clone(), event_queue).insert(loop_handle.clone()) {
        fail_startup(
            &proxy_tx,
            &running_flag,
            &event_tx,
            format!("failed to insert wayland source: {err}"),
        );
        return;
    }

    let (wake_tx, wake_rx) = calloop::channel::channel();
    if let Err(err) = loop_handle.insert_source(wake_rx, |event, _, state| match event {
        calloop::channel::Event::Msg(action) => state.handle_wake_action(action),
        calloop::channel::Event::Closed => {
            state.running_flag.store(false, Ordering::Relaxed);
            state.exit = true;
        }
    }) {
        fail_startup(
            &proxy_tx,
            &running_flag,
            &event_tx,
            format!("failed to insert wayland wake source: {err}"),
        );
        return;
    }

    let wake = BackendWakeHandle::new(WaylandWake {
        tx: wake_tx.clone(),
    });

    let mut app = match WaylandApp::new(
        &conn,
        &globals,
        qh,
        Arc::clone(&running_flag),
        tree_tx,
        event_tx.clone(),
        render_rx,
        video_registry,
        &config,
    ) {
        Ok(app) => app,
        Err(err) => {
            fail_startup(&proxy_tx, &running_flag, &event_tx, err);
            return;
        }
    };

    let _ = proxy_tx.send(Ok(WindowBackendStartupInfo {
        wake,
        prime_video_supported: false,
    }));

    while !app.exit {
        if let Err(err) = event_loop.dispatch(None::<Duration>, &mut app) {
            eprintln!("wayland event loop dispatch failed: {err}");
            app.running_flag.store(false, Ordering::Relaxed);
            app.exit = true;
            break;
        }

        app.flush_render_updates();
        app.maybe_draw();
    }
}
