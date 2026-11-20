use super::*;

fn chip() -> Bytes {
    block(&[px(1, 36.), px(2, 12.)])
}
fn wrap(h: Bytes) -> Bytes {
    block(&[
        weighted::weight(1, 3.),
        h,
        [vec![3, 0], 2f64.to_be_bytes().to_vec()].concat(),
        number(4, 4.),
        color([255, 0, 0]),
        vec![40, 1],
    ])
}
pub(super) fn wrapped() -> Bytes {
    upload(&[
        node(2, 2, wrap(row_height::mixed(4)), &[3, 4]),
        node(3, 6, chip(), &[]),
        node(4, 6, chip(), &[]),
    ])
}

#[test]
fn resizing_only_the_viewport_has_the_same_postwrap_fit_as_an_el_offer() {
    let mut direct = Tree::default();
    direct.upload(&wrapped()).unwrap();
    let mut nested = Tree::default();
    nested
        .upload(&upload(&[
            node(1, 4, block(&[px(1, 80.), px(2, 80.)]), &[2]),
            node(2, 2, wrap(row_height::mixed(4)), &[3, 4]),
            node(3, 6, chip(), &[]),
            node(4, 6, chip(), &[]),
        ]))
        .unwrap();
    for (w, h, aw, ah, chip_y) in [
        (80., 80., 80., 16., 2.),
        (79., 80., 79., 32., 18.),
        (79., 10., 79., 10., 18.),
        (0., 0., 0., 0., 18.),
        (80., 80., 80., 16., 2.),
    ] {
        nested
            .patch(&set(1, 4, block(&[px(1, w), px(2, h)])))
            .unwrap();
        for t in [&mut direct, &mut nested] {
            t.refresh(w as f32, h as f32, 1.).unwrap();
        }
        let frame = |t: &Tree, id| t.layers.frames[t.elements.ids[&id]];
        for id in [2, 3, 4] {
            assert_eq!(frame(&direct, id), frame(&nested, id));
        }
        let a = frame(&direct, 2);
        assert_eq!([a.x, a.y, a.width, a.height], [0., 0., aw, ah]);
        assert_eq!(frame(&direct, 4).y, chip_y);
    }
}

#[test]
fn column_demand_changes_when_an_equal_fit_switches_from_measured_fill_to_nonfill() {
    // The same child has measured height 12 and reflowed height 28 in both
    // policies. The content-capped Column must change WHICH value it charges.
    let height = |fill| {
        block(&[
            px(1, 76.),
            if fill { vec![2, 0] } else { vec![2, 1] },
            number(4, 4.),
        ])
    };
    let full = |fill| {
        upload(&[
            node(1, 3, block(&[px(1, 76.), row_height::mixed(4)]), &[2]),
            node(2, 2, height(fill), &[3, 4]),
            node(3, 6, block(&[px(1, 38.), px(2, 12.)]), &[]),
            node(4, 6, block(&[px(1, 38.), px(2, 12.)]), &[]),
        ])
    };
    let mut t = Tree::default();
    t.upload(&full(true)).unwrap();
    for fill in [true, false, true] {
        t.patch(&set(2, 2, height(fill))).unwrap();
        t.refresh(76., 80., 1.).unwrap();
        let h = if fill { 12. } else { 28. };
        assert_eq!(t.layers.frames[t.elements.ids[&1]].height, h);
        assert_eq!(t.layers.frames[t.elements.ids[&2]].height, h);
        let mut cold = Tree::default();
        cold.upload(&full(fill)).unwrap();
        cold.refresh(76., 80., 1.).unwrap();
        for (id, frame) in t.frames() {
            assert_eq!(frame, cold.layers.frames[cold.elements.ids[&id]]);
        }
    }
}

#[test]
fn promotion_still_proves_masked_lengths_at_the_incoming_scale() {
    let mut r = Raster::default();
    let a = |n| {
        block(&[
            [vec![1, 4, 0, 4], number(2, 20.), number(2, n)].concat(),
            weighted::weight(2, 3.),
            color([255, 0, 0]),
            vec![40, 1],
        ])
    };
    r.tree
        .upload(&upload(&[
            node(
                1,
                4,
                block(&[px(1, 120.), px(2, 80.), number(78, 0.5)]),
                &[2],
            ),
            node(2, 4, a(200.), &[]),
        ]))
        .unwrap();
    let old = r.draw(120., 80., 1.).unwrap().to_vec();
    let press = r.target_at(8., 10.).unwrap();
    r.tree.patch(&set(2, 4, a(4e38))).unwrap();
    let pending = r.tree.encode();
    assert_eq!(
        r.tree.patch(&patch(1, 2, &[])).unwrap_err(),
        "scaled length exceeds device range"
    );
    assert_eq!(r.tree.encode(), pending);
    assert!(r.tree.layers.worksets_empty());
    assert_eq!(r.pixels(), old);
    assert_eq!(r.target_at(8., 10.), Some(press));
    r.tree.patch(&set(2, 4, a(200.))).unwrap();
    r.tree.patch(&patch(1, 2, &[])).unwrap();
    r.draw(120., 80., 1.).unwrap();
    let f = bounds(&r, 2);
    assert_eq!((f.width, f.height), (20., 80.));
    assert_eq!(r.target_at(18., 70.), Some((press.0, 2, 18., 70.)));
}

#[test]
fn promoting_a_closed_box_removes_old_assignments_but_not_its_installed_target() {
    let mut r = Raster::default();
    let a = block(&[
        weighted::weight(1, 3.),
        row_height::mixed(5),
        color([255, 0, 0]),
        vec![40, 1],
    ]);
    r.tree
        .upload(&upload(&[
            node(1, 1, block(&[px(1, 100.), px(2, 40.)]), &[9, 2]),
            node(9, 6, block(&[px(1, 20.), px(2, 12.)]), &[]),
            node(2, 4, a, &[3]),
            node(3, 6, block(&[px(2, 30.)]), &[]),
        ]))
        .unwrap();
    let old = r.draw(120., 80., 1.).unwrap().to_vec();
    let press = r.target_at(27., 12.).unwrap();
    assert_eq!((press.1, press.2, press.3), (2, 7., 12.));
    r.tree.patch(&patch(1, 2, &[])).unwrap();
    assert!(r.tree.layers.worksets_empty());
    assert_eq!(
        r.prepare(120., 80., 1.)
            .unwrap()
            .submit(|_, _| Err("refused promotion".into()))
            .unwrap_err(),
        "refused promotion"
    );
    assert_eq!(
        bounds(&r, 2),
        Rect {
            x: 0.,
            y: 0.,
            width: 120.,
            height: 80.
        }
    );
    assert_eq!(r.pixels(), old);
    assert_eq!(r.target_at(27., 12.), Some(press));
    assert!(r.target_at(110., 70.).is_none());
    r.draw(120., 80., 1.).unwrap();
    assert_eq!(r.target_at(110., 70.), Some((press.0, 2, 110., 70.)));
    // Demote again at the same viewport; the enclosing Row supplies the
    // assigned width and its local origin, not the stale viewport edge.
    r.tree
        .patch(&patch(
            2,
            10,
            &[
                node(10, 1, block(&[px(1, 100.), px(2, 40.)]), &[11, 2]),
                node(11, 6, block(&[px(1, 20.), px(2, 12.)]), &[]),
            ],
        ))
        .unwrap();
    assert_eq!(r.draw(120., 80., 1.).unwrap(), old);
    assert_eq!(r.target_at(27., 12.), Some(press));
}
