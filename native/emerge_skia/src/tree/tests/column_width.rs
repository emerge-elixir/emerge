use super::{weighted::panel, *};

pub(super) fn mixed(op: u8) -> Bytes {
    [vec![1, op, 1], number(3, 3.)].concat() // width(min/max(content(), fill(3)))
}
fn pad(n: f64) -> Bytes {
    [vec![3, 0], n.to_be_bytes().to_vec()].concat()
}
fn host(width: Option<Bytes>) -> Bytes {
    block(
        &width
            .into_iter()
            .chain([px(2, 40.), pad(6.), number(4, 4.), vec![40, 1]])
            .collect::<Vec<_>>(),
    )
}
pub(super) fn stack() -> Bytes {
    upload(&[
        node(1, 3, host(Some(px(1, 120.))), &[2, 3]),
        node(2, 4, panel(1, 2, mixed(4)), &[4]),
        node(4, 6, block(&[px(1, 30.)]), &[]),
        node(3, 4, panel(1, 3, vec![1, 0]), &[5]),
        node(5, 5, text("i"), &[]),
    ])
}

#[test]
fn a_content_floor_reads_the_independent_offer_even_at_equal_measured_width() {
    let mut r = Raster::default();
    r.tree.upload(&stack()).unwrap();
    draw(&mut r);
    let widths = |r: &Raster| [1, 2, 3].map(|id| bounds(r, id).width);
    assert_eq!(widths(&r), [120., 30., 108.]);
    r.tree.patch(&set(2, 4, panel(1, 2, mixed(5)))).unwrap();
    draw(&mut r);
    assert_eq!(widths(&r), [120., 108., 108.]);
    assert!(ran(&r, Flow, 2) && !ran(&r, Flow, 1) && !ran(&r, Flow, 3));
    assert!(!r.tree.layers.trace.iter().any(|(p, _, _)| *p == Measure));
    r.tree.patch(&set(4, 6, block(&[px(1, 150.)]))).unwrap();
    draw(&mut r);
    assert_eq!(widths(&r), [120., 150., 108.]);
    assert!(!ran(&r, Measure, 5));
    // A content host sizes naturally; an omitted width still fills when a
    // direct child asks for fill. Neither splits width between the two lines.
    for (width, expected) in [
        (Some(vec![1, 1]), [162., 150., 150.]),
        (None, [180., 168., 168.]),
        (Some(px(1, 10.)), [10., 150., 0.]),
    ] {
        r.tree.patch(&set(1, 3, host(width))).unwrap();
        draw(&mut r);
        assert_eq!(widths(&r), expected);
    }
    r.tree.patch(&set(2, 4, panel(1, 2, mixed(4)))).unwrap();
    draw(&mut r);
    assert_eq!(widths(&r), [10., 0., 0.]);
}

#[test]
fn an_auto_column_observes_fill_presence_not_peer_weights() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, block(&[]), &[2]),
            node(2, 4, block(&[vec![1, 1]]), &[3]),
            node(3, 6, block(&[px(1, 30.)]), &[]),
        ]))
        .unwrap();
    for (policy, host, child) in [
        (vec![1, 1], 30., 30.),
        (mixed(4), 180., 30.),
        (mixed(5), 180., 180.),
        (weighted::weight(1, 0.25), 180., 180.),
        (vec![1, 1], 30., 30.),
    ] {
        r.tree.patch(&set(2, 4, block(&[policy]))).unwrap();
        draw(&mut r);
        assert_eq!((bounds(&r, 1).width, bounds(&r, 2).width), (host, child));
        if child == 180. {
            assert!(!ran(&r, Flow, 1));
        }
    }
}

#[test]
fn independent_width_reflows_wrapped_lines_and_places_the_following_child() {
    let mut r = Raster::default();
    let chip = || block(&[px(1, 46.), px(2, 12.)]);
    r.tree
        .upload(&upload(&[
            node(1, 3, host(Some(px(1, 120.))), &[2, 3]),
            node(2, 2, block(&[mixed(4), pad(2.), number(4, 4.)]), &[4, 5]),
            node(4, 6, chip(), &[]),
            node(5, 6, chip(), &[]),
            node(3, 6, block(&[vec![1, 0], px(2, 12.)]), &[]),
        ]))
        .unwrap();
    // Two 46px chips + 4px gap + 4px padding need 100px of outer width.
    for (w, a, h, second_y) in [
        (120., 100., 16., 8.),
        (112., 100., 16., 8.),
        (111.5, 99.5, 32., 24.),
        (120., 100., 16., 8.),
    ] {
        r.tree.patch(&set(1, 3, host(Some(px(1, w))))).unwrap();
        draw(&mut r);
        assert_eq!((bounds(&r, 2).width, bounds(&r, 2).height), (a, h));
        assert_eq!(bounds(&r, 5).y, second_y);
        assert_eq!(
            (bounds(&r, 3).width, bounds(&r, 3).y),
            (w as f32 - 12., h + 10.)
        );
        if w < 120. {
            assert!(!ran(&r, Measure, 4) && !ran(&r, Measure, 5));
        }
    }
}

#[test]
fn a_closed_move_checks_site_and_masked_scale_before_publishing_the_new_width() {
    let mut r = Raster::default();
    // A caps to min(content, 3u, masked px); the Row splits 120 as 90/30.
    // The other Column's width is 54 logical px at scale 2, i.e. 108 device px.
    let cap = |n| {
        panel(
            1,
            2,
            [vec![1, 4, 4, 1], number(3, 3.), number(2, n)].concat(),
        )
    };
    r.tree
        .upload(&upload(&[
            node(1, 3, block(&[px(1, 120.), number(4, 4.)]), &[10, 20, 30]),
            node(10, 1, block(&[px(1, 120.), px(2, 12.)]), &[2, 3]),
            node(2, 4, cap(120.), &[4]),
            node(4, 6, block(&[px(1, 120.)]), &[]),
            node(3, 4, panel(1, 3, vec![1, 0]), &[]),
            node(20, 3, block(&[px(1, 54.), px(2, 12.), number(78, 2.)]), &[]),
            node(30, 4, block(&[]), &[]),
        ]))
        .unwrap();
    let old = draw(&mut r).to_vec();
    let old_hit = r.target_at(85., 5.).unwrap();
    assert_eq!(old_hit.1, 2);
    let transfer = |parent, kind| {
        patch(
            1,
            1,
            &[
                edges(1, 3, &[10, 20, 30]),
                edges(10, 1, &[3]),
                edges(parent, kind, &[2]),
            ],
        )
    };
    r.tree.patch(&set(2, 4, cap(2e38))).unwrap();
    let pending = r.tree.encode();
    assert!(
        r.tree
            .patch(&transfer(30, 4))
            .unwrap_err()
            .contains("requires a root")
    );
    assert_eq!(
        r.tree.patch(&transfer(20, 3)).unwrap_err(),
        "scaled length exceeds device range"
    );
    assert_eq!(r.tree.encode(), pending);
    assert!(r.tree.layers.worksets_empty());
    assert_eq!(r.target_at(85., 5.), Some(old_hit));
    r.tree.patch(&set(2, 4, cap(120.))).unwrap();
    r.tree.patch(&transfer(20, 3)).unwrap();
    assert_eq!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("refused handoff".into()))
            .unwrap_err(),
        "refused handoff"
    );
    assert_eq!(bounds(&r, 2).width, 108.);
    assert_eq!(r.pixels(), old);
    assert_eq!(r.target_at(85., 5.), Some(old_hit));
    draw(&mut r);
    assert_eq!(r.target_at(100., 26.), Some((old_hit.0, 2, 50., 5.)));
    assert_eq!(r.target_at(85., 5.).unwrap().1, 3);
    r.tree
        .patch(&patch(
            1,
            1,
            &[
                edges(1, 3, &[10, 20, 30]),
                edges(10, 1, &[2, 3]),
                edges(20, 3, &[]),
            ],
        ))
        .unwrap();
    assert_eq!(draw(&mut r), old);
    assert_eq!(bounds(&r, 2).width, 90.);
    assert_eq!(r.target_at(85., 5.), Some(old_hit));
}
