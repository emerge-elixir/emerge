use super::*;

pub(super) struct Image;
impl ShapeMeta for Image {
    const TAG: u8 = 9;
    type Payload = Self;
    type Paint = Self;
}
impl Payload for Image {
    type Data<'a> = Mounted<'a, Self>;
    const MAX_CHILDREN: usize = 0;
    fn get(e: &Elements, k: Key) -> Mounted<'_, Self> {
        Mounted::new(e, k)
    }
    fn decode(tag: u8, r: &mut wire::Reader<'_>, raw: &[u8], a: &mut Attrs) -> Result<(), String> {
        match tag {
            53 if !a.has_art() => a.image_src = r.artwork(raw, true)?,
            63 => (a.svg_color, a.svg_gradient) = r.ink(raw)?,
            64 => a.svg_expected = r.flag()?,
            54 => a.image_fit = r.image_fit(false)?,
            _ => return Err(format!("unsupported image attribute or source {tag}")),
        }
        Ok(())
    }
}
fn projected(extent: f32, to: f32, from: f32) -> f32 {
    (f64::from(extent.max(0.)) * f64::from(to) / f64::from(from)).min(f64::from(f32::MAX)) as f32
}
// The same unallotted demands are available in Context (for independent
// Column slots) and in Measure. Unknown art supplies extents, never a ratio.
fn natural(e: &Elements, k: Key) -> Demand {
    let a = &e.attrs[k];
    let size = e.images.source(k).dimensions();
    let (mut w, mut h) = size.unwrap_or((64., 64.));
    if size.is_some() {
        let b = a.sizing;
        let s = e.slots[k].admitted.scale;
        let [t, r, d, l] = b.padding.map(|v| scale::pixels(v, s));
        match a.image_axes {
            [ImageAxis::Pixel, ImageAxis::Content] => {
                h = projected(b.width.resolve(0., || s) - (l + r), h, w)
            }
            [ImageAxis::Content, ImageAxis::Pixel] => {
                w = projected(b.height.resolve(0., || s) - (t + d), w, h)
            }
            _ => (),
        }
    }
    Demand { w, h, min_w: w }
}
fn unallotted_height(e: &Elements, k: Key, inner: f32) -> f32 {
    let b = e.attrs[k].sizing;
    let s = e.slots[k].admitted.scale;
    let inset = scale::pixels(b.padding[0], s) + scale::pixels(b.padding[2], s);
    b.height.resolve(inner + inset, || s)
}
pub(super) fn independent_height(e: &Elements, k: Key) -> f32 {
    unallotted_height(e, k, natural(e, k).h)
}
fn projected_width(e: &Elements, k: Key, height: f32) -> Option<f32> {
    let (iw, ih) = e.images.source(k).dimensions()?;
    let s = e.slots[k].admitted.scale;
    let b = e.attrs[k].sizing;
    Some(projected(
        height - (scale::pixels(b.padding[0], s) + scale::pixels(b.padding[2], s)),
        iw,
        ih,
    ))
}
pub(in crate::tree) fn width_at_height(e: &Elements, k: Key, height: f32) -> Option<f32> {
    let b = e.attrs[k].sizing;
    let s = e.slots[k].admitted.scale;
    let w = projected_width(e, k, height)?;
    Some(b.width.resolve(
        w + (scale::pixels(b.padding[1], s) + scale::pixels(b.padding[3], s)),
        || s,
    ))
}
impl LayoutBehaviour for Image {
    fn measure(node: Mounted<'_, Self>, _: &mut Measuring<'_>) -> Demand {
        natural(node.elements, node.key)
    }
    fn reflow(node: Mounted<'_, Self>, cx: &mut ReflowCx<'_>) -> Heights {
        let b = node.attrs().sizing;
        if b.height_from_width()
            && let Some((iw, ih)) = node.elements.images.source(node.key).dimensions()
        {
            cx.box_height(projected(cx.inner_width(node.key), ih, iw))
                .into()
        } else {
            cx.measured_height.into()
        }
    }
}
impl PaintBehaviour<Image> for Image {
    fn attrs_changed(a: &Attrs, b: &Attrs) -> bool {
        a.background != b.background
            || a.svg_color != b.svg_color
            || a.image_host_clip != b.image_host_clip
            || a.sizing.padding != b.sizing.padding
    }
    fn reads_context(_: &Attrs, old: Option<PaintContext>, ctx: PaintContext) -> bool {
        old.is_none_or(|p| {
            p.frame != ctx.frame
                || p.clip != ctx.clip
                || p.child_clip != ctx.child_clip
                || p.scale != ctx.scale
        })
    }
}
