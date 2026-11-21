use super::*;

pub(super) struct Text;
impl ShapeMeta for Text {
    const TAG: u8 = 5;
    type Payload = Utf8;
    type Paint = Self;
}
impl LayoutBehaviour for Text {
    fn measure(text: &str, cx: &mut Measuring<'_>) -> Demand {
        cx.glyph_run(text)
    }
    fn reflow(_: Mounted<'_, Utf8>, cx: &mut ReflowCx<'_>) -> Heights {
        cx.measured_height.into()
    }
}
impl PaintBehaviour<Utf8> for Text {
    fn context_attrs_changed(a: &Attrs, b: &Attrs) -> bool {
        let p = a.sizing.padding;
        let q = b.sizing.padding;
        p[0] != q[0] || p[3] != q[3]
    }
    fn reads_context(a: &Attrs, old: Option<PaintContext>, ctx: PaintContext) -> bool {
        <BoxPaint as PaintBehaviour<Utf8>>::reads_context(a, old, ctx)
            || old.is_none_or(|p| {
                p.color != ctx.color || p.text_origin != ctx.text_origin || p.clip != ctx.clip
            })
    }
    fn paint(node: Mounted<'_, Utf8>, ctx: &PaintContext, run: Option<&Run>) -> OwnPaint {
        let mut own = OwnPaint::background(node.attrs(), ctx);
        if let Some(run) = run.filter(|_| ctx.color & 255 != 0) {
            let m = run.metrics;
            let pos = [ctx.text_origin[0] + m.left, ctx.text_origin[1] + m.ascent];
            let ink = Rect {
                x: pos[0] + m.ink.x,
                y: pos[1] + m.ink.y,
                ..m.ink
            };
            let clip = ctx.pixel_clip();
            if let Some(visible) = ink.intersection(clip) {
                own.text = Some(paint::GlyphPaint {
                    pos,
                    color: ctx.color,
                    revision: run.revision,
                    clip: (visible != ink).then_some(clip),
                });
            }
        }
        own
    }
}
