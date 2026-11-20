use super::*;

/// Split off the next line using only outer measurements. No child has a
/// share yet, and a single oversized child still makes progress.
fn take_line<T: Copy>(
    children: &[T],
    inner: f32,
    gap: f32,
    width: impl Fn(T) -> f32,
) -> Option<(&[T], &[T])> {
    let (&first, rest) = children.split_first()?;
    let mut used = width(first);
    let following = rest
        .iter()
        .take_while(|&&c| {
            used = used + gap + width(c);
            used <= inner
        })
        .count();
    Some(children.split_at(1 + following))
}

pub(super) struct WrappedRow;
impl ShapeMeta for WrappedRow {
    const TAG: u8 = 2;
    type Payload = Children;
    type Paint = ClippedBoxPaint;
}
impl LayoutBehaviour for WrappedRow {
    const ALLOTS_WIDTH: bool = true;
    fn measure(children: &[Key], cx: &mut Measuring<'_>) -> Demand {
        measure_line(children, cx, true)
    }
    fn reads_flow(_: Length, input: FlowInput) -> bool {
        input.affects_width_group(true) // each line distributes its own width
    }
    fn offer_widths(node: Mounted<'_, Children>, cx: &mut FlowCx<'_>) {
        let mut rest = node.data();
        let mut break_before = false;
        while let Some((line, tail)) = take_line(rest, cx.inner, cx.gap, |c| cx.line_measure(c)) {
            cx.assign_group(line, Some(cx.inner), break_before, |cx, c| {
                node.width_slot(cx, c).with_basis(cx.line_measure(c))
            });
            rest = tail;
            break_before = true;
        }
    }
    fn reflow(node: Mounted<'_, Children>, cx: &mut ReflowCx<'_>) -> f32 {
        let (mut x, mut y, mut line_height) = (0., 0., 0f32);
        for (i, &c) in node.data().iter().enumerate() {
            if cx.break_before(c) {
                x = 0.;
                y += line_height + cx.gap[1];
                line_height = 0.;
            } else if i != 0 {
                x += cx.gap[0];
            }
            let size = cx.child(c);
            cx.offset(c, x, y);
            x += size.w;
            line_height = line_height.max(size.h);
        }
        cx.box_height(y + line_height)
    }
}
