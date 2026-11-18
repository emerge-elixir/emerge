use super::*;

pub(super) struct El;
impl ShapeMeta for El {
    const TAG: u8 = 4;
    type Payload = OnlyChild;
    type Paint = BoxPaint;
}
impl LayoutBehaviour for El {
    fn measure(child: Option<Key>, cx: &mut Measuring<'_>) -> Size {
        child.map_or(Size::default(), |c| cx.child(c))
    }
    fn reflow(node: Mounted<'_, OnlyChild>, cx: &mut ReflowCx<'_>) -> f32 {
        let h = node.data().map_or(0., |c| {
            cx.offset(c, 0., 0.);
            cx.child(c).h
        });
        cx.box_height(h)
    }
}
