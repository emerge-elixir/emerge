use super::*;
use crate::raster_assets::{BitmapRef, Source};
use skia_safe::{
    Canvas, FilterMode, MipmapMode, PathBuilder, RRect, Rect as SkRect, SamplingOptions,
    canvas::SrcRectConstraint, paint::Style,
};

/// Prepared samples and geometry. Pending/failed indicators have more than
/// one paint operation and do not qualify for direct bitmap opacity.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Sample {
    pub source: Source,
    // Keep xywh until binding: subtracting SkRect edges to recover a crop
    // width can move a strict sampling boundary by one texel after scaling.
    pub src: Rect,
    pub dst: SkRect,
    pub clip: SkRect,
    pub relaxed_masks: u8,
    pub alpha: u8,
}
impl Sample {
    pub fn new(
        e: &Elements,
        k: Key,
        ctx: PaintContext,
        relaxed_masks: u8,
        fits: &mut fits::Fits,
    ) -> Option<Self> {
        let a = &e.attrs[k];
        let [t, r, b, l] = a.sizing.padding.map(|n| scale::pixels(n, ctx.scale));
        let f = ctx.frame;
        let (w, h) = (f.width - l - r, f.height - t - b);
        if w <= 0. || h <= 0. {
            return None;
        }
        let (x, y) = (f.x + l, f.y + t);
        let rect = SkRect::from_xywh(x, y, w, h);
        let source = e.images.source(k);
        let own_clip = a.image_host_clip;
        let relaxed = own_clip || e.slots[k].parent.is_some();
        let mut clip = sk(ctx.child_clip);
        if own_clip && !clip.intersect(sk(a.decor.inner_rect(f, ctx.scale)).with_outset((0.5, 0.5)))
        {
            return None;
        }
        let mut dst = rect;
        let mut src = Rect::default();
        let mut target = None;
        if let Some((iw, ih)) = source.dimensions() {
            let (iw, ih) = (iw as f32, ih as f32);
            let factor = if a.image_cover {
                (w / iw).max(h / ih)
            } else {
                (w / iw).min(h / ih)
            };
            if a.image_cover {
                let (cw, ch) = ((w / factor).min(iw), (h / factor).min(ih));
                src = Rect {
                    x: (iw - cw) * 0.5,
                    y: (ih - ch) * 0.5,
                    width: cw,
                    height: ch,
                };
            } else {
                let (dw, dh) = ((iw * factor).min(w), (ih * factor).min(h));
                dst = SkRect::from_xywh(x + (w - dw) * 0.5, y + (h - dh) * 0.5, dw, dh);
                src = Rect {
                    width: iw,
                    height: ih,
                    ..Rect::default()
                };
            }
            if let Source::Sized(record) = source {
                // The full fitted image in device coordinates, before adding
                // bleed. A cover crop needs more texels than the visible slot.
                let full = if a.image_cover {
                    let (fw, fh) = (iw * factor, ih * factor);
                    SkRect::from_xywh(x + (w - fw) * 0.5, y + (h - fh) * 0.5, fw, fh)
                } else {
                    dst
                };
                if !full.is_finite() {
                    return None;
                }
                let width = full.width().ceil().clamp(1., iw) as i32;
                let height = full.height().ceil().clamp(1., ih) as i32;
                target = Some((record, (width, height).into()));
            }
            if relaxed {
                let fills = |a: f32, b: f32| (a - b).abs() <= 0.01;
                let dx = fills(dst.left, rect.left) && fills(dst.right, rect.right);
                let dy = fills(dst.top, rect.top) && fills(dst.bottom, rect.bottom);
                dst.outset((u8::from(dx) as f32, u8::from(dy) as f32));
            }
        } else if relaxed {
            dst.outset((1., 1.));
        }
        if !dst.intersects(clip) {
            return None;
        }
        if let Some((record, size)) = target {
            fits.add(k, record, size);
        }
        Some(Self {
            source: source.clone(),
            src,
            dst,
            clip,
            relaxed_masks,
            alpha: 255,
        })
    }
    pub(super) fn bind(&mut self, bitmap: &BitmapRef, intrinsic: skia_safe::ISize) {
        let actual = bitmap.image().dimensions();
        let (sx, sy) = (
            actual.width as f32 / intrinsic.width as f32,
            actual.height as f32 / intrinsic.height as f32,
        );
        self.src.x *= sx;
        self.src.width *= sx;
        self.src.y *= sy;
        self.src.height *= sy;
        self.source = Source::Ready(bitmap.clone());
    }
    pub fn draw(&self, canvas: &Canvas, clips: &Clips, mask: Mask) {
        canvas.save();
        // Rasterize the AA mask inside the rectangular device clip. Reversing
        // these operations changes edge coverage even at opacity one.
        canvas.clip_rect(self.clip, None, false);
        clips.apply(mask, canvas, self.relaxed_masks);
        match &self.source {
            Source::Ready(b) => {
                #[cfg(test)]
                opacity::bitmap_drawn();
                let mut p = Paint::default();
                p.set_alpha(self.alpha);
                canvas.draw_image_rect_with_sampling_options(
                    b.image(),
                    Some((&sk(self.src), SrcRectConstraint::Strict)),
                    self.dst,
                    SamplingOptions::new(FilterMode::Linear, MipmapMode::None),
                    &p,
                );
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
