use super::*;

pub(super) fn attrs(kind: u8, flag: Option<u8>) -> Bytes {
    // Put the flag before the text: moving the UTF-8 within its raw block
    // must not look like new content to the glyph reader.
    let fields = flag
        .into_iter()
        .map(|v| vec![37, v])
        .chain([px(1, 32.), px(2, 20.), color([0, 0, 255]), vec![40, 1]])
        .chain((kind == 5).then(|| vec![21, 0, 2, b'H', b'i']))
        .collect::<Vec<_>>();
    block(&fields)
}

#[test]
fn inactive_even_flags_round_trip_without_waking_layout_paint_or_hits() {
    for kind in [4, 5, 6] {
        // El, Text and None have no main axis.
        let full = |flag| upload(&[node(1, kind, attrs(kind, flag), &[])]);
        let mut r = Raster::default();
        r.tree.upload(&full(None)).unwrap();
        let visible = draw(&mut r).to_vec();
        let draws = r.draws;
        for flag in [Some(1), Some(0), None] {
            let passes = r.tree.layers.invalidation_passes;
            r.tree.patch(&set(1, kind, attrs(kind, flag))).unwrap();
            assert!(r.tree.layers.worksets_empty());
            assert_eq!(r.tree.encode(), full(flag)); // false is not absence
            assert_eq!(draw(&mut r), visible);
            assert_eq!(r.tree.layers.invalidation_passes, passes + 1);
            assert!(r.tree.layers.trace.is_empty());
            assert_eq!(r.draws, draws);
            assert_eq!(r.hit_at(5., 6.), Some((1, 5., 6.)));
            check_fresh(&r);
        }
    }
}
