use super::*;
use crate::raster_assets::{Loader, Request, Source, digest};

fn source(xml: &[u8], define: bool) -> Bytes {
    let id = digest(xml);
    if define {
        [&[53, 3][..], &id, &(xml.len() as u32).to_be_bytes(), xml].concat()
    } else {
        [&[53, 4][..], &id].concat()
    }
}
fn svg(i: u64) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 8"><rect width="16" height="8" fill="rgb({i},0,255)"/></svg>"#
    )
}
#[test]
fn refused_definition_does_not_replace_a_live_source_or_an_outstanding_ticket() {
    let mut t = Tree::default();
    let old = svg(2);
    let new = svg(3);
    t.upload(&upload(&[node(
        1,
        9,
        block(&[source(old.as_bytes(), true)]),
        &[],
    )]))
    .unwrap();
    let before = t.encode();
    let (ticket, req) = t.next_image().unwrap();
    let next = set(1, 9, block(&[source(new.as_bytes(), true)]));
    // Hold a lease (not a giant test allocation) to model overlapping stages.
    let pressure = t
        .input_budget
        .reserve(256 * 1024 * 1024 - old.len() - 256 - 1)
        .unwrap();
    assert!(t.patch(&next).is_err());
    drop(pressure);
    assert!(t.patch(&next[..next.len() - 1]).is_err());
    assert!(
        t.patch(&set(1, 9, block(&[source(new.as_bytes(), false)])))
            .is_err()
    );
    assert_eq!(t.encode(), before);
    assert!(images::loader().fulfill(req, |s| t.complete_image(&ticket, s)));
    t.patch(&next).unwrap();
    let (stale, req) = t.next_image().unwrap();
    t.patch(&patch(1, 0, &[])).unwrap();
    // A detached job still owns its verified bytes, but no publication rights.
    assert!(!images::loader().fulfill(req, |s| t.complete_image(&stale, s)));
    assert!(
        t.patch(&upload(&[node(
            1,
            9,
            block(&[source(new.as_bytes(), false)]),
            &[]
        )]))
        .is_err()
    );
}

#[test]
fn expired_export_deadline_reports_timeout_without_publishing_pixels() {
    let bytes = upload(&[node(1, 9, block(&[source(svg(0).as_bytes(), true)]), &[])]);
    let result = crate::services::render_tree_offscreen(&bytes, 16, 8, 1., || {
        Ok((
            crate::raster_assets::Settings {
                sources: vec![format!("{}/../../priv", env!("CARGO_MANIFEST_DIR"))],
                ..Default::default()
            },
            true,
            std::time::Duration::ZERO,
        ))
    });
    let error = result.unwrap_err();
    assert!(error.contains("timed out"), "{error}");
}

fn settle(r: &mut Raster, l: &mut Loader) {
    while let Some((ticket, request)) = r.tree.next_image() {
        assert!(
            matches!(request, Request::Open(..)),
            "no fitted-pixel jobs for SVGs"
        );
        l.fulfill(request, |s| {
            assert!(matches!(s, Source::Picture(_)));
            r.tree.complete_image(&ticket, s)
        });
    }
}

#[test]
fn mixed_sizes_record_once_per_resident_content_not_per_occurrence_or_edit() {
    for distinct in [false, true] {
        let mut l = images::image_loader(true, 0);
        let mut r = Raster::default();
        let ids: Vec<_> = (2..130).collect();
        let item = |i: u64, w, cover, tint, define| {
            let mut fields = vec![
                source(svg(if distinct { i } else { 0 }).as_bytes(), define),
                vec![64, 1],
                px(1, w),
                px(2, 9.),
                vec![54, u8::from(cover)],
            ];
            if tint {
                fields.push(vec![63, 0, 255, 0, 0]);
            }
            block(&fields)
        };
        // Reference before definition in record order, including with zero idle cache.
        let nodes: Vec<_> = std::iter::once(node(1, 2, block(&[px(1, 600.)]), &ids))
            .chain(ids.iter().map(|&i| {
                node(
                    i,
                    9,
                    item(i, (i % 9 + 5) as f64, false, false, distinct || i == 129),
                    &[],
                )
            }))
            .collect();
        r.tree.upload(&upload(&nodes)).unwrap();
        settle(&mut r, &mut l);
        let n = if distinct { 128 } else { 1 };
        assert_eq!(l.pictures.counts(), (n, n));
        let original = r.draw(640., 180., 1.).unwrap().to_vec();
        assert!(original.chunks_exact(4).any(|p| p[2] == 255));
        for (w, cover, tint, scale) in [
            (32., true, true, 1.5),
            (18., false, true, 2.),
            (7., false, false, 1.),
        ] {
            r.tree
                .patch(&set(2, 9, item(2, w, cover, tint, false)))
                .unwrap();
            assert!(!r.tree.needs_images());
            let pixels = r.draw(640., 180., scale).unwrap();
            if tint {
                assert!(pixels.chunks_exact(4).any(|p| p == [255, 0, 0, 255]));
            }
            assert!(r.tree.next_image().is_none());
        }
        assert_eq!(r.pixels(), original);
        assert_eq!(l.pictures.counts(), (n, n));
        r.tree
            .patch(&set(2, 9, block(&[source(svg(240).as_bytes(), true)])))
            .unwrap();
        settle(&mut r, &mut l);
        assert_eq!(l.pictures.counts(), (n + 1, n + 1));
    }
}

#[test]
fn file_and_bytes_share_the_sealed_picture_but_not_the_declaration_or_hit_lifetime() {
    const NAME: &str = "test_assets/single_axis_wide.svg";
    let xml = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../priv")
            .join(NAME),
    )
    .unwrap();
    for submit in [false, true] {
        let mut l = images::image_loader(true, 0);
        let mut r = Raster::default();
        r.tree.upload(&images::story(NAME, 1.)).unwrap();
        settle(&mut r, &mut l);
        let a = draw(&mut r).to_vec();
        let inline = |define| block(&[source(&xml, define), vec![64, 1], vec![1, 0], vec![40, 1]]);
        r.tree.patch(&set(2, 9, inline(true))).unwrap();
        r.tree.bind_ready_pictures(&l.pictures);
        assert!(!r.tree.needs_images());
        assert_eq!(draw(&mut r), a);
        r.tree.patch(&set(1, 3, block(&[px(1, 160.)]))).unwrap();
        let mut sealed = r.prepare(180., 170., 1.).unwrap();
        let b = sealed.accepted().replay_drawing();
        assert_ne!(a, b);
        let tree = &mut sealed.accepted().tree;
        tree.patch(&patch(1, 0, &[])).unwrap();
        // B is pinned but clear ended the inline declaration. Neither a sealed
        // picture nor the worker cache grants the right to use a wire reference.
        assert!(
            tree.patch(&upload(&[node(8, 9, inline(false), &[])]))
                .is_err()
        );
        tree.patch(&upload(&[node(8, 9, inline(true), &[])]))
            .unwrap();
        tree.bind_ready_pictures(&l.pictures);
        assert!(!tree.needs_images());
        assert_eq!(sealed.accepted().hit_at(2., 42.), Some((3, 2., 2.)));
        assert_eq!(
            sealed
                .submit(|p, _| {
                    assert_eq!(p, b);
                    if submit {
                        Ok(())
                    } else {
                        Err("refused".into())
                    }
                })
                .is_ok(),
            submit
        );
        assert_eq!(r.pixels(), if submit { &b } else { &a });
        assert_eq!(
            r.hit_at(2., if submit { 82. } else { 42. }),
            Some((3, 2., 2.))
        );
        draw(&mut r);
        assert_eq!(r.hit_at(2., 42.), Some((8, 2., 42.)));
        assert_eq!(l.pictures.counts(), (1, 1));
    }
}
