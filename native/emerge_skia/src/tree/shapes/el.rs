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
    fn reflow(node: Mounted<'_, OnlyChild>, cx: &mut ReflowCx<'_>) -> f32 {
        line_height(node.children(), cx)
    }
    fn place_children(node: Mounted<'_, OnlyChild>, cx: &mut PlaceCx<'_>) {
        if let Some(child) = node.data() {
            let (origin, height) = cx.padded_height_offer(node.attrs().sizing);
            cx.place(child, origin, Constraint::Available(height));
        }
    }
}
