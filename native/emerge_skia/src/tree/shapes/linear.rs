//! Linear layouts differ in the main axis and in how a group ends. Width
//! groups receive their constraints in Flow; height groups receive them in
//! Place. Keep that scheduling detail out of the group geometry.
use super::*;

pub(super) const X: usize = 0;
pub(super) const Y: usize = 1;
pub(super) struct Linear<const MAIN: usize, const WRAP: bool, const TAG: u8>;

impl<const M: usize, const W: bool, const T: u8> ShapeMeta for Linear<M, W, T> {
    const TAG: u8 = T;
    type Payload = Children;
    type Paint = ClippedBoxPaint;
}

/// Borrow contiguous groups from the only child order. In a measurement pass
/// the first item always fits, even when oversized. In a packing pass use
/// the saved breaks: do not infer them again from allocated child sizes.
struct Groups<'a>(&'a [Key]);
impl<'a> Groups<'a> {
    fn take(&mut self, count: impl FnOnce(Key, &[Key]) -> usize) -> Option<&'a [Key]> {
        let (&first, rest) = self.0.split_first()?;
        let (line, tail) = self.0.split_at(1 + count(first, rest));
        self.0 = tail;
        Some(line)
    }
    fn measured(
        &mut self,
        wrap: bool,
        inner: f32,
        gap: f32,
        size: impl Fn(Key) -> f32,
    ) -> Option<&'a [Key]> {
        self.take(|first, rest| {
            if !wrap {
                return rest.len();
            }
            let mut used = size(first);
            rest.iter()
                .take_while(|&&c| {
                    used = used + gap + size(c);
                    used <= inner
                })
                .count()
        })
    }
    fn settled(&mut self, wrap: bool, starts: impl Fn(Key) -> bool) -> Option<&'a [Key]> {
        self.take(|_, rest| {
            if wrap {
                rest.iter().position(|&c| starts(c)).unwrap_or(rest.len())
            } else {
                rest.len()
            }
        })
    }
}

/// `edge` excludes the gap that `next` reserves for another member of this zone.
struct Cursor {
    edge: f32,
    next: f32,
}
impl Cursor {
    fn at(edge: f32) -> Self {
        Self { edge, next: edge }
    }
}

impl<const M: usize, const W: bool, const T: u8> Linear<M, W, T> {
    // Map between (x,y) and (main,cross), in either direction.
    fn axes(pair: [f32; 2]) -> [f32; 2] {
        [pair[M], pair[1 - M]]
    }

    fn even_gap(line: &[Key], room: f32, extent: impl Fn(Key) -> f32) -> f32 {
        if line.len() < 2 {
            return 0.;
        }
        let occupied: f32 = line.iter().copied().map(extent).sum();
        (room - occupied).max(0.) / (line.len() - 1) as f32
    }

    fn advance(cursor: &mut Cursor, extent: f32, gap: f32, zone: Alignment) -> f32 {
        if zone == Alignment::End {
            cursor.edge = cursor.next - extent;
            cursor.next = cursor.edge - gap;
            cursor.edge
        } else {
            let start = cursor.next;
            cursor.edge = start + extent;
            // Preserve the existing Row's f32 step rounding.
            cursor.next = if M == X && !W {
                start + (extent + gap)
            } else {
                cursor.edge + gap
            };
            start
        }
    }

    /// Anchor the two outer zones, then center *one group* between their actual
    /// edges. End members are positioned backwards so even a very large first
    /// member cannot lose the last one's right/bottom edge to cancellation.
    /// Only placement writes vary in order; paint and hit walk the declaration.
    fn place_zones(
        line: &[Key],
        room: f32,
        gap: f32,
        zone: impl Fn(Key) -> Alignment,
        cx: &mut PlaceCx<'_>,
        resolve: impl Fn(Key, &PlaceCx<'_>) -> (Size, Constraint),
        mut put: impl FnMut(&mut PlaceCx<'_>, Key, f32, f32, Constraint),
    ) -> f32 {
        use Alignment::{Center, End, Start};
        let (mut start, mut center, mut end) = (Cursor::at(0.), Cursor::at(0.), Cursor::at(room));
        let mut widest = 0f32;
        let mut place = |c, z, cursor: &mut Cursor, cx: &mut PlaceCx<'_>| {
            let (size, edge) = resolve(c, cx);
            let [main, across] = Self::axes([size.w, size.h]);
            let at = Self::advance(cursor, main, gap, z);
            put(cx, c, at, across, edge);
            widest = widest.max(across);
        };
        let (mut centers, mut ends) = (&line[..0], &line[..0]);
        for (i, &c) in line.iter().enumerate() {
            match zone(c) {
                Start => place(c, Start, &mut start, cx),
                End => ends = &line[..=i],
                Center => {
                    let (s, _) = resolve(c, cx);
                    Self::advance(&mut center, Self::axes([s.w, s.h])[0], gap, Center);
                    centers = &line[..=i];
                }
            }
        }
        for &c in ends.iter().rev().filter(|&&c| zone(c) == End) {
            place(c, End, &mut end, cx);
        }
        if !centers.is_empty() {
            let free = (end.edge - start.edge) - center.edge;
            center = Cursor::at(start.edge + free.max(0.) * 0.5);
            for &c in centers.iter().filter(|&&c| zone(c) == Center) {
                place(c, Center, &mut center, cx);
            }
        }
        widest
    }
}

impl<const M: usize, const W: bool, const T: u8> LayoutBehaviour for Linear<M, W, T> {
    const ALLOTS_WIDTH: bool = M == X;
    const ALLOTS_HEIGHT: bool = M == Y;
    const OFFERS_INNER_HEIGHT: bool = M == X && !W;

    fn measure(children: &[Key], cx: &mut Measuring<'_>) -> Demand {
        let gaps = cx.gap[M] * children.len().saturating_sub(1) as f32;
        // Column historically adds the gap total first, Row last.
        let initial = if M == Y { gaps } else { 0. };
        let (sum, max, minimum) = children
            .iter()
            .fold((initial, 0f32, 0f32), |(s, m, n), &c| {
                let d = cx.child(c);
                let [main, cross] = Self::axes([d.w, d.h]);
                (
                    s + main,
                    m.max(cross),
                    if M == X && !W {
                        n + d.min_w
                    } else {
                        n.max(d.min_w)
                    },
                )
            });
        let [w, h] = Self::axes([if M == X { sum + gaps } else { sum }, max]);
        Demand {
            w,
            h,
            min_w: if M == X && !W {
                minimum + gaps
            } else {
                minimum
            },
        }
    }
    fn auto_fills(node: Mounted<'_, Children>) -> bool {
        M == Y
            && !W
            && node
                .data()
                .iter()
                .any(|&c| node.elements.attrs[c].sizing.width.weight().is_some())
    }
    fn reads_flow(width: Length, input: FlowInput) -> bool {
        if M == X {
            input.affects_width_group(W || width.definite())
        } else {
            (!W && width == Length::Auto && input == FlowInput::ChildFill)
                || (W && !width.definite() && matches!(input, FlowInput::ChildWidth(_)))
        }
    }
    fn offer_widths(node: Mounted<'_, Children>, cx: &mut FlowCx<'_>) {
        if M == Y {
            if cx.offer_children {
                for &c in node.data() {
                    cx.offer(c, Constraint::Available(cx.inner));
                }
            }
            return;
        }
        let available = (W || cx.policy.definite()).then_some(cx.inner);
        let mut groups = Groups(node.data());
        let mut after_first = false;
        while let Some(line) = groups.measured(W, cx.inner, cx.gap, |c| cx.line_measure(c)) {
            cx.assign_group(line, available, after_first, |cx, c| {
                let slot = node.width_slot(cx, c);
                if W {
                    slot.with_basis(cx.line_measure(c))
                } else {
                    slot
                }
            });
            after_first = true;
        }
    }
    fn reflow(node: Mounted<'_, Children>, cx: &mut ReflowCx<'_>) -> Heights {
        if cx.fixed_basis {
            return cx.measured_height.into();
        }
        let children = node.data();
        if M == Y {
            return children
                .iter()
                .map(|&c| {
                    let fit = cx.child(c);
                    if W {
                        Heights {
                            fit: fit.h,
                            min: vertical_basis(node.elements, c, fit, true),
                        }
                    } else if node.elements.attrs[c].sizing.height.weight().is_some() {
                        let h = cx.measured_height(c);
                        Heights {
                            fit: h,
                            min: h.min(fit.min_h),
                        }
                    } else {
                        fit.heights()
                    }
                })
                .reduce(|a, b| Heights {
                    fit: a.fit + cx.gap[Y] + b.fit,
                    min: if W {
                        a.min.max(b.min)
                    } else {
                        a.min + cx.gap[Y] + b.min
                    },
                })
                .unwrap_or_default()
                .map(|v| cx.box_height(v));
        }
        if !W {
            return line_height(children, cx);
        }
        let mut basis = None;
        let mut groups = Groups(children);
        let heights = std::iter::from_fn(|| groups.settled(true, |c| cx.break_before(c)))
            .map(|line| {
                line.iter()
                    .map(|&c| {
                        let fit = cx.child(c);
                        let height = node.elements.attrs[c].sizing.height;
                        if height.weight().is_none() {
                            return fit.heights();
                        }
                        let inner = *basis.get_or_insert_with(|| {
                            line_height(children, cx).map(|v| cx.inner_height(v))
                        });
                        let eval = |n, offer| height.at_offer(node.elements, c, n, offer);
                        let h = eval(fit.h, inner.fit);
                        Heights {
                            fit: h,
                            min: if fit.h == fit.min_h && inner.fit == inner.min {
                                h
                            } else {
                                eval(fit.min_h, inner.min)
                            },
                        }
                    })
                    .fold(Heights::default(), Heights::max)
            })
            .reduce(|a, b| Heights {
                fit: a.fit + cx.gap[Y] + b.fit,
                min: a.min + cx.gap[Y] + b.min,
            })
            .unwrap_or_default()
            .map(|v| cx.box_height(v));
        cx.row_offer = basis.map_or(0., |b| b.fit);
        heights
    }
    fn place_children(node: Mounted<'_, Children>, cx: &mut PlaceCx<'_>) {
        let style = node.attrs().sizing;
        let even = M == X && !W && node.attrs().even_row();
        let children = node.data();
        let e = node.elements;
        let (origin, inner) = cx.padded_height_offer(style);
        let pad = style.padding.map(|v| scale::pixels(v, cx.scale));
        let gap = style.spacing.map(|v| scale::pixels(v, cx.scale));
        let [main_room, cross_room] = Self::axes([(cx.width - (pad[1] + pad[3])).max(0.), inner]);
        let slot = |c, cx: &PlaceCx<'_>| {
            let fit = cx.child(c);
            let s = fill::Slot::new(e, c, e.attrs[c].sizing.height, fit.h);
            if W {
                s.with_basis(vertical_basis(e, c, fit, false))
            } else {
                s
            }
        };
        let mut groups = Groups(children);
        let mut cross = 0.;
        while let Some(line) = if M == Y {
            groups.measured(
                W && style.height.limits_column_height(),
                inner,
                gap[Y],
                |c| vertical_basis(e, c, cx.child(c), false),
            )
        } else {
            groups.settled(W, |c| cx.break_before(c))
        } {
            let split = if M == Y {
                cx.declared_shares(node.key).or_else(|| {
                    FillShares::group(
                        (W || style.height.definite()).then_some(inner),
                        gap[M],
                        line,
                        |c| slot(c, cx),
                    )
                })
            } else {
                None
            };
            let resolved = |c: Key, cx: &PlaceCx<'_>| {
                let fit = cx.child(c);
                let height = e.attrs[c].sizing.height;
                let (h, edge) = if M == Y {
                    let allotted = split
                        .and_then(|s| s.offer(slot(c, cx)))
                        .or_else(|| height.weight().map(|_| cx.measured_height(c)));
                    let edge = allotted.map_or(Constraint::Natural, Constraint::Assigned);
                    (allotted.unwrap_or(fit.h), edge)
                } else if height.weight().is_some()
                    && (W || e.attrs[c].align[Y].is_some_and(|a| a != Alignment::Start))
                {
                    let offer = if W && !style.height.definite() {
                        cx.row_offer
                    } else {
                        inner
                    };
                    let h = height.at_offer(e, c, fit.h, offer);
                    (h, Constraint::Assigned(h))
                } else {
                    (
                        fit.h,
                        if W {
                            Constraint::Natural
                        } else {
                            Constraint::Available(inner)
                        },
                    )
                };
                let w = if M == X || W || e.alignment(c, X) != Alignment::Start {
                    cx.width_at(c, edge)
                } else {
                    fit.w
                };
                (Size { w, h, ..fit }, edge)
            };
            // Sizes are settled. Even spacing uses the existing forward cursor
            // for every child; it replaces main-axis zones, not cross alignment.
            let main_gap = if even {
                Self::even_gap(line, main_room, |c| {
                    let (s, _) = resolved(c, cx);
                    Self::axes([s.w, s.h])[0]
                })
            } else {
                gap[M]
            };
            let zone = |c: Key| {
                if even {
                    Alignment::Start
                } else {
                    e.attrs[c].align[M].unwrap_or_default()
                }
            };
            let align = |c: Key| e.attrs[c].align[1 - M].unwrap_or_default();
            let interval = if W && line.iter().any(|&c| align(c) != Alignment::Start) {
                line.iter()
                    .map(|&c| {
                        let (s, _) = resolved(c, cx);
                        Self::axes([s.w, s.h])[1]
                    })
                    .fold(0., f32::max)
            } else {
                cross_room
            };
            let occupied_cross = Self::place_zones(
                line,
                main_room,
                main_gap,
                zone,
                cx,
                resolved,
                |cx, c, pos, across, edge| {
                    let offset = Self::axes([pos, cross + align(c).offset(interval, across)]);
                    cx.place(c, [origin[0] + offset[0], origin[1] + offset[1]], edge);
                },
            );
            cross = (cross + occupied_cross) + gap[1 - M];
        }
    }
}

fn vertical_basis(e: &Elements, c: Key, fit: Size, minimum: bool) -> f32 {
    let a = &e.attrs[c];
    if a.sizing.height.weight().is_none() && !minimum {
        return fit.h;
    }
    a.height_floor
        .map_or(fit.min_h, |b| b.resolve(fit.h, e.slots[c].admitted.scale))
}

/// Cross extent of the groups. Width offers are independent of group membership.
/// The caller supplies either proven width-independent heights or width-settled fits.
fn packed_widths<const N: usize>(
    e: &Elements,
    k: Key,
    height: f32,
    break_height: impl Fn(Key) -> f32,
    widths: impl Fn(Key) -> [f32; N],
) -> [f32; N] {
    let style = e.attrs[k].sizing;
    let scale = e.slots[k].admitted.scale;
    let [t, r, b, l] = style.padding.map(|v| scale::pixels(v, scale));
    let [gx, gy] = style.spacing.map(|v| scale::pixels(v, scale));
    let inner = (height - (t + b)).max(0.);
    let mut groups = Groups(e.children(k));
    std::iter::from_fn(|| {
        groups.measured(
            style.height.limits_column_height(),
            inner,
            gy,
            &break_height,
        )
    })
    .map(|line| {
        line.iter()
            .map(|&c| widths(c))
            .fold([0f32; N], |a, b| std::array::from_fn(|i| a[i].max(b[i])))
    })
    .reduce(|a, b| std::array::from_fn(|i| a[i] + gx + b[i]))
    .unwrap_or([0.; N])
    .map(|v| style.width.resolve(v + (l + r), || scale))
}
pub(in crate::tree) fn column_width(
    e: &Elements,
    k: Key,
    height: f32,
    child: impl Fn(Key) -> Size,
) -> f32 {
    packed_widths(
        e,
        k,
        height,
        |c| vertical_basis(e, c, child(c), false),
        |c| [child(c).w],
    )[0]
}

pub(in crate::tree) fn column_demand(
    e: &Elements,
    k: Key,
    single: Demand,
    height_edge: Option<Constraint>,
    child: impl Fn(Key) -> Demand,
) -> Demand {
    let style = e.attrs[k].sizing;
    if matches!(style.width, Length::Px(_))
        || (style.height.limits_column_height()
            && !e
                .children(k)
                .iter()
                .all(|&c| e.slots[c].shape.has_intrinsic_height(e.attrs[c].sizing)))
    {
        return single;
    }
    let scale = e.slots[k].admitted.scale;
    let [t, r, b, l] = style.padding.map(|v| scale::pixels(v, scale));
    let inner = (single.w - (l + r)).max(0.);
    let gap = scale::pixels(style.spacing[Y], scale);
    let demand = e
        .children(k)
        .iter()
        .map(|&c| child(c).h)
        .reduce(|a, b| a + gap + b)
        .unwrap_or(0.);
    let fit = style.height.resolve((demand + t) + b, || scale);
    let h = if style.height.weight().is_some() {
        let Some(edge) = height_edge else {
            return single;
        };
        edge.resolve(e, k, style.height, fit)
    } else {
        fit
    };
    let [w, min_w] = packed_widths(
        e,
        k,
        h,
        |c| {
            let h = child(c).h;
            e.attrs[c]
                .height_floor
                .map_or(h, |b| b.resolve(h, e.slots[c].admitted.scale))
        },
        |c| {
            let m = child(c);
            let w = if e.slots[c].shape.uses_offer(e, c) {
                e.attrs[c].sizing.width.at_offer(e, c, m.w, inner)
            } else {
                m.w
            };
            [w, m.min_w]
        },
    );
    Demand { w, min_w, ..single }
}
