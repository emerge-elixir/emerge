//! Group delimiters refer into one flat command stream. Nested extents are
//! already known when closing a parent; never copy or rescan a child group.
use super::*;
use skia_safe::{Canvas, IRect, Rect as SkRect, RoundOut};

#[cfg(test)]
#[derive(Default, Clone, Copy)]
struct Calls {
    images: usize,
    layers: usize,
    last: SkRect,
}
#[cfg(test)]
thread_local! { static CALLS: std::cell::Cell<Calls> = Default::default(); }
#[cfg(test)]
pub(super) fn record(bounds: SkRect) {
    let c = CALLS.get();
    CALLS.set(Calls {
        layers: c.layers + 1,
        last: bounds,
        ..c
    });
}
#[cfg(test)]
pub(super) fn bitmap_drawn() {
    let c = CALLS.get();
    CALLS.set(Calls {
        images: c.images + 1,
        ..c
    });
}

impl Drawing {
    pub(super) fn begin_group(&mut self, alpha: u8) -> usize {
        let start = self.commands.len();
        self.push(Command::Group {
            alpha,
            bounds: SkRect::default(),
            end: 0,
        });
        start
    }
    pub(super) fn end_group(&mut self, start: usize, viewport: Rect) {
        let Command::Group { alpha, .. } = self.commands[start].1 else {
            unreachable!()
        };
        let end = self.commands.len();
        if end == start + 1 {
            self.commands.pop();
            return;
        }
        if end == start + 2
            && matches!(&self.commands[start+1].1, Command::Image(s) if matches!(s.source, crate::raster_assets::Source::Ready(_) | crate::raster_assets::Source::Sized(_)))
        {
            let (mask, Command::Image(mut bitmap)) = self.commands.pop().unwrap() else {
                unreachable!()
            };
            bitmap.alpha = ((u16::from(bitmap.alpha) * u16::from(alpha) + 127) / 255) as u8;
            self.commands[start] = (mask, Command::Image(bitmap));
            return;
        }
        let mut bounds = SkRect::default();
        let mut i = start + 1;
        while i < end {
            let (scope, cmd) = &self.commands[i];
            let extent = match cmd {
                Command::Fill(rect, _) => sk(*rect).with_outset((1., 1.)),
                Command::Box {
                    rect,
                    material,
                    phase,
                    clip,
                } => self.materials.paint_bounds(*material, *rect, *phase, *clip),
                Command::Image(sample) => {
                    let mut r = sample.dst.with_outset((1., 1.));
                    if r.intersect(sample.clip.with_outset((1., 1.))) {
                        r
                    } else {
                        SkRect::default()
                    }
                }
                Command::Glyphs { bounds, .. } => sk(*bounds).with_outset((1., 1.)),
                Command::Group { end, bounds, .. } => {
                    i = *end;
                    *bounds
                }
                Command::Restore => unreachable!("child extent includes its restore"),
            };
            let extent = self.planes.get(scope.plane).map_or(extent, |p| {
                if cmd.outer_shadow(&self.materials).is_some() {
                    p.matrix.map_rect(extent).0 // outer shadows escape ordinary host clips
                } else {
                    p.bounds(extent)
                }
            });
            if extent.is_finite() {
                bounds.join(extent);
            } else {
                bounds = sk(viewport);
                break;
            }
            i += 1;
        }
        self.commands[start].1 = Command::Group { alpha, bounds, end };
        self.push(Command::Restore);
    }
}
#[cfg(test)]
mod tests;

/// Skia wants local coordinates, not device pixels. A bound is a hint, not
/// a clip. Round out only after mapping and intersecting the active surface.
pub(super) fn visible(canvas: &Canvas, local: SkRect) -> Option<SkRect> {
    let clip = IRect::intersect(
        &canvas.device_clip_bounds()?,
        &IRect::from_size(canvas.base_layer_size()),
    )?;
    let matrix = canvas.local_to_device_as_3x3();
    let mut device = matrix.map_rect(local).0;
    if !device.is_finite() {
        device = clip.into();
    }
    if !device.intersect(SkRect::from(clip)) {
        return None;
    }
    let outward: SkRect = device.round_out();
    matrix
        .invert()
        .map(|inv| inv.map_rect(outward).0)
        .or_else(|| canvas.local_clip_bounds())
}
