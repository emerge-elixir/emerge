use super::*;

fn fill_image() -> Bytes {
    block(&[path(WIDE), vec![2, 0], vec![40, 1]])
}

#[test]
fn a_column_share_uses_the_final_charges_before_width_flow_or_only_at_placement() {
    for late in [false, true] {
        let host = if late {
            block(&[px(1, 160.), px(2, 48.)])
        } else {
            block(&[px(2, 48.)])
        };
        let leaf = block(&[path(WIDE), vec![2, 0], vec![5, 2], vec![40, 1]]);
        let sibling = |h| block(&[px(1, 8.), px(2, h), color([0, 0, 255]), vec![40, 1]]);
        let doc = |h| {
            upload(&[
                node(1, 3, host.clone(), &[2, 3]),
                node(2, 9, leaf.clone(), &[]),
                node(3, 4, sibling(h), &[]),
            ])
        };
        let mut r = Raster::default();
        let mut l = loader();
        r.tree.upload(&doc(8.)).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 2).width, 64.);
        load(&mut r.tree, &mut l);
        for h in [8., 16., 8.] {
            r.tree.patch(&set(3, 4, sibling(h))).unwrap();
            let bytes = draw(&mut r).to_vec();
            let w = 2. * (48. - h as f32);
            assert_eq!((bounds(&r, 2).width, bounds(&r, 3).y), (w, w / 2.));
            assert_eq!(bounds(&r, 2).x, if late { 160. - w } else { 0. });
            let mut fresh = Raster::default();
            fresh.tree.upload(&doc(h)).unwrap();
            load(&mut fresh.tree, &mut l);
            assert_eq!(draw(&mut fresh), bytes);
            assert_eq!(r.hit_at(3., w / 2. + 2.), Some((3, 3., 2.)));
        }
        // An unallotted content sibling can depend on width reflow. A fixed
        // cross extent only needs Image width at placement; an auto Column can't guess it.
        let edit = set(3, 4, block(&[px(1, 8.)]));
        let accepted = r.tree.encode();
        if late {
            r.tree.patch(&edit).unwrap();
            draw(&mut r);
            assert_eq!((bounds(&r, 2).width, bounds(&r, 2).x), (96., 64.));
        } else {
            assert!(
                r.tree
                    .patch(&edit)
                    .unwrap_err()
                    .contains("share depends on reflow")
            );
            assert_eq!(r.tree.encode(), accepted);
        }
    }
}

#[test]
fn inherited_height_reaches_nested_wrappers_before_their_peers_split_width() {
    for kind in [4, 1, 3] {
        let mut r = Raster::default();
        r.tree
            .upload(&upload(&[
                node(1, 1, block(&[vec![1, 0], vec![2, 0]]), &[2, 4]),
                node(2, kind, block(&[vec![2, 0]]), &[3]),
                node(3, 9, fill_image(), &[]),
                node(4, 4, block(&[vec![1, 0], vec![2, 0]]), &[]),
            ]))
            .unwrap();
        load(&mut r.tree, &mut loader());
        for h in [100., 300., 80., 0., 300.] {
            r.tree.refresh(1000., h, 1.).unwrap();
            let w = 2. * h;
            let outer = if kind == 4 { w } else { w.max(200.) };
            assert_eq!((bounds(&r, 3).width, bounds(&r, 3).height), (w, h));
            assert_eq!(
                (bounds(&r, 2).width, bounds(&r, 4).x, bounds(&r, 4).width),
                (outer, outer, 1000. - outer)
            );
        }
        let old = r.tree.encode();
        assert!(r.tree.patch(&set(1, 1, block(&[vec![1, 0]]))).is_err());
        assert_eq!(r.tree.encode(), old);
    }
}

#[test]
fn wrapped_row_breaks_use_resolved_image_widths_before_offering_a_line() {
    let mut r = Raster::default();
    let host = |w| block(&[px(1, w), px(2, 150.)]);
    r.tree
        .upload(&upload(&[
            node(1, 2, host(450.), &[2, 3]),
            node(2, 9, fill_image(), &[]),
            node(3, 9, fill_image(), &[]),
        ]))
        .unwrap();
    let mut l = loader();
    load(&mut r.tree, &mut l);
    load(&mut r.tree, &mut l);
    for (w, x, y) in [(450., 0., 150.), (600., 300., 0.), (450., 0., 150.)] {
        r.tree.patch(&set(1, 2, host(w))).unwrap();
        r.tree.refresh(1000., 600., 1.).unwrap();
        let f = bounds(&r, 3);
        assert_eq!((f.x, f.y, f.width, f.height), (x, y, 300., 150.));
    }
    let old = r.tree.encode();
    assert!(r.tree.patch(&set(1, 2, block(&[px(1, 450.)]))).is_err());
    assert_eq!(old, r.tree.encode());
}

#[test]
fn transposed_bounds_project_display_pixels_inside_the_allocated_insets() {
    let mut r = Raster::default();
    let mut l = loader();
    let leaf = |name, w, h| {
        block(&[
            path(name),
            w,
            h,
            [vec![3, 0], 2f64.to_be_bytes().to_vec()].concat(),
        ])
    };
    let folded = [vec![2, 4], number(2, 40.), number(2, 100.)].concat();
    let doc = |a| {
        upload(&[
            node(1, 1, block(&[px(1, 240.), px(2, 40.)]), &[2, 3]),
            node(2, 9, a, &[]),
            node(3, 4, block(&[px(1, 8.), px(2, 8.), vec![40, 1]]), &[]),
        ])
    };
    let first = leaf(WIDE, vec![1, 1], px(2, 40.));
    r.tree.upload(&doc(first.clone())).unwrap();
    load(&mut r.tree, &mut l);
    // At scale 1.5 intrinsic 200 texels stay 200; declared padding becomes 6.
    for (a, x) in [
        (first.clone(), 114.),
        (leaf(WIDE, vec![1, 1], folded), 206.),
        (first, 114.),
        (
            leaf("test_assets/turn_right.jpeg", vec![1, 5, 1, 1], vec![2, 0]),
            33.,
        ),
        (
            leaf(WIDE, vec![1, 1], bounded::bound(2, 4, 2., None, false)),
            6.,
        ),
    ] {
        r.tree.patch(&set(2, 9, a.clone())).unwrap();
        if r.tree.needs_images() {
            load(&mut r.tree, &mut l);
        }
        r.draw(400., 100., 1.5).unwrap();
        assert_eq!(bounds(&r, 3).x, x);
        let mut fresh = Raster::default();
        fresh.tree.upload(&doc(a)).unwrap();
        load(&mut fresh.tree, &mut l);
        assert_eq!(fresh.draw(400., 100., 1.5).unwrap(), r.pixels());
        assert_eq!(r.hit_at(x + 3., 3.), Some((3, 2., 2.)));
    }
}

#[test]
fn sparse_kept_move_uses_the_final_column_weights_and_rolls_back_a_cycle() {
    let mut r = Raster::default();
    let wrapped = block(&[vec![2, 0]]);
    let weighted = |weight| block(&[images::path(WIDE), [vec![2], number(3, weight)].concat()]);
    let doc = |moved, w| {
        upload(&[
            node(1, 1, block(&[px(2, 60.)]), &[2, 3]),
            node(2, 4, wrapped.clone(), if moved { &[] } else { &[4] }),
            node(3, 3, wrapped.clone(), if moved { &[4, 5] } else { &[5] }),
            node(4, 9, fill_image(), &[]),
            node(5, 9, weighted(w), &[]),
        ])
    };
    let mut l = loader();
    r.tree.upload(&doc(false, 1.)).unwrap();
    load(&mut r.tree, &mut l);
    load(&mut r.tree, &mut l);
    draw(&mut r);
    let transfer = |valid| {
        patch(
            1,
            1,
            &[
                edges(1, 1, &[2, 3]),
                edges(2, 4, &[]),
                node(
                    3,
                    3,
                    if valid { wrapped.clone() } else { block(&[]) },
                    &[4, 5],
                ),
                attr_record(5, 9, weighted(2.)),
            ],
        )
    };
    let before = r.tree.encode();
    assert!(r.tree.patch(&transfer(false)).is_err());
    assert_eq!(before, r.tree.encode());
    r.tree.patch(&transfer(true)).unwrap();
    draw(&mut r);
    assert_eq!(
        (bounds(&r, 4).width, bounds(&r, 5).width, bounds(&r, 5).y),
        (40., 80., 20.)
    );
    let mut fresh = Raster::default();
    fresh.tree.upload(&doc(true, 2.)).unwrap();
    load(&mut fresh.tree, &mut l);
    load(&mut fresh.tree, &mut l);
    assert_eq!(draw(&mut fresh), r.pixels());
}

#[test]
fn mixed_image_height_uses_its_display_basis_for_the_early_group_and_place() {
    let mut r = Raster::default();
    let leaf = |name| block(&[path(name), vec![2, 4, 1, 0]]); // min(content, fill)
    r.tree
        .upload(&upload(&[
            node(1, 3, block(&[px(2, 180.)]), &[2, 3]),
            node(2, 9, leaf(WIDE), &[]),
            node(3, 9, fill_image(), &[]),
        ]))
        .unwrap();
    let mut l = loader();
    load(&mut r.tree, &mut l);
    load(&mut r.tree, &mut l);
    draw(&mut r);
    assert_eq!((bounds(&r, 2).width, bounds(&r, 3).width), (180., 180.));
    r.tree.patch(&set(2, 9, leaf(GRID))).unwrap();
    load(&mut r.tree, &mut l);
    draw(&mut r);
    assert_eq!(
        (bounds(&r, 2).width, bounds(&r, 3).width, bounds(&r, 3).y),
        (10., 348., 6.)
    );
    // Two fill requests don't infer from each other. The first height's
    // content cap is still the six source texels, not a reflow at the
    // viewport's offered width (180). The second image gets the other 174px.
    r.tree
        .patch(&set(
            2,
            9,
            block(&[path(GRID), vec![1, 0], vec![2, 4, 1, 0]]),
        ))
        .unwrap();
    draw(&mut r);
    assert_eq!(
        (
            bounds(&r, 2).width,
            bounds(&r, 2).height,
            bounds(&r, 3).width
        ),
        (180., 6., 348.)
    );
}
