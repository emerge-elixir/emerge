use super::*;

pub(super) struct Empty;
impl ShapeMeta for Empty {
    const TAG: u8 = 6;
    type Payload = NoData;
    type Paint = BoxPaint;
}
impl LayoutBehaviour for Empty {
    fn measure(_: NoData, _: &mut Measuring<'_>) -> Demand {
        Demand::default()
    }
    fn reflow(_: Mounted<'_, NoData>, cx: &mut ReflowCx<'_>) -> Heights {
        cx.box_height(0.).into()
    }
}
