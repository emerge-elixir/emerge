//! EmergeSkia NIF - Minimal Skia renderer for Elixir.
//!
//! This crate provides a Rustler NIF that exposes tree upload, layout,
//! rendering, and headless rasterization for Emerge.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError, bounded};

use rustler::{Atom, Binary, Env, LocalPid, NewBinary, NifResult, ResourceArc, Term};
use skia_safe::Font;

mod actors;
mod assets;
mod backend;
mod clipboard;
mod cursor;
mod debug_trace;
mod drm_input;
mod events;
mod input;
mod renderer;
mod tree;
mod video;

use actors::{EventMsg, RenderMsg, TreeMsg};
use assets::AssetConfig;
use backend::drm;
use backend::raster::{RasterBackend, RasterConfig};
use backend::wayland::{self, UserEvent, WaylandConfig};
use drm_input::DrmInput;
use events::spawn_event_actor;
use renderer::{RenderState, get_default_typeface, load_font, set_render_log_enabled};
use std::time::Instant;
use tree::element::{ElementId, ElementTree};
use tree::animation::AnimationRuntime;
use tree::layout::{layout_and_refresh_default, layout_and_refresh_default_with_animation};
use video::{VideoMode, VideoRegistry, VideoTargetResource, VideoWake};

type LayoutFrame<'a> = (Binary<'a>, f32, f32, f32, f32);
type LayoutFrames<'a> = Vec<LayoutFrame<'a>>;

// ============================================================================
// Atoms
// ============================================================================

mod atoms {
    rustler::atoms! {
        ok,
        error,
    }
}

// ============================================================================
// NIF Resource
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendKind {
    Wayland,
    Drm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OffscreenAssetMode {
    Await,
    Snapshot,
}

impl OffscreenAssetMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "await" => Ok(Self::Await),
            "snapshot" => Ok(Self::Snapshot),
            other => Err(format!(
                "invalid offscreen asset mode: {other}; expected 'await' or 'snapshot'"
            )),
        }
    }
}

struct RendererResource {
    running_flag: Arc<AtomicBool>,
    backend: BackendKind,
    event_proxy: Arc<Mutex<Option<winit::event_loop::EventLoopProxy<UserEvent>>>>,
    stop_flag: Arc<AtomicBool>,
    tree_tx: Sender<TreeMsg>,
    event_tx: Sender<EventMsg>,
    render_tx: RenderSender,
    cursor_tx: Option<Sender<RenderMsg>>,
    video_registry: Arc<VideoRegistry>,
    video_wake: VideoWake,
    prime_video_supported: bool,
    log_render: bool,
    log_input: bool,
}

#[derive(Clone)]
struct RenderSender {
    tx: Sender<RenderMsg>,
    drop_rx: Receiver<RenderMsg>,
    log_render: bool,
}

impl RenderSender {
    fn send_latest(&self, msg: RenderMsg) {
        match self.tx.try_send(msg) {
            Ok(()) => {}
            Err(TrySendError::Full(msg)) => {
                let _ = self.drop_rx.try_recv();
                let _ = self.tx.try_send(msg);
                if self.log_render {
                    eprintln!("render queue overwrite");
                }
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

/// Resource for holding an element tree (for layout/rendering).
struct TreeResource {
    tree: Mutex<ElementTree>,
}

struct TestHarnessHandles {
    proxy_handle: thread::JoinHandle<()>,
    tree_handle: thread::JoinHandle<()>,
    event_handle: thread::JoinHandle<()>,
}

struct TestHarnessResource {
    tree_tx: Sender<TreeMsg>,
    event_tx: Sender<EventMsg>,
    render_rx: Receiver<RenderMsg>,
    tree_tap_rx: Receiver<TreeMsg>,
    base_instant: Mutex<Instant>,
    handles: Mutex<Option<TestHarnessHandles>>,
}

impl rustler::Resource for RendererResource {}

impl rustler::Resource for TreeResource {}

impl rustler::Resource for TestHarnessResource {}

impl Drop for TestHarnessResource {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

impl TestHarnessResource {
    fn stop_inner(&self) {
        let mut handles_guard = match self.handles.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };

        let Some(handles) = handles_guard.take() else {
            return;
        };

        send_event(&self.event_tx, EventMsg::Stop, false);
        send_tree(&self.tree_tx, TreeMsg::Stop, false);

        let _ = handles.proxy_handle.join();
        let _ = handles.event_handle.join();
        let _ = handles.tree_handle.join();
        assets::stop();
    }
}

impl RendererResource {
    fn stop(&self) {
        assets::stop();
        self.stop_flag.store(true, Ordering::Relaxed);
        send_tree(&self.tree_tx, TreeMsg::Stop, self.log_render);
        send_event(&self.event_tx, EventMsg::Stop, self.log_input);
        self.render_tx.send_latest(RenderMsg::Stop);
        if let Some(cursor_tx) = &self.cursor_tx {
            let _ = cursor_tx.send(RenderMsg::Stop);
        }
        match self.backend {
            BackendKind::Wayland => {
                if let Ok(guard) = self.event_proxy.lock()
                    && let Some(proxy) = guard.as_ref()
                {
                    let _ = proxy.send_event(UserEvent::Stop);
                }
            }
            BackendKind::Drm => {
                self.running_flag.store(false, Ordering::Relaxed);
            }
        }
    }
}

fn send_tree(tree_tx: &Sender<TreeMsg>, msg: TreeMsg, log_render: bool) {
    match tree_tx.try_send(msg) {
        Ok(()) => {}
        Err(TrySendError::Full(msg)) => {
            if log_render {
                eprintln!("tree channel full, blocking send");
            }
            crate::debug_trace::hover_trace!("tree_channel", "tree channel full, blocking send");
            let _ = tree_tx.send(msg);
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

fn send_event(event_tx: &Sender<EventMsg>, msg: EventMsg, log_input: bool) {
    match event_tx.try_send(msg) {
        Ok(()) => {}
        Err(TrySendError::Full(msg)) => {
            if log_input {
                eprintln!("event channel full, blocking send");
            }
            crate::debug_trace::hover_trace!("event_channel", "event channel full, blocking send");
            let _ = event_tx.send(msg);
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

fn push_tree_message_flat(msg: TreeMsg, out: &mut Vec<TreeMsg>) {
    match msg {
        TreeMsg::Batch(messages) => {
            for nested in messages {
                push_tree_message_flat(nested, out);
            }
        }
        other => out.push(other),
    }
}

fn is_animation_pulse(msg: &TreeMsg) -> bool {
    matches!(msg, TreeMsg::AnimationPulse { .. })
}

fn batch_is_animation_only(messages: &[TreeMsg]) -> bool {
    !messages.is_empty() && messages.iter().all(is_animation_pulse)
}

#[cfg(feature = "hover-trace")]
fn trace_element_snapshots(
    tree: &ElementTree,
) -> Vec<(ElementId, f32, f32, f32, f32, Option<f64>)> {
    tree.nodes
        .values()
        .filter(|element| {
            element.attrs.on_mouse_move.unwrap_or(false)
                || element.attrs.mouse_over.is_some()
                || element.attrs.mouse_over_active.unwrap_or(false)
        })
        .filter_map(|element| {
            element.frame.map(|frame| {
                (
                    element.id.clone(),
                    frame.x,
                    frame.y,
                    frame.width,
                    frame.height,
                    element.attrs.move_x,
                )
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefreshDecision {
    Skip,
    UseCachedRebuild,
    Recompute,
}

fn decide_refresh_action(
    tree_changed: bool,
    registry_requested: bool,
    has_cached_rebuild: bool,
) -> RefreshDecision {
    if tree_changed {
        RefreshDecision::Recompute
    } else if registry_requested && has_cached_rebuild {
        RefreshDecision::UseCachedRebuild
    } else if registry_requested {
        RefreshDecision::Recompute
    } else {
        RefreshDecision::Skip
    }
}

// ============================================================================
// NIF Functions
// ============================================================================

#[derive(Clone, Debug)]
struct StartConfig {
    backend: BackendKind,
    title: String,
    width: u32,
    height: u32,
    drm_card: Option<String>,
    drm_hw_cursor: bool,
    drm_input_log: bool,
    render_log: bool,
}

#[derive(rustler::NifMap)]
struct StartOptsNif {
    backend: String,
    title: String,
    width: u32,
    height: u32,
    drm_card: Option<String>,
    hw_cursor: bool,
    input_log: bool,
    render_log: bool,
}

struct TreeActorConfig {
    render_sender: RenderSender,
    event_tx: Sender<EventMsg>,
    render_counter: Arc<AtomicU64>,
    log_input: bool,
    wayland_proxy: Option<winit::event_loop::EventLoopProxy<UserEvent>>,
    initial_width: u32,
    initial_height: u32,
}

fn spawn_tree_actor(tree_rx: Receiver<TreeMsg>, config: TreeActorConfig) -> thread::JoinHandle<()> {
    spawn_tree_actor_with_initial_tree(tree_rx, config, ElementTree::new())
}

fn spawn_tree_actor_with_initial_tree(
    tree_rx: Receiver<TreeMsg>,
    config: TreeActorConfig,
    initial_tree: ElementTree,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let TreeActorConfig {
            render_sender,
            event_tx,
            render_counter,
            log_input,
            wayland_proxy,
            initial_width,
            initial_height,
        } = config;

        let mut tree = initial_tree;
        let mut width = (initial_width as f32).max(1.0);
        let mut height = (initial_height as f32).max(1.0);
        let mut scale = 1.0f32;
        let mut cached_rebuild: Option<events::RegistryRebuildPayload> = None;
        let mut animation_runtime = AnimationRuntime::default();
        let mut latest_animation_sample_time: Option<Instant> = None;
        let mut pending_msg: Option<TreeMsg> = None;

        loop {
            let msg = match pending_msg.take() {
                Some(msg) => msg,
                None => match tree_rx.recv() {
                    Ok(msg) => msg,
                    Err(_) => return,
                },
            };
            let mut messages = Vec::new();
            push_tree_message_flat(msg, &mut messages);
            let animation_only_batch = batch_is_animation_only(&messages);
            while let Ok(next) = tree_rx.try_recv() {
                let mut next_messages = Vec::new();
                push_tree_message_flat(next, &mut next_messages);

                if batch_is_animation_only(&next_messages) != animation_only_batch {
                    pending_msg = Some(TreeMsg::Batch(next_messages));
                    break;
                }

                messages.extend(next_messages);
            }

            let mut scroll_acc = std::collections::HashMap::new();
            let mut thumb_drag_x_acc = std::collections::HashMap::new();
            let mut thumb_drag_y_acc = std::collections::HashMap::new();
            let mut hover_x_state = std::collections::HashMap::new();
            let mut hover_y_state = std::collections::HashMap::new();
            let mut mouse_over_active_state = std::collections::HashMap::new();
            let mut mouse_down_active_state = std::collections::HashMap::new();
            let mut focused_active_state = std::collections::HashMap::new();
            let mut tree_changed = false;
            let mut registry_requested = false;
            let mut animation_sample_time = latest_animation_sample_time;

            for message in messages.iter().cloned() {
                match message {
                    TreeMsg::Stop => return,
                    TreeMsg::Batch(_) => {
                        unreachable!("tree batches must be flattened before processing")
                    }
                    TreeMsg::UploadTree { bytes } => match tree::deserialize::decode_tree(&bytes) {
                        Ok(decoded) => {
                            tree = decoded;
                            tree_changed = true;
                        }
                        Err(err) => {
                            eprintln!("tree upload failed: {err}");
                        }
                    },
                    TreeMsg::PatchTree { bytes } => {
                        let patches = match tree::patch::decode_patches(&bytes) {
                            Ok(patches) => patches,
                            Err(err) => {
                                eprintln!("tree patch decode failed: {err}");
                                continue;
                            }
                        };
                        if let Err(err) = tree::patch::apply_patches(&mut tree, patches) {
                            eprintln!("tree patch apply failed: {err}");
                            continue;
                        }
                        tree_changed = true;
                    }
                    TreeMsg::Resize {
                        width: w,
                        height: h,
                        scale: s,
                    } => {
                        width = w.max(1.0);
                        height = h.max(1.0);
                        scale = s;
                        tree_changed = true;
                    }
                    TreeMsg::ScrollRequest { element_id, dx, dy } => {
                        let entry = scroll_acc.entry(element_id).or_insert((0.0, 0.0));
                        entry.0 += dx;
                        entry.1 += dy;
                    }
                    TreeMsg::ScrollbarThumbDragX { element_id, dx } => {
                        let entry = thumb_drag_x_acc.entry(element_id).or_insert(0.0);
                        *entry += dx;
                    }
                    TreeMsg::ScrollbarThumbDragY { element_id, dy } => {
                        let entry = thumb_drag_y_acc.entry(element_id).or_insert(0.0);
                        *entry += dy;
                    }
                    TreeMsg::SetScrollbarXHover {
                        element_id,
                        hovered,
                    } => {
                        hover_x_state.insert(element_id, hovered);
                    }
                    TreeMsg::SetScrollbarYHover {
                        element_id,
                        hovered,
                    } => {
                        hover_y_state.insert(element_id, hovered);
                    }
                    TreeMsg::SetMouseOverActive { element_id, active } => {
                        crate::debug_trace::hover_trace!(
                            "tree_msg",
                            "set_mouse_over_active id={:?} active={}",
                            element_id.0,
                            active
                        );
                        mouse_over_active_state.insert(element_id, active);
                    }
                    TreeMsg::SetMouseDownActive { element_id, active } => {
                        mouse_down_active_state.insert(element_id, active);
                    }
                    TreeMsg::SetFocusedActive { element_id, active } => {
                        focused_active_state.insert(element_id, active);
                    }
                    TreeMsg::SetTextInputContent {
                        element_id,
                        content,
                    } => {
                        let changed = tree.set_text_input_content(&element_id, content);
                        tree_changed |= changed;
                    }
                    TreeMsg::SetTextInputRuntime {
                        element_id,
                        focused,
                        cursor,
                        selection_anchor,
                        preedit,
                        preedit_cursor,
                    } => {
                        let changed = tree.set_text_input_runtime(
                            &element_id,
                            focused,
                            cursor,
                            selection_anchor,
                            preedit,
                            preedit_cursor,
                        );
                        tree_changed |= changed;
                    }
                    TreeMsg::AnimationPulse {
                        presented_at,
                        predicted_next_present_at,
                    } => {
                        crate::debug_trace::hover_trace!(
                            "tree_pulse",
                            "presented_at={:?} predicted_next={:?}",
                            presented_at,
                            predicted_next_present_at
                        );
                        animation_sample_time = Some(predicted_next_present_at.max(presented_at));
                        tree_changed = true;
                    }
                    TreeMsg::RebuildRegistry => {
                        registry_requested = true;
                    }
                    TreeMsg::AssetStateChanged => {
                        tree_changed = true;
                    }
                }
            }

            for (id, (dx, dy)) in scroll_acc {
                let changed = tree.apply_scroll(&id, dx, dy);
                tree_changed |= changed;
            }

            for (id, dx) in thumb_drag_x_acc {
                let changed = tree.apply_scroll_x(&id, dx);
                tree_changed |= changed;
            }

            for (id, dy) in thumb_drag_y_acc {
                let changed = tree.apply_scroll_y(&id, dy);
                tree_changed |= changed;
            }

            for (id, hovered) in hover_x_state {
                tree_changed |= tree.set_scrollbar_x_hover(&id, hovered);
            }

            for (id, hovered) in hover_y_state {
                tree_changed |= tree.set_scrollbar_y_hover(&id, hovered);
            }

            for (id, active) in &mouse_over_active_state {
                tree_changed |= tree.set_mouse_over_active(id, *active);
            }

            for (id, active) in mouse_down_active_state {
                tree_changed |= tree.set_mouse_down_active(&id, active);
            }

            for (id, active) in focused_active_state {
                tree_changed |= tree.set_focused_active(&id, active);
            }

            let refresh_decision =
                decide_refresh_action(tree_changed, registry_requested, cached_rebuild.is_some());

            match refresh_decision {
                RefreshDecision::Skip => continue,
                RefreshDecision::UseCachedRebuild => {
                    if let Some(rebuild) = cached_rebuild.clone() {
                        send_event(&event_tx, EventMsg::RegistryUpdate { rebuild }, log_input);
                    }
                    continue;
                }
                RefreshDecision::Recompute => {
                    assets::ensure_tree_sources(&tree);

                    let constraint = tree::layout::Constraint::new(width, height);
                    let sample_time = animation_sample_time.unwrap_or_else(Instant::now);
                    latest_animation_sample_time = Some(sample_time);
                    crate::debug_trace::hover_trace!(
                        "tree_recompute",
                        "sample_time={:?} cached_rebuild={} tree_changed={} registry_requested={}",
                        sample_time,
                        cached_rebuild.is_some(),
                        tree_changed,
                        registry_requested
                    );
                    animation_runtime.sync_with_tree(&tree, sample_time);
                    let output = if animation_runtime.is_empty() {
                        layout_and_refresh_default(&mut tree, constraint, scale)
                    } else {
                        layout_and_refresh_default_with_animation(
                            &mut tree,
                            constraint,
                            scale,
                            &animation_runtime,
                            sample_time,
                        )
                    };
                    cached_rebuild = Some(output.event_rebuild.clone());
                    send_event(
                        &event_tx,
                        EventMsg::RegistryUpdate {
                            rebuild: output.event_rebuild,
                        },
                        log_input,
                    );

                    let version = render_counter.fetch_add(1, Ordering::Relaxed) + 1;
                    render_sender.send_latest(RenderMsg::Commands {
                        commands: output.commands,
                        version,
                        animate: output.animations_active,
                        ime_enabled: output.ime_enabled,
                        ime_cursor_area: output.ime_cursor_area,
                    });

                    if let Some(proxy) = wayland_proxy.as_ref() {
                        let _ = proxy.send_event(UserEvent::Redraw);
                    }

                    #[cfg(feature = "hover-trace")]
                    {
                        for (id, x, y, w, h, move_x) in trace_element_snapshots(&tree) {
                            crate::debug_trace::hover_trace!(
                                "tree_snapshot",
                                "id={:?} frame=({x:.2},{y:.2},{w:.2},{h:.2}) move_x={:.2} visual_x={:.2}",
                                id.0,
                                move_x.unwrap_or(0.0),
                                x + move_x.unwrap_or(0.0) as f32
                            );
                        }
                    }

                    if animation_runtime.is_empty() || !output.animations_active {
                        latest_animation_sample_time = None;
                    }
                }
            }
        }
    })
}

fn start_with_config(config: StartConfig) -> NifResult<ResourceArc<RendererResource>> {
    let running_flag = Arc::new(AtomicBool::new(true));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let render_counter = Arc::new(AtomicU64::new(0));

    let log_input = matches!(config.backend, BackendKind::Drm) && config.drm_input_log;
    let log_render = config.render_log;
    set_render_log_enabled(log_render);

    let (tree_tx, tree_rx) = bounded(512);
    let (event_tx, event_rx) = bounded(4096);
    let (render_tx, render_rx) = bounded(1);
    let render_sender = RenderSender {
        tx: render_tx,
        drop_rx: render_rx.clone(),
        log_render,
    };
    let (cursor_tx, cursor_rx) = bounded(1024);

    assets::start(tree_tx.clone(), log_render);

    let system_clipboard = matches!(config.backend, BackendKind::Wayland);
    let _event_handle = spawn_event_actor(event_rx, tree_tx.clone(), log_render, system_clipboard);

    let initial_width = config.width;
    let initial_height = config.height;
    let release_tx = video::spawn_release_worker();
    let video_registry = Arc::new(VideoRegistry::new(release_tx));
    let event_proxy = Arc::new(Mutex::new(None));

    let (backend, prime_video_supported) = match config.backend {
        BackendKind::Wayland => {
            let (proxy_tx, proxy_rx) = mpsc::channel();
            let running_flag_clone = Arc::clone(&running_flag);
            let tree_tx_clone = tree_tx.clone();
            let event_tx_clone = event_tx.clone();
            let video_registry_clone = Arc::clone(&video_registry);
            let wayland_config = WaylandConfig {
                title: config.title,
                width: config.width,
                height: config.height,
            };

            thread::spawn(move || {
                wayland::run(
                    wayland_config,
                    running_flag_clone,
                    tree_tx_clone,
                    event_tx_clone,
                    render_rx,
                    video_registry_clone,
                    proxy_tx,
                );
            });

            let startup = proxy_rx
                .recv()
                .map_err(|_| rustler::Error::Term(Box::new("failed to receive event proxy")))?
                .map_err(|reason| rustler::Error::Term(Box::new(reason)))?;

            if let Ok(mut guard) = event_proxy.lock() {
                *guard = Some(startup.proxy.clone());
            }

            spawn_tree_actor(
                tree_rx,
                TreeActorConfig {
                    render_sender: render_sender.clone(),
                    event_tx: event_tx.clone(),
                    render_counter: Arc::clone(&render_counter),
                    log_input,
                    wayland_proxy: Some(startup.proxy.clone()),
                    initial_width,
                    initial_height,
                },
            );

            (BackendKind::Wayland, startup.prime_video_supported)
        }
        BackendKind::Drm => {
            let (screen_tx, screen_rx) = bounded(1);
            let event_tx_clone = event_tx.clone();
            let cursor_tx_clone = cursor_tx.clone();
            let stop_clone = Arc::clone(&stop_flag);
            let input_log = log_input;
            let drm_input_size = (initial_width, initial_height);
            let video_registry_clone = Arc::clone(&video_registry);

            thread::spawn(move || {
                let mut input = DrmInput::new(
                    drm_input_size,
                    screen_rx,
                    event_tx_clone,
                    cursor_tx_clone,
                    input_log,
                );
                while !stop_clone.load(Ordering::Relaxed) {
                    input.poll();
                    std::thread::sleep(Duration::from_millis(2));
                }
            });

            let running_flag_clone = Arc::clone(&running_flag);
            let stop_for_thread = Arc::clone(&stop_flag);
            let render_counter_clone = Arc::clone(&render_counter);
            let tree_tx_clone = tree_tx.clone();
            let event_tx_clone = event_tx.clone();
            let drm_config = drm::DrmRunConfig {
                requested_size: Some((config.width, config.height)),
                card_path: config.drm_card,
                hw_cursor: config.drm_hw_cursor,
                render_log: log_render,
            };

            thread::spawn(move || {
                drm::run(
                    drm::DrmRunContext {
                        stop: stop_for_thread,
                        running_flag: running_flag_clone,
                        tree_tx: tree_tx_clone,
                        render_rx,
                        cursor_rx,
                        event_tx: event_tx_clone,
                        screen_tx,
                        render_counter: render_counter_clone,
                        video_registry: video_registry_clone,
                    },
                    drm_config,
                );
            });

            spawn_tree_actor(
                tree_rx,
                TreeActorConfig {
                    render_sender: render_sender.clone(),
                    event_tx: event_tx.clone(),
                    render_counter: Arc::clone(&render_counter),
                    log_input,
                    wayland_proxy: None,
                    initial_width,
                    initial_height,
                },
            );

            (BackendKind::Drm, true)
        }
    };

    let video_wake = if matches!(backend, BackendKind::Wayland) {
        VideoWake::Wayland(Arc::clone(&event_proxy))
    } else {
        VideoWake::Noop
    };

    let resource = RendererResource {
        running_flag,
        backend,
        event_proxy,
        stop_flag,
        tree_tx,
        event_tx,
        render_tx: render_sender,
        cursor_tx: if matches!(backend, BackendKind::Drm) {
            Some(cursor_tx)
        } else {
            None
        },
        video_registry,
        video_wake,
        prime_video_supported,
        log_render,
        log_input,
    };

    Ok(ResourceArc::new(resource))
}

#[rustler::nif]
fn start(title: String, width: u32, height: u32) -> NifResult<ResourceArc<RendererResource>> {
    start_with_config(StartConfig {
        backend: BackendKind::Wayland,
        title,
        width,
        height,
        drm_card: None,
        drm_hw_cursor: true,
        drm_input_log: false,
        render_log: false,
    })
}

#[rustler::nif]
fn start_opts(opts: StartOptsNif) -> NifResult<ResourceArc<RendererResource>> {
    let backend = opts.backend.to_lowercase();
    let backend = match backend.as_str() {
        "drm" => BackendKind::Drm,
        "wayland" | "x11" => BackendKind::Wayland,
        other => {
            return Err(rustler::Error::Term(Box::new(format!(
                "unsupported backend: {other}"
            ))));
        }
    };

    start_with_config(StartConfig {
        backend,
        title: opts.title,
        width: opts.width,
        height: opts.height,
        drm_card: opts.drm_card,
        drm_hw_cursor: opts.hw_cursor,
        drm_input_log: opts.input_log,
        render_log: opts.render_log,
    })
}

#[rustler::nif]
fn stop(renderer: ResourceArc<RendererResource>) -> Atom {
    renderer.stop();
    atoms::ok()
}

#[rustler::nif]
fn video_target_new(
    renderer: ResourceArc<RendererResource>,
    id: String,
    width: u32,
    height: u32,
    mode: String,
) -> Result<ResourceArc<VideoTargetResource>, String> {
    let mode = VideoMode::parse(&mode)?;

    if matches!(mode, VideoMode::Prime) && !renderer.prime_video_supported {
        return Err(
            "prime video targets require a real Wayland session or the DRM backend".to_string(),
        );
    }

    let spec = video::VideoTargetSpec {
        id: id.clone(),
        width,
        height,
        mode,
    };
    renderer.video_registry.create_target(spec)?;

    Ok(ResourceArc::new(VideoTargetResource {
        id,
        _width: width,
        _height: height,
        _mode: mode,
        registry: Arc::clone(&renderer.video_registry),
        wake: renderer.video_wake.clone(),
    }))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn video_target_submit_prime(
    target: ResourceArc<VideoTargetResource>,
    desc: video::PrimeDesc,
) -> Result<Atom, String> {
    target.registry.submit_prime(&target.id, desc.into())?;
    target.wake.notify();
    Ok(atoms::ok())
}

#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_upload(renderer: ResourceArc<RendererResource>, data: Binary) -> Result<Atom, String> {
    let bytes = data.as_slice().to_vec();
    send_tree(
        &renderer.tree_tx,
        TreeMsg::UploadTree { bytes },
        renderer.log_render,
    );
    Ok(atoms::ok())
}

#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_patch(renderer: ResourceArc<RendererResource>, data: Binary) -> Result<Atom, String> {
    let bytes = data.as_slice().to_vec();
    send_tree(
        &renderer.tree_tx,
        TreeMsg::PatchTree { bytes },
        renderer.log_render,
    );
    Ok(atoms::ok())
}

#[rustler::nif]
fn measure_text(text: String, font_size: f32) -> (f32, f32, f32, f32) {
    let typeface = get_default_typeface();
    let font = Font::new(&*typeface, font_size);

    let (width, _bounds) = font.measure_str(&text, None);
    let (_, metrics) = font.metrics();

    let ascent = metrics.ascent.abs();
    let descent = metrics.descent;
    let line_height = ascent + descent;

    (width, line_height, ascent, descent)
}

/// Load a font from binary data and register it with a name.
///
/// - `name`: Family name to register (e.g., "my-font")
/// - `weight`: Font weight (100-900, 400=normal, 700=bold)
/// - `italic`: Whether this is an italic variant
/// - `data`: Binary font data (TTF file contents)
#[rustler::nif(schedule = "DirtyIo")]
fn load_font_nif(name: String, weight: u32, italic: bool, data: Binary) -> Result<Atom, String> {
    load_font(&name, weight as u16, italic, data.as_slice())?;
    Ok(atoms::ok())
}

#[rustler::nif(schedule = "DirtyIo")]
fn configure_assets_nif(
    _renderer: ResourceArc<RendererResource>,
    sources: Vec<String>,
    runtime_enabled: bool,
    allowlist: Vec<String>,
    follow_symlinks: bool,
    max_file_size: u64,
    extensions: Vec<String>,
) -> Atom {
    assets::configure(AssetConfig {
        sources,
        runtime_enabled,
        runtime_allowlist: allowlist,
        runtime_follow_symlinks: follow_symlinks,
        runtime_max_file_size: max_file_size,
        runtime_extensions: extensions,
    });
    atoms::ok()
}

#[rustler::nif]
fn is_running(renderer: ResourceArc<RendererResource>) -> bool {
    renderer.running_flag.load(Ordering::Relaxed)
}

// ============================================================================
// Input NIF Functions
// ============================================================================

/// Set the input event mask to filter which events are sent.
///
/// Mask bits:
/// - 0x01: Key events
/// - 0x02: Text input commit/preedit events
/// - 0x04: Cursor position events
/// - 0x08: Cursor button events
/// - 0x10: Cursor scroll events
/// - 0x20: Cursor enter/exit events
/// - 0x40: Resize events
/// - 0x80: Focus events
/// - 0xFF: All events
#[rustler::nif]
fn set_input_mask(renderer: ResourceArc<RendererResource>, mask: u32) -> Atom {
    send_event(
        &renderer.event_tx,
        EventMsg::SetInputMask(mask),
        renderer.log_input,
    );
    atoms::ok()
}

/// Set the target process to receive input events.
///
/// Input events are sent directly to the target process as
/// `{:emerge_skia_event, event}` messages.
#[rustler::nif]
fn set_input_target(renderer: ResourceArc<RendererResource>, pid: Option<LocalPid>) -> Atom {
    send_event(
        &renderer.event_tx,
        EventMsg::SetInputTarget(pid),
        renderer.log_input,
    );
    atoms::ok()
}

// ============================================================================
// Raster NIF Functions
// ============================================================================

/// Render an encoded tree to an RGBA pixel buffer (synchronous, no window).
#[rustler::nif(schedule = "DirtyCpu")]
fn render_tree_to_pixels_nif<'a>(
    env: Env<'a>,
    data: Binary,
    width: u32,
    height: u32,
    scale: f32,
    sources: Vec<String>,
    runtime_enabled: bool,
    allowlist: Vec<String>,
    follow_symlinks: bool,
    max_file_size: u64,
    extensions: Vec<String>,
    asset_mode: String,
    asset_timeout_ms: u64,
) -> Result<Binary<'a>, String> {
    let mode = OffscreenAssetMode::parse(&asset_mode)?;
    let mut tree = tree::deserialize::decode_tree(data.as_slice()).map_err(|e| e.to_string())?;

    assets::configure(AssetConfig {
        sources,
        runtime_enabled,
        runtime_allowlist: allowlist,
        runtime_follow_symlinks: follow_symlinks,
        runtime_max_file_size: max_file_size,
        runtime_extensions: extensions,
    });

    match mode {
        OffscreenAssetMode::Await => {
            assets::resolve_tree_sources_sync(&tree, Some(Duration::from_millis(asset_timeout_ms)))?
        }
        OffscreenAssetMode::Snapshot => assets::snapshot_tree_sources(&tree),
    }

    let constraint = tree::layout::Constraint::new(width as f32, height as f32);
    let output = layout_and_refresh_default(&mut tree, constraint, scale);

    let config = RasterConfig { width, height };
    let mut backend = RasterBackend::new(&config)?;

    let state = RenderState {
        commands: output.commands,
        ..Default::default()
    };

    let frame = backend.render(&state);

    let mut binary = NewBinary::new(env, frame.data.len());
    binary.as_mut_slice().copy_from_slice(&frame.data);

    Ok(binary.into())
}

// ============================================================================
// Tree NIF Functions
// ============================================================================

/// Create a new empty tree resource.
#[rustler::nif]
fn tree_new() -> ResourceArc<TreeResource> {
    ResourceArc::new(TreeResource {
        tree: Mutex::new(ElementTree::new()),
    })
}

/// Upload a full tree from EMRG binary format.
/// Replaces any existing tree contents.
#[rustler::nif]
fn tree_upload(tree_res: ResourceArc<TreeResource>, data: Binary) -> Result<Atom, String> {
    let decoded = tree::deserialize::decode_tree(data.as_slice()).map_err(|e| e.to_string())?;

    if let Ok(mut tree) = tree_res.tree.lock() {
        *tree = decoded;
        Ok(atoms::ok())
    } else {
        Err("failed to lock tree".to_string())
    }
}

#[rustler::nif]
fn tree_upload_roundtrip<'a>(
    env: Env<'a>,
    tree_res: ResourceArc<TreeResource>,
    data: Binary,
) -> Result<Binary<'a>, String> {
    let decoded = tree::deserialize::decode_tree(data.as_slice()).map_err(|e| e.to_string())?;

    if let Ok(mut tree) = tree_res.tree.lock() {
        *tree = decoded;
        Ok(encode_tree_binary(env, &tree))
    } else {
        Err("failed to lock tree".to_string())
    }
}

/// Apply patches to an existing tree.
#[rustler::nif]
fn tree_patch(tree_res: ResourceArc<TreeResource>, data: Binary) -> Result<Atom, String> {
    let patches = tree::patch::decode_patches(data.as_slice()).map_err(|e| e.to_string())?;

    if let Ok(mut tree) = tree_res.tree.lock() {
        tree::patch::apply_patches(&mut tree, patches)?;
        Ok(atoms::ok())
    } else {
        Err("failed to lock tree".to_string())
    }
}

#[rustler::nif]
fn tree_patch_roundtrip<'a>(
    env: Env<'a>,
    tree_res: ResourceArc<TreeResource>,
    data: Binary,
) -> Result<Binary<'a>, String> {
    let patches = tree::patch::decode_patches(data.as_slice()).map_err(|e| e.to_string())?;

    if let Ok(mut tree) = tree_res.tree.lock() {
        tree::patch::apply_patches(&mut tree, patches)?;
        Ok(encode_tree_binary(env, &tree))
    } else {
        Err("failed to lock tree".to_string())
    }
}

/// Get the number of nodes in the tree.
#[rustler::nif]
fn tree_node_count(tree_res: ResourceArc<TreeResource>) -> usize {
    if let Ok(tree) = tree_res.tree.lock() {
        tree.len()
    } else {
        0
    }
}

/// Check if the tree is empty.
#[rustler::nif]
fn tree_is_empty(tree_res: ResourceArc<TreeResource>) -> bool {
    if let Ok(tree) = tree_res.tree.lock() {
        tree.is_empty()
    } else {
        true
    }
}

/// Clear the tree.
#[rustler::nif]
fn tree_clear(tree_res: ResourceArc<TreeResource>) -> Atom {
    if let Ok(mut tree) = tree_res.tree.lock() {
        tree.clear();
    }
    atoms::ok()
}

/// Compute layout for the tree with the given constraints and scale factor.
/// Returns list of {id_bytes, x, y, width, height} tuples for all elements.
/// Scale is applied to all pixel-based attributes (px sizes, padding, spacing, etc.)
#[rustler::nif]
fn tree_layout<'a>(
    env: Env<'a>,
    tree_res: ResourceArc<TreeResource>,
    width: f64,
    height: f64,
    scale: f64,
) -> Result<LayoutFrames<'a>, String> {
    if let Ok(mut tree) = tree_res.tree.lock() {
        let constraint = tree::layout::Constraint::new(width as f32, height as f32);
        tree::layout::layout_tree_default(&mut tree, constraint, scale as f32);

        // Collect all frames
        let mut frames = Vec::with_capacity(tree.len());
        for (id, element) in tree.nodes.iter() {
            if let Some(frame) = element.frame {
                let mut id_binary = NewBinary::new(env, id.0.len());
                id_binary.as_mut_slice().copy_from_slice(&id.0);
                frames.push((
                    id_binary.into(),
                    frame.x,
                    frame.y,
                    frame.width,
                    frame.height,
                ));
            }
        }
        Ok(frames)
    } else {
        Err("failed to lock tree".to_string())
    }
}

/// Round-trip EMRG binary: decode in Rust and re-encode.
#[rustler::nif]
fn tree_roundtrip<'a>(env: Env<'a>, data: Binary) -> Result<Binary<'a>, String> {
    let tree = tree::deserialize::decode_tree(data.as_slice()).map_err(|e| e.to_string())?;
    Ok(encode_tree_binary(env, &tree))
}

type HoverMsg<'a> = (Binary<'a>, bool);
type HoverMsgList<'a> = Vec<HoverMsg<'a>>;

#[rustler::nif]
fn test_harness_new(
    width: u32,
    height: u32,
) -> Result<ResourceArc<TestHarnessResource>, String> {
    let (tree_tx, tree_rx_proxy) = bounded(512);
    let (tree_actor_tx, tree_actor_rx) = bounded(512);
    let (tree_tap_tx, tree_tap_rx) = bounded(4096);
    let (event_tx, event_rx) = bounded(4096);
    let (render_tx, render_rx) = bounded(8);
    let render_sender = RenderSender {
        tx: render_tx,
        drop_rx: render_rx.clone(),
        log_render: false,
    };
    let render_counter = Arc::new(AtomicU64::new(0));

    assets::start(tree_tx.clone(), false);

    let proxy_handle = thread::spawn(move || {
        while let Ok(msg) = tree_rx_proxy.recv() {
            let is_stop = matches!(msg, TreeMsg::Stop);
            let _ = tree_tap_tx.send(msg.clone());
            if tree_actor_tx.send(msg).is_err() || is_stop {
                break;
            }
        }
    });

    let event_handle = spawn_event_actor(event_rx, tree_tx.clone(), false, false);
    let tree_handle = spawn_tree_actor_with_initial_tree(
        tree_actor_rx,
        TreeActorConfig {
            render_sender,
            event_tx: event_tx.clone(),
            render_counter,
            log_input: false,
            wayland_proxy: None,
            initial_width: width,
            initial_height: height,
        },
        ElementTree::new(),
    );

    Ok(ResourceArc::new(TestHarnessResource {
        tree_tx,
        event_tx,
        render_rx,
        tree_tap_rx,
        base_instant: Mutex::new(Instant::now()),
        handles: Mutex::new(Some(TestHarnessHandles {
            proxy_handle,
            tree_handle,
            event_handle,
        })),
    }))
}

#[rustler::nif]
fn test_harness_upload(
    harness: ResourceArc<TestHarnessResource>,
    data: Binary,
) -> Result<Atom, String> {
    send_tree(
        &harness.tree_tx,
        TreeMsg::UploadTree {
            bytes: data.as_slice().to_vec(),
        },
        false,
    );
    Ok(atoms::ok())
}

#[rustler::nif]
fn test_harness_patch(
    harness: ResourceArc<TestHarnessResource>,
    data: Binary,
) -> Result<Atom, String> {
    send_tree(
        &harness.tree_tx,
        TreeMsg::PatchTree {
            bytes: data.as_slice().to_vec(),
        },
        false,
    );
    Ok(atoms::ok())
}

#[rustler::nif]
fn test_harness_cursor_pos(
    harness: ResourceArc<TestHarnessResource>,
    x: f64,
    y: f64,
) -> Result<Atom, String> {
    send_event(
        &harness.event_tx,
        EventMsg::InputEvent(input::InputEvent::CursorPos {
            x: x as f32,
            y: y as f32,
        }),
        false,
    );
    Ok(atoms::ok())
}

#[rustler::nif]
fn test_harness_animation_pulse(
    harness: ResourceArc<TestHarnessResource>,
    presented_ms: u64,
    predicted_ms: u64,
) -> Result<Atom, String> {
    let base_instant = *harness
        .base_instant
        .lock()
        .map_err(|_| "failed to lock test harness clock".to_string())?;
    send_tree(
        &harness.tree_tx,
        TreeMsg::AnimationPulse {
            presented_at: base_instant + Duration::from_millis(presented_ms),
            predicted_next_present_at: base_instant + Duration::from_millis(predicted_ms),
        },
        false,
    );
    Ok(atoms::ok())
}

#[rustler::nif]
fn test_harness_reset_clock(harness: ResourceArc<TestHarnessResource>) -> Atom {
    if let Ok(mut base_instant) = harness.base_instant.lock() {
        *base_instant = Instant::now();
    }
    atoms::ok()
}

#[rustler::nif(schedule = "DirtyIo")]
fn test_harness_await_render(
    harness: ResourceArc<TestHarnessResource>,
    timeout_ms: u64,
) -> Result<Atom, String> {
    let timeout = Duration::from_millis(timeout_ms);

    match harness.render_rx.recv_timeout(timeout) {
        Ok(_) => {}
        Err(RecvTimeoutError::Timeout) => return Err("render timeout".to_string()),
        Err(RecvTimeoutError::Disconnected) => return Err("render channel disconnected".to_string()),
    }

    while harness
        .render_rx
        .recv_timeout(Duration::from_millis(10))
        .is_ok()
    {}

    Ok(atoms::ok())
}

#[rustler::nif(schedule = "DirtyIo")]
fn test_harness_drain_mouse_over_msgs<'a>(
    env: Env<'a>,
    harness: ResourceArc<TestHarnessResource>,
    timeout_ms: u64,
) -> HoverMsgList<'a> {
    let timeout = Duration::from_millis(timeout_ms);
    let mut flat = Vec::new();

    if let Ok(msg) = harness.tree_tap_rx.recv_timeout(timeout) {
        push_tree_message_flat(msg, &mut flat);
        while let Ok(msg) = harness.tree_tap_rx.recv_timeout(Duration::from_millis(10)) {
            push_tree_message_flat(msg, &mut flat);
        }
    }

    flat.into_iter()
        .filter_map(|msg| match msg {
            TreeMsg::SetMouseOverActive { element_id, active } => {
                Some(encode_hover_msg(env, &element_id, active))
            }
            _ => None,
        })
        .collect()
}

#[rustler::nif(schedule = "DirtyIo")]
fn test_harness_stop(harness: ResourceArc<TestHarnessResource>) -> Atom {
    harness.stop_inner();
    atoms::ok()
}

fn encode_hover_msg<'a>(env: Env<'a>, element_id: &ElementId, active: bool) -> HoverMsg<'a> {
    let mut id_binary = NewBinary::new(env, element_id.0.len());
    id_binary.as_mut_slice().copy_from_slice(&element_id.0);
    (id_binary.into(), active)
}

fn encode_tree_binary<'a>(env: Env<'a>, tree: &ElementTree) -> Binary<'a> {
    let encoded = tree::serialize::encode_tree(tree);
    let mut binary = NewBinary::new(env, encoded.len());
    binary.as_mut_slice().copy_from_slice(&encoded);
    binary.into()
}

fn load(env: Env, _info: Term) -> bool {
    env.register::<RendererResource>().is_ok()
        && env.register::<TreeResource>().is_ok()
        && env.register::<TestHarnessResource>().is_ok()
        && env.register::<VideoTargetResource>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::RegistryRebuildPayload;
    use crate::events::test_support::AnimatedNearbyHitCase;
    use crate::input::InputEvent;
    use crate::tree::element::ElementId;
    use crossbeam_channel::RecvTimeoutError;

    struct LiveActorHarness {
        tree_tx: Sender<TreeMsg>,
        event_tx: Sender<EventMsg>,
        render_rx: Receiver<RenderMsg>,
        tree_tap_rx: Receiver<TreeMsg>,
        proxy_handle: thread::JoinHandle<()>,
        tree_handle: thread::JoinHandle<()>,
        event_handle: thread::JoinHandle<()>,
    }

    impl LiveActorHarness {
        fn new(width: u32, height: u32, initial_tree: ElementTree) -> Self {
            let (tree_tx, tree_rx_proxy) = bounded(512);
            let (tree_actor_tx, tree_actor_rx) = bounded(512);
            let (tree_tap_tx, tree_tap_rx) = bounded(4096);
            let (event_tx, event_rx) = bounded(4096);
            let (render_tx, render_rx) = bounded(8);
            let render_sender = RenderSender {
                tx: render_tx,
                drop_rx: render_rx.clone(),
                log_render: false,
            };
            let render_counter = Arc::new(AtomicU64::new(0));

            assets::start(tree_tx.clone(), false);

            let proxy_handle = thread::spawn(move || {
                while let Ok(msg) = tree_rx_proxy.recv() {
                    let is_stop = matches!(msg, TreeMsg::Stop);
                    let _ = tree_tap_tx.send(msg.clone());
                    if tree_actor_tx.send(msg).is_err() || is_stop {
                        break;
                    }
                }
            });

            let event_handle = spawn_event_actor(event_rx, tree_tx.clone(), false, false);
            let tree_handle = spawn_tree_actor_with_initial_tree(
                tree_actor_rx,
                TreeActorConfig {
                    render_sender,
                    event_tx: event_tx.clone(),
                    render_counter,
            log_input: false,
                    wayland_proxy: None,
                    initial_width: width,
                    initial_height: height,
                },
                initial_tree,
            );

            Self {
                tree_tx,
                event_tx,
                render_rx,
                tree_tap_rx,
                proxy_handle,
                tree_handle,
                event_handle,
            }
        }

        fn send_tree(&self, msg: TreeMsg) {
            super::send_tree(&self.tree_tx, msg, false);
        }

        fn send_input(&self, event: crate::input::InputEvent) {
            super::send_event(&self.event_tx, EventMsg::InputEvent(event), false);
        }

        fn wait_for_render_settle(&self) {
            match self.render_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => panic!("expected render message"),
                Err(RecvTimeoutError::Disconnected) => panic!("render channel disconnected"),
            }

            while self.render_rx.recv_timeout(Duration::from_millis(15)).is_ok() {}
        }

        fn drain_set_mouse_over_active(&self, element_id: &ElementId) -> Vec<bool> {
            let mut msgs = Vec::new();
            while let Ok(msg) = self.tree_tap_rx.try_recv() {
                push_tree_message_flat(msg, &mut msgs);
            }

            msgs.into_iter()
                .filter_map(|msg| match msg {
                    TreeMsg::SetMouseOverActive {
                        element_id: id,
                        active,
                    } if &id == element_id => Some(active),
                    _ => None,
                })
                .collect()
        }

        fn stop(self) {
            super::send_event(&self.event_tx, EventMsg::Stop, false);
            super::send_tree(&self.tree_tx, TreeMsg::Stop, false);
            let _ = self.proxy_handle.join();
            let _ = self.event_handle.join();
            let _ = self.tree_handle.join();
            assets::stop();
        }
    }

    struct SpawnedEventActorHarness {
        event_tx: Sender<EventMsg>,
        tree_rx: Receiver<TreeMsg>,
        handle: thread::JoinHandle<()>,
    }

    impl SpawnedEventActorHarness {
        fn new() -> Self {
            let (event_tx, event_rx) = bounded(4096);
            let (tree_tx, tree_rx) = bounded(4096);
            let handle = spawn_event_actor(event_rx, tree_tx, false, false);

            Self {
                event_tx,
                tree_rx,
                handle,
            }
        }

        fn send_input(&self, event: InputEvent) {
            super::send_event(&self.event_tx, EventMsg::InputEvent(event), false);
        }

        fn send_rebuild(&self, rebuild: RegistryRebuildPayload) {
            super::send_event(&self.event_tx, EventMsg::RegistryUpdate { rebuild }, false);
        }

        fn wait_for_tree_msgs_quiet(&self) -> Vec<TreeMsg> {
            collect_tree_messages_until_quiet(&self.tree_rx)
        }

        fn stop(self) {
            super::send_event(&self.event_tx, EventMsg::Stop, false);
            let _ = self.handle.join();
        }
    }

    fn collect_tree_messages_until_quiet(rx: &Receiver<TreeMsg>) -> Vec<TreeMsg> {
        let mut out = Vec::new();

        if let Ok(msg) = rx.recv_timeout(Duration::from_millis(50)) {
            push_tree_message_flat(msg, &mut out);
            while let Ok(msg) = rx.recv_timeout(Duration::from_millis(10)) {
                push_tree_message_flat(msg, &mut out);
            }
        }

        out
    }

    #[test]
    fn spawned_event_actor_harness_activates_hover_on_first_target_sample() {
        let case = AnimatedNearbyHitCase::width_move_in_front();
        let probe = case.probe("newly_occupied_outside_host");
        let harness = SpawnedEventActorHarness::new();

        harness.send_rebuild(case.rebuild_at(0, false));
        let _ = harness.wait_for_tree_msgs_quiet();

        harness.send_input(InputEvent::CursorPos {
            x: probe.point.0,
            y: probe.point.1,
        });
        assert!(harness.wait_for_tree_msgs_quiet().is_empty());

        harness.send_rebuild(case.rebuild_at(500, false));
        let msgs = harness.wait_for_tree_msgs_quiet();

        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseOverActive { element_id, active }
                if *element_id == case.target_id && *active
        )));

        harness.stop();
    }

    #[test]
    fn live_actor_harness_static_cursor_activates_on_first_target_sample() {
        let case = AnimatedNearbyHitCase::width_move_in_front();
        let probe = case.probe("newly_occupied_outside_host");
        let first_target_sample = case
            .first_target_sample_ms(probe.label)
            .expect("probe should eventually hit target");
        let base = Instant::now();
        let harness = LiveActorHarness::new(
            case.constraint.max_width(0.0) as u32,
            case.constraint.max_height(0.0) as u32,
            case.source_tree(false),
        );

        harness.send_tree(TreeMsg::AnimationPulse {
            presented_at: base,
            predicted_next_present_at: base,
        });
        harness.wait_for_render_settle();
        let _ = harness.drain_set_mouse_over_active(&case.target_id);

        harness.send_input(input::InputEvent::CursorPos {
            x: probe.point.0,
            y: probe.point.1,
        });

        let mut activation_sample = None;

        for sample_ms in (50..=1000).step_by(50) {
            harness.send_tree(TreeMsg::AnimationPulse {
                presented_at: base + Duration::from_millis(sample_ms),
                predicted_next_present_at: base + Duration::from_millis(sample_ms),
            });
            harness.wait_for_render_settle();

            let activations = harness.drain_set_mouse_over_active(&case.target_id);
            if activation_sample.is_none() && activations.into_iter().any(|active| active) {
                activation_sample = Some(sample_ms);
            }
        }

        harness.stop();

        assert_eq!(activation_sample, Some(first_target_sample));
    }

    #[test]
    fn live_actor_harness_render_driven_pulses_activate_hover_without_tree_quiet_waits() {
        let case = AnimatedNearbyHitCase::width_move_in_front();
        let probe = case.probe("newly_occupied_outside_host");
        let base = Instant::now();
        let harness = LiveActorHarness::new(
            case.constraint.max_width(0.0) as u32,
            case.constraint.max_height(0.0) as u32,
            case.source_tree(false),
        );

        harness.send_tree(TreeMsg::AnimationPulse {
            presented_at: base,
            predicted_next_present_at: base,
        });
        match harness.render_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => panic!("expected initial render"),
            Err(RecvTimeoutError::Disconnected) => panic!("render channel disconnected"),
        }

        harness.send_input(input::InputEvent::CursorPos {
            x: probe.point.0,
            y: probe.point.1,
        });

        let mut saw_activation = false;

        for sample_ms in (50..=1400).step_by(50) {
            harness.send_tree(TreeMsg::AnimationPulse {
                presented_at: base + Duration::from_millis(sample_ms),
                predicted_next_present_at: base + Duration::from_millis(sample_ms),
            });

            match harness.render_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => panic!("expected render for sample {sample_ms}"),
                Err(RecvTimeoutError::Disconnected) => panic!("render channel disconnected"),
            }

            saw_activation |= harness
                .drain_set_mouse_over_active(&case.target_id)
                .into_iter()
                .any(|active| active);
        }

        saw_activation |= collect_tree_messages_until_quiet(&harness.tree_tap_rx)
            .into_iter()
            .any(|msg| matches!(
                msg,
                TreeMsg::SetMouseOverActive { element_id, active }
                    if element_id == case.target_id && active
            ));

        harness.stop();

        assert!(saw_activation);
    }

}

rustler::init!("Elixir.EmergeSkia.Native", load = load);
