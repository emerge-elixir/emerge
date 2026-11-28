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

/// Pack the cross extents of already chosen groups. Each component (e.g.
/// fit and minimum) takes its own maximum. Add gaps only between groups.
fn packed_cross_extent<'a, const N: usize>(
    groups: impl Iterator<Item = &'a [Key]>,
    gap: f32,
    mut extent: impl FnMut(Key) -> [f32; N],
) -> [f32; N] {
    groups
        .map(|group| {
            group
                .iter()
                .copied()
                .map(&mut extent)
                .fold([0f32; N], |a, b| std::array::from_fn(|i| a[i].max(b[i])))
        })
        .reduce(|a, b| std::array::from_fn(|i| a[i] + gap + b[i]))
        .unwrap_or([0.; N])
}

// Sizing and advancement are distinct at a turn. In particular a Column
// allocates a local height, but places the next child below the occupied box.
struct ResolvedSlot {
    charge: f32, // resolved main-axis allocation for even gaps and centering
    advance: f32,
    cross: f32,
    height: Constraint,
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

    fn advance(cursor: &mut Cursor, extent: f32, gap: f32, zone: Alignment, even: bool) -> f32 {
        if zone == Alignment::End {
            cursor.edge = coordinate(cursor.next - extent);
            cursor.next = coordinate(cursor.edge - gap);
            cursor.edge
        } else {
            let start = cursor.next;
            cursor.edge = coordinate(start + extent);
            // Every even group uses the plain Row's f32 step. Leave ordinary
            // wrapped and vertical placement's edge rounding unchanged.
            cursor.next = coordinate(if even || (M == X && !W) {
                start + (extent + gap)
            } else {
                cursor.edge + gap
            });
            start
        }
    }

    /// Anchor the two outer zones, then center *one group* between their actual
    /// edges. End members are positioned backwards so even a very large first
    /// member cannot lose the last one's right/bottom edge to cancellation.
    /// Only placement writes vary in order; paint and hit walk the declaration.
    fn place_zones(
        line: &[Key],
        room: Option<f32>,
        spacing: MainSpacing,
        alignment: impl Fn(Key) -> Alignment,
        cx: &mut PlaceCx<'_>,
        resolve: impl Fn(Key, &PlaceCx<'_>) -> ResolvedSlot,
        mut put: impl FnMut(&mut PlaceCx<'_>, Key, f32, f32, Constraint),
    ) -> f32 {
        use Alignment::{Center, End, Start};
        let even = spacing == MainSpacing::Even;
        let zone = |c| if even { Start } else { alignment(c) };
        let gap = match spacing {
            MainSpacing::Fixed(gap) => scale::pixels(gap, cx.scale),
            MainSpacing::Even => {
                Self::even_gap(line, room.unwrap_or_default(), |c| resolve(c, cx).charge)
            }
        };
        let (mut start, mut center, mut end) = (
            Cursor::at(0.),
            Cursor::at(0.),
            Cursor::at(room.unwrap_or_default()),
        );
        let mut widest = 0f32;
        let mut place = |c, z, cursor: &mut Cursor, cx: &mut PlaceCx<'_>| {
            let slot = resolve(c, cx);
            let at = Self::advance(cursor, slot.advance, gap, z, even);
            put(cx, c, at, slot.cross, slot.height);
            widest = widest.max(slot.cross);
        };
        let (mut centers, mut ends) = (&line[..0], &line[..0]);
        for (i, &c) in line.iter().enumerate() {
            match zone(c) {
                Start => place(c, Start, &mut start, cx),
                End => ends = &line[..=i],
                Center => {
                    if room.is_some() {
                        Self::advance(&mut center, resolve(c, cx).charge, gap, Center, even);
                    }
                    centers = &line[..=i];
                }
            }
        }
        if room.is_none() {
            // A scrollable Column has no viewport-anchored center or footer.
            // Like the reference, each trailing zone starts at the top group's
            // actual end. Interleaved zones still paint in declaration order.
            for (members, z) in [(ends, End), (centers, Center)] {
                let mut cursor = Cursor::at(start.edge);
                for &c in members.iter().filter(|&&c| zone(c) == z) {
                    place(c, Start, &mut cursor, cx);
                }
            }
            return widest;
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
    const MAIN_AXIS: Option<usize> = Some(M);
    const OFFERS_INNER_HEIGHT: bool = M == X && !W;
    const ADVANCES_X: bool = M == X || W;

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
            let splits = W || width.definite();
            match input {
                FlowInput::HorizontalGaps { authored, charge } => {
                    (W && authored) || (splits && charge)
                }
                FlowInput::ChildWidth(w) => {
                    !splits || w.weight().is_none() || w.reads_fill_content()
                }
                FlowInput::ChildFill | FlowInput::ChildSplit | FlowInput::Order => splits,
            }
        } else {
            (!W && width == Length::Auto && input == FlowInput::ChildFill)
                || (W && !width.definite() && matches!(input, FlowInput::ChildWidth(_)))
        }
    }
    fn offer_widths(node: Mounted<'_, Children>, cx: &mut FlowCx<'_>) {
        if M == Y {
            if !W || !cx.offer_pack(node.key) {
                cx.offer_inner_width(node.key);
            }
            return;
        }
        let attrs = node.attrs();
        let available = (W || attrs.sizing.width.definite()).then_some(cx.inner);
        let break_gap = scale::pixels(attrs.sizing.spacing[M], cx.scale);
        let charge = scale::pixels(attrs.main_spacing(M).charge(), cx.scale);
        let mut groups = Groups(node.data());
        let mut after_first = false;
        while let Some(line) = groups.measured(W, cx.inner, break_gap, |c| cx.line_measure(c)) {
            cx.assign_group(line, available, charge, after_first, |cx, c| {
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
        let lines = std::iter::from_fn(|| groups.settled(true, |c| cx.break_before(c)));
        let [fit, min] = packed_cross_extent(lines, cx.gap[Y], |c| {
            let fit = cx.child(c);
            let height = node.elements.attrs[c].sizing.height;
            if height.weight().is_none() {
                return [fit.h, fit.min_h];
            }
            let inner =
                *basis.get_or_insert_with(|| line_height(children, cx).map(|v| cx.inner_height(v)));
            let eval = |n, offer| height.at_offer(node.elements, c, n, offer);
            let h = eval(fit.h, inner.fit);
            let min = if fit.h == fit.min_h && inner.fit == inner.min {
                h
            } else {
                eval(fit.min_h, inner.min)
            };
            [h, min]
        })
        .map(|v| cx.box_height(v));
        cx.row_offer = basis.map_or(0., |b| b.fit);
        Heights { fit, min }
    }
    fn place_children(node: Mounted<'_, Children>, cx: &mut PlaceCx<'_>) {
        let style = node.attrs().sizing;
        let spacing = node.attrs().main_spacing(M);
        let children = node.data();
        let e = node.elements;
        let area = cx.padded_area(style);
        let gap = style.spacing.map(|v| scale::pixels(v, cx.scale));
        let [main_room, cross_room] = Self::axes([area.width, area.height]);
        let slot = |c, cx: &PlaceCx<'_>| {
            let fit = cx.child(c);
            let h = e.attrs[c].sizing.height;
            let basis = if h.weight().is_some() {
                cx.local_child(c).h
            } else {
                fit.h
            };
            let s = fill::Slot::new(e, c, h, basis);
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
                area.height,
                gap[Y],
                |c| vertical_basis(e, c, cx.child(c), false),
            )
        } else {
            groups.settled(W, |c| cx.break_before(c))
        } {
            let split = if M == Y {
                cx.declared_shares(node.key).or_else(|| {
                    FillShares::group(
                        (W || style.height.definite()).then_some(area.height),
                        scale::pixels(spacing.charge(), cx.scale),
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
                        .or_else(|| height.weight().map(|_| cx.local_height(c)));
                    let edge = allotted.map_or(Constraint::Natural, Constraint::Assigned);
                    (allotted.unwrap_or(fit.h), edge)
                } else if let Some(h) = cx.intrinsic_turn_height(c) {
                    (h, Constraint::Assigned(h))
                } else if height.weight().is_some()
                    && (W || e.attrs[c].align[Y].is_some_and(|a| a != Alignment::Start))
                {
                    let offer = if W && !style.height.definite() {
                        cx.row_offer
                    } else {
                        area.height
                    };
                    let h = height.at_offer(e, c, cx.local_child(c).h, offer);
                    (h, Constraint::Assigned(h))
                } else {
                    (
                        fit.h,
                        if W {
                            Constraint::Natural
                        } else {
                            Constraint::Available(area.height)
                        },
                    )
                };
                let [advance, cross] = if e.attrs[c].rotation.mixes_axes() {
                    let occupied = cx.occupied_at(c, edge);
                    let w = if M == X
                        && (W || style.width.definite())
                        && e.attrs[c].sizing.width.weight().is_some()
                    {
                        cx.local_child(c).w // A splitting Row advances by the assigned width.
                    } else {
                        occupied.w
                    };
                    Self::axes([w, occupied.h])
                } else {
                    let w = if Self::reads_placed_width(|| e.alignment(c, X)) {
                        cx.width_at(c, edge)
                    } else {
                        fit.w
                    };
                    Self::axes([w, h])
                };
                ResolvedSlot {
                    charge: if M == Y && split.is_some() && height.weight().is_some() {
                        h
                    } else {
                        advance
                    },
                    advance,
                    cross,
                    height: edge,
                }
            };
            let align = |c: Key| e.attrs[c].align[1 - M].unwrap_or_default();
            let interval = if W && line.iter().any(|&c| align(c) != Alignment::Start) {
                line.iter()
                    .map(|&c| resolved(c, cx).cross)
                    .fold(0., f32::max)
            } else {
                cross_room
            };
            // If the end zone were anchored to an overflowing viewport, it
            // would overlap earlier siblings or start before reachable content.
            let scroll = node.attrs().scroll;
            let main_room = if M == Y && !W && scroll.port() && spacing != MainSpacing::Even {
                None
            } else if scroll.enabled[M]
                && spacing != MainSpacing::Even
                && line
                    .iter()
                    .any(|&c| e.attrs[c].align[M].unwrap_or_default() != Alignment::Start)
            {
                Some(main_room.max(
                    line.iter().map(|&c| resolved(c, cx).advance).sum::<f32>()
                        + gap[M] * line.len().saturating_sub(1) as f32,
                ))
            } else {
                Some(main_room)
            };
            let occupied_cross = Self::place_zones(
                line,
                main_room,
                spacing,
                |c| e.attrs[c].align[M].unwrap_or_default(),
                cx,
                resolved,
                |cx, c, pos, across, edge| {
                    let offset =
                        Self::axes([pos, cross + scroll.align(1 - M, align(c), interval, across)]);
                    cx.place(c, [area.x + offset[0], area.y + offset[1]], edge);
                },
            );
            cross = coordinate((cross + occupied_cross) + gap[1 - M]);
        }
    }
}

fn vertical_basis(e: &Elements, c: Key, fit: Size, minimum: bool) -> f32 {
    let a = &e.attrs[c];
    if a.sizing.height.weight().is_none() && !minimum {
        return fit.h;
    }
    wrap_minimum(a.height_floor, fit.h, fit.min_h, || {
        e.slots[c].admitted.scale
    })
}

/// Membership and allocation are the same whether the caller needs a width
/// or needs to pass the assigned height through a filling card.
pub(in crate::tree) struct ColumnGroups<'e> {
    e: &'e Elements,
    groups: Groups<'e>,
    inner: f32,
    wrap: bool,
    gap: f32,
    charge: f32,
}
impl<'e> ColumnGroups<'e> {
    pub fn new(e: &'e Elements, k: Key, height: f32) -> Self {
        let b = e.attrs[k].sizing;
        let scale = e.slots[k].admitted.scale;
        Self {
            e,
            groups: Groups(e.children(k)),
            inner: (height
                - (scale::pixels(b.padding[0], scale) + scale::pixels(b.padding[2], scale)))
            .max(0.),
            wrap: b.height.limits_column_height(),
            gap: scale::pixels(b.spacing[Y], scale),
            charge: scale::pixels(e.attrs[k].main_spacing(Y).charge(), scale),
        }
    }
    pub fn next(
        &mut self,
        fit: impl Fn(Key) -> (Size, Size),
    ) -> Option<(&'e [Key], Option<FillShares>)> {
        let e = self.e;
        let group = self.groups.measured(self.wrap, self.inner, self.gap, |c| {
            vertical_basis(e, c, fit(c).1, false)
        })?;
        let shares = FillShares::group(Some(self.inner), self.charge, group, |c| {
            pack_slot(e, c, fit(c))
        });
        Some((group, shares))
    }
}
fn pack_slot(e: &Elements, c: Key, (local, outer): (Size, Size)) -> fill::Slot<'_> {
    let h = e.attrs[c].sizing.height;
    fill::Slot::new(
        e,
        c,
        h,
        if h.weight().is_some() {
            local.h
        } else {
            outer.h
        },
    )
    .with_basis(vertical_basis(e, c, outer, false))
}
pub(in crate::tree) fn pack_edge(
    e: &Elements,
    c: Key,
    fit: (Size, Size),
    s: Option<FillShares>,
) -> Constraint {
    s.and_then(|s| s.offer(pack_slot(e, c, fit)))
        .map_or(Constraint::Natural, Constraint::Assigned)
}
fn packed_widths<const N: usize>(
    e: &Elements,
    k: Key,
    height: f32,
    fit: impl Fn(Key) -> (Size, Size) + Copy,
    widths: impl Fn(Key, Constraint) -> [f32; N],
) -> [f32; N] {
    let style = e.attrs[k].sizing;
    let scale = e.slots[k].admitted.scale;
    let [_, r, _, l] = style.padding.map(|v| scale::pixels(v, scale));
    let gx = scale::pixels(style.spacing[X], scale);
    let mut groups = ColumnGroups::new(e, k, height);
    std::iter::from_fn(|| groups.next(fit))
        .map(|(line, shares)| {
            line.iter()
                .map(|&c| {
                    let edge = pack_edge(e, c, fit(c), shares);
                    widths(c, edge)
                })
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
    fit: impl Fn(Key) -> (Size, Size) + Copy,
    width: impl Fn(Key, Constraint) -> f32,
) -> f32 {
    packed_widths(e, k, height, fit, |c, edge| [width(c, edge)])[0]
}

pub(in crate::tree) fn column_demand(
    e: &Elements,
    k: Key,
    single: Demand,
    height_edge: Option<Constraint>,
    child: impl Fn(Key) -> (Size, Size) + Copy,
    offered_width: impl Fn(Key, Constraint) -> [f32; 2],
) -> Demand {
    let Some(h) = column_height(e, k, height_edge, child) else {
        return single;
    };
    let [w, min_w] = packed_widths(e, k, h, child, offered_width);
    Demand { w, min_w, ..single }
}
pub(in crate::tree) fn column_height(
    e: &Elements,
    k: Key,
    height_edge: Option<Constraint>,
    child: impl Fn(Key) -> (Size, Size),
) -> Option<f32> {
    let style = e.attrs[k].sizing;
    let scale = e.slots[k].admitted.scale;
    if let Length::Px(px) = style.height {
        return Some(scale::pixels(px, scale));
    }
    if let Some(Constraint::Assigned(h)) = height_edge {
        return Some(h);
    }
    let [t, _, b, _] = style.padding.map(|v| scale::pixels(v, scale));
    let gap = scale::pixels(style.spacing[Y], scale);
    let demand = e
        .children(k)
        .iter()
        .map(|&c| child(c).1.h)
        .reduce(|a, b| a + gap + b)
        .unwrap_or(0.);
    let fit = style.height.resolve((demand + t) + b, || scale);
    Some(if style.height.weight().is_some() {
        height_edge?.resolve(e, k, style.height, fit)
    } else {
        fit
    })
}
