use super::*;

fn pad(n: f64) -> Bytes {
    [3, 0].into_iter().chain(n.to_be_bytes()).collect()
}
pub(super) fn header(height: Bytes) -> Bytes {
    block(&[vec![1, 0], height, color([255, 0, 0])])
}
pub(super) fn panel(rgb: [u8; 3]) -> Bytes {
    block(&[vec![1, 0], vec![2, 0], color(rgb), vec![40, 1]])
}
pub(super) fn stack(height: Bytes, gap: f64) -> Bytes {
    block(&[px(1, 80.), height, pad(5.), number(4, gap)])
}
pub(super) fn document() -> Bytes {
    upload(&[
        node(1, 3, stack(px(2, 120.), 5.), &[2, 3, 4]),
        node(2, 4, header(px(2, 20.)), &[5]),
        node(5, 5, text("Pay"), &[]),
        node(3, 4, panel([0, 255, 0]), &[]),
        node(4, 4, panel([0, 0, 255]), &[]),
    ])
}
fn yh(r: &Raster, id: u64) -> (f32, f32) {
    let f = bounds(r, id);
    (f.y, f.height)
}

#[test]
fn a_height_split_changes_placements_not_widths_or_glyphs_and_waits_for_submit() {
    let mut r = Raster::default();
    r.tree.upload(&document()).unwrap();
    let short = draw(&mut r).to_vec();
    assert_eq!(yh(&r, 3), (30., 40.));
    assert_eq!(yh(&r, 4), (75., 40.));
    let old = (4, 5., 5.);
    assert_eq!(r.hit_at(10., 80.), Some(old));
    let edit = |h| set(2, 4, header(px(2, h)));
    r.tree.patch(&edit(60.)).unwrap();
    assert!(r.tree.layers.worksets_empty());
    assert_eq!(r.hit_at(10., 80.), Some(old));
    let error = r
        .prepare(180., 170., 1.)
        .unwrap()
        .submit(|_, _| Err("refused height split".into()))
        .unwrap_err();
    assert_eq!(error, "refused height split");
    assert_eq!(yh(&r, 3), (70., 20.));
    assert_eq!(yh(&r, 4), (95., 20.));
    assert!(r.tree.layers.trace.iter().all(|(p, _, _)| *p != Flow));
    assert!((3..=5).all(|id| !ran(&r, Measure, id)));
    assert_eq!(r.pixels(), short);
    assert_eq!(r.hit_at(10., 80.), Some(old));
    assert_ne!(draw(&mut r), short);
    assert_eq!(r.hit_at(10., 80.), Some((3, 5., 10.)));
    r.tree.patch(&edit(20.)).unwrap();
    assert_eq!(draw(&mut r), short);
    assert_eq!(r.hit_at(10., 80.), Some(old));
    for patch in [
        edit(60.),
        order(1, 3, &[2, 4, 3]),
        edit(20.),
        order(1, 3, &[2, 3, 4]),
    ] {
        r.tree.patch(&patch).unwrap();
    }
    assert!(r.tree.layers.worksets_empty());
    assert_eq!(draw(&mut r), short);
    assert!(r.tree.layers.trace.is_empty());
}

#[test]
fn a_content_host_uses_each_fill_basis_and_an_allocated_column_subdivides_even_that_offer() {
    let mut r = Raster::default();
    let root = |h| block(&[h, number(4, 4.)]);
    r.tree
        .upload(&upload(&[
            node(1, 3, block(&[number(4, 4.)]), &[2, 3]),
            node(2, 3, block(&[vec![2, 0]]), &[4, 5]),
            node(4, 4, panel([255, 0, 0]), &[6]),
            node(5, 4, panel([0, 0, 255]), &[7]),
            node(6, 4, block(&[px(2, 10.)]), &[]),
            node(7, 4, block(&[px(2, 30.)]), &[]),
            node(3, 6, block(&[vec![2, 0]]), &[]),
        ]))
        .unwrap();
    for (h, outer, first, second) in [
        (None, 44., 40., 0.),
        (Some(vec![2, 1]), 44., 40., 0.),
        (Some(vec![2, 0]), 100., 48., 48.),
        (Some(px(2, 2.)), 2., 0., 0.),
    ] {
        if let Some(h) = h {
            r.tree.patch(&set(1, 3, root(h))).unwrap();
        }
        r.tree.refresh(80., 100., 1.).unwrap();
        assert_eq!(yh(&r, 1), (0., outer));
        assert_eq!(yh(&r, 2), (0., first));
        assert_eq!(yh(&r, 3), (first + 4., second));
        assert_eq!(yh(&r, 4), (0., first / 2.));
        assert_eq!(yh(&r, 5), (first / 2., first / 2.));
    }
    // Equal intrinsic value, different membership: #4's content also measures 10.
    r.tree.patch(&set(1, 3, root(px(2, 100.)))).unwrap();
    r.tree.refresh(80., 100., 1.).unwrap();
    r.tree.patch(&set(4, 4, block(&[px(2, 10.)]))).unwrap();
    r.tree.refresh(80., 100., 1.).unwrap();
    assert_eq!(yh(&r, 4), (0., 10.));
    assert_eq!(yh(&r, 5), (10., 38.));
    assert!(!ran(&r, Measure, 2));
}

#[test]
fn wrapping_charges_reflowed_nonfill_but_a_content_host_offers_fill_its_prewrap_basis() {
    let host = |w, h| block(&[px(1, w), h, number(4, 4.)]);
    let wrap = |h| block(&[vec![1, 0], h, number(4, 4.)]);
    let chip = block(&[px(1, 48.), px(2, 20.)]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, host(100., px(2, 100.)), &[2, 3]),
            node(2, 2, wrap(vec![2, 1]), &[4, 5]),
            node(3, 4, panel([0, 0, 255]), &[]),
            node(4, 4, chip.clone(), &[]),
            node(5, 4, chip, &[]),
        ]))
        .unwrap();
    for (w, demand) in [(100., 20.), (80., 44.), (100., 20.)] {
        r.tree.patch(&set(1, 3, host(w, px(2, 100.)))).unwrap();
        draw(&mut r);
        assert_eq!(yh(&r, 2), (0., demand));
        assert_eq!(yh(&r, 3), (demand + 4., 96. - demand));
        if w == 80. {
            assert!(!ran(&r, Measure, 4) && !ran(&r, Measure, 5));
        }
    }
    r.tree.patch(&set(1, 3, host(80., vec![2, 1]))).unwrap();
    r.tree.patch(&set(2, 2, wrap(vec![2, 0]))).unwrap();
    r.tree.patch(&set(3, 4, block(&[px(2, 4.)]))).unwrap();
    draw(&mut r);
    assert_eq!(yh(&r, 1), (0., 28.));
    assert_eq!(yh(&r, 2), (0., 20.));
    assert_eq!(yh(&r, 3), (24., 4.));
    assert_eq!(yh(&r, 5), (24., 20.), "still wrapped, not squeezed");
}

#[test]
fn viewport_height_only_resizes_outer_shares_without_scaling_them_or_their_hit_edges() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(
                1,
                3,
                block(&[px(1, 32.), vec![2, 0], number(4, 2.), vec![40, 1]]),
                &[2, 3],
            ),
            node(
                2,
                3,
                block(&[number(78, 2.), vec![1, 0], vec![2, 0], pad(1.)]),
                &[4],
            ),
            node(
                4,
                4,
                block(&[vec![1, 0], px(2, 30.), color([0, 255, 0]), vec![40, 1]]),
                &[],
            ),
            node(3, 4, panel([0, 0, 255]), &[]),
        ]))
        .unwrap();
    for (i, h) in [43., 23., 1., 43.].into_iter().enumerate() {
        r.draw(32., h, 1.).unwrap();
        let share = (h - 2.).max(0.) / 2.;
        assert_eq!(yh(&r, 1), (0., h));
        assert_eq!(yh(&r, 2), (0., share));
        assert_eq!(yh(&r, 3), (share + 2., share));
        assert_eq!(yh(&r, 4), (2., 60.));
        if i != 0 {
            assert!(
                r.tree
                    .layers
                    .trace
                    .iter()
                    .all(|(p, _, _)| !matches!(p, Measure | Flow | Reflow))
            );
        }
        if share == 0. {
            assert_eq!(r.hit_at(8., 0.), Some((1, 8., 0.)));
        } else {
            let pos = 4 * (32 * share.floor() as usize + 8);
            assert_eq!(&r.pixels()[pos..pos + 4], [0, 255, 0, 255]);
            assert_eq!(
                r.hit_at(8., share),
                Some((4, 3., f64::from(share - 2.) / 2.))
            );
            assert_eq!(
                r.hit_at(8., share + 0.25),
                Some((1, 8., f64::from(share + 0.25)))
            );
        }
    }
}

#[test]
fn site_proof_uses_final_edges_and_accepted_attrs_without_disturbing_pending_or_installed_state() {
    let host = |h| block(&[px(1, 30.), px(2, h)]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            // A WrappedRow can keep these lanes side by side but cannot
            // itself give an incoming child a fill-height offer.
            node(1, 2, block(&[]), &[2, 3, 4]),
            node(2, 3, host(50.), &[17]),
            node(3, 2, block(&[]), &[]),
            node(4, 3, host(70.), &[]),
            node(17, 3, block(&[vec![1, 0], vec![2, 0]]), &[18]),
            node(18, 4, block(&[vec![1, 0], px(2, 50.), vec![40, 1]]), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    let pressed = r.target_at(5., 45.).unwrap();
    r.tree.patch(&set(2, 3, host(24.))).unwrap();
    // #18's fill is accepted but never drawn before the attempted promotion.
    r.tree.patch(&set(18, 4, panel([0, 0, 255]))).unwrap();
    let pending = r.tree.encode();
    let to_wrap = [
        edges(1, 2, &[2, 3, 4]),
        edges(2, 3, &[]),
        edges(3, 2, &[17]),
    ];
    for records in [
        to_wrap.to_vec(),
        [to_wrap.as_slice(), &[edges(17, 3, &[18])]].concat(),
    ] {
        assert!(
            r.tree
                .patch(&patch(1, 1, &records))
                .unwrap_err()
                .contains("child of a Column")
        );
    }
    for bad in [
        patch(2, 17, &[]),
        patch(1, 18, &[]),
        patch(3, 8, &[node(8, 4, panel([1, 2, 3]), &[])]),
    ] {
        assert!(r.tree.patch(&bad).unwrap_err().contains("root Column"));
    }
    for variant in [4, 5] {
        let value = vec![2, variant, 0, 1];
        assert_eq!(
            r.tree.patch(&set(3, 2, block(&[value]))).unwrap_err(),
            "height(fill()) requires a root Column or a child of a Column, Row or El"
        );
    }
    assert_eq!(r.tree.encode(), pending);
    assert!(r.tree.layers.worksets_empty());
    assert_eq!(r.target_at(5., 45.), Some(pressed));
    draw(&mut r);
    assert_eq!(yh(&r, 17), (0., 24.));
    assert_eq!(r.hit_at(5., 45.), None);
    r.tree
        .patch(&patch(
            1,
            1,
            &[
                edges(1, 2, &[2, 3, 4]),
                edges(2, 3, &[]),
                edges(4, 3, &[17]),
            ],
        ))
        .unwrap();
    draw(&mut r);
    assert_eq!(yh(&r, 17), (0., 70.));
    assert_eq!(r.target_at(35., 45.).unwrap().0, pressed.0);
    r.tree.patch(&patch(1, 17, &[])).unwrap(); // closed column is now the root
    draw(&mut r);
    assert_eq!(yh(&r, 17), (0., 170.));
    assert_eq!(yh(&r, 18), (0., 170.));
    // Conversely, once #18 clears fill it can be promoted without a draw.
    r.tree.patch(&set(18, 4, block(&[px(2, 12.)]))).unwrap();
    r.tree.patch(&patch(17, 18, &[])).unwrap();
    assert_eq!(r.target_at(5., 45.).unwrap().0, pressed.0);
    draw(&mut r);
    assert_eq!(yh(&r, 18), (0., 12.));
    assert!(r.hit_at(5., 45.).is_none());
}
