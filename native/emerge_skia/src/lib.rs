//! EmergeSkia NIF - Minimal Skia renderer for Elixir.
//!
//! This crate provides a Rustler NIF that exposes Skia rendering to Elixir
//! through a simple command-based API.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};

use rustler::{Atom, Binary, Env, LocalPid, NewBinary, NifResult, ResourceArc, Term};
use skia_safe::Font;

mod actors;
mod assets;
mod backend;
mod clipboard;
mod cursor;
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
use renderer::{DrawCmd, RenderState, get_default_typeface, load_font, set_render_log_enabled};
use tree::element::ElementTree;
use tree::layout::layout_and_refresh_default;
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

struct RendererResource {
    running_flag: Arc<AtomicBool>,
    backend: BackendKind,
    event_proxy: Arc<Mutex<Option<winit::event_loop::EventLoopProxy<UserEvent>>>>,
    stop_flag: Arc<AtomicBool>,
    tree_tx: Sender<TreeMsg>,
    event_tx: Sender<EventMsg>,
    render_tx: RenderSender,
    cursor_tx: Option<Sender<RenderMsg>>,
    render_counter: Arc<AtomicU64>,
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

impl rustler::Resource for RendererResource {}

impl rustler::Resource for TreeResource {}

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

        let mut tree = ElementTree::new();
        let mut width = (initial_width as f32).max(1.0);
        let mut height = (initial_height as f32).max(1.0);
        let mut scale = 1.0f32;
        let mut cached_rebuild: Option<events::RegistryRebuildPayload> = None;

        while let Ok(msg) = tree_rx.recv() {
            let mut messages = Vec::new();
            push_tree_message_flat(msg, &mut messages);
            while let Ok(next) = tree_rx.try_recv() {
                push_tree_message_flat(next, &mut messages);
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

            for message in messages {
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
                        tree_changed |= tree.set_text_input_content(&element_id, content);
                    }
                    TreeMsg::SetTextInputRuntime {
                        element_id,
                        focused,
                        cursor,
                        selection_anchor,
                        preedit,
                        preedit_cursor,
                    } => {
                        tree_changed |= tree.set_text_input_runtime(
                            &element_id,
                            focused,
                            cursor,
                            selection_anchor,
                            preedit,
                            preedit_cursor,
                        );
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
                tree_changed |= tree.apply_scroll(&id, dx, dy);
            }

            for (id, dx) in thumb_drag_x_acc {
                tree_changed |= tree.apply_scroll_x(&id, dx);
            }

            for (id, dy) in thumb_drag_y_acc {
                tree_changed |= tree.apply_scroll_y(&id, dy);
            }

            for (id, hovered) in hover_x_state {
                tree_changed |= tree.set_scrollbar_x_hover(&id, hovered);
            }

            for (id, hovered) in hover_y_state {
                tree_changed |= tree.set_scrollbar_y_hover(&id, hovered);
            }

            for (id, active) in mouse_over_active_state {
                tree_changed |= tree.set_mouse_over_active(&id, active);
            }

            for (id, active) in mouse_down_active_state {
                tree_changed |= tree.set_mouse_down_active(&id, active);
            }

            for (id, active) in focused_active_state {
                tree_changed |= tree.set_focused_active(&id, active);
            }

            match decide_refresh_action(tree_changed, registry_requested, cached_rebuild.is_some())
            {
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
                    let output = layout_and_refresh_default(&mut tree, constraint, scale);
                    cached_rebuild = Some(output.event_rebuild.clone());
                    send_event(
                        &event_tx,
                        EventMsg::RegistryUpdate {
                            rebuild: output.event_rebuild,
                        },
                        log_input,
                    );

                    let version = render_counter.fetch_add(1, Ordering::Relaxed) + 1;
                    let animate = assets::has_pending_assets();
                    render_sender.send_latest(RenderMsg::Commands {
                        commands: output.commands,
                        version,
                        animate,
                        ime_enabled: output.ime_enabled,
                        ime_cursor_area: output.ime_cursor_area,
                    });

                    if let Some(proxy) = wayland_proxy.as_ref() {
                        let _ = proxy.send_event(UserEvent::Redraw);
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
        render_counter,
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

#[rustler::nif]
fn render(renderer: ResourceArc<RendererResource>, commands: Vec<DrawCmd>) -> Atom {
    let version = renderer.render_counter.fetch_add(1, Ordering::Relaxed) + 1;
    renderer.render_tx.send_latest(RenderMsg::Commands {
        commands,
        version,
        animate: false,
        ime_enabled: false,
        ime_cursor_area: None,
    });
    if renderer.backend == BackendKind::Wayland
        && let Ok(guard) = renderer.event_proxy.lock()
        && let Some(proxy) = guard.as_ref()
    {
        let _ = proxy.send_event(UserEvent::Redraw);
    }
    atoms::ok()
}

// ============================================================================
// Tree -> Layout -> Render Pipeline
// ============================================================================

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

/// Render commands to an RGBA pixel buffer (synchronous, no window).
///
/// This is useful for testing, headless rendering, and image generation.
/// Each call creates a fresh CPU surface, renders, and returns the pixels.
#[rustler::nif]
fn render_to_pixels(
    env: Env,
    width: u32,
    height: u32,
    commands: Vec<DrawCmd>,
) -> NifResult<Binary> {
    let config = RasterConfig { width, height };
    let mut backend = RasterBackend::new(&config).map_err(|e| rustler::Error::Term(Box::new(e)))?;

    let state = RenderState {
        commands,
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

fn encode_tree_binary<'a>(env: Env<'a>, tree: &ElementTree) -> Binary<'a> {
    let encoded = tree::serialize::encode_tree(tree);
    let mut binary = NewBinary::new(env, encoded.len());
    binary.as_mut_slice().copy_from_slice(&encoded);
    binary.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_tree_message_flat_expands_nested_batches_in_order() {
        let mut out = Vec::new();
        push_tree_message_flat(
            TreeMsg::Batch(vec![
                TreeMsg::RebuildRegistry,
                TreeMsg::Batch(vec![
                    TreeMsg::AssetStateChanged,
                    TreeMsg::Resize {
                        width: 10.0,
                        height: 20.0,
                        scale: 1.0,
                    },
                ]),
            ]),
            &mut out,
        );

        assert!(matches!(out[0], TreeMsg::RebuildRegistry));
        assert!(matches!(out[1], TreeMsg::AssetStateChanged));
        assert!(matches!(
            out[2],
            TreeMsg::Resize {
                width: 10.0,
                height: 20.0,
                scale: 1.0,
            }
        ));
    }

    #[test]
    fn decide_refresh_action_prefers_cache_for_registry_only_requests() {
        assert_eq!(
            decide_refresh_action(false, false, false),
            RefreshDecision::Skip
        );
        assert_eq!(
            decide_refresh_action(false, true, true),
            RefreshDecision::UseCachedRebuild
        );
        assert_eq!(
            decide_refresh_action(false, true, false),
            RefreshDecision::Recompute
        );
        assert_eq!(
            decide_refresh_action(true, false, true),
            RefreshDecision::Recompute
        );
        assert_eq!(
            decide_refresh_action(true, true, true),
            RefreshDecision::Recompute
        );
    }
}

// ============================================================================
// NIF Registration
// ============================================================================

fn load(env: Env, _info: Term) -> bool {
    env.register::<RendererResource>().is_ok()
        && env.register::<TreeResource>().is_ok()
        && env.register::<VideoTargetResource>().is_ok()
}

rustler::init!("Elixir.EmergeSkia.Native", load = load);
