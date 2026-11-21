use super::*;

pub(super) struct El;
impl ShapeMeta for El {
    const TAG: u8 = 4;
    type Payload = OnlyChild;
    type Paint = BoxPaint;
}
impl LayoutBehaviour for El {
    const OFFERS_INNER_HEIGHT: bool = true;
    fn measure(child: Option<Key>, cx: &mut Measuring<'_>) -> Demand {
        child.map_or(Demand::default(), |c| cx.child(c))
    }
    fn reflow(node: Mounted<'_, OnlyChild>, cx: &mut ReflowCx<'_>) -> Heights {
        line_height(node.children(), cx)
    }
    fn place_children(node: Mounted<'_, OnlyChild>, cx: &mut PlaceCx<'_>) {
        if let Some(child) = node.data() {
            let host = node.attrs();
            let (mut origin, height) = cx.padded_height_offer(host.sizing);
            let edge = Constraint::Available(height);
            let a = &node.elements.attrs[child];
            let across = |i: usize| a.align[i].or(host.align[i]).unwrap_or_default();
            if across(0) != Alignment::Start {
                let right = scale::pixels(host.sizing.padding[1], cx.scale);
                origin[0] += across(0).offset(
                    (cx.width - (origin[0] + right)).max(0.),
                    cx.width_at(child, edge),
                );
            }
            if across(1) != Alignment::Start {
                let fit = cx.child(child).h;
                let h = if a.sizing.height.weight().is_some() {
                    a.sizing.height.at_offer(node.elements, child, fit, height)
                } else {
                    fit
                };
                origin[1] += across(1).offset(height, h);
            }
            cx.place(child, origin, edge);
        }
    }
}
