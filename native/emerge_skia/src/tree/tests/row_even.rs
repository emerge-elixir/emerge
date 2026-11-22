//! A Row first budgets its children's widths, then distributes any remaining room.
use super::*;

fn host(width: Bytes, right: f64, gap: f64, flag: Option<u8>) -> Bytes {
    let pad = [3, 2]
        .into_iter()
        .chain([4., right, 4., 4.].into_iter().flat_map(f64::to_be_bytes))
        .collect();
    block(
        &[width, px(2, 40.), pad, number(4, gap)]
            .into_iter()
            .chain(flag.map(|v| vec![37, v]))
            .collect::<Vec<_>>(),
    )
}
fn card(width: Bytes, zone: u8) -> Bytes {
    block(&[
        width,
        px(2, 20.),
        vec![5, zone],
        color([0, 0, 255]),
        vec![40, 1],
    ])
}
fn three(host: Bytes, middle: Bytes) -> Bytes {
    upload(&[
        node(1, 1, host, &[2, 3, 4]),
        node(2, 4, card(px(1, 20.), 0), &[]),
        node(3, 4, card(middle, 0), &[]),
        node(4, 4, card(px(1, 20.), 0), &[]),
    ])
}
fn spans(r: &Raster) -> [(f32, f32); 3] {
    [2, 3, 4].map(|id| weighted::span(r, 1, id))
}
#[test]
fn equal_width_policy_and_trailing_padding_edits_reposition_even_leading_children() {
    let fixed = px(1, 100.);
    let natural = [vec![1, 5, 1], number(2, 100.)].concat(); // max(content(), 100px)
    let mut r = Raster::default();
    r.tree
        .upload(&three(host(natural.clone(), 4., 4., None), px(1, 20.)))
        .unwrap();
    draw(&mut r);
    r.tree
        .patch(&set(1, 1, host(natural.clone(), 4., 4., Some(1))))
        .unwrap();
    draw(&mut r);
    assert!(r.tree.layers.trace.is_empty()); // inactive, even though the frame is 100px
    assert_eq!(spans(&r), [(4., 20.), (28., 20.), (52., 20.)]);
    for (width, right, flag, positions) in [
        (fixed.clone(), 4., Some(1), [4., 40., 76.]),
        (fixed.clone(), 10., Some(1), [4., 37., 70.]),
        (fixed.clone(), 4., Some(0), [4., 28., 52.]),
        (fixed.clone(), 4., Some(1), [4., 40., 76.]),
        (fixed, 4., None, [4., 28., 52.]),
        (natural, 4., Some(1), [4., 28., 52.]),
    ] {
        r.tree
            .patch(&set(1, 1, host(width, right, 4., flag)))
            .unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 1).width, 100.);
        assert_eq!(spans(&r).map(|(x, _)| x), positions);
        check_fresh(&r);
    }
}
#[test]
fn only_space_left_after_bounded_shares_becomes_a_gap() {
    let mut r = Raster::default();
    r.tree
        .upload(&three(host(px(1, 100.), 4., 4., Some(1)), vec![1, 0]))
        .unwrap();
    let compound = |cap| [vec![1, 5, 4], number(3, 3.), number(2, cap), vec![0]].concat(); // max(min(3u, cap), u)
    // 72px available for A and B after padding and the 20px last card.
    for (a, b, expected) in [
        (
            weighted::weight(1, 3.),
            vec![1, 0],
            [(4., 54.), (58., 18.), (76., 20.)],
        ),
        (
            compound(30.),
            vec![1, 0],
            [(4., 36.), (40., 36.), (76., 20.)],
        ),
        (
            compound(50.),
            vec![1, 0],
            [(4., 50.), (54., 22.), (76., 20.)],
        ),
        // Two saturated fills: the remaining 28px becomes two 14px gaps.
        (
            nested::range(1, 0., 32., 3.),
            nested::range(1, 0., 12., 1.),
            [(4., 32.), (50., 12.), (76., 20.)],
        ),
    ] {
        r.tree.patch(&set(2, 4, card(a, 2))).unwrap();
        r.tree.patch(&set(3, 4, card(b, 1))).unwrap();
        draw(&mut r);
        assert_eq!(spans(&r), expected);
        check_fresh(&r);
    }
}
#[test]
fn an_even_gap_is_not_charged_to_the_fill_and_is_not_published_on_accept() {
    let root = |flag| host(px(1, 100.), 4., 4., flag);
    let mut r = Raster::default();
    r.tree.upload(&three(root(None), vec![1, 0])).unwrap();
    let visible = draw(&mut r).to_vec();
    assert_eq!(spans(&r), [(4., 20.), (28., 44.), (76., 20.)]);
    let a = Some((3, 16., 6.));
    assert_eq!(r.hit_at(44., 10.), a);
    let on = set(1, 1, root(Some(1)));
    let off = set(1, 1, root(Some(0)));
    let n = r.tree.layers.invalidation_passes;
    for edit in [&on, &off, &on] {
        r.tree.patch(edit).unwrap();
    }
    assert!(r.tree.layers.worksets_empty());
    assert!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("handoff".into()))
            .is_err()
    );
    assert_eq!(r.tree.layers.invalidation_passes, n + 1);
    assert_eq!(r.pixels(), visible);
    assert_eq!(r.hit_at(44., 10.), a);
    assert_eq!(spans(&r), [(4., 20.), (24., 52.), (76., 20.)]);
    assert!(ran(&r, Flow, 1) && !ran(&r, Measure, 1));
    r.settle(180., 170., 1.).unwrap();
    r.tree.patch(&off).unwrap(); // C is accepted, but B is what the compositor is handed.
    let pending = r.tree.encode();
    assert_eq!(
        r.tree.patch(&set(1, 1, root(Some(2)))).unwrap_err(),
        "invalid space_evenly flag 2"
    );
    for kind in [4, 5, 6] {
        for flag in [0, 1] {
            assert_eq!(
                r.tree
                    .patch(&patch(
                        1,
                        10,
                        &[node(10, kind, block(&[vec![37, flag]]), &[])]
                    ))
                    .unwrap_err(),
                "unsupported attribute 37 in first slice"
            );
        }
    }
    assert_eq!(r.tree.encode(), pending);
    assert_eq!(r.hit_at(44., 10.), a);
    r.install();
    assert_eq!(r.hit_at(44., 10.), Some((3, 20., 6.)));
    draw(&mut r);
    assert_eq!(r.hit_at(44., 10.), a);
    check_fresh(&r);
}
#[test]
fn zero_fixed_charge_still_switches_zones_but_does_not_change_the_width_budget() {
    let root = |w, flag| host(px(1, w), 4., 0., flag);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[node(1, 1, root(100., Some(1)), &[])]))
        .unwrap();
    draw(&mut r);
    r.tree
        .patch(&patch(
            1,
            1,
            &[edges(1, 1, &[2]), node(2, 4, card(px(1, 20.), 2), &[])],
        ))
        .unwrap();
    for (flag, x) in [(Some(1), 4.), (Some(0), 76.), (Some(1), 4.)] {
        r.tree.patch(&set(1, 1, root(100., flag))).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 2).x, x);
        if flag == Some(0) {
            assert!(ran(&r, Place, 1) && !ran(&r, Flow, 1));
        }
    }
    r.tree.patch(&set(2, 4, card(px(1, 20.), 1))).unwrap();
    draw(&mut r);
    assert!(!ran(&r, Place, 1)); // ignored x alignment; an El can still use it for its own child
    r.tree
        .patch(&patch(
            1,
            1,
            &[
                node(1, 1, root(32., Some(1)), &[2, 3]),
                node(3, 4, card(px(1, 20.), 1), &[]),
            ],
        ))
        .unwrap();
    draw(&mut r);
    assert_eq!(weighted::span(&r, 1, 3), (24., 20.)); // overflow: no negative gap or shrink
    assert_eq!(r.hit_at(30., 8.), Some((3, 6., 4.)));
    assert_eq!(r.hit_at(40., 8.), None); // the ordinary Row clip is unchanged
    check_fresh(&r);
}
#[test]
fn fixed_gaps_still_contribute_to_the_natural_width_before_any_slot_is_assigned() {
    let mut r = Raster::default();
    let root = |gap| host(vec![1, 0], 4., gap, Some(1));
    r.tree.upload(&three(root(4.), px(1, 20.))).unwrap();
    // At a content Row's natural site, its child is assigned its measured width.
    r.tree
        .patch(&patch(1, 9, &[node(9, 1, block(&[vec![1, 1]]), &[1])]))
        .unwrap();
    for (gap, w, last) in [(4., 76., 52.), (10., 88., 64.), (4., 76., 52.)] {
        r.tree.patch(&set(1, 1, root(gap))).unwrap();
        draw(&mut r);
        assert_eq!(
            (bounds(&r, 9).width, bounds(&r, 1).width, bounds(&r, 4).x),
            (w, w, last)
        );
        check_fresh(&r);
    }
}
#[test]
fn an_unchanged_middle_child_moves_when_a_sibling_packs_a_second_column() {
    let mut r = Raster::default();
    r.tree.upload(&wrapped_column::shared()).unwrap();
    r.tree
        .patch(&patch(
            1,
            1,
            &[
                edges(1, 1, &[2, 5, 8]),
                attr_record(5, 2, block(&[px(1, 8.)])),
                node(8, 6, block(&[px(1, 8.)]), &[]),
            ],
        ))
        .unwrap();
    for (h, packed, middle) in [(44., 12., 30.), (43., 30., 39.), (44., 12., 30.)] {
        r.tree
            .patch(&set(
                1,
                1,
                block(&[px(1, 64.), px(2, h), number(4, 1.), vec![37, 1]]),
            ))
            .unwrap();
        draw(&mut r);
        assert_eq!(
            (bounds(&r, 2).width, bounds(&r, 5).x, bounds(&r, 8).x),
            (packed, middle, 56.)
        );
        check_fresh(&r);
    }
    assert!(
        r.tree
            .patch(&set(4, 4, block(&[px(1, 12.)])))
            .unwrap_err()
            .contains("before child reflow")
    );
}
