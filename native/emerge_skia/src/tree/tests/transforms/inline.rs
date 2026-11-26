use super::*;

#[test]
fn a_direct_text_turn_projects_preferred_demand_and_places_one_turned_phrase() {
    let source = |d| {
        block(&[
            text("AB CD EF")[2..].to_vec(),
            px(1, 80.),
            px(2, 24.),
            number(79, d),
            vec![40, 1],
        ])
    };
    let scene = |p, d| {
        upload(&[
            node(1, 1, axes(170., 130.), &[2, 4]),
            node(2, 7, p, &[3]),
            node(3, 5, source(d), &[]),
            node(4, 4, marker(), &[]),
        ])
    };
    let content = block(&[vec![1, 1]]);
    let mut plain = Raster::default();
    plain.tree.upload(&scene(axes(160., 120.), 0.)).unwrap();
    draw(&mut plain);
    let ink = bounds(&plain, 3);
    let mut r = Raster::default();
    r.tree.upload(&scene(content.clone(), 90.)).unwrap();
    let diagonal = 104. * std::f32::consts::FRAC_1_SQRT_2;
    for (d, w, h, solves) in [
        (90., 24., 80., 1),
        (-90., 24., 80., 0),
        (-45., diagonal, diagonal, 1),
        (180., 80., 24., 1),
        (0., 80., 24., 0),
    ] {
        r.tree.patch(&set(3, 5, source(d))).unwrap();
        let old = r.tree.layers.paragraphs.solves;
        draw(&mut r);
        assert_eq!(r.tree.layers.paragraphs.solves - old, solves);
        assert_eq!(r.tree.encode(), scene(content.clone(), d));
        assert!((bounds(&r, 2).width - w).abs() < 1e-4);
        assert!((bounds(&r, 2).height - h).abs() < 1e-4);
        assert_eq!(bounds(&r, 4).x, bounds(&r, 2).width);
        // Preferred dimensions can differ from the actual phrase. Its own
        // slot encloses the turned unwrapped line, not several upright words.
        let (s, c) = (d as f32).to_radians().sin_cos();
        let (sw, sh) = (
            c.abs() * ink.width + s.abs() * ink.height,
            s.abs() * ink.width + c.abs() * ink.height,
        );
        let slot = bounds(&r, 3);
        assert!((slot.width - sw).abs() < 1e-4 && (slot.height - sh).abs() < 1e-4);
        let (dx, dy) = (2. - ink.width / 2., 3. - ink.height / 2.);
        hit(
            &r,
            [sw / 2. + c * dx - s * dy, sh / 2. + s * dx + c * dy],
            3,
            [2., 3.],
        );
        check_fresh(&r);
    }
}

#[test]
fn a_turned_phrase_clears_a_narrow_float_band_and_aligns_as_one_item() {
    let paper = |width, align| block(&[px(1, width), vec![30, align]]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 7, paper(96., 2), &[2, 3, 4]),
            node(2, 4, document::margin(2, 50., 32.), &[]),
            node(3, 5, text("I"), &[]),
            node(4, 4, block(&[number(79, 45.), vec![40, 1]]), &[5]),
            node(5, 5, text("AB CD EF"), &[]),
        ]))
        .unwrap();
    for (w, align) in [(96., 2), (150., 2), (96., 1), (96., 2)] {
        r.tree.patch(&set(1, 7, paper(w, align))).unwrap();
        draw(&mut r);
        let (prefix, phrase) = (bounds(&r, 3), bounds(&r, 4));
        assert_eq!(prefix.y, 0.);
        let right = w as f32 - 50.;
        if w == 150. {
            assert!(phrase.y.abs() < 1e-4);
            assert!((phrase.x - (prefix.x + prefix.width)).abs() < 1e-4);
            assert!((phrase.x + phrase.width - right).abs() < 1e-4);
        } else {
            // Finishing the prefix's line must not shift the pending phrase.
            let factor = if align == 2 { 1. } else { 0.5 };
            assert!((prefix.x - (right - prefix.width) * factor).abs() < 1e-4);
            assert!((phrase.y - 32.).abs() < 1e-4);
            assert!((phrase.x - (w as f32 - phrase.width) * factor).abs() < 1e-4);
        }
        check_fresh(&r);
    }
}

#[test]
fn vertical_alignment_places_each_inline_owner_within_the_tallest_occupied_slot() {
    let prefix = |align| block(&[text("I ")[2..].to_vec(), vec![6, align], vec![40, 1]]);
    let small = |align| block(&[number(79, 90.), vec![6, align], vec![40, 1]]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 7, axes(160., 150.), &[2, 3, 5]),
            node(2, 5, prefix(0), &[]),
            node(3, 4, small(0), &[4]),
            node(4, 5, text("AB CD"), &[]),
            node(5, 4, block(&[number(79, -90.)]), &[6]),
            node(6, 5, text("AB CD EF GH IJ"), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    let tall = bounds(&r, 5);
    let [first, turned] = [2, 3].map(|id| bounds(&r, id));
    assert!(first.height < turned.height && turned.height < tall.height && tall.height < 150.);
    for (a, b) in [(2, 1), (1, 2), (0, 0)] {
        let solves = r.tree.layers.paragraphs.solves;
        r.tree.patch(&set(2, 5, prefix(a))).unwrap();
        r.tree.patch(&set(3, 4, small(b))).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 5), tall);
        for (id, before, align) in [(2, first, a), (3, turned, b)] {
            let dy = (tall.height - before.height) * f32::from(align) / 2.;
            let site = bounds(&r, id);
            assert!(
                (site.y - dy).abs() < 1e-4,
                "id {id}: {site:?}, expected y={dy}"
            );
            assert_eq!(
                (site.x, site.width, site.height),
                (before.x, before.width, before.height)
            );
            let center = [site.x + site.width / 2., site.y + site.height / 2.];
            let local = if id == 2 {
                [site.width, site.height]
            } else {
                [site.height, site.width]
            };
            hit(&r, center, id, local.map(|v| f64::from(v / 2.)));
        }
        assert_eq!(r.tree.layers.paragraphs.solves - solves, 1);
        check_fresh(&r);
    }
}

#[test]
fn nested_opposite_quarter_turns_keep_the_glyphs_upright_inside_the_turned_wrapper() {
    let source = |tag, d| block(&[text("AB CD EF")[2..].to_vec(), number(tag, d), vec![40, 1]]);
    let wrapper = block(&[
        number(79, 90.),
        [vec![3, 0], 2f64.to_be_bytes().to_vec()].concat(),
        color([255, 0, 0]),
        vec![40, 1],
    ]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 7, axes(170., 120.), &[2, 3, 5]),
            node(2, 5, text("I "), &[]),
            node(3, 4, wrapper, &[4]),
            node(4, 5, source(79, -90.), &[]),
            node(5, 5, text("Z"), &[]),
        ]))
        .unwrap();
    let first = draw(&mut r).to_vec();
    let f = bounds(&r, 3);
    let w = f.width - 4.;
    let h = f.height - 4.;
    assert!(w > 3. * h);
    assert!((bounds(&r, 5).x - (f.x + f.width)).abs() < 1e-4);
    let point = [f.x + 7., f.y + 5.];
    hit(&r, point, 4, [5., 3.]);
    hit(&r, [f.x + 1., f.y + 4.], 3, [4., f64::from(f.width - 1.)]);
    let n = r.tree.layers.paragraphs.solves;
    r.tree.patch(&set(4, 5, source(79, 90.))).unwrap();
    assert_ne!(draw(&mut r), first);
    assert_eq!(r.tree.layers.paragraphs.solves, n);
    hit(&r, point, 4, [w - 5., h - 3.].map(f64::from));
    r.tree.patch(&set(4, 5, source(33, 90.))).unwrap();
    draw(&mut r);
    assert!((bounds(&r, 3).width - f.height).abs() < 1e-4);
    check_fresh(&r);
    r.tree.patch(&set(4, 5, source(79, -90.))).unwrap();
    assert_eq!(draw(&mut r), first);
    hit(&r, point, 4, [5., 3.]);
}

#[test]
fn an_inline_wrapper_does_not_give_its_turned_text_a_block_height_offer() {
    let owner = |float: bool| {
        let a = [
            px(2, 96.),
            [vec![3, 0], 2f64.to_be_bytes().to_vec()].concat(),
        ];
        block(
            &a.into_iter()
                .chain(float.then(|| vec![5, 0]))
                .collect::<Vec<_>>(),
        )
    };
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 1, axes(170., 130.), &[2, 5]),
            node(2, 7, block(&[vec![1, 1]]), &[3]),
            node(3, 4, owner(false), &[4]),
            node(
                4,
                5,
                block(&[
                    text("AB CD EF")[2..].to_vec(),
                    px(1, 80.),
                    vec![2, 0],
                    number(79, 90.),
                    vec![40, 1],
                ]),
                &[],
            ),
            node(5, 4, axes(100., 100.), &[]),
        ]))
        .unwrap();
    let move_to = |paragraph| {
        patch(
            1,
            1,
            &[
                edges(1, 1, &[2, 5]),
                edges(2, 7, if paragraph { &[3] } else { &[] }),
                edges(5, 4, if paragraph { &[] } else { &[3] }),
            ],
        )
    };
    // Neither floating the wrapper nor moving it as a closed branch names its Text.
    for (edit, inline) in [
        (None, true),
        (Some(set(3, 4, owner(true))), false),
        (Some(set(3, 4, owner(false))), true),
        (Some(move_to(false)), false),
        (Some(move_to(true)), true),
    ] {
        if let Some(edit) = edit {
            r.tree.patch(&edit).unwrap();
        }
        draw(&mut r);
        if inline {
            assert_eq!(bounds(&r, 2), rect(0., 0., 23.359375, 96.)); // 19.359375 + two 2px insets
            hit(&r, [5., 5.], 4, [3., 16.359375]); // 2px wrapper inset, inverse +90°
        } else {
            assert_eq!(bounds(&r, 4), rect(2., 2., 92., 80.)); // 96 - 4 offered to the local height
            hit(&r, [5., 5.], 4, [3., 89.]);
        }
        check_fresh(&r);
    }
}

#[test]
fn a_turned_span_does_not_make_its_paragraph_card_depend_on_a_pack_share() {
    let span = |angle| block(&[px(1, 80.), vec![2, 0], number(79, angle), vec![40, 1]]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, block(&[vec![1, 1], px(2, 160.)]), &[2, 5]),
            node(2, 2, block(&[vec![1, 0]]), &[3, 4]),
            node(3, 6, axes(20., 10.), &[]),
            node(4, 6, axes(20., 10.), &[]),
            node(5, 14, block(&[vec![1, 1], vec![2, 0]]), &[6]),
            node(6, 7, block(&[vec![1, 1]]), &[7]),
            node(7, 4, span(90.), &[8]),
            node(8, 5, text("AB CD EF"), &[]),
        ]))
        .unwrap();
    for (d, w) in [(90., 19.359375), (0., 80.), (90., 19.359375)] {
        r.tree.patch(&set(7, 4, span(d))).unwrap();
        let n = r.tree.layers.paragraphs.solves;
        draw(&mut r);
        assert_eq!(r.tree.layers.paragraphs.solves - n, 1);
        assert_eq!((bounds(&r, 5).width, bounds(&r, 6).width), (w, w));
        hit(
            &r,
            [2., bounds(&r, 6).y + 3.],
            7,
            if d == 0. { [2., 3.] } else { [3., 17.359375] },
        );
        check_fresh(&r);
    }
    // Lift the same filling El out of prose: it now reads the pack share,
    // which is decided after its width-sensitive WrappedRow sibling.
    let accepted = r.tree.encode();
    assert!(
        r.tree
            .patch(&patch(5, 5, &[edges(5, 14, &[7]), edges(7, 4, &[8])]))
            .unwrap_err()
            .contains("Column share depends on that same width")
    );
    assert_eq!(r.tree.encode(), accepted);
}

#[test]
fn equal_bounds_do_not_hide_a_half_turn_or_install_it_early() {
    let source = |tag, angle| block(&[text("AB")[2..].to_vec(), number(tag, angle), vec![40, 1]]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 7, axes(100., 80.), &[2]),
            node(2, 4, block(&[]), &[3]),
            node(3, 5, source(79, 0.), &[]),
        ]))
        .unwrap();
    let upright = draw(&mut r).to_vec();
    let f = bounds(&r, 3);
    let point = [2., 3.];
    hit(&r, point, 3, [2., 3.]);
    r.tree.patch(&set(3, 5, source(79, 180.))).unwrap();
    assert!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("not installed".into()))
            .is_err()
    );
    assert_eq!(bounds(&r, 3), f);
    assert_eq!(r.pixels(), upright);
    hit(&r, point, 3, [2., 3.]);
    let reversed = draw(&mut r).to_vec();
    assert_ne!(reversed, upright);
    hit(
        &r,
        point,
        3,
        [(f.width - 2.).into(), (f.height - 3.).into()],
    );
    // The two tags have the same signed pose and the same line footprint at 180°.
    for tag in [33, 79] {
        r.tree.patch(&set(3, 5, source(tag, 180.))).unwrap();
        assert_eq!(draw(&mut r), reversed);
        assert!(!ran(&r, Measure, 1) && !ran(&r, Reflow, 1));
        hit(&r, point, 3, [f.width - 2., f.height - 3.].map(f64::from));
    }
    r.tree.patch(&set(3, 5, source(79, 0.))).unwrap();
    assert_eq!(draw(&mut r), upright);
    hit(&r, point, 3, [2., 3.]);
}
