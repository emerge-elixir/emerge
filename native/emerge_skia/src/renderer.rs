//! Backend-agnostic Skia renderer.
//!
//! This module contains:
//! - `DrawCmd` enum and decoder for Elixir terms
//! - `RenderState` for holding commands between frames
//! - `Renderer` struct that executes draw commands on a Skia surface
//! - Font cache for text rendering

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use rustler::{Atom, Decoder, Error as RustlerError, Term};
use skia_safe::{
    BlurStyle, Color, ColorType, Data, FilterMode, Font, FontMgr, Image, MaskFilter, Matrix,
    MipmapMode, Paint, PaintStyle, PathBuilder, PathFillType, Point, RRect, Rect, SamplingOptions,
    Shader, Surface, TileMode, Typeface, Vector,
    canvas::SrcRectConstraint,
    dash_path_effect,
    gpu::{self, SurfaceOrigin, backend_render_targets, gl::FramebufferInfo},
};

use crate::tree::attrs::{BorderStyle, ImageFit};
use crate::video::{RendererVideoState, VideoSyncResult};

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
        image,
        push_clip,
        pop_clip,
        translate,
        save,
        restore,
        contain,
        cover,
        repeat,
        repeat_x,
        repeat_y,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawCmd {
    Clear(u32),
    Rect(f32, f32, f32, f32, u32),
    RoundedRect(f32, f32, f32, f32, f32, u32),
    RoundedRectCorners(f32, f32, f32, f32, f32, f32, f32, f32, u32),
    Border(f32, f32, f32, f32, f32, f32, u32, BorderStyle),
    BorderCorners(
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        u32,
        BorderStyle,
    ),
    /// Per-edge border: x, y, w, h, radius, top, right, bottom, left, color, style
    BorderEdges(
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        u32,
        BorderStyle,
    ),
    /// Outer shadow: x, y, w, h, offset_x, offset_y, blur, size, radius, color
    Shadow(f32, f32, f32, f32, f32, f32, f32, f32, f32, u32),
    /// Inner (inset) shadow: x, y, w, h, offset_x, offset_y, blur, size, radius, color
    InsetShadow(f32, f32, f32, f32, f32, f32, f32, f32, f32, u32),
    Text(f32, f32, String, f32, u32),
    /// Text with custom font: x, y, text, font_size, color, family, weight, italic
    TextWithFont(f32, f32, String, f32, u32, String, u16, bool),
    /// Gradient: x, y, w, h, from_color, to_color, angle, radius
    Gradient(f32, f32, f32, f32, u32, u32, f32, f32),
    /// Image draw: x, y, w, h, image_id, fit
    Image(f32, f32, f32, f32, String, ImageFit),
    /// Video draw: x, y, w, h, target_id, fit
    Video(f32, f32, f32, f32, String, ImageFit),
    /// Loading placeholder: x, y, w, h
    ImageLoading(f32, f32, f32, f32),
    /// Failed placeholder: x, y, w, h
    ImageFailed(f32, f32, f32, f32),
    PushClip(f32, f32, f32, f32),
    PushClipRounded(f32, f32, f32, f32, f32),
    PushClipRoundedCorners(f32, f32, f32, f32, f32, f32, f32, f32),
    PushClipHard(f32, f32, f32, f32),
    PushClipRoundedHard(f32, f32, f32, f32, f32),
    PushClipRoundedCornersHard(f32, f32, f32, f32, f32, f32, f32, f32),
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
                BorderStyle::Solid,
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
                0.0,
            ))
        } else if tag == cmd_atoms::image() && tuple.len() == 7 {
            let fit_atom: Atom = tuple[6].decode()?;
            let fit = if fit_atom == cmd_atoms::cover() {
                ImageFit::Cover
            } else if fit_atom == cmd_atoms::repeat() {
                ImageFit::Repeat
            } else if fit_atom == cmd_atoms::repeat_x() {
                ImageFit::RepeatX
            } else if fit_atom == cmd_atoms::repeat_y() {
                ImageFit::RepeatY
            } else {
                ImageFit::Contain
            };

            Ok(DrawCmd::Image(
                tuple[1].decode()?,
                tuple[2].decode()?,
                tuple[3].decode()?,
                tuple[4].decode()?,
                tuple[5].decode()?,
                fit,
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
    pub animate: bool,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            commands: Vec::new(),
            clear_color: Color::WHITE,
            render_version: 0,
            animate: false,
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
    pub weight: u16, // 100-900, 400=normal, 700=bold
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

pub fn make_font_with_style(family: &str, weight: u16, italic: bool, size: f32) -> Font {
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

#[derive(Clone)]
struct CachedImage {
    image: Image,
    width: u32,
    height: u32,
}

static IMAGE_CACHE: OnceLock<Mutex<HashMap<String, Arc<CachedImage>>>> = OnceLock::new();

fn get_image_cache() -> &'static Mutex<HashMap<String, Arc<CachedImage>>> {
    IMAGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_image(id: &str) -> Option<Arc<CachedImage>> {
    let cache = get_image_cache().lock().ok()?;
    cache.get(id).cloned()
}

pub fn image_dimensions(id: &str) -> Option<(u32, u32)> {
    cached_image(id).map(|cached| (cached.width, cached.height))
}

pub fn insert_image(id: &str, data: &[u8]) -> Result<(u32, u32), String> {
    let image = Image::from_encoded(Data::new_copy(data))
        .ok_or_else(|| "failed to decode image data".to_string())?;

    let width = image.width().max(0) as u32;
    let height = image.height().max(0) as u32;

    let cache = get_image_cache();
    let mut cache = cache.lock().map_err(|_| "image cache lock poisoned")?;
    cache.insert(
        id.to_string(),
        Arc::new(CachedImage {
            image,
            width,
            height,
        }),
    );

    Ok((width, height))
}

#[cfg(test)]
fn remove_image(id: &str) {
    if let Ok(mut cache) = get_image_cache().lock() {
        cache.remove(id);
    }
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
    Raster,
}

pub struct Renderer {
    surface: Surface,
    gr_context: Option<skia_safe::gpu::DirectContext>,
    source: SurfaceSource,
    video_state: RendererVideoState,
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
            video_state: RendererVideoState::default(),
        }
    }

    /// Create a new renderer with a CPU-backed surface (for raster backend).
    pub fn from_surface(surface: Surface) -> Self {
        Self {
            surface,
            gr_context: None,
            source: SurfaceSource::Raster,
            video_state: RendererVideoState::default(),
        }
    }

    pub fn sync_video_frames(
        &mut self,
        registry: &Arc<crate::video::VideoRegistry>,
        ctx: Option<&crate::video::VideoImportContext>,
    ) -> Result<VideoSyncResult, String> {
        let Some(gr_context) = self.gr_context.as_mut() else {
            return Ok(VideoSyncResult::default());
        };

        let result = self.video_state.sync_pending(registry, gr_context, ctx)?;
        if result.resources_changed {
            gr_context.reset(None);
        }
        Ok(result)
    }

    /// Get mutable access to the underlying Skia surface.
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
        let mut clip_stack: Vec<bool> = Vec::new();
        let mut hard_clip_depth: usize = 0;

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

                DrawCmd::Border(x, y, w, h, radius, width, color, style) => {
                    draw_border(
                        canvas,
                        BorderDrawSpec {
                            rect: RectSpec {
                                x: *x,
                                y: *y,
                                w: *w,
                                h: *h,
                            },
                            corners: [*radius, *radius, *radius, *radius],
                            insets: EdgeInsets::uniform(*width),
                            color: *color,
                            style: *style,
                        },
                    );
                }

                DrawCmd::BorderCorners(x, y, w, h, tl, tr, br, bl, width, color, style) => {
                    draw_border(
                        canvas,
                        BorderDrawSpec {
                            rect: RectSpec {
                                x: *x,
                                y: *y,
                                w: *w,
                                h: *h,
                            },
                            corners: [*tl, *tr, *br, *bl],
                            insets: EdgeInsets::uniform(*width),
                            color: *color,
                            style: *style,
                        },
                    );
                }

                DrawCmd::BorderEdges(
                    x,
                    y,
                    w,
                    h,
                    radius,
                    top,
                    right,
                    bottom,
                    left,
                    color,
                    style,
                ) => {
                    draw_border(
                        canvas,
                        BorderDrawSpec {
                            rect: RectSpec {
                                x: *x,
                                y: *y,
                                w: *w,
                                h: *h,
                            },
                            corners: [*radius, *radius, *radius, *radius],
                            insets: EdgeInsets {
                                top: *top,
                                right: *right,
                                bottom: *bottom,
                                left: *left,
                            },
                            color: *color,
                            style: *style,
                        },
                    );
                }

                DrawCmd::Shadow(x, y, w, h, offset_x, offset_y, blur, size, radius, color) => {
                    // Expand rect by spread size and offset
                    let shadow_x = *x + *offset_x - *size;
                    let shadow_y = *y + *offset_y - *size;
                    let shadow_w = *w + *size * 2.0;
                    let shadow_h = *h + *size * 2.0;
                    let shadow_radius = (*radius + *size).max(0.0);

                    let rect = Rect::from_xywh(shadow_x, shadow_y, shadow_w, shadow_h);
                    let rrect = if shadow_radius > 0.0 {
                        RRect::new_rect_xy(rect, shadow_radius, shadow_radius)
                    } else {
                        RRect::new_rect(rect)
                    };

                    let mut paint = Paint::default();
                    paint.set_color(color_from_u32(*color));
                    paint.set_anti_alias(true);

                    if *blur > 0.0 {
                        let sigma = *blur / 2.0;
                        if let Some(filter) = MaskFilter::blur(BlurStyle::Normal, sigma, false) {
                            paint.set_mask_filter(filter);
                        }
                    }

                    canvas.draw_rrect(rrect, &paint);
                }

                DrawCmd::InsetShadow(x, y, w, h, offset_x, offset_y, blur, size, radius, color) => {
                    // Clip to the element bounds so shadow doesn't leak outside
                    let bounds_rect = Rect::from_xywh(*x, *y, *w, *h);
                    let bounds_rrect = if *radius > 0.0 {
                        RRect::new_rect_xy(bounds_rect, *radius, *radius)
                    } else {
                        RRect::new_rect(bounds_rect)
                    };

                    canvas.save();
                    canvas.clip_rrect(bounds_rrect, skia_safe::ClipOp::Intersect, true);

                    // Create inset shadow by drawing a large rect with a hole
                    let inset_x = *x + *offset_x + *size;
                    let inset_y = *y + *offset_y + *size;
                    let inset_w = *w - *size * 2.0;
                    let inset_h = *h - *size * 2.0;
                    let inset_radius = (*radius - *size).max(0.0);

                    let inner_rect =
                        Rect::from_xywh(inset_x, inset_y, inset_w.max(0.0), inset_h.max(0.0));
                    let inner_rrect = if inset_radius > 0.0 {
                        RRect::new_rect_xy(inner_rect, inset_radius, inset_radius)
                    } else {
                        RRect::new_rect(inner_rect)
                    };

                    // Create a path: large outer rect minus inner rrect (EvenOdd)
                    let margin = (*blur + *size) * 4.0 + 100.0;
                    let outer_rect = Rect::from_xywh(
                        *x - margin,
                        *y - margin,
                        *w + margin * 2.0,
                        *h + margin * 2.0,
                    );
                    let mut builder = PathBuilder::new_with_fill_type(PathFillType::EvenOdd);
                    builder.add_rect(outer_rect, None, None);
                    builder.add_rrect(inner_rrect, None, None);
                    let path = builder.detach();

                    let mut paint = Paint::default();
                    paint.set_color(color_from_u32(*color));
                    paint.set_anti_alias(true);

                    if *blur > 0.0 {
                        let sigma = *blur / 2.0;
                        if let Some(filter) = MaskFilter::blur(BlurStyle::Normal, sigma, false) {
                            paint.set_mask_filter(filter);
                        }
                    }

                    canvas.draw_path(&path, &paint);
                    canvas.restore();
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

                DrawCmd::Gradient(x, y, w, h, from, to, angle, radius) => {
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
                        if *radius > 0.0 {
                            let rrect = RRect::new_rect_xy(rect, *radius, *radius);
                            canvas.draw_rrect(rrect, &paint);
                        } else {
                            canvas.draw_rect(rect, &paint);
                        }
                    }
                }

                DrawCmd::Image(x, y, w, h, image_id, fit) => {
                    let inside_hard_clip = hard_clip_depth > 0;
                    draw_cached_image_with_fit(
                        canvas,
                        ImageDrawSpec {
                            rect: RectSpec {
                                x: *x,
                                y: *y,
                                w: *w,
                                h: *h,
                            },
                            image_id,
                            fit: *fit,
                            inside_hard_clip,
                        },
                    );
                }

                DrawCmd::Video(x, y, w, h, target_id, fit) => {
                    let inside_hard_clip = hard_clip_depth > 0;
                    if let Some((image, image_width, image_height)) =
                        self.video_state.image(target_id)
                    {
                        draw_image_with_fit(
                            canvas,
                            image,
                            image_width,
                            image_height,
                            ImageDrawSpec {
                                rect: RectSpec {
                                    x: *x,
                                    y: *y,
                                    w: *w,
                                    h: *h,
                                },
                                image_id: target_id,
                                fit: *fit,
                                inside_hard_clip,
                            },
                        );
                    }
                }

                DrawCmd::ImageLoading(x, y, w, h) => {
                    draw_image_loading(canvas, *x, *y, *w, *h);
                }

                DrawCmd::ImageFailed(x, y, w, h) => {
                    draw_image_failed(canvas, *x, *y, *w, *h);
                }

                DrawCmd::PushClip(x, y, w, h) => {
                    canvas.save();
                    let rect = Rect::from_xywh(*x, *y, *w, *h);
                    canvas.clip_rect(rect, skia_safe::ClipOp::Intersect, true);
                    clip_stack.push(false);
                }

                DrawCmd::PushClipRounded(x, y, w, h, radius) => {
                    canvas.save();
                    let rect = Rect::from_xywh(*x, *y, *w, *h);
                    let rrect = RRect::new_rect_xy(rect, *radius, *radius);
                    canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);
                    clip_stack.push(false);
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
                    clip_stack.push(false);
                }

                DrawCmd::PushClipHard(x, y, w, h) => {
                    canvas.save();
                    let rect = Rect::from_xywh(*x, *y, *w, *h);
                    let expanded = snap_outset_rect_to_device(canvas, rect);
                    canvas.clip_rect(expanded, skia_safe::ClipOp::Intersect, false);
                    clip_stack.push(true);
                    hard_clip_depth += 1;
                }

                DrawCmd::PushClipRoundedHard(x, y, w, h, radius) => {
                    canvas.save();
                    let rect = Rect::from_xywh(*x, *y, *w, *h);
                    let expanded = snap_outset_rect_to_device(canvas, rect);
                    let (outset_x, outset_y) = rect_outset_amount(rect, expanded);
                    let radius = (*radius + outset_x.max(outset_y)).max(0.0);
                    let rrect = RRect::new_rect_xy(expanded, radius, radius);
                    canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, false);
                    clip_stack.push(true);
                    hard_clip_depth += 1;
                }

                DrawCmd::PushClipRoundedCornersHard(x, y, w, h, tl, tr, br, bl) => {
                    canvas.save();
                    let rect = Rect::from_xywh(*x, *y, *w, *h);
                    let expanded = snap_outset_rect_to_device(canvas, rect);
                    let (outset_x, outset_y) = rect_outset_amount(rect, expanded);
                    let dr = outset_x.max(outset_y);
                    let radii = [
                        Point::new(*tl + dr, *tl + dr),
                        Point::new(*tr + dr, *tr + dr),
                        Point::new(*br + dr, *br + dr),
                        Point::new(*bl + dr, *bl + dr),
                    ];
                    let rrect = RRect::new_rect_radii(expanded, &radii);
                    canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, false);
                    clip_stack.push(true);
                    hard_clip_depth += 1;
                }

                DrawCmd::PopClip => {
                    canvas.restore();
                    if clip_stack.pop().unwrap_or(false) {
                        hard_clip_depth = hard_clip_depth.saturating_sub(1);
                    }
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

#[derive(Clone, Copy, Debug, PartialEq)]
struct RectSpec {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct EdgeInsets {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

impl EdgeInsets {
    fn uniform(width: f32) -> Self {
        Self {
            top: width,
            right: width,
            bottom: width,
            left: width,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ImageDrawSpec<'a> {
    rect: RectSpec,
    image_id: &'a str,
    fit: ImageFit,
    inside_hard_clip: bool,
}

#[derive(Clone, Copy, Debug)]
struct BorderDrawSpec {
    rect: RectSpec,
    corners: [f32; 4],
    insets: EdgeInsets,
    color: u32,
    style: BorderStyle,
}

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

fn draw_cached_image_with_fit(canvas: &skia_safe::Canvas, spec: ImageDrawSpec<'_>) {
    let RectSpec { w, h, .. } = spec.rect;

    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let Some(cached) = cached_image(spec.image_id) else {
        return;
    };

    draw_image_with_fit(canvas, &cached.image, cached.width, cached.height, spec);
}

fn draw_image_with_fit(
    canvas: &skia_safe::Canvas,
    image: &Image,
    image_width: u32,
    image_height: u32,
    spec: ImageDrawSpec<'_>,
) {
    let RectSpec { x, y, w, h } = spec.rect;

    match spec.fit {
        ImageFit::Contain | ImageFit::Cover => {
            let src_w = image_width as f32;
            let src_h = image_height as f32;
            let Some(rects) = compute_image_fit_rects(src_w, src_h, x, y, w, h, spec.fit) else {
                return;
            };

            let mut paint = Paint::default();
            paint.set_anti_alias(false);
            let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);

            let src_rect = Rect::from_xywh(rects.src_x, rects.src_y, rects.src_w, rects.src_h);
            let dst_rect = Rect::from_xywh(rects.dst_x, rects.dst_y, rects.dst_w, rects.dst_h);
            let dst_rect = if spec.inside_hard_clip && matches!(spec.fit, ImageFit::Cover) {
                snap_outset_rect_to_device(canvas, dst_rect)
            } else {
                dst_rect
            };
            canvas.draw_image_rect_with_sampling_options(
                image,
                Some((&src_rect, SrcRectConstraint::Strict)),
                dst_rect,
                sampling,
                &paint,
            );
        }
        ImageFit::Repeat | ImageFit::RepeatX | ImageFit::RepeatY => {
            draw_tiled_image(canvas, image, x, y, w, h, spec.fit);
        }
    }
}

fn draw_tiled_image(
    canvas: &skia_safe::Canvas,
    image: &Image,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fit: ImageFit,
) {
    let Some(tile_modes) = tile_modes_for_fit(fit) else {
        return;
    };

    let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);
    let local_matrix = Matrix::translate((x, y));
    let Some(shader) = image.to_shader(Some(tile_modes), sampling, Some(&local_matrix)) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_anti_alias(false);
    paint.set_shader(shader);

    let dst_rect = Rect::from_xywh(x, y, w, h);
    canvas.draw_rect(dst_rect, &paint);
}

fn tile_modes_for_fit(fit: ImageFit) -> Option<(TileMode, TileMode)> {
    match fit {
        ImageFit::Repeat => Some((TileMode::Repeat, TileMode::Repeat)),
        ImageFit::RepeatX => Some((TileMode::Repeat, TileMode::Decal)),
        ImageFit::RepeatY => Some((TileMode::Decal, TileMode::Repeat)),
        ImageFit::Contain | ImageFit::Cover => None,
    }
}

fn snap_outset_rect_to_device(canvas: &skia_safe::Canvas, rect: Rect) -> Rect {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return rect;
    }

    let matrix = canvas.local_to_device_as_3x3();
    let (device_rect, _) = matrix.map_rect(rect);
    if !device_rect.left().is_finite()
        || !device_rect.top().is_finite()
        || !device_rect.right().is_finite()
        || !device_rect.bottom().is_finite()
    {
        return rect;
    }

    let snapped_device = Rect::from_ltrb(
        device_rect.left().floor(),
        device_rect.top().floor(),
        device_rect.right().ceil(),
        device_rect.bottom().ceil(),
    );

    let Some(inv) = matrix.invert() else {
        return rect;
    };

    let (mapped_back, _) = inv.map_rect(snapped_device);
    if !mapped_back.left().is_finite()
        || !mapped_back.top().is_finite()
        || !mapped_back.right().is_finite()
        || !mapped_back.bottom().is_finite()
    {
        return rect;
    }

    Rect::from_ltrb(
        mapped_back.left().min(rect.left()),
        mapped_back.top().min(rect.top()),
        mapped_back.right().max(rect.right()),
        mapped_back.bottom().max(rect.bottom()),
    )
}

fn rect_outset_amount(original: Rect, expanded: Rect) -> (f32, f32) {
    let outset_x = (original.left() - expanded.left())
        .max(expanded.right() - original.right())
        .max(0.0);
    let outset_y = (original.top() - expanded.top())
        .max(expanded.bottom() - original.bottom())
        .max(0.0);
    (outset_x, outset_y)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ImageFitRects {
    src_x: f32,
    src_y: f32,
    src_w: f32,
    src_h: f32,
    dst_x: f32,
    dst_y: f32,
    dst_w: f32,
    dst_h: f32,
}

fn compute_image_fit_rects(
    src_w: f32,
    src_h: f32,
    dst_x: f32,
    dst_y: f32,
    dst_w: f32,
    dst_h: f32,
    fit: ImageFit,
) -> Option<ImageFitRects> {
    let values = [src_w, src_h, dst_x, dst_y, dst_w, dst_h];
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }

    if src_w <= 0.0 || src_h <= 0.0 || dst_w <= 0.0 || dst_h <= 0.0 {
        return None;
    }

    match fit {
        ImageFit::Contain => {
            let scale = (dst_w / src_w).min(dst_h / src_h);
            if !scale.is_finite() || scale <= 0.0 {
                return None;
            }

            let draw_w = (src_w * scale).clamp(0.0, dst_w);
            let draw_h = (src_h * scale).clamp(0.0, dst_h);
            let draw_x = dst_x + (dst_w - draw_w) * 0.5;
            let draw_y = dst_y + (dst_h - draw_h) * 0.5;

            Some(ImageFitRects {
                src_x: 0.0,
                src_y: 0.0,
                src_w,
                src_h,
                dst_x: draw_x,
                dst_y: draw_y,
                dst_w: draw_w,
                dst_h: draw_h,
            })
        }
        ImageFit::Cover => {
            let scale = (dst_w / src_w).max(dst_h / src_h);
            if !scale.is_finite() || scale <= 0.0 {
                return None;
            }

            let crop_w = (dst_w / scale).clamp(0.0, src_w);
            let crop_h = (dst_h / scale).clamp(0.0, src_h);
            if crop_w <= 0.0 || crop_h <= 0.0 {
                return None;
            }

            let crop_x = ((src_w - crop_w) * 0.5).clamp(0.0, src_w - crop_w);
            let crop_y = ((src_h - crop_h) * 0.5).clamp(0.0, src_h - crop_h);

            Some(ImageFitRects {
                src_x: crop_x,
                src_y: crop_y,
                src_w: crop_w,
                src_h: crop_h,
                dst_x,
                dst_y,
                dst_w,
                dst_h,
            })
        }
        ImageFit::Repeat | ImageFit::RepeatX | ImageFit::RepeatY => None,
    }
}

fn draw_image_loading(canvas: &skia_safe::Canvas, x: f32, y: f32, w: f32, h: f32) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let rect = Rect::from_xywh(x, y, w, h);

    let mut bg = Paint::default();
    bg.set_anti_alias(true);
    bg.set_color(Color::from_argb(255, 44, 48, 58));
    canvas.draw_rect(rect, &bg);

    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as f32)
        .unwrap_or(0.0);

    let period = 1400.0;
    let phase = (millis % period) / period;
    let band_w = (w * 0.35).max(24.0);
    let band_x = x - band_w + (w + band_w * 2.0) * phase;

    let shimmer_rect = Rect::from_xywh(band_x, y, band_w, h);
    let mut shimmer = Paint::default();
    shimmer.set_anti_alias(true);
    shimmer.set_color(Color::from_argb(130, 130, 140, 160));

    canvas.save();
    canvas.clip_rect(rect, skia_safe::ClipOp::Intersect, true);
    canvas.draw_rect(shimmer_rect, &shimmer);
    canvas.restore();
}

fn draw_image_failed(canvas: &skia_safe::Canvas, x: f32, y: f32, w: f32, h: f32) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let rect = Rect::from_xywh(x, y, w, h);

    let mut bg = Paint::default();
    bg.set_anti_alias(true);
    bg.set_color(Color::from_argb(255, 56, 38, 45));
    canvas.draw_rect(rect, &bg);

    let stroke = (w.min(h) * 0.08).clamp(1.0, 6.0);

    let mut line = Paint::default();
    line.set_anti_alias(true);
    line.set_style(PaintStyle::Stroke);
    line.set_stroke_width(stroke);
    line.set_color(Color::from_argb(230, 232, 190, 200));

    let inset = stroke * 1.6;
    let x0 = x + inset;
    let y0 = y + inset;
    let x1 = x + w - inset;
    let y1 = y + h - inset;

    canvas.draw_line((x0, y0), (x1, y1), &line);
    canvas.draw_line((x1, y0), (x0, y1), &line);
}

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1.0e-3
}

fn resolve_inset_pair(start: f32, end: f32, total: f32) -> (f32, f32) {
    let start = start.max(0.0);
    let end = end.max(0.0);
    let total = total.max(0.0);

    let sum = start + end;
    if sum <= total || sum <= f32::EPSILON {
        (start, end)
    } else {
        let scale = total / sum;
        (start * scale, end * scale)
    }
}

fn corner_rrect(rect: Rect, corners: [f32; 4]) -> RRect {
    let max_rx = rect.width().max(0.0) * 0.5;
    let max_ry = rect.height().max(0.0) * 0.5;

    let radii = [
        Point::new(
            corners[0].max(0.0).min(max_rx),
            corners[0].max(0.0).min(max_ry),
        ),
        Point::new(
            corners[1].max(0.0).min(max_rx),
            corners[1].max(0.0).min(max_ry),
        ),
        Point::new(
            corners[2].max(0.0).min(max_rx),
            corners[2].max(0.0).min(max_ry),
        ),
        Point::new(
            corners[3].max(0.0).min(max_rx),
            corners[3].max(0.0).min(max_ry),
        ),
    ];

    if radii.iter().all(|p| p.x <= 0.0 && p.y <= 0.0) {
        RRect::new_rect(rect)
    } else {
        RRect::new_rect_radii(rect, &radii)
    }
}

fn border_band_path(outer_rrect: RRect, inner_rrect: Option<RRect>) -> skia_safe::Path {
    let mut builder = PathBuilder::new_with_fill_type(PathFillType::EvenOdd);
    builder.add_rrect(outer_rrect, None, None);
    if let Some(inner) = inner_rrect {
        builder.add_rrect(inner, None, None);
    }
    builder.detach()
}

fn quad_path(quad: [(f32, f32); 4]) -> skia_safe::Path {
    PathBuilder::new()
        .move_to(Point::new(quad[0].0, quad[0].1))
        .line_to(Point::new(quad[1].0, quad[1].1))
        .line_to(Point::new(quad[2].0, quad[2].1))
        .line_to(Point::new(quad[3].0, quad[3].1))
        .close()
        .detach()
}

fn draw_border(canvas: &skia_safe::Canvas, spec: BorderDrawSpec) {
    let RectSpec { x, y, w, h } = spec.rect;
    let corners = spec.corners;
    let EdgeInsets {
        top,
        right,
        bottom,
        left,
    } = spec.insets;
    let color = spec.color;
    let style = spec.style;

    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let (left, right) = resolve_inset_pair(left, right, w);
    let (top, bottom) = resolve_inset_pair(top, bottom, h);

    if top <= 0.0 && right <= 0.0 && bottom <= 0.0 && left <= 0.0 {
        return;
    }

    let outer_rect = Rect::from_xywh(x, y, w, h);
    let outer_rrect = corner_rrect(outer_rect, corners);

    let outer_tl = outer_rrect.radii(skia_safe::rrect::Corner::UpperLeft);
    let outer_tr = outer_rrect.radii(skia_safe::rrect::Corner::UpperRight);
    let outer_br = outer_rrect.radii(skia_safe::rrect::Corner::LowerRight);
    let outer_bl = outer_rrect.radii(skia_safe::rrect::Corner::LowerLeft);

    let inner_x = x + left;
    let inner_y = y + top;
    let inner_w = (w - left - right).max(0.0);
    let inner_h = (h - top - bottom).max(0.0);

    let inner_rrect = if inner_w > 0.0 && inner_h > 0.0 {
        let inner_rect = Rect::from_xywh(inner_x, inner_y, inner_w, inner_h);
        let max_rx = inner_w * 0.5;
        let max_ry = inner_h * 0.5;
        let radii = [
            Point::new(
                (outer_tl.x - left).max(0.0).min(max_rx),
                (outer_tl.y - top).max(0.0).min(max_ry),
            ),
            Point::new(
                (outer_tr.x - right).max(0.0).min(max_rx),
                (outer_tr.y - top).max(0.0).min(max_ry),
            ),
            Point::new(
                (outer_br.x - right).max(0.0).min(max_rx),
                (outer_br.y - bottom).max(0.0).min(max_ry),
            ),
            Point::new(
                (outer_bl.x - left).max(0.0).min(max_rx),
                (outer_bl.y - bottom).max(0.0).min(max_ry),
            ),
        ];

        Some(if radii.iter().all(|p| p.x <= 0.0 && p.y <= 0.0) {
            RRect::new_rect(inner_rect)
        } else {
            RRect::new_rect_radii(inner_rect, &radii)
        })
    } else {
        None
    };

    let band_path = border_band_path(outer_rrect, inner_rrect);

    match style {
        BorderStyle::Solid => {
            let mut fill_paint = Paint::default();
            fill_paint.set_color(color_from_u32(color));
            fill_paint.set_anti_alias(true);
            canvas.draw_path(&band_path, &fill_paint);
        }
        BorderStyle::Dashed | BorderStyle::Dotted => {
            let mut stroke_paint = Paint::default();
            stroke_paint.set_color(color_from_u32(color));
            stroke_paint.set_style(PaintStyle::Stroke);
            stroke_paint.set_anti_alias(true);

            let uniform =
                approx_eq(top, right) && approx_eq(right, bottom) && approx_eq(bottom, left);

            if uniform {
                let width = top;
                if width <= 0.0 {
                    return;
                }
                stroke_paint.set_stroke_width(width * 2.0);
                apply_border_style(&mut stroke_paint, style, width);
                canvas.save();
                canvas.clip_path(&band_path, skia_safe::ClipOp::Intersect, true);
                canvas.draw_rrect(outer_rrect, &stroke_paint);
                canvas.restore();
            } else {
                let edge_clips = border_edge_clip_quads(spec.rect, spec.insets);
                for (width, quad) in &edge_clips {
                    if *width <= 0.0 {
                        continue;
                    }

                    let edge_path = quad_path(*quad);

                    canvas.save();
                    canvas.clip_path(&band_path, skia_safe::ClipOp::Intersect, true);
                    canvas.clip_path(&edge_path, skia_safe::ClipOp::Intersect, false);
                    stroke_paint.set_stroke_width(*width * 2.0);
                    apply_border_style(&mut stroke_paint, style, *width);
                    canvas.draw_rrect(outer_rrect, &stroke_paint);
                    canvas.restore();
                }
            }
        }
    }
}

fn apply_border_style(paint: &mut Paint, style: BorderStyle, stroke_width: f32) {
    match style {
        BorderStyle::Solid => {}
        BorderStyle::Dashed => {
            let segment = (stroke_width * 3.0).max(4.0);
            let gap = (stroke_width * 2.0).max(3.0);
            if let Some(effect) = dash_path_effect::new(&[segment, gap], 0.0) {
                paint.set_path_effect(effect);
            }
        }
        BorderStyle::Dotted => {
            let dot = stroke_width.max(1.0);
            let gap = (stroke_width * 1.5).max(2.0);
            if let Some(effect) = dash_path_effect::new(&[dot, gap], 0.0) {
                paint.set_path_effect(effect);
            }
        }
    }
}

pub fn color_from_u32(c: u32) -> Color {
    // RGBA format: 0xRRGGBBAA
    let r = ((c >> 24) & 0xFF) as u8;
    let g = ((c >> 16) & 0xFF) as u8;
    let b = ((c >> 8) & 0xFF) as u8;
    let a = (c & 0xFF) as u8;
    Color::from_argb(a, r, g, b)
}

/// Compute the four clip polygons used by `BorderEdges` rendering.
///
/// The clips are quads that split corners at the CSS-style inner-join points
/// derived from per-edge insets.
///
/// Returns `[(stroke_width, [(x,y); 4]); 4]` in top/right/bottom/left order.
fn border_edge_clip_quads(rect: RectSpec, insets: EdgeInsets) -> [(f32, [(f32, f32); 4]); 4] {
    let RectSpec { x, y, w, h } = rect;
    let EdgeInsets {
        top,
        right,
        bottom,
        left,
    } = insets;

    let (left, right) = resolve_inset_pair(left, right, w);
    let (top, bottom) = resolve_inset_pair(top, bottom, h);

    let join_tl = (x + left, y + top);
    let join_tr = (x + w - right, y + top);
    let join_br = (x + w - right, y + h - bottom);
    let join_bl = (x + left, y + h - bottom);

    let margin = top.max(right).max(bottom).max(left) * 2.0 + 20.0;

    [
        (
            top,
            [
                (x - margin, y - margin),
                (x + w + margin, y - margin),
                join_tr,
                join_tl,
            ],
        ),
        (
            right,
            [
                (x + w + margin, y - margin),
                (x + w + margin, y + h + margin),
                join_br,
                join_tr,
            ],
        ),
        (
            bottom,
            [
                (x + w + margin, y + h + margin),
                (x - margin, y + h + margin),
                join_bl,
                join_br,
            ],
        ),
        (
            left,
            [
                (x - margin, y + h + margin),
                (x - margin, y - margin),
                join_tl,
                join_bl,
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point_in_convex_polygon(p: (f32, f32), vertices: &[(f32, f32)]) -> bool {
        const EPS: f32 = 1.0e-4;

        let mut sign = 0i8;
        for i in 0..vertices.len() {
            let a = vertices[i];
            let b = vertices[(i + 1) % vertices.len()];
            let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);

            if cross.abs() <= EPS {
                continue;
            }

            let current_sign = if cross > 0.0 { 1 } else { -1 };
            if sign == 0 {
                sign = current_sign;
            } else if sign != current_sign {
                return false;
            }
        }

        true
    }

    fn render_commands_to_pixels(width: u32, height: u32, commands: Vec<DrawCmd>) -> Vec<u8> {
        let info = skia_safe::ImageInfo::new(
            (width as i32, height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let surface = skia_safe::surfaces::raster(&info, None, None)
            .expect("raster surface should be created for renderer test");

        let mut renderer = Renderer::from_surface(surface);
        let state = RenderState {
            commands,
            clear_color: Color::TRANSPARENT,
            render_version: 1,
            animate: false,
        };
        renderer.render(&state);

        let mut pixels = vec![0u8; (width * height * 4) as usize];
        renderer.surface_mut().read_pixels(
            &info,
            pixels.as_mut_slice(),
            (width * 4) as usize,
            (0, 0),
        );
        pixels
    }

    fn render_single_command_to_pixels(width: u32, height: u32, cmd: DrawCmd) -> Vec<u8> {
        render_commands_to_pixels(width, height, vec![cmd])
    }

    fn rgba_at(pixels: &[u8], width: u32, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let idx = ((y * width + x) * 4) as usize;
        (
            pixels[idx],
            pixels[idx + 1],
            pixels[idx + 2],
            pixels[idx + 3],
        )
    }

    fn cache_test_image(id: &str, width: u32, height: u32, rgba_pixels: Vec<u8>) {
        let info = skia_safe::ImageInfo::new(
            (width as i32, height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let data = Data::new_copy(&rgba_pixels);
        let image = skia_safe::images::raster_from_data(&info, data, (width * 4) as usize)
            .expect("test image should be created from RGBA pixels");

        let mut cache = get_image_cache()
            .lock()
            .expect("image cache lock for test image insertion");
        cache.insert(
            id.to_string(),
            Arc::new(CachedImage {
                image,
                width,
                height,
            }),
        );
    }

    fn max_alpha_in_region(pixels: &[u8], width: u32, x0: u32, y0: u32, x1: u32, y1: u32) -> u8 {
        let mut max_alpha = 0u8;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let idx = ((y * width + x) * 4 + 3) as usize;
                max_alpha = max_alpha.max(pixels[idx]);
            }
        }
        max_alpha
    }

    fn assert_close(actual: f32, expected: f32, label: &str) {
        assert!(
            approx_eq(actual, expected),
            "{} expected {:.6}, got {:.6}",
            label,
            expected,
            actual
        );
    }

    fn point_in_rounded_rect(
        px: f32,
        py: f32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
    ) -> bool {
        if w <= 0.0 || h <= 0.0 {
            return false;
        }

        let r = radius.max(0.0).min((w * 0.5).min(h * 0.5));
        let left = x;
        let right = x + w;
        let top = y;
        let bottom = y + h;

        if px < left || px > right || py < top || py > bottom {
            return false;
        }

        if r <= 0.0 {
            return true;
        }

        if (px >= left + r && px <= right - r) || (py >= top + r && py <= bottom - r) {
            return true;
        }

        let cx = if px < left + r { left + r } else { right - r };
        let cy = if py < top + r { top + r } else { bottom - r };
        let dx = px - cx;
        let dy = py - cy;
        dx * dx + dy * dy <= r * r
    }

    fn point_in_inset_rounded_rect(
        px: f32,
        py: f32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        inset: f32,
    ) -> bool {
        let inset = inset.max(0.0);
        let inset_x = x + inset;
        let inset_y = y + inset;
        let inset_w = (w - inset * 2.0).max(0.0);
        let inset_h = (h - inset * 2.0).max(0.0);
        let inset_r = (radius - inset).max(0.0);
        point_in_rounded_rect(px, py, inset_x, inset_y, inset_w, inset_h, inset_r)
    }

    #[test]
    fn test_compute_image_fit_rects_contain_wide_frame() {
        let rects =
            compute_image_fit_rects(640.0, 420.0, 0.0, 0.0, 280.0, 120.0, ImageFit::Contain)
                .expect("contain fit rects");

        assert_close(rects.src_x, 0.0, "src_x");
        assert_close(rects.src_y, 0.0, "src_y");
        assert_close(rects.src_w, 640.0, "src_w");
        assert_close(rects.src_h, 420.0, "src_h");
        assert_close(rects.dst_w, 182.85715, "dst_w");
        assert_close(rects.dst_h, 120.0, "dst_h");
        assert_close(rects.dst_x, 48.571426, "dst_x");
        assert_close(rects.dst_y, 0.0, "dst_y");
    }

    #[test]
    fn test_compute_image_fit_rects_cover_wide_frame() {
        let rects = compute_image_fit_rects(640.0, 420.0, 0.0, 0.0, 280.0, 120.0, ImageFit::Cover)
            .expect("cover fit rects");

        assert_close(rects.src_x, 0.0, "src_x");
        assert_close(rects.src_y, 72.85715, "src_y");
        assert_close(rects.src_w, 640.0, "src_w");
        assert_close(rects.src_h, 274.2857, "src_h");
        assert_close(rects.dst_x, 0.0, "dst_x");
        assert_close(rects.dst_y, 0.0, "dst_y");
        assert_close(rects.dst_w, 280.0, "dst_w");
        assert_close(rects.dst_h, 120.0, "dst_h");
    }

    #[test]
    fn test_compute_image_fit_rects_contain_tall_frame() {
        let rects =
            compute_image_fit_rects(640.0, 420.0, 0.0, 0.0, 140.0, 240.0, ImageFit::Contain)
                .expect("contain fit rects");

        assert_close(rects.dst_x, 0.0, "dst_x");
        assert_close(rects.dst_y, 74.0625, "dst_y");
        assert_close(rects.dst_w, 140.0, "dst_w");
        assert_close(rects.dst_h, 91.875, "dst_h");
    }

    #[test]
    fn test_compute_image_fit_rects_cover_tall_frame() {
        let rects = compute_image_fit_rects(640.0, 420.0, 0.0, 0.0, 140.0, 240.0, ImageFit::Cover)
            .expect("cover fit rects");

        assert_close(rects.src_x, 197.5, "src_x");
        assert_close(rects.src_y, 0.0, "src_y");
        assert_close(rects.src_w, 245.0, "src_w");
        assert_close(rects.src_h, 420.0, "src_h");
        assert_close(rects.dst_w, 140.0, "dst_w");
        assert_close(rects.dst_h, 240.0, "dst_h");
    }

    #[test]
    fn test_compute_image_fit_rects_cover_square_frame() {
        let rects = compute_image_fit_rects(640.0, 420.0, 0.0, 0.0, 180.0, 180.0, ImageFit::Cover)
            .expect("cover fit rects");

        assert_close(rects.src_x, 110.0, "src_x");
        assert_close(rects.src_y, 0.0, "src_y");
        assert_close(rects.src_w, 420.0, "src_w");
        assert_close(rects.src_h, 420.0, "src_h");
        assert_close(rects.dst_w, 180.0, "dst_w");
        assert_close(rects.dst_h, 180.0, "dst_h");
    }

    #[test]
    fn test_compute_image_fit_rects_rejects_invalid_dimensions() {
        assert!(
            compute_image_fit_rects(0.0, 420.0, 0.0, 0.0, 180.0, 180.0, ImageFit::Contain)
                .is_none()
        );
        assert!(
            compute_image_fit_rects(640.0, 420.0, 0.0, 0.0, 0.0, 180.0, ImageFit::Contain)
                .is_none()
        );
        assert!(
            compute_image_fit_rects(640.0, 420.0, f32::NAN, 0.0, 180.0, 180.0, ImageFit::Cover)
                .is_none()
        );
    }

    #[test]
    fn test_cover_fit_avoids_edge_bleed_from_outside_crop() {
        let image_id = "test_cover_edge_bleed";

        // 8x4 image: red | green-center | blue
        let mut src = vec![0u8; 8 * 4 * 4];
        for y in 0..4u32 {
            for x in 0..8u32 {
                let i = ((y * 8 + x) * 4) as usize;
                let (r, g, b) = if x < 2 {
                    (255, 0, 0)
                } else if x < 6 {
                    (0, 220, 0)
                } else {
                    (0, 0, 255)
                };
                src[i] = r;
                src[i + 1] = g;
                src[i + 2] = b;
                src[i + 3] = 255;
            }
        }

        cache_test_image(image_id, 8, 4, src);

        // Square destination forces cover crop to center 4 columns (green-only).
        let pixels = render_commands_to_pixels(
            32,
            32,
            vec![DrawCmd::Image(
                8.0,
                8.0,
                15.0,
                15.0,
                image_id.to_string(),
                ImageFit::Cover,
            )],
        );

        for (x, y) in &[(8, 14), (8, 16), (8, 18), (22, 14), (22, 16), (22, 18)] {
            let (r, g, b, a) = rgba_at(&pixels, 32, *x, *y);
            assert!(
                g >= 150,
                "expected green edge pixel at ({}, {}) without crop bleed, got rgba({}, {}, {}, {})",
                x,
                y,
                r,
                g,
                b,
                a
            );
            assert!(
                r <= 90 && b <= 90,
                "expected low red/blue crop bleed at ({}, {}), got rgba({}, {}, {}, {})",
                x,
                y,
                r,
                g,
                b,
                a
            );
        }

        remove_image(image_id);
    }

    #[test]
    fn test_repeat_fit_tiles_both_axes() {
        let image_id = "test_repeat_fit_tiles_both_axes";

        // 2x2 source pattern:
        // [red, green]
        // [blue, yellow]
        let src = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ];
        cache_test_image(image_id, 2, 2, src);

        let pixels = render_commands_to_pixels(
            8,
            8,
            vec![DrawCmd::Image(
                0.0,
                0.0,
                8.0,
                8.0,
                image_id.to_string(),
                ImageFit::Repeat,
            )],
        );

        assert_eq!(rgba_at(&pixels, 8, 0, 0), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 8, 1, 0), (0, 255, 0, 255));
        assert_eq!(rgba_at(&pixels, 8, 0, 1), (0, 0, 255, 255));
        assert_eq!(rgba_at(&pixels, 8, 1, 1), (255, 255, 0, 255));

        // Repeat period is source dimensions (2x2)
        assert_eq!(rgba_at(&pixels, 8, 0, 0), rgba_at(&pixels, 8, 2, 0));
        assert_eq!(rgba_at(&pixels, 8, 0, 0), rgba_at(&pixels, 8, 0, 2));
        assert_eq!(rgba_at(&pixels, 8, 1, 1), rgba_at(&pixels, 8, 3, 3));

        remove_image(image_id);
    }

    #[test]
    fn test_repeat_x_fit_tiles_horizontally_only() {
        let image_id = "test_repeat_x_fit_tiles_horizontally_only";

        let src = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ];
        cache_test_image(image_id, 2, 2, src);

        let pixels = render_commands_to_pixels(
            20,
            20,
            vec![DrawCmd::Image(
                4.0,
                5.0,
                8.0,
                8.0,
                image_id.to_string(),
                ImageFit::RepeatX,
            )],
        );

        // Horizontal repetition exists in the top tile row.
        assert_eq!(rgba_at(&pixels, 20, 4, 5), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 20, 6, 5), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 20, 5, 6), (255, 255, 0, 255));

        // Outside image height should be transparent (no Y repeat).
        assert_eq!(rgba_at(&pixels, 20, 5, 9).3, 0);
        assert_eq!(rgba_at(&pixels, 20, 11, 12).3, 0);

        remove_image(image_id);
    }

    #[test]
    fn test_repeat_y_fit_tiles_vertically_only() {
        let image_id = "test_repeat_y_fit_tiles_vertically_only";

        let src = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ];
        cache_test_image(image_id, 2, 2, src);

        let pixels = render_commands_to_pixels(
            20,
            20,
            vec![DrawCmd::Image(
                4.0,
                5.0,
                8.0,
                8.0,
                image_id.to_string(),
                ImageFit::RepeatY,
            )],
        );

        // Vertical repetition exists in the left tile column.
        assert_eq!(rgba_at(&pixels, 20, 4, 5), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 20, 4, 7), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 20, 5, 8), (255, 255, 0, 255));

        // Outside image width should be transparent (no X repeat).
        assert_eq!(rgba_at(&pixels, 20, 7, 6).3, 0);
        assert_eq!(rgba_at(&pixels, 20, 12, 12).3, 0);

        remove_image(image_id);
    }

    #[test]
    fn test_fractional_scale_cover_border_has_no_dark_inner_hairline() {
        let image_id = "test_fractional_cover_hairline";

        // Opaque, high-contrast source color to make dark seams visible.
        let mut src = vec![0u8; 24 * 16 * 4];
        for px in src.chunks_exact_mut(4) {
            px[0] = 36;
            px[1] = 216;
            px[2] = 72;
            px[3] = 255;
        }
        cache_test_image(image_id, 24, 16, src);

        // Fractional geometry mirrors a 1.5x-scaled layout.
        // Choose values that keep the inner clip on half-pixel boundaries.
        let outer_x: f32 = 12.0;
        let outer_y: f32 = 8.0;
        let outer_w: f32 = 54.0;
        let outer_h: f32 = 38.0;
        let border: f32 = 1.5;
        let radius: f32 = 9.0;

        let inner_x = outer_x + border;
        let inner_y = outer_y + border;
        let inner_w = outer_w - border * 2.0;
        let inner_h = outer_h - border * 2.0;
        let inner_r = (radius - border).max(0.0);

        let render_scene = |background_color: u32| {
            render_commands_to_pixels(
                80,
                60,
                vec![
                    DrawCmd::RoundedRect(
                        outer_x,
                        outer_y,
                        outer_w,
                        outer_h,
                        radius,
                        background_color,
                    ),
                    DrawCmd::PushClipRounded(outer_x, outer_y, outer_w, outer_h, radius),
                    DrawCmd::PushClipRoundedHard(inner_x, inner_y, inner_w, inner_h, inner_r),
                    DrawCmd::Image(
                        inner_x,
                        inner_y,
                        inner_w,
                        inner_h,
                        image_id.to_string(),
                        ImageFit::Cover,
                    ),
                    DrawCmd::PopClip,
                    DrawCmd::Border(
                        outer_x,
                        outer_y,
                        outer_w,
                        outer_h,
                        radius,
                        border,
                        0xD6DCECDD,
                        BorderStyle::Solid,
                    ),
                    DrawCmd::PopClip,
                ],
            )
        };

        let dark_bg_pixels = render_scene(0x05070BFF);
        let bright_bg_pixels = render_scene(0xF5E87AFF);

        let mut band_count = 0usize;
        let mut changed_count = 0usize;
        let mut max_channel_diff = 0u8;

        for y in 0..60u32 {
            for x in 0..80u32 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;

                // Only inspect a thin band *inside* the inner clip edge.
                let in_inner_near_edge = point_in_inset_rounded_rect(
                    px, py, inner_x, inner_y, inner_w, inner_h, inner_r, 0.05,
                );
                let in_inner_deep = point_in_inset_rounded_rect(
                    px, py, inner_x, inner_y, inner_w, inner_h, inner_r, 1.25,
                );

                if !(in_inner_near_edge && !in_inner_deep) {
                    continue;
                }

                let (dr, dg, db, _da) = rgba_at(&dark_bg_pixels, 80, x, y);
                let (br, bg, bb, _ba) = rgba_at(&bright_bg_pixels, 80, x, y);

                let d_r = dr.abs_diff(br);
                let d_g = dg.abs_diff(bg);
                let d_b = db.abs_diff(bb);
                let local_max = d_r.max(d_g).max(d_b);
                max_channel_diff = max_channel_diff.max(local_max);

                band_count += 1;
                if local_max > 8 {
                    changed_count += 1;
                }
            }
        }

        assert!(band_count > 0, "expected non-empty inner edge band");
        assert!(
            max_channel_diff <= 8,
            "expected inner edge pixels to be background-invariant (no hairline leak), max channel diff was {}",
            max_channel_diff
        );
        assert!(
            changed_count <= 2,
            "expected <=2 significantly changed inner-edge pixels, got {} of {}",
            changed_count,
            band_count
        );

        remove_image(image_id);
    }

    #[test]
    fn test_border_edge_clips_do_not_overlap_at_corners() {
        // Element at (10, 20) with size 200x100 and asymmetric widths
        let clips = border_edge_clip_quads(
            RectSpec {
                x: 10.0,
                y: 20.0,
                w: 200.0,
                h: 100.0,
            },
            EdgeInsets {
                top: 4.0,
                right: 1.0,
                bottom: 4.0,
                left: 1.0,
            },
        );

        // Sample points deep inside each corner quadrant of the element
        let top_left = (20.0, 25.0); // near top-left
        let top_right = (200.0, 25.0); // near top-right
        let bottom_right = (200.0, 115.0); // near bottom-right
        let bottom_left = (20.0, 115.0); // near bottom-left

        let corner_points = [top_left, top_right, bottom_right, bottom_left];

        for point in &corner_points {
            let mut hit_count = 0;
            for (width, quad) in &clips {
                if *width > 0.0 && point_in_convex_polygon(*point, quad) {
                    hit_count += 1;
                }
            }
            assert!(
                hit_count <= 1,
                "corner point {:?} is inside {} clip regions (expected at most 1)",
                point,
                hit_count,
            );
        }
    }

    #[test]
    fn test_border_edge_clips_cover_all_edge_midpoints() {
        let clips = border_edge_clip_quads(
            RectSpec {
                x: 10.0,
                y: 20.0,
                w: 200.0,
                h: 100.0,
            },
            EdgeInsets {
                top: 4.0,
                right: 1.0,
                bottom: 3.0,
                left: 2.0,
            },
        );

        // Midpoints of each edge (on the border, not inside)
        let edge_midpoints: [(f32, f32, &str); 4] = [
            (110.0, 20.0, "top"),     // top midpoint
            (210.0, 70.0, "right"),   // right midpoint
            (110.0, 120.0, "bottom"), // bottom midpoint
            (10.0, 70.0, "left"),     // left midpoint
        ];

        for (i, (px, py, label)) in edge_midpoints.iter().enumerate() {
            let (width, quad) = &clips[i];
            assert!(
                *width > 0.0 && point_in_convex_polygon((*px, *py), quad),
                "{} edge midpoint ({}, {}) should be inside clip region {}",
                label,
                px,
                py,
                i,
            );
        }
    }

    #[test]
    fn test_border_edge_clips_asymmetric_top_right_corner_prefers_top() {
        // Regression: thick top (4) and thin right (1) should keep near-corner
        // top-band pixels owned by the top edge clip.
        let clips = border_edge_clip_quads(
            RectSpec {
                x: 10.0,
                y: 20.0,
                w: 200.0,
                h: 100.0,
            },
            EdgeInsets {
                top: 4.0,
                right: 1.0,
                bottom: 4.0,
                left: 1.0,
            },
        );

        // Very close to the top-right outer corner, but still inside top band.
        let p = (209.0, 21.8);

        let top_hit = point_in_convex_polygon(p, &clips[0].1);
        let right_hit = point_in_convex_polygon(p, &clips[1].1);

        assert!(
            top_hit,
            "top clip should include near-corner top-band point"
        );
        assert!(
            !right_hit,
            "right clip should not steal near-corner top-band point"
        );
    }

    #[test]
    fn test_border_edge_clips_bottom_only_covers_near_corners() {
        // bottom-only border: top=0, right=0, bottom=3, left=0
        let clips = border_edge_clip_quads(
            RectSpec {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 50.0,
            },
            EdgeInsets {
                top: 0.0,
                right: 0.0,
                bottom: 3.0,
                left: 0.0,
            },
        );

        assert_eq!(clips[0].0, 0.0, "top width should be 0");
        assert_eq!(clips[1].0, 0.0, "right width should be 0");
        assert_eq!(clips[2].0, 3.0, "bottom width should be 3");
        assert_eq!(clips[3].0, 0.0, "left width should be 0");

        // Bottom midpoint and near-corner points should stay in bottom clip.
        let bottom_mid = (50.0, 50.0);
        let bottom_left = (1.0, 49.0);
        let bottom_right = (99.0, 49.0);
        assert!(
            point_in_convex_polygon(bottom_mid, &clips[2].1),
            "bottom edge midpoint should be inside the bottom clip region",
        );
        assert!(
            point_in_convex_polygon(bottom_left, &clips[2].1),
            "bottom-left near-corner point should be inside the bottom clip region",
        );
        assert!(
            point_in_convex_polygon(bottom_right, &clips[2].1),
            "bottom-right near-corner point should be inside the bottom clip region",
        );
    }

    #[test]
    fn test_solid_border_edges_asymmetric_keeps_top_right_corner_covered() {
        // Regression: with top=4 and right=1 on rounded corners, a point near
        // the outer top-right arc should remain filled (no corner gap).
        let pixels = render_single_command_to_pixels(
            160,
            100,
            DrawCmd::BorderEdges(
                20.0,
                20.0,
                100.0,
                40.0,
                8.0,
                4.0,
                1.0,
                4.0,
                1.0,
                0x78C8A0FF,
                BorderStyle::Solid,
            ),
        );

        // Near top-right arc location inside the border band.
        let corner_alpha = max_alpha_in_region(&pixels, 160, 116, 22, 117, 23);
        assert!(
            corner_alpha >= 96,
            "expected top-right arc coverage alpha >= 96, got {}",
            corner_alpha
        );

        // Ensure interior remains unfilled for border-only drawing.
        let interior_alpha = max_alpha_in_region(&pixels, 160, 60, 38, 61, 39);
        assert!(
            interior_alpha <= 8,
            "expected interior alpha <= 8, got {}",
            interior_alpha
        );
    }

    #[test]
    fn test_all_border_styles_stay_inside_border_box() {
        for style in [BorderStyle::Solid, BorderStyle::Dashed, BorderStyle::Dotted] {
            let pixels = render_single_command_to_pixels(
                180,
                120,
                DrawCmd::BorderEdges(
                    20.0, 20.0, 100.0, 40.0, 8.0, 4.0, 1.0, 4.0, 1.0, 0x78C8A0FF, style,
                ),
            );

            let outside_right_alpha = max_alpha_in_region(&pixels, 180, 121, 24, 123, 56);
            assert!(
                outside_right_alpha <= 8,
                "expected no paint outside right edge for {:?}, got alpha {}",
                style,
                outside_right_alpha
            );

            let outside_top_alpha = max_alpha_in_region(&pixels, 180, 24, 17, 116, 19);
            assert!(
                outside_top_alpha <= 8,
                "expected no paint outside top edge for {:?}, got alpha {}",
                style,
                outside_top_alpha
            );

            let interior_alpha = max_alpha_in_region(&pixels, 180, 68, 36, 72, 40);
            assert!(
                interior_alpha <= 8,
                "expected no interior fill for {:?}, got alpha {}",
                style,
                interior_alpha
            );
        }
    }
}
