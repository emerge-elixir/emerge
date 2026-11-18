use super::*;

pub(super) struct Row;
impl ShapeMeta for Row {
    const TAG: u8 = 1;
    type Payload = Children;
    type Paint = ClippedBoxPaint;
}
impl LayoutBehaviour for Row {
    fn measure(children: &[Key], cx: &mut Measuring<'_>) -> Size {
        measure_line(children, cx)
    }

    fn reads_flow(width: Length, input: FlowInput) -> bool {
        let definite = width.definite();
        match input {
            FlowInput::ChildWidth(child) => !definite || child != Length::Fill,
            FlowInput::ChildFill | FlowInput::HorizontalGap | FlowInput::Order => definite,
        }
    }
    fn offer_widths(node: Mounted<'_, Children>, cx: &mut FlowCx<'_>) {
        let children = node.data();
        // A content-sized row gives even a fill child its own intrinsic extent.
        let share = if cx.policy.definite() {
            let (fixed, fills) = children.iter().fold((0.0, 0usize), |(w, n), &c| {
                if cx.policy_of(c) == Length::Fill {
                    (w, n + 1)
                } else {
                    (w + cx.measured_width(c), n)
                }
            });
            let gaps = cx.gap * children.len().saturating_sub(1) as f32;
            (fills != 0).then(|| (cx.inner - fixed - gaps).max(0.) / fills as f32)
        } else {
            None
        };
        for &c in children {
            let w = match (cx.policy_of(c), share) {
                (Length::Fill, Some(w)) => w,
                _ => cx.measured_width(c),
            };
            cx.offer(c, w);
        }
    }
    fn reflow(node: Mounted<'_, Children>, cx: &mut ReflowCx<'_>) -> f32 {
        let mut x = 0.;
        let mut h = 0f32;
        for &c in node.data() {
            let s = cx.child(c);
            cx.offset(c, x, 0.);
            h = h.max(s.h);
            x += s.w + cx.gap[0];
        }
        cx.box_height(h)
    }
}
