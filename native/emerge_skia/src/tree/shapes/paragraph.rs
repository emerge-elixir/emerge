use super::*;

pub(super) struct Paragraph;
impl ShapeMeta for Paragraph {
    const TAG: u8 = 7;
    type Payload = Children;
    type Paint = ClippedBoxPaint;
}
impl LayoutBehaviour for Paragraph {
    fn measure(children: &[Key], cx: &mut Measuring<'_>) -> Demand {
        children
            .iter()
            .map(|&c| cx.child(c))
            .fold(Demand::default(), |m, c| Demand {
                w: m.w + c.w,
                h: m.h.max(c.h),
                min_w: m.min_w.max(c.min_w),
            })
    }
    fn reads_flow(_: Length, input: FlowInput) -> bool {
        matches!(
            input,
            FlowInput::ChildWidth(_)
                | FlowInput::ChildSplit
                | FlowInput::ChildFill
                | FlowInput::Order
        )
    }
    fn offer_widths(node: Mounted<'_, Children>, cx: &mut FlowCx<'_>) {
        for &c in node.children() {
            // A float uses its intrinsic width constrained by the paragraph,
            // not all remaining space after an earlier float.
            // It is an offer, not an allocated share: a minimum may overflow.
            cx.offer_intrinsic_width(c);
        }
    }
    fn reflow(node: Mounted<'_, Children>, cx: &mut ReflowCx<'_>) -> Heights {
        cx.paragraph(node.elements, node.key)
    }
    fn place_children(node: Mounted<'_, Children>, cx: &mut PlaceCx<'_>) {
        for &c in node.children() {
            cx.place_site(node.key, c);
        }
    }
}
