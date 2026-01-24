//! EmergeSkia NIF - Minimal Skia renderer for Elixir.
//!
//! This crate provides a Rustler NIF that exposes Skia rendering to Elixir
//! through a simple command-based API.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use rustler::{Atom, Binary, Env, LocalPid, NewBinary, NifResult, ResourceArc, Term};
use skia_safe::Font;

mod backend;
mod input;
mod renderer;
mod tree;

use backend::raster::{RasterBackend, RasterConfig};
use backend::wayland::{self, UserEvent, WaylandConfig};
use input::{InputHandler, build_click_registry};
use renderer::{DrawCmd, RenderState, get_default_typeface};
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

struct RendererResource {
    render_state: Arc<Mutex<RenderState>>,
    running_flag: Arc<AtomicBool>,
    event_proxy: Mutex<Option<winit::event_loop::EventLoopProxy<UserEvent>>>,
    input_handler: Arc<Mutex<InputHandler>>,
    tree: Mutex<ElementTree>,
}

/// Resource for holding an element tree (for layout/rendering).
struct TreeResource {
    tree: Mutex<ElementTree>,
}

impl RendererResource {
    fn request_redraw(&self) {
        if let Ok(guard) = self.event_proxy.lock()
            && let Some(proxy) = guard.as_ref()
        {
            let _ = proxy.send_event(UserEvent::Redraw);
        }
    }

    fn stop(&self) {
        if let Ok(guard) = self.event_proxy.lock()
            && let Some(proxy) = guard.as_ref()
        {
            let _ = proxy.send_event(UserEvent::Stop);
        }
    }
}


// ============================================================================
// NIF Functions
// ============================================================================

#[rustler::nif]
fn start(title: String, width: u32, height: u32) -> NifResult<ResourceArc<RendererResource>> {
    let render_state = Arc::new(Mutex::new(RenderState::default()));
    let running_flag = Arc::new(AtomicBool::new(true));
    let input_handler = Arc::new(Mutex::new(InputHandler::new()));

    let render_state_clone = Arc::clone(&render_state);
    let running_flag_clone = Arc::clone(&running_flag);
    let input_handler_clone = Arc::clone(&input_handler);

    let (proxy_tx, proxy_rx) = mpsc::channel();

    let config = WaylandConfig {
        title,
        width,
        height,
    };

    thread::spawn(move || {
        wayland::run(config, render_state_clone, running_flag_clone, input_handler_clone, proxy_tx);
    });

    let event_proxy = proxy_rx
        .recv()
        .map_err(|_| rustler::Error::Term(Box::new("failed to receive event proxy")))?;

    let resource = RendererResource {
        render_state,
        running_flag,
        event_proxy: Mutex::new(Some(event_proxy)),
        input_handler,
        tree: Mutex::new(ElementTree::new()),
    };

    Ok(ResourceArc::new(resource))
}

#[rustler::nif]
fn stop(renderer: ResourceArc<RendererResource>) -> Atom {
    renderer.stop();
    atoms::ok()
}

#[rustler::nif]
fn render(renderer: ResourceArc<RendererResource>, commands: Vec<DrawCmd>) -> Atom {
    if let Ok(mut state) = renderer.render_state.lock() {
        state.commands = commands;
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
    let decoded = tree::deserialize::decode_tree(data.as_slice()).map_err(|e| e.to_string())?;

    if let Ok(mut tree) = renderer.tree.lock() {
        *tree = decoded;
        let constraint = tree::layout::Constraint::new(width as f32, height as f32);
        tree::layout::layout_tree_default(&mut tree, constraint, scale as f32);
        if let Ok(mut handler) = renderer.input_handler.lock() {
            handler.set_event_registry(build_click_registry(&tree));
        }
        let commands = tree::render::render_tree(&tree);

        if let Ok(mut state) = renderer.render_state.lock() {
            state.commands = commands;
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
    let patches = tree::patch::decode_patches(data.as_slice()).map_err(|e| e.to_string())?;

    if let Ok(mut tree) = renderer.tree.lock() {
        tree::patch::apply_patches(&mut tree, patches)?;
        let constraint = tree::layout::Constraint::new(width as f32, height as f32);
        tree::layout::layout_tree_default(&mut tree, constraint, scale as f32);
        if let Ok(mut handler) = renderer.input_handler.lock() {
            handler.set_event_registry(build_click_registry(&tree));
        }
        let commands = tree::render::render_tree(&tree);

        if let Ok(mut state) = renderer.render_state.lock() {
            state.commands = commands;
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
    let font = Font::new(typeface, font_size);

    let (width, _bounds) = font.measure_str(&text, None);
    let (_, metrics) = font.metrics();

    let ascent = metrics.ascent.abs();
    let descent = metrics.descent;
    let line_height = ascent + descent;

    (width, line_height, ascent, descent)
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
    if let Ok(mut handler) = renderer.input_handler.lock() {
        handler.set_target(pid);
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
    let encoded = tree::serialize::encode_tree(&tree);
    let mut binary = NewBinary::new(env, encoded.len());
    binary.as_mut_slice().copy_from_slice(&encoded);
    Ok(binary.into())
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
