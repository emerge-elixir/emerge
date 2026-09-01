use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossbeam_channel::{Receiver, Sender, at, bounded, never, select, unbounded};
use rustler::{Encoder, Env, LocalPid, NifResult, OwnedBinary, OwnedEnv, ResourceArc, Term};
use skia_safe::Color;
#[cfg(any(feature = "video-interop-support", test))]
use video_interop::{AcquireSync, Descriptor, Layer, Modifier, Object, Plane};

use self::output::{
    FramePixelFormat, FrameSource, convert_packed_gray_into, packed_gray_output_len,
};

use crate::{
    BackendKind, HeadlessConfig, InputTargetRelay, LatestFrameStore, RenderSender, RendererHandles,
    RendererResource, RendererRuntimeInfo, RenderingApi, StartConfig, VideoWake,
    actors::{RenderMsg, TreeMsg},
    assets,
    backend::{
        raster::{RasterBackend, RasterConfig, RasterPixelFormat},
        wake::BackendWakeHandle,
    },
    events::{SpawnEventActorConfig, spawn_event_actor},
    native_log::NativeLogRelay,
    renderer::{RenderState, RenderTimings, RendererCacheConfig},
    renderer_cache_status,
    runtime::tree_actor::TreeActorConfig,
    send_tree, spawn_running_heartbeat,
    stats::RendererStatsCollector,
    video::{self, VideoRegistry},
};

#[cfg(all(target_os = "linux", feature = "headless-opengl"))]
mod offscreen_gl;
mod output;
#[cfg(all(target_os = "linux", feature = "headless-vulkan"))]
mod vulkan;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeadlessMode {
    Binary,
    Prime,
}

impl HeadlessMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "binary" => Ok(Self::Binary),
            "prime" => Ok(Self::Prime),
            other => Err(format!("unsupported headless mode: {other}")),
        }
    }
}

#[cfg_attr(not(any(feature = "video-interop-support", test)), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
pub(crate) enum HeadlessReleaseMsg {
    PrimeFrame(u64),
}

struct HeadlessStartupInfo {
    selected_rendering_api: RenderingApi,
    #[cfg(feature = "vulkan")]
    vulkan_device: Option<crate::backend::vulkan::VulkanRendererReport>,
}

#[cfg(any(feature = "video-interop-support", test))]
pub struct HeadlessPrimeBackendToken {
    release_tx: Sender<HeadlessReleaseMsg>,
    release_id: u64,
    released: AtomicBool,
}

#[cfg(any(feature = "video-interop-support", test))]
impl HeadlessPrimeBackendToken {
    fn new(release_tx: Sender<HeadlessReleaseMsg>, release_id: u64) -> Self {
        Self {
            release_tx,
            release_id,
            released: AtomicBool::new(false),
        }
    }

    pub(crate) fn release(&self) {
        if !self.released.swap(true, Ordering::AcqRel) {
            let _ = self
                .release_tx
                .send(HeadlessReleaseMsg::PrimeFrame(self.release_id));
        }
    }
}

#[cfg(any(feature = "video-interop-support", test))]
impl Drop for HeadlessPrimeBackendToken {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(any(feature = "video-interop-support", test))]
#[rustler::resource_impl]
impl rustler::Resource for HeadlessPrimeBackendToken {}

#[cfg(any(feature = "video-interop-support", test))]
pub(crate) fn release_backend_token(backend_token: ResourceArc<HeadlessPrimeBackendToken>) {
    backend_token.release();
}

pub(crate) fn start_renderer_with_config(
    config: StartConfig,
    initial_log_target: Option<LocalPid>,
) -> NifResult<ResourceArc<RendererResource>> {
    let mode = HeadlessMode::parse(&config.headless.mode)
        .map_err(|reason| rustler::Error::Term(Box::new(reason)))?;
    if config.headless.dither
        && (!matches!(mode, HeadlessMode::Binary)
            || !matches!(config.headless.pixel_format.as_str(), "bw1" | "gray2")
            || !matches!(config.rendering_api.kind, RenderingApi::Raster))
    {
        return Err(rustler::Error::Term(Box::new(
            "headless dithering currently requires binary BW1 or Gray2 output with rendering_api :raster"
                .to_string(),
        )));
    }
    if config.headless.prime.on_backpressure != "drop_new" {
        return Err(rustler::Error::Term(Box::new(format!(
            "unsupported headless PRIME backpressure policy: {}",
            config.headless.prime.on_backpressure
        ))));
    }
    let Some(target) = config.headless.target else {
        return Err(rustler::Error::Term(Box::new(
            "headless target pid is required".to_string(),
        )));
    };

    let running_flag = Arc::new(AtomicBool::new(true));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let render_counter = Arc::new(AtomicU64::new(0));
    let input_target = Arc::new(InputTargetRelay::new(None));
    let native_log = Arc::new(NativeLogRelay::new(initial_log_target));
    let latest_frame = Arc::new(LatestFrameStore::default());
    let renderer_stats = (config.stats_enabled || config.renderer_stats_log)
        .then(|| Arc::new(RendererStatsCollector::new()));

    let (tree_tx, tree_rx) = bounded(512);
    let (event_tx, event_rx) = bounded(4096);
    let (render_tx, render_rx) = bounded(1);
    let (release_tx, release_rx) = unbounded();
    let render_sender = RenderSender {
        tx: render_tx,
        drop_rx: render_rx.clone(),
        log_render: config.render_log,
    };

    let backend_wake = BackendWakeHandle::noop();
    let video_release_tx = video::spawn_release_worker()
        .map_err(|err| rustler::Error::Term(Box::new(err.to_string())))?;
    let cleanup_dispatcher =
        crate::CleanupDispatcher::start().map_err(|err| rustler::Error::Term(Box::new(err)))?;
    let video_registry = Arc::new(VideoRegistry::new(
        video_release_tx,
        cleanup_dispatcher.clone(),
        renderer_stats.clone(),
    ));
    let headless = config.headless.clone();
    let latest_frame_for_thread = Arc::clone(&latest_frame);
    let stats_for_thread = renderer_stats.clone();
    let tree_tx_for_thread = tree_tx.clone();
    let running_for_thread = Arc::clone(&running_flag);
    let width = config.width;
    let height = config.height;
    let mut renderer_cache_config = config.renderer_cache_config;
    let mut renderer_cache_enabled_configured = config.renderer_cache_enabled_configured;
    if matches!(mode, HeadlessMode::Binary)
        && matches!(config.headless.pixel_format.as_str(), "bw1" | "gray2")
    {
        renderer_cache_config.enabled = false;
        renderer_cache_enabled_configured = true;
    }
    let rendering_api = config.rendering_api.kind;
    let (startup_tx, startup_rx) = bounded(1);

    let render_handle = thread::spawn(move || {
        run_render_loop(
            render_rx,
            release_rx,
            tree_tx_for_thread,
            running_for_thread,
            latest_frame_for_thread,
            stats_for_thread,
            headless,
            target,
            width,
            height,
            renderer_cache_config,
            renderer_cache_enabled_configured,
            rendering_api,
            mode,
            release_tx,
            startup_tx,
        );
    });

    let startup = match startup_rx.recv() {
        Ok(Ok(startup)) => startup,
        Ok(Err(reason)) => {
            running_flag.store(false, Ordering::Relaxed);
            let _ = render_handle.join();
            return Err(rustler::Error::Term(Box::new(reason)));
        }
        Err(_) => {
            running_flag.store(false, Ordering::Relaxed);
            let _ = render_handle.join();
            return Err(rustler::Error::Term(Box::new(
                "headless render thread exited before startup completed".to_string(),
            )));
        }
    };

    let selected_rendering_api = startup.selected_rendering_api;
    let selected_renderer_cache_config = effective_renderer_cache_config(
        selected_rendering_api,
        renderer_cache_config,
        renderer_cache_enabled_configured,
    );
    let renderer_info = RendererRuntimeInfo {
        backend: BackendKind::Headless,
        requested_rendering_api: config.requested_rendering_api,
        selected_rendering_api,
        raster_present: config.rendering_api.raster_present,
        renderer_cache: renderer_cache_status(
            selected_rendering_api,
            selected_renderer_cache_config,
        ),
        screenshot_supported: matches!(mode, HeadlessMode::Binary),
        prime_video_supported: false,
        prime_video_formats: Vec::new(),
        #[cfg(feature = "vulkan")]
        vulkan_device: startup.vulkan_device,
    };
    let heartbeat_stats = if config.renderer_stats_log {
        renderer_stats.clone()
    } else {
        None
    };
    let heartbeat_handle = spawn_running_heartbeat(
        Arc::clone(&running_flag),
        Arc::clone(&input_target),
        Arc::clone(&native_log),
        heartbeat_stats,
        "headless",
        renderer_info.renderer_label(),
    );

    assets::start(tree_tx.clone(), config.render_log);

    let tree_handle = crate::runtime::tree_actor::spawn_tree_actor(
        tree_rx,
        TreeActorConfig {
            render_sender: render_sender.clone(),
            event_tx: event_tx.clone(),
            render_counter: Arc::clone(&render_counter),
            stats: renderer_stats.clone(),
            log_input: false,
            window_wake: backend_wake.clone(),
            initial_width: width,
            initial_height: height,
        },
    );

    let (backend_cursor_tx, _backend_cursor_rx) = unbounded();
    let event_handle = spawn_event_actor(SpawnEventActorConfig {
        event_rx,
        tree_tx: tree_tx.clone(),
        backend_cursor_tx: Some(backend_cursor_tx),
        backend_wake: backend_wake.clone(),
        scroll_line_pixels: config.scroll_line_pixels,
        log_render: config.render_log,
        native_log: Arc::clone(&native_log),
        system_clipboard: false,
        stats: renderer_stats.clone(),
    });

    let resource = RendererResource {
        running_flag,
        backend_wake,
        stop_flag,
        tree_tx,
        event_tx,
        input_target,
        render_tx: render_sender,
        video_registry,
        video_wake: VideoWake::noop(),
        native_log,
        stats: renderer_stats,
        latest_frame,
        info: renderer_info,
        close_signal_log: config.close_signal_log,
        log_render: config.render_log,
        log_input: false,
        cleanup_dispatcher,
        handles: Mutex::new(Some(RendererHandles {
            backend_handle: Some(render_handle),
            input_handle: None,
            tree_handle: Some(tree_handle),
            event_handle: Some(event_handle),
            heartbeat_handle: Some(heartbeat_handle),
        })),
    };

    Ok(ResourceArc::new(resource))
}

#[allow(clippy::too_many_arguments)]
fn run_render_loop(
    render_rx: Receiver<RenderMsg>,
    release_rx: Receiver<HeadlessReleaseMsg>,
    tree_tx: Sender<TreeMsg>,
    running_flag: Arc<AtomicBool>,
    latest_frame: Arc<LatestFrameStore>,
    stats: Option<Arc<RendererStatsCollector>>,
    headless: HeadlessConfig,
    target: LocalPid,
    width: u32,
    height: u32,
    renderer_cache_config: RendererCacheConfig,
    renderer_cache_enabled_configured: bool,
    rendering_api: RenderingApi,
    mode: HeadlessMode,
    release_tx: Sender<HeadlessReleaseMsg>,
    startup_tx: Sender<Result<HeadlessStartupInfo, String>>,
) {
    let mut renderer = match HeadlessRenderer::new(
        mode,
        rendering_api,
        width,
        height,
        renderer_cache_config,
        renderer_cache_enabled_configured,
        headless.prime.max_in_flight,
        headless.prime.drm_node.as_deref(),
        &headless.pixel_format,
    ) {
        Ok(renderer) => renderer,
        Err(err) => {
            let _ = startup_tx.send(Err(err));
            running_flag.store(false, Ordering::Relaxed);
            return;
        }
    };
    let startup = HeadlessStartupInfo {
        selected_rendering_api: renderer.selected_rendering_api(),
        #[cfg(feature = "vulkan")]
        vulkan_device: renderer.vulkan_renderer_report(),
    };
    if startup_tx.send(Ok(startup)).is_err() {
        running_flag.store(false, Ordering::Relaxed);
        return;
    }

    let mut sequence = 0_u64;
    let mut pending_prime_animation = false;
    let frame_interval = headless
        .target_fps
        .map(|fps| Duration::from_secs_f64(1.0 / f64::from(fps.max(1))))
        .unwrap_or_else(|| Duration::from_millis(16));
    let mut animation_tick = never();
    let prime_completion_rx = renderer.prime_completion_receiver();
    let mut next_animation_at = None;

    while running_flag.load(Ordering::Relaxed) {
        select! {
            recv(animation_tick) -> _ => {
                animation_tick = never();
                maybe_send_animation_pulse(&tree_tx, true, frame_interval);
            }
            recv(prime_completion_rx) -> completion => {
                let Ok(release_id) = completion else { break; };
                if renderer.complete_prime_retirement(release_id) {
                    maybe_resume_prime_animation(
                        &mut pending_prime_animation,
                        &tree_tx,
                        frame_interval,
                    );
                }
                if renderer.terminal_prime_shutdown_ready() {
                    break;
                }
            }
            recv(release_rx) -> msg => {
                match msg {
                    Ok(HeadlessReleaseMsg::PrimeFrame(release_id)) => {
                        let capacity_released = renderer.release_prime(release_id);
                        if capacity_released {
                            maybe_resume_prime_animation(
                                &mut pending_prime_animation,
                                &tree_tx,
                                frame_interval,
                            );
                        }
                        if renderer.terminal_prime_shutdown_ready() {
                            break;
                        }
                    },
                    Err(_) => break,
                }
            }
            recv(render_rx) -> msg => {
                let Ok(msg) = msg else { break; };
                match msg {
                    RenderMsg::Scene {
                        scene,
                        version: _,
                        pipeline_submitted_at,
                        pipeline_render_queued_at: _,
                        animation_trace: _,
                        animate,
                        ime_enabled: _,
                        ime_cursor_area: _,
                        ime_text_state: _,
                    } => {
                        let render_started_at = Instant::now();
                        let clear_color = if matches!(headless.pixel_format.as_str(), "bw1" | "gray2") {
                            Color::WHITE
                        } else {
                            Color::TRANSPARENT
                        };
                        let state = RenderState::new(*scene, clear_color, sequence, animate);
                        match mode {
                            HeadlessMode::Binary => match renderer.render_binary(&state) {
                                Ok(frame) => {
                                    record_render_stats(
                                        stats.as_deref(),
                                        render_started_at,
                                        &frame.timings,
                                        pipeline_submitted_at,
                                        frame_interval,
                                    );
                                    sequence = sequence.wrapping_add(1);
                                    let packed_bits = match headless.pixel_format.as_str() {
                                        "bw1" => Some(1),
                                        "gray2" => Some(2),
                                        _ => None,
                                    };
                                    let converted = if let Some(bits) = packed_bits {
                                        let policy = if headless.dither {
                                            renderer.render_grayscale_dither_policy(&state).map(Some)
                                        } else {
                                            Ok(None)
                                        };
                                        policy.and_then(|policy| {
                                            let mut data = vec![
                                                0_u8;
                                                packed_gray_output_len(
                                                    frame.width,
                                                    frame.height,
                                                    bits,
                                                )?
                                            ];
                                            let stride = convert_packed_gray_into(
                                                FrameSource {
                                                    data: &frame.data,
                                                    width: frame.width,
                                                    height: frame.height,
                                                    row_bytes: frame.row_bytes,
                                                    format: frame.pixel_format,
                                                },
                                                bits,
                                                &headless.bw1_polarity,
                                                policy.as_deref(),
                                                &mut data,
                                            )?;
                                            Ok((data, stride))
                                        })
                                    } else if frame.pixel_format == FramePixelFormat::Rgba8888Premul {
                                        convert_frame(
                                            &frame.data,
                                            frame.width,
                                            &headless.pixel_format,
                                            &headless.bw1_polarity,
                                        )
                                    } else {
                                        Err("non-packed output unexpectedly received Gray8 pixels".to_string())
                                    };

                                    match frame.pixel_format {
                                        FramePixelFormat::Rgba8888Premul => latest_frame.publish_rgba(
                                            frame.width,
                                            frame.height,
                                            1.0,
                                            frame.data,
                                        ),
                                        FramePixelFormat::Gray8Opaque => latest_frame.publish_gray8(
                                            frame.width,
                                            frame.height,
                                            1.0,
                                            frame.data,
                                        ),
                                    }

                                    match converted {
                                        Ok((data, stride_bytes)) => send_binary_frame(
                                            target,
                                            &headless.frame_message,
                                            sequence,
                                            frame.width,
                                            frame.height,
                                            &headless.pixel_format,
                                            stride_bytes,
                                            data,
                                        ),
                                        Err(err) => eprintln!("headless frame conversion failed: {err}"),
                                    }
                                    animation_tick = animation_tick_receiver(
                                        animate,
                                        frame_interval,
                                        Instant::now(),
                                        &mut next_animation_at,
                                    );
                                }
                                Err(err) => eprintln!("headless render failed: {err}"),
                            },
                            HeadlessMode::Prime => match renderer.render_prime(&state) {
                                Ok(Some(frame)) => {
                                    record_render_stats(
                                        stats.as_deref(),
                                        render_started_at,
                                        &frame.timings,
                                        pipeline_submitted_at,
                                        frame_interval,
                                    );
                                    if let Some(stats) = stats.as_deref() {
                                        stats.record_headless_prime_timings(
                                            frame.prime_timings.prepare,
                                            frame.prime_timings.retarget,
                                            frame.prime_timings.fence_export,
                                            frame.prime_timings.gpu_finish_fallback,
                                            frame.prime_timings.export_metadata,
                                        );
                                    }
                                    sequence = sequence.wrapping_add(1);
                                    send_prime_frame(
                                        target,
                                        &headless.frame_message,
                                        sequence,
                                        frame,
                                        release_tx.clone(),
                                    );
                                    animation_tick = animation_tick_receiver(
                                        animate,
                                        frame_interval,
                                        Instant::now(),
                                        &mut next_animation_at,
                                    );
                                }
                                Ok(None) => {
                                    // Resume retained animation after a consumer frees a slot and
                                    // its independent Vulkan release fence signals.
                                    animation_tick = never();
                                    pending_prime_animation |= animate;
                                }
                                Err(err) => {
                                    eprintln!("headless PRIME render failed terminally: {err}");
                                    animation_tick = never();
                                    pending_prime_animation = false;
                                    if renderer.terminal_prime_shutdown_ready() {
                                        break;
                                    }
                                }
                            },
                        }
                    }
                    RenderMsg::Stop => break,
                }
            }
        }
    }

    running_flag.store(false, Ordering::Relaxed);
}

fn record_render_stats(
    stats: Option<&RendererStatsCollector>,
    render_started_at: Instant,
    timings: &RenderTimings,
    pipeline_submitted_at: Option<Instant>,
    frame_interval: Duration,
) {
    if let Some(stats) = stats {
        stats.record_render_timings(render_started_at.elapsed(), timings);
        stats.record_present_submit(Duration::ZERO);
        stats.record_display_interval(frame_interval);
        stats.record_frame_present();
        if let Some(submitted_at) = pipeline_submitted_at {
            stats.record_pipeline_submit_to_swap(submitted_at, Instant::now());
        }
    }
}

fn maybe_resume_prime_animation(
    pending: &mut bool,
    tree_tx: &Sender<TreeMsg>,
    frame_interval: Duration,
) {
    if std::mem::take(pending) {
        maybe_send_animation_pulse(tree_tx, true, frame_interval);
    }
}

fn animation_tick_receiver(
    animate: bool,
    frame_interval: Duration,
    now: Instant,
    next_animation_at: &mut Option<Instant>,
) -> Receiver<Instant> {
    if !animate {
        *next_animation_at = None;
        return never();
    }

    let deadline = next_animation_deadline(*next_animation_at, now, frame_interval);
    *next_animation_at = Some(deadline);
    at(deadline)
}

fn next_animation_deadline(
    previous_deadline: Option<Instant>,
    now: Instant,
    frame_interval: Duration,
) -> Instant {
    let Some(previous_deadline) = previous_deadline else {
        return now + frame_interval;
    };
    if previous_deadline > now {
        return previous_deadline;
    }

    let missed_intervals = now
        .saturating_duration_since(previous_deadline)
        .as_nanos()
        .checked_div(frame_interval.as_nanos())
        .unwrap_or(0)
        .saturating_add(1);
    u32::try_from(missed_intervals)
        .ok()
        .and_then(|count| frame_interval.checked_mul(count))
        .and_then(|advance| previous_deadline.checked_add(advance))
        .unwrap_or(now + frame_interval)
}

fn maybe_send_animation_pulse(tree_tx: &Sender<TreeMsg>, animate: bool, frame_interval: Duration) {
    if animate {
        let now = Instant::now();
        send_tree(
            tree_tx,
            TreeMsg::AnimationPulse {
                presented_at: now,
                predicted_next_present_at: now + frame_interval,
                trace: None,
            },
            false,
        );
    }
}

fn effective_renderer_cache_config(
    selected_rendering_api: RenderingApi,
    mut config: RendererCacheConfig,
    enabled_configured: bool,
) -> RendererCacheConfig {
    if matches!(selected_rendering_api, RenderingApi::Raster) && !enabled_configured {
        config.enabled = false;
    }
    config
}

struct HeadlessRgbaFrame {
    width: u32,
    height: u32,
    row_bytes: usize,
    pixel_format: FramePixelFormat,
    data: Vec<u8>,
    timings: RenderTimings,
}

struct HeadlessPrimeExport {
    #[cfg(any(feature = "video-interop-support", test))]
    release_id: u64,
    #[cfg(any(feature = "video-interop-support", test))]
    width: u32,
    #[cfg(any(feature = "video-interop-support", test))]
    height: u32,
    #[cfg(any(feature = "video-interop-support", test))]
    format: u32,
    #[cfg(any(feature = "video-interop-support", test))]
    objects: Vec<PrimeObjectMeta>,
    #[cfg(any(feature = "video-interop-support", test))]
    planes: Vec<PrimePlaneMeta>,
    #[cfg(any(feature = "video-interop-support", test))]
    acquire_sync: AcquireSync,
    timings: RenderTimings,
    prime_timings: HeadlessPrimeTimings,
}

#[derive(Clone, Copy, Debug, Default)]
struct HeadlessPrimeTimings {
    prepare: Duration,
    retarget: Duration,
    fence_export: Option<Duration>,
    gpu_finish_fallback: Option<Duration>,
    export_metadata: Duration,
}

#[cfg(any(feature = "video-interop-support", test))]
struct PrimeObjectMeta {
    fd: i32,
    size: u64,
    modifier: Option<u64>,
}

#[cfg(any(feature = "video-interop-support", test))]
#[derive(Clone)]
struct PrimePlaneMeta {
    object_index: u32,
    pitch: u32,
    offset: u64,
}

enum HeadlessRenderer {
    Raster(Box<RasterHeadlessRenderer>),
    #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
    Gl(Box<offscreen_gl::GlHeadlessRenderer>),
    #[cfg(all(target_os = "linux", feature = "headless-vulkan"))]
    Vulkan(Box<vulkan::VulkanHeadlessRenderer>),
}

impl HeadlessRenderer {
    #[allow(clippy::too_many_arguments)]
    fn new(
        mode: HeadlessMode,
        rendering_api: RenderingApi,
        width: u32,
        height: u32,
        renderer_cache_config: RendererCacheConfig,
        renderer_cache_enabled_configured: bool,
        _max_prime_in_flight: u32,
        _prime_drm_node: Option<&str>,
        pixel_format: &str,
    ) -> Result<Self, String> {
        let mut raster_renderer_cache_config = effective_renderer_cache_config(
            RenderingApi::Raster,
            renderer_cache_config,
            renderer_cache_enabled_configured,
        );
        if matches!(pixel_format, "bw1" | "gray2") {
            raster_renderer_cache_config.enabled = false;
        }
        let raster_pixel_format = if matches!(pixel_format, "bw1" | "gray2") {
            RasterPixelFormat::Gray8Opaque
        } else {
            RasterPixelFormat::Rgba8888Premul
        };

        match (mode, rendering_api) {
            (HeadlessMode::Binary, RenderingApi::Auto) => {
                #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
                {
                    match offscreen_gl::GlHeadlessRenderer::new(width, height, renderer_cache_config) {
                        Ok(renderer) => return Ok(Self::Gl(Box::new(renderer))),
                        Err(err) => eprintln!(
                            "headless auto GL startup failed; falling back to raster: {err}"
                        ),
                    }
                }
                RasterHeadlessRenderer::new(
                    width,
                    height,
                    raster_renderer_cache_config,
                    raster_pixel_format,
                )
                .map(Box::new)
                .map(Self::Raster)
            }
            (HeadlessMode::Binary, RenderingApi::Raster) => {
                RasterHeadlessRenderer::new(
                    width,
                    height,
                    raster_renderer_cache_config,
                    raster_pixel_format,
                )
                .map(Box::new)
                .map(Self::Raster)
            }
            (HeadlessMode::Prime, RenderingApi::Auto | RenderingApi::OpenGl) => {
                #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
                {
                    offscreen_gl::GlHeadlessRenderer::new_prime(
                        width,
                        height,
                        renderer_cache_config,
                        _max_prime_in_flight,
                        _prime_drm_node,
                    )
                    .map(Box::new)
                    .map(Self::Gl)
                }
                #[cfg(not(all(target_os = "linux", feature = "headless-opengl")))]
                {
                    Err("headless PRIME output requires Linux headless GL".to_string())
                }
            }
            (HeadlessMode::Prime, RenderingApi::Raster) => Err(
                "headless PRIME output requires rendering_api :opengl or :auto; :raster cannot export dma-buf frames"
                    .to_string(),
            ),
            (_, RenderingApi::OpenGl) => {
                #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
                {
                    offscreen_gl::GlHeadlessRenderer::new(width, height, renderer_cache_config)
                        .map(Box::new)
                        .map(Self::Gl)
                }
                #[cfg(not(all(target_os = "linux", feature = "headless-opengl")))]
                {
                    Err("rendering_api :opengl is not available for backend :headless in this build".to_string())
                }
            }
            (_, RenderingApi::Metal) => {
                Err("rendering_api :metal is only supported with backend :macos".to_string())
            }
            (HeadlessMode::Binary, RenderingApi::Vulkan) => Err(
                "headless Vulkan supports PRIME output only; binary output is not supported"
                    .to_string(),
            ),
            (HeadlessMode::Prime, RenderingApi::Vulkan) => {
                #[cfg(all(target_os = "linux", feature = "headless-vulkan"))]
                {
                    vulkan::VulkanHeadlessRenderer::new_prime(
                        width,
                        height,
                        renderer_cache_config,
                        _max_prime_in_flight,
                        _prime_drm_node,
                    )
                    .map(Box::new)
                    .map(Self::Vulkan)
                }
                #[cfg(not(all(target_os = "linux", feature = "headless-vulkan")))]
                {
                    Err("Vulkan rendering support is not available in this build".to_string())
                }
            }
        }
    }

    fn selected_rendering_api(&self) -> RenderingApi {
        match self {
            Self::Raster(_) => RenderingApi::Raster,
            #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
            Self::Gl(_) => RenderingApi::OpenGl,
            #[cfg(all(target_os = "linux", feature = "headless-vulkan"))]
            Self::Vulkan(_) => RenderingApi::Vulkan,
        }
    }

    #[cfg(feature = "vulkan")]
    fn vulkan_renderer_report(&self) -> Option<crate::backend::vulkan::VulkanRendererReport> {
        match self {
            #[cfg(all(target_os = "linux", feature = "headless-vulkan"))]
            Self::Vulkan(renderer) => Some(renderer.renderer_report()),
            _ => None,
        }
    }

    fn render_binary(&mut self, state: &RenderState) -> Result<HeadlessRgbaFrame, String> {
        match self {
            Self::Raster(renderer) => Ok(renderer.render(state)),
            #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
            Self::Gl(renderer) => renderer.render_binary(state),
            #[cfg(all(target_os = "linux", feature = "headless-vulkan"))]
            Self::Vulkan(_) => Err("headless Vulkan does not support binary output".to_string()),
        }
    }

    fn render_prime(
        &mut self,
        _state: &RenderState,
    ) -> Result<Option<HeadlessPrimeExport>, String> {
        match self {
            Self::Raster(_) => {
                Err("raster headless renderer cannot export PRIME frames".to_string())
            }
            #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
            Self::Gl(renderer) => renderer.render_prime(_state),
            #[cfg(all(target_os = "linux", feature = "headless-vulkan"))]
            Self::Vulkan(renderer) => renderer.render_prime(_state),
        }
    }

    fn release_prime(&mut self, _release_id: u64) -> bool {
        match self {
            Self::Raster(_) => false,
            #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
            Self::Gl(renderer) => {
                renderer.release_prime(_release_id);
                true
            }
            #[cfg(all(target_os = "linux", feature = "headless-vulkan"))]
            Self::Vulkan(renderer) => renderer.release_prime(_release_id),
        }
    }

    fn prime_completion_receiver(&self) -> Receiver<u64> {
        match self {
            #[cfg(all(target_os = "linux", feature = "headless-vulkan"))]
            Self::Vulkan(renderer) => renderer.completion_receiver(),
            Self::Raster(_) => never(),
            #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
            Self::Gl(_) => never(),
        }
    }

    fn complete_prime_retirement(&mut self, _release_id: u64) -> bool {
        match self {
            #[cfg(all(target_os = "linux", feature = "headless-vulkan"))]
            Self::Vulkan(renderer) => renderer.complete_retirement(_release_id),
            Self::Raster(_) => false,
            #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
            Self::Gl(_) => false,
        }
    }

    fn render_grayscale_dither_policy(&self, state: &RenderState) -> Result<Vec<u8>, String> {
        match self {
            Self::Raster(renderer) => renderer.renderer.render_grayscale_dither_policy(state),
            #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
            Self::Gl(_) => {
                Err("grayscale dithering currently requires raster rendering".to_string())
            }
            #[cfg(all(target_os = "linux", feature = "headless-vulkan"))]
            Self::Vulkan(_) => {
                Err("grayscale dithering is unavailable for Vulkan PRIME".to_string())
            }
        }
    }

    fn terminal_prime_shutdown_ready(&self) -> bool {
        match self {
            Self::Raster(_) => false,
            #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
            Self::Gl(renderer) => renderer.terminal_prime_shutdown_ready(),
            #[cfg(all(target_os = "linux", feature = "headless-vulkan"))]
            Self::Vulkan(renderer) => renderer.terminal_prime_shutdown_ready(),
        }
    }
}

struct RasterHeadlessRenderer {
    renderer: RasterBackend,
    width: u32,
    height: u32,
}

impl RasterHeadlessRenderer {
    fn new(
        width: u32,
        height: u32,
        renderer_cache_config: RendererCacheConfig,
        pixel_format: RasterPixelFormat,
    ) -> Result<Self, String> {
        let renderer = RasterBackend::with_cache_config_and_format(
            &RasterConfig { width, height },
            renderer_cache_config,
            pixel_format,
        )?;
        Ok(Self {
            renderer,
            width,
            height,
        })
    }

    fn render(&mut self, state: &RenderState) -> HeadlessRgbaFrame {
        let (frame, timings) = self.renderer.render_with_timings(state);
        HeadlessRgbaFrame {
            width: self.width,
            height: self.height,
            row_bytes: frame.row_bytes,
            pixel_format: match frame.format {
                RasterPixelFormat::Rgba8888Premul => FramePixelFormat::Rgba8888Premul,
                RasterPixelFormat::Gray8Opaque => FramePixelFormat::Gray8Opaque,
            },
            data: frame.data,
            timings,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn send_binary_frame(
    target: LocalPid,
    frame_message: &str,
    sequence: u64,
    width: u32,
    height: u32,
    pixel_format: &str,
    stride_bytes: u32,
    data: Vec<u8>,
) {
    let mut binary =
        OwnedBinary::new(data.len()).expect("failed to allocate headless frame binary");
    binary.as_mut_slice().copy_from_slice(&data);
    let mut env = OwnedEnv::new();
    let use_default_message = frame_message == "emerge_skia_frame";
    let frame_message = frame_message.to_string();
    let pixel_format = pixel_format.to_string();
    let _ = env.send_and_clear(&target, move |inner_env| {
        let data = binary.release(inner_env);
        let frame = vec![
            ("mode", "binary".encode(inner_env)),
            ("sequence", sequence.encode(inner_env)),
            ("width", width.encode(inner_env)),
            ("height", height.encode(inner_env)),
            ("scale", 1.0_f64.encode(inner_env)),
            ("pixel_format", pixel_format.encode(inner_env)),
            ("stride_bytes", stride_bytes.encode(inner_env)),
            ("data", data.encode(inner_env)),
            ("timestamp_native", current_wall_ms().encode(inner_env)),
        ]
        .encode(inner_env);
        encode_frame_message(inner_env, use_default_message, &frame_message, frame)
    });
}

#[cfg(any(feature = "video-interop-support", test))]
fn send_prime_frame(
    target: LocalPid,
    frame_message: &str,
    sequence: u64,
    frame: HeadlessPrimeExport,
    release_tx: Sender<HeadlessReleaseMsg>,
) {
    let mut env = OwnedEnv::new();
    let use_default_message = frame_message == "emerge_skia_frame";
    let frame_message = frame_message.to_string();
    let _ = env.send_and_clear(&target, move |inner_env| {
        let backend_token =
            ResourceArc::new(HeadlessPrimeBackendToken::new(release_tx, frame.release_id));
        let width = frame.width;
        let height = frame.height;
        let descriptor = Descriptor {
            version: 1,
            objects: frame
                .objects
                .into_iter()
                .map(|object| Object {
                    fd: object.fd,
                    size: object.size,
                    modifier: object
                        .modifier
                        .map(Modifier::Explicit)
                        .unwrap_or(Modifier::Implicit),
                })
                .collect(),
            layers: vec![Layer {
                fourcc: frame.format,
                planes: frame
                    .planes
                    .into_iter()
                    .map(|plane| Plane {
                        object_index: plane.object_index,
                        pitch: plane.pitch,
                        offset: plane.offset,
                    })
                    .collect(),
            }],
        };
        let frame = vec![
            ("mode", "prime".encode(inner_env)),
            ("sequence", sequence.encode(inner_env)),
            ("width", width.encode(inner_env)),
            ("height", height.encode(inner_env)),
            ("descriptor", descriptor.encode(inner_env)),
            ("acquire_sync", frame.acquire_sync.encode(inner_env)),
            ("backend_token", backend_token.encode(inner_env)),
            ("timestamp_native", current_wall_ms().encode(inner_env)),
        ]
        .encode(inner_env);
        encode_frame_message(inner_env, use_default_message, &frame_message, frame)
    });
}

#[cfg(not(any(feature = "video-interop-support", test)))]
fn send_prime_frame(
    _target: LocalPid,
    _frame_message: &str,
    _sequence: u64,
    _frame: HeadlessPrimeExport,
    _release_tx: Sender<HeadlessReleaseMsg>,
) {
    unreachable!("embedded CPU builds reject headless PRIME during startup")
}

fn encode_frame_message<'a>(
    env: Env<'a>,
    use_default_message: bool,
    frame_message: &str,
    frame: Term<'a>,
) -> Term<'a> {
    if use_default_message {
        (crate::atoms::emerge_skia_frame(), frame).encode(env)
    } else {
        (frame_message, frame).encode(env)
    }
}

fn convert_frame(
    rgba: &[u8],
    width: u32,
    pixel_format: &str,
    _bw1_polarity: &str,
) -> Result<(Vec<u8>, u32), String> {
    match pixel_format {
        "rgba8888" => Ok((rgba.to_vec(), width * 4)),
        "rgb888" => Ok((
            rgba.as_chunks::<4>()
                .0
                .iter()
                .flat_map(|px| [px[0], px[1], px[2]])
                .collect(),
            width * 3,
        )),
        "gray8" => Ok((
            rgba.as_chunks::<4>().0.iter().map(|px| luma8(px)).collect(),
            width,
        )),
        "gray4" => Ok((pack_gray4(rgba), width.div_ceil(2))),
        other => Err(format!("unsupported headless pixel format: {other}")),
    }
}

fn luma8(px: &[u8]) -> u8 {
    ((u16::from(px[0]) * 77 + u16::from(px[1]) * 150 + u16::from(px[2]) * 29) >> 8) as u8
}

fn pack_gray4(rgba: &[u8]) -> Vec<u8> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .map(|px| ((u16::from(luma8(px)) * 15) / 255) as u8)
        .collect::<Vec<_>>()
        .chunks(2)
        .map(|chunk| {
            chunk.iter().enumerate().fold(0_u8, |byte, (index, value)| {
                byte | (value << (4 - 4 * index))
            })
        })
        .collect()
}

fn current_wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raster_fallback_disables_cache_without_explicit_opt_in() {
        let default_config = RendererCacheConfig {
            enabled: true,
            ..RendererCacheConfig::default()
        };

        assert!(
            !effective_renderer_cache_config(RenderingApi::Raster, default_config, false).enabled
        );
        assert!(
            effective_renderer_cache_config(RenderingApi::Raster, default_config, true).enabled
        );
        assert!(
            effective_renderer_cache_config(RenderingApi::OpenGl, default_config, false).enabled
        );
    }

    #[test]
    fn target_fps_deadlines_do_not_accumulate_render_time() {
        let now = Instant::now();
        let interval = Duration::from_millis(33);
        let first = next_animation_deadline(None, now, interval);
        assert_eq!(first, now + interval);

        let before_deadline =
            next_animation_deadline(Some(first), now + Duration::from_millis(10), interval);
        assert_eq!(before_deadline, first);

        let after_missed_deadlines =
            next_animation_deadline(Some(first), now + Duration::from_millis(100), interval);
        assert_eq!(after_missed_deadlines, now + Duration::from_millis(132));
    }

    #[test]
    fn disabling_animation_cancels_the_target_fps_deadline() {
        let mut next_animation_at = Some(Instant::now() + Duration::from_secs(1));
        let disabled = animation_tick_receiver(
            false,
            Duration::from_millis(10),
            Instant::now(),
            &mut next_animation_at,
        );

        assert!(next_animation_at.is_none());
        assert!(matches!(
            disabled.recv_timeout(Duration::from_millis(1)),
            Err(crossbeam_channel::RecvTimeoutError::Timeout)
        ));
    }

    #[test]
    fn prime_release_resumes_one_pending_animation_pulse() {
        let (tree_tx, tree_rx) = unbounded();
        let mut pending = true;

        maybe_resume_prime_animation(&mut pending, &tree_tx, Duration::from_millis(16));
        assert!(!pending);
        assert!(matches!(
            tree_rx.try_recv(),
            Ok(TreeMsg::AnimationPulse { .. })
        ));

        maybe_resume_prime_animation(&mut pending, &tree_tx, Duration::from_millis(16));
        assert!(tree_rx.try_recv().is_err());
    }

    #[test]
    fn convert_frame_packs_gray_formats() {
        let rgba = vec![0, 0, 0, 255, 255, 255, 255, 255, 128, 128, 128, 255];

        assert_eq!(
            convert_frame(&rgba, 3, "gray8", "one_is_black").unwrap().1,
            3
        );
        assert_eq!(
            convert_frame(&rgba, 3, "gray4", "one_is_black").unwrap().1,
            2
        );
    }
}
