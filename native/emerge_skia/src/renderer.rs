//! Backend-agnostic Skia renderer.
//!
//! This module contains:
//! - `DrawCmd` enum and decoder for Elixir terms
//! - `RenderState` for holding commands between frames
//! - `Renderer` struct that executes draw commands on a Skia surface
//! - Font cache for text rendering

use std::sync::OnceLock;

use rustler::{Atom, Decoder, Error as RustlerError, Term};
use skia_safe::{
    Color, ColorType, Font, FontMgr, FontStyle, Paint, PaintStyle, Point, RRect, Rect, Shader,
    Surface, TileMode, Typeface, Vector,
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

#[derive(Clone, Debug)]
pub enum DrawCmd {
    Clear(u32),
    Rect(f32, f32, f32, f32, u32),
    RoundedRect(f32, f32, f32, f32, f32, u32),
    RoundedRectCorners(f32, f32, f32, f32, f32, f32, f32, f32, u32),
    Border(f32, f32, f32, f32, f32, f32, u32),
    BorderCorners(f32, f32, f32, f32, f32, f32, f32, f32, f32, u32),
    Text(f32, f32, String, f32, u32),
    Gradient(f32, f32, f32, f32, u32, u32, f32),
    PushClip(f32, f32, f32, f32),
    PopClip,
    Translate(f32, f32),
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
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            commands: Vec::new(),
            clear_color: Color::WHITE,
        }
    }
}

// ============================================================================
// Font Cache
// ============================================================================

static DEFAULT_TYPEFACE: OnceLock<Typeface> = OnceLock::new();

pub fn get_default_typeface() -> &'static Typeface {
    DEFAULT_TYPEFACE.get_or_init(|| {
        let font_mgr = FontMgr::new();
        font_mgr
            .match_family_style("sans-serif", FontStyle::normal())
            .or_else(|| font_mgr.match_family_style("DejaVu Sans", FontStyle::normal()))
            .or_else(|| font_mgr.match_family_style("", FontStyle::normal()))
            .expect("No fonts available on system")
    })
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

                DrawCmd::PopClip => {
                    canvas.restore();
                }

                DrawCmd::Translate(x, y) => {
                    canvas.translate(Vector::new(*x, *y));
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
