//! A scrollport has one wheel region below its content and chrome above it.
//! Both are in the installed hit stream, not an independently sorted registry.
use super::*;
use skia_safe::Matrix;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Bar {
    track: Rect,
    thumb: Rect,
    pub start: f32,
    pub length: f32,
    pub travel: f32,
}
impl Bar {
    fn new(frame: Rect, axis: usize, view: scroll::View) -> Option<Self> {
        let [size, cross] = if axis == 0 {
            [frame.width, frame.height]
        } else {
            [frame.height, frame.width]
        };
        let range = view.range[axis];
        if range <= 0. || size <= 0. || cross <= 0. {
            return None;
        }
        let length = (f64::from(size).powi(2) / (f64::from(size) + f64::from(range))) as f32;
        let length = length.max(24.).min(size);
        let travel = size - length;
        let at = (view.axes[axis].pos * f64::from(travel)) as f32;
        let thickness = cross.min(if view.hover == Some(axis) { 7. } else { 5. });
        let rect = |offset, len| {
            if axis == 0 {
                Rect {
                    x: frame.x + offset,
                    y: frame.y + cross - thickness,
                    width: len,
                    height: thickness,
                }
            } else {
                Rect {
                    x: frame.x + cross - thickness,
                    y: frame.y + offset,
                    width: thickness,
                    height: len,
                }
            }
        };
        Some(Self {
            track: rect(0., size),
            thumb: rect(at, length),
            start: [frame.x, frame.y][axis],
            length,
            travel,
        })
    }
    pub fn fraction(self, point: f32, grip: f32) -> Option<f64> {
        (self.travel > 0.).then(|| {
            (f64::from(point - self.start - grip * self.length) / f64::from(self.travel))
                .clamp(0., 1.)
        })
    }
}
#[derive(Clone, Copy)]
pub(crate) struct ScrollHit {
    pub key: Attachment,
    pub id: u64,
    pub view: scroll::View,
    parent: u32,
    pub frame: Rect,
    // Captured drags can leave the shape. Keep the absolute inverse too;
    // wheel vectors do not include the translation of its pivot.
    inverse: Option<Matrix>,
}
impl ScrollHit {
    pub(super) fn unprojected(key: Attachment, id: u64, view: scroll::View, frame: Rect) -> Self {
        Self {
            key,
            id,
            view,
            frame,
            parent: 0,
            inverse: None,
        }
    }
    pub fn bar(self, axis: usize) -> Option<Bar> {
        Bar::new(self.frame, axis, self.view)
    }
    pub(super) fn track(self, x: f32, y: f32) -> Option<usize> {
        [0, 1]
            .into_iter()
            .find(|&i| self.bar(i).is_some_and(|b| b.track.contains(x, y)))
    }
    pub fn point(self, x: f32, y: f32) -> [f32; 2] {
        self.inverse.map_or([x, y], |m| {
            let p = m.map_point((x, y));
            [p.x, p.y]
        })
    }
    pub fn vector(self, x: f32, y: f32) -> [f32; 2] {
        self.inverse.map_or([x, y], |m| {
            [
                m.scale_x() * x + m.skew_x() * y,
                m.skew_y() * x + m.scale_y() * y,
            ]
        })
    }
}
#[test]
fn track_thumb_and_inverse_share_the_frame_origin_and_travel() {
    let frame = Rect {
        x: 13.,
        y: 19.,
        width: 100.,
        height: 50.,
    };
    let mut view = scroll::View::default();
    assert!(Bar::new(frame, 1, view).is_none());
    view.range = [100., 150.];
    view.axes[0].pos = 0.5;
    view.axes[1].pos = 1.;
    let x = Bar::new(frame, 0, view).unwrap();
    assert_eq!(
        x.thumb,
        Rect {
            x: 38.,
            y: 64.,
            width: 50.,
            height: 5.
        }
    );
    let y = Bar::new(frame, 1, view).unwrap();
    assert_eq!(
        y.thumb,
        Rect {
            x: 108.,
            y: 45.,
            width: 5.,
            height: 24.
        }
    );
    for bar in [x, y] {
        for f in [0., 0.5, 1.] {
            for grip in [0., 0.25, 1.] {
                assert_eq!(
                    bar.fraction(bar.start + bar.travel * f + bar.length * grip, grip),
                    Some(f64::from(f))
                );
            }
        }
    }
    // Positive but too small for travel; invisible cross-axis makes no track.
    let small = Bar::new(
        Rect {
            height: 10.,
            ..frame
        },
        1,
        view,
    )
    .unwrap();
    assert_eq!(small.length, 10.);
    assert_eq!(small.fraction(30., 0.), None);
    assert!(Bar::new(Rect { width: 0., ..frame }, 1, view).is_none());
}
impl Hits {
    pub(super) fn at(&self, v: Rect, x: f32, y: f32) -> impl Iterator<Item = (&Hit, f32, f32)> {
        let inside = x >= 0. && x < v.width && y >= 0. && y < v.height;
        self.items
            .iter()
            .rev()
            .take(if inside { usize::MAX } else { 0 })
            .filter_map(move |h| {
                let (x, y, stop) =
                    self.planes
                        .point(h.scope.plane, &self.clips, h.scope.before, x, y)?;
                (h.rect.contains(x, y) && self.clips.contains(h.scope.mask, stop, x, y))
                    .then_some((h, x, y))
            })
    }
    pub(super) fn port(
        &mut self,
        e: &Elements,
        k: Key,
        ctx: PaintContext,
        (scope, rect): (Scope, Rect),
        view: scroll::View,
        parent: u32,
    ) -> u32 {
        let key = Attachment(k, e.slots[k].attachment);
        let id = e.slots[k].id;
        self.scrolls.insert(
            key,
            ScrollHit {
                key,
                id,
                view,
                parent,
                frame: ctx.frame,
                inverse: self.planes.get(scope.plane).and_then(|p| p.matrix.invert()),
            },
        );
        let wheel = self.items.len() as u32 + 1;
        self.items.push(Hit {
            kind: HitKind::Port,
            actions: Actions::default(),
            wheel,
            key,
            id,
            scope,
            rect,
            origin: [ctx.frame.x, ctx.frame.y],
            scale: ctx.scale,
        });
        wheel
    }
    pub fn scroll_port(&self, k: Attachment) -> Option<ScrollHit> {
        self.scrolls.get(&k).copied()
    }
    fn enclosing(&self, link: u32) -> Option<ScrollHit> {
        link.checked_sub(1)
            .and_then(|i| self.scroll_port(self.items[i as usize].key))
    }
    pub fn keyboard_ports(&self) -> impl Iterator<Item = ScrollHit> {
        self.items
            .iter()
            .rev()
            .filter(|h| h.kind == HitKind::Port)
            .map(|h| self.scrolls[&h.key])
    }
    pub fn ancestors(&self, first: Option<ScrollHit>) -> impl Iterator<Item = ScrollHit> {
        std::iter::successors(first, |p| self.enclosing(p.parent))
    }
    // Only bubble along the winning region's content ancestry, never into
    // an overlapping sibling behind it. Chrome wins on its track only.
    pub fn region_at(&self, v: Rect, x: f32, y: f32) -> Option<(ScrollHit, Option<usize>)> {
        if self.scrolls.is_empty() {
            return None;
        }
        self.at(v, x, y)
            .find_map(|(h, x, y)| match h.kind {
                HitKind::Bars => {
                    let p = self.scrolls[&h.key];
                    p.track(x, y).map(|a| Some((p, Some(a))))
                }
                _ => Some(self.enclosing(h.wheel).map(|p| (p, None))),
            })
            .flatten()
    }
    pub fn thumb_at(&self, v: Rect, x: f32, y: f32) -> Option<(Attachment, usize)> {
        let (p, Some(axis)) = self.region_at(v, x, y)? else {
            return None;
        };
        let [x, y] = p.point(x, y);
        p.bar(axis)?.thumb.contains(x, y).then_some((p.key, axis))
    }
}
impl Output {
    pub(in crate::tree) fn reserve_scrolls(
        &mut self,
        n: usize,
        owners: usize,
        words: usize,
        b: Budget,
    ) -> Result<(), String> {
        if n != 0 {
            self.draw.reserve([0, 0, 0, 0, 0, 0, 0, n], b)?;
            self.hits
                .reserve([owners + 2 * words + 2 * n, 0, 0, n], b)?;
        }
        Ok(())
    }
    pub(super) fn chrome(&mut self, e: &Elements, k: Key, l: &Layers, side: usize, scope: Scope) {
        let ctx = l.painted(k);
        if side == 0 {
            let draw = &mut self.draw.build;
            draw.scope = scope;
            let view = l.scroll.view(k);
            for bar in [1, 0]
                .into_iter()
                .filter_map(|i| Bar::new(ctx.frame, i, view))
            {
                draw.push(Command::Thumb(bar.thumb, ctx.clip));
            }
        } else if let Some(rect) = ctx.frame.intersection(ctx.clip) {
            self.hits.build.items.push(Hit {
                kind: HitKind::Bars,
                actions: Actions::default(),
                wheel: 0,
                key: Attachment(k, e.slots[k].attachment),
                id: e.slots[k].id,
                scope,
                rect,
                origin: [ctx.frame.x, ctx.frame.y],
                scale: ctx.scale,
            });
        }
    }
}
