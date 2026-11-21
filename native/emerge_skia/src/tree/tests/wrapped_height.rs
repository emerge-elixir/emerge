use super::*;

pub(super) fn host(height: Bytes) -> Bytes {
    block(&[
        px(1, 48.),
        height,
        number(4, 4.),
        [vec![3, 0], 2f64.to_be_bytes().to_vec()].concat(),
    ])
}

// Layout scale 2 on A only: its 10px width and 14px cap become 20/28 device
// pixels. The containing Column assigns the WrappedRow a height after flow.
pub(super) fn assigned() -> Bytes {
    upload(&[
        node(1, 3, block(&[px(1, 48.), px(2, 64.)]), &[2, 3]),
        node(2, 6, block(&[px(2, 16.)]), &[]),
        node(3, 2, host(vec![2, 0]), &[4, 5, 6]),
        node(
            4,
            4,
            block(&[
                px(1, 10.),
                bounded::bound(2, 4, 14., Some(3.), true),
                number(78, 2.),
                vec![40, 1],
                color([255, 0, 0]),
            ]),
            &[],
        ),
        node(5, 6, block(&[px(1, 20.), px(2, 12.)]), &[]),
        node(
            6,
            4,
            block(&[px(1, 20.), vec![2, 0], vec![40, 1], color([0, 0, 255])]),
            &[],
        ),
    ])
}

#[test]
fn an_assigned_height_moves_later_lines_and_hits_only_on_handoff() {
    let mut r = Raster::default();
    r.tree.upload(&assigned()).unwrap();
    let old = draw(&mut r).to_vec();
    assert_eq!((bounds(&r, 4).height, bounds(&r, 6).y), (28., 50.));
    assert_eq!(r.hit_at(8., 54.), Some((6, 6., 4.)));
    assert_eq!(r.hit_at(8., 30.), Some((4, 3., 6.)));
    let header = |h| set(2, 6, block(&[px(2, h)]));
    r.tree.patch(&header(64.)).unwrap();
    let accepted = r.tree.encode();
    assert_eq!(
        r.tree
            .patch(&set(
                4,
                4,
                block(&[bounded::bound(2, 4, 2e38, Some(3.), true), number(78, 2.)])
            ))
            .unwrap_err(),
        "scaled length exceeds device range"
    );
    assert_eq!(r.tree.encode(), accepted);
    assert!(r.tree.layers.worksets_empty());
    assert_eq!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("handoff failed".into()))
            .unwrap_err(),
        "handoff failed"
    );
    assert_eq!(
        (
            bounds(&r, 3).height,
            bounds(&r, 4).height,
            bounds(&r, 6).height
        ),
        (0., 0., 0.)
    );
    assert_eq!(r.pixels(), old);
    assert_eq!(r.hit_at(8., 54.), Some((6, 6., 4.)));
    draw(&mut r);
    assert_eq!(r.hit_at(8., 54.), None);
    r.tree.patch(&header(16.)).unwrap();
    assert_eq!(draw(&mut r), old);
    assert_eq!(r.hit_at(8., 54.), Some((6, 6., 4.)));
}

#[test]
fn kept_moves_switch_between_column_share_and_wrapped_cross_height() {
    row_height::closed_move(2);
    row_height::edit_and_move(2);
}

#[test]
fn equal_natural_heights_do_not_hide_a_new_occupied_height() {
    let mut r = Raster::default();
    let build = |op, w| block(&[px(1, 20.), [vec![2, op, 1], number(3, w)].concat()]);
    r.tree
        .upload(&upload(&[
            node(
                1,
                2,
                block(&[px(1, 20.), px(2, 40.), number(4, 4.)]),
                &[2, 3],
            ),
            node(2, 4, build(4, 1.), &[4]),
            node(4, 6, block(&[px(2, 12.)]), &[]),
            node(3, 4, block(&[px(1, 20.), px(2, 12.)]), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 3).y, 16.);
    r.tree.patch(&set(2, 4, build(5, 3.))).unwrap();
    draw(&mut r);
    assert_eq!((bounds(&r, 2).height, bounds(&r, 3).y), (40., 44.));
    assert!(ran(&r, Place, 1));
    assert!(
        r.tree
            .layers
            .trace
            .iter()
            .all(|(p, _, _)| !matches!(p, Measure | Flow | Reflow))
    );
    // Numeric weights are not vertical shares, even when wrapping.
    let plain = |w| set(2, 4, block(&[px(1, 20.), weighted::weight(2, w)]));
    r.tree.patch(&plain(3.)).unwrap();
    draw(&mut r);
    r.tree.patch(&plain(f64::from_bits(1))).unwrap();
    draw(&mut r);
    assert!(r.tree.layers.trace.is_empty());
}
