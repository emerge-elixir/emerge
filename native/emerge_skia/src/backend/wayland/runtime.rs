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
    delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{self, EventLoop},
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{
            CursorIcon as SctkCursorIcon, PointerEvent, PointerEventKind, PointerHandler,
            PointerThemeError, ThemeSpec,
        },
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
};

use crate::{
    InputTargetRelay,
    actors::{
        AnimationFrameTrace, AnimationPulseTrace, EventMsg, RenderMsg, TreeMsg,
        earliest_pipeline_submitted_at,
    },
    backend::{
        wake::{
            BackendWake, BackendWakeHandle, WindowBackendStartupInfo, WindowBackendStartupResult,
        },
        wayland_config::WaylandConfig,
    },
    events::CursorIcon,
    input::InputEvent,
    native_log::NativeLogRelay,
    renderer::{RenderState, RendererCacheConfig},
    stats::{
        RendererStatsCollector, SLOW_PRESENT_SUBMIT_THRESHOLD, format_slow_present_frame_log,
        format_slow_render_frame_log, render_frame_has_slow_stage,
    },
    video::{VideoImportContext, VideoRegistry},
};

use super::{
    egl::{GlEnv, create_gl_env, resize_gl_env},
    geometry::SurfaceGeometry,
    input::{PointerInputState, pointer_button_event, pointer_scroll_event},
    keyboard::{KeyboardInputState, key_from_keysym, mods_from_sctk, normalize_commit_text},
    present::{DrawDecision, DrawKind, PresentState},
    protocols::ProtocolHandles,
    text_input::TextInputProtocolState,
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

struct WaylandAppRuntime {
    running_flag: Arc<AtomicBool>,
    tree_tx: CrossbeamSender<TreeMsg>,
    event_tx: crossbeam_channel::Sender<EventMsg>,
    input_target: Arc<InputTargetRelay>,
    close_signal_log: bool,
    stats: Option<Arc<RendererStatsCollector>>,
    renderer_stats_log: bool,
    renderer_cache_config: RendererCacheConfig,
    native_log: Arc<NativeLogRelay>,
    render_rx: Receiver<RenderMsg>,
    cursor_icon_rx: Receiver<CursorIcon>,
    video_registry: Arc<VideoRegistry>,
    loop_handle: calloop::LoopHandle<'static, WaylandApp>,
}

pub(crate) struct WaylandRunArgs {
    pub config: WaylandConfig,
    pub running_flag: Arc<AtomicBool>,
    pub tree_tx: CrossbeamSender<TreeMsg>,
    pub event_tx: crossbeam_channel::Sender<EventMsg>,
    pub input_target: Arc<InputTargetRelay>,
    pub close_signal_log: bool,
    pub stats: Option<Arc<RendererStatsCollector>>,
    pub renderer_stats_log: bool,
    pub renderer_cache_config: RendererCacheConfig,
    pub native_log: Arc<NativeLogRelay>,
    pub render_rx: Receiver<RenderMsg>,
    pub cursor_icon_rx: Receiver<CursorIcon>,
    pub video_registry: Arc<VideoRegistry>,
    pub proxy_tx: Sender<WindowBackendStartupResult>,
}

enum WaylandVideoImportState {
    PendingGlInit,
    Ready(Box<VideoImportContext>),
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaylandVideoSyncAction {
    Hold,
    Import,
    Drop,
}

impl WaylandVideoImportState {
    fn sync_action(&self) -> WaylandVideoSyncAction {
        match self {
            Self::PendingGlInit => WaylandVideoSyncAction::Hold,
            Self::Ready(_) => WaylandVideoSyncAction::Import,
            Self::Unavailable => WaylandVideoSyncAction::Drop,
        }
    }

    fn context(&self) -> Option<&VideoImportContext> {
        match self {
            Self::Ready(ctx) => Some(ctx.as_ref()),
            Self::PendingGlInit | Self::Unavailable => None,
        }
    }
}

fn should_reconfigure_surface(size_changed: bool, env_missing: bool) -> bool {
    size_changed || env_missing
}

fn frame_draw_decision(
    present: &PresentState,
    env_ready: bool,
    exit: bool,
    allow_late_replacement: bool,
) -> DrawDecision {
    if env_ready {
        present.draw_decision(exit, allow_late_replacement)
    } else {
        DrawDecision::Skip
    }
}

// The compositor thread must never block on actor queues. Under backpressure,
// dropping stale work is preferable to letting the window stop responding.
fn try_send_wayland_event(event_tx: &crossbeam_channel::Sender<EventMsg>, msg: EventMsg) {
    match event_tx.try_send(msg) {
        Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
    }
}

fn try_send_wayland_tree(tree_tx: &CrossbeamSender<TreeMsg>, msg: TreeMsg) {
    match tree_tx.try_send(msg) {
        Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
    }
}

fn log_close_signal(enabled: bool, message: impl Into<String>) {
    if enabled {
        let message = message.into();
        eprintln!("EmergeSkia native[wayland_close] {message}");
    }
}

fn key_text_commit_event(
    utf8: Option<&str>,
    mods: u8,
    protocol_text_active: bool,
    ime_preedit_active: bool,
    allow_protocol_text_active: bool,
) -> Option<InputEvent> {
    if ime_preedit_active || (protocol_text_active && !allow_protocol_text_active) {
        return None;
    }

    utf8.and_then(normalize_commit_text)
        .map(|text| InputEvent::TextCommit { text, mods })
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
    shm: Shm,
    env: Option<GlEnv>,
    protocols: ProtocolHandles,
    pub(super) geometry: SurfaceGeometry,
    present: PresentState,
    input: PointerInputState,
    current_cursor_icon: CursorIcon,
    pub(super) keyboard: KeyboardInputState,
    pub(super) text_input: TextInputProtocolState,
    video_import: WaylandVideoImportState,
    exit: bool,
    running_flag: Arc<AtomicBool>,
    tree_tx: CrossbeamSender<TreeMsg>,
    render_rx: Receiver<RenderMsg>,
    cursor_icon_rx: Receiver<CursorIcon>,
    event_tx: crossbeam_channel::Sender<EventMsg>,
    input_target: Arc<InputTargetRelay>,
    close_signal_log: bool,
    stats: Option<Arc<RendererStatsCollector>>,
    renderer_stats_log: bool,
    renderer_cache_config: RendererCacheConfig,
    native_log: Arc<NativeLogRelay>,
    video_registry: Arc<VideoRegistry>,
    loop_handle: calloop::LoopHandle<'static, WaylandApp>,
    render_state: RenderState,
    render_animation_trace: Option<AnimationFrameTrace>,
    animation_pulse_sequence: u64,
    pending_pipeline_submitted_at: Option<std::time::Instant>,
    pending_pipeline_swap_done_at: Option<std::time::Instant>,
}

impl WaylandApp {
    fn new(
        conn: &Connection,
        globals: &wayland_client::globals::GlobalList,
        qh: QueueHandle<Self>,
        runtime: WaylandAppRuntime,
        config: &WaylandConfig,
    ) -> Result<Self, String> {
        let compositor_state = CompositorState::bind(globals, &qh)
            .map_err(|err| format!("wl_compositor not available: {err}"))?;
        let xdg_shell = XdgShell::bind(globals, &qh)
            .map_err(|err| format!("xdg shell not available: {err}"))?;
        let shm = Shm::bind(globals, &qh).map_err(|err| format!("wl_shm not available: {err}"))?;

        let surface = compositor_state.create_surface(&qh);
        let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
        window.set_title(&config.title);
        window.set_app_id("dev.emerge.emerge_skia");

        let protocols = ProtocolHandles::new(globals, &qh, compositor_state, &window);

        window.commit();

        let WaylandAppRuntime {
            running_flag,
            tree_tx,
            event_tx,
            input_target,
            close_signal_log,
            stats,
            renderer_stats_log,
            renderer_cache_config,
            native_log,
            render_rx,
            cursor_icon_rx,
            video_registry,
            loop_handle,
        } = runtime;

        let mut app = Self {
            registry_state: RegistryState::new(globals),
            output_state: OutputState::new(globals, &qh),
            qh: qh.clone(),
            window,
            shm,
            env: None,
            protocols,
            geometry: SurfaceGeometry::new(config),
            present: PresentState::default(),
            input: PointerInputState::new(globals, &qh),
            current_cursor_icon: CursorIcon::Default,
            keyboard: KeyboardInputState::new(),
            text_input: TextInputProtocolState::new(globals, &qh),
            video_import: WaylandVideoImportState::PendingGlInit,
            exit: false,
            running_flag,
            tree_tx,
            render_rx,
            cursor_icon_rx,
            event_tx,
            input_target,
            close_signal_log,
            stats,
            renderer_stats_log,
            renderer_cache_config,
            native_log,
            video_registry,
            loop_handle,
            render_state: RenderState::default(),
            render_animation_trace: None,
            animation_pulse_sequence: 0,
            pending_pipeline_submitted_at: None,
            pending_pipeline_swap_done_at: None,
        };

        app.apply_surface_scale_state();

        if app.geometry.buffer_size != app.geometry.logical_size {
            app.reconfigure_surface_geometry(conn);
        }

        Ok(app)
    }

    fn handle_wake_action(&mut self, conn: &Connection, action: WakeAction) {
        match action {
            WakeAction::Stop => {
                self.running_flag.store(false, Ordering::Relaxed);
                self.exit = true;
            }
            WakeAction::Redraw => {
                self.flush_backend_updates(conn);
            }
            WakeAction::VideoFrameAvailable => {
                self.queue_redraw();
            }
        }
    }

    fn queue_redraw(&mut self) {
        self.present.queue_redraw();
    }

    pub(super) fn send_input_event(&self, event: InputEvent) {
        try_send_wayland_event(&self.event_tx, EventMsg::InputEvent(event));
    }

    fn emit_key_press(&self, event: &KeyEvent, allow_protocol_text_active_text_commit: bool) {
        self.send_input_event(InputEvent::Key {
            key: key_from_keysym(event.keysym),
            action: crate::input::ACTION_PRESS,
            mods: self.keyboard.current_mods,
        });

        if let Some(text_commit) = key_text_commit_event(
            event.utf8.as_deref(),
            self.keyboard.current_mods,
            self.text_input.protocol_text_active(),
            self.keyboard.ime_preedit_active,
            allow_protocol_text_active_text_commit,
        ) {
            self.send_input_event(text_commit);
        }
    }

    fn emit_key_repeat(&self, event: &KeyEvent) {
        self.emit_key_press(event, true);
    }

    fn unmap_for_close(&self, conn: &Connection) {
        log_close_signal(self.close_signal_log, "request_close before unmap");
        self.window.attach(None, 0, 0);
        self.window.wl_surface().commit();

        match conn.flush() {
            Ok(()) => log_close_signal(self.close_signal_log, "request_close after unmap flush"),
            Err(err) => log_close_signal(
                self.close_signal_log,
                format!("request_close unmap flush failed: {err}"),
            ),
        }
    }

    fn flush_backend_updates(&mut self, conn: &Connection) {
        if self.exit {
            return;
        }

        if self.drain_backend_messages(conn) {
            self.queue_redraw();
        }
    }

    fn drain_backend_messages(&mut self, conn: &Connection) -> bool {
        let mut updated = false;

        while let Ok(msg) = self.render_rx.try_recv() {
            match msg {
                RenderMsg::Scene {
                    scene,
                    version,
                    pipeline_submitted_at,
                    pipeline_render_queued_at,
                    animation_trace,
                    animate,
                    ime_enabled,
                    ime_cursor_area,
                    ime_text_state,
                    ..
                } => {
                    let animation_trace = animation_trace.map(|trace| *trace);
                    let scene = *scene;
                    self.render_state.has_cache_candidates = scene.has_cache_candidates();
                    self.render_state.scene = scene;
                    self.render_state.render_version = version;
                    self.render_state.pipeline_submitted_at = pipeline_submitted_at;
                    self.render_state.pipeline_render_queued_at = pipeline_render_queued_at;
                    self.render_state.animate = animate;
                    self.render_animation_trace = animation_trace;
                    self.present.note_scene_received(
                        version,
                        pipeline_submitted_at.is_some(),
                        animate,
                    );
                    if self.renderer_stats_log && (animate || animation_trace.is_some()) {
                        self.native_log.info(
                            "renderer_animation",
                            format_animation_scene_log(
                                "wayland",
                                version,
                                animate,
                                animation_trace,
                                std::time::Instant::now(),
                            ),
                        );
                    }
                    if let (Some(stats), Some(render_queued_at)) =
                        (self.stats.as_ref(), pipeline_render_queued_at)
                    {
                        stats.record_pipeline_render_queue(
                            render_queued_at,
                            std::time::Instant::now(),
                        );
                    }

                    if self.text_input.update_render_state(
                        ime_enabled,
                        ime_cursor_area,
                        *ime_text_state,
                    ) {
                        self.text_input.sync(&self.window, &self.geometry);
                    }

                    updated = true;
                }
                RenderMsg::Stop => {
                    self.running_flag.store(false, Ordering::Relaxed);
                    self.exit = true;
                    return false;
                }
            }
        }

        while let Ok(icon) = self.cursor_icon_rx.try_recv() {
            self.current_cursor_icon = icon;

            if self.input.entered {
                self.apply_current_cursor_icon(conn);
            }
        }

        updated
    }

    fn sctk_cursor_icon(icon: CursorIcon) -> SctkCursorIcon {
        match icon {
            CursorIcon::Default => SctkCursorIcon::Default,
            CursorIcon::Text => SctkCursorIcon::Text,
            CursorIcon::Pointer => SctkCursorIcon::Pointer,
        }
    }

    fn apply_cursor_icon(&self, conn: &Connection, icon: CursorIcon) {
        let Some(pointer) = self.input.pointer.as_ref() else {
            return;
        };

        match pointer.set_cursor(conn, Self::sctk_cursor_icon(icon)) {
            Ok(()) | Err(PointerThemeError::MissingEnterSerial) => {}
            Err(PointerThemeError::CursorNotFound) if icon != CursorIcon::Default => {
                if let Err(err) =
                    pointer.set_cursor(conn, Self::sctk_cursor_icon(CursorIcon::Default))
                    && !matches!(err, PointerThemeError::MissingEnterSerial)
                {
                    eprintln!("failed to apply wayland fallback cursor: {err}");
                }
            }
            Err(err) => eprintln!("failed to apply wayland cursor: {err}"),
        }
    }

    fn apply_current_cursor_icon(&self, conn: &Connection) {
        self.apply_cursor_icon(conn, self.current_cursor_icon);
    }

    fn update_logical_size(&mut self, conn: &Connection, width: u32, height: u32) {
        let size_changed = self.geometry.set_logical_size(width, height);

        if !should_reconfigure_surface(size_changed, self.env.is_none()) {
            return;
        }

        self.reconfigure_surface_geometry(conn);
    }

    fn maybe_draw(&mut self) {
        let allow_late_replacement = self
            .env
            .as_ref()
            .is_some_and(|env| env.swap_buffers_nonblocking);
        let decision = frame_draw_decision(
            &self.present,
            self.env.is_some(),
            self.exit,
            allow_late_replacement,
        );

        let DrawDecision::Draw(draw_kind) = decision else {
            self.present.clear_ready_frame_callback_timing_if_idle();
            return;
        };

        self.draw(draw_kind);
    }

    fn draw(&mut self, draw_kind: DrawKind) {
        let (video_import, video_registry) = (&self.video_import, &self.video_registry);
        let sync_action = video_import.sync_action();
        let video_import_ctx = video_import.context();
        let animation_trace = self.render_animation_trace;
        let draw_started_at = std::time::Instant::now();

        let Some(env) = self.env.as_mut() else {
            return;
        };

        self.present.prepare_draw(draw_kind, &self.window, &self.qh);

        let mut video_needs_cleanup = false;

        {
            let mut frame = env.frame_surface.frame();

            match sync_action {
                WaylandVideoSyncAction::Hold => {}
                WaylandVideoSyncAction::Import => {
                    match env.renderer.sync_video_frames(
                        &mut frame,
                        video_registry,
                        video_import_ctx,
                    ) {
                        Ok(result) => video_needs_cleanup = result.needs_cleanup,
                        Err(err) => eprintln!("video sync failed: {err}"),
                    }
                }
                WaylandVideoSyncAction::Drop => {
                    if let Err(err) = video_registry.drain_pending_to_release() {
                        eprintln!("video sync failed: {err}");
                    }
                }
            }

            let render_timings = if self.renderer_stats_log {
                env.renderer.render_profiled(&mut frame, &self.render_state)
            } else {
                env.renderer.render(&mut frame, &self.render_state)
            };

            if let Some(stats) = self.stats.as_ref() {
                stats.record_render(render_timings.total);
                stats.record_render_draw(render_timings.draw);
                stats.record_render_flush(render_timings.flush);
                stats.record_render_gpu_flush(render_timings.gpu_flush);
                stats.record_render_submit(render_timings.submit);
                if let Some(renderer_cache) = render_timings.renderer_cache.as_deref() {
                    stats.record_renderer_cache(*renderer_cache);
                }
            }

            if self.renderer_stats_log && render_frame_has_slow_stage(&render_timings) {
                self.native_log.info(
                    "renderer_slow_frame",
                    format_slow_render_frame_log(
                        "wayland",
                        &render_timings,
                        self.render_state.scene.summary(),
                    ),
                );
            }
        }

        let present_submit_started_at = std::time::Instant::now();

        if let Err(err) = env.gl_surface.swap_buffers(&env.gl_context) {
            eprintln!("wayland egl swap_buffers failed: {err}");
            self.running_flag.store(false, Ordering::Relaxed);
            self.exit = true;
            return;
        }

        let present_submit = present_submit_started_at.elapsed();
        let swap_done_at = std::time::Instant::now();
        if self.renderer_stats_log && (self.render_state.animate || animation_trace.is_some()) {
            self.native_log.info(
                "renderer_animation",
                format_animation_draw_log(AnimationDrawLogInput {
                    backend_label: "wayland",
                    version: self.render_state.render_version,
                    draw_kind,
                    animate: self.render_state.animate,
                    trace: animation_trace,
                    draw_started_at,
                    swap_done_at,
                    present_submit,
                }),
            );
        }
        if let (Some(stats), Some(submitted_at)) =
            (self.stats.as_ref(), self.render_state.pipeline_submitted_at)
        {
            stats.record_pipeline_submit_to_swap(submitted_at, swap_done_at);
        }
        if self.render_state.pipeline_submitted_at.is_some() {
            self.pending_pipeline_swap_done_at = Some(swap_done_at);
        }
        self.pending_pipeline_submitted_at = earliest_pipeline_submitted_at(
            self.pending_pipeline_submitted_at,
            self.render_state.pipeline_submitted_at.take(),
        );
        self.render_state.pipeline_render_queued_at = None;

        if let Some(stats) = self.stats.as_ref() {
            stats.record_present_submit(present_submit);
        }

        if self.renderer_stats_log && present_submit >= SLOW_PRESENT_SUBMIT_THRESHOLD {
            self.native_log.info(
                "renderer_slow_frame",
                format_slow_present_frame_log(
                    "wayland",
                    present_submit,
                    self.render_state.scene.summary(),
                ),
            );
        }

        if let Some(stats) = self.stats.as_ref() {
            stats.record_frame_present();
        }

        if draw_kind == DrawKind::Normal {
            let fallback_presented_at = std::time::Instant::now();
            let (presented_at, predicted_next_present_at) = self
                .present
                .present_timing_for_normal_draw(fallback_presented_at);

            if let Some(stats) = self.stats.as_ref() {
                stats.record_display_interval(
                    predicted_next_present_at.saturating_duration_since(presented_at),
                );
            }

            self.send_present_timing(presented_at, predicted_next_present_at);

            if self.render_state.animate {
                self.send_animation_pulse(presented_at, predicted_next_present_at);
            }
        }

        self.present.finish_present(
            self.render_state.render_version,
            draw_kind,
            video_needs_cleanup,
        );
        self.render_animation_trace = None;
    }

    fn apply_surface_scale_state(&mut self) {
        self.geometry
            .apply_to_surface(&self.window, self.protocols.viewport.as_ref());
    }

    fn initialize_video_import(&mut self) {
        if !matches!(self.video_import, WaylandVideoImportState::PendingGlInit) {
            return;
        }

        self.video_import = match VideoImportContext::new_current() {
            Ok(ctx) => WaylandVideoImportState::Ready(Box::new(ctx)),
            Err(err) => {
                eprintln!("prime video import unavailable: {err}");
                WaylandVideoImportState::Unavailable
            }
        };
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
            self.video_import = WaylandVideoImportState::PendingGlInit;

            match create_gl_env(
                conn,
                self.window.wl_surface(),
                self.geometry.buffer_size,
                self.renderer_cache_config,
            ) {
                Ok(env) => {
                    self.env = Some(env);
                    self.initialize_video_import();
                }
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
            self.text_input.sync(&self.window, &self.geometry);
        }
    }

    fn send_animation_pulse(
        &mut self,
        presented_at: std::time::Instant,
        predicted_next_present_at: std::time::Instant,
    ) {
        self.animation_pulse_sequence = self.animation_pulse_sequence.wrapping_add(1);
        let trace = AnimationPulseTrace {
            sequence: self.animation_pulse_sequence,
            sent_at: std::time::Instant::now(),
        };
        if self.renderer_stats_log {
            self.native_log.info(
                "renderer_animation",
                format_animation_pulse_log(
                    "wayland",
                    self.render_state.render_version,
                    trace,
                    presented_at,
                    predicted_next_present_at,
                ),
            );
        }
        try_send_wayland_tree(
            &self.tree_tx,
            TreeMsg::AnimationPulse {
                presented_at,
                predicted_next_present_at,
                trace: Some(trace),
            },
        );
    }

    fn send_present_timing(
        &self,
        presented_at: std::time::Instant,
        predicted_next_present_at: std::time::Instant,
    ) {
        try_send_wayland_event(
            &self.event_tx,
            EventMsg::PresentTiming {
                presented_at,
                predicted_next_present_at,
            },
        );
    }
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn signed_instant_delta_ms(later: std::time::Instant, earlier: std::time::Instant) -> f64 {
    if later >= earlier {
        duration_ms(later.duration_since(earlier))
    } else {
        -duration_ms(earlier.duration_since(later))
    }
}

fn optional_signed_instant_delta_ms(
    later: std::time::Instant,
    earlier: Option<std::time::Instant>,
) -> String {
    earlier.map_or_else(
        || "n/a".to_string(),
        |earlier| format!("{:.3} ms", signed_instant_delta_ms(later, earlier)),
    )
}

fn trace_source(trace: Option<AnimationFrameTrace>) -> String {
    match trace.and_then(|trace| trace.sequence) {
        Some(sequence) => format!("pulse seq={sequence}"),
        None => "tree update".to_string(),
    }
}

fn format_animation_scene_log(
    backend_label: &str,
    version: u64,
    animate: bool,
    trace: Option<AnimationFrameTrace>,
    received_at: std::time::Instant,
) -> String {
    let Some(trace) = trace else {
        return format!(
            "animation scene\n  backend: {backend_label}\n  version: {version}\n  animate: {animate}\n  trace: none"
        );
    };

    let sample_regressed = trace
        .previous_sample_time
        .is_some_and(|previous| trace.sample_time < previous);
    let pulse_to_tree =
        optional_signed_instant_delta_ms(trace.tree_started_at, trace.pulse_sent_at);
    let tree_to_queue = duration_ms(
        trace
            .render_queued_at
            .saturating_duration_since(trace.tree_started_at),
    );
    let pulse_to_queue =
        optional_signed_instant_delta_ms(trace.render_queued_at, trace.pulse_sent_at);
    let presented_to_sample =
        optional_signed_instant_delta_ms(trace.sample_time, trace.presented_at);
    let predicted_to_sample =
        optional_signed_instant_delta_ms(trace.sample_time, trace.predicted_next_present_at);
    let previous_to_sample =
        optional_signed_instant_delta_ms(trace.sample_time, trace.previous_sample_time);
    let receive_delay = duration_ms(received_at.saturating_duration_since(trace.render_queued_at));

    format!(
        concat!(
            "animation scene\n",
            "  backend: {backend_label}\n",
            "  version: {version}\n",
            "  animate: {animate}\n",
            "  source: {source}\n",
            "  active: animations_active={animations_active} pulse_requested_sample={pulse_requested_sample}\n",
            "  tree: pulse->tree={pulse_to_tree} tree->queue={tree_to_queue:.3} ms pulse->queue={pulse_to_queue} queue->backend={receive_delay:.3} ms\n",
            "  sample: presented->sample={presented_to_sample} predicted->sample={predicted_to_sample} previous->sample={previous_to_sample} regressed={sample_regressed}"
        ),
        backend_label = backend_label,
        version = version,
        animate = animate,
        source = trace_source(Some(trace)),
        animations_active = trace.animations_active,
        pulse_requested_sample = trace.pulse_requested_sample,
        pulse_to_tree = pulse_to_tree,
        tree_to_queue = tree_to_queue,
        pulse_to_queue = pulse_to_queue,
        receive_delay = receive_delay,
        presented_to_sample = presented_to_sample,
        predicted_to_sample = predicted_to_sample,
        previous_to_sample = previous_to_sample,
        sample_regressed = sample_regressed,
    )
}

struct AnimationDrawLogInput<'a> {
    backend_label: &'a str,
    version: u64,
    draw_kind: DrawKind,
    animate: bool,
    trace: Option<AnimationFrameTrace>,
    draw_started_at: std::time::Instant,
    swap_done_at: std::time::Instant,
    present_submit: Duration,
}

fn format_animation_draw_log(input: AnimationDrawLogInput<'_>) -> String {
    let AnimationDrawLogInput {
        backend_label,
        version,
        draw_kind,
        animate,
        trace,
        draw_started_at,
        swap_done_at,
        present_submit,
    } = input;

    let Some(trace) = trace else {
        return format!(
            concat!(
                "animation draw\n",
                "  backend: {backend_label}\n",
                "  version: {version}\n",
                "  kind: {draw_kind:?}\n",
                "  animate: {animate}\n",
                "  trace: none\n",
                "  present_submit: {present_submit:.3} ms"
            ),
            backend_label = backend_label,
            version = version,
            draw_kind = draw_kind,
            animate = animate,
            present_submit = duration_ms(present_submit),
        );
    };

    let queue_to_draw =
        duration_ms(draw_started_at.saturating_duration_since(trace.render_queued_at));
    let tree_to_draw =
        duration_ms(draw_started_at.saturating_duration_since(trace.tree_started_at));
    let pulse_to_draw = optional_signed_instant_delta_ms(draw_started_at, trace.pulse_sent_at);
    let draw_to_sample = signed_instant_delta_ms(trace.sample_time, draw_started_at);
    let draw_to_swap = duration_ms(swap_done_at.saturating_duration_since(draw_started_at));
    let swap_to_sample = signed_instant_delta_ms(trace.sample_time, swap_done_at);

    format!(
        concat!(
            "animation draw\n",
            "  backend: {backend_label}\n",
            "  version: {version}\n",
            "  kind: {draw_kind:?}\n",
            "  animate: {animate}\n",
            "  source: {source}\n",
            "  queue: queue->draw={queue_to_draw:.3} ms tree->draw={tree_to_draw:.3} ms pulse->draw={pulse_to_draw}\n",
            "  sample: draw->sample={draw_to_sample:.3} ms swap->sample={swap_to_sample:.3} ms\n",
            "  draw: draw->swap={draw_to_swap:.3} ms present_submit={present_submit:.3} ms"
        ),
        backend_label = backend_label,
        version = version,
        draw_kind = draw_kind,
        animate = animate,
        source = trace_source(Some(trace)),
        queue_to_draw = queue_to_draw,
        tree_to_draw = tree_to_draw,
        pulse_to_draw = pulse_to_draw,
        draw_to_sample = draw_to_sample,
        swap_to_sample = swap_to_sample,
        draw_to_swap = draw_to_swap,
        present_submit = duration_ms(present_submit),
    )
}

fn format_animation_pulse_log(
    backend_label: &str,
    version: u64,
    trace: AnimationPulseTrace,
    presented_at: std::time::Instant,
    predicted_next_present_at: std::time::Instant,
) -> String {
    let presented_to_predicted =
        duration_ms(predicted_next_present_at.saturating_duration_since(presented_at));
    let presented_to_send = signed_instant_delta_ms(trace.sent_at, presented_at);
    let send_to_predicted = signed_instant_delta_ms(predicted_next_present_at, trace.sent_at);

    format!(
        concat!(
            "animation pulse\n",
            "  backend: {backend_label}\n",
            "  seq: {sequence}\n",
            "  submitted_version: {version}\n",
            "  timing: presented->predicted={presented_to_predicted:.3} ms presented->send={presented_to_send:.3} ms send->predicted={send_to_predicted:.3} ms"
        ),
        backend_label = backend_label,
        sequence = trace.sequence,
        version = version,
        presented_to_predicted = presented_to_predicted,
        presented_to_send = presented_to_send,
        send_to_predicted = send_to_predicted,
    )
}

fn format_animation_frame_callback_log(
    backend_label: &str,
    version: u64,
    callback_time_ms: u32,
    previous_estimated_interval: Duration,
    estimated_interval: Duration,
) -> String {
    format!(
        concat!(
            "animation frame callback\n",
            "  backend: {backend_label}\n",
            "  submitted_version: {version}\n",
            "  callback_time_ms: {callback_time_ms}\n",
            "  interval: previous_estimate={previous:.3} ms estimate={current:.3} ms"
        ),
        backend_label = backend_label,
        version = version,
        callback_time_ms = callback_time_ms,
        previous = duration_ms(previous_estimated_interval),
        current = duration_ms(estimated_interval),
    )
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
        time: u32,
    ) {
        let received_at = std::time::Instant::now();
        if let Some(submitted_at) = self.pending_pipeline_submitted_at.take()
            && let Some(stats) = self.stats.as_ref()
        {
            stats.record_pipeline(submitted_at, received_at);
        }
        if let Some(swap_done_at) = self.pending_pipeline_swap_done_at.take()
            && let Some(stats) = self.stats.as_ref()
        {
            stats.record_pipeline_swap_to_frame_callback(swap_done_at, received_at);
        }
        let previous_estimated_interval = self.present.estimated_frame_interval();
        self.present.frame_callback_received(received_at, time);
        if self.renderer_stats_log && self.render_state.animate {
            self.native_log.info(
                "renderer_animation",
                format_animation_frame_callback_log(
                    "wayland",
                    self.render_state.render_version,
                    time,
                    previous_estimated_interval,
                    self.present.estimated_frame_interval(),
                ),
            );
        }
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
        log_close_signal(self.close_signal_log, "request_close begin");
        self.unmap_for_close(_conn);
        self.running_flag.store(false, Ordering::Relaxed);
        self.exit = true;
        self.input_target
            .send_close_requested(self.close_signal_log);
        log_close_signal(
            self.close_signal_log,
            "request_close after send_close_requested",
        );
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
            let cursor_surface = self.protocols.compositor_state.create_surface(qh);

            match self.input.seat_state.get_pointer_with_theme(
                qh,
                &seat,
                self.shm.wl_shm(),
                cursor_surface,
                ThemeSpec::System,
            ) {
                Ok(pointer) => self.input.pointer = Some(pointer),
                Err(err) => eprintln!("failed to create wayland pointer: {err}"),
            }
        } else if capability == Capability::Keyboard && self.keyboard.keyboard.is_none() {
            let loop_handle = self.loop_handle.clone();
            match self.input.seat_state.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                loop_handle,
                Box::new(|state, _keyboard, event| {
                    state.emit_key_repeat(&event);
                }),
            ) {
                Ok(keyboard) => {
                    self.keyboard.keyboard = Some(keyboard);
                    self.text_input.create_for_seat(qh, &seat);
                }
                Err(err) => eprintln!("failed to create wayland keyboard: {err}"),
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
        if capability == Capability::Pointer && self.input.pointer.take().is_some() {
            self.input.entered = false;
        } else if capability == Capability::Keyboard
            && let Some(keyboard) = self.keyboard.keyboard.take()
        {
            keyboard.release();

            if self.keyboard.focused {
                self.send_input_event(InputEvent::Focused { focused: false });
            }

            self.keyboard.focused = false;
            self.keyboard.current_mods = 0;
            self.keyboard.ime_preedit_active = false;
            self.input.current_mods = 0;
            self.text_input.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for WaylandApp {
    fn pointer_frame(
        &mut self,
        conn: &Connection,
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
                    self.input.entered = true;
                    self.apply_cursor_icon(conn, CursorIcon::Default);
                    self.send_input_event(InputEvent::CursorEntered { entered: true });
                    self.send_input_event(InputEvent::CursorPos { x, y });
                }
                Leave { .. } => {
                    self.input.entered = false;
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

impl KeyboardHandler for WaylandApp {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        if surface == self.window.wl_surface() && !self.keyboard.focused {
            self.keyboard.focused = true;
            self.send_input_event(InputEvent::Focused { focused: true });
        }
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
        if surface == self.window.wl_surface() {
            self.keyboard.focused = false;
            self.keyboard.current_mods = 0;
            self.keyboard.ime_preedit_active = false;
            self.input.current_mods = 0;
            self.send_input_event(InputEvent::Focused { focused: false });
        }
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.emit_key_press(&event, false);
    }

    fn repeat_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: KeyEvent,
    ) {
        // Repeats are routed through SCTK's repeat callback so we get consistent
        // behavior across compositors, including those that do not emit
        // wl_keyboard repeated key events directly.
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.send_input_event(InputEvent::Key {
            key: key_from_keysym(event.keysym),
            action: crate::input::ACTION_RELEASE,
            mods: self.keyboard.current_mods,
        });
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        modifiers: Modifiers,
        _raw_modifiers: RawModifiers,
        _layout: u32,
    ) {
        let mods = mods_from_sctk(modifiers);
        self.keyboard.current_mods = mods;
        self.input.current_mods = mods;
    }
}

delegate_compositor!(WaylandApp);
delegate_keyboard!(WaylandApp);
delegate_output!(WaylandApp);
delegate_pointer!(WaylandApp);
delegate_seat!(WaylandApp);
delegate_shm!(WaylandApp);
delegate_xdg_shell!(WaylandApp);
delegate_xdg_window!(WaylandApp);
delegate_registry!(WaylandApp);

impl ShmHandler for WaylandApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

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

pub(crate) fn run(args: WaylandRunArgs) {
    let WaylandRunArgs {
        config,
        running_flag,
        tree_tx,
        event_tx,
        input_target,
        close_signal_log,
        stats,
        renderer_stats_log,
        renderer_cache_config,
        native_log,
        render_rx,
        cursor_icon_rx,
        video_registry,
        proxy_tx,
    } = args;

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
    if let Err(err) = loop_handle.insert_source(wake_rx, {
        let conn = conn.clone();

        move |event, _, state| match event {
            calloop::channel::Event::Msg(action) => state.handle_wake_action(&conn, action),
            calloop::channel::Event::Closed => {
                state.running_flag.store(false, Ordering::Relaxed);
                state.exit = true;
            }
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
        WaylandAppRuntime {
            running_flag: Arc::clone(&running_flag),
            tree_tx,
            event_tx: event_tx.clone(),
            input_target,
            close_signal_log,
            stats,
            renderer_stats_log,
            renderer_cache_config,
            native_log,
            render_rx,
            cursor_icon_rx,
            video_registry,
            loop_handle: event_loop.handle(),
        },
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
        prime_video_supported: true,
    }));

    while !app.exit {
        if let Err(err) = event_loop.dispatch(None::<Duration>, &mut app) {
            eprintln!("wayland event loop dispatch failed: {err}");
            app.running_flag.store(false, Ordering::Relaxed);
            app.exit = true;
            break;
        }

        app.flush_backend_updates(&conn);
        app.maybe_draw();
    }

    let env = app.env.take();
    drop(env);
    drop(app);
    drop(event_loop);
    drop(conn);
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use crossbeam_channel::bounded;

    use super::{
        DrawDecision, DrawKind, PresentState, WaylandVideoImportState, WaylandVideoSyncAction,
        frame_draw_decision, key_text_commit_event, should_reconfigure_surface,
        try_send_wayland_event, try_send_wayland_tree,
    };
    use crate::actors::{EventMsg, TreeMsg};
    use crate::input::{InputEvent, MOD_SHIFT};

    #[test]
    fn wayland_video_import_states_map_to_expected_sync_actions() {
        assert_eq!(
            WaylandVideoImportState::PendingGlInit.sync_action(),
            WaylandVideoSyncAction::Hold
        );
        assert_eq!(
            WaylandVideoImportState::Unavailable.sync_action(),
            WaylandVideoSyncAction::Drop
        );
    }

    #[test]
    fn same_size_first_configure_still_requires_surface_reconfigure_when_env_missing() {
        assert!(should_reconfigure_surface(false, true));
        assert!(should_reconfigure_surface(true, false));
        assert!(should_reconfigure_surface(true, true));
        assert!(!should_reconfigure_surface(false, false));
    }

    #[test]
    fn draw_requires_gl_env_before_present_state_starts_frame() {
        let mut present = PresentState::default();
        present.configured = true;
        present.queue_redraw();

        assert_eq!(
            present.draw_decision(false, false),
            DrawDecision::Draw(DrawKind::Normal)
        );
        assert_eq!(
            frame_draw_decision(&present, false, false, true),
            DrawDecision::Skip
        );
        assert_eq!(
            frame_draw_decision(&present, true, false, false),
            DrawDecision::Draw(DrawKind::Normal)
        );
        assert_eq!(
            frame_draw_decision(&present, true, true, true),
            DrawDecision::Skip
        );
    }

    #[test]
    fn key_text_commit_event_suppresses_press_when_protocol_text_is_active() {
        let event = key_text_commit_event(Some("a"), 0, true, false, false);

        assert!(event.is_none());
    }

    #[test]
    fn key_text_commit_event_allows_repeat_when_protocol_text_is_active() {
        let event = key_text_commit_event(Some("a"), MOD_SHIFT, true, false, true);

        assert!(matches!(
            event,
            Some(InputEvent::TextCommit { text, mods }) if text == "a" && mods == MOD_SHIFT
        ));
    }

    #[test]
    fn key_text_commit_event_blocks_repeat_while_preedit_is_active() {
        let event = key_text_commit_event(Some("a"), 0, true, true, true);

        assert!(event.is_none());
    }

    #[test]
    fn key_text_commit_event_keeps_non_protocol_repeat_behavior() {
        let event = key_text_commit_event(Some("b"), 0, false, false, true);

        assert!(matches!(
            event,
            Some(InputEvent::TextCommit { text, mods }) if text == "b" && mods == 0
        ));
    }

    #[test]
    fn wayland_event_send_does_not_block_when_event_channel_is_full() {
        let (event_tx, event_rx) = bounded(1);
        event_tx.send(EventMsg::Stop).unwrap();

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let handle = thread::spawn(move || {
            try_send_wayland_event(
                &event_tx,
                EventMsg::InputEvent(InputEvent::Focused { focused: true }),
            );
            let _ = done_tx.send(());
        });

        let completed = done_rx.recv_timeout(Duration::from_millis(100)).is_ok();

        if completed {
            assert!(matches!(event_rx.try_recv(), Ok(EventMsg::Stop)));
        }

        drop(event_rx);
        let _ = handle.join();

        assert!(
            completed,
            "wayland event send should not block when event channel is full"
        );
    }

    #[test]
    fn wayland_animation_pulse_send_does_not_block_when_tree_channel_is_full() {
        let (tree_tx, tree_rx) = bounded(1);
        tree_tx.send(TreeMsg::Stop).unwrap();

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let handle = thread::spawn(move || {
            try_send_wayland_tree(
                &tree_tx,
                TreeMsg::AnimationPulse {
                    presented_at: std::time::Instant::now(),
                    predicted_next_present_at: std::time::Instant::now(),
                    trace: None,
                },
            );
            let _ = done_tx.send(());
        });

        let completed = done_rx.recv_timeout(Duration::from_millis(100)).is_ok();

        if completed {
            assert!(matches!(tree_rx.try_recv(), Ok(TreeMsg::Stop)));
        }

        drop(tree_rx);
        let _ = handle.join();

        assert!(
            completed,
            "wayland tree send should not block when tree channel is full"
        );
    }
}
