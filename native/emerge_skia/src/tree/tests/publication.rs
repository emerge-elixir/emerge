use super::*;
use std::sync::Arc;

#[test]
fn two_unconsumed_revisions_share_untouched_orders_and_layout_uses_the_final_one() {
    let mut writer = Publisher::new(80);
    let mut apply = |bytes: &[u8]| writer.apply(bytes, 1.);
    let mut actor = Tree::for_session(0);
    let attrs = |c| block(&[px(1, 20.), px(2, 20.), color(c), vec![40, 1]]);
    let leaf = |id, c| node(id, 4, attrs(c), &[]);
    let full = |c| {
        upload(&[
            node(1, 1, block(&[]), &[2, 3]),
            leaf(2, c),
            leaf(3, [0, 255, 0]),
        ])
    };
    apply(&full([255, 0, 0])).unwrap().install(&mut actor);
    actor.refresh(80., 20., 1.).unwrap();
    let a = actor.seal().unwrap();
    let k = actor.elements.ids[&1];
    let root = actor
        .elements
        .attrs
        .version
        .as_ref()
        .unwrap()
        .get(k)
        .unwrap()
        .clone();
    let b = apply(&set(2, 4, attrs([0, 0, 255]))).unwrap();
    assert!(apply(&patch(1, 1, &[edges(1, 1, &[2, 4])])).is_err());
    let c = apply(&set(2, 4, attrs([255, 0, 0]))).unwrap();
    assert_eq!(actor.encode(), full([255, 0, 0]));
    b.install(&mut actor);
    c.install(&mut actor);
    assert!(Arc::ptr_eq(
        &root,
        actor
            .elements
            .attrs
            .version
            .as_ref()
            .unwrap()
            .get(k)
            .unwrap()
    ));
    let passes = actor.layers.invalidation_passes;
    actor.refresh(80., 20., 1.).unwrap();
    assert_eq!(actor.layers.invalidation_passes, passes + 1);
    assert!(actor.layers.trace.is_empty());
    let again = actor.seal().unwrap();
    assert!(Arc::ptr_eq(&a.drawing, &again.drawing));
    let old = a.hits.target_at(a.viewport, 5., 5.).unwrap().0;
    let remove = apply(&patch(2, 0, &[])).unwrap();
    let insert = apply(&patch(1, 1, &[edges(1, 1, &[2, 3]), leaf(2, [0, 0, 255])])).unwrap();
    remove.install(&mut actor);
    insert.install(&mut actor);
    actor.refresh(80., 20., 1.).unwrap();
    let blue = actor.seal().unwrap();
    assert!(!blue.hits.has_target(old));
    assert!(a.hits.has_target(old));
    assert_ne!(a.rasterize().unwrap(), blue.rasterize().unwrap());
    assert_eq!(actor.encode(), full([0, 0, 255]));
}

#[test]
fn a_small_wire_can_displace_many_pages_but_one_apply_slice_cannot() {
    let ids: Vec<_> = (2..4098).collect();
    let wire = upload(
        &std::iter::once(node(1, 1, block(&[]), &ids))
            .chain(ids.iter().map(|&id| node(id, 6, block(&[]), &[])))
            .collect::<Vec<_>>(),
    );
    let storage = storage::Storage::default();
    let mut writer = Publisher::with_storage(0, storage.clone());
    let mut t = Tree::for_session(0);
    writer.apply(&wire, 1.).unwrap().install(&mut t);
    let remove = patch(1, 0, &[]);
    assert_eq!(remove.len(), 25);
    let mut stream = Stream::bounded(25, &storage).unwrap();
    stream.push(0, &remove).unwrap();
    let mut d = writer.prepare(stream.finish().unwrap(), 1.).unwrap();
    assert!(!d.apply_slice());
    writer.release(d, false);
    assert_eq!(t.encode(), wire);
    writer
        .apply(&set(2, 6, block(&[])), 1.)
        .unwrap()
        .install(&mut t);
    assert_eq!(t.encode(), wire);
    writer.apply(&remove, 1.).unwrap().install(&mut t);
    assert!(t.root.is_none());
}
#[test]
fn published_footprint_counts_shared_paths_once_in_the_current_root() {
    let s = storage::Storage::default();
    let mut w = Publisher::with_storage(0, s.clone());
    let mut t = Tree::for_session(0);
    for wire in [
        upload(&[
            node(1, 1, block(&[]), &[2, 3]),
            node(2, 6, block(&[]), &[]),
            node(3, 6, block(&[]), &[]),
        ]),
        patch(2, 0, &[]),
        set(3, 6, block(&[px(1, 8.)])),
    ] {
        w.apply(&wire, 1.).unwrap().install(&mut t);
    }
    let pin = t.elements.attrs.version.clone().unwrap();
    drop(t);
    drop(w);
    assert!(pin.extent > 0);
    assert_eq!(s.document.bytes().0, pin.extent);
    drop(pin);
    assert_eq!(s.document.bytes().0, 0);
}
#[test]
fn temporary_media_overlap_releases_credit_only_after_the_old_publication_retires() {
    let attr = |v| {
        let bytes = vec![v; 1024];
        block(&[[
            &[53, 3][..],
            &crate::raster_assets::digest(&bytes),
            &1024u32.to_be_bytes(),
            &bytes,
        ]
        .concat()])
    };
    let extent = attr(1).len() + 256;
    let s = storage::Storage::with_limits(1 << 20, 1 << 20, 3 * extent - 1);
    let mut w = Publisher::with_storage(0, s.clone());
    let old = w.apply(&upload(&[node(1, 9, attr(1), &[])]), 1.).unwrap();
    w.apply(&set(1, 9, attr(2)), 1.).unwrap();
    let next = set(1, 9, attr(3));
    assert_eq!(w.apply(&next, 1.).err().as_deref(), Some(storage::BUSY));
    w.trim_if_pressured();
    assert!(!s.available());
    drop(old);
    assert!(s.available());
    w.apply(&next, 1.).unwrap();
    drop(w);
    assert_eq!(s.media.bytes(), (0, 2 * extent));
}
#[test]
fn workspace_credit_follows_the_last_sealed_reader_not_just_the_old_tree() {
    let turns = [
        upload(&[node(1, 5, text(&"a".repeat(120)), &[])]),
        set(1, 5, text(&"b".repeat(121))),
        set(1, 5, text(&"c".repeat(122))),
    ];
    let generous = storage::Storage::default();
    let mut baseline = Publisher::with_storage(0, generous.clone());
    let mut t = Tree::for_session(0);
    for wire in &turns {
        baseline.apply(wire, 1.).unwrap().install(&mut t);
    }
    let cap = generous.workspace.bytes().0;
    drop((baseline, t));
    let s = storage::Storage::with_limits(1 << 20, 2 * cap, 1 << 20);
    let mut w = Publisher::with_storage(0, s.clone());
    let mut t = Tree::for_session(0);
    w.apply(&turns[0], 1.).unwrap().install(&mut t);
    t.refresh(100., 20., 1.).unwrap();
    let pin = t.seal().unwrap();
    w.apply(&turns[1], 1.).unwrap().install(&mut t);
    assert_eq!(w.apply(&turns[2], 1.).err().as_deref(), Some(storage::BUSY));
    assert!(!s.available());
    drop(pin);
    assert!(s.available());
    w.apply(&turns[2], 1.).unwrap().install(&mut t);
    drop((t, w));
    assert_eq!(s.workspace.bytes().0, 0);
}
#[test]
fn pinned_drawings_defer_collection_not_layout_or_new_edits() {
    let mut w = Publisher::new(0);
    let mut t = Tree::for_session(0);
    let attrs = |width: u8| {
        block(&[
            px(1, f64::from(width)),
            px(2, 4.),
            color([width, 0, 0]),
            vec![40, 1],
        ])
    };
    let mut pins: Vec<_> = (1..=5)
        .map(|i| {
            let edit = if i == 1 {
                upload(&[node(1, 4, attrs(i), &[])])
            } else {
                set(1, 4, attrs(i))
            };
            w.apply(&edit, 1.).unwrap().install(&mut t);
            t.layout(16., 4., 1.).unwrap();
            assert!(t.collect_if_available());
            t.seal().unwrap()
        })
        .collect();
    for i in 6..=9 {
        w.apply(&set(1, 4, attrs(i)), 1.).unwrap().install(&mut t);
        t.layout(16., 4., 1.).unwrap();
        assert!(!t.collect_if_available());
        assert_eq!(t.layers.frames[t.root.unwrap()].width, f32::from(i));
    }
    assert!(pins[0].hits.target_at(pins[0].viewport, 5., 1.).is_none());
    pins.pop();
    assert!(t.collect_if_available());
    let winner = t.seal().unwrap();
    assert_eq!(&winner.rasterize().unwrap()[4 * 8..4 * 9], &[9, 0, 0, 255]);
    assert!(winner.hits.target_at(winner.viewport, 8., 1.).is_some());
}

#[test]
fn an_outstanding_fit_cannot_replace_pixels_after_a_live_resize_and_remount() {
    let mut w = Publisher::new(0);
    let mut t = Tree::for_session(0);
    let mut loader = images::image_loader(true, 0);
    w.apply(&images::story(images::WIDE, 1.), 1.)
        .unwrap()
        .install(&mut t);
    images::load(&mut t, &mut loader);
    t.refresh(180., 170., 1.).unwrap();
    images::load(&mut t, &mut loader);
    t.refresh(180., 170., 1.).unwrap();
    let a = t.seal().unwrap();
    let original = a.rasterize().unwrap();
    w.apply(&set(1, 3, block(&[px(1, 160.)])), 1.)
        .unwrap()
        .install(&mut t);
    t.refresh(180., 170., 1.).unwrap();
    assert!(t.seal().is_none());
    let (stale, req) = t.next_image().unwrap();
    w.apply(&patch(2, 0, &[]), 1.).unwrap().install(&mut t);
    w.apply(
        &patch(
            1,
            1,
            &[
                edges(1, 3, &[2, 3]),
                node(2, 9, images::attrs(images::GRID, 1.), &[]),
            ],
        ),
        1.,
    )
    .unwrap()
    .install(&mut t);
    assert!(!loader.fulfill(req, |s| t.complete_image(&stale, s)));
    images::load(&mut t, &mut loader);
    t.refresh(180., 170., 1.).unwrap();
    images::load(&mut t, &mut loader);
    t.refresh(180., 170., 1.).unwrap();
    let c = t.seal().unwrap();
    assert_ne!(c.rasterize().unwrap(), original);
    assert_eq!(a.rasterize().unwrap(), original);
    let hit = |s: &Scene| s.hits.target_at(s.viewport, 2., 43.).unwrap();
    assert_eq!((hit(&a).1, hit(&c).1), (3, 2));
}

#[test]
fn a_file_backdrop_rebinds_on_its_host_without_reviving_a_superseded_ticket() {
    let mut writer = Publisher::new(0);
    let mut t = Tree::for_session(0);
    let mut loader = images::image_loader(true, 0);
    let file = |name: &str, tile| {
        [
            vec![12, 2, 1],
            (name.len() as u16).to_be_bytes().to_vec(),
            name.as_bytes().to_vec(),
            vec![tile],
        ]
        .concat()
    };
    let attrs = |bg: Bytes| block(&[px(1, 60.), px(2, 50.), vec![40, 1], bg]);
    let bmp = || attrs(file("test_assets/media.bmp", 2));
    let gif = || attrs(file("test_assets/media.gif", 2));
    let mut publish = |t: &mut Tree, wire: Bytes| writer.apply(&wire, 1.).unwrap().install(t);
    let frame = |t: &mut Tree| {
        t.refresh(60., 50., 1.).unwrap();
        t.seal().unwrap()
    };
    publish(&mut t, upload(&[node(1, 4, bmp(), &[])]));
    images::load(&mut t, &mut loader);
    t.refresh(60., 50., 1.).unwrap();
    let (ticket, req) = t.next_image().unwrap();
    assert!(
        matches!(&req, crate::raster_assets::Request::Fit(_,target) if *target == (9,8).into())
    );
    assert!(loader.fulfill(req, |s| t.complete_image(&ticket, s)));
    let first = frame(&mut t);
    let bytes = first.rasterize().unwrap();
    let marker = 4 * (2 * 60 + 4);
    assert_eq!(&bytes[marker..marker + 4], &[0, 0, 0, 255]);
    // Accepted A -> solid -> A before layout. The fresh A has no pixels yet.
    publish(&mut t, set(1, 4, attrs(color([0, 255, 0]))));
    publish(&mut t, set(1, 4, bmp()));
    let pending = frame(&mut t);
    assert_ne!(pending.rasterize().unwrap(), bytes);
    assert_eq!(
        pending
            .hits
            .target_at(pending.viewport, 32., 10.)
            .unwrap()
            .1,
        1
    );
    let (stale, req) = t.next_image().unwrap();
    publish(&mut t, set(1, 4, gif()));
    assert!(!loader.fulfill(req, |s| t.complete_image(&stale, s)));
    images::load(&mut t, &mut loader);
    t.refresh(60., 50., 1.).unwrap();
    let (fit, req) = t.next_image().unwrap();
    publish(&mut t, set(1, 4, attrs(color([0, 255, 0]))));
    assert!(!loader.fulfill(req, |s| t.complete_image(&fit, s)));
    let green = frame(&mut t);
    assert_eq!(
        &green.rasterize().unwrap()[marker..marker + 4],
        &[0, 255, 0, 255]
    );
    assert!(t.next_image().is_none());
    // Dimensions of non-leaf art never schedule measurement or placement.
    publish(&mut t, set(1, 4, gif()));
    images::load(&mut t, &mut loader);
    t.refresh(60., 50., 1.).unwrap();
    assert!(t.layers.trace.iter().all(|&(p, _, _)| p == Compose));
    images::load(&mut t, &mut loader);
    let final_frame = frame(&mut t);
    assert_eq!(
        &final_frame.rasterize().unwrap()[marker..marker + 4],
        &[0; 4]
    );
    assert_eq!(first.rasterize().unwrap(), bytes);
}

#[test]
fn grow_a_workspace_while_old_glyphs_hits_and_drawing_are_pinned() {
    let mut writer = Publisher::new(0);
    let mut actor = Tree::for_session(0);
    let mut expected = Raster::default();
    for (i, s) in ["AA", "BB", "a much longer text run", "i"]
        .into_iter()
        .enumerate()
    {
        let wire = if i == 0 {
            upload(&[node(1, 5, text(s), &[])])
        } else {
            set(1, 5, text(s))
        };
        let p = writer.apply(&wire, 1.).unwrap();
        p.install(&mut actor);
        actor.refresh(180., 170., 1.).unwrap();
        expected.tree.patch(&wire).unwrap();
        assert_eq!(
            actor.seal().unwrap().rasterize().unwrap(),
            draw(&mut expected)
        );
        assert_eq!(actor.encode(), expected.tree.encode());
    }
}
