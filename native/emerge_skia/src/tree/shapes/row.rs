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
            FlowInput::ChildWidth(child) => {
                !definite || child.weight().is_none() || child.reads_fill_content()
            }
            FlowInput::ChildFill
            | FlowInput::ChildSplit
            | FlowInput::HorizontalGap
            | FlowInput::Order => definite,
        }
    }
    fn offer_widths(node: Mounted<'_, Children>, cx: &mut FlowCx<'_>) {
        let children = node.data();
        // A content-sized row gives even a fill child its own intrinsic extent.
        let slot = |c, cx: &FlowCx<'_>| {
            fill::Slot::new(node.elements, c, cx.policy_of(c), cx.measured_width(c))
        };
        let split = FillShares::split(cx.policy, cx.inner, cx.gap, children, |c| slot(c, cx));
        for &c in children {
            let w = split
                .and_then(|s| s.offer(slot(c, cx)))
                .unwrap_or_else(|| cx.measured_width(c));
            cx.offer(c, w);
        }
    }
    fn reflow(node: Mounted<'_, Children>, cx: &mut ReflowCx<'_>) -> f32 {
        if cx.fixed_basis {
            return cx.measured_height;
        }
        let h = node
            .data()
            .iter()
            .map(|&c| cx.child(c).h)
            .fold(0., f32::max);
        cx.box_height(h)
    }
    fn place_children(node: Mounted<'_, Children>, cx: &mut PlaceCx<'_>) {
        let style = node.attrs().sizing;
        let [top, _, bottom, left] = style.padding.map(|v| scale::pixels(v, cx.scale));
        let inner = (cx.height - (top + bottom)).max(0.);
        let gap = scale::pixels(style.spacing[0], cx.scale);
        let mut x = 0.;
        for &c in node.data() {
            let fit = cx.child(c);
            let policy = node.elements.attrs[c].sizing.height;
            let height = policy
                .weight()
                .map(|_| policy.at_offer(node.elements, c, fit.h, inner));
            cx.place(c, [left + x, top], height);
            x += fit.w + gap;
        }
    }
}
