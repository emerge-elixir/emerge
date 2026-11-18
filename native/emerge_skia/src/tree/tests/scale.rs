use super::*;

fn zoom(s: f64) -> Bytes {
    block(&[number(78, s)])
}
fn label(s: &str, size: f64) -> Bytes {
    let raw = text(s);
    block(&[raw[2..].to_vec(), number(16, size)])
}
fn metric(r: &Raster, id: u64) -> f32 {
    r.tree.layers.runs[r.tree.elements.ids[&id]].metrics.size
}

#[test]
fn root_and_nested_thresholds_match_main_and_a_net_zero_batch_runs_no_layout() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 4, block(&[number(78, 1e-9), px(1, 1e9)]), &[2]),
            node(2, 4, block(&[px(1, 1e9)]), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 1).width, 1.);
    assert_eq!(bounds(&r, 2).width, 1.);
    let nested = set(2, 4, block(&[px(1, 1e9), number(78, 1.)]));
    let flat = set(2, 4, block(&[px(1, 1e9)]));
    r.tree.patch(&nested).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 1).width, 1e9 * f32::EPSILON);
    assert_eq!(bounds(&r, 2).width, bounds(&r, 1).width);
    r.tree.patch(&flat).unwrap();
    r.tree.patch(&nested).unwrap();
    assert!(r.tree.layers.worksets_empty());
    draw(&mut r);
    assert!(r.tree.layers.trace.is_empty());
    r.tree.patch(&flat).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 1).width, 1.);
}

#[test]
fn closed_moved_branch_and_promotion_take_the_accepted_not_last_drawn_font_inputs() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 1, block(&[]), &[2, 3]),
            node(2, 3, zoom(2.), &[4]),
            node(3, 3, zoom(0.5), &[]),
            node(4, 3, block(&[number(16, 12.)]), &[5, 6]),
            node(5, 5, text("Pay"), &[]),
            node(6, 5, label("Pay", 12.), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(metric(&r, 5), 24.);
    r.tree.patch(&set(3, 3, zoom(1.25))).unwrap();
    r.tree
        .patch(&patch(
            1,
            1,
            &[edges(1, 1, &[2, 3]), edges(2, 3, &[]), edges(3, 3, &[4])],
        ))
        .unwrap();
    draw(&mut r);
    assert_eq!(metric(&r, 5), 15.);
    assert_eq!(metric(&r, 6), 15.);
    r.tree
        .patch(&set(4, 3, block(&[number(16, 12.), number(78, 0.5)])))
        .unwrap();
    r.tree.patch(&patch(1, 4, &[])).unwrap();
    draw(&mut r);
    assert_eq!(metric(&r, 5), 6.);
    assert_eq!(metric(&r, 6), 6.);
    r.tree
        .patch(&set(
            6,
            5,
            block(&[text("Pay")[2..].to_vec(), number(78, 2.)]),
        ))
        .unwrap();
    draw(&mut r);
    assert_eq!(metric(&r, 6), 6., "inherit the declaration's resolved size");
}

#[test]
fn default_font_scales_at_the_text_site_until_a_size_is_declared() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, zoom(1.5), &[2, 3]),
            node(2, 5, text("Pay"), &[]),
            node(3, 3, zoom(0.5), &[4, 5]),
            node(4, 5, text("Pay"), &[]),
            node(5, 5, label("Pay", 16.), &[]),
        ]))
        .unwrap();
    let sizes = |r: &Raster| [2, 4, 5].map(|id| metric(r, id));
    for display in [1., 1.5, 2., 1.] {
        r.tree.refresh(180., 170., display).unwrap();
        assert_eq!(sizes(&r), [24., 12., 12.].map(|s| s * display));
    }
    // Equal size at the parent, but its descendants now inherit a declaration.
    r.tree
        .patch(&set(1, 3, block(&[number(78, 1.5), number(16, 16.)])))
        .unwrap();
    draw(&mut r);
    assert_eq!(sizes(&r), [24., 24., 12.]);
    r.tree.patch(&set(1, 3, zoom(1.5))).unwrap();
    draw(&mut r);
    assert_eq!(sizes(&r), [24., 12., 12.]);
}

#[test]
fn an_unused_default_need_not_fit_but_an_overflowing_text_default_rejects_atomically() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 4, zoom(1.5), &[2]),
            node(2, 5, text("Pay"), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    // 16 × 2^124 overflows f32, but the actual declaration resolves to 32.
    let large = 2f64.powi(124);
    r.tree
        .patch(&patch(
            1,
            1,
            &[
                node(1, 4, zoom(large), &[2]),
                attr_record(2, 5, label("Pay", 32. / large)),
            ],
        ))
        .unwrap();
    let pending = r.tree.encode();
    assert_eq!(
        r.tree.patch(&set(2, 5, text("Pay"))).unwrap_err(),
        "font size exceeds device range"
    );
    assert_eq!(r.tree.encode(), pending);
    draw(&mut r);
    assert_eq!(metric(&r, 2), 32.);
}

#[test]
fn invalid_or_unrepresentable_scale_preserves_pending_declaration_and_submitted_hits() {
    let mut r = Raster::default();
    let hit = block(&[px(1, 20.), px(2, 10.), vec![40, 1]]);
    r.tree
        .upload(&upload(&[node(1, 3, zoom(1.), &[2]), node(2, 4, hit, &[])]))
        .unwrap();
    draw(&mut r);
    r.tree.patch(&set(1, 3, zoom(2.))).unwrap();
    let pending = r.tree.encode();
    for bad in [0., -1., f64::NAN, f64::INFINITY, f64::MAX] {
        assert!(r.tree.patch(&set(1, 3, zoom(bad))).is_err());
        assert_eq!(r.tree.encode(), pending);
    }
    assert!(
        r.tree
            .patch(&set(2, 4, block(&[number(78, 3e38)])))
            .unwrap_err()
            .contains("layout scale")
    );
    assert!(
        r.tree
            .patch(&set(2, 4, block(&[px(1, 3e38)])))
            .unwrap_err()
            .contains("scaled length")
    );
    assert_eq!(r.hit_at(10., 5.), Some((2, 10., 5.)));
    assert!(r.prepare(180., 170., 3e38).is_err());
    assert_eq!(r.tree.encode(), pending);
    assert!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("failed handoff".into()))
            .is_err()
    );
    assert_eq!(r.hit_at(10., 5.), Some((2, 10., 5.)));
    draw(&mut r);
    assert_eq!(r.hit_at(10., 5.), Some((2, 5., 2.5)));
}

#[test]
fn many_sites_of_one_authored_font_size_and_a_bounded_warm_history() {
    let mut r = Raster::default();
    let ids: Vec<_> = (2..=70).collect();
    let mut nodes = vec![node(1, 3, block(&[]), &ids)];
    nodes.extend(ids.iter().map(|&i| {
        node(
            i,
            5,
            block(&[
                text("a")[2..].to_vec(),
                number(16, 12.),
                number(78, i as f64 / 64.),
            ]),
            &[],
        )
    }));
    r.tree.upload(&upload(&nodes)).unwrap();
    r.tree.refresh(120., 20., 1.).unwrap();
    assert_eq!(r.tree.layers.text.strikes.resident(), ids.len());
    for id in ids {
        assert_eq!(metric(&r, id), 12. * id as f32 / 64.);
    }
    r.tree.patch(&patch(1, 2, &[])).unwrap();
    for i in 1..=24 {
        r.tree
            .patch(&set(
                2,
                5,
                block(&[
                    text("a")[2..].to_vec(),
                    number(16, 12.),
                    number(78, i as f64),
                ]),
            ))
            .unwrap();
        r.tree.refresh(120., 20., 1.).unwrap();
        assert_eq!(metric(&r, 2), 12. * i as f32);
        assert!(r.tree.layers.text.strikes.resident() <= 9);
    }
}

#[test]
fn device_range_is_checked_after_scaling_authored_f64_values() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[node(
            1,
            4,
            block(&[number(78, 1e-38), px(1, 1e40), px(2, 1e40)]),
            &[],
        )]))
        .unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 1).width, (1e40 * f64::from(1e-38f32)) as f32);
    assert!(r.tree.patch(&set(1, 4, block(&[px(1, 1e40)]))).is_err());
}

#[test]
fn a_scaled_chip_wraps_and_displaces_but_does_not_reshape_its_siblings() {
    let mut r = Raster::default();
    let chip = |local| block(&[px(1, 20.), px(2, 10.), number(78, local)]);
    r.tree
        .upload(&upload(&[
            node(1, 3, block(&[]), &[2, 5]),
            node(2, 2, block(&[px(1, 42.), number(4, 1.5)]), &[3, 4]),
            node(3, 4, chip(1.), &[]),
            node(4, 4, chip(1.), &[]),
            node(5, 5, text("next"), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    let shapes = r.tree.layers.text.shapes;
    for (local, after) in [(2., 31.5), (1., 10.)] {
        r.tree.patch(&set(3, 4, chip(local))).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 5).y, after);
        assert!(!ran(&r, Measure, 4) && !ran(&r, Measure, 5));
        assert_eq!(r.tree.layers.text.shapes, shapes);
    }
}

#[test]
fn fractional_threshold_scales_a_length_before_narrowing_and_uses_the_actual_line_height() {
    let mut r = Raster::default();
    // (authored as f32) * 1.25 rounds down; main scales the f64 length first.
    let w = 32.0000018;
    let leaf = |i| node(i, 4, block(&[px(1, w), px(2, 8.)]), &[]);
    r.tree
        .upload(&upload(&[
            node(1, 2, block(&[px(1, 64.), number(78, 1.25)]), &[2, 3]),
            leaf(2),
            leaf(3),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 2).width, (w * 1.25) as f32);
    assert!(bounds(&r, 2).width > 40.);
    assert_eq!(bounds(&r, 3).y, 10.);
}
