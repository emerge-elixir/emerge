use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use rustler::{Encoder, LocalPid, NifResult, OwnedBinary, OwnedEnv, ResourceArc};

use crate::{
    BackendKind, HeadlessConfig, InputTargetRelay, LatestFrameStore, RenderSender,
    RendererBackendKind, RendererHandles, RendererResource, RendererRuntimeInfo, StartConfig,
    VideoWake,
    actors::{RenderMsg, TreeMsg},
    assets,
    backend::{
        raster::{RasterBackend, RasterConfig},
        wake::BackendWakeHandle,
    },
    events::{SpawnEventActorConfig, spawn_event_actor},
    native_log::NativeLogRelay,
    renderer::{RenderState, RenderTimings, RendererCacheConfig},
    renderer_cache_status,
    runtime::tree_actor::TreeActorConfig,
    send_tree,
    stats::RendererStatsCollector,
    video::{self, VideoRegistry},
};

#[cfg(target_os = "linux")]
mod offscreen_gl;

pub(crate) fn start_renderer_with_config(
    config: StartConfig,
    initial_log_target: Option<LocalPid>,
) -> NifResult<ResourceArc<RendererResource>> {
    if config.headless.mode != "binary" {
        return Err(rustler::Error::Term(Box::new(
            "headless mode :prime is not implemented yet".to_string(),
        )));
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
    let render_sender = RenderSender {
        tx: render_tx,
        drop_rx: render_rx.clone(),
        log_render: config.render_log,
    };

    let backend_wake = BackendWakeHandle::noop();
    let video_registry = Arc::new(VideoRegistry::new(video::spawn_release_worker()));
    let headless = config.headless.clone();
    let latest_frame_for_thread = Arc::clone(&latest_frame);
    let stats_for_thread = renderer_stats.clone();
    let tree_tx_for_thread = tree_tx.clone();
    let running_for_thread = Arc::clone(&running_flag);
    let width = config.width;
    let height = config.height;
    let renderer_cache_config = config.renderer_cache_config;
    let renderer_backend = config.backend_renderer.kind;
    let (startup_tx, startup_rx) = bounded(1);

    let render_handle = thread::spawn(move || {
        run_render_loop(
            render_rx,
            tree_tx_for_thread,
            running_for_thread,
            latest_frame_for_thread,
            stats_for_thread,
            headless,
            target,
            width,
            height,
            renderer_cache_config,
            renderer_backend,
            startup_tx,
        );
    });

    let selected_renderer = match startup_rx.recv() {
        Ok(Ok(selected_renderer)) => selected_renderer,
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
        prime_video_supported: false,
        native_log,
        stats: renderer_stats,
        latest_frame,
        info: RendererRuntimeInfo {
            backend: BackendKind::Headless,
            requested_renderer: config.backend_renderer.kind,
            selected_renderer,
            raster_present: config.backend_renderer.raster_present,
            renderer_cache: renderer_cache_status(selected_renderer, renderer_cache_config),
            prime_video_supported: false,
        },
        close_signal_log: config.close_signal_log,
        log_render: config.render_log,
        log_input: false,
        handles: Mutex::new(Some(RendererHandles {
            backend_handle: Some(render_handle),
            input_handle: None,
            tree_handle: Some(tree_handle),
            event_handle: Some(event_handle),
            heartbeat_handle: None,
        })),
    };

    Ok(ResourceArc::new(resource))
}

fn run_render_loop(
    render_rx: Receiver<RenderMsg>,
    tree_tx: Sender<TreeMsg>,
    running_flag: Arc<AtomicBool>,
    latest_frame: Arc<LatestFrameStore>,
    stats: Option<Arc<RendererStatsCollector>>,
    headless: HeadlessConfig,
    target: LocalPid,
    width: u32,
    height: u32,
    renderer_cache_config: RendererCacheConfig,
    renderer_backend: RendererBackendKind,
    startup_tx: Sender<Result<RendererBackendKind, String>>,
) {
    let mut renderer =
        match HeadlessRenderer::new(renderer_backend, width, height, renderer_cache_config) {
            Ok(renderer) => renderer,
            Err(err) => {
                let _ = startup_tx.send(Err(err));
                running_flag.store(false, Ordering::Relaxed);
                return;
            }
        };
    let selected_renderer = renderer.selected_renderer();
    if startup_tx.send(Ok(selected_renderer)).is_err() {
        running_flag.store(false, Ordering::Relaxed);
        return;
    }

    let mut sequence = 0_u64;
    let frame_interval = headless
        .target_fps
        .map(|fps| Duration::from_secs_f64(1.0 / f64::from(fps.max(1))))
        .unwrap_or_else(|| Duration::from_millis(16));

    while running_flag.load(Ordering::Relaxed) {
        let Ok(msg) = render_rx.recv() else {
            break;
        };

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
                let state = RenderState::new(*scene, Default::default(), sequence, animate);
                match renderer.render(&state) {
                    Ok(frame) => {
                        if let Some(stats) = stats.as_ref() {
                            stats
                                .record_render_timings(render_started_at.elapsed(), &frame.timings);
                            stats.record_present_submit(Duration::ZERO);
                            stats.record_frame_present();
                            if let Some(submitted_at) = pipeline_submitted_at {
                                stats.record_pipeline_submit_to_swap(submitted_at, Instant::now());
                            }
                        }

                        sequence = sequence.wrapping_add(1);
                        latest_frame.publish_rgba(
                            frame.width,
                            frame.height,
                            1.0,
                            frame.data.clone(),
                        );
                        let converted = convert_frame(
                            &frame.data,
                            frame.width,
                            &headless.pixel_format,
                            &headless.bw1_polarity,
                        );
                        match converted {
                            Ok((data, stride_bytes)) => send_frame(
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

                        if animate {
                            let now = Instant::now();
                            send_tree(
                                &tree_tx,
                                TreeMsg::AnimationPulse {
                                    presented_at: now,
                                    predicted_next_present_at: now + frame_interval,
                                    trace: None,
                                },
                                false,
                            );
                        }
                    }
                    Err(err) => eprintln!("headless render failed: {err}"),
                }
            }
            RenderMsg::Stop => break,
        }
    }

    running_flag.store(false, Ordering::Relaxed);
}

struct HeadlessRgbaFrame {
    width: u32,
    height: u32,
    data: Vec<u8>,
    timings: RenderTimings,
}

enum HeadlessRenderer {
    Raster(RasterHeadlessRenderer),
    #[cfg(target_os = "linux")]
    Gl(offscreen_gl::GlHeadlessRenderer),
}

impl HeadlessRenderer {
    fn new(
        renderer_backend: RendererBackendKind,
        width: u32,
        height: u32,
        renderer_cache_config: RendererCacheConfig,
    ) -> Result<Self, String> {
        match renderer_backend {
            RendererBackendKind::Auto | RendererBackendKind::Raster => {
                RasterHeadlessRenderer::new(width, height, renderer_cache_config).map(Self::Raster)
            }
            #[cfg(target_os = "linux")]
            RendererBackendKind::Gl => {
                offscreen_gl::GlHeadlessRenderer::new(width, height, renderer_cache_config)
                    .map(Self::Gl)
            }
            #[cfg(not(target_os = "linux"))]
            RendererBackendKind::Gl => Err(
                "backend_renderer :gl is not available for backend :headless in this build"
                    .to_string(),
            ),
            RendererBackendKind::Metal => {
                Err("backend_renderer :metal is only supported with backend :macos".to_string())
            }
            RendererBackendKind::Vulkan => {
                Err("backend_renderer :vulkan is not implemented yet".to_string())
            }
        }
    }

    fn selected_renderer(&self) -> RendererBackendKind {
        match self {
            Self::Raster(_) => RendererBackendKind::Raster,
            #[cfg(target_os = "linux")]
            Self::Gl(_) => RendererBackendKind::Gl,
        }
    }

    fn render(&mut self, state: &RenderState) -> Result<HeadlessRgbaFrame, String> {
        match self {
            Self::Raster(renderer) => Ok(renderer.render(state)),
            #[cfg(target_os = "linux")]
            Self::Gl(renderer) => renderer.render(state),
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
    ) -> Result<Self, String> {
        let renderer = RasterBackend::with_cache_config(
            &RasterConfig { width, height },
            renderer_cache_config,
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
            data: frame.data,
            timings,
        }
    }
}

fn send_frame(
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
        if use_default_message {
            (crate::atoms::emerge_skia_frame(), frame).encode(inner_env)
        } else {
            (frame_message, frame).encode(inner_env)
        }
    });
}

fn convert_frame(
    rgba: &[u8],
    width: u32,
    pixel_format: &str,
    bw1_polarity: &str,
) -> Result<(Vec<u8>, u32), String> {
    match pixel_format {
        "rgba8888" => Ok((rgba.to_vec(), width * 4)),
        "rgb888" => Ok((
            rgba.chunks_exact(4)
                .flat_map(|px| [px[0], px[1], px[2]])
                .collect(),
            width * 3,
        )),
        "gray8" => Ok((rgba.chunks_exact(4).map(luma8).collect(), width)),
        "gray4" => Ok((pack_gray(rgba, 4, bw1_polarity), width.div_ceil(2))),
        "gray2" => Ok((pack_gray(rgba, 2, bw1_polarity), width.div_ceil(4))),
        "bw1" => Ok((pack_gray(rgba, 1, bw1_polarity), width.div_ceil(8))),
        other => Err(format!("unsupported headless pixel format: {other}")),
    }
}

fn luma8(px: &[u8]) -> u8 {
    ((u16::from(px[0]) * 77 + u16::from(px[1]) * 150 + u16::from(px[2]) * 29) >> 8) as u8
}

fn pack_gray(rgba: &[u8], bits: u8, bw1_polarity: &str) -> Vec<u8> {
    let values_per_byte = 8 / bits;
    let max_value = (1_u8 << bits) - 1;
    rgba.chunks_exact(4)
        .map(|px| {
            if bits == 1 {
                let black = luma8(px) < 128;
                match (black, bw1_polarity) {
                    (true, "one_is_black") | (false, "one_is_white") => 1,
                    _ => 0,
                }
            } else {
                ((u16::from(luma8(px)) * u16::from(max_value)) / 255) as u8
            }
        })
        .collect::<Vec<_>>()
        .chunks(values_per_byte as usize)
        .map(|chunk| {
            chunk.iter().enumerate().fold(0_u8, |byte, (index, value)| {
                let shift = 8 - bits * (index as u8 + 1);
                byte | (value << shift)
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
        assert_eq!(
            convert_frame(&rgba, 3, "gray2", "one_is_black").unwrap().1,
            1
        );
        assert_eq!(convert_frame(&rgba, 3, "bw1", "one_is_black").unwrap().1, 1);
    }
}
