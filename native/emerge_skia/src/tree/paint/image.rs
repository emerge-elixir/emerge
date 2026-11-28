use super::*;
use crate::raster_assets::{BitmapRef, Source};
use skia_safe::{
    Canvas, FilterMode, MipmapMode, PathBuilder, RRect, Rect as SkRect, SamplingOptions,
    canvas::SrcRectConstraint, paint::Style,
};

/// Source crop, destination and full fitted extent (before any bitmap bleed).
/// A cover crop can require more texels than the slot; a repeated tile never does
/// more decode work just because the slot is large.
struct Placement {
    src: Rect,
    dst: SkRect,
    full: SkRect,
}
impl ImageFit {
    fn place(self, slot: Rect, iw: f32, ih: f32) -> Placement {
        let src = Rect {
            width: iw,
            height: ih,
            ..Rect::default()
        };
        let (w, h) = (slot.width, slot.height);
        let centered =
            |dw, dh| SkRect::from_xywh(slot.x + (w - dw) * 0.5, slot.y + (h - dh) * 0.5, dw, dh);
        match self {
            Self::Cover => {
                let factor = (w / iw).max(h / ih);
                let (cw, ch) = ((w / factor).min(iw), (h / factor).min(ih));
                Placement {
                    src: Rect {
                        x: (iw - cw) * 0.5,
                        y: (ih - ch) * 0.5,
                        width: cw,
                        height: ch,
                    },
                    dst: sk(slot),
                    full: centered(iw * factor, ih * factor),
                }
            }
            Self::Contain => {
                let factor = (w / iw).min(h / ih);
                let dst = centered((iw * factor).min(w), (ih * factor).min(h));
                Placement {
                    src,
                    dst,
                    full: dst,
                }
            }
            Self::Repeat | Self::RepeatX | Self::RepeatY => Placement {
                src,
                dst: sk(slot),
                full: sk(src),
            },
        }
    }
}

// A sample has either a source/crop or a retained binding, never both. In
// particular, a sealed local bitmap need not pin its compressed input.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Pixels {
    Direct(fits::Bound),
    Fitted(fits::Slot),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Bleed {
    None,
    Host,
    Entry,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Sample {
    pub pixels: Pixels,
    pub fit: ImageFit,
    pub dst: SkRect,
    pub clip: SkRect,
    pub relaxed_masks: u8,
    // A retained call may be below an authored pose in another recording.
    pub antialias: bool,
    // The unrelaxed host must still intersect the current exposure before its
    // half-pixel sampling allowance applies. Never relax an empty clip.
    pub host_clip: Rect,
    pub bleed: Bleed,
    pub alpha: u8,
    pub tint: Option<Brush>,
}
impl Sample {
    pub fn new(
        e: &Elements,
        k: Key,
        ctx: PaintContext,
        (plane, antialias): (Option<Plane>, bool),
        relaxed_masks: u8,
        gradient: Option<u32>,
        fits: &mut fits::Fits,
    ) -> Option<Self> {
        let a = &e.attrs[k];
        let leaf = e.slots[k].shape == Shape::Image;
        let [t, r, b, l] = if leaf {
            a.sizing.padding.map(|n| scale::pixels(n, ctx.scale))
        } else {
            [0.; 4]
        }; // backgrounds cover the border box
        let f = ctx.frame;
        let (w, h) = (f.width - l - r, f.height - t - b);
        if w <= 0. || h <= 0. {
            return None;
        }
        let (x, y) = (f.x + l, f.y + t);
        let slot = Rect {
            x,
            y,
            width: w,
            height: h,
        };
        let rect = sk(slot);
        let source = e.images.source(k);
        let own_clip = leaf && a.image_host_clip;
        // The half-pixel clip relaxation and extra texel at a fitted edge are
        // bitmap sampling rules. They must not enlarge vector geometry.
        let picture = matches!(source, Source::Picture(_));
        let relaxed = leaf && !picture && (own_clip || e.layout_parent(k).is_some());
        let pad = if fits.recording.is_some() {
            let v = ctx.zoom.recip();
            if v.is_finite() { (v, v) } else { (0., 0.) }
        } else {
            plane.map_or((1., 1.), |p| pose::allowance(p.matrix, 1.))
        };
        let bleed = if picture {
            (0., 0.)
        } else {
            (0.5 * pad.0, 0.5 * pad.1)
        };
        let zoom = ctx.zoom;
        let mut clip = if !leaf {
            sk(ctx.clip)
        } else if a.posed() {
            let borrowed = !own_clip
                && e.layout_parent(k)
                    .is_some_and(|p| !e.attrs[p].scroll.port() && e.attrs[p].clips_media());
            sk(ctx.child_clip).with_outset(if borrowed || picture { bleed } else { pad })
        } else {
            sk(if picture { ctx.clip } else { ctx.child_clip })
        };
        if own_clip && !clip.intersect(sk(a.decor.inner_rect(f, ctx.scale)).with_outset(bleed)) {
            return None;
        }
        let mut dst = rect;
        let mut src = Rect::default();
        let mut target = None;
        if let Some((iw, ih)) = source.dimensions() {
            let placement = a.image_fit.place(slot, iw, ih);
            (src, dst) = (placement.src, placement.dst);
            if let Source::Sized(record) = source {
                let full = placement.full;
                if !full.is_finite() {
                    return None;
                }
                let width = (full.width() * zoom).ceil().clamp(1., iw) as i32;
                let height = (full.height() * zoom).ceil().clamp(1., ih) as i32;
                target = Some((record, (width, height).into()));
            }
            if relaxed {
                let fills = |a: f32, b: f32| (a - b).abs() <= 0.01;
                let dx = fills(dst.left, rect.left) && fills(dst.right, rect.right);
                let dy = fills(dst.top, rect.top) && fills(dst.bottom, rect.bottom);
                dst.outset((if dx { pad.0 } else { 0. }, if dy { pad.1 } else { 0. }));
            }
        } else if relaxed {
            dst.outset(pad);
        }
        let mut visible = dst;
        if !visible.intersect(clip) || plane.is_some_and(|p| p.bounds(visible).is_empty()) {
            return None;
        }
        let pixels = if let Some(slot) = target.and_then(|(r, size)| fits.sample(k, r, size, src)) {
            Pixels::Fitted(slot)
        } else if let Source::Picture(art) = source {
            let fits = if a.image_fit.tiled() {
                art.fits_tile(zoom) && art.fits(zoom, zoom)
            } else {
                art.fits(
                    dst.width() / src.width * zoom,
                    dst.height() / src.height * zoom,
                )
            };
            Pixels::Direct(fits::Bound {
                source: if fits { source.clone() } else { Source::Failed },
                src,
            })
        } else {
            Pixels::Direct(fits::Bound {
                source: source.clone(),
                src,
            })
        };
        let bleed = if !relaxed || a.posed() {
            Bleed::None
        } else if fits.recording == Some(k)
            || e.layout_parent(k).is_some_and(|p| e.attrs[p].scroll.port())
        {
            Bleed::Entry
        } else {
            Bleed::Host
        };
        Some(Self {
            pixels,
            antialias,
            host_clip: ctx.clip,
            bleed,
            fit: a.image_fit,
            tint: a
                .svg_color
                .map(Brush::Solid)
                .or_else(|| gradient.map(|i| Brush::Svg(i, slot))),
            dst,
            clip,
            relaxed_masks: if picture { 0 } else { relaxed_masks },
            alpha: 255,
        })
    }
    pub(super) fn limit(&self, view: pose::Aperture) -> Rect {
        match self.bleed {
            Bleed::None => view.body,
            _ if self.host_clip.intersection(view.body).is_none() => Rect::default(),
            Bleed::Entry => view.media,
            Bleed::Host => view.body,
        }
    }
    // Pending/failed indicators draw multiple primitives, unlike a bitmap.
    pub(super) fn is_bitmap(&self) -> bool {
        match &self.pixels {
            Pixels::Fitted(_) => true,
            Pixels::Direct(b) => matches!(b.source, Source::Ready(_) | Source::Sized(_)),
        }
    }
    pub fn draw(
        &self,
        canvas: &Canvas,
        clips: &Clips,
        mask: Mask,
        plane: Option<Plane>,
        materials: &Materials,
    ) {
        canvas.save();
        // Rasterize the AA mask inside the rectangular device clip. Reversing
        // these operations changes edge coverage even at opacity one.
        canvas.clip_rect(self.clip, None, false);
        clips.apply(
            mask,
            plane.map_or(OPEN, |p| p.stop),
            canvas,
            self.relaxed_masks,
            pose::allowance(canvas.local_to_device_as_3x3(), 0.5),
        );
        let b = match &self.pixels {
            Pixels::Direct(b) => b,
            Pixels::Fitted(slot) => slot.get().expect("sealed image binding"),
        };
        let (source, src) = (&b.source, b.src);
        if self.fit.tiled() && self.draw_tiles(canvas, source, src) {
            canvas.restore();
            return;
        }
        match source {
            Source::Ready(b) => {
                #[cfg(test)]
                opacity::bitmap_drawn();
                let mut p = Paint::default();
                p.set_alpha(self.alpha).set_anti_alias(self.antialias);
                canvas.draw_image_rect_with_sampling_options(
                    b.image(),
                    Some((&sk(src), SrcRectConstraint::Strict)),
                    self.dst,
                    SamplingOptions::new(FilterMode::Linear, MipmapMode::None),
                    &p,
                );
            }
            Source::Picture(art) => {
                canvas.clip_rect(self.dst, None, false);
                // Tint replaces the RGB of the *composited* source, not each
                // component path. The gradient's coordinates are the content box.
                if self.tint.is_some() {
                    canvas.save_layer_alpha(self.dst, 255);
                }
                canvas.save();
                canvas.translate((self.dst.left, self.dst.top));
                canvas.scale((self.dst.width() / src.width, self.dst.height() / src.height));
                canvas.translate((-src.x, -src.y));
                canvas.draw_picture(art.picture(), None, None);
                canvas.restore();
                if let Some(brush) = self.tint {
                    let mut p = materials.paint(brush);
                    p.set_blend_mode(skia_safe::BlendMode::SrcIn);
                    canvas.draw_paint(&p);
                    canvas.restore();
                }
            }
            Source::Sized(_) => unreachable!("fitted samples must be bound before sealing"),
            Source::Pending => {
                let r = self.dst;
                let size = r.width().min(r.height()).min(24.);
                let paint = indicator(220, 148, 163, 184);
                for offset in [-1., 0., 1.] {
                    canvas.draw_circle(
                        (r.center_x() + offset * size / 3., r.center_y()),
                        size / 12.,
                        &paint,
                    );
                }
            }
            Source::Failed => failed(canvas, self.dst),
        }
        canvas.restore();
    }
    // One shader operation even for a one-pixel tile in a large box. A
    // picture shader references the existing recording, never the SVG bytes.
    fn draw_tiles(&self, canvas: &Canvas, source: &Source, src: Rect) -> bool {
        use skia_safe::{
            Matrix,
            TileMode::{Decal, Repeat},
        };
        let modes = match self.fit {
            ImageFit::Repeat => (Repeat, Repeat),
            ImageFit::RepeatX => (Repeat, Decal),
            ImageFit::RepeatY => (Decal, Repeat),
            _ => return false,
        };
        let matrix = Matrix::translate((self.dst.left, self.dst.top));
        let shader = match source {
            Source::Ready(b) => {
                b.image()
                    .to_shader(modes, SamplingOptions::from(FilterMode::Linear), &matrix)
            }
            Source::Picture(art) => {
                Some(
                    art.picture()
                        .to_shader(modes, FilterMode::Linear, &matrix, &sk(src)),
                )
            }
            _ => return false, // normal pending/failed indicator
        };
        if let Some(shader) = shader {
            let mut paint = Paint::default();
            paint.set_shader(shader).set_alpha(self.alpha);
            canvas.draw_rect(self.dst, &paint);
        } else {
            failed(canvas, self.dst);
        }
        true
    }
}
pub(super) fn bound_crop(src: Rect, bitmap: &BitmapRef, intrinsic: skia_safe::ISize) -> Rect {
    let actual = bitmap.image().dimensions();
    let (sx, sy) = (
        actual.width as f32 / intrinsic.width as f32,
        actual.height as f32 / intrinsic.height as f32,
    );
    Rect {
        x: src.x * sx,
        y: src.y * sy,
        width: src.width * sx,
        height: src.height * sy,
    }
}
fn indicator(a: u8, r: u8, g: u8, b: u8) -> Paint {
    let mut p = Paint::default();
    p.set_anti_alias(true)
        .set_color(Color::from_argb(a, r, g, b));
    p
}
fn failed(canvas: &Canvas, r: SkRect) {
    let side = r.width().min(r.height());
    canvas.draw_rect(r, &indicator(255, 254, 242, 242));
    if side >= 28. {
        let w = (side * 0.38).clamp(18., 52.);
        let h = w * 0.72;
        let x = r.x() + (r.width() - w) * 0.5;
        let y = r.y() + (r.height() - h) * 0.5;
        let mut p = indicator(210, 248, 113, 113);
        p.set_style(Style::Stroke)
            .set_stroke_width((side * 0.018).clamp(1., 2.));
        canvas.draw_rrect(
            RRect::new_rect_xy(SkRect::from_xywh(x, y, w, h), 3., 3.),
            &p,
        );
        canvas.draw_circle((x + w * 0.72, y + h * 0.28), (w * 0.055).max(1.2), &p);
        let mut path = PathBuilder::new();
        path.move_to((x + w * 0.18, y + h * 0.74));
        for (dx, dy) in [(0.40, 0.52), (0.54, 0.65), (0.68, 0.48), (0.84, 0.74)] {
            path.line_to((x + w * dx, y + h * dy));
        }
        canvas.draw_path(&path.detach(), &p);
    }
    let stroke = (side * 0.08).clamp(1., 6.);
    let mut p = indicator(230, 220, 38, 38);
    p.set_style(Style::Stroke).set_stroke_width(stroke);
    let inset = stroke * 1.6;
    canvas.draw_line(
        (r.left + inset, r.top + inset),
        (r.right - inset, r.bottom - inset),
        &p,
    );
    canvas.draw_line(
        (r.right - inset, r.top + inset),
        (r.left + inset, r.bottom - inset),
        &p,
    );
}
