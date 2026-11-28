use super::*;

#[test]
fn position_receipts_share_focus_order_without_packing_offscreen_stops() {
    let ids = (2..1002).collect::<Vec<u64>>();
    let root = |pos| block(&[px(1, 60.), px(2, 40.), vec![7, 1], fraction(86, pos)]);
    let stop = block(&[px(1, 50.), px(2, 20.), vec![61, 1]]);
    let bytes = upload(
        &std::iter::once(node(1, 3, root(0.), &ids))
            .chain(ids.iter().map(|&i| node(i, 4, stop.clone(), &[])))
            .collect::<Vec<_>>(),
    );
    let mut writer = Publisher::new(0);
    let mut t = Tree::for_session(0);
    writer.apply(&bytes, 1.).unwrap().install(&mut t);
    let seal = |t: &mut Tree| {
        t.layout(60., 40., 1.).unwrap();
        assert!(t.collect_if_available());
        t.seal().unwrap()
    };
    let first = seal(&mut t);
    let nav = first.navigation.as_ref().unwrap();
    let last = nav.tab(None, true).unwrap();
    assert_eq!((last.id, last.bounds.y), (1001, 19980.));
    assert!(first.hits.captured(last.key, 1., 1.).is_none());
    assert_eq!(first.packed_counts()[2], 4); // 2 visible stops, wheel and chrome
    assert!(set_input(&mut t, 1, [0., 1.], 1, None));
    let end = seal(&mut t);
    let after = end.navigation.as_ref().unwrap();
    assert!(nav.same_order(after) && nav.same_receipts(after));
    assert_eq!(end.packed_counts()[2], 4);
    assert_eq!(
        end.hits.listener_at(end.viewport, 5., 25.).unwrap().id,
        last.id
    );
    assert_eq!(t.output.packed_children, [2, 2]);
    writer
        .apply(&set(1, 3, root(0.5)), 1.)
        .unwrap()
        .install(&mut t);
    let half = seal(&mut t);
    let replaced = half.navigation.as_ref().unwrap();
    assert!(nav.same_order(replaced));
    assert!(!nav.same_receipts(replaced));
    let port = last.parent.unwrap();
    assert_eq!(replaced.enclosure(port).unwrap().1.view.axes[1].pos, 0.5);
    assert_eq!(nav.enclosure(port).unwrap().1.view.axes[1].pos, 0.);
    assert_eq!(t.output.packed_children, [2, 2]);
    let reversed = ids.iter().rev().copied().collect::<Vec<_>>();
    writer
        .apply(&order(1, 3, &reversed), 1.)
        .unwrap()
        .install(&mut t);
    let reordered = seal(&mut t);
    let next = reordered.navigation.as_ref().unwrap();
    assert!(!nav.same_order(next));
    let first = next.tab(None, false).unwrap();
    assert_eq!((first.key, first.bounds.y), (last.key, 0.));
    assert_eq!(nav.tab(None, false).unwrap().id, 2);
}

#[test]
fn removing_a_branchs_last_stop_retires_its_ordinal_even_with_unchanged_sibling_geometry() {
    let b = block(&[px(1, 20.), px(2, 20.)]);
    let stop = block(&[px(1, 20.), px(2, 20.), vec![61, 1]]);
    let mut t = Tree::default();
    t.upload(&upload(&[
        node(1, 3, block(&[]), &[2, 3]),
        node(2, 4, stop.clone(), &[]),
        node(3, 3, b, &[4]),
        node(4, 4, stop, &[]),
    ]))
    .unwrap();
    let last = |t: &mut Tree| {
        t.refresh(60., 60., 1.).unwrap();
        t.seal()
            .unwrap()
            .navigation
            .unwrap()
            .tab(None, true)
            .unwrap()
            .id
    };
    assert_eq!(last(&mut t), 4);
    t.patch(&order(3, 3, &[])).unwrap();
    assert_eq!(last(&mut t), 2);
}

#[test]
fn newly_declared_stops_require_admission_before_the_pinned_workspace_or_order_changes() {
    let attrs = |focus: bool| block(&[px(1, 16.), px(2, 20.), vec![61, u8::from(focus)]]);
    let edits = [
        upload(&[
            node(1, 3, block(&[]), &[2, 3, 4]),
            node(2, 4, attrs(true), &[]),
            node(3, 4, attrs(false), &[]),
            node(4, 4, attrs(false), &[]),
        ]),
        set(3, 4, attrs(true)),
        set(4, 4, attrs(true)),
    ];
    let cap = {
        let s = storage::Storage::default();
        let mut w = Publisher::with_storage(0, s.clone());
        let mut t = Tree::for_session(0);
        for edit in &edits {
            w.apply(edit, 1.).unwrap().install(&mut t);
        }
        s.workspace.bytes().0
    };
    let mut w =
        Publisher::with_storage(0, storage::Storage::with_limits(1 << 20, 2 * cap, 1 << 20));
    let mut t = Tree::for_session(0);
    let seal = |t: &mut Tree| {
        t.refresh(30., 80., 1.).unwrap();
        t.seal().unwrap()
    };
    w.apply(&edits[0], 1.).unwrap().install(&mut t);
    let pin = seal(&mut t);
    w.apply(&edits[1], 1.).unwrap().install(&mut t);
    let installed = seal(&mut t);
    assert_eq!(w.apply(&edits[2], 1.).err().as_deref(), Some(storage::BUSY));
    assert_eq!(
        installed
            .navigation
            .as_ref()
            .unwrap()
            .tab(None, true)
            .unwrap()
            .id,
        3
    );
    assert!(
        installed
            .hits
            .listener_at(installed.viewport, 5., 45.)
            .is_none()
    );
    drop(pin);
    w.apply(&edits[2], 1.).unwrap().install(&mut t);
    let new = seal(&mut t);
    assert_eq!(
        new.navigation.as_ref().unwrap().tab(None, true).unwrap().id,
        4
    );
    assert_eq!(new.hits.listener_at(new.viewport, 5., 45.).unwrap().id, 4);
}
