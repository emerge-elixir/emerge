//! Immutable paint inputs owned by a candidate drawing. The variable part of
//! a gradient or shadow list is copied once, not once per continuation/word.
use super::*;
use crate::tree::{
    decoration::{Bytes, Decor},
    wire::Reader,
};
use skia_safe::{
    BlurStyle, Canvas, ClipOp, MaskFilter, PathBuilder, PathFillType, RRect, Rect as SkRect,
    TileMode,
    gradient::{Colors, Gradient, Interpolation},
    paint::Style,
    shaders,
};
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Brush {
    Solid(u32),
    Font(u32, Rect),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Phase {
    Under,
    Border,
}
#[derive(Clone, Copy, PartialEq)]
struct Material {
    decor: Decor,
    background: Option<u32>,
    font: Bytes,
    scale: f32,
}
#[derive(Default, PartialEq)]
pub(super) struct Materials {
    styles: Vec<Material>,
    bytes: Vec<u8>,
}
impl Materials {
    pub fn reserve(&mut self, styles: usize, bytes: usize, b: Budget) -> Result<(), String> {
        if styles > self.styles.capacity() {
            pool::reserve(&mut self.styles, b.capacity(styles))?;
        }
        if bytes > self.bytes.capacity() {
            pool::reserve(&mut self.bytes, b.capacity(bytes))?;
        }
        Ok(())
    }
    pub fn clear(&mut self) {
        self.styles.clear();
        self.bytes.clear();
    }
    fn add(&mut self, e: &Elements, k: Key) -> u32 {
        let a = &e.attrs[k];
        let mut copy = |span: Bytes| {
            if span.len == 0 {
                return Bytes::default();
            }
            let start = self.bytes.len() as u32;
            self.bytes.extend_from_slice(span.get(e.raw(k)));
            Bytes {
                start,
                len: span.len,
            }
        };
        let decor = Decor {
            background_gradient: copy(a.decor.background_gradient),
            border_gradient: copy(a.decor.border_gradient),
            shadows: copy(a.decor.shadows),
            ..a.decor
        };
        let font = copy(a.font_gradient);
        let idx = self.styles.len() as u32;
        self.styles.push(Material {
            decor,
            background: a.background,
            font,
            scale: e.slots[k].admitted.scale,
        });
        idx
    }
    pub fn paint(&self, b: Brush) -> Paint {
        match b {
            Brush::Solid(c) => solid(c),
            Brush::Font(i, r) => {
                let m = self.styles[i as usize];
                ink(None, m.font, &self.bytes, r)
            }
        }
    }
    pub fn paint_bounds(&self, i: u32, r: Rect, phase: Phase, clip: Rect) -> SkRect {
        let m = self.styles[i as usize];
        // Each of offset, spread and blur is <= max_shadow. Include both
        // offset and spread, three blur sigmas (sigma = blur / 2), and AA.
        if phase == Phase::Under && m.decor.shadows.len != 0 {
            let outset = 4. * scale::pixels(m.decor.max_shadow, m.scale) + 2.;
            sk(r).with_outset((outset, outset))
        } else {
            let mut area = sk(r).with_outset((1., 1.));
            if area.intersect(sk(clip)) {
                area
            } else {
                SkRect::default()
            }
        }
    }
    pub fn outer_shadow(&self, canvas: &Canvas, i: u32, r: Rect) {
        let m = self.styles[i as usize];
        let raw = &self.bytes;
        draw_shadows(
            canvas,
            m.decor.shadows.get(raw),
            m.scale,
            r,
            m.decor.radii(m.scale),
            false,
        );
    }
    pub fn draw(&self, canvas: &Canvas, i: u32, r: Rect, clip: Rect, phase: Phase) {
        let m = self.styles[i as usize];
        let d = m.decor;
        let raw = &self.bytes;
        let corners = d.radius.map(|v| scale::pixels(v, m.scale));
        let outer = rounded(r, corners);
        let shadows = d.shadows.get(raw);
        // Outer shadows escape ordinary host clips; the surface still clips.
        // Future scroll boundaries belong here, not in the line-breaking code.
        canvas.save();
        canvas.clip_rect(sk(clip), ClipOp::Intersect, false);
        if phase != Phase::Border {
            if m.background.is_some() || d.background_gradient.len != 0 {
                let p = ink(m.background, d.background_gradient, raw, r);
                if corners == [0.; 4] {
                    canvas.draw_rect(sk(r), &p);
                } else {
                    canvas.save();
                    canvas.clip_rrect(outer, ClipOp::Intersect, true);
                    canvas.draw_rect(sk(r), &p);
                    canvas.restore();
                }
            }
            draw_shadows(canvas, shadows, m.scale, r, corners, true);
        }
        if phase != Phase::Under
            && (d.border_color.is_some() || d.border_gradient.len != 0)
            && d.border != [0.; 4]
        {
            border(
                canvas,
                r,
                outer,
                corners,
                d.border.map(|v| scale::pixels(v, m.scale)),
                d.style,
                ink(d.border_color, d.border_gradient, raw, r),
            );
        }
        canvas.restore();
    }
}
#[derive(Default)]
pub(super) struct MaterialIndex {
    epoch: u64,
    by_owner: SecondaryMap<Key, (u64, u32)>,
}
impl MaterialIndex {
    pub fn reserve(&mut self, n: usize) -> Result<(), String> {
        self.by_owner
            .try_set_capacity(n)
            .map_err(pool::capacity_error)
    }
    pub fn begin(&mut self) {
        self.epoch += 1;
    }
    pub fn get(&mut self, draw: &mut Drawing, e: &Elements, k: Key) -> u32 {
        if let Some(&(stamp, i)) = self.by_owner.get(k)
            && stamp == self.epoch
        {
            return i;
        }
        let i = draw.materials.add(e, k);
        self.by_owner.insert(k, (self.epoch, i));
        i
    }
    pub fn brush(
        &mut self,
        draw: &mut Drawing,
        e: &Elements,
        ctx: PaintContext,
        bounds: Rect,
    ) -> Brush {
        ctx.gradient.map_or(Brush::Solid(ctx.color), |(owner, _)| {
            Brush::Font(self.get(draw, e, owner), bounds)
        })
    }
}
fn sk(r: Rect) -> SkRect {
    SkRect::from_xywh(r.x, r.y, r.width, r.height)
}
fn rgba(v: u32) -> Color {
    let [r, g, b, a] = v.to_be_bytes();
    Color::from_argb(a, r, g, b)
}
fn solid(v: u32) -> Paint {
    let mut p = Paint::default();
    p.set_anti_alias(true);
    p.set_color(rgba(v));
    p
}
fn ink(value: Option<u32>, gradient: Bytes, raw: &[u8], r: Rect) -> Paint {
    if let Some(c) = value {
        return solid(c);
    }
    let mut s = Reader(gradient.get(raw));
    let _ = s.byte().expect("validated gradient");
    let n = s.long().unwrap();
    let colors: Vec<skia_safe::Color4f> = (0..n).map(|_| rgba(s.color().unwrap()).into()).collect();
    if colors.iter().all(|c| *c == colors[0]) {
        let mut p = Paint::default();
        p.set_anti_alias(true);
        p.set_color4f(colors[0], None);
        return p;
    }
    let theta = ((s.signed().unwrap() % 360.) as f32 as f64).to_radians();
    let half = f64::from(r.width).hypot(f64::from(r.height)) * 0.5;
    let x = f64::from(r.x) + f64::from(r.width) * 0.5;
    let y = f64::from(r.y) + f64::from(r.height) * 0.5;
    let dx = half * theta.cos();
    let dy = half * theta.sin();
    let pts = (
        ((x - dx) as f32, (y - dy) as f32),
        ((x + dx) as f32, (y + dy) as f32),
    );
    let gradient = Gradient::new(
        Colors::new_evenly_spaced(&colors, TileMode::Clamp, None),
        Interpolation::default(),
    );
    let mut p = solid(0);
    if let Some(shader) = shaders::linear_gradient(pts, &gradient, None) {
        p.set_color(Color::WHITE);
        p.set_shader(shader);
    }
    p
}
fn rounded(r: Rect, c: [f32; 4]) -> RRect {
    let cap = r.width.min(r.height).max(0.) * 0.5;
    RRect::new_rect_radii(
        sk(r),
        &c.map(|v| {
            let v = v.clamp(0., cap);
            Point::new(v, v)
        }),
    )
}
fn draw_shadows(canvas: &Canvas, raw: &[u8], scale: f32, r: Rect, c: [f32; 4], inset: bool) {
    if raw.is_empty() {
        return;
    }
    let mut s = Reader(raw);
    for _ in 0..s.byte().unwrap() {
        let [ox, oy, blur, size] =
            std::array::from_fn(|_| scale::pixels(s.signed().unwrap(), scale));
        let (v, g) = s.ink(raw).unwrap();
        if (s.byte().unwrap() != 0) != inset {
            continue;
        }
        let mut p = ink(v, g, raw, r);
        if blur > 0. {
            p.set_mask_filter(MaskFilter::blur(BlurStyle::Normal, blur * 0.5, false));
        }
        let size = if inset { -size } else { size };
        let shadow = Rect {
            x: r.x + ox - size,
            y: r.y + oy - size,
            width: (r.width + 2. * size).max(0.),
            height: (r.height + 2. * size).max(0.),
        };
        let rr = rounded(shadow, c.map(|n| (n + size).max(0.)));
        if inset {
            canvas.save();
            canvas.clip_rrect(rounded(r, c), ClipOp::Intersect, true);
            let margin = (blur + size.abs()) * 4. + 100.;
            let mut path = PathBuilder::new_with_fill_type(PathFillType::EvenOdd);
            path.add_rect(sk(r).with_outset((margin, margin)), None, None);
            path.add_rrect(rr, None, None);
            canvas.draw_path(&path.detach(), &p);
            canvas.restore();
        } else {
            canvas.draw_rrect(rr, &p);
        }
    }
}
fn border(
    canvas: &Canvas,
    r: Rect,
    outer: RRect,
    c: [f32; 4],
    widths: [f32; 4],
    style: u8,
    mut p: Paint,
) {
    let [t, ri, b, l] = widths;
    let constrain = |a: f32, b: f32, w: f32| {
        if a + b > w && a + b > 0. {
            w / (a + b)
        } else {
            1.
        }
    };
    let sx = constrain(l, ri, r.width);
    let sy = constrain(t, b, r.height);
    let [t, ri, b, l] = [t * sy, ri * sx, b * sy, l * sx];
    let inner = Rect {
        x: r.x + l,
        y: r.y + t,
        width: (r.width - l - ri).max(0.),
        height: (r.height - t - b).max(0.),
    };
    let ic = [(l, t), (ri, t), (ri, b), (l, b)];
    let cap = r.width.min(r.height) * 0.5;
    let hole = (inner.width > 0. && inner.height > 0.).then(|| {
        RRect::new_rect_radii(
            sk(inner),
            &std::array::from_fn(|i| {
                let rad = c[i].min(cap);
                Point::new(
                    (rad - ic[i].0).clamp(0., inner.width * 0.5),
                    (rad - ic[i].1).clamp(0., inner.height * 0.5),
                )
            }),
        )
    });
    if style == 0 {
        if let Some(h) = hole {
            canvas.draw_drrect(outer, h, &p);
        } else {
            canvas.draw_rrect(outer, &p);
        }
        return;
    }
    let mut band = PathBuilder::new_with_fill_type(PathFillType::EvenOdd);
    band.add_rrect(outer, None, None);
    if let Some(h) = hole {
        band.add_rrect(h, None, None);
    }
    canvas.save();
    canvas.clip_path(&band.detach(), ClipOp::Intersect, true);
    p.set_style(Style::Stroke);
    let pts = |rect: Rect| {
        [
            (rect.x, rect.y),
            (rect.x + rect.width, rect.y),
            (rect.x + rect.width, rect.y + rect.height),
            (rect.x, rect.y + rect.height),
        ]
    };
    let margin = t.max(ri).max(b).max(l) * 2. + 20.;
    let a = pts(Rect {
        x: r.x - margin,
        y: r.y - margin,
        width: r.width + 2. * margin,
        height: r.height + 2. * margin,
    });
    let z = [
        (r.x + l, r.y + t),
        (r.x + r.width - ri, r.y + t),
        (r.x + r.width - ri, r.y + r.height - b),
        (r.x + l, r.y + r.height - b),
    ];
    let same = [ri, b, l].iter().all(|w| (*w - t).abs() <= 0.001);
    for (i, width) in [t, ri, b, l]
        .into_iter()
        .enumerate()
        .filter(|(_, w)| *w > 0.)
    {
        canvas.save();
        if !same {
            let j = (i + 1) % 4;
            let path = PathBuilder::new()
                .move_to(a[i])
                .line_to(a[j])
                .line_to(z[j])
                .line_to(z[i])
                .close()
                .detach();
            canvas.clip_path(&path, ClipOp::Intersect, false);
        }
        p.set_stroke_width(2. * width);
        let dash = if style == 1 {
            [(3. * width).max(4.), (2. * width).max(3.)]
        } else {
            [width.max(1.), (1.5 * width).max(2.)]
        };
        p.set_path_effect(skia_safe::PathEffect::dash(&dash, 0.));
        canvas.draw_rrect(outer, &p);
        canvas.restore();
        if same {
            break;
        }
    }
    canvas.restore();
}
