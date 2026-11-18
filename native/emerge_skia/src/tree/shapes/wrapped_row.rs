use super::*;

pub(super) struct WrappedRow;
impl ShapeMeta for WrappedRow {
    const TAG: u8 = 2;
    type Payload = Children;
    type Paint = ClippedBoxPaint;
}
impl LayoutBehaviour for WrappedRow {
    fn measure(children: &[Key], cx: &mut Measuring<'_>) -> Size {
        measure_line(children, cx)
    }
    fn reflow(node: Mounted<'_, Children>, cx: &mut ReflowCx<'_>) -> f32 {
        let (mut x, mut y, mut line_height) = (0., 0., 0f32);
        for (i, &c) in node.data().iter().enumerate() {
            let size = cx.child(c);
            if i != 0 {
                if x + cx.gap[0] + size.w > cx.inner {
                    x = 0.;
                    y += line_height + cx.gap[1];
                    line_height = 0.;
                } else {
                    x += cx.gap[0];
                }
            }
            cx.offset(c, x, y);
            x += size.w;
            line_height = line_height.max(size.h);
        }
        cx.box_height(y + line_height)
    }
}
