use super::*;

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
        // Fill minima have their own dependency. A content cap also reads
        // preferred width, which can change while the minimum stays equal.
        match input {
            FlowInput::ChildWidth(w) => w.weight().is_none() || w.reads_fill_content(),
            _ => true,
        }
    }
    fn offer_widths(node: Mounted<'_, Children>, cx: &mut FlowCx<'_>) {
        let slot = |c, cx: &FlowCx<'_>| {
            let w = cx.policy_of(c);
            if let Some(weight) = w.weight() {
                let cap = if w.reads_fill_content() {
                    cx.measured_width(c) // padded, pre-wrap, already in device pixels
                } else {
                    w.bounds().map_or(f32::INFINITY, |b| {
                        scale::pixels(b.cap, node.elements.slots[c].admitted.scale)
                    })
                };
                fill::Slot::line_fill(weight, cx.minimum_width(c), cap)
            } else {
                fill::Slot::new(node.elements, c, w, cx.nonfill_width(c))
            }
        };
        let mut rest = node.data();
        let mut first_line = true;
        while let Some((&first, others)) = rest.split_first() {
            // Break on hypothetical outer widths, not on rounded allocations.
            // Even a single oversized child belongs to a line.
            let mut used = slot(first, cx).basis();
            let additional = others
                .iter()
                .take_while(|&&c| {
                    used = used + cx.gap + slot(c, cx).basis();
                    used <= cx.inner
                })
                .count();
            let (line, tail) = rest.split_at(1 + additional);
            let split = FillShares::line(cx.inner, cx.gap, line, |c| slot(c, cx));
            for &c in line {
                let s = slot(c, cx);
                let w = split.and_then(|share| share.offer(s)).unwrap_or(s.basis());
                cx.line_width(c, w, c == first && !first_line);
            }
            rest = tail;
            first_line = false;
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
