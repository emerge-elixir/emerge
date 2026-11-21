use super::*;

fn xy(r: &Raster, id: u64) -> (f32, f32) {
    let b = bounds(r, id);
    (b.x, b.y)
}

#[test]
fn each_axis_uses_only_the_immediate_explicit_override() {
    let attrs = |x: Option<u8>, y: Option<u8>| {
        block(
            &[px(1, 20.), px(2, 12.)]
                .into_iter()
                .chain(x.map(|z| vec![5, z]))
                .chain(y.map(|z| vec![6, z]))
                .collect::<Vec<_>>(),
        )
    };
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, el_child::demand(100., 64.), &[2]),
            node(
                2,
                4,
                block(&[
                    px(1, 100.),
                    px(2, 64.),
                    vec![5, 2],
                    vec![6, 2],
                    [
                        vec![3, 2],
                        [2., 10., 6., 4.]
                            .into_iter()
                            .flat_map(f64::to_be_bytes)
                            .collect(),
                    ]
                    .concat(),
                ]),
                &[3],
            ),
            node(3, 4, attrs(None, None), &[4]),
            node(4, 6, el_child::demand(4., 4.), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    // The outer El puts the card at the end. The card's own child does
    // not inherit its grandparent's alignment.
    assert_eq!((xy(&r, 3), xy(&r, 4)), ((70., 46.), (70., 46.)));
    for (x, y, card, dot) in [
        (Some(0), None, (4., 46.), (4., 46.)),
        (None, Some(1), (70., 24.), (70., 28.)),
        (Some(1), Some(0), (37., 2.), (45., 2.)),
        (None, None, (70., 46.), (70., 46.)),
    ] {
        let before = r.tree.layers.invalidation_passes;
        r.tree.patch(&set(3, 4, attrs(Some(2), Some(2)))).unwrap();
        r.tree.patch(&set(3, 4, attrs(x, y))).unwrap();
        assert!(r.tree.layers.worksets_empty());
        draw(&mut r);
        assert_eq!(r.tree.layers.invalidation_passes, before + 1);
        assert_eq!((xy(&r, 3), xy(&r, 4)), (card, dot));
        assert!(ran(&r, Place, 2) && !ran(&r, Place, 1));
        assert!(
            r.tree
                .layers
                .trace
                .iter()
                .all(|(p, _, _)| !matches!(p, Context | Measure | Flow | Reflow))
        );
        check_fresh(&r);
    }
}

pub(super) fn packed_attrs(op: u8, align: u8) -> Bytes {
    packed_width(vec![1, 1], op, align)
}
fn packed_width(width: Bytes, op: u8, align: u8) -> Bytes {
    block(&[
        width,
        row_height::mixed(op),
        vec![5, align],
        [
            vec![36],
            6f64.to_be_bytes().to_vec(),
            4f64.to_be_bytes().to_vec(),
        ]
        .concat(),
        color([255, 0, 0]),
        vec![40, 1],
    ])
}
// The first card's height is reflowed, not a declared height slot.
// With a fixed-width El the packed width can be consumed late in Place.
pub(super) fn packed() -> Bytes {
    upload(&[
        node(1, 4, block(&[px(1, 64.), px(2, 43.), vec![40, 1]]), &[2]),
        node(2, 14, packed_attrs(5, 2), &[3, 4]),
        node(3, 4, block(&[px(1, 12.)]), &[5]),
        node(
            4,
            4,
            block(&[px(1, 12.), px(2, 20.), color([0, 0, 255])]),
            &[],
        ),
        node(5, 6, el_child::demand(12., 20.), &[]),
    ])
}
#[test]
fn changing_just_the_width_policy_at_an_equal_early_width_updates_the_el_offset() {
    let mut r = Raster::default();
    r.tree.upload(&packed()).unwrap();
    for (policy, x, w) in [
        (vec![1, 1], 34., 30.),
        (px(1, 12.), 52., 12.),
        ([vec![1, 4, 1], number(2, 20.)].concat(), 44., 20.),
        (vec![1, 1], 34., 30.),
    ] {
        r.tree
            .patch(&set(2, 14, packed_width(policy, 4, 2)))
            .unwrap();
        draw(&mut r);
        let b = bounds(&r, 2);
        assert_eq!((b.x, b.width), (x, w));
        check_fresh(&r);
    }
}
#[test]
fn a_right_aligned_packed_width_reads_a_height_edit_even_at_equal_natural_fit() {
    let mut r = Raster::default();
    r.tree.upload(&packed()).unwrap();
    for (op, x, w, h, card_y) in [
        (5, 52., 12., 44., 24.),
        (4, 34., 30., 43., 0.),
        (5, 52., 12., 44., 24.),
    ] {
        r.tree.patch(&set(2, 14, packed_attrs(op, 2))).unwrap();
        draw(&mut r);
        let b = bounds(&r, 2);
        assert_eq!((b.x, b.width, b.height), (x, w, h));
        assert_eq!(xy(&r, 4), (52., card_y));
        check_fresh(&r);
    }
    r.tree.patch(&set(2, 14, packed_attrs(4, 2))).unwrap();
    let pending = r.tree.encode();
    assert!(
        r.tree
            .patch(&patch(1, 2, &[]))
            .unwrap_err()
            .contains("alignment")
    );
    assert_eq!(pending, r.tree.encode());
    let prev = r.pixels().to_vec();
    assert!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("handoff".into()))
            .is_err()
    );
    assert_eq!(r.pixels(), prev);
    assert_eq!(r.hit_at(40., 6.), Some((1, 40., 6.)));
    draw(&mut r);
    assert_eq!(r.hit_at(40., 6.), Some((2, 6., 6.)));
}
