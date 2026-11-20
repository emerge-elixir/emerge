use super::*;

pub(super) fn policy(op: u8) -> Bytes {
    block(&[
        column_width::mixed(op),
        row_height::mixed(op),
        color([255, 0, 0]),
        vec![40, 1],
    ])
}
fn host(height: Bytes, padding: [f64; 4]) -> Bytes {
    block(&[
        px(1, 120.),
        height,
        [
            vec![3, 2],
            padding.into_iter().flat_map(f64::to_be_bytes).collect(),
        ]
        .concat(),
        vec![40, 1],
    ])
}
pub(super) fn demand(w: f64, h: f64) -> Bytes {
    block(&[px(1, w), px(2, h)])
}
pub(super) fn example() -> Bytes {
    upload(&[
        node(1, 4, host(px(2, 80.), [6.; 4]), &[2]),
        node(2, 4, policy(4), &[3]),
        node(3, 4, demand(30., 16.), &[4]),
        node(4, 5, text("i"), &[]),
    ])
}
#[test]
fn two_axis_policy_and_padding_edits_read_only_the_changed_inputs() {
    let mut r = Raster::default();
    r.tree.upload(&example()).unwrap();
    draw(&mut r);
    let xywh = |r: &Raster| {
        let b = bounds(r, 2);
        [b.x, b.y, b.width, b.height]
    };
    assert_eq!(xywh(&r), [6., 6., 30., 16.]);
    r.tree.patch(&set(2, 4, policy(5))).unwrap();
    assert!(r.tree.layers.worksets_empty());
    draw(&mut r);
    assert_eq!(xywh(&r), [6., 6., 108., 68.]);
    assert!(ran(&r, Flow, 2) && ran(&r, Place, 1) && !ran(&r, Flow, 1) && !ran(&r, Reflow, 1));
    assert!(!r.tree.layers.trace.iter().any(|(p, _, _)| *p == Measure));
    // Keep both inner dimensions. Only the padded origin changes.
    r.tree
        .patch(&set(1, 4, host(px(2, 80.), [8., 9., 4., 3.])))
        .unwrap();
    draw(&mut r);
    assert_eq!(xywh(&r), [3., 8., 108., 68.]);
    let target = r.target_at(4., 9.).unwrap();
    assert_eq!(target.1, 2);
    let weights = |w, h| {
        block(&[
            weighted::weight(1, w),
            weighted::weight(2, h),
            color([255, 0, 0]),
            vec![40, 1],
        ])
    };
    for w in [1., f64::from_bits(1), f64::MAX] {
        r.tree.patch(&set(2, 4, weights(w, w))).unwrap();
        draw(&mut r);
        assert_eq!(xywh(&r), [3., 8., 108., 68.]);
        assert_eq!(r.target_at(4., 9.), Some(target));
        if w != 1. {
            assert!(r.tree.layers.trace.is_empty());
        }
    }
}
#[test]
fn an_allocated_el_offers_the_placed_height_including_zero_not_a_cached_basis() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, demand(120., 80.), &[9, 10]),
            node(9, 6, demand(120., 20.), &[]),
            node(10, 4, host(vec![2, 0], [6.; 4]), &[2]),
            node(
                2,
                4,
                block(&[weighted::weight(1, 3.), weighted::weight(2, 0.5)]),
                &[3],
            ),
            node(3, 6, demand(30., 16.), &[]),
        ]))
        .unwrap();
    for (header, outer, h) in [(20., 60., 48.), (80., 0., 0.), (20., 60., 48.)] {
        r.tree.patch(&set(9, 6, demand(120., header))).unwrap();
        draw(&mut r);
        let (el, a) = (bounds(&r, 10), bounds(&r, 2));
        assert_eq!((el.y, el.height), (header as f32, outer));
        assert_eq!(
            [a.x, a.y, a.width, a.height],
            [6., header as f32 + 6., 108., h]
        );
    }
    r.tree
        .patch(&set(
            2,
            4,
            block(&[
                number(78, 1.5),
                [vec![1, 4], number(2, 20.), number(3, 3.)].concat(),
                [vec![2, 5], number(2, 50.), number(3, 3.)].concat(),
            ]),
        ))
        .unwrap();
    draw(&mut r);
    let a = bounds(&r, 2);
    assert_eq!((a.width, a.height), (30., 75.));
}
#[test]
fn el_moves_check_the_first_policy_and_site_refusals_and_handoff() {
    row_height::closed_move(4);
    row_height::edit_and_move(4);
}
