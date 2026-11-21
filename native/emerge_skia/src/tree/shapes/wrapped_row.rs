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

// Walk the breaks already settled by width flow. Both natural fit and final
// placement advance by the tallest occupied outer box; neither chooses lines.
fn pack<C>(
    children: &[Key],
    cx: &mut C,
    gap: [f32; 2],
    starts: impl Fn(&C, Key) -> bool,
    mut visit: impl FnMut(&mut C, Key, f32, f32) -> layers::Size,
) -> f32 {
    let (mut x, mut y, mut tallest) = (0., 0., 0f32);
    for (i, &c) in children.iter().enumerate() {
        if starts(cx, c) {
            x = 0.;
            y += tallest + gap[1];
            tallest = 0.;
        } else if i != 0 {
            x += gap[0];
        }
        let size = visit(cx, c, x, y);
        x += size.w;
        tallest = tallest.max(size.h);
    }
    y + tallest
}

fn occupied(
    e: &Elements,
    c: Key,
    fit: layers::Size,
    inner: impl FnOnce() -> f32,
) -> (layers::Size, Constraint) {
    let height = e.attrs[c].sizing.height;
    if height.weight().is_none() {
        return (fit, Constraint::Natural);
    }
    let h = height.at_offer(e, c, fit.h, inner());
    (layers::Size { h, ..fit }, Constraint::Assigned(h))
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
        if cx.fixed_basis {
            return cx.measured_height;
        }
        let children = node.data();
        let natural = !node.attrs().sizing.height.definite();
        // Before the stack grows, its natural cross-axis offer is that of a
        // content Row at these settled child widths, not the sum of its lines.
        let mut inner = None;
        let packed = pack(
            children,
            cx,
            cx.gap,
            |cx, c| cx.break_before(c),
            |cx, c, x, y| {
                let (size, edge) = occupied(node.elements, c, cx.child(c), || {
                    *inner.get_or_insert_with(|| cx.inner_height(line_height(children, cx)))
                });
                if natural {
                    cx.offset(c, x, y, edge);
                }
                size
            },
        );
        cx.box_height(packed)
    }
    fn place_children(node: Mounted<'_, Children>, cx: &mut PlaceCx<'_>) {
        let style = node.attrs().sizing;
        if !style.height.definite() {
            // Reflow already placed the natural stack. Never feed its grown
            // height back into the same fill children.
            cx.translate_children(node.data());
            return;
        }
        let ([left, top], inner) = cx.padded_height_offer(style);
        let gap = style.spacing.map(|v| scale::pixels(v, cx.scale));
        pack(
            node.data(),
            cx,
            gap,
            |cx, c| cx.break_before(c),
            |cx, c, x, y| {
                let (size, edge) = occupied(node.elements, c, cx.child(c), || inner);
                cx.place(c, [left + x, top + y], edge);
                size
            },
        );
    }
}
