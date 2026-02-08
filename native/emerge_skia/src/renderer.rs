//! Backend-agnostic Skia renderer.
//!
//! This module contains:
//! - `DrawCmd` enum and decoder for Elixir terms
//! - `RenderState` for holding commands between frames
//! - `Renderer` struct that executes draw commands on a Skia surface
//! - Font cache for text rendering

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};

use rustler::{Atom, Decoder, Error as RustlerError, Term};
use skia_safe::{
    Color, ColorType, Font, FontMgr, Paint, PaintStyle, Point, RRect, Rect, Shader, Surface,
    TileMode, Typeface, Vector,
    gpu::{self, SurfaceOrigin, backend_render_targets, gl::FramebufferInfo},
};

// ============================================================================
// Draw Commands
// ============================================================================

mod cmd_atoms {
    rustler::atoms! {
        clear,
        rect,
        rounded_rect,
        border,
        text,
        gradient,
        push_clip,
        pop_clip,
        translate,
        save,
        restore,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawCmd {
    Clear(u32),
    Rect(f32, f32, f32, f32, u32),
    RoundedRect(f32, f32, f32, f32, f32, u32),
    RoundedRectCorners(f32, f32, f32, f32, f32, f32, f32, f32, u32),
    Border(f32, f32, f32, f32, f32, f32, u32),
    BorderCorners(f32, f32, f32, f32, f32, f32, f32, f32, f32, u32),
    Text(f32, f32, String, f32, u32),
    /// Text with custom font: x, y, text, font_size, color, family, weight, italic
    TextWithFont(f32, f32, String, f32, u32, String, u16, bool),
    Gradient(f32, f32, f32, f32, u32, u32, f32),
    PushClip(f32, f32, f32, f32),
    PushClipRounded(f32, f32, f32, f32, f32),
    PushClipRoundedCorners(f32, f32, f32, f32, f32, f32, f32, f32),
    PopClip,
    Translate(f32, f32),
    Rotate(f32),
    Scale(f32, f32),
    SaveLayerAlpha(f32),
    Save,
    Restore,
}

impl<'a> Decoder<'a> for DrawCmd {
    fn decode(term: Term<'a>) -> Result<Self, RustlerError> {
        // Handle bare atoms first
        if let Ok(atom) = term.decode::<Atom>() {
            if atom == cmd_atoms::pop_clip() {
                return Ok(DrawCmd::PopClip);
            } else if atom == cmd_atoms::save() {
                return Ok(DrawCmd::Save);
            } else if atom == cmd_atoms::restore() {
                return Ok(DrawCmd::Restore);
            }
            return Err(RustlerError::BadArg);
        }

        // Handle tuples
        let tuple = rustler::types::tuple::get_tuple(term)?;
        if tuple.is_empty() {
            return Err(RustlerError::BadArg);
        }

        let tag: Atom = tuple[0].decode()?;

        if tag == cmd_atoms::clear() && tuple.len() == 2 {
            Ok(DrawCmd::Clear(tuple[1].decode()?))
        } else if tag == cmd_atoms::rect() && tuple.len() == 6 {
            Ok(DrawCmd::Rect(
                tuple[1].decode()?,
                tuple[2].decode()?,
                tuple[3].decode()?,
                tuple[4].decode()?,
                tuple[5].decode()?,
            ))
        } else if tag == cmd_atoms::rounded_rect() && tuple.len() == 7 {
            Ok(DrawCmd::RoundedRect(
                tuple[1].decode()?,
                tuple[2].decode()?,
                tuple[3].decode()?,
                tuple[4].decode()?,
                tuple[5].decode()?,
                tuple[6].decode()?,
            ))
        } else if tag == cmd_atoms::border() && tuple.len() == 8 {
            Ok(DrawCmd::Border(
                tuple[1].decode()?,
                tuple[2].decode()?,
                tuple[3].decode()?,
                tuple[4].decode()?,
                tuple[5].decode()?,
                tuple[6].decode()?,
                tuple[7].decode()?,
            ))
        } else if tag == cmd_atoms::text() && tuple.len() == 6 {
            Ok(DrawCmd::Text(
                tuple[1].decode()?,
                tuple[2].decode()?,
                tuple[3].decode()?,
                tuple[4].decode()?,
                tuple[5].decode()?,
            ))
        } else if tag == cmd_atoms::gradient() && tuple.len() == 8 {
            Ok(DrawCmd::Gradient(
                tuple[1].decode()?,
                tuple[2].decode()?,
                tuple[3].decode()?,
                tuple[4].decode()?,
                tuple[5].decode()?,
                tuple[6].decode()?,
                tuple[7].decode()?,
            ))
        } else if tag == cmd_atoms::push_clip() && tuple.len() == 5 {
            Ok(DrawCmd::PushClip(
                tuple[1].decode()?,
                tuple[2].decode()?,
                tuple[3].decode()?,
                tuple[4].decode()?,
            ))
        } else if tag == cmd_atoms::translate() && tuple.len() == 3 {
            Ok(DrawCmd::Translate(tuple[1].decode()?, tuple[2].decode()?))
        } else {
            Err(RustlerError::BadArg)
        }
    }
}

// ============================================================================
// Render State
// ============================================================================

pub struct RenderState {
    pub commands: Vec<DrawCmd>,
    pub clear_color: Color,
    pub render_version: u64,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            commands: Vec::new(),
            clear_color: Color::WHITE,
            render_version: 0,
        }
    }
}

// ============================================================================
// Font Cache
// ============================================================================

// Embedded default fonts (Inter, OFL licensed)
static DEFAULT_FONT_REGULAR: &[u8] = include_bytes!("fonts/Inter-Regular.ttf");
static DEFAULT_FONT_BOLD: &[u8] = include_bytes!("fonts/Inter-Bold.ttf");
static DEFAULT_FONT_ITALIC: &[u8] = include_bytes!("fonts/Inter-Italic.ttf");
static DEFAULT_FONT_BOLD_ITALIC: &[u8] = include_bytes!("fonts/Inter-BoldItalic.ttf");

/// Key for looking up fonts in the cache.
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct FontKey {
    pub family: String,
    pub weight: u16,   // 100-900, 400=normal, 700=bold
    pub italic: bool,
}

impl FontKey {
    pub fn new(family: impl Into<String>, weight: u16, italic: bool) -> Self {
        Self {
            family: family.into(),
            weight,
            italic,
        }
    }

    pub fn default_regular() -> Self {
        Self::new("default", 400, false)
    }

    pub fn default_bold() -> Self {
        Self::new("default", 700, false)
    }

    pub fn default_italic() -> Self {
        Self::new("default", 400, true)
    }

    pub fn default_bold_italic() -> Self {
        Self::new("default", 700, true)
    }
}

impl Default for FontKey {
    fn default() -> Self {
        Self::default_regular()
    }
}

static FONT_CACHE: OnceLock<Mutex<HashMap<FontKey, Arc<Typeface>>>> = OnceLock::new();
static SYNTHETIC_LOGGED: OnceLock<Mutex<HashSet<FontKey>>> = OnceLock::new();
static RENDER_LOG_ENABLED: AtomicBool = AtomicBool::new(false);

fn get_font_cache() -> &'static Mutex<HashMap<FontKey, Arc<Typeface>>> {
    FONT_CACHE.get_or_init(|| {
        let mut cache = HashMap::new();
        let font_mgr = FontMgr::new();

        // Load embedded default fonts
        if let Some(tf) = font_mgr.new_from_data(DEFAULT_FONT_REGULAR, 0) {
            cache.insert(FontKey::default_regular(), Arc::new(tf));
        }
        if let Some(tf) = font_mgr.new_from_data(DEFAULT_FONT_BOLD, 0) {
            cache.insert(FontKey::default_bold(), Arc::new(tf));
        }
        if let Some(tf) = font_mgr.new_from_data(DEFAULT_FONT_ITALIC, 0) {
            cache.insert(FontKey::default_italic(), Arc::new(tf));
        }
        if let Some(tf) = font_mgr.new_from_data(DEFAULT_FONT_BOLD_ITALIC, 0) {
            cache.insert(FontKey::default_bold_italic(), Arc::new(tf));
        }

        Mutex::new(cache)
    })
}

fn get_synthetic_log_cache() -> &'static Mutex<HashSet<FontKey>> {
    SYNTHETIC_LOGGED.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn set_render_log_enabled(enabled: bool) {
    RENDER_LOG_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Get a typeface from the cache by key.
pub fn get_typeface(key: &FontKey) -> Option<Arc<Typeface>> {
    let cache = get_font_cache().lock().ok()?;
    cache.get(key).cloned()
}

fn resolve_typeface_with_fallback(
    family: &str,
    weight: u16,
    italic: bool,
) -> (Arc<Typeface>, bool) {
    let requested = FontKey::new(family, weight, italic);
    if let Some(tf) = get_typeface(&requested) {
        return (tf, true);
    }

    let default_requested = FontKey::new("default", weight, italic);
    if let Some(tf) = get_typeface(&default_requested) {
        return (tf, true);
    }

    let key = FontKey::new(family, 400, false);
    if let Some(tf) = get_typeface(&key) {
        return (tf, false);
    }

    let fallback = FontKey::default_regular();
    let tf = get_typeface(&fallback).expect("embedded default font must exist");
    (tf, false)
}

pub fn make_font_with_style(
    family: &str,
    weight: u16,
    italic: bool,
    size: f32,
) -> Font {
    let (typeface, exact) = resolve_typeface_with_fallback(family, weight, italic);
    let mut font = Font::new(&*typeface, size);

    if !exact {
        if weight >= 600 {
            font.set_embolden(true);
        }
        if italic {
            font.set_skew_x(-0.25);
        }

        if RENDER_LOG_ENABLED.load(Ordering::Relaxed) {
            let key = FontKey::new(family, weight, italic);
            if let Ok(mut cache) = get_synthetic_log_cache().lock()
                && cache.insert(key)
            {
                eprintln!(
                    "synthetic font style applied family={} weight={} italic={}",
                    family, weight, italic
                );
            }
        }
    }

    font
}

/// Get a typeface with fallback to default if not found.
pub fn get_typeface_with_fallback(family: &str, weight: u16, italic: bool) -> Arc<Typeface> {
    let (tf, _exact) = resolve_typeface_with_fallback(family, weight, italic);
    tf
}

/// Load a font from binary data and register it in the cache.
pub fn load_font(family: &str, weight: u16, italic: bool, data: &[u8]) -> Result<(), String> {
    let font_mgr = FontMgr::new();
    let typeface = font_mgr
        .new_from_data(data, 0)
        .ok_or_else(|| "Invalid font data".to_string())?;

    let cache = get_font_cache();
    let mut cache = cache.lock().map_err(|_| "Font cache lock poisoned")?;

    cache.insert(FontKey::new(family, weight, italic), Arc::new(typeface));

    Ok(())
}

/// Get the default typeface (for backward compatibility).
pub fn get_default_typeface() -> Arc<Typeface> {
    get_typeface_with_fallback("default", 400, false)
}

// ============================================================================
// Renderer
// ============================================================================

#[derive(Clone, Copy)]
pub enum SurfaceSource {
    Gl {
        fb_info: FramebufferInfo,
        num_samples: usize,
        stencil_size: usize,
    },
    #[allow(dead_code)]
    Raster,
}

pub struct Renderer {
    surface: Surface,
    gr_context: Option<skia_safe::gpu::DirectContext>,
    source: SurfaceSource,
}

impl Renderer {
    /// Create a new renderer with a GPU-backed surface (for GL/Wayland/DRM backends).
    pub fn new_gl(
        dimensions: (u32, u32),
        fb_info: FramebufferInfo,
        gr_context: skia_safe::gpu::DirectContext,
        num_samples: usize,
        stencil_size: usize,
    ) -> Self {
        let mut gr_context = gr_context;
        let surface = create_gl_surface(
            (dimensions.0 as i32, dimensions.1 as i32),
            fb_info,
            &mut gr_context,
            num_samples,
            stencil_size,
        );

        Self {
            surface,
            gr_context: Some(gr_context),
            source: SurfaceSource::Gl {
                fb_info,
                num_samples,
                stencil_size,
            },
        }
    }

    /// Create a new renderer with a CPU-backed surface (for raster backend).
    #[allow(dead_code)]
    pub fn from_surface(surface: Surface) -> Self {
        Self {
            surface,
            gr_context: None,
            source: SurfaceSource::Raster,
        }
    }

    /// Get mutable access to the underlying Skia surface.
    #[allow(dead_code)]
    pub fn surface_mut(&mut self) -> &mut Surface {
        &mut self.surface
    }

    /// Resize the surface (only works for GL surfaces).
    pub fn resize(&mut self, dimensions: (u32, u32)) {
        if let SurfaceSource::Gl {
            fb_info,
            num_samples,
            stencil_size,
        } = self.source
            && let Some(context) = self.gr_context.as_mut()
        {
            self.surface = create_gl_surface(
                (dimensions.0 as i32, dimensions.1 as i32),
                fb_info,
                context,
                num_samples,
                stencil_size,
            );
        }
    }

    /// Render the given state to the surface.
    pub fn render(&mut self, state: &RenderState) {
        let canvas = self.surface.canvas();
        canvas.clear(state.clear_color);

        let typeface = get_default_typeface();
        let typeface = typeface.as_ref();

        for cmd in &state.commands {
            match cmd {
                DrawCmd::Clear(color) => {
                    canvas.clear(color_from_u32(*color));
                }

                DrawCmd::Rect(x, y, w, h, fill) => {
                    let rect = Rect::from_xywh(*x, *y, *w, *h);
                    let mut paint = Paint::default();
                    paint.set_color(color_from_u32(*fill));
                    paint.set_anti_alias(true);
                    canvas.draw_rect(rect, &paint);
                }

                DrawCmd::RoundedRect(x, y, w, h, radius, fill) => {
                    let rect = Rect::from_xywh(*x, *y, *w, *h);
                    let rrect = RRect::new_rect_xy(rect, *radius, *radius);
                    let mut paint = Paint::default();
                    paint.set_color(color_from_u32(*fill));
                    paint.set_anti_alias(true);
                    canvas.draw_rrect(rrect, &paint);
                }

                DrawCmd::RoundedRectCorners(x, y, w, h, tl, tr, br, bl, fill) => {
                    let rect = Rect::from_xywh(*x, *y, *w, *h);
                    let radii = [
                        Point::new(*tl, *tl),
                        Point::new(*tr, *tr),
                        Point::new(*br, *br),
                        Point::new(*bl, *bl),
                    ];
                    let rrect = RRect::new_rect_radii(rect, &radii);
                    let mut paint = Paint::default();
                    paint.set_color(color_from_u32(*fill));
                    paint.set_anti_alias(true);
                    canvas.draw_rrect(rrect, &paint);
                }

                DrawCmd::Border(x, y, w, h, radius, width, color) => {
                    let rect = Rect::from_xywh(*x, *y, *w, *h);
                    let rrect = RRect::new_rect_xy(rect, *radius, *radius);
                    let mut paint = Paint::default();
                    paint.set_color(color_from_u32(*color));
                    paint.set_style(PaintStyle::Stroke);
                    paint.set_stroke_width(*width);
                    paint.set_anti_alias(true);
                    canvas.draw_rrect(rrect, &paint);
                }

                DrawCmd::BorderCorners(x, y, w, h, tl, tr, br, bl, width, color) => {
                    let rect = Rect::from_xywh(*x, *y, *w, *h);
                    let radii = [
                        Point::new(*tl, *tl),
                        Point::new(*tr, *tr),
                        Point::new(*br, *br),
                        Point::new(*bl, *bl),
                    ];
                    let rrect = RRect::new_rect_radii(rect, &radii);
                    let mut paint = Paint::default();
                    paint.set_color(color_from_u32(*color));
                    paint.set_style(PaintStyle::Stroke);
                    paint.set_stroke_width(*width);
                    paint.set_anti_alias(true);
                    canvas.draw_rrect(rrect, &paint);
                }

                DrawCmd::Text(x, y, text, font_size, fill) => {
                    let font = Font::new(typeface, *font_size);
                    let mut paint = Paint::default();
                    paint.set_color(color_from_u32(*fill));
                    paint.set_anti_alias(true);
                    canvas.draw_str(text, (*x, *y), &font, &paint);
                }

                DrawCmd::TextWithFont(x, y, text, font_size, fill, family, weight, italic) => {
                    let font = make_font_with_style(family, *weight, *italic, *font_size);
                    let mut paint = Paint::default();
                    paint.set_color(color_from_u32(*fill));
                    paint.set_anti_alias(true);
                    canvas.draw_str(text, (*x, *y), &font, &paint);
                }

                DrawCmd::Gradient(x, y, w, h, from, to, angle) => {
                    let rect = Rect::from_xywh(*x, *y, *w, *h);

                    let radians = angle.to_radians();
                    let cx = x + w / 2.0;
                    let cy = y + h / 2.0;
                    let half_diag = (w * w + h * h).sqrt() / 2.0;

                    let start = (
                        cx - radians.cos() * half_diag,
                        cy - radians.sin() * half_diag,
                    );
                    let end = (
                        cx + radians.cos() * half_diag,
                        cy + radians.sin() * half_diag,
                    );

                    let colors = [color_from_u32(*from), color_from_u32(*to)];
                    if let Some(shader) = Shader::linear_gradient(
                        (start, end),
                        colors.as_slice(),
                        None,
                        TileMode::Clamp,
                        None,
                        None,
                    ) {
                        let mut paint = Paint::default();
                        paint.set_shader(shader);
                        paint.set_anti_alias(true);
                        canvas.draw_rect(rect, &paint);
                    }
                }

                DrawCmd::PushClip(x, y, w, h) => {
                    canvas.save();
                    let rect = Rect::from_xywh(*x, *y, *w, *h);
                    canvas.clip_rect(rect, skia_safe::ClipOp::Intersect, true);
                }

                DrawCmd::PushClipRounded(x, y, w, h, radius) => {
                    canvas.save();
                    let rect = Rect::from_xywh(*x, *y, *w, *h);
                    let rrect = RRect::new_rect_xy(rect, *radius, *radius);
                    canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);
                }

                DrawCmd::PushClipRoundedCorners(x, y, w, h, tl, tr, br, bl) => {
                    canvas.save();
                    let rect = Rect::from_xywh(*x, *y, *w, *h);
                    let radii = [
                        Point::new(*tl, *tl),
                        Point::new(*tr, *tr),
                        Point::new(*br, *br),
                        Point::new(*bl, *bl),
                    ];
                    let rrect = RRect::new_rect_radii(rect, &radii);
                    canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);
                }

                DrawCmd::PopClip => {
                    canvas.restore();
                }

                DrawCmd::Translate(x, y) => {
                    canvas.translate(Vector::new(*x, *y));
                }

                DrawCmd::Rotate(degrees) => {
                    canvas.rotate(*degrees, None);
                }

                DrawCmd::Scale(x, y) => {
                    canvas.scale((*x, *y));
                }

                DrawCmd::SaveLayerAlpha(alpha) => {
                    let clamped = alpha.clamp(0.0, 1.0);
                    let alpha_u8 = (clamped * 255.0).round() as u8;
                    canvas.save_layer_alpha(None, alpha_u8.into());
                }

                DrawCmd::Save => {
                    canvas.save();
                }

                DrawCmd::Restore => {
                    canvas.restore();
                }
            }
        }

        if let Some(gr) = self.gr_context.as_mut() {
            gr.flush_and_submit();
        }
    }

    /// Flush the GPU context after manual drawing.
    pub fn flush(&mut self) {
        if let Some(gr) = self.gr_context.as_mut() {
            gr.flush_and_submit();
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn create_gl_surface(
    dimensions: (i32, i32),
    fb_info: FramebufferInfo,
    gr_context: &mut skia_safe::gpu::DirectContext,
    num_samples: usize,
    stencil_size: usize,
) -> Surface {
    let backend_render_target =
        backend_render_targets::make_gl(dimensions, num_samples, stencil_size, fb_info);

    gpu::surfaces::wrap_backend_render_target(
        gr_context,
        &backend_render_target,
        SurfaceOrigin::BottomLeft,
        ColorType::RGBA8888,
        None,
        None,
    )
    .expect("Could not create Skia surface")
}

pub fn color_from_u32(c: u32) -> Color {
    // RGBA format: 0xRRGGBBAA
    let r = ((c >> 24) & 0xFF) as u8;
    let g = ((c >> 16) & 0xFF) as u8;
    let b = ((c >> 8) & 0xFF) as u8;
    let a = (c & 0xFF) as u8;
    Color::from_argb(a, r, g, b)
}
