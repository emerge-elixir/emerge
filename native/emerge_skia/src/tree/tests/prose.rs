use super::*;

pub(super) fn host(width: f64, align: u8) -> Bytes {
    block(&[
        px(1, width),
        [3, 0].into_iter().chain(4f64.to_be_bytes()).collect(),
        vec![30, align],
        vec![40, 1],
        [18, 0, 0, 9].into_iter().chain(*b"monospace").collect(),
    ])
}
pub(super) fn wrapper(color: u8) -> Bytes {
    block(&[
        vec![40, 1],
        vec![
            12, 0, 3, 0, 0, 0, 2, 0, color, 0, 255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        vec![47, 1],
        vec![48, 1],
    ])
}
pub(super) fn doc(s: &str, w: f64) -> Bytes {
    upload(&[
        node(1, 7, host(w, 0), &[2]),
        node(2, 4, wrapper(90), &[3]),
        node(3, 5, text(s), &[]),
    ])
}

#[test]
fn inline_paint_does_not_rewrap_and_equal_metrics_do_not_hide_new_words() {
    let mut r = Raster::default();
    r.tree.upload(&doc("AAA BBB C", 64.)).unwrap();
    let initial = draw(&mut r).to_vec();
    let spans = |r: &Raster| {
        r.tree
            .layers
            .paragraphs
            .fragments(r.tree.elements.ids[&1])
            .to_vec()
    };
    let old = spans(&r);
    assert_eq!(old.len(), 3);
    assert_ne!(old[0].bounds.y, old[1].bounds.y);
    r.tree.patch(&set(2, 4, wrapper(250))).unwrap();
    assert_ne!(draw(&mut r), initial);
    assert!(!ran(&r, Reflow, 1));
    assert_eq!(spans(&r), old);
    check_fresh(&r);
    for word in ["AXA BBB Z", "AAA BBB C"] {
        r.tree.patch(&set(3, 5, text(word))).unwrap();
        draw(&mut r);
        assert!(ran(&r, Reflow, 1));
        check_fresh(&r);
    }
    r.tree.patch(&set(2, 4, wrapper(90))).unwrap();
    assert_eq!(draw(&mut r), initial);
    r.tree.patch(&set(1, 7, host(100., 2))).unwrap();
    r.tree.patch(&set(1, 7, host(64., 0))).unwrap();
    assert_eq!(draw(&mut r), initial);
    assert!(r.tree.layers.trace.is_empty());
}
#[test]
fn failure_and_a_later_accepted_edit_cannot_publish_a_candidates_line_masks() {
    let mut r = Raster::default();
    r.tree.upload(&doc("AAA BBB C", 64.)).unwrap();
    let a = draw(&mut r).to_vec();
    let hit = |r: &Raster| r.hit_at(48., 8.).unwrap().0;
    assert_eq!(hit(&r), 1);
    r.tree.patch(&set(1, 7, host(124., 0))).unwrap();
    assert!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|b, _| {
                assert_ne!(b, a);
                Err("sink refused".into())
            })
            .is_err()
    );
    assert_eq!(hit(&r), 1);
    assert_eq!(r.pixels(), a);
    r.settle(180., 170., 1.).unwrap();
    r.tree.patch(&set(1, 7, host(64., 0))).unwrap();
    r.tree.patch(&set(2, 4, wrapper(10))).unwrap();
    r.install();
    assert_eq!(hit(&r), 2);
    draw(&mut r);
    assert_eq!(hit(&r), 1);
    check_fresh(&r);
}
#[test]
fn display_scale_changes_rebuild_installed_lines_and_preserve_listener_local_coordinates() {
    let doc = doc("AAA BBB C", 64.);
    let mut live = Raster::default();
    live.tree.upload(&doc).unwrap();
    for scale in [1., 1.5, 2., 1.] {
        let mut fresh = Raster::default();
        fresh.tree.upload(&doc).unwrap();
        let bytes = fresh.draw(180., 170., scale).unwrap();
        assert!(live.draw(180., 170., scale).unwrap() == bytes);
        assert_eq!(bounds(&live, 1).width, 64. * scale);
        assert_eq!(live.hit_at(20. * scale, 8. * scale), Some((2, 16., 4.)));
    }
}
#[test]
fn admission_checks_kept_multiline_sources_at_their_final_site_and_rejects_bad_ink_atomically() {
    let mut r = Raster::default();
    r.tree.upload(&doc("A\nB", 64.)).unwrap();
    draw(&mut r);
    r.tree.patch(&set(1, 7, host(98., 1))).unwrap();
    let prior = r.tree.encode();
    for bad in [
        patch(1, 2, &[]),
        set(2, 4, block(&[vec![5, 0]])),
        set(2, 4, block(&[vec![12, 0, 3, 255, 255, 255, 255]])),
        set(2, 4, block(&[vec![52, 1, 255]])),
    ] {
        assert!(r.tree.patch(&bad).is_err());
        assert_eq!(r.tree.encode(), prior);
    }
    draw(&mut r);
    check_fresh(&r);
}
#[test]
fn a_closed_wrapper_can_switch_between_inline_float_and_ordinary_at_the_same_origin() {
    let words = "AAA BBB CCC DDD EEE";
    let attrs = |side: Option<u8>| {
        let fields = [
            Some(px(1, 60.)),
            Some(vec![40, 1]),
            side.map(|s| vec![5, s]),
        ];
        block(&fields.into_iter().flatten().collect::<Vec<_>>())
    };
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 7, block(&[px(1, 60.)]), &[2]),
            node(2, 4, attrs(None), &[3]),
            node(3, 5, text(words), &[]),
        ]))
        .unwrap();
    let start = draw(&mut r).to_vec();
    for side in [Some(0), None, Some(2), Some(1), None] {
        r.tree.patch(&set(2, 4, attrs(side))).unwrap();
        draw(&mut r);
        check_fresh(&r);
        let expected = if matches!(side, Some(0 | 2)) { 0 } else { 5 };
        assert_eq!(r.tree.layers.paragraphs.words, expected);
    }
    r.tree.patch(&patch(1, 2, &[])).unwrap();
    draw(&mut r);
    check_fresh(&r);
    assert_eq!(r.tree.layers.paragraphs.words, 0);
    r.tree
        .patch(&patch(2, 1, &[node(1, 7, block(&[px(1, 60.)]), &[2])]))
        .unwrap();
    assert_eq!(draw(&mut r), start);
    check_fresh(&r);
}
#[test]
fn long_word_ranges_and_reparented_spans_survive_without_changing_child_order() {
    let mut r = Raster::default();
    let tail = format!("{} Z", "\u{200b}".repeat(21_838));
    r.tree.upload(&doc(&tail, 40.)).unwrap();
    draw(&mut r);
    let p = r.tree.elements.ids[&1];
    let spans = r.tree.layers.paragraphs.fragments(p);
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].width, 0.);
    assert_eq!(spans[1].end, 21_840);
    let next = upload(&[
        node(4, 3, block(&[]), &[1, 5]),
        node(1, 7, host(40., 0), &[]),
        node(5, 7, host(88., 2), &[2]),
        node(2, 4, wrapper(90), &[3]),
        node(3, 5, text(&tail), &[]),
    ]);
    r.tree.patch(&at(next, 1)).unwrap();
    draw(&mut r);
    assert!(r.tree.layers.paragraphs.fragments(p).is_empty());
    check_fresh(&r);
}
