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
            53 if r.byte()? == 1 => {
                let path = r.string()?;
                if crate::raster_assets::Format::logical(path).is_none() {
                    return Err("expected a relative logical PNG or JPEG name".into());
                }
                a.image_src = decoration::Bytes::of(raw, path.as_bytes());
            }
            54 => {
                a.image_cover = match r.byte()? {
                    0 => false,
                    1 => true,
                    _ => return Err("expected contain or cover for image".into()),
                }
            }
            _ => return Err(format!("unsupported image attribute or source {tag}")),
        }
        Ok(())
    }
}
fn projected(extent: f32, to: i32, from: i32) -> f32 {
    (f64::from(extent.max(0.)) * f64::from(to) / f64::from(from)).min(f64::from(f32::MAX)) as f32
}
impl LayoutBehaviour for Image {
    fn measure(node: Mounted<'_, Self>, cx: &mut Measuring<'_>) -> Demand {
        let b = node.attrs().sizing;
        let size = node.elements.images.source(node.key).dimensions();
        let (iw, ih) = size.unwrap_or((64, 64));
        // Intrinsic texel dimensions are device extents on main. Only authored
        // box lengths/insets are scaled. An unknown file supplies no aspect ratio.
        let (mut w, mut h) = (iw as f32, ih as f32);
        if size.is_some() {
            let scale = cx.scale();
            let [t, r, d, l] = b.padding.map(|v| scale::pixels(v, scale));
            match (b.width, b.height) {
                (Length::Px(px), Length::Auto | Length::Content) => {
                    h = projected(scale::pixels(px, scale) - (l + r), ih, iw)
                }
                (Length::Auto | Length::Content, Length::Px(px)) => {
                    w = projected(scale::pixels(px, scale) - (t + d), iw, ih)
                }
                _ => (),
            }
        }
        Demand { min_w: w, w, h }
    }
    fn reflow(node: Mounted<'_, Self>, cx: &mut ReflowCx<'_>) -> Heights {
        let b = node.attrs().sizing;
        if b.width.weight().is_some()
            && !b.height.definite()
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
            || a.image_cover != b.image_cover
            || a.image_host_clip != b.image_host_clip
            || a.sizing.padding != b.sizing.padding
    }
    fn reads_context(_: &Attrs, old: Option<PaintContext>, ctx: PaintContext) -> bool {
        old.is_none_or(|p| p.frame != ctx.frame || p.child_clip != ctx.child_clip || p.scale != ctx.scale)
    }
}
