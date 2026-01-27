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
    time::Instant,
};

use rustler::{Atom, Binary, Env, LocalPid, NewBinary, NifResult, ResourceArc, Term};
use skia_safe::Font;

mod backend;
mod cursor;
mod drm_input;
mod input;
mod events;
mod renderer;
mod tree;

use backend::drm;
use backend::raster::{RasterBackend, RasterConfig};
use backend::wayland::{self, UserEvent, WaylandConfig};
use cursor::CursorState;
use events::EventProcessor;
use tree::layout::layout_and_refresh_default;
use input::InputHandler;
use renderer::{DrawCmd, RenderState, get_default_typeface, load_font};
use tree::element::ElementTree;

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
    render_state: Arc<Mutex<RenderState>>,
    running_flag: Arc<AtomicBool>,
    backend: BackendKind,
    event_proxy: Mutex<Option<winit::event_loop::EventLoopProxy<UserEvent>>>,
    dirty_flag: Option<Arc<AtomicBool>>,
    stop_flag: Option<Arc<AtomicBool>>,
    render_counter: Arc<AtomicU64>,
    log_render: bool,
    input_handler: Arc<Mutex<InputHandler>>,
    tree: Arc<Mutex<ElementTree>>,
    event_processor: Arc<Mutex<EventProcessor>>,
    input_target: Arc<Mutex<Option<LocalPid>>>,
}

/// Resource for holding an element tree (for layout/rendering).
struct TreeResource {
    tree: Mutex<ElementTree>,
}

impl RendererResource {
    fn request_redraw(&self) {
        match self.backend {
            BackendKind::Wayland => {
                if let Ok(guard) = self.event_proxy.lock()
                    && let Some(proxy) = guard.as_ref()
                {
                    let _ = proxy.send_event(UserEvent::Redraw);
                }
            }
            BackendKind::Drm => {
                if let Some(dirty) = &self.dirty_flag {
                    dirty.store(true, Ordering::Relaxed);
                }
            }
        }
    }

    fn stop(&self) {
        match self.backend {
            BackendKind::Wayland => {
                if let Ok(guard) = self.event_proxy.lock()
                    && let Some(proxy) = guard.as_ref()
                {
                    let _ = proxy.send_event(UserEvent::Stop);
                }
            }
            BackendKind::Drm => {
                if let Some(stop_flag) = &self.stop_flag {
                    stop_flag.store(true, Ordering::Relaxed);
                }
                self.running_flag.store(false, Ordering::Relaxed);
            }
        }
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

fn start_with_config(config: StartConfig) -> NifResult<ResourceArc<RendererResource>> {
    let render_state = Arc::new(Mutex::new(RenderState::default()));
    let running_flag = Arc::new(AtomicBool::new(true));
    let render_counter = Arc::new(AtomicU64::new(0));
    let input_handler = Arc::new(Mutex::new(InputHandler::new()));
    let tree = Arc::new(Mutex::new(ElementTree::new()));
    let event_processor = Arc::new(Mutex::new(EventProcessor::new()));

    let input_target = Arc::new(Mutex::new(None));

    let log_input = matches!(config.backend, BackendKind::Drm) && config.drm_input_log;
    let log_render = config.render_log;
    let (backend, event_proxy, dirty_flag, stop_flag) = match config.backend {
        BackendKind::Wayland => {
            let render_state_clone = Arc::clone(&render_state);
            let running_flag_clone = Arc::clone(&running_flag);
            let input_handler_clone = Arc::clone(&input_handler);
            let event_processor_for_thread = Arc::clone(&event_processor);

            let (proxy_tx, proxy_rx) = mpsc::channel();
            let config = WaylandConfig {
                title: config.title,
                width: config.width,
                height: config.height,
            };
            thread::spawn(move || {
                wayland::run(
                    config,
                    render_state_clone,
                    running_flag_clone,
                    input_handler_clone,
                    event_processor_for_thread,
                    proxy_tx,
                );
            });

            let event_proxy = proxy_rx
                .recv()
                .map_err(|_| rustler::Error::Term(Box::new("failed to receive event proxy")))?;
            (BackendKind::Wayland, Some(event_proxy), None, None)
        }
        BackendKind::Drm => {
            let stop_flag = Arc::new(AtomicBool::new(false));
            let dirty_flag = Arc::new(AtomicBool::new(false));
            let cursor_state = Arc::new(Mutex::new(CursorState::new()));
            let render_state_clone = Arc::clone(&render_state);
            let running_flag_clone = Arc::clone(&running_flag);
            let render_counter_clone = Arc::clone(&render_counter);
            let input_handler_clone = Arc::clone(&input_handler);
            let event_processor_clone = Arc::clone(&event_processor);
            let stop_for_thread = Arc::clone(&stop_flag);
            let dirty_for_thread = Arc::clone(&dirty_flag);
            let drm_config = drm::DrmRunConfig {
                requested_size: Some((config.width, config.height)),
                cursor_state: Arc::clone(&cursor_state),
                card_path: config.drm_card,
                hw_cursor: config.drm_hw_cursor,
                input_log: config.drm_input_log,
                render_log: config.render_log,
            };

            thread::spawn(move || {
                drm::run(
                    stop_for_thread,
                    dirty_for_thread,
                    render_state_clone,
                    input_handler_clone,
                    event_processor_clone,
                    running_flag_clone,
                    render_counter_clone,
                    drm_config,
                );
            });

            (
                BackendKind::Drm,
                None,
                Some(Arc::clone(&dirty_flag)),
                Some(Arc::clone(&stop_flag)),
            )
        }
    };

    let resource = RendererResource {
        render_state,
        running_flag,
        backend,
        event_proxy: Mutex::new(event_proxy),
        dirty_flag: dirty_flag.clone(),
        stop_flag: stop_flag.clone(),
        render_counter,
        log_render,
        input_handler,
        tree,
        event_processor: Arc::clone(&event_processor),
        input_target: Arc::clone(&input_target),
    };

    let redraw: Arc<dyn Fn() + Send + Sync> = match backend {
        BackendKind::Wayland => {
            let event_proxy = resource.event_proxy.lock().ok().and_then(|p| p.as_ref().cloned());
            Arc::new(move || {
                if let Some(proxy) = event_proxy.as_ref() {
                    let _ = proxy.send_event(UserEvent::Redraw);
                }
            })
        }
        BackendKind::Drm => {
            let dirty_flag = dirty_flag.clone();
            Arc::new(move || {
                if let Some(dirty) = dirty_flag.as_ref() {
                    dirty.store(true, Ordering::Relaxed);
                }
            })
        }
    };

    EventProcessor::start_loop(
        Arc::clone(&event_processor),
        Arc::clone(&resource.tree),
        Arc::clone(&resource.render_state),
        Arc::clone(&resource.input_target),
        redraw,
        log_input,
    );

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
    if let Ok(mut state) = renderer.render_state.lock() {
        let version = renderer.render_counter.fetch_add(1, Ordering::Relaxed) + 1;
        state.commands = commands;
        state.render_version = version;
        if renderer.log_render {
            eprintln!("renderer_render version={version}");
        }
    }
    renderer.request_redraw();
    atoms::ok()
}

// ============================================================================
// Tree -> Layout -> Render Pipeline
// ============================================================================

#[rustler::nif]
fn renderer_upload(
    renderer: ResourceArc<RendererResource>,
    data: Binary,
    width: f64,
    height: f64,
    scale: f64,
) -> Result<Atom, String> {
    let log_render = renderer.log_render;
    let start = if log_render { Some(Instant::now()) } else { None };
    let decoded = tree::deserialize::decode_tree(data.as_slice()).map_err(|e| e.to_string())?;

    if let Ok(mut tree) = renderer.tree.lock() {
        *tree = decoded;
        let constraint = tree::layout::Constraint::new(width as f32, height as f32);
        let output = layout_and_refresh_default(&mut tree, constraint, scale as f32);

        if let Ok(mut processor) = renderer.event_processor.lock() {
            processor.rebuild_registry(output.event_registry);
        }
        if let Ok(mut state) = renderer.render_state.lock() {
            let version = renderer.render_counter.fetch_add(1, Ordering::Relaxed) + 1;
            state.commands = output.commands;
            state.render_version = version;
            if log_render {
                let elapsed = start.map(|t| t.elapsed()).unwrap_or_default();
                eprintln!(
                    "renderer_upload version={version} elapsed_ms={}",
                    elapsed.as_secs_f64() * 1000.0
                );
            }
        }
        renderer.request_redraw();
        Ok(atoms::ok())
    } else {
        Err("failed to lock tree".to_string())
    }
}

#[rustler::nif]
fn renderer_patch(
    renderer: ResourceArc<RendererResource>,
    data: Binary,
    width: f64,
    height: f64,
    scale: f64,
) -> Result<Atom, String> {
    let log_render = renderer.log_render;
    let start = if log_render { Some(Instant::now()) } else { None };
    let patches = tree::patch::decode_patches(data.as_slice()).map_err(|e| e.to_string())?;

    if let Ok(mut tree) = renderer.tree.lock() {
        tree::patch::apply_patches(&mut tree, patches)?;
        let constraint = tree::layout::Constraint::new(width as f32, height as f32);
        let output = layout_and_refresh_default(&mut tree, constraint, scale as f32);

        if let Ok(mut processor) = renderer.event_processor.lock() {
            processor.rebuild_registry(output.event_registry);
        }
        if let Ok(mut state) = renderer.render_state.lock() {
            let version = renderer.render_counter.fetch_add(1, Ordering::Relaxed) + 1;
            state.commands = output.commands;
            state.render_version = version;
            if log_render {
                let elapsed = start.map(|t| t.elapsed()).unwrap_or_default();
                eprintln!(
                    "renderer_patch version={version} elapsed_ms={}",
                    elapsed.as_secs_f64() * 1000.0
                );
            }
        }
        renderer.request_redraw();
        Ok(atoms::ok())
    } else {
        Err("failed to lock tree".to_string())
    }
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
    if let Ok(mut handler) = renderer.input_handler.lock() {
        handler.set_mask(mask);
    }
    atoms::ok()
}

/// Set the target process to receive input events.
///
/// Input events are sent directly to the target process as
/// `{:emerge_skia_event, event}` messages.
#[rustler::nif]
fn set_input_target(renderer: ResourceArc<RendererResource>, pid: Option<LocalPid>) -> Atom {
    if let Ok(mut target) = renderer.input_target.lock() {
        *target = pid;
    }
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
    let mut backend = RasterBackend::new(&config)
        .map_err(|e| rustler::Error::Term(Box::new(e)))?;

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
    let decoded = tree::deserialize::decode_tree(data.as_slice())
        .map_err(|e| e.to_string())?;

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
    let patches = tree::patch::decode_patches(data.as_slice())
        .map_err(|e| e.to_string())?;

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
