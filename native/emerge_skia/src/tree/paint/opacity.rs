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

// The own program contains at most one child-order call. Its extent is an
// input, not part of the immutable own ink. A child edit can therefore change
// the layer size without rewriting the parent's program or visiting its rows.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub(super) struct Extent {
    fixed: SkRect,
    children: Option<usize>,
}
impl Extent {
    pub(super) fn resolved(self, d: &Drawing, kids: SkRect) -> SkRect {
        let mut r = self.fixed;
        if let Some(i) = self.children {
            let (s, _) = &d.commands[i];
            r.join(
                d.planes
                    .get(s.plane)
                    .map_or(kids, |p| p.matrix.map_rect(kids).0),
            );
        }
        r
    }
}
impl Drawing {
    pub(super) fn begin_group(&mut self, alpha: u8) -> usize {
        let start = self.commands.len();
        self.push(Command::Group {
            alpha,
            bounds: Extent::default(),
            end: 0,
        });
        start
    }
    pub(super) fn end_group(&mut self, start: usize) {
        let Command::Group { alpha, .. } = self.commands[start].1 else {
            unreachable!()
        };
        let end = self.commands.len();
        if end == start + 1 {
            self.commands.edit().pop();
            return;
        }
        if end == start + 2
            && matches!(&self.commands[start+1].1, Command::Image(s) if s.is_bitmap())
        {
            let (mask, Command::Image(mut bitmap)) = self.commands.edit().pop().unwrap() else {
                unreachable!()
            };
            bitmap.alpha = ((u16::from(bitmap.alpha) * u16::from(alpha) + 127) / 255) as u8;
            self.commands[start] = (mask, Command::Image(bitmap));
            return;
        }
        let (plane, bounds) = self.extent(start + 1..end);
        self.commands[start] = (
            Scope {
                plane,
                ..Default::default()
            },
            Command::Group { alpha, bounds, end },
        );
        self.push(Command::Restore);
    }
    // A group whose contents share a translated plane can keep its tight
    // local extent. Resolve that one plane at replay, not a whole list of
    // commands. In particular a small escaping panel must not acquire a
    // viewport-sized alpha layer merely because its row moves.
    pub(super) fn extent(&self, range: Range<usize>) -> (u32, Extent) {
        let mut bounds = SkRect::default();
        let mut children = None;
        let mut plane = 0;
        let mut i = range.start;
        while i < range.end {
            let (scope, cmd) = &self.commands[i];
            let extent = match cmd {
                Command::Order {
                    call: Call::Children,
                    ..
                } => {
                    children = Some(i);
                    i += 1;
                    continue;
                }
                Command::Order {
                    defer_clips: true,
                    call: Call::Window(_) | Call::Segment(..),
                    ..
                } => sk(Rect::UNBOUNDED),
                Command::Order { bounds, .. } => *bounds,
                Command::Fill(rect, _)
                | Command::ScrollThumb(_, rect, _, _)
                | Command::Thumb(rect, _) => sk(*rect).with_outset((1., 1.)),
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
                    children = children.or(bounds.children);
                    bounds.fixed
                }
                Command::Restore => unreachable!("child extent includes its restore"),
            };
            let (id, extent) = self.planes.get(scope.plane).map_or((0, extent), |p| {
                if p.moving {
                    (scope.plane, extent)
                } else if matches!(
                    cmd,
                    Command::Order {
                        defer_clips: true,
                        ..
                    }
                ) || cmd.outer_shadow(&self.materials).is_some()
                {
                    (0, p.matrix.map_rect(extent).0)
                } else {
                    (0, p.bounds(extent))
                }
            });
            if !extent.is_finite() || (!bounds.is_empty() && !extent.is_empty() && id != plane) {
                return (
                    0,
                    Extent {
                        fixed: sk(Rect::UNBOUNDED),
                        children: None,
                    },
                );
            }
            if !extent.is_empty() {
                plane = id;
                bounds.join(extent);
            }
            i += 1;
        }
        (
            plane,
            Extent {
                fixed: bounds,
                children,
            },
        )
    }
}
// Establish the group's outward-rounded device extent just once. Mapping a
// device AABB back through a rotated recording, then letting saveLayer map
// that AABB forward again, creates a different sized surface at the AA edge.
pub(super) fn open(canvas: &Canvas, bounds: SkRect, alpha: u8) -> bool {
    let base = canvas.local_to_device();
    let device = canvas.local_to_device_as_3x3().map_rect(bounds).0;
    canvas.save();
    canvas.reset_matrix();
    if let Some(area) = visible(canvas, device) {
        canvas.save_layer_alpha(area, u32::from(alpha));
        canvas.set_matrix(&base);
        #[cfg(test)]
        record(area);
        true
    } else {
        canvas.restore();
        false
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
