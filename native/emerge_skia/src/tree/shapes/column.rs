use super::*;

pub(super) struct Column;
impl ShapeMeta for Column {
    const TAG: u8 = 3;
    type Payload = Children;
    type Paint = BoxPaint;
}
impl LayoutBehaviour for Column {
    fn measure(children: &[Key], cx: &mut Measuring<'_>) -> Size {
        children.iter().map(|&c| cx.child(c)).fold(
            Size {
                w: 0.,
                h: cx.gap[1] * children.len().saturating_sub(1) as f32,
            },
            |s, c| Size {
                w: s.w.max(c.w),
                h: s.h + c.h,
            },
        )
    }
    fn auto_fills(node: Mounted<'_, Children>) -> bool {
        node.data()
            .iter()
            .any(|&c| node.elements.attrs[c].sizing.width == Length::Fill)
    }
    fn reads_flow(width: Length, input: FlowInput) -> bool {
        width == Length::Auto && input == FlowInput::ChildFill
    }
    fn reflow(node: Mounted<'_, Children>, cx: &mut ReflowCx<'_>) -> f32 {
        let mut y = 0.;
        for (i, &c) in node.data().iter().enumerate() {
            if i != 0 {
                y += cx.gap[1]
            }
            cx.offset(c, 0., y);
            y += cx.child(c).h;
        }
        cx.box_height(y)
    }
}
