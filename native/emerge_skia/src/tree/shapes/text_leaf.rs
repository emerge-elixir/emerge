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
    fn attrs_changed(a: &Attrs, b: &Attrs) -> bool {
        <BoxPaint as PaintBehaviour<Utf8>>::attrs_changed(a, b)
            || [0, 1, 3]
                .into_iter()
                .any(|i| a.sizing.padding[i] != b.sizing.padding[i])
    }
    fn reads_context(_: &Attrs, old: Option<PaintContext>, ctx: PaintContext) -> bool {
        old != Some(ctx)
    }
    fn paint(node: Mounted<'_, Utf8>, ctx: &PaintContext, run: Option<&Run>) -> OwnPaint {
        let mut own = OwnPaint::background(node.attrs(), ctx);
        if let Some(run) = run.filter(|_| ctx.color & 255 != 0) {
            let m = run.metrics;
            let f = ctx.frame;
            let [top, right, _, left] = node
                .attrs()
                .sizing
                .padding
                .map(|v| scale::pixels(v, ctx.scale));
            // Keep the signed space, including when the run or the insets overflow.
            let x = match ctx.text_align {
                Alignment::Start => f.x + left,
                Alignment::Center => f.x + left + (f.width - left - right - m.width) * 0.5,
                Alignment::End => f.x + f.width - right - m.width,
            };
            let pos = [x + m.left, f.y + top + m.ascent];
            let ink = Rect {
                x: pos[0] + m.ink.x,
                y: pos[1] + m.ink.y,
                ..m.ink
            };
            // Clip the ink to its own frame, not its padded content area.
            let clip = f.intersection(ctx.clip).unwrap_or_default().pixel_clip();
            if let Some(visible) = ink.intersection(clip) {
                own.text = Some(paint::GlyphPaint {
                    pos,
                    color: ctx.color,
                    rules: ctx.rules,
                    gradient: ctx.gradient,
                    revision: run.revision,
                    clip: (visible != ink).then_some(clip),
                });
            }
        }
        own
    }
}
