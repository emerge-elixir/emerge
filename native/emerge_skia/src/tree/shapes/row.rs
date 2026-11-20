use super::*;

pub(super) struct Row;
impl ShapeMeta for Row {
    const TAG: u8 = 1;
    type Payload = Children;
    type Paint = ClippedBoxPaint;
}
impl LayoutBehaviour for Row {
    const ALLOTS_WIDTH: bool = true;
    const OFFERS_INNER_HEIGHT: bool = true;
    fn measure(children: &[Key], cx: &mut Measuring<'_>) -> Demand {
        measure_line(children, cx, false)
    }

    fn reads_flow(width: Length, input: FlowInput) -> bool {
        input.affects_width_group(width.definite())
    }
    fn offer_widths(node: Mounted<'_, Children>, cx: &mut FlowCx<'_>) {
        let available = cx.policy.definite().then_some(cx.inner);
        cx.assign_group(node.data(), available, false, |cx, c| {
            node.width_slot(cx, c)
        });
    }
    fn reflow(node: Mounted<'_, Children>, cx: &mut ReflowCx<'_>) -> f32 {
        line_height(node.data(), cx)
    }
    fn place_children(node: Mounted<'_, Children>, cx: &mut PlaceCx<'_>) {
        let style = node.attrs().sizing;
        let ([left, top], inner) = cx.padded_height_offer(style);
        let gap = scale::pixels(style.spacing[0], cx.scale);
        let mut x = 0.;
        for &c in node.data() {
            cx.place(c, [left + x, top], Constraint::Available(inner));
            x += cx.child(c).w + gap;
        }
    }
}
