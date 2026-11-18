use super::*;

pub(super) struct Row;
impl ShapeMeta for Row {
    const TAG: u8 = 1;
    type Payload = Children;
    type Paint = ClippedBoxPaint;
}
impl LayoutBehaviour for Row {
    const ALLOTS_WIDTH: bool = true;
    fn measure(children: &[Key], cx: &mut Measuring<'_>) -> Size {
        measure_line(children, cx)
    }

    fn reads_flow(width: Length, input: FlowInput) -> bool {
        let definite = width.definite();
        match input {
            FlowInput::ChildWidth(child) => !definite || child.weight().is_none(),
            FlowInput::ChildFill | FlowInput::HorizontalGap | FlowInput::Order => definite,
        }
    }
    fn offer_widths(node: Mounted<'_, Children>, cx: &mut FlowCx<'_>) {
        let children = node.data();
        // A content-sized row gives even a fill child its own intrinsic extent.
        let split = FillShares::split(
            cx.policy,
            cx.inner,
            cx.gap,
            children,
            |c| cx.policy_of(c),
            |c| cx.measured_width(c),
        );
        for &c in children {
            let w = split
                .and_then(|s| s.offer(cx.policy_of(c)))
                .unwrap_or_else(|| cx.measured_width(c));
            cx.offer(c, w);
        }
    }
    fn reflow(node: Mounted<'_, Children>, cx: &mut ReflowCx<'_>) -> f32 {
        let mut x = 0.;
        let mut h = 0f32;
        for &c in node.data() {
            let s = cx.child(c);
            cx.offset(c, x, 0.);
            h = h.max(s.h);
            x += s.w + cx.gap[0];
        }
        cx.box_height(h)
    }
}
