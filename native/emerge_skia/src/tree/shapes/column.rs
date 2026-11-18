use super::*;

pub(super) struct Column;
impl ShapeMeta for Column {
    const TAG: u8 = 3;
    type Payload = Children;
    type Paint = ClippedBoxPaint;
}
impl LayoutBehaviour for Column {
    const ALLOTS_HEIGHT: bool = true;
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
            .any(|&c| node.elements.attrs[c].sizing.width.weight().is_some())
    }
    fn reads_flow(width: Length, input: FlowInput) -> bool {
        width == Length::Auto && input == FlowInput::ChildFill
    }
    fn reflow(node: Mounted<'_, Children>, cx: &mut ReflowCx<'_>) -> f32 {
        if node.attrs().sizing.height.definite() {
            return cx.measured_height;
        }
        let demand = node
            .data()
            .iter()
            .map(|&c| cx.child(c).h)
            .reduce(|y, h| y + cx.gap[1] + h)
            .unwrap_or(0.);
        cx.box_height(demand)
    }
    fn place_children(node: Mounted<'_, Children>, cx: &mut PlaceCx<'_>) {
        let children = node.data();
        let style = node.attrs().sizing;
        let [top, _, bottom, left] = style.padding.map(|n| scale::pixels(n, cx.scale));
        let gap = scale::pixels(style.spacing[1], cx.scale);
        let length = |c: Key| node.elements.attrs[c].sizing.height;
        let split = FillShares::split(
            style.height,
            cx.height - (top + bottom),
            gap,
            children,
            length,
            |c| cx.basis(c),
            |c| node.elements.slots[c].admitted.scale,
        );
        let mut y = 0.;
        for &c in children {
            let allotted =
                split.and_then(|s| s.offer(length(c), || node.elements.slots[c].admitted.scale));
            cx.place(c, [left, top + y], allotted);
            y += allotted.unwrap_or_else(|| cx.basis(c));
            y += gap;
        }
    }
}
