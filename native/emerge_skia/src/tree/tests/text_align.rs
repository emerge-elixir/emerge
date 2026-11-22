use super::*;

pub(super) fn host(align: Option<u8>) -> Bytes {
    let fields = [
        px(1, 100.),
        px(2, 40.),
        number(4, 4.),
        [vec![3, 0], 4f64.to_be_bytes().to_vec()].concat(),
        vec![40, 1],
    ];
    block(
        &fields
            .into_iter()
            .chain(align.map(|v| vec![30, v]))
            .collect::<Vec<_>>(),
    )
}
pub(super) fn label(align: Option<u8>) -> Bytes {
    block(
        &[vec![21, 0, 2, b'H', b'i'], vec![40, 1]]
            .into_iter()
            .chain(align.map(|v| vec![30, v]))
            .collect::<Vec<_>>(),
    )
}
pub(super) fn line() -> Bytes {
    upload(&[node(1, 4, host(None), &[2]), node(2, 5, label(None), &[])])
}

#[test]
fn inherited_text_alignment_consumes_the_offer_not_a_new_glyph_run() {
    let mut r = Raster::default();
    r.tree.upload(&line()).unwrap();
    let left = draw(&mut r).to_vec();
    let natural = bounds(&r, 2).width;
    assert!(natural > 14. && natural < 17.);
    for (parent, own, wide) in [
        (1, None, true),
        (2, None, true),
        (2, Some(0), false),
        (2, None, true),
        (0, None, false),
    ] {
        let n = r.tree.layers.invalidation_passes;
        let was_wide = bounds(&r, 2).width == 92.;
        r.tree.patch(&set(1, 4, host(Some(parent)))).unwrap();
        r.tree.patch(&set(2, 5, label(own))).unwrap();
        assert!(r.tree.layers.worksets_empty());
        draw(&mut r);
        assert_eq!(r.tree.layers.invalidation_passes, n + 1);
        assert_eq!(bounds(&r, 2).width, if wide { 92. } else { natural });
        assert!(!ran(&r, Measure, 2));
        assert_eq!(ran(&r, Flow, 2), wide != was_wide);
        if wide && was_wide {
            assert!(r.tree.layers.trace.contains(&(Compose, 2, Outputs::Draw)));
        }
        assert_eq!(
            r.hit_at(90., 10.),
            if wide {
                Some((2, 86., 6.))
            } else {
                Some((1, 90., 10.))
            }
        );
        check_fresh(&r);
    }
    assert_eq!(r.pixels(), left);
    assert_eq!(r.tree.layers.text.shapes, 1);
}

#[test]
fn a_sealed_text_box_owns_hits_even_after_another_alignment_is_accepted() {
    let mut r = Raster::default();
    r.tree.upload(&line()).unwrap();
    let a = draw(&mut r).to_vec();
    let center = set(1, 4, host(Some(1)));
    let off = set(1, 4, host(None));
    r.tree.patch(&center).unwrap();
    assert!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("handoff".into()))
            .is_err()
    );
    assert_eq!(r.pixels(), a);
    assert_eq!(r.hit_at(90., 10.), Some((1, 90., 10.)));
    r.settle(180., 170., 1.).unwrap();
    r.tree.patch(&off).unwrap();
    let pending = r.tree.encode();
    assert_eq!(
        r.tree.patch(&set(1, 4, host(Some(3)))).unwrap_err(),
        "invalid text alignment 3"
    );
    assert_eq!(
        r.tree.patch(&set(2, 5, block(&[vec![30]]))).unwrap_err(),
        "truncated EMRG data"
    );
    assert_eq!(r.tree.encode(), pending);
    r.install();
    assert_eq!(r.hit_at(90., 10.), Some((2, 86., 6.)));
    assert_eq!(draw(&mut r), a);
    assert_eq!(r.hit_at(90., 10.), Some((1, 90., 10.)));
}

#[test]
fn font_context_stops_at_an_override_and_a_cancelled_edit_schedules_no_text_work() {
    let mut r = Raster::default();
    let whole = upload(&[
        node(1, 3, host(None), &[2, 3]),
        node(2, 5, label(None), &[]),
        node(3, 4, host(Some(0)), &[4]),
        node(4, 5, label(None), &[]),
    ]);
    r.tree.upload(&whole).unwrap();
    let left = draw(&mut r).to_vec();
    let center = set(1, 3, host(Some(1)));
    r.tree.patch(&center).unwrap();
    r.tree.patch(&set(1, 3, host(None))).unwrap();
    assert_eq!(draw(&mut r), left);
    assert!(r.tree.layers.trace.is_empty());
    assert_eq!(r.tree.encode(), whole); // explicit left remains on the nested El
    r.tree.patch(&center).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 2).width, 92.);
    assert!(bounds(&r, 4).width < 17.);
    assert!(!ran(&r, Context, 3) && !ran(&r, Context, 4));
    assert_eq!(r.tree.layers.text.shapes, 1); // two independent sites, one glyph value
    check_fresh(&r);
}

#[test]
fn auto_text_reads_a_real_zero_offer_and_a_promoted_root_reads_the_viewport() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 4, block(&[px(1, 0.)]), &[2]),
            node(2, 5, label(Some(2)), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 2).width, 0.);
    assert!(r.pixels().iter().all(|v| *v == 0));
    r.tree.patch(&patch(1, 2, &[])).unwrap();
    for width in [180., 125., 180.] {
        r.draw(width, 50., 1.).unwrap();
        assert_eq!(bounds(&r, 2).width, width);
        assert_eq!(
            r.hit_at(width - 10., 9.),
            Some((2, f64::from(width) - 10., 9.))
        );
    }
    assert_eq!(r.tree.layers.text.shapes, 1);
}

pub(super) fn packed(width: Bytes) -> Bytes {
    upload(&[
        node(1, 1, block(&[px(1, 100.)]), &[2, 5]),
        node(2, 14, block(&[width, px(2, 24.), number(4, 4.)]), &[3, 4]),
        node(3, 5, label(None), &[]),
        node(4, 6, block(&[px(1, 30.), px(2, 20.)]), &[]),
        node(5, 6, block(&[vec![1, 0]]), &[]),
    ])
}
#[test]
fn aligned_text_can_change_the_content_cap_of_a_width_share() {
    let mut r = Raster::default();
    r.tree.upload(&packed(vec![1, 4, 1, 0])).unwrap(); // min(content(), fill())
    draw(&mut r);
    let cap = bounds(&r, 3).width + 4. + 30.;
    assert!(cap < 50.);
    for (mode, w) in [(0, cap), (1, 50.), (2, 50.), (0, cap)] {
        r.tree.patch(&set(3, 5, label(Some(mode)))).unwrap();
        draw(&mut r);
        assert_eq!((bounds(&r, 2).width, bounds(&r, 5).width), (w, 100. - w));
        check_fresh(&r);
    }
}

#[test]
fn auto_text_width_in_a_packed_column_is_known_before_its_peer_flows() {
    let mut r = Raster::default();
    r.tree.upload(&packed(vec![1, 1])).unwrap(); // content()
    draw(&mut r);
    let natural = bounds(&r, 3).width;
    for (mode, width) in [(1, 30.), (2, 30.), (0, natural)] {
        r.tree.patch(&set(3, 5, label(Some(mode)))).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 3).width, width);
        let packed = width + 4. + 30.;
        assert_eq!(
            (bounds(&r, 2).width, bounds(&r, 5).x, bounds(&r, 5).width),
            (packed, packed, 100. - packed)
        );
        assert_eq!(ran(&r, Measure, 2), mode != 2);
        check_fresh(&r);
    }
}
