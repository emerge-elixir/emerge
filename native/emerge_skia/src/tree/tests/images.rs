use super::*;
use crate::raster_assets::{Loader, Request, Settings, Source};
pub(super) const WIDE: &str = "test_assets/single_axis_wide.png";
pub(super) const GRID: &str = "test_assets/alpha_grid.png";
const BLUE: &str = "test_assets/alpha_inverse.png";
pub(super) fn loader() -> Loader {
    image_loader(false, 0)
}
pub(super) fn image_loader(decode_at_size: bool, cache_max_entries: usize) -> Loader {
    Loader::new(Settings {
        sources: vec![format!("{}/../../priv", env!("CARGO_MANIFEST_DIR"))],
        decode_at_size,
        cache_max_entries,
        cache_max_bytes: 1 << 20,
        ..Default::default()
    })
    .unwrap()
}
pub(super) fn path(name: &str) -> Bytes {
    [53, 1]
        .into_iter()
        .chain((name.len() as u16).to_be_bytes())
        .chain(name.bytes())
        .collect()
}
pub(super) fn attrs(name: &str, alpha: f64) -> Bytes {
    block(&[path(name), vec![1, 0], number(35, alpha), vec![40, 1]])
}
pub(super) fn story(name: &str, alpha: f64) -> Bytes {
    upload(&[
        node(1, 3, block(&[px(1, 80.)]), &[2, 3]),
        node(2, 9, attrs(name, alpha), &[]),
        node(
            3,
            4,
            block(&[px(1, 12.), px(2, 9.), vec![40, 1], color([0, 0, 255])]),
            &[],
        ),
    ])
}
pub(super) fn load(t: &mut Tree, l: &mut Loader) {
    let (job, name) = t.next_image().unwrap();
    assert!(l.fulfill(name, |s| t.complete_image(&job, s)));
}
#[test]
fn equal_size_pixel_swap_and_static_opacity_have_no_layout_readers() {
    let mut l = loader();
    let mut r = Raster::default();
    r.tree.upload(&story(GRID, 1.)).unwrap();
    load(&mut r.tree, &mut l);
    let a = draw(&mut r).to_vec();
    assert_eq!(bounds(&r, 3).y, 48.);
    r.tree.patch(&set(2, 9, attrs(BLUE, 1.))).unwrap();
    assert_eq!(
        draw(&mut r),
        a,
        "the still-bound source supplies both size and pixels"
    );
    load(&mut r.tree, &mut l);
    assert_ne!(draw(&mut r), a);
    assert!(!ran(&r, Measure, 2) && !ran(&r, Reflow, 2));
    assert_eq!(bounds(&r, 3).y, 48.);
    for alpha in [0.5, 0., 1.] {
        r.tree.patch(&set(2, 9, attrs(BLUE, alpha))).unwrap();
        assert!(!r.tree.needs_images());
        draw(&mut r);
        assert_eq!(r.hit_at(2., 2.), Some((2, 2., 2.)));
        assert!(
            r.tree
                .layers
                .trace
                .iter()
                .all(|&(pass, _, out)| pass == Compose && out == Outputs::Draw)
        );
    }
    r.tree.patch(&set(2, 9, attrs(WIDE, 1.))).unwrap();
    load(&mut r.tree, &mut l);
    draw(&mut r);
    assert_eq!(bounds(&r, 3).y, 40.);
    assert_eq!(r.hit_at(3., 44.), Some((3, 3., 4.)));
}
#[test]
fn superseded_or_detached_tickets_cannot_change_the_final_declaration() {
    let mut r = Raster::default();
    r.tree.upload(&story(WIDE, 1.)).unwrap();
    let (old, _) = r.tree.next_image().unwrap();
    r.tree.patch(&set(2, 9, attrs(GRID, 1.))).unwrap();
    let (next, _) = r.tree.next_image().unwrap();
    assert!(!r.tree.complete_image(&old, Source::Failed));
    r.tree.patch(&order(1, 3, &[3])).unwrap();
    assert!(!r.tree.complete_image(&next, Source::Failed));
    draw(&mut r);
    assert_eq!(bounds(&r, 3).y, 0.);
    r.tree
        .patch(&patch(
            1,
            1,
            &[edges(1, 3, &[2, 3]), node(2, 9, attrs(BLUE, 1.), &[])],
        ))
        .unwrap();
    assert!(!r.tree.complete_image(&next, Source::Failed));
    load(&mut r.tree, &mut loader());
    draw(&mut r);
    assert_eq!(bounds(&r, 3).y, 48.);
    let pending = r.tree.encode();
    for fields in [
        vec![path("../outside.png")],
        vec![path("/a.png")],
        vec![path("../photos/a.jpeg")],
        vec![path("photos//a.jpg")],
        vec![path(GRID), vec![54, 2]],
        vec![path(GRID), vec![2, 0]],
        vec![path(GRID), number(35, f64::NAN)],
        vec![path(GRID), number(33, f64::NAN)],
        vec![path(GRID), number(33, f64::INFINITY)],
        vec![path(GRID), number(79, 45.)], // layout rotation is not a paint pose
        vec![vec![53, 2, 0, 1, b'x']],
        vec![vec![53, 0, 0, 2, b'i', b'd']], // retired preloaded-ID source
    ] {
        assert!(r.tree.patch(&set(2, 9, block(&fields))).is_err());
    }
    assert_eq!(r.tree.encode(), pending);
    assert!(!r.tree.needs_images());
}
#[test]
fn turns_reuse_fitted_pixels_without_layout_or_moving_siblings() {
    let mut r = Raster::default();
    let mut l = image_loader(true, 0);
    r.tree.upload(&story(WIDE, 1.)).unwrap();
    load(&mut r.tree, &mut l);
    r.settle(180., 170., 1.).unwrap();
    fit(&mut r.tree, &mut l, (80, 40));
    let a = draw(&mut r).to_vec();
    for degrees in [30., 45., 90., -45., f64::MAX, 360.] {
        let fields = block(&[path(WIDE), vec![1, 0], vec![40, 1], number(33, degrees)]);
        r.tree.patch(&set(2, 9, fields)).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 3).y, 40.);
        assert!(
            r.tree
                .layers
                .trace
                .iter()
                .all(|&(p, _, _)| matches!(p, PaintContext | Compose))
        );
        assert!(r.tree.next_image().is_none());
    }
    assert_eq!(r.pixels(), a);
    let zoom = |z| {
        set(
            2,
            9,
            block(&[
                path(WIDE),
                vec![1, 0],
                vec![40, 1],
                number(33, 30.),
                number(34, z),
            ]),
        )
    };
    r.tree.patch(&zoom(2.)).unwrap();
    r.settle(180., 170., 1.).unwrap();
    fit(&mut r.tree, &mut l, (160, 80));
    draw(&mut r);
    r.tree.patch(&zoom(-0.5)).unwrap();
    draw(&mut r);
    assert!(
        r.tree.next_image().is_none(),
        "a smaller reflected use reuses the grown pixels"
    );
    assert_eq!(bounds(&r, 3).y, 40.);
    let accepted = r.tree.encode();
    assert!(r.tree.patch(&zoom(f64::MAX)).is_err());
    assert!(r.tree.patch(&set(1, 3, block(&[number(33, 30.)]))).is_err());
    assert_eq!(r.tree.encode(), accepted);
}
#[test]
fn early_image_width_uses_the_final_row_offer_never_a_previous_fit() {
    let mut r = Raster::default();
    let leaf = block(&[path(WIDE), vec![2, 0], vec![40, 1]]);
    r.tree
        .upload(&upload(&[
            node(1, 1, block(&[px(2, 40.)]), &[2, 3]),
            node(2, 9, leaf, &[]),
            node(3, 3, block(&[px(2, 60.)]), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 2).width, 64.); // pending has no ratio
    load(&mut r.tree, &mut loader());
    draw(&mut r);
    assert_eq!(bounds(&r, 2).width, 80.);
    r.tree.patch(&set(1, 1, block(&[px(2, 60.)]))).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 2).width, 120.);
    let before = r.tree.encode();
    for invalid in [
        set(1, 1, block(&[])),
        patch(1, 1, &[edges(1, 1, &[3]), edges(3, 3, &[2])]),
    ] {
        assert!(
            r.tree
                .patch(&invalid)
                .unwrap_err()
                .contains("automatic width from fill height")
        );
        assert_eq!(r.tree.encode(), before);
    }
    // Releasing the parent's definite height is fine if the same batch also
    // changes the kept Image to an explicit height.
    r.tree
        .patch(&patch(
            1,
            1,
            &[
                node(1, 1, block(&[]), &[2, 3]),
                attr_record(2, 9, block(&[path(WIDE), px(2, 22.)])),
            ],
        ))
        .unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 2).width, 44.);

    let mut root = Raster::default();
    root.tree
        .upload(&upload(&[node(
            1,
            9,
            block(&[path(WIDE), vec![2, 0]]),
            &[],
        )]))
        .unwrap();
    load(&mut root.tree, &mut loader());
    for h in [40., 60., 40.] {
        root.draw(180., h, 1.).unwrap();
        assert_eq!(bounds(&root, 1).width, 2. * h);
    }
}
fn fit(t: &mut Tree, l: &mut Loader, size: (i32, i32)) {
    let (ticket, request) = t.next_image().expect("expected fitted request");
    assert!(matches!(&request, Request::Fit(_,target) if *target == size.into()));
    assert!(l.fulfill(request, |s| t.complete_image(&ticket, s)));
    assert!(
        t.next_image().is_none(),
        "one job for all visible uses of one source"
    );
}

#[test]
fn coalesced_contain_and_cover_use_content_device_pixels_not_the_crop_or_bleed() {
    let mut r = Raster::default();
    let mut l = image_loader(true, 0);
    let sample = |w, h, cover| {
        block(&[
            path(WIDE),
            px(1, w),
            px(2, h),
            vec![54, u8::from(cover)],
            [vec![3, 0], 2f64.to_be_bytes().to_vec()].concat(),
        ])
    };
    r.tree
        .upload(&upload(&[
            node(1, 3, block(&[]), &[2, 3]),
            node(2, 9, sample(44., 24., false), &[]),
            node(3, 9, sample(24., 34., true), &[]),
        ]))
        .unwrap();
    load(&mut r.tree, &mut l);
    load(&mut r.tree, &mut l);
    r.settle(200., 200., 1.5).unwrap();
    // 40×20 logical contain, but square-ish 20×30 cover needs the
    // entire 60×30 source at 1.5 device pixels per layout unit.
    fit(&mut r.tree, &mut l, (90, 45));
    r.draw(200., 200., 1.5).unwrap();
    r.tree.patch(&set(3, 9, sample(24., 34., false))).unwrap();
    r.draw(200., 200., 1.5).unwrap();
    assert!(
        r.tree.next_image().is_none(),
        "installed pixels suffice even with zero cache"
    );
    r.tree.patch(&set(2, 9, sample(304., 154., false))).unwrap();
    r.settle(200., 200., 1.).unwrap();
    fit(&mut r.tree, &mut l, (200, 100)); // No bitmap larger than the source.
    r.draw(200., 200., 1.).unwrap();
}

#[test]
fn stale_growth_cannot_replace_a_thumbnail_or_advance_its_hits() {
    let mut l = image_loader(true, 1);
    let mut r = Raster::default();
    r.tree.upload(&story(WIDE, 1.)).unwrap();
    load(&mut r.tree, &mut l);
    r.settle(180., 170., 1.).unwrap();
    fit(&mut r.tree, &mut l, (80, 40));
    let small = draw(&mut r).to_vec();
    let resize = |w| set(1, 3, block(&[px(1, w)]));
    r.tree.patch(&resize(160.)).unwrap();
    assert!(r.prepare(180., 170., 1.).is_err());
    let (stale, request) = r.tree.next_image().unwrap();
    r.tree.patch(&resize(80.)).unwrap();
    assert!(!l.fulfill(request, |s| r.tree.complete_image(&stale, s)));
    assert!(draw(&mut r) == small);
    r.tree.patch(&resize(160.)).unwrap();
    r.settle(180., 170., 1.).unwrap();
    // A late result must not populate either the weak link or the LRU.
    assert_eq!(r.hit_at(2., 43.), Some((3, 2., 3.)));
    let (job, req) = r.tree.next_image().unwrap();
    r.tree.patch(&set(2, 9, attrs(WIDE, 0.5))).unwrap();
    r.settle(180., 170., 1.).unwrap();
    assert!(
        !r.tree.complete_image(&job, Source::Failed),
        "never reuse a stale failure"
    );
    assert!(
        l.fulfill(req, |s| r.tree.complete_image(&job, s)),
        "same immutable source and current demand survive an unrelated paint edit"
    );
    assert!(r.tree.next_image().is_none());
    let b = r.prepare(180., 170., 1.).unwrap();
    assert!(b.submit(|_, _| Err("sink refused".into())).is_err());
    assert!(r.pixels() == small);
    assert_eq!(r.hit_at(2., 43.), Some((3, 2., 3.)));
    draw(&mut r);
    assert_eq!(r.hit_at(2., 83.), Some((3, 2., 3.)));
}

#[test]
fn metadata_alone_does_not_allocate_pixels_for_invisible_draws() {
    let mut l = image_loader(true, 0);
    let mut r = Raster::default();
    r.tree.upload(&story(WIDE, 0.)).unwrap();
    load(&mut r.tree, &mut l);
    draw(&mut r);
    assert_eq!(bounds(&r, 3).y, 40.);
    assert!(r.tree.next_image().is_none());
    r.tree.patch(&set(2, 9, attrs(WIDE, 1.))).unwrap();
    r.settle(180., 170., 1.).unwrap();
    let (job, _) = r.tree.next_image().unwrap();
    assert!(r.tree.complete_image(&job, Source::Failed));
    // A codec/pixel-budget failure after a good header must settle, not spin.
    draw(&mut r);
    assert_eq!(bounds(&r, 3).y, 64.);
    assert!(!r.tree.waiting_for_pixels());
    let panel = node(4, 4, block(&[px(1, 40.), px(2, 160.)]), &[]);
    r.tree
        .upload(&at(
            upload(&[
                node(10, 3, block(&[]), &[4, 5]),
                panel,
                node(5, 9, attrs(WIDE, 1.), &[]),
            ]),
            1,
        ))
        .unwrap();
    load(&mut r.tree, &mut l);
    r.draw(100., 80., 1.).unwrap();
    assert!(r.tree.next_image().is_none());
}

#[test]
fn failed_or_successful_submit_uses_sealed_image_and_hits_even_after_eviction() {
    for (success, sized) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut l = image_loader(sized, 0);
        let mut r = Raster::default();
        r.tree.upload(&story(WIDE, 1.)).unwrap();
        load(&mut r.tree, &mut l);
        if sized {
            r.settle(180., 170., 1.).unwrap();
            fit(&mut r.tree, &mut l, (80, 40));
        }
        let a = draw(&mut r).to_vec();
        r.tree.patch(&set(2, 9, attrs(GRID, 1.))).unwrap();
        load(&mut r.tree, &mut l);
        if sized {
            r.settle(180., 170., 1.).unwrap();
            fit(&mut r.tree, &mut l, (10, 6));
        }
        let mut prep = r.prepare(180., 170., 1.).unwrap();
        let b = prep.accepted().replay_drawing();
        assert!(b != a);
        // C displaces the entire accepted root. Neither loader nor accepted
        // bindings keep B alive. The sealed side still owns that exact bitmap.
        prep.accepted()
            .tree
            .patch(&at(
                upload(&[node(
                    20,
                    4,
                    block(&[px(1, 150.), px(2, 60.), vec![40, 1], color([0, 255, 0])]),
                    &[],
                )]),
                1,
            ))
            .unwrap();
        assert!(prep.accepted().replay_drawing() == b);
        assert_eq!(prep.accepted().hit_at(2., 42.), Some((3, 2., 2.)));
        let result = prep.submit(|pixels, _| {
            assert!(pixels == b);
            if success {
                Ok(())
            } else {
                Err("sink refused".into())
            }
        });
        assert_eq!(result.is_ok(), success);
        assert!(r.pixels() == if success { &b } else { &a });
        assert_eq!(r.hit_at(2., 42.).unwrap().0, if success { 2 } else { 3 });
        draw(&mut r);
        assert_eq!(r.hit_at(2., 42.).unwrap().0, 20);
    }
}
