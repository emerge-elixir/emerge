//! Native tree boundary for the clean slice. No legacy producer or paint engine
//! is linked into this library. See plans/README.md for the supported wire subset.
pub mod input;
pub mod keys;
#[path = "runtime/lifecycle.rs"]
mod lifecycle;
mod services;
mod session;
pub mod tree;
#[cfg(all(target_os = "linux", feature = "wayland", feature = "vulkan"))]
#[path = "backend/vulkan/window.rs"]
mod vk_window;
use rustler::{Atom, Binary, Env, NewBinary, ResourceArc, Term};
use std::sync::{Mutex, MutexGuard};
use tree::Tree;

mod atoms {
    rustler::atoms! {ok}
}
#[derive(Default)]
struct TreeResource {
    tree: Mutex<Tree>,
}
#[rustler::resource_impl]
impl rustler::Resource for TreeResource {}
fn lock<T>(m: &Mutex<T>) -> Result<MutexGuard<'_, T>, String> {
    m.lock().map_err(|_| "tree resource unavailable".into())
}
fn binary<'a>(env: Env<'a>, bytes: &[u8]) -> Binary<'a> {
    let mut b = NewBinary::new(env, bytes.len());
    b.as_mut_slice().copy_from_slice(bytes);
    b.into()
}

#[rustler::nif]
fn tree_new() -> ResourceArc<TreeResource> {
    ResourceArc::new(TreeResource::default())
}
#[rustler::nif(schedule = "DirtyCpu")]
fn tree_upload(r: ResourceArc<TreeResource>, data: Binary) -> Result<bool, String> {
    lock(&r.tree)?.upload(data.as_slice())?;
    Ok(true)
}
#[rustler::nif(schedule = "DirtyCpu")]
fn tree_patch(r: ResourceArc<TreeResource>, data: Binary) -> Result<bool, String> {
    lock(&r.tree)?.patch(data.as_slice())?;
    Ok(true)
}
// The regular Elixir TreeRenderer transport expects bare :ok, whereas the
// lower-level tree resource bindings return {:ok, true}. Both edit this tree.
fn submitted(result: Result<(), String>) -> rustler::NifResult<Atom> {
    result
        .map(|_| atoms::ok())
        .map_err(|e| rustler::Error::Term(Box::new(e)))
}
#[rustler::nif(schedule = "DirtyCpu")]
fn tree_roundtrip<'a>(env: Env<'a>, data: Binary) -> Result<Binary<'a>, String> {
    let mut tree = Tree::default();
    tree.upload(data.as_slice())?;
    Ok(binary(env, &tree.encode()))
}
#[rustler::nif(schedule = "DirtyCpu")]
fn tree_upload_roundtrip<'a>(
    env: Env<'a>,
    r: ResourceArc<TreeResource>,
    data: Binary,
) -> Result<Binary<'a>, String> {
    let mut tree = lock(&r.tree)?;
    tree.upload(data.as_slice())?;
    Ok(binary(env, &tree.encode()))
}
#[rustler::nif(schedule = "DirtyCpu")]
fn tree_patch_roundtrip<'a>(
    env: Env<'a>,
    r: ResourceArc<TreeResource>,
    data: Binary,
) -> Result<Binary<'a>, String> {
    let mut tree = lock(&r.tree)?;
    tree.patch(data.as_slice())?;
    Ok(binary(env, &tree.encode()))
}

type Frames<'a> = Vec<(Binary<'a>, f32, f32, f32, f32)>;
#[rustler::nif(schedule = "DirtyCpu")]
fn tree_layout<'a>(
    env: Env<'a>,
    r: ResourceArc<TreeResource>,
    width: f64,
    height: f64,
    scale: f64,
) -> Result<Frames<'a>, String> {
    let mut t = lock(&r.tree)?;
    t.refresh(width as f32, height as f32, scale as f32)?;
    Ok(t.frames()
        .map(|(id, f)| (binary(env, &id.to_be_bytes()), f.x, f.y, f.width, f.height))
        .collect())
}
#[rustler::nif(schedule = "DirtyCpu")]
fn measure_text(text: &str, size: f64) -> (f32, f32, f32, f32) {
    tree::TextMeasurer::dimensions(text, size as f32)
}
#[derive(rustler::NifMap)]
struct Offscreen<'a> {
    width: u32,
    height: u32,
    scale: f64,
    fonts: Vec<Term<'a>>,
    asset_mode: &'a str,
}
fn offscreen(data: Binary, opts: &Offscreen) -> Result<Vec<u8>, String> {
    if !matches!(opts.asset_mode, "snapshot" | "await") {
        return Err(format!("unsupported asset mode: {}", opts.asset_mode));
    }
    if !opts.fonts.is_empty() {
        return Err("custom fonts are not in the first slice".into());
    }
    services::render_tree_offscreen(data.as_slice(), opts.width, opts.height, opts.scale as f32)
}
#[rustler::nif(schedule = "DirtyCpu")]
fn render_tree_to_pixels_nif<'a>(
    env: Env<'a>,
    data: Binary,
    opts: Offscreen,
) -> Result<Binary<'a>, String> {
    Ok(binary(env, &offscreen(data, &opts)?))
}
#[rustler::nif(schedule = "DirtyCpu")]
fn render_tree_to_png_nif<'a>(
    env: Env<'a>,
    data: Binary,
    opts: Offscreen,
) -> Result<Binary<'a>, String> {
    let mut pixels = offscreen(data, &opts)?;
    let png = services::png(opts.width, opts.height, &mut pixels)?;
    Ok(binary(env, &png))
}
rustler::init!("Elixir.EmergeSkia.Native");
