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
mod cursor;
mod drm_input;
mod events;
mod input;
mod renderer;
mod tree;

use actors::{EventMsg, RenderMsg, TreeMsg};
use assets::AssetConfig;
use backend::drm;
use backend::raster::{RasterBackend, RasterConfig};
use backend::wayland::{self, UserEvent, WaylandConfig};
use drm_input::DrmInput;
use events::{EventProcessor, MouseOverRequest, ScrollbarHoverRequest, ScrollbarThumbDragRequest};
use input::{InputEvent, InputHandler};
use renderer::{DrawCmd, RenderState, get_default_typeface, load_font, set_render_log_enabled};
use tree::element::ElementTree;
use tree::layout::layout_and_refresh_default;

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
    event_proxy: Mutex<Option<winit::event_loop::EventLoopProxy<UserEvent>>>,
    stop_flag: Arc<AtomicBool>,
    tree_tx: Sender<TreeMsg>,
    event_tx: Sender<EventMsg>,
    render_tx: RenderSender,
    cursor_tx: Option<Sender<RenderMsg>>,
    render_counter: Arc<AtomicU64>,
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

#[allow(clippy::too_many_arguments)]
fn spawn_tree_actor(
    tree_rx: Receiver<TreeMsg>,
    render_sender: RenderSender,
    event_tx: Sender<EventMsg>,
    render_counter: Arc<AtomicU64>,
    log_input: bool,
    wayland_proxy: Option<winit::event_loop::EventLoopProxy<UserEvent>>,
    initial_width: u32,
    initial_height: u32,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut tree = ElementTree::new();
        let mut width = (initial_width as f32).max(1.0);
        let mut height = (initial_height as f32).max(1.0);
        let mut scale = 1.0f32;

        while let Ok(msg) = tree_rx.recv() {
            let mut messages = vec![msg];
            while let Ok(next) = tree_rx.try_recv() {
                messages.push(next);
            }

            let mut scroll_acc = std::collections::HashMap::new();
            let mut thumb_drag_x_acc = std::collections::HashMap::new();
            let mut thumb_drag_y_acc = std::collections::HashMap::new();
            let mut hover_x_state = std::collections::HashMap::new();
            let mut hover_y_state = std::collections::HashMap::new();
            let mut mouse_over_active_state = std::collections::HashMap::new();
            let mut changed = false;

            for message in messages {
                match message {
                    TreeMsg::Stop => return,
                    TreeMsg::UploadTree { bytes } => match tree::deserialize::decode_tree(&bytes) {
                        Ok(decoded) => {
                            tree = decoded;
                            changed = true;
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
                        changed = true;
                    }
                    TreeMsg::Resize {
                        width: w,
                        height: h,
                        scale: s,
                    } => {
                        width = w.max(1.0);
                        height = h.max(1.0);
                        scale = s;
                        changed = true;
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
                    TreeMsg::AssetStateChanged => {
                        changed = true;
                    }
                }
            }

            for (id, (dx, dy)) in scroll_acc {
                changed |= tree.apply_scroll(&id, dx, dy);
            }

            for (id, dx) in thumb_drag_x_acc {
                changed |= tree.apply_scroll_x(&id, dx);
            }

            for (id, dy) in thumb_drag_y_acc {
                changed |= tree.apply_scroll_y(&id, dy);
            }

            for (id, hovered) in hover_x_state {
                changed |= tree.set_scrollbar_x_hover(&id, hovered);
            }

            for (id, hovered) in hover_y_state {
                changed |= tree.set_scrollbar_y_hover(&id, hovered);
            }

            for (id, active) in mouse_over_active_state {
                changed |= tree.set_mouse_over_active(&id, active);
            }

            if !changed {
                continue;
            }

            assets::ensure_tree_sources(&tree);

            let constraint = tree::layout::Constraint::new(width, height);
            let output = layout_and_refresh_default(&mut tree, constraint, scale);
            send_event(
                &event_tx,
                EventMsg::RegistryUpdate {
                    registry: output.event_registry,
                },
                log_input,
            );

            let version = render_counter.fetch_add(1, Ordering::Relaxed) + 1;
            let animate = assets::has_pending_assets();
            render_sender.send_latest(RenderMsg::Commands {
                commands: output.commands,
                version,
                animate,
            });

            if let Some(proxy) = wayland_proxy.as_ref() {
                let _ = proxy.send_event(UserEvent::Redraw);
            }
        }
    })
}

fn process_input_events(
    events: &mut Vec<InputEvent>,
    processor: &mut EventProcessor,
    input_handler: &mut InputHandler,
    target: &Option<LocalPid>,
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
) {
    if events.is_empty() {
        return;
    }

    let mut coalesced = Vec::new();
    let mut last_cursor: Option<InputEvent> = None;
    let mut scroll_acc: Option<(f32, f32, f32, f32)> = None;

    for event in events.drain(..) {
        let event = event.normalize_scroll();
        match event {
            InputEvent::CursorPos { .. } => {
                last_cursor = Some(event);
            }
            InputEvent::CursorScroll { dx, dy, x, y } => {
                scroll_acc = Some(match scroll_acc {
                    Some((acc_dx, acc_dy, _, _)) => (acc_dx + dx, acc_dy + dy, x, y),
                    None => (dx, dy, x, y),
                });
            }
            other => coalesced.push(other),
        }
    }

    if let Some((dx, dy, x, y)) = scroll_acc {
        coalesced.push(InputEvent::CursorScroll { dx, dy, x, y });
    }
    if let Some(cursor) = last_cursor {
        coalesced.push(cursor);
    }

    for event in coalesced {
        if let InputEvent::Resized {
            width,
            height,
            scale_factor,
        } = &event
        {
            send_tree(
                tree_tx,
                TreeMsg::Resize {
                    width: *width as f32,
                    height: *height as f32,
                    scale: *scale_factor,
                },
                log_render,
            );
        }

        if !input_handler.accepts(&event) {
            continue;
        }

        if let InputEvent::CursorPos { x, y } = &event {
            input_handler.set_cursor_pos(*x, *y);
        }

        if let Some(pid) = target.as_ref() {
            let pid = *pid;
            events::send_input_event(pid, &event);

            if let Some(clicked_id) = processor.detect_click(&event) {
                events::send_element_event(pid, &clicked_id, events::click_atom());
            }

            if let Some((mouse_id, mouse_event)) = processor.detect_mouse_button_event(&event) {
                events::send_element_event(pid, &mouse_id, mouse_event);
            }

            for (hover_id, hover_event) in processor.handle_hover_event(&event) {
                events::send_element_event(pid, &hover_id, hover_event);
            }
        } else {
            processor.detect_click(&event);
            processor.detect_mouse_button_event(&event);
            processor.handle_hover_event(&event);
        }

        for request in processor.scrollbar_thumb_drag_requests(&event) {
            match request {
                ScrollbarThumbDragRequest::X { element_id, dx } => {
                    send_tree(
                        tree_tx,
                        TreeMsg::ScrollbarThumbDragX { element_id, dx },
                        log_render,
                    );
                }
                ScrollbarThumbDragRequest::Y { element_id, dy } => {
                    send_tree(
                        tree_tx,
                        TreeMsg::ScrollbarThumbDragY { element_id, dy },
                        log_render,
                    );
                }
            }
        }

        for (id, dx, dy) in processor.scroll_requests(&event) {
            send_tree(
                tree_tx,
                TreeMsg::ScrollRequest {
                    element_id: id,
                    dx,
                    dy,
                },
                log_render,
            );
        }

        for request in processor.scrollbar_hover_requests(&event) {
            match request {
                ScrollbarHoverRequest::X {
                    element_id,
                    hovered,
                } => {
                    send_tree(
                        tree_tx,
                        TreeMsg::SetScrollbarXHover {
                            element_id,
                            hovered,
                        },
                        log_render,
                    );
                }
                ScrollbarHoverRequest::Y {
                    element_id,
                    hovered,
                } => {
                    send_tree(
                        tree_tx,
                        TreeMsg::SetScrollbarYHover {
                            element_id,
                            hovered,
                        },
                        log_render,
                    );
                }
            }
        }

        for request in processor.mouse_over_requests(&event) {
            match request {
                MouseOverRequest::SetMouseOverActive { element_id, active } => {
                    send_tree(
                        tree_tx,
                        TreeMsg::SetMouseOverActive { element_id, active },
                        log_render,
                    );
                }
            }
        }
    }
}

fn spawn_event_actor(
    event_rx: Receiver<EventMsg>,
    tree_tx: Sender<TreeMsg>,
    log_render: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut processor = EventProcessor::new();
        let mut input_handler = InputHandler::new();
        let mut target: Option<LocalPid> = None;

        while let Ok(msg) = event_rx.recv() {
            let mut messages = vec![msg];
            while let Ok(next) = event_rx.try_recv() {
                messages.push(next);
            }

            let mut pending_inputs = Vec::new();

            for message in messages {
                match message {
                    EventMsg::InputEvent(event) => pending_inputs.push(event),
                    EventMsg::RegistryUpdate { registry } => {
                        process_input_events(
                            &mut pending_inputs,
                            &mut processor,
                            &mut input_handler,
                            &target,
                            &tree_tx,
                            log_render,
                        );
                        processor.rebuild_registry(registry);
                    }
                    EventMsg::SetInputMask(mask) => {
                        process_input_events(
                            &mut pending_inputs,
                            &mut processor,
                            &mut input_handler,
                            &target,
                            &tree_tx,
                            log_render,
                        );
                        input_handler.set_mask(mask);
                    }
                    EventMsg::SetInputTarget(pid) => {
                        process_input_events(
                            &mut pending_inputs,
                            &mut processor,
                            &mut input_handler,
                            &target,
                            &tree_tx,
                            log_render,
                        );
                        target = pid;
                    }
                    EventMsg::Stop => return,
                }
            }

            process_input_events(
                &mut pending_inputs,
                &mut processor,
                &mut input_handler,
                &target,
                &tree_tx,
                log_render,
            );
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

    let _event_handle = spawn_event_actor(event_rx, tree_tx.clone(), log_render);

    let initial_width = config.width;
    let initial_height = config.height;

    let (backend, event_proxy) = match config.backend {
        BackendKind::Wayland => {
            let (proxy_tx, proxy_rx) = mpsc::channel();
            let running_flag_clone = Arc::clone(&running_flag);
            let event_tx_clone = event_tx.clone();
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
                    proxy_tx,
                );
            });

            let proxy = proxy_rx
                .recv()
                .map_err(|_| rustler::Error::Term(Box::new("failed to receive event proxy")))?;

            spawn_tree_actor(
                tree_rx,
                render_sender.clone(),
                event_tx.clone(),
                Arc::clone(&render_counter),
                log_input,
                Some(proxy.clone()),
                initial_width,
                initial_height,
            );

            (BackendKind::Wayland, Some(proxy))
        }
        BackendKind::Drm => {
            let (screen_tx, screen_rx) = bounded(1);
            let event_tx_clone = event_tx.clone();
            let cursor_tx_clone = cursor_tx.clone();
            let stop_clone = Arc::clone(&stop_flag);
            let input_log = log_input;
            let drm_input_size = (initial_width, initial_height);

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
                    stop_for_thread,
                    running_flag_clone,
                    render_rx,
                    cursor_rx,
                    event_tx_clone,
                    screen_tx,
                    render_counter_clone,
                    drm_config,
                );
            });

            spawn_tree_actor(
                tree_rx,
                render_sender.clone(),
                event_tx.clone(),
                Arc::clone(&render_counter),
                log_input,
                None,
                initial_width,
                initial_height,
            );

            (BackendKind::Drm, None)
        }
    };

    let resource = RendererResource {
        running_flag,
        backend,
        event_proxy: Mutex::new(event_proxy),
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
#[allow(clippy::too_many_arguments)]
fn start_opts(
    backend: String,
    title: String,
    width: u32,
    height: u32,
    drm_card: Option<String>,
    hw_cursor: bool,
    input_log: bool,
    render_log: bool,
) -> NifResult<ResourceArc<RendererResource>> {
    let backend = backend.to_lowercase();
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
        title,
        width,
        height,
        drm_card,
        drm_hw_cursor: hw_cursor,
        drm_input_log: input_log,
        render_log,
    })
}

#[rustler::nif]
fn stop(renderer: ResourceArc<RendererResource>) -> Atom {
    renderer.stop();
    atoms::ok()
}

#[rustler::nif]
fn render(renderer: ResourceArc<RendererResource>, commands: Vec<DrawCmd>) -> Atom {
    let version = renderer.render_counter.fetch_add(1, Ordering::Relaxed) + 1;
    renderer.render_tx.send_latest(RenderMsg::Commands {
        commands,
        version,
        animate: false,
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
    manifest_path: String,
    runtime_enabled: bool,
    allowlist: Vec<String>,
    follow_symlinks: bool,
    max_file_size: u64,
    extensions: Vec<String>,
) -> Atom {
    assets::configure(AssetConfig {
        manifest_path,
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
/// - 0x02: Codepoint (text input) events
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

// ============================================================================
// NIF Registration
// ============================================================================

#[allow(non_local_definitions)]
fn load(env: Env, _info: Term) -> bool {
    let _ = rustler::resource!(RendererResource, env);
    let _ = rustler::resource!(TreeResource, env);
    true
}

rustler::init!("Elixir.EmergeSkia.Native", load = load);
