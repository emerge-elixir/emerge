use super::*;

pub(super) struct Column;
impl ShapeMeta for Column {
    const TAG: u8 = 3;
    type Payload = Children;
    type Paint = ClippedBoxPaint;
}
impl LayoutBehaviour for Column {
    const ALLOTS_HEIGHT: bool = true;
    fn measure(children: &[Key], cx: &mut Measuring<'_>) -> Demand {
        children.iter().map(|&c| cx.child(c)).fold(
            Demand {
                h: cx.gap[1] * children.len().saturating_sub(1) as f32,
                ..Demand::default()
            },
            |s, c| Demand {
                min_w: s.min_w.max(c.min_w),
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
        if cx.fixed_basis {
            return cx.measured_height;
        }
        let demand = node
            .data()
            .iter()
            .map(|&c| cx.column_basis(c))
            .reduce(|y, h| y + cx.gap[1] + h)
            .unwrap_or(0.);
        cx.box_height(demand)
    }
    fn place_children(node: Mounted<'_, Children>, cx: &mut PlaceCx<'_>) {
        let children = node.data();
        let style = node.attrs().sizing;
        let ([left, top], inner) = cx.padded_height_offer(style);
        let gap = scale::pixels(style.spacing[1], cx.scale);
        let length = |c: Key| node.elements.attrs[c].sizing.height;
        let slot =
            |c, cx: &PlaceCx<'_>| fill::Slot::new(node.elements, c, length(c), cx.child(c).h);
        let split = FillShares::group(
            style.height.definite().then_some(inner),
            gap,
            children,
            |c| slot(c, cx),
        );
        let mut y = 0.;
        for &c in children {
            let allotted = split
                .and_then(|s| s.offer(slot(c, cx)))
                .or_else(|| length(c).weight().map(|_| cx.measured_height(c)));
            cx.place(
                c,
                [left, top + y],
                allotted.map_or(Constraint::Natural, Constraint::Assigned),
            );
            y += allotted.unwrap_or_else(|| cx.child(c).h);
            y += gap;
        }
    }
}
