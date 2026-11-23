use super::*;

pub(super) fn row_attrs(width: Bytes, gap: f64, inset: f64) -> Bytes {
    block(&[
        width,
        number(4, gap),
        [3, 0].into_iter().chain(inset.to_be_bytes()).collect(),
    ])
}
pub(super) fn red(width: Bytes) -> Bytes {
    block(&[width, px(2, 20.), color([255, 0, 0])])
}
pub(super) fn band(row: Bytes, a: Bytes, order: &[u64]) -> Bytes {
    let fill = |rgb| block(&[vec![1, 0], px(2, 20.), color(rgb), vec![40, 1]]);
    upload(&[
        node(1, 1, row, order),
        node(2, 4, a, &[5]),
        node(5, 4, block(&[px(1, 20.)]), &[]),
        node(3, 4, fill([0, 0, 255]), &[]),
        node(4, 4, fill([255, 255, 255]), &[]),
    ])
}
fn check_cold(r: &mut Raster, full: Bytes) {
    let mut cold = Raster::default();
    cold.tree.upload(&full).unwrap();
    assert_eq!(draw(r), draw(&mut cold));
    for (id, frame) in cold.tree.frames() {
        assert_eq!(bounds(r, id), frame);
    }
    for x in [5., 35., 55., 80., 95., 114., 116.] {
        assert_eq!(r.hit_at(x, 10.), cold.hit_at(x, 10.));
    }
}
#[test]
fn row_offers_offsets_and_paint_invalidate_only_their_readers() {
    let row = |gap, pad| row_attrs(px(1, 120.), gap, pad);
    let ids = [2, 3, 4];
    let swap = [2, 4, 3];
    let mut r = Raster::default();
    r.tree
        .upload(&band(row(5., 5.), red(px(1, 20.)), &ids))
        .unwrap();
    draw(&mut r);
    for (w, share) in [(60., 20.), (20., 40.)] {
        let a = red(px(1, w));
        r.tree.patch(&set(2, 4, a.clone())).unwrap();
        check_cold(&mut r, band(row(5., 5.), a, &ids));
        assert_eq!(bounds(&r, 1).width, 120.);
        assert_eq!((bounds(&r, 3).width, bounds(&r, 4).width), (share, share));
        // Width shares feed x placement, not the unchanged height basis.
        assert!(ran(&r, Flow, 1) && ran(&r, Place, 1) && !ran(&r, Reflow, 1));
        assert!((3..=5).all(|id| !ran(&r, Measure, id)));
    }
    r.tree.patch(&order(1, 1, &swap)).unwrap();
    check_cold(&mut r, band(row(5., 5.), red(px(1, 20.)), &swap));
    assert!(
        r.tree
            .layers
            .trace
            .iter()
            .all(|(p, _, _)| !matches!(p, Measure | Flow))
    );
    assert!(ran(&r, Place, 3) && ran(&r, Place, 4) && !ran(&r, Place, 2));
    assert_eq!(r.hit_at(80., 10.), Some((3, 5., 5.)));

    // A's intrinsic width is still 20 (its child), but it now participates in the split.
    let a = red(vec![1, 0]);
    r.tree.patch(&set(2, 4, a.clone())).unwrap();
    check_cold(&mut r, band(row(5., 5.), a.clone(), &swap));
    assert!(ran(&r, Flow, 1) && !ran(&r, Measure, 1));
    assert!((2..=4).all(|id| bounds(&r, id).width == 100. / 3.));
    for (gap, pad, share) in [(8., 5., 94. / 3.), (8., 7., 30.)] {
        let attrs = row(gap, pad);
        r.tree.patch(&set(1, 1, attrs.clone())).unwrap();
        check_cold(&mut r, band(attrs, a.clone(), &swap));
        assert_eq!(bounds(&r, 3).width, share);
        assert!((2..=5).all(|id| !ran(&r, Measure, id)));
    }
    r.tree
        .patch(&set(
            3,
            4,
            block(&[vec![1, 0], px(2, 20.), color([0, 255, 0]), vec![40, 1]]),
        ))
        .unwrap();
    draw(&mut r);
    assert_eq!(
        r.tree.layers.trace,
        [(Compose, 3, Outputs::Draw), (Compose, 1, Outputs::Draw)]
    );
}

#[test]
fn content_row_does_not_share_viewport_width_and_nested_auto_column_reads_its_own_offer() {
    let attrs = |width| row_attrs(width, 4., 2.);
    let col = block(&[]);
    let full = |width| {
        upload(&[
            node(1, 1, attrs(width), &[2, 3, 4]),
            node(2, 3, col.clone(), &[5]),
            node(5, 4, block(&[vec![1, 0]]), &[6]),
            node(6, 4, block(&[px(1, 18.), px(2, 7.)]), &[]),
            node(3, 4, block(&[vec![1, 0]]), &[7]),
            node(7, 4, block(&[px(1, 11.)]), &[]),
            node(4, 4, block(&[vec![1, 0]]), &[8]),
            node(8, 4, block(&[px(1, 29.)]), &[]),
        ])
    };
    let mut r = Raster::default();
    r.tree.upload(&full(px(1, 100.))).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 5).width, 18.);
    assert_eq!(bounds(&r, 3).width, 35.);
    for (width, outer, b, c) in [(vec![1, 1], 70., 11., 29.), (vec![1, 0], 180., 75., 75.)] {
        r.tree.patch(&set(1, 1, attrs(width.clone()))).unwrap();
        check_cold(&mut r, full(width));
        assert_eq!(
            (
                bounds(&r, 1).width,
                bounds(&r, 3).width,
                bounds(&r, 4).width
            ),
            (outer, b, c)
        );
        assert_eq!((bounds(&r, 2).width, bounds(&r, 5).width), (18., 18.));
    }
}

#[test]
fn equal_glyph_advances_repaint_without_redistributing_row_offers_or_hits() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(
                1,
                1,
                block(&[
                    px(1, 120.),
                    [18, 0, 0, 9].into_iter().chain(*b"monospace").collect(),
                ]),
                &[2, 3],
            ),
            node(2, 5, text("AA"), &[]),
            node(3, 4, block(&[vec![1, 0], px(2, 20.), vec![40, 1]]), &[]),
        ]))
        .unwrap();
    let aa = draw(&mut r).to_vec();
    let box_before = bounds(&r, 3);
    r.tree.patch(&set(2, 5, text("BB"))).unwrap();
    assert_ne!(draw(&mut r), aa);
    assert_eq!(bounds(&r, 3), box_before);
    assert_eq!(
        r.tree.layers.trace,
        [
            (Measure, 2, Outputs::Both),
            (Compose, 2, Outputs::Draw),
            (Compose, 1, Outputs::Draw)
        ]
    );
}

#[test]
fn narrowing_a_row_clips_the_existing_glyph_run_without_measuring_it_again() {
    let mut r = Raster::default();
    let width = |w| block(&[px(1, w)]);
    r.tree
        .upload(&upload(&[
            node(1, 1, width(140.), &[2]),
            node(2, 5, text("WWWWWW"), &[]),
        ]))
        .unwrap();
    let wide = draw(&mut r).to_vec();
    let frame = bounds(&r, 2);
    let mut cropped = wide.clone();
    for line in cropped.chunks_exact_mut(180 * 4) {
        line[21 * 4..].fill(0)
    }
    assert_ne!(cropped, wide);
    r.tree.patch(&set(1, 1, width(20.5))).unwrap();
    assert_eq!(draw(&mut r), cropped);
    assert_eq!(bounds(&r, 2), frame);
    assert!(!ran(&r, Measure, 2));
    r.tree.patch(&set(1, 1, width(140.))).unwrap();
    assert_eq!(draw(&mut r), wide);
}

#[test]
fn row_rejects_unsupported_fields_and_malformed_shapes_without_losing_pending_edits() {
    let base = |w| band(row_attrs(px(1, 120.), 5., 5.), red(px(1, w)), &[2, 3, 4]);
    let mut r = Raster::default();
    r.tree.upload(&base(20.)).unwrap();
    draw(&mut r);
    r.tree.patch(&set(2, 4, red(px(1, 60.)))).unwrap();
    let pending = r.tree.encode();
    for bad in [
        block(&[vec![2, 255]]),
        block(&[vec![1, 3]]),
        block(&[vec![5, 3]]),
        text("not row data"),
    ] {
        assert!(r.tree.patch(&set(1, 1, bad)).is_err());
    }
    for (wire, err) in [
        (4, "El supports at most one child"),
        (5, "leaf has children"),
        (6, "leaf has children"),
        (9, "image requires a logical PNG or JPEG source"),
        (10, "unsupported element 10 in first slice"),
    ] {
        let full = upload(&[
            node(10, wire, block(&[]), &[20, 30]),
            node(20, 6, block(&[]), &[]),
            node(30, 6, block(&[]), &[]),
        ]);
        assert_eq!(r.tree.patch(&at(full, 1)).unwrap_err(), err);
    }
    assert!(r.tree.patch(&order(1, 1, &[2, 3, 3])).is_err());
    assert_eq!(r.tree.encode(), pending);
    assert_eq!(r.hit_at(80., 10.), Some((4, 5., 5.)));
    check_cold(&mut r, base(60.));
    assert_eq!(r.hit_at(80., 10.), Some((3, 10., 5.)));
    let centered = block(&[vec![5, 1]]);
    r.tree.patch(&set(1, 1, centered.clone())).unwrap();
    check_cold(&mut r, band(centered, red(px(1, 60.)), &[2, 3, 4]));
    assert_eq!(bounds(&r, 1).x, 0.);
}
