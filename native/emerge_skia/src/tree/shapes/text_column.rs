use super::*;

/// Document flow isn't a linear Column: no vertical zones or shared fill slots.
pub(super) struct TextColumn;
impl ShapeMeta for TextColumn {
    const TAG: u8 = 8;
    type Payload = Children;
    type Paint = ClippedBoxPaint;
}
impl LayoutBehaviour for TextColumn {
    fn measure(children: &[Key], cx: &mut Measuring<'_>) -> Demand {
        let mut size = children
            .iter()
            .map(|&c| cx.child(c))
            .fold(Demand::default(), |d, c| Demand {
                w: d.w.max(c.w),
                h: d.h + c.h,
                min_w: d.min_w.max(c.min_w),
            });
        size.h += cx.gap[1] * children.len().saturating_sub(1) as f32;
        size
    }
    fn auto_fills(node: Mounted<'_, Children>) -> bool {
        column::Column::auto_fills(node)
    }
    fn reads_flow(_: Length, input: FlowInput) -> bool {
        matches!(
            input,
            FlowInput::ChildWidth(_)
                | FlowInput::ChildFill
                | FlowInput::ChildSplit
                | FlowInput::Order
        )
    }
    fn offer_widths(node: Mounted<'_, Children>, cx: &mut FlowCx<'_>) {
        for &c in node.children() {
            if node.elements.float_side(c).is_some() {
                cx.offer_intrinsic_width(c);
            } else {
                cx.offer_available_width(c, cx.inner);
            }
        }
    }
    fn reflow(node: Mounted<'_, Children>, cx: &mut ReflowCx<'_>) -> Heights {
        cx.document(node.elements, node.key)
    }
    fn place_children(node: Mounted<'_, Children>, cx: &mut PlaceCx<'_>) {
        for &c in node.children() {
            cx.place_site(node.key, c);
        }
    }
}
