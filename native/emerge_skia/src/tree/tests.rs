#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod allocations;
mod bounded;
mod column;
mod identity;
mod row;
mod scale;
mod topology;
mod weighted;
use super::*;
use layers::Pass::*;
use paint::Outputs;

type Bytes = Vec<u8>;
fn block(fields: &[Bytes]) -> Bytes {
    (fields.len() as u16)
        .to_be_bytes()
        .into_iter()
        .chain(fields.iter().flatten().copied())
        .collect()
}
fn number(tag: u8, v: f64) -> Bytes {
    [tag].into_iter().chain(v.to_be_bytes()).collect()
}
fn px(tag: u8, v: f64) -> Bytes {
    [tag, 2].into_iter().chain(v.to_be_bytes()).collect()
}
fn text(s: &str) -> Bytes {
    block(&[[21]
        .into_iter()
        .chain((s.len() as u16).to_be_bytes())
        .chain(s.bytes())
        .collect()])
}
fn node(id: u64, kind: u8, raw: Bytes, children: &[u64]) -> Bytes {
    id.to_be_bytes()
        .into_iter()
        .chain([kind, 3])
        .chain((raw.len() as u32).to_be_bytes())
        .chain(raw)
        .chain((children.len() as u16).to_be_bytes())
        .chain(children.iter().flat_map(|c| c.to_be_bytes()))
        .chain([0, 0])
        .collect()
}
fn upload(nodes: &[Bytes]) -> Bytes {
    let root = nodes
        .first()
        .map_or(0, |r| u64::from_be_bytes(r[..8].try_into().unwrap()));
    patch(0, root, nodes)
}
fn patch(target: u64, root: u64, nodes: &[Bytes]) -> Bytes {
    b"EMRG\x0a"
        .iter()
        .copied()
        .chain(target.to_be_bytes())
        .chain(root.to_be_bytes())
        .chain((nodes.len() as u32).to_be_bytes())
        .chain(nodes.iter().flatten().copied())
        .collect()
}
fn attr_record(id: u64, kind: u8, raw: Bytes) -> Bytes {
    id.to_be_bytes()
        .into_iter()
        .chain([kind, 1])
        .chain((raw.len() as u32).to_be_bytes())
        .chain(raw)
        .collect()
}
fn set(id: u64, kind: u8, raw: Bytes) -> Bytes {
    patch(id, id, &[attr_record(id, kind, raw)])
}
fn edges(id: u64, kind: u8, ids: &[u64]) -> Bytes {
    id.to_be_bytes()
        .into_iter()
        .chain([kind, 2])
        .chain((ids.len() as u16).to_be_bytes())
        .chain(ids.iter().flat_map(|i| i.to_be_bytes()))
        .chain([0, 0])
        .collect()
}
fn order(id: u64, kind: u8, ids: &[u64]) -> Bytes {
    patch(id, id, &[edges(id, kind, ids)])
}
fn at(mut full: Bytes, target: u64) -> Bytes {
    full[5..13].copy_from_slice(&target.to_be_bytes());
    full
}
fn color(rgb: [u8; 3]) -> Bytes {
    [12, 0, 0].into_iter().chain(rgb).collect()
}
fn draw(r: &mut Raster) -> &[u8] {
    r.draw(180., 170., 1.).unwrap()
}
fn bounds(r: &Raster, id: u64) -> Rect {
    r.tree.layers.frames[r.tree.elements.ids[&id]]
}
fn ran(r: &Raster, pass: Pass, id: u64) -> bool {
    r.tree
        .layers
        .trace
        .iter()
        .any(|&(p, k, _)| (p, k) == (pass, id))
}

#[test]
fn padded_text_updates_glyphs_even_when_measure_stops() {
    let full = |s| {
        upload(&[
            node(
                1,
                4,
                block(&[
                    [3, 0].into_iter().chain(4f64.to_be_bytes()).collect(),
                    [18, 0, 0, 9].into_iter().chain(*b"monospace").collect(),
                ]),
                &[2],
            ),
            node(2, 5, text(s), &[]),
        ])
    };
    let mut r = Raster::default();
    r.tree.upload(&full("AA")).unwrap();
    let aa = draw(&mut r).to_vec();
    let parent = bounds(&r, 1);
    let child = bounds(&r, 2);
    assert_eq!([child.x, child.y], [4., 4.]);
    assert_eq!(
        [parent.width, parent.height],
        [child.width + 8., child.height + 8.]
    );

    r.tree.patch(&set(2, 5, text("BB"))).unwrap();
    assert_ne!(draw(&mut r), aa);
    assert_eq!(bounds(&r, 1), parent);
    assert!(ran(&r, Measure, 2) && ran(&r, Compose, 2));
    assert!(!ran(&r, Measure, 1) && !ran(&r, Place, 2));
    r.tree.patch(&set(2, 5, text("BB"))).unwrap();
    draw(&mut r);
    assert!(r.tree.layers.trace.is_empty());
    assert_eq!(r.draws, 2);

    r.tree.patch(&set(2, 5, text("BBBB"))).unwrap();
    let mut cold = Raster::default();
    cold.tree.upload(&full("BBBB")).unwrap();
    assert_eq!(draw(&mut r), draw(&mut cold));
    assert!(ran(&r, Measure, 1));
    assert!(bounds(&r, 1).width > parent.width);
}

#[test]
fn width_edit_reflows_row_and_moves_the_painted_and_clickable_box_only_on_draw() {
    let col = |width| block(&[px(1, width), number(4, 10.)]);
    let chip = block(&[px(1, 50.), px(2, 30.), color([0, 0, 255])]);
    let below = |click| block(&[vec![1, 0], px(2, 40.), vec![40, click], color([255, 0, 0])]);
    let full = |w| {
        upload(&[
            node(1, 3, col(w), &[2, 6]),
            node(2, 2, block(&[vec![1, 0], number(4, 10.)]), &[3, 4, 5]),
            node(3, 4, chip.clone(), &[]),
            node(4, 4, chip.clone(), &[]),
            node(5, 4, chip.clone(), &[]),
            node(6, 4, below(1), &[]),
        ])
    };
    let mut r = Raster::default();
    r.tree.upload(&full(170.)).unwrap();
    assert_eq!(r.hit_at(5., 45.), None);
    for (step, (width, row_h, y)) in [(170., 30., 40.), (100., 110., 120.), (170., 30., 40.)]
        .into_iter()
        .enumerate()
    {
        if step != 0 {
            r.tree.patch(&set(1, 3, col(width))).unwrap();
            let prev = if step == 1 { 45. } else { 125. };
            assert_eq!(r.hit_at(5., prev), Some((6, 5., 5.)));
            assert_eq!(r.hit_at(5., y + 5.), None);
        }
        let expected: Vec<u8> = (0..170)
            .flat_map(|py| {
                (0..180).flat_map(move |px| {
                    if px < width as u32 && (y as u32..y as u32 + 40).contains(&py) {
                        [255, 0, 0, 255]
                    } else if (width == 170. && py < 30 && px < 170 && px % 60 < 50)
                        || (width == 100. && py < 110 && px < 50 && py % 40 < 30)
                    {
                        [0, 0, 255, 255]
                    } else {
                        [0; 4]
                    }
                })
            })
            .collect();
        assert_eq!(draw(&mut r), expected);
        assert_eq!(bounds(&r, 2).height, row_h);
        assert_eq!(
            bounds(&r, 6),
            Rect {
                x: 0.,
                y,
                width: width as f32,
                height: 40.
            }
        );
        if step != 0 {
            assert!((3..=5).all(|chip| !ran(&r, Measure, chip)));
            assert!(ran(&r, Reflow, 2) && ran(&r, Place, 6));
        }
        assert_eq!(r.hit_at(5., y + 5.), Some((6, 5., 5.)));
        assert_eq!(r.hit_at(5., if y == 40. { 125. } else { 45. }), None);
        assert_eq!(r.hit_at(5., y - 1.), None);
        assert_eq!(r.hit_at(width as f32, y + 5.), Some((6, width, 5.)));
        assert_eq!(r.hit_at(width as f32 + 1., y + 5.), None);
    }
    let draws = r.draws;
    r.tree.patch(&set(6, 4, below(0))).unwrap();
    draw(&mut r);
    assert_eq!(
        r.tree.layers.trace,
        [(Compose, 6, Outputs::Hit), (Compose, 1, Outputs::Hit)]
    );
    assert_eq!(r.hit_at(5., 45.), None);
    r.tree.patch(&patch(1, 0, &[])).unwrap();
    r.tree.upload(&full(170.)).unwrap(); // true remount, identical visible paint
    draw(&mut r);
    assert_eq!(r.draws, draws);
    assert_eq!(r.hit_at(5., 45.), Some((6, 5., 5.)));
    r.tree.patch(&patch(1, 0, &[])).unwrap();
    assert_eq!(r.hit_at(5., 45.), Some((6, 5., 5.)));
    assert!(draw(&mut r).iter().all(|&v| v == 0));
    assert_eq!(r.hit_at(5., 45.), None);
}

#[test]
fn reorder_and_viewport_clip_use_paint_order_for_hits() {
    let target = |id, rgb| {
        node(
            id,
            4,
            block(&[px(1, 40.), px(2, 30.), color(rgb), vec![40, 1]]),
            &[],
        )
    };
    let short = block(&[px(2, 5.)]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, block(&[]), &[2, 3]),
            node(2, 4, short.clone(), &[22]),
            target(22, [255, 0, 0]),
            node(3, 4, short, &[33]),
            target(33, [0, 0, 255]),
        ]))
        .unwrap();
    // The two short wrappers make an auto-height column only 10px tall.
    let clipped = [
        [255, 0, 0, 255].repeat(24 * 5),
        [0, 0, 255, 255].repeat(24 * 5),
        vec![0; 4 * 24 * 10],
    ]
    .concat();
    assert_eq!(r.draw(24., 20., 1.).unwrap(), clipped);
    assert_eq!(r.hit_at(23., 9.), Some((33, 23., 4.)));
    assert_eq!(r.hit_at(23., 19.), None);

    // Grow past the viewport so the viewport, not the column, limits the overlap.
    r.tree.patch(&set(1, 3, block(&[px(2, 30.)]))).unwrap();
    assert!(
        r.prepare(24., 20., 1.)
            .unwrap()
            .submit(|_, _| Err("presentation refused".into()))
            .is_err()
    );
    assert_eq!(r.pixels(), clipped);
    assert_eq!(r.hit_at(23., 19.), None);
    for (id, upper, lower) in [
        (33, [255, 0, 0, 255], [0, 0, 255, 255]),
        (22, [0, 0, 255, 255], [255, 0, 0, 255]),
    ] {
        if id == 22 {
            r.tree.patch(&order(1, 3, &[3, 2])).unwrap();
        }
        assert_eq!(
            r.draw(24., 20., 1.).unwrap(),
            [upper.repeat(24 * 5), lower.repeat(24 * 15)].concat()
        );
        assert_eq!(r.hit_at(23., 19.), Some((id, 23., 14.)));
        assert_eq!(r.hit_at(24., 19.), None);
        assert_eq!(r.hit_at(23., 20.), None);
    }
}

#[test]
fn paint_and_input_are_independent_readers_of_one_composition() {
    let attrs = |rgb, listen| block(&[px(1, 8.), px(2, 8.), color(rgb), vec![40, listen]]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 4, block(&[vec![40, 1]]), &[2]),
            node(2, 4, attrs([255, 0, 0], 0), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(r.hit_at(4., 3.), Some((1, 4., 3.))); // listener without paint
    r.tree.patch(&set(2, 4, attrs([0, 0, 255], 0))).unwrap();
    assert_eq!(&draw(&mut r)[..4], &[0, 0, 255, 255]);
    assert_eq!(
        r.tree.layers.trace,
        [(Compose, 2, Outputs::Draw), (Compose, 1, Outputs::Draw)]
    );
    assert_eq!(r.draws, 2);
    r.tree.patch(&set(2, 4, attrs([0, 0, 255], 1))).unwrap();
    r.tree.patch(&set(2, 4, attrs([0, 255, 0], 1))).unwrap();
    assert_eq!(r.hit_at(4., 3.), Some((1, 4., 3.)));
    assert_eq!(&draw(&mut r)[..4], &[0, 255, 0, 255]);
    assert_eq!(
        r.tree.layers.trace,
        [(Compose, 2, Outputs::Both), (Compose, 1, Outputs::Both)]
    );
    assert_eq!(r.hit_at(4., 3.), Some((2, 4., 3.)));
}

#[test]
fn rejecting_a_strike_keeps_pending_inputs_and_layout() {
    let mut t = Tree::default();
    let base = upload(&[
        node(1, 4, block(&[number(16, 12.)]), &[2]),
        node(2, 5, text("AA"), &[]),
    ]);
    t.upload_scaled(&base, 2.).unwrap();
    t.refresh(180., 170., 2.).unwrap();
    t.patch(&set(2, 5, text("BBBB"))).unwrap();
    let before = t.encode();
    let bad = patch(
        1,
        1,
        &[
            node(1, 4, block(&[number(16, 3e38)]), &[2]),
            attr_record(2, 5, text("other")),
        ],
    );
    assert_eq!(t.patch(&bad).unwrap_err(), "font size exceeds device range");
    assert_eq!(t.encode(), before);
    assert_eq!(t.layers.bounds.desired(), 4);
    t.refresh(180., 170., 2.).unwrap();
    let k = t.elements.ids[&2];
    assert_eq!(t.layers.runs[k].metrics.size, 24.);
    assert_eq!(t.layers.text.glyphs.get(&t.layers.runs[k].span).len(), 4);
    // Declared sizes are counted, not appended forever as attributes change.
    let ids: Vec<_> = (102..=166).collect();
    let mut nodes = vec![node(101, 3, block(&[]), &ids)];
    nodes.extend(
        ids.iter()
            .map(|&i| node(i, 4, block(&[number(16, i as f64)]), &[])),
    );
    assert_eq!(
        t.patch(&at(upload(&nodes), 1)).unwrap_err(),
        "too many distinct declared font sizes"
    );
    assert_eq!(t.encode(), before);
}

#[test]
fn output_sink_failure_preserves_installed_pixels_hit_geometry_and_attachment() {
    let mut r = Raster::default();
    let rect = |rgb, click| block(&[px(1, 30.), px(2, 20.), color(rgb), vec![40, click]]);
    r.tree
        .upload(&upload(&[node(1, 4, rect([255, 0, 0], 1), &[])]))
        .unwrap();
    let fail = |_: &[u8], _: bool| Err("presentation refused".to_owned());
    assert!(r.prepare(10., 10., 1.).unwrap().submit(fail).is_err());
    assert!(r.pixels().is_empty() && r.target_at(5., 5.).is_none());
    let old = r.draw(10., 10., 1.).unwrap().to_vec();
    let hit = r.target_at(5., 5.).unwrap();
    r.tree.patch(&set(1, 4, rect([0, 0, 255], 0))).unwrap();
    // An unavailable sink may decline even before submission. Both that
    // preparation and a failed submit leave the previous installation alone.
    drop(r.prepare(20., 20., 2.).unwrap());
    assert!(r.prepare(20., 20., 2.).unwrap().submit(fail).is_err());
    assert_eq!(r.pixels(), old);
    assert_eq!(r.target_at(5., 5.), Some(hit));
    assert!(r.target_at(15., 5.).is_none());
    r.tree.patch(&set(1, 4, rect([0, 255, 0], 1))).unwrap();
    r.prepare(20., 20., 2.)
        .unwrap()
        .submit(|buf, reuse| {
            assert!(!reuse);
            assert_eq!(&buf[..4], &[0, 255, 0, 255]);
            Ok(())
        })
        .unwrap();
    assert_eq!(r.target_at(15., 5.).unwrap().2, 7.5);
    r.tree.patch(&set(1, 4, rect([0, 255, 0], 0))).unwrap();
    assert!(
        r.prepare(20., 20., 2.)
            .unwrap()
            .submit(|p, reuse| {
                assert!(reuse);
                fail(p, reuse)
            })
            .is_err()
    );
    assert_eq!(r.target_at(5., 5.).unwrap().0, hit.0);
    r.prepare(20., 20., 2.)
        .unwrap()
        .submit(|_, reuse| {
            assert!(reuse);
            Ok(())
        })
        .unwrap();
    assert!(r.target_at(5., 5.).is_none());
}

#[test]
fn edits_accepted_during_submit_do_not_modify_the_sealed_candidate() {
    let mut r = Raster::default();
    let panel =
        |width, click| block(&[px(1, width), px(2, 20.), color([1, 2, 3]), vec![40, click]]);
    r.tree
        .upload(&upload(&[node(1, 4, panel(10., 1), &[])]))
        .unwrap();
    r.settle(32., 20., 1.).unwrap();
    assert!(r.hit_at(5., 5.).is_none());
    r.install();
    r.tree.patch(&set(1, 4, panel(20., 1))).unwrap();
    r.settle(32., 20., 1.).unwrap();
    r.tree.patch(&set(1, 4, panel(30., 0))).unwrap(); // no second settle during submit
    assert!(r.hit_at(15., 5.).is_none());
    r.install();
    assert_eq!(r.hit_at(15., 5.), Some((1, 15., 5.)));
    assert!(r.hit_at(25., 5.).is_none());
    r.settle(32., 20., 1.).unwrap();
    assert!(r.hit_at(15., 5.).is_some());
    r.install();
    assert!(r.hit_at(5., 5.).is_none());
    assert!(r.pixels().is_empty());
}

#[test]
fn failed_draw_and_layout_query_never_install_pixels_or_hits() {
    let square = |rgb, top| {
        upload(&[
            node(1, 3, block(&[]), &[2, 3]),
            node(2, 4, block(&[px(2, top)]), &[]),
            node(
                3,
                4,
                block(&[px(1, 10.), px(2, 10.), color(rgb), vec![40, 1]]),
                &[],
            ),
        ])
    };
    let mut r = Raster::default();
    r.tree.upload(&square([255, 0, 0], 0.)).unwrap();
    assert_eq!(&draw(&mut r)[..4], &[255, 0, 0, 255]);
    r.tree.patch(&at(square([0, 0, 255], 20.), 1)).unwrap(); // reuses the same wire IDs
    r.tree.refresh(180., 170., 1.).unwrap();
    assert_eq!(r.hit_at(5., 5.), Some((3, 5., 5.)));
    assert!(r.draw(100_000., 100_000., 1.).is_err());
    assert_eq!(r.draws, 1);
    assert_eq!(r.hit_at(5., 5.), Some((3, 5., 5.)));
    assert_eq!(r.hit_at(5., 25.), None);
    let pixels = draw(&mut r);
    assert_eq!(&pixels[..4], &[0; 4]);
    let blue = 20 * 180 * 4;
    assert_eq!(&pixels[blue..blue + 4], &[0, 0, 255, 255]);
    assert_eq!(r.draws, 2);
    assert_eq!(r.hit_at(5., 5.), None);
    assert_eq!(r.hit_at(5., 25.), Some((3, 5., 5.)));
}

#[test]
fn wire_upload_decodes_attributes_and_children_and_accepts_an_empty_replacement() {
    let mut t = Tree::default();
    let empty = upload(&[]);
    assert!(t.root.is_none() && t.elements.slots.is_empty());
    assert_eq!(t.encode(), empty);
    assert!(t.upload(b"XXXX\x09\0\0\0\0").is_err());
    t.upload(&upload(&[node(9, 4, block(&[]), &[])])).unwrap();
    assert_eq!(t.elements.slots.len(), 1);
    let full = upload(&[
        node(10, 3, block(&[number(4, 10.)]), &[11, 12]),
        node(11, 4, block(&[px(1, 100.), px(2, 200.)]), &[]),
        node(12, 5, block(&[]), &[]),
    ]);
    t.patch(&at(full.clone(), 9)).unwrap();
    let e = &t.elements;
    assert_eq!(e.attached_count(), 3);
    assert_eq!(t.root, Some(e.ids[&10]));
    assert_eq!(e.children(e.ids[&10]), &[e.ids[&11], e.ids[&12]]);
    assert_eq!(e.attrs[e.ids[&10]].sizing.spacing, [10., 10.]);
    assert_eq!(e.attrs[e.ids[&11]].sizing.width, Length::Px(100.));
    assert_eq!(e.attrs[e.ids[&11]].sizing.height, Length::Px(200.));
    assert_eq!(t.encode(), full);
    t.patch(&patch(10, 0, &[])).unwrap();
    assert!(t.root.is_none() && t.elements.attached_count() == 0);
    assert_eq!(t.encode(), empty);
}

#[test]
fn invalid_edits_are_atomic_and_stale_keys_cannot_touch_a_new_mount() {
    let data = upload(&[node(1, 4, block(&[]), &[2]), node(2, 5, text("abc"), &[])]);
    let mut r = Raster::default();
    r.tree.upload(&data).unwrap();
    let old = r.tree.elements.ids[&2];
    draw(&mut r);
    for bad in [
        set(2, 5, text("lost")).into_iter().chain([4]).collect(),
        patch(1, 1, &[edges(1, 4, &[1]), edges(1, 4, &[2])]),
        patch(
            1,
            1,
            &[edges(1, 4, &[2]), attr_record(2, 5, block(&[vec![65]]))],
        ),
        order(1, 4, &[1]),
        set(2, 5, block(&[vec![65]])),
    ] {
        assert!(r.tree.patch(&bad).is_err());
        assert_eq!(r.tree.encode(), data);
        draw(&mut r);
        assert!(r.tree.layers.trace.is_empty());
        assert_eq!(r.draws, 1);
    }
    for bad in [
        upload(&[node(1, 4, block(&[]), &[1])]),
        upload(&[node(1, 7, block(&[]), &[])]),
        upload(&[node(1, 4, block(&[]), &[]), node(2, 4, block(&[]), &[])]),
    ] {
        assert!(r.tree.upload(&bad).is_err());
        assert_eq!(r.tree.encode(), data);
    }
    r.tree.patch(&patch(1, 0, &[])).unwrap();
    r.tree
        .upload(&upload(&[
            node(1, 4, block(&[]), &[2]),
            node(2, 6, block(&[]), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert!(!r.tree.elements.slots.contains_key(old));
    r.tree.layers.invalidate(Measure, old, 1);
    draw(&mut r);
    assert!(r.tree.layers.trace.is_empty());
}
