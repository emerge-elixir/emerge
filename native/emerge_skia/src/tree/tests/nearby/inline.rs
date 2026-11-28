use super::*;

// An exporting float paints before all inline ink, but its menu doesn't
// jump over an earlier/later span in the same visual band.
#[test]
fn floats_and_spans_submit_their_menus_in_declaration_order_once() {
    let mut r = Raster::default();
    let panel = |id| node(id, 4, chip(160., 100., [id as u8, 0, 0], true), &[]);
    r.tree
        .upload(&upload(&[
            node(1, 7, block(&[px(1, 140.), number(16, 12.)]), &[2, 4, 6]),
            mounted(node(2, 4, block(&[]), &[3]), &[(6, 7)]),
            node(3, 5, text("A"), &[]),
            mounted(
                node(4, 4, block(&[px(1, 20.), px(2, 20.), vec![5, 0]]), &[]),
                &[(6, 8)],
            ),
            mounted(node(6, 4, block(&[]), &[5]), &[(6, 9)]),
            node(5, 5, text("B"), &[]),
            panel(7),
            panel(8),
            panel(9),
        ]))
        .unwrap();
    for (order_, winner) in [([2, 4, 6], 9), ([2, 6, 4], 8), ([6, 4, 2], 7)] {
        r.tree.patch(&order(1, 7, &order_)).unwrap();
        draw(&mut r);
        assert_eq!(r.tree.output.mounted_visits, [3, 3]);
        assert_eq!(r.hit_at(100., 80.).unwrap().0, winner);
        assert_eq!(pixel(&r, 100, 80)[0], winner as u8);
        fresh(&r, &[(100., 80.)]);
    }
}

#[test]
fn continuation_backdrops_then_local_behind_then_ink_and_nested_escape() {
    let menu = || block(&[vec![1, 0], vec![2, 0], color([0, 255, 0]), vec![40, 1]]);
    let wrapper = |rgb| block(&[color(rgb), vec![40, 1]]);
    let doc = |mounts| {
        let owner = node(
            2,
            4,
            wrapper(if mounts { [255, 0, 0] } else { [0, 255, 0] }),
            &[3],
        );
        let nodes = [
            node(1, 7, prose::host(60., 1), &[2]),
            mounted(owner, if mounts { &[(1, 4)] } else { &[] }),
            node(3, 5, text("MM MMM W MMMMM"), &[]),
            mounted(node(4, 4, menu(), &[]), &[(4, 5)]),
            node(5, 4, chip(12., 8., [0, 0, 255], true), &[]),
        ];
        upload(&nodes[..if mounts { 5 } else { 3 }])
    };
    let mut twin = Raster::default();
    twin.tree.upload(&doc(false)).unwrap();
    draw(&mut twin);
    let mut r = Raster::default();
    r.tree.upload(&doc(true)).unwrap();
    draw(&mut r);
    assert_eq!(r.tree.output.mounted_visits, [2, 2]);
    let f = bounds(&r, 2);
    assert_eq!(bounds(&r, 4), f);
    let mut black = 0;
    for y in f.y.ceil() as usize..(f.y + f.height).floor() as usize {
        for x in f.x.ceil() as usize..(f.x + f.width).floor() as usize {
            let rgb = pixel(&twin, x, y);
            if rgb[3] == 255 {
                assert_eq!(pixel(&r, x, y), rgb);
                black += usize::from(rgb == [0, 0, 0, 255]);
            }
        }
    }
    assert!(black > 0);
    let (x, y) = (f.x + 2., f.y + f.height + 2.);
    assert_eq!(r.hit_at(x, y), Some((5, 2., 2.)));
    assert_eq!(pixel(&r, x as usize, y as usize), [0, 0, 255, 255]);
    let solves = r.tree.layers.paragraphs.solves;
    // Switch just the behind to a front; no paragraph or span solve.
    r.tree
        .patch(&patch(2, 2, &[mounted(edges(2, 4, &[3]), &[(6, 4)])]))
        .unwrap();
    draw(&mut r);
    assert_eq!(r.tree.layers.paragraphs.solves, solves);
    assert!(
        r.tree
            .layers
            .trace
            .iter()
            .all(|&(p, _, _)| matches!(p, PaintContext | Compose))
    );
    assert_eq!(r.hit_at(f.x + 2., f.y + 2.), Some((4, 2., 2.)));
    fresh(&r, &[(x, y)]);
}

#[test]
fn an_empty_phrase_does_not_lose_or_reuse_its_anchor() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 7, prose::host(128., 2), &[2, 3]),
            node(2, 5, text("prefix"), &[]),
            mounted(node(3, 4, block(&[]), &[4]), &[(4, 5)]),
            node(4, 5, text("AA BBB CC"), &[]),
            node(5, 4, chip(10., 8., [0, 255, 0], true), &[]),
        ]))
        .unwrap();
    for empty in [false, true, false] {
        r.tree
            .patch(&set(4, 5, text(if empty { "" } else { "AA BBB CC" })))
            .unwrap();
        draw(&mut r);
        let f = bounds(&r, 3);
        assert_eq!(f.width == 0., empty);
        assert_eq!((bounds(&r, 5).x, bounds(&r, 5).y), (f.x, f.y + f.height));
        assert_eq!(r.tree.output.mounted_visits, [1, 1]);
        assert_eq!(r.hit_at(f.x + 2., f.y + f.height + 2.), Some((5, 2., 2.)));
        fresh(&r, &[(f.x + 2., f.y + f.height + 2.)]);
    }
}
