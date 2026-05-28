use std::{
    fs::OpenOptions,
    io::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::Sender,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossbeam_channel::{Receiver, Sender as CrossbeamSender, TrySendError};
use glutin::prelude::GlSurface;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            self, EventLoop,
            timer::{TimeoutAction, Timer},
        },
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
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

use crate::{
    InputTargetRelay, LatestFrameStore, RasterPresentKind, RendererBackendKind,
    actors::{AnimationFrameTrace, AnimationPulseTrace, EventMsg, RenderMsg, TreeMsg},
    backend::{
        raster::{RasterBackend, RasterConfig},
        wake::{
            BackendWake, BackendWakeHandle, WindowBackendStartupInfo, WindowBackendStartupResult,
        },
        wayland_config::WaylandConfig,
    },
    events::{CursorIcon, CursorIconState},
    input::{InputEvent, keyboard::normalize_commit_text},
    native_log::NativeLogRelay,
    renderer::{RenderState, RendererCacheConfig},
    stats::{
        RendererStatsCollector, SLOW_PRESENT_SUBMIT_THRESHOLD, earliest_pipeline_instant,
        format_slow_present_frame_log, format_slow_render_frame_log, render_frame_has_slow_stage,
    },
    video::{VideoImportContext, VideoRegistry},
};

use super::{
    egl::{GlEnv, create_gl_env, resize_gl_env},
    geometry::SurfaceGeometry,
    input::{PointerInputState, pointer_button_event, pointer_scroll_event},
    keyboard::{KeyboardInputState, key_from_keysym, mods_from_sctk},
    present::{DrawDecision, DrawKind, FrameCallbackState, PresentSnapshot, PresentState},
    protocols::ProtocolHandles,
    text_input::TextInputProtocolState,
};

#[derive(Clone, Debug)]
enum WakeAction {
    Stop,
    Redraw,
    VideoFrameAvailable,
}

const WAYLAND_DISPATCH_STOP_POLL_INTERVAL: Duration = Duration::from_millis(250);

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
    render_log: bool,
    stats: Option<Arc<RendererStatsCollector>>,
    renderer_stats_log: bool,
    renderer_animation_log: bool,
    renderer_backend: RendererBackendKind,
    raster_present: RasterPresentKind,
    renderer_cache_config: RendererCacheConfig,
    latest_frame: Arc<LatestFrameStore>,
    native_log: Arc<NativeLogRelay>,
    render_rx: Receiver<RenderMsg>,
    cursor_icon_rx: Receiver<CursorIcon>,
    video_registry: Arc<VideoRegistry>,
    loop_handle: calloop::LoopHandle<'static, WaylandApp>,
    watchdog: Arc<WaylandThreadWatchdogState>,
    direct_watchdog_log: Option<Arc<WaylandDirectWatchdogLog>>,
}

pub(crate) struct WaylandRunArgs {
    pub config: WaylandConfig,
    pub running_flag: Arc<AtomicBool>,
    pub tree_tx: CrossbeamSender<TreeMsg>,
    pub event_tx: crossbeam_channel::Sender<EventMsg>,
    pub input_target: Arc<InputTargetRelay>,
    pub close_signal_log: bool,
    pub render_log: bool,
    pub stats: Option<Arc<RendererStatsCollector>>,
    pub renderer_stats_log: bool,
    pub renderer_animation_log: bool,
    pub renderer_backend: RendererBackendKind,
    pub raster_present: RasterPresentKind,
    pub renderer_cache_config: RendererCacheConfig,
    pub latest_frame: Arc<LatestFrameStore>,
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

enum RasterWaylandPresentEnv {
    Cpu { pool: SlotPool },
    GpuUpload { gl_env: GlEnv },
}

struct RasterWaylandEnv {
    renderer: RasterBackend,
    present: RasterWaylandPresentEnv,
    size: (u32, u32),
}

impl RasterWaylandEnv {
    fn new(
        shm: &Shm,
        conn: &Connection,
        surface: &wl_surface::WlSurface,
        size: (u32, u32),
        raster_present: RasterPresentKind,
        renderer_cache_config: RendererCacheConfig,
    ) -> Result<Self, String> {
        let size = (size.0.max(1), size.1.max(1));
        let present = match raster_present {
            RasterPresentKind::Auto | RasterPresentKind::Cpu => RasterWaylandPresentEnv::Cpu {
                pool: SlotPool::new(raster_pool_size(size)?, shm)
                    .map_err(|err| format!("failed to create Wayland shm pool: {err}"))?,
            },
            RasterPresentKind::GpuUpload => RasterWaylandPresentEnv::GpuUpload {
                gl_env: create_gl_env(conn, surface, size, renderer_cache_config)?,
            },
        };
        let renderer = RasterBackend::with_cache_config(
            &RasterConfig {
                width: size.0,
                height: size.1,
            },
            renderer_cache_config,
        )?;

        Ok(Self {
            renderer,
            present,
            size,
        })
    }

    fn resize(&mut self, size: (u32, u32)) -> Result<(), String> {
        let size = (size.0.max(1), size.1.max(1));
        match &mut self.present {
            RasterWaylandPresentEnv::Cpu { pool } => pool
                .resize(raster_pool_size(size)?)
                .map_err(|err| format!("failed to resize Wayland shm pool: {err}"))?,
            RasterWaylandPresentEnv::GpuUpload { gl_env } => resize_gl_env(gl_env, size),
        }
        self.renderer.resize(size.0, size.1)?;
        self.size = size;
        Ok(())
    }
}

fn rgba_to_wayland_argb(rgba: &[u8], argb: &mut [u8]) {
    rgba.chunks_exact(4)
        .zip(argb.chunks_exact_mut(4))
        .for_each(|(source, dest)| {
            dest[0] = source[2];
            dest[1] = source[1];
            dest[2] = source[0];
            dest[3] = source[3];
        });
}

fn raster_pool_size(size: (u32, u32)) -> Result<usize, String> {
    usize::try_from(size.0)
        .ok()
        .and_then(|width| {
            usize::try_from(size.1)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4).and_then(|bytes| bytes.checked_mul(2)))
        .ok_or_else(|| "Wayland raster surface is too large".to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaylandVideoSyncAction {
    Hold,
    Import,
    Drop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PresentSkipReason {
    Exit,
    EnvMissing,
    NotConfigured,
    NoRedrawRequested,
    WaitingForFrameCallback,
    WaitingForLateReplacementCallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PresentSkipLogKey {
    reason: PresentSkipReason,
    requested_frame_callback_sequence: Option<u64>,
    age_bucket: Option<u64>,
    latest_received_render_version: Option<u64>,
    last_submitted_render_version: Option<u64>,
}

#[derive(Default)]
struct WaylandRenderDiagnostics {
    draw_sequence: u64,
    last_wake_at: Option<Instant>,
    last_render_scene_received_at: Option<Instant>,
    last_render_scene_version: Option<u64>,
    last_draw_started_at: Option<Instant>,
    last_draw_finished_at: Option<Instant>,
    last_swap_started_at: Option<Instant>,
    last_swap_done_at: Option<Instant>,
    last_frame_callback_received_at: Option<Instant>,
    last_present_skip_log_key: Option<PresentSkipLogKey>,
}

struct WaylandDirectWatchdogLog {
    path: String,
    file: Mutex<Option<std::fs::File>>,
}

impl WaylandDirectWatchdogLog {
    fn open_for_current_process() -> Self {
        let path = format!("/tmp/emerge-wayland-watchdog-{}.log", std::process::id());
        let file = match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => Some(file),
            Err(err) => {
                eprintln!(
                    "EmergeSkia native[wayland_watchdog] failed to open direct watchdog log {path}: {err}"
                );
                None
            }
        };

        Self {
            path,
            file: Mutex::new(file),
        }
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn write(&self, message: impl AsRef<str>) {
        let mut guard = self
            .file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(file) = guard.as_mut() else {
            return;
        };

        let _ = writeln!(
            file,
            "\n{} EmergeSkia native[wayland_watchdog]\n{}",
            current_wall_ms(),
            message.as_ref()
        );
        let _ = file.flush();
    }
}

#[derive(Default)]
struct WaylandThreadWatchdogState {
    dispatch_active: AtomicBool,
    dispatch_generation: AtomicU64,
    last_dispatch_enter_wall_ms: AtomicU64,
    last_dispatch_exit_wall_ms: AtomicU64,
    last_loop_tick_wall_ms: AtomicU64,
    last_wake_wall_ms: AtomicU64,
    last_render_scene_wall_ms: AtomicU64,
    last_draw_start_wall_ms: AtomicU64,
    last_swap_start_wall_ms: AtomicU64,
    last_swap_done_wall_ms: AtomicU64,
    last_frame_callback_wall_ms: AtomicU64,
}

impl WaylandThreadWatchdogState {
    fn mark_dispatch_enter(&self) {
        self.dispatch_active.store(true, Ordering::Relaxed);
        self.dispatch_generation.fetch_add(1, Ordering::Relaxed);
        self.last_dispatch_enter_wall_ms
            .store(current_wall_ms(), Ordering::Relaxed);
    }

    fn mark_dispatch_exit(&self) {
        self.dispatch_active.store(false, Ordering::Relaxed);
        self.last_dispatch_exit_wall_ms
            .store(current_wall_ms(), Ordering::Relaxed);
    }

    fn mark_loop_tick(&self) {
        self.last_loop_tick_wall_ms
            .store(current_wall_ms(), Ordering::Relaxed);
    }

    fn mark_wake(&self) {
        self.last_wake_wall_ms
            .store(current_wall_ms(), Ordering::Relaxed);
    }

    fn mark_render_scene(&self) {
        self.last_render_scene_wall_ms
            .store(current_wall_ms(), Ordering::Relaxed);
    }

    fn mark_draw_start(&self) {
        self.last_draw_start_wall_ms
            .store(current_wall_ms(), Ordering::Relaxed);
    }

    fn mark_swap_start(&self) {
        self.last_swap_start_wall_ms
            .store(current_wall_ms(), Ordering::Relaxed);
    }

    fn mark_swap_done(&self) {
        self.last_swap_done_wall_ms
            .store(current_wall_ms(), Ordering::Relaxed);
    }

    fn mark_frame_callback(&self) {
        self.last_frame_callback_wall_ms
            .store(current_wall_ms(), Ordering::Relaxed);
    }

    fn snapshot(&self) -> WaylandThreadWatchdogSnapshot {
        WaylandThreadWatchdogSnapshot {
            dispatch_active: self.dispatch_active.load(Ordering::Relaxed),
            dispatch_generation: self.dispatch_generation.load(Ordering::Relaxed),
            last_dispatch_enter_wall_ms: self.last_dispatch_enter_wall_ms.load(Ordering::Relaxed),
            last_dispatch_exit_wall_ms: self.last_dispatch_exit_wall_ms.load(Ordering::Relaxed),
            last_loop_tick_wall_ms: self.last_loop_tick_wall_ms.load(Ordering::Relaxed),
            last_wake_wall_ms: self.last_wake_wall_ms.load(Ordering::Relaxed),
            last_render_scene_wall_ms: self.last_render_scene_wall_ms.load(Ordering::Relaxed),
            last_draw_start_wall_ms: self.last_draw_start_wall_ms.load(Ordering::Relaxed),
            last_swap_start_wall_ms: self.last_swap_start_wall_ms.load(Ordering::Relaxed),
            last_swap_done_wall_ms: self.last_swap_done_wall_ms.load(Ordering::Relaxed),
            last_frame_callback_wall_ms: self.last_frame_callback_wall_ms.load(Ordering::Relaxed),
        }
    }
}

struct WaylandThreadWatchdogSnapshot {
    dispatch_active: bool,
    dispatch_generation: u64,
    last_dispatch_enter_wall_ms: u64,
    last_dispatch_exit_wall_ms: u64,
    last_loop_tick_wall_ms: u64,
    last_wake_wall_ms: u64,
    last_render_scene_wall_ms: u64,
    last_draw_start_wall_ms: u64,
    last_swap_start_wall_ms: u64,
    last_swap_done_wall_ms: u64,
    last_frame_callback_wall_ms: u64,
}

struct DrawStartLogInput<'a> {
    draw_sequence: u64,
    draw_kind: DrawKind,
    version: u64,
    animate: bool,
    sync_action: WaylandVideoSyncAction,
    summary: crate::render_scene::RenderSceneSummary,
    snapshot: &'a PresentSnapshot,
    geometry: &'a SurfaceGeometry,
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
    raster_env: Option<RasterWaylandEnv>,
    protocols: ProtocolHandles,
    pub(super) geometry: SurfaceGeometry,
    present: PresentState,
    input: PointerInputState,
    cursor_icon_state: CursorIconState,
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
    render_log: bool,
    stats: Option<Arc<RendererStatsCollector>>,
    renderer_stats_log: bool,
    renderer_animation_log: bool,
    renderer_backend: RendererBackendKind,
    raster_present: RasterPresentKind,
    renderer_cache_config: RendererCacheConfig,
    latest_frame: Arc<LatestFrameStore>,
    native_log: Arc<NativeLogRelay>,
    video_registry: Arc<VideoRegistry>,
    loop_handle: calloop::LoopHandle<'static, WaylandApp>,
    watchdog: Arc<WaylandThreadWatchdogState>,
    direct_watchdog_log: Option<Arc<WaylandDirectWatchdogLog>>,
    render_state: RenderState,
    render_animation_trace: Option<AnimationFrameTrace>,
    animation_pulse_sequence: u64,
    diagnostics: WaylandRenderDiagnostics,
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
            render_log,
            stats,
            renderer_stats_log,
            renderer_animation_log,
            renderer_backend,
            raster_present,
            renderer_cache_config,
            latest_frame,
            native_log,
            render_rx,
            cursor_icon_rx,
            video_registry,
            loop_handle,
            watchdog,
            direct_watchdog_log,
        } = runtime;

        let mut app = Self {
            registry_state: RegistryState::new(globals),
            output_state: OutputState::new(globals, &qh),
            qh: qh.clone(),
            window,
            shm,
            env: None,
            raster_env: None,
            protocols,
            geometry: SurfaceGeometry::new(config),
            present: PresentState::default(),
            input: PointerInputState::new(globals, &qh),
            cursor_icon_state: CursorIconState::default(),
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
            render_log,
            stats,
            renderer_stats_log,
            renderer_animation_log,
            renderer_backend,
            raster_present,
            renderer_cache_config,
            latest_frame,
            native_log,
            video_registry,
            loop_handle,
            watchdog,
            direct_watchdog_log,
            render_state: RenderState::default(),
            render_animation_trace: None,
            animation_pulse_sequence: 0,
            diagnostics: WaylandRenderDiagnostics::default(),
            pending_pipeline_submitted_at: None,
            pending_pipeline_swap_done_at: None,
        };

        app.apply_surface_scale_state();

        if app.geometry.buffer_size != app.geometry.logical_size {
            app.reconfigure_surface_geometry(conn);
        }

        Ok(app)
    }

    pub(super) fn log_render_diagnostic(&self, message: impl Into<String>) {
        if self.render_log {
            self.native_log.info("wayland_render", message);
        }
    }

    fn log_direct_watchdog(&self, message: impl AsRef<str>) {
        if let Some(log) = self.direct_watchdog_log.as_ref() {
            log.write(message);
        }
    }

    fn present_snapshot(&self) -> PresentSnapshot {
        self.present.snapshot(Instant::now(), SystemTime::now())
    }

    fn log_present_skip_if_needed(&mut self, env_ready: bool, allow_late_replacement: bool) {
        if !self.render_log {
            return;
        }

        let snapshot = self.present_snapshot();
        let Some(reason) =
            present_skip_reason(&snapshot, env_ready, self.exit, allow_late_replacement)
        else {
            return;
        };

        if reason == PresentSkipReason::NoRedrawRequested {
            return;
        }

        let key = PresentSkipLogKey {
            reason,
            requested_frame_callback_sequence: snapshot.requested_frame_callback_sequence,
            age_bucket: frame_callback_wait_log_bucket(&snapshot),
            latest_received_render_version: snapshot.latest_received_render_version,
            last_submitted_render_version: snapshot.last_submitted_render_version,
        };

        if self.diagnostics.last_present_skip_log_key == Some(key) {
            return;
        }

        self.diagnostics.last_present_skip_log_key = Some(key);
        self.log_render_diagnostic(format_present_skip_log(
            reason,
            env_ready,
            allow_late_replacement,
            &snapshot,
            &self.geometry,
            &self.diagnostics,
        ));
    }

    fn log_render_watchdog(&mut self) {
        if !self.render_log {
            return;
        }

        let snapshot = self.present_snapshot();
        self.log_render_diagnostic(format_wayland_watchdog_log(
            &snapshot,
            &self.geometry,
            self.env.is_some(),
            &self.diagnostics,
        ));
    }

    fn handle_wake_action(&mut self, conn: &Connection, action: WakeAction) {
        self.watchdog.mark_wake();
        self.diagnostics.last_wake_at = Some(Instant::now());
        if self.render_log {
            self.log_render_diagnostic(format!(
                "wake action\n  action: {action:?}\n  {}",
                format_present_snapshot(&self.present_snapshot())
            ));
        }

        match action {
            WakeAction::Stop => {
                self.log_direct_watchdog("wake stop action received; exiting wayland loop");
                self.unmap_for_shutdown(conn, "wake stop action");
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

    fn unmap_for_shutdown(&self, conn: &Connection, reason: &str) {
        self.log_direct_watchdog(format!("wayland shutdown unmap begin\n  reason: {reason}"));
        self.window.attach(None, 0, 0);
        self.window.wl_surface().commit();

        match conn.flush() {
            Ok(()) => self.log_direct_watchdog(format!(
                "wayland shutdown unmap flushed\n  reason: {reason}"
            )),
            Err(err) => self.log_direct_watchdog(format!(
                "wayland shutdown unmap flush failed\n  reason: {reason}\n  error: {err}"
            )),
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
        let mut scene_count = 0_u32;
        let mut latest_scene_version = None;
        let mut latest_scene_animate = false;
        let mut latest_scene_from_patch = false;
        let mut cursor_count = 0_u32;

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
                    let received_at = Instant::now();
                    let animation_trace = animation_trace.map(|trace| *trace);
                    let scene = *scene;
                    self.render_state.set_scene(scene);
                    self.render_state.render_version = version;
                    self.render_state.pipeline_submitted_at = pipeline_submitted_at;
                    self.render_state.pipeline_render_queued_at = pipeline_render_queued_at;
                    self.render_state.animate = animate;
                    self.render_animation_trace = animation_trace;
                    self.diagnostics.last_render_scene_received_at = Some(received_at);
                    self.diagnostics.last_render_scene_version = Some(version);
                    self.watchdog.mark_render_scene();
                    scene_count = scene_count.saturating_add(1);
                    latest_scene_version = Some(version);
                    latest_scene_animate = animate;
                    latest_scene_from_patch = pipeline_submitted_at.is_some();
                    self.present.note_scene_received(
                        version,
                        pipeline_submitted_at.is_some(),
                        animate,
                    );
                    if self.renderer_animation_log && (animate || animation_trace.is_some()) {
                        self.native_log.info(
                            "renderer_animation",
                            format_animation_scene_log(
                                "wayland",
                                version,
                                animate,
                                animation_trace,
                                received_at,
                            ),
                        );
                    }
                    if let Some(stats) = self.stats.as_ref() {
                        stats.record_pipeline_draw_started(
                            pipeline_render_queued_at,
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
                    self.log_render_diagnostic("render queue stop message received");
                    self.log_direct_watchdog(
                        "render queue stop message received; exiting wayland loop",
                    );
                    self.unmap_for_shutdown(conn, "render stop message");
                    self.running_flag.store(false, Ordering::Relaxed);
                    self.exit = true;
                    return false;
                }
            }
        }

        while let Ok(icon) = self.cursor_icon_rx.try_recv() {
            cursor_count = cursor_count.saturating_add(1);
            if let Some(cursor) = self.cursor_icon_state.request(icon, self.input.entered) {
                self.apply_cursor_icon(conn, cursor);
            }
        }

        if self.render_log && (scene_count > 0 || cursor_count > 0) {
            self.log_render_diagnostic(format!(
                concat!(
                    "backend updates drained\n",
                    "  scenes: count={} latest_version={:?} animate={} from_patch={}\n",
                    "  cursors: count={}\n",
                    "  updated: {}\n",
                    "  {}"
                ),
                scene_count,
                latest_scene_version,
                latest_scene_animate,
                latest_scene_from_patch,
                cursor_count,
                updated,
                format_present_snapshot(&self.present_snapshot())
            ));
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

    fn update_logical_size(&mut self, conn: &Connection, width: u32, height: u32) {
        let previous = self.geometry;
        let size_changed = self.geometry.set_logical_size(width, height);
        if self.render_log {
            self.log_render_diagnostic(format!(
                concat!(
                    "configure logical size\n",
                    "  requested: {}x{}\n",
                    "  previous: {}\n",
                    "  after_logical: {}\n",
                    "  size_changed: {}\n",
                    "  env_ready: {}"
                ),
                width,
                height,
                format_surface_geometry(&previous),
                format_surface_geometry(&self.geometry),
                size_changed,
                self.env.is_some(),
            ));
        }

        if !should_reconfigure_surface(size_changed, self.env.is_none()) {
            if self.render_log {
                self.log_render_diagnostic("configure did not require surface reconfigure");
            }
            return;
        }

        self.reconfigure_surface_geometry(conn);
    }

    fn maybe_draw(&mut self) {
        let allow_late_replacement = matches!(self.renderer_backend, RendererBackendKind::Gl)
            && self
                .env
                .as_ref()
                .is_some_and(|env| env.swap_buffers_nonblocking);
        let env_ready = match self.renderer_backend {
            RendererBackendKind::Gl => self.env.is_some(),
            RendererBackendKind::Raster => self.raster_env.is_some(),
            RendererBackendKind::Auto
            | RendererBackendKind::Metal
            | RendererBackendKind::Vulkan => false,
        };
        let decision =
            frame_draw_decision(&self.present, env_ready, self.exit, allow_late_replacement);

        let DrawDecision::Draw(draw_kind) = decision else {
            self.log_present_skip_if_needed(env_ready, allow_late_replacement);
            self.present.clear_ready_frame_callback_timing_if_idle();
            return;
        };

        self.diagnostics.last_present_skip_log_key = None;

        self.draw(draw_kind);
    }

    fn draw(&mut self, draw_kind: DrawKind) {
        if matches!(self.renderer_backend, RendererBackendKind::Raster) {
            self.draw_raster(draw_kind);
            return;
        }

        let (video_import, video_registry) = (&self.video_import, &self.video_registry);
        let sync_action = video_import.sync_action();
        let video_import_ctx = video_import.context();
        let animation_trace = self.render_animation_trace;
        let draw_started_at = Instant::now();
        let draw_sequence = self.diagnostics.draw_sequence;
        self.diagnostics.draw_sequence = self.diagnostics.draw_sequence.wrapping_add(1);
        self.diagnostics.last_draw_started_at = Some(draw_started_at);
        self.watchdog.mark_draw_start();
        let render_log = self.render_log;
        let native_log = Arc::clone(&self.native_log);

        if render_log {
            native_log.info(
                "wayland_render",
                format_draw_start_log(DrawStartLogInput {
                    draw_sequence,
                    draw_kind,
                    version: self.render_state.render_version,
                    animate: self.render_state.animate,
                    sync_action,
                    summary: self.render_state.scene.summary(),
                    snapshot: &self.present_snapshot(),
                    geometry: &self.geometry,
                }),
            );
        }

        let Some(env) = self.env.as_mut() else {
            return;
        };

        if matches!(sync_action, WaylandVideoSyncAction::Hold)
            && env
                .renderer
                .can_skip_unchanged_visible_frame(&self.render_state, self.geometry.buffer_size)
        {
            self.present
                .finish_noop_present(self.render_state.render_version);
            self.render_state.pipeline_submitted_at = None;
            self.render_state.pipeline_render_queued_at = None;
            self.render_animation_trace = None;
            self.diagnostics.last_draw_finished_at = Some(Instant::now());
            if render_log {
                native_log.info(
                    "wayland_render",
                    format!(
                        "draw noop\n  sequence: {draw_sequence}\n  version: {}\n  reason: unchanged visible frame while video import pending",
                        self.render_state.render_version,
                    ),
                );
            }
            return;
        }

        let frame_request = self.present.prepare_draw(draw_kind, &self.window, &self.qh);
        if render_log && let Some(request) = frame_request {
            native_log.info(
                "wayland_render",
                format!(
                    "frame callback requested\n  sequence: {}\n  draw_sequence: {draw_sequence}\n  render_version: {}",
                    request.sequence, self.render_state.render_version
                ),
            );
        }

        let mut video_needs_cleanup = false;

        let captured_frame = {
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
                stats.record_render_timings(render_timings.total, &render_timings);
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

            if render_log {
                native_log.info(
                    "wayland_render",
                    format!(
                        "render done\n  sequence: {draw_sequence}\n  version: {}\n  render_total: {:.3} ms",
                        self.render_state.render_version,
                        duration_ms(render_timings.total),
                    ),
                );
            }

            drop(frame);
            env.frame_surface.capture_rgba_pixels()
        };

        let present_submit_started_at = Instant::now();
        self.diagnostics.last_swap_started_at = Some(present_submit_started_at);
        self.watchdog.mark_swap_start();
        if render_log {
            native_log.info(
                "wayland_render",
                format!(
                    "swap start\n  sequence: {draw_sequence}\n  version: {}\n  kind: {draw_kind:?}",
                    self.render_state.render_version,
                ),
            );
        }

        if let Err(err) = env.gl_surface.swap_buffers(&env.gl_context) {
            eprintln!("wayland egl swap_buffers failed: {err}");
            if render_log {
                native_log.info(
                    "wayland_render",
                    format!(
                        "swap error\n  sequence: {draw_sequence}\n  version: {}\n  error: {err}\n  {}",
                        self.render_state.render_version,
                        format_present_snapshot(&self.present_snapshot()),
                    ),
                );
            }
            self.running_flag.store(false, Ordering::Relaxed);
            self.exit = true;
            return;
        }

        let present_submit = present_submit_started_at.elapsed();
        let swap_done_at = Instant::now();
        if let Some((width, height, pixels)) = captured_frame {
            self.latest_frame
                .publish_rgba(width, height, self.geometry.scale_factor(), pixels);
        }
        self.diagnostics.last_swap_done_at = Some(swap_done_at);
        self.diagnostics.last_draw_finished_at = Some(swap_done_at);
        self.watchdog.mark_swap_done();
        if render_log {
            native_log.info(
                "wayland_render",
                format!(
                    "swap done\n  sequence: {draw_sequence}\n  version: {}\n  present_submit: {:.3} ms",
                    self.render_state.render_version,
                    duration_ms(present_submit),
                ),
            );
        }
        if self.renderer_animation_log && (self.render_state.animate || animation_trace.is_some()) {
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
        self.pending_pipeline_submitted_at = earliest_pipeline_instant(
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
        if render_log {
            native_log.info(
                "wayland_render",
                format!(
                    "present finished\n  sequence: {draw_sequence}\n  version: {}\n  video_needs_cleanup: {video_needs_cleanup}\n  {}",
                    self.render_state.render_version,
                    format_present_snapshot(&self.present_snapshot()),
                ),
            );
        }
        self.render_animation_trace = None;
    }

    fn draw_raster(&mut self, draw_kind: DrawKind) {
        let animation_trace = self.render_animation_trace;
        let draw_started_at = Instant::now();
        let draw_sequence = self.diagnostics.draw_sequence;
        self.diagnostics.draw_sequence = self.diagnostics.draw_sequence.wrapping_add(1);
        self.diagnostics.last_draw_started_at = Some(draw_started_at);
        self.watchdog.mark_draw_start();

        if self.render_log {
            self.native_log.info(
                "wayland_render",
                format_draw_start_log(DrawStartLogInput {
                    draw_sequence,
                    draw_kind,
                    version: self.render_state.render_version,
                    animate: self.render_state.animate,
                    sync_action: WaylandVideoSyncAction::Drop,
                    summary: self.render_state.scene.summary(),
                    snapshot: &self.present_snapshot(),
                    geometry: &self.geometry,
                }),
            );
        }

        let Some(env) = self.raster_env.as_mut() else {
            return;
        };

        let frame_request = self.present.prepare_draw(draw_kind, &self.window, &self.qh);
        if self.render_log
            && let Some(request) = frame_request
        {
            self.native_log.info(
                "wayland_render",
                format!(
                    "frame callback requested\n  sequence: {}\n  draw_sequence: {draw_sequence}\n  render_version: {}",
                    request.sequence, self.render_state.render_version
                ),
            );
        }

        let render_started_at = Instant::now();
        let (frame, render_timings) = env.renderer.render_with_timings(&self.render_state);
        if let Some(stats) = self.stats.as_ref() {
            stats.record_render_timings(render_started_at.elapsed(), &render_timings);
        }

        let present_submit_started_at = Instant::now();
        self.diagnostics.last_swap_started_at = Some(present_submit_started_at);
        self.watchdog.mark_swap_start();

        let present_result = match &mut env.present {
            RasterWaylandPresentEnv::Cpu { pool } => {
                let stride = (self.geometry.buffer_size.0 * 4) as i32;
                pool.create_buffer(
                    self.geometry.buffer_size.0 as i32,
                    self.geometry.buffer_size.1 as i32,
                    stride,
                    wl_shm::Format::Argb8888,
                )
                .map_err(|err| format!("failed to create Wayland raster buffer: {err}"))
                .and_then(|(buffer, canvas)| {
                    rgba_to_wayland_argb(&frame.data, canvas);
                    self.window.wl_surface().damage_buffer(
                        0,
                        0,
                        self.geometry.buffer_size.0 as i32,
                        self.geometry.buffer_size.1 as i32,
                    );
                    buffer
                        .attach_to(self.window.wl_surface())
                        .map_err(|err| format!("failed to attach Wayland raster buffer: {err}"))?;
                    self.window.commit();
                    Ok(())
                })
            }
            RasterWaylandPresentEnv::GpuUpload { gl_env } => gl_env
                .frame_surface
                .present_rgba_pixels(
                    self.geometry.buffer_size.0,
                    self.geometry.buffer_size.1,
                    &frame.data,
                )
                .and_then(|()| {
                    gl_env
                        .gl_surface
                        .swap_buffers(&gl_env.gl_context)
                        .map_err(|err| format!("wayland raster gpu-upload swap failed: {err}"))
                }),
        };

        if let Err(err) = present_result {
            eprintln!("wayland raster present failed: {err}");
            self.running_flag.store(false, Ordering::Relaxed);
            self.exit = true;
            return;
        }

        let present_submit = present_submit_started_at.elapsed();
        let swap_done_at = Instant::now();
        self.latest_frame.publish_rgba(
            self.geometry.buffer_size.0,
            self.geometry.buffer_size.1,
            self.geometry.scale_factor(),
            frame.data,
        );
        self.diagnostics.last_swap_done_at = Some(swap_done_at);
        self.diagnostics.last_draw_finished_at = Some(swap_done_at);
        self.watchdog.mark_swap_done();

        if let (Some(stats), Some(submitted_at)) =
            (self.stats.as_ref(), self.render_state.pipeline_submitted_at)
        {
            stats.record_pipeline_submit_to_swap(submitted_at, swap_done_at);
        }
        if self.render_state.pipeline_submitted_at.is_some() {
            self.pending_pipeline_swap_done_at = Some(swap_done_at);
        }
        self.pending_pipeline_submitted_at = earliest_pipeline_instant(
            self.pending_pipeline_submitted_at,
            self.render_state.pipeline_submitted_at.take(),
        );
        self.render_state.pipeline_render_queued_at = None;

        if let Some(stats) = self.stats.as_ref() {
            stats.record_present_submit(present_submit);
            stats.record_frame_present();
        }

        if self.renderer_animation_log && (self.render_state.animate || animation_trace.is_some()) {
            self.native_log.info(
                "renderer_animation",
                format_animation_draw_log(AnimationDrawLogInput {
                    backend_label: "wayland-raster",
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

        self.present
            .finish_present(self.render_state.render_version, draw_kind, false);
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
        let env_was_ready = self.env.is_some();

        self.apply_surface_scale_state();

        if !self.present.configured && self.env.is_none() {
            if self.render_log {
                self.log_render_diagnostic(format!(
                    "surface reconfigure deferred\n  reason: waiting for first configure\n  geometry: {}",
                    format_surface_geometry(&self.geometry),
                ));
            }
            return;
        }

        if self.geometry.buffer_size.0 == 0 || self.geometry.buffer_size.1 == 0 {
            if self.render_log {
                self.log_render_diagnostic(format!(
                    "surface reconfigure skipped\n  reason: zero buffer size\n  geometry: {}",
                    format_surface_geometry(&self.geometry),
                ));
            }
            return;
        }

        let geometry_changed = previous != self.geometry;
        let buffer_changed = previous.buffer_size != self.geometry.buffer_size;

        if matches!(self.renderer_backend, RendererBackendKind::Raster) {
            if self.raster_env.is_none() {
                match RasterWaylandEnv::new(
                    &self.shm,
                    conn,
                    self.window.wl_surface(),
                    self.geometry.buffer_size,
                    self.raster_present,
                    self.renderer_cache_config,
                ) {
                    Ok(env) => {
                        self.raster_env = Some(env);
                        self.video_import = WaylandVideoImportState::Unavailable;
                        if self.render_log {
                            self.log_render_diagnostic(format!(
                                "raster env created\n  geometry: {}",
                                format_surface_geometry(&self.geometry),
                            ));
                        }
                    }
                    Err(err) => {
                        eprintln!("wayland raster setup failed: {err}");
                        if self.render_log {
                            self.log_render_diagnostic(format!(
                                "raster env create failed\n  geometry: {}\n  error: {err}",
                                format_surface_geometry(&self.geometry),
                            ));
                        }
                        self.running_flag.store(false, Ordering::Relaxed);
                        self.exit = true;
                        return;
                    }
                }
            } else if buffer_changed && let Some(env) = self.raster_env.as_mut() {
                if let Err(err) = env.resize(self.geometry.buffer_size) {
                    eprintln!("wayland raster resize failed: {err}");
                    self.running_flag.store(false, Ordering::Relaxed);
                    self.exit = true;
                    return;
                }
                if self.render_log {
                    self.log_render_diagnostic(format!(
                        "raster env resized\n  previous: {}\n  current: {}",
                        format_surface_geometry(&previous),
                        format_surface_geometry(&self.geometry),
                    ));
                }
            }
        } else if self.env.is_none() {
            self.video_import = WaylandVideoImportState::PendingGlInit;

            match create_renderer_env(
                self.renderer_backend,
                conn,
                self.window.wl_surface(),
                self.geometry.buffer_size,
                self.renderer_cache_config,
            ) {
                Ok(env) => {
                    self.env = Some(env);
                    self.initialize_video_import();
                    if self.render_log {
                        self.log_render_diagnostic(format!(
                            "egl env created\n  geometry: {}\n  swap_nonblocking: {}",
                            format_surface_geometry(&self.geometry),
                            self.env
                                .as_ref()
                                .is_some_and(|env| env.swap_buffers_nonblocking),
                        ));
                    }
                }
                Err(err) => {
                    eprintln!("wayland egl setup failed: {err}");
                    if self.render_log {
                        self.log_render_diagnostic(format!(
                            "egl env create failed\n  geometry: {}\n  error: {err}",
                            format_surface_geometry(&self.geometry),
                        ));
                    }
                    self.running_flag.store(false, Ordering::Relaxed);
                    self.exit = true;
                    return;
                }
            }
        } else if buffer_changed && let Some(env) = self.env.as_mut() {
            resize_gl_env(env, self.geometry.buffer_size);
            env.renderer.invalidate_visible_frame_fingerprint();
            if self.render_log {
                self.log_render_diagnostic(format!(
                    "egl env resized\n  previous: {}\n  current: {}",
                    format_surface_geometry(&previous),
                    format_surface_geometry(&self.geometry),
                ));
            }
        }

        if geometry_changed {
            let resize_width = self.geometry.buffer_size.0;
            let resize_height = self.geometry.buffer_size.1;
            let resize_scale = self.geometry.scale_factor();

            // Keep layout/render dimensions in lockstep with the Wayland buffer.
            // The input event below still notifies Elixir observers, but it can be
            // buffered behind the registry lane during startup and scale changes.
            let _ = self.tree_tx.send(TreeMsg::Resize {
                width: resize_width as f32,
                height: resize_height as f32,
                scale: resize_scale,
            });
            self.queue_redraw();
            self.send_input_event(InputEvent::Resized {
                width: resize_width,
                height: resize_height,
                scale_factor: resize_scale,
            });
            self.text_input.sync(&self.window, &self.geometry);
        }

        if self.render_log {
            self.log_render_diagnostic(format!(
                concat!(
                    "surface reconfigured\n",
                    "  env_was_ready: {}\n",
                    "  env_ready: {}\n",
                    "  geometry_changed: {}\n",
                    "  buffer_changed: {}\n",
                    "  previous: {}\n",
                    "  current: {}\n",
                    "  {}"
                ),
                env_was_ready,
                self.env.is_some(),
                geometry_changed,
                buffer_changed,
                format_surface_geometry(&previous),
                format_surface_geometry(&self.geometry),
                format_present_snapshot(&self.present_snapshot()),
            ));
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
        if self.renderer_animation_log {
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

fn current_wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn format_watchdog_wall_age(now_ms: u64, timestamp_ms: u64) -> String {
    if timestamp_ms == 0 {
        "n/a".to_string()
    } else {
        format!("{:.3} ms", now_ms.saturating_sub(timestamp_ms) as f64)
    }
}

fn format_thread_watchdog_snapshot(snapshot: &WaylandThreadWatchdogSnapshot) -> String {
    let now_ms = current_wall_ms();
    format!(
        concat!(
            "wayland thread watchdog\n",
            "  dispatch_active: {}\n",
            "  dispatch_generation: {}\n",
            "  ages: dispatch_enter={} dispatch_exit={} loop_tick={} wake={} render_scene={} draw_start={} swap_start={} swap_done={} frame_callback={}"
        ),
        snapshot.dispatch_active,
        snapshot.dispatch_generation,
        format_watchdog_wall_age(now_ms, snapshot.last_dispatch_enter_wall_ms),
        format_watchdog_wall_age(now_ms, snapshot.last_dispatch_exit_wall_ms),
        format_watchdog_wall_age(now_ms, snapshot.last_loop_tick_wall_ms),
        format_watchdog_wall_age(now_ms, snapshot.last_wake_wall_ms),
        format_watchdog_wall_age(now_ms, snapshot.last_render_scene_wall_ms),
        format_watchdog_wall_age(now_ms, snapshot.last_draw_start_wall_ms),
        format_watchdog_wall_age(now_ms, snapshot.last_swap_start_wall_ms),
        format_watchdog_wall_age(now_ms, snapshot.last_swap_done_wall_ms),
        format_watchdog_wall_age(now_ms, snapshot.last_frame_callback_wall_ms),
    )
}

fn spawn_wayland_thread_watchdog(
    state: Arc<WaylandThreadWatchdogState>,
    native_log: Arc<NativeLogRelay>,
    direct_log: Arc<WaylandDirectWatchdogLog>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_secs(2));
            if stop.load(Ordering::Relaxed) {
                break;
            }

            let snapshot = format_thread_watchdog_snapshot(&state.snapshot());
            direct_log.write(&snapshot);
            native_log.info("wayland_watchdog", snapshot.clone());
        }

        direct_log.write(format!(
            "wayland watchdog thread exiting\n  stop_flag: {}",
            stop.load(Ordering::Relaxed)
        ));
    })
}

fn format_surface_geometry(geometry: &SurfaceGeometry) -> String {
    format!(
        "logical={}x{} buffer={}x{} scale={:.3}",
        geometry.logical_size.0,
        geometry.logical_size.1,
        geometry.buffer_size.0,
        geometry.buffer_size.1,
        geometry.scale_factor(),
    )
}

fn format_optional_duration_ms(duration: Option<Duration>) -> String {
    duration.map_or_else(
        || "n/a".to_string(),
        |duration| format!("{:.3} ms", duration_ms(duration)),
    )
}

fn format_optional_elapsed_ms(now: Instant, instant: Option<Instant>) -> String {
    format_optional_duration_ms(instant.map(|instant| now.saturating_duration_since(instant)))
}

fn frame_callback_wait_age(snapshot: &PresentSnapshot) -> Option<Duration> {
    snapshot
        .requested_frame_callback_wall_age
        .or(snapshot.requested_frame_callback_age)
}

fn frame_callback_wait_log_bucket(snapshot: &PresentSnapshot) -> Option<u64> {
    let age = frame_callback_wait_age(snapshot)?;

    if age < Duration::from_millis(250) {
        Some(0)
    } else if age < Duration::from_secs(1) {
        Some(1)
    } else if age < Duration::from_secs(5) {
        Some(2)
    } else {
        Some(3 + age.as_secs() / 5)
    }
}

fn present_skip_reason(
    snapshot: &PresentSnapshot,
    env_ready: bool,
    exit: bool,
    allow_late_replacement: bool,
) -> Option<PresentSkipReason> {
    if exit {
        return Some(PresentSkipReason::Exit);
    }
    if !env_ready {
        return Some(PresentSkipReason::EnvMissing);
    }
    if !snapshot.configured {
        return Some(PresentSkipReason::NotConfigured);
    }
    if !snapshot.redraw_requested {
        return Some(PresentSkipReason::NoRedrawRequested);
    }
    if snapshot.frame_callback_state == FrameCallbackState::Requested {
        return Some(if allow_late_replacement {
            PresentSkipReason::WaitingForLateReplacementCallback
        } else {
            PresentSkipReason::WaitingForFrameCallback
        });
    }

    None
}

fn format_present_snapshot(snapshot: &PresentSnapshot) -> String {
    format!(
        concat!(
            "present: configured={} redraw={} frame_callback={:?} request_seq={:?} request_age={} request_wall_age={} ",
            "latest_received={:?} from_patch={} animation_active={} last_submitted={:?} newer_scene={} can_late_replace={} late_used={} ready_timing={} estimated_interval={:.3} ms"
        ),
        snapshot.configured,
        snapshot.redraw_requested,
        snapshot.frame_callback_state,
        snapshot.requested_frame_callback_sequence,
        format_optional_duration_ms(snapshot.requested_frame_callback_age),
        format_optional_duration_ms(snapshot.requested_frame_callback_wall_age),
        snapshot.latest_received_render_version,
        snapshot.latest_received_from_patch,
        snapshot.latest_received_animation_active,
        snapshot.last_submitted_render_version,
        snapshot.has_newer_received_scene,
        snapshot.can_late_replace,
        snapshot.late_replacement_used,
        snapshot.ready_frame_callback_buffered,
        duration_ms(snapshot.estimated_frame_interval),
    )
}

fn format_diagnostics_snapshot(diagnostics: &WaylandRenderDiagnostics) -> String {
    let now = Instant::now();
    format!(
        concat!(
            "diagnostics: last_wake={} last_scene={} last_scene_version={:?} ",
            "last_draw_start={} last_draw_finish={} last_swap_start={} last_swap_done={} last_frame_callback={}"
        ),
        format_optional_elapsed_ms(now, diagnostics.last_wake_at),
        format_optional_elapsed_ms(now, diagnostics.last_render_scene_received_at),
        diagnostics.last_render_scene_version,
        format_optional_elapsed_ms(now, diagnostics.last_draw_started_at),
        format_optional_elapsed_ms(now, diagnostics.last_draw_finished_at),
        format_optional_elapsed_ms(now, diagnostics.last_swap_started_at),
        format_optional_elapsed_ms(now, diagnostics.last_swap_done_at),
        format_optional_elapsed_ms(now, diagnostics.last_frame_callback_received_at),
    )
}

fn format_present_skip_log(
    reason: PresentSkipReason,
    env_ready: bool,
    allow_late_replacement: bool,
    snapshot: &PresentSnapshot,
    geometry: &SurfaceGeometry,
    diagnostics: &WaylandRenderDiagnostics,
) -> String {
    format!(
        concat!(
            "present skip\n",
            "  reason: {:?}\n",
            "  env_ready: {}\n",
            "  allow_late_replacement: {}\n",
            "  geometry: {}\n",
            "  {}\n",
            "  {}"
        ),
        reason,
        env_ready,
        allow_late_replacement,
        format_surface_geometry(geometry),
        format_present_snapshot(snapshot),
        format_diagnostics_snapshot(diagnostics),
    )
}

fn format_wayland_watchdog_log(
    snapshot: &PresentSnapshot,
    geometry: &SurfaceGeometry,
    env_ready: bool,
    diagnostics: &WaylandRenderDiagnostics,
) -> String {
    format!(
        concat!(
            "render watchdog\n",
            "  env_ready: {}\n",
            "  geometry: {}\n",
            "  {}\n",
            "  {}"
        ),
        env_ready,
        format_surface_geometry(geometry),
        format_present_snapshot(snapshot),
        format_diagnostics_snapshot(diagnostics),
    )
}

fn format_draw_start_log(input: DrawStartLogInput<'_>) -> String {
    let DrawStartLogInput {
        draw_sequence,
        draw_kind,
        version,
        animate,
        sync_action,
        summary,
        snapshot,
        geometry,
    } = input;

    format!(
        concat!(
            "draw start\n",
            "  sequence: {}\n",
            "  version: {}\n",
            "  kind: {:?}\n",
            "  animate: {}\n",
            "  video_sync: {:?}\n",
            "  geometry: {}\n",
            "  scene: nodes={} primitives={} clips={} clip_shapes={} texts={} images={} videos={} paint_layers={} cacheable_layers={} moving_layers={}\n",
            "  {}"
        ),
        draw_sequence,
        version,
        draw_kind,
        animate,
        sync_action,
        format_surface_geometry(geometry),
        summary.nodes,
        summary.primitives,
        summary.clips,
        summary.clip_shapes,
        summary.texts,
        summary.images,
        summary.videos,
        summary.paint_layers,
        summary.cacheable_layers,
        summary.moving_layers,
        format_present_snapshot(snapshot),
    )
}

fn format_frame_callback_log(
    version: u64,
    callback_time_ms: u32,
    previous_estimated_interval: Duration,
    estimated_interval: Duration,
    snapshot_before: &PresentSnapshot,
    snapshot_after: &PresentSnapshot,
) -> String {
    format!(
        concat!(
            "frame callback received\n",
            "  submitted_version: {}\n",
            "  callback_time_ms: {}\n",
            "  interval: previous_estimate={:.3} ms estimate={:.3} ms\n",
            "  before: {}\n",
            "  after: {}"
        ),
        version,
        callback_time_ms,
        duration_ms(previous_estimated_interval),
        duration_ms(estimated_interval),
        format_present_snapshot(snapshot_before),
        format_present_snapshot(snapshot_after),
    )
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
        if self.render_log {
            self.log_render_diagnostic(format!(
                "integer scale factor changed\n  new_factor: {new_factor}\n  before: {}",
                format_surface_geometry(&self.geometry),
            ));
        }
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
        let received_at = Instant::now();
        let snapshot_before = self.present_snapshot();
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
        self.diagnostics.last_frame_callback_received_at = Some(received_at);
        self.diagnostics.last_present_skip_log_key = None;
        self.watchdog.mark_frame_callback();
        if self.render_log {
            self.log_render_diagnostic(format_frame_callback_log(
                self.render_state.render_version,
                time,
                previous_estimated_interval,
                self.present.estimated_frame_interval(),
                &snapshot_before,
                &self.present_snapshot(),
            ));
        }
        if self.renderer_animation_log && self.render_state.animate {
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
        self.log_render_diagnostic(format!(
            "surface enter\n  geometry: {}\n  {}",
            format_surface_geometry(&self.geometry),
            format_present_snapshot(&self.present_snapshot()),
        ));
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
        self.log_render_diagnostic(format!(
            "surface leave\n  geometry: {}\n  {}",
            format_surface_geometry(&self.geometry),
            format_present_snapshot(&self.present_snapshot()),
        ));
    }
}

impl WindowHandler for WaylandApp {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &Window) {
        log_close_signal(self.close_signal_log, "request_close begin");
        self.log_direct_watchdog("wayland window close requested; exiting wayland loop");
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

        if self.render_log {
            self.log_render_diagnostic(format!(
                concat!(
                    "xdg configure\n",
                    "  size: {}x{}\n",
                    "  previous_configured: {}\n",
                    "  geometry: {}\n",
                    "  env_ready: {}\n",
                    "  {}"
                ),
                width,
                height,
                self.present.configured,
                format_surface_geometry(&self.geometry),
                self.env.is_some(),
                format_present_snapshot(&self.present_snapshot()),
            ));
        }
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
                    self.cursor_icon_state.pointer_entered();
                    self.apply_cursor_icon(conn, CursorIcon::Default);
                    self.send_input_event(InputEvent::CursorEntered { entered: true });
                    self.send_input_event(InputEvent::CursorPos { x, y });
                }
                Leave { .. } => {
                    self.input.entered = false;
                    self.cursor_icon_state.pointer_left();
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
                    let absolute_delta = self
                        .geometry
                        .surface_to_buffer_delta((horizontal.absolute, vertical.absolute));
                    if let Some(scroll_event) =
                        pointer_scroll_event(horizontal, vertical, (x, y), absolute_delta)
                    {
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

fn create_renderer_env(
    renderer_backend: RendererBackendKind,
    conn: &Connection,
    surface: &wl_surface::WlSurface,
    dimensions: (u32, u32),
    renderer_cache_config: RendererCacheConfig,
) -> Result<GlEnv, String> {
    match renderer_backend {
        RendererBackendKind::Gl => create_gl_env(conn, surface, dimensions, renderer_cache_config),
        RendererBackendKind::Auto => unreachable!("auto is resolved before Wayland startup"),
        RendererBackendKind::Raster => {
            Err("Wayland raster renderer is not implemented yet".to_string())
        }
        RendererBackendKind::Metal => Err("Wayland does not support Metal renderer".to_string()),
        RendererBackendKind::Vulkan => {
            Err("Wayland Vulkan renderer is not implemented yet".to_string())
        }
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

pub(crate) fn run(args: WaylandRunArgs) {
    let WaylandRunArgs {
        config,
        running_flag,
        tree_tx,
        event_tx,
        input_target,
        close_signal_log,
        render_log,
        stats,
        renderer_stats_log,
        renderer_animation_log,
        renderer_backend,
        raster_present,
        renderer_cache_config,
        latest_frame,
        native_log,
        render_rx,
        cursor_icon_rx,
        video_registry,
        proxy_tx,
    } = args;

    let direct_watchdog_log =
        render_log.then(|| Arc::new(WaylandDirectWatchdogLog::open_for_current_process()));

    if render_log {
        if let Some(log) = direct_watchdog_log.as_ref() {
            log.write("direct watchdog log started");
            native_log.info(
                "wayland_watchdog",
                format!("direct watchdog log\n  path: {}", log.path()),
            );
        }
        native_log.info(
            "wayland_render",
            format!(
                "startup begin\n  title: {:?}\n  requested_size: {}x{}",
                config.title, config.width, config.height
            ),
        );
    }

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

    if render_log {
        native_log.info("wayland_render", "wayland source inserted");
    }

    let (wake_tx, wake_rx) = calloop::channel::channel();
    if let Err(err) = loop_handle.insert_source(wake_rx, {
        let conn = conn.clone();

        move |event, _, state| match event {
            calloop::channel::Event::Msg(action) => state.handle_wake_action(&conn, action),
            calloop::channel::Event::Closed => {
                state.log_direct_watchdog("wayland wake source closed; exiting wayland loop");
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

    if render_log {
        native_log.info("wayland_render", "wake source inserted");
        if let Err(err) = loop_handle.insert_source(Timer::from_duration(Duration::from_secs(1)), {
            move |_, _, state| {
                state.log_render_watchdog();
                TimeoutAction::ToDuration(Duration::from_secs(1))
            }
        }) {
            fail_startup(
                &proxy_tx,
                &running_flag,
                &event_tx,
                format!("failed to insert wayland render diagnostics timer: {err}"),
            );
            return;
        }
        native_log.info("wayland_render", "render diagnostics watchdog inserted");
    }

    let wake = BackendWakeHandle::new(WaylandWake {
        tx: wake_tx.clone(),
    });
    let watchdog = Arc::new(WaylandThreadWatchdogState::default());

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
            render_log,
            stats,
            renderer_stats_log,
            renderer_animation_log,
            renderer_backend,
            raster_present,
            renderer_cache_config,
            latest_frame,
            native_log,
            render_rx,
            cursor_icon_rx,
            video_registry,
            loop_handle: event_loop.handle(),
            watchdog: Arc::clone(&watchdog),
            direct_watchdog_log: direct_watchdog_log.clone(),
        },
        &config,
    ) {
        Ok(app) => app,
        Err(err) => {
            fail_startup(&proxy_tx, &running_flag, &event_tx, err);
            return;
        }
    };
    let watchdog_stop = Arc::new(AtomicBool::new(false));
    let watchdog_handle = render_log.then(|| {
        spawn_wayland_thread_watchdog(
            Arc::clone(&watchdog),
            Arc::clone(&app.native_log),
            direct_watchdog_log
                .clone()
                .expect("render_log creates a direct watchdog log"),
            Arc::clone(&watchdog_stop),
        )
    });

    let _ = proxy_tx.send(Ok(WindowBackendStartupInfo {
        wake,
        prime_video_supported: matches!(renderer_backend, RendererBackendKind::Gl),
    }));
    app.log_render_diagnostic(format!(
        "startup complete\n  geometry: {}\n  env_ready: {}\n  {}",
        format_surface_geometry(&app.geometry),
        app.env.is_some(),
        format_present_snapshot(&app.present_snapshot()),
    ));
    app.log_direct_watchdog("wayland event loop starting");

    app.watchdog.mark_loop_tick();
    let mut loop_exit_reason = "exit flag set".to_string();
    while !app.exit {
        let dispatch_enter_wall_ms = current_wall_ms();
        app.watchdog.mark_dispatch_enter();
        let dispatch_result =
            event_loop.dispatch(Some(WAYLAND_DISPATCH_STOP_POLL_INTERVAL), &mut app);
        let dispatch_exit_wall_ms = current_wall_ms();
        app.watchdog.mark_dispatch_exit();

        let dispatch_duration_ms = dispatch_exit_wall_ms.saturating_sub(dispatch_enter_wall_ms);
        if dispatch_duration_ms >= 5_000 {
            app.log_direct_watchdog(format!(
                "wayland dispatch returned after slow wall-clock wait\n  duration: {dispatch_duration_ms} ms\n  exit: {}",
                app.exit
            ));
        }

        if let Err(err) = dispatch_result {
            loop_exit_reason = format!("dispatch error: {err}");
            app.log_direct_watchdog(format!(
                "wayland event loop dispatch failed\n  error: {err}"
            ));
            app.unmap_for_shutdown(&conn, "dispatch error");
            eprintln!("wayland event loop dispatch failed: {err}");
            app.running_flag.store(false, Ordering::Relaxed);
            app.exit = true;
            break;
        }

        if !app.running_flag.load(Ordering::Relaxed) && !app.exit {
            loop_exit_reason = "running flag cleared".to_string();
            app.log_direct_watchdog("wayland running flag cleared; exiting event loop");
            app.unmap_for_shutdown(&conn, "running flag cleared");
            app.exit = true;
        }

        if app.exit {
            break;
        }

        app.flush_backend_updates(&conn);
        app.maybe_draw();
        app.watchdog.mark_loop_tick();
    }

    app.log_direct_watchdog(format!(
        "wayland event loop exiting\n  reason: {loop_exit_reason}\n  exit: {}",
        app.exit
    ));

    watchdog_stop.store(true, Ordering::Relaxed);
    if let Some(handle) = watchdog_handle {
        app.log_direct_watchdog("joining wayland watchdog thread");
        let _ = handle.join();
        app.log_direct_watchdog("wayland watchdog thread joined");
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
        let mut present = PresentState::configured_for_test();
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
