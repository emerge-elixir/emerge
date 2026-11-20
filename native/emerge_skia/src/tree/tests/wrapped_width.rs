use super::*;

pub(super) fn chip(w: f64) -> Bytes {
    block(&[px(1, w), px(2, 12.)])
}
pub(super) fn nested(a: f64, b: f64) -> Bytes {
    let wrap = || block(&[vec![1, 0], number(4, 4.)]);
    upload(&[
        node(1, 2, wrap(), &[2, 3]),
        node(2, 2, wrap(), &[4, 5]),
        node(4, 6, chip(a), &[]),
        node(5, 6, chip(b), &[]),
        node(3, 4, block(&[weighted::weight(1, 3.)]), &[6]),
        node(6, 6, chip(20.), &[]),
    ])
}

#[test]
fn a_new_minimum_at_equal_preferred_width_can_move_a_peer_to_another_line() {
    let mut r = Raster::default();
    r.tree.upload(&nested(20., 20.)).unwrap();
    for (a, b, viewport, scale, aw, bx, by, bw, nested_h) in [
        (20., 20., 50., 1., 20., 24., 0., 26., 28.),
        (10., 30., 50., 1., 50., 0., 16., 50., 12.),
        (10., 30., 54., 1., 30., 34., 0., 20., 28.),
        (10., 30., 81., 1.5, 45., 51., 0., 30., 42.),
        (20., 20., 50., 1., 20., 24., 0., 26., 28.),
        (20., 20., 0., 1., 20., 0., 32., 20., 28.),
        (20., 20., 100., 1., 24., 28., 0., 72., 28.),
    ] {
        r.tree.patch(&set(4, 6, chip(a))).unwrap();
        r.tree.patch(&set(5, 6, chip(b))).unwrap();
        assert!(r.tree.layers.worksets_empty());
        r.tree.refresh(viewport, 80., scale).unwrap();
        let (aa, bb) = (bounds(&r, 2), bounds(&r, 3));
        assert_eq!(
            (aa.width, aa.height, bb.x, bb.y, bb.width),
            (aw, nested_h, bx, by, bw)
        );
        let mut cold = Tree::default();
        cold.upload(&nested(a, b)).unwrap();
        cold.refresh(viewport, 80., scale).unwrap();
        for (id, frame) in cold.frames() {
            assert_eq!(bounds(&r, id), frame);
        }
    }
}

#[test]
fn an_auto_column_can_consume_the_line_without_changing_its_measured_width() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 2, block(&[px(1, 100.), number(4, 4.)]), &[2, 3]),
            node(2, 3, block(&[]), &[4]),
            node(4, 4, block(&[]), &[5]),
            node(5, 6, chip(20.), &[]),
            node(3, 4, block(&[vec![1, 0]]), &[6]),
            node(6, 6, chip(20.), &[]),
        ]))
        .unwrap();
    for (id, kind, style, aw, bx, by, bw) in [
        (4, 4, block(&[]), 20., 24., 0., 76.),
        (4, 4, block(&[vec![1, 0]]), 100., 0., 16., 100.),
        (2, 3, block(&[vec![1, 1]]), 20., 24., 0., 76.),
        (2, 3, block(&[]), 100., 0., 16., 100.),
        (4, 4, block(&[]), 20., 24., 0., 76.),
    ] {
        r.tree.patch(&set(id, kind, style)).unwrap();
        draw(&mut r);
        let b = bounds(&r, 3);
        assert_eq!((bounds(&r, 2).width, b.x, b.y, b.width), (aw, bx, by, bw));
    }
}
