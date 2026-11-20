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
            cx.offer(c, Constraint::Assigned(w));
        }
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
