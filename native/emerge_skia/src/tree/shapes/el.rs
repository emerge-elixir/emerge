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
            let area = cx.padded_area(node.attrs().sizing);
            let mut origin = [area.x, area.y];
            let edge = Constraint::Available(area.height);
            let e = node.elements;
            let [x, y] = [0, 1].map(|axis| e.alignment(child, axis));
            if Self::reads_placed_width(|| x) {
                origin[0] += x.offset(area.width, cx.width_at(child, edge));
            }
            if y != Alignment::Start {
                let h = if e.attrs[child].rotation.mixes_axes() {
                    cx.occupied_at(child, edge).h
                } else {
                    edge.resolve_height(e, child, cx.child(child).h)
                };
                origin[1] += y.offset(area.height, h);
            }
            cx.place(child, origin, edge);
        }
    }
}
