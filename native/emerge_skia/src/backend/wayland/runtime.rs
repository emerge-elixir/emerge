use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender as CrossbeamSender, TrySendError};
use glutin::prelude::GlSurface;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_pointer, delegate_registry, delegate_seat,
    delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{self, EventLoop},
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_surface},
};

use crate::{
    actors::{EventMsg, RenderMsg, TreeMsg},
    backend::{
        wake::{
            BackendWake, BackendWakeHandle, WindowBackendStartupInfo, WindowBackendStartupResult,
        },
        wayland_config::WaylandConfig,
    },
    input::InputEvent,
    renderer::RenderState,
    video::VideoRegistry,
};

use super::{
    egl::{GlEnv, create_gl_env, resize_gl_env},
    geometry::SurfaceGeometry,
    input::{PointerInputState, pointer_button_event, pointer_scroll_event},
    present::PresentState,
    protocols::ProtocolHandles,
};

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

pub(super) struct WaylandApp {
    registry_state: RegistryState,
    output_state: OutputState,
    qh: QueueHandle<Self>,
    pub(super) window: Window,
    env: Option<GlEnv>,
    protocols: ProtocolHandles,
    pub(super) geometry: SurfaceGeometry,
    present: PresentState,
    input: PointerInputState,
    exit: bool,
    running_flag: Arc<AtomicBool>,
    tree_tx: CrossbeamSender<TreeMsg>,
    render_rx: Receiver<RenderMsg>,
    event_tx: crossbeam_channel::Sender<EventMsg>,
    video_registry: Arc<VideoRegistry>,
    render_state: RenderState,
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
            qh: qh.clone(),
            window,
            env: None,
            protocols,
            geometry: SurfaceGeometry::new(config),
            present: PresentState::default(),
            input: PointerInputState::new(globals, &qh),
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

    fn send_input_event(&self, event: InputEvent) {
        let _ = self.event_tx.send(EventMsg::InputEvent(event));
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

        let presented_at = std::time::Instant::now();
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

    pub(super) fn reconfigure_surface_geometry(&mut self, conn: &Connection) {
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
            self.send_input_event(InputEvent::Resized {
                width: self.geometry.buffer_size.0,
                height: self.geometry.buffer_size.1,
                scale_factor: self.geometry.scale_factor(),
            });
        }
    }

    fn send_animation_pulse(
        &self,
        presented_at: std::time::Instant,
        predicted_next_present_at: std::time::Instant,
    ) {
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

impl SeatHandler for WaylandApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.input.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.input.pointer.is_none() {
            match self.input.seat_state.get_pointer(qh, &seat) {
                Ok(pointer) => self.input.pointer = Some(pointer),
                Err(err) => eprintln!("failed to create wayland pointer: {err}"),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Some(pointer) = self.input.pointer.take()
        {
            pointer.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for WaylandApp {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        use PointerEventKind::*;

        for event in events {
            if &event.surface != self.window.wl_surface() {
                continue;
            }

            let (x, y) = self.geometry.surface_to_buffer_position(event.position);
            self.input.set_cursor_pos(x, y);

            match event.kind {
                Enter { .. } => {
                    self.send_input_event(InputEvent::CursorEntered { entered: true });
                    self.send_input_event(InputEvent::CursorPos { x, y });
                }
                Leave { .. } => {
                    self.send_input_event(InputEvent::CursorEntered { entered: false });
                }
                Motion { .. } => {
                    self.send_input_event(InputEvent::CursorPos { x, y });
                }
                Press { button, .. } => {
                    self.send_input_event(pointer_button_event(
                        button,
                        true,
                        self.input.current_mods,
                        (x, y),
                    ));
                }
                Release { button, .. } => {
                    self.send_input_event(pointer_button_event(
                        button,
                        false,
                        self.input.current_mods,
                        (x, y),
                    ));
                }
                Axis {
                    horizontal,
                    vertical,
                    ..
                } => {
                    if let Some(scroll_event) = pointer_scroll_event(horizontal, vertical, (x, y)) {
                        self.send_input_event(scroll_event);
                    }
                }
            }
        }
    }
}

delegate_compositor!(WaylandApp);
delegate_output!(WaylandApp);
delegate_pointer!(WaylandApp);
delegate_seat!(WaylandApp);
delegate_xdg_shell!(WaylandApp);
delegate_xdg_window!(WaylandApp);
delegate_registry!(WaylandApp);

impl ProvidesRegistryState for WaylandApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
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

pub(crate) fn run(
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
