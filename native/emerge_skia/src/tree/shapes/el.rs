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
            let e = node.elements;
            let [x, y] = [0, 1].map(|axis| e.alignment(child, axis));
            if x != Alignment::Start {
                let right = scale::pixels(host.sizing.padding[1], cx.scale);
                origin[0] += x.offset(
                    (cx.width - (origin[0] + right)).max(0.),
                    cx.width_at(child, edge),
                );
            }
            if y != Alignment::Start {
                origin[1] += y.offset(height, edge.resolve_height(e, child, cx.child(child).h));
            }
            cx.place(child, origin, edge);
        }
    }
}
