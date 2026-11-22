use super::*;

fn pressable_list(styled: bool) -> ElementTree {
    let mut tree = ElementTree::new();
    tree.set_revision(1);
    tree.insert(encoded_node(
        NodeId(9),
        ElementKind::Column,
        &[7],
        200.0,
        100.0,
    ));
    for id in 10..15 {
        let mut row = encoded_node(NodeId(id), ElementKind::El, &[61], 200.0, 80.0);
        row.spec.declared.mouse_down = styled.then_some(MouseOverAttrs {
            alpha: Some(0.7),
            ..Default::default()
        });
        tree.insert(row);
    }
    tree.set_children(&NodeId(9), (10..15).map(NodeId).collect())
        .unwrap();
    tree.set_root_id(NodeId(9));
    tree.stamp_all_mounted_at_revision(1);
    tree
}

#[test]
fn drag_on_pressable_row_flings_the_scroll_container_without_pressing_the_row() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    for styled in [false, true] {
        let mut engine = TreeUpdateEngine::new(pressable_list(styled), 200, 100);
        let (mut host, recorder) = recorded_host();
        host.install_rebuild(response(
            &mut engine,
            vec![TreeMsg::RebuildRegistry],
            policy,
        ));
        host.handle_input(button(ACTION_PRESS, 20.0, 75.0));
        settle(&mut host, &mut engine, policy);
        host.handle_input(InputEvent::CursorPos { x: 20.0, y: 50.0 });
        settle(&mut host, &mut engine, policy);
        assert!(host.driver.runtime.runtime_overlay.click_press.is_none());
        assert!(
            !engine
                .tree()
                .get(&NodeId(10))
                .unwrap()
                .runtime
                .mouse_down_active,
            "scroll promotion must cancel the row's pressed feedback"
        );
        host.handle_input(InputEvent::CursorPos { x: 20.0, y: 25.0 });
        settle(&mut host, &mut engine, policy);
        assert!(engine.tree().get(&NodeId(9)).unwrap().layout.scroll_y > 0.0);
        assert_eq!(
            host.driver.runtime.drag_motion.as_ref().unwrap().element_id,
            NodeId(9),
            "fling ownership must follow the scrolling parent, not the pressed row"
        );
        // Target ownership, not wall-clock sampling latency, is under test here.
        host.driver
            .runtime
            .drag_motion
            .as_mut()
            .unwrap()
            .velocity_px_per_sec = -1_000.0;
        host.handle_input(button(ACTION_RELEASE, 20.0, 25.0));
        settle(&mut host, &mut engine, policy);
        let inertia = host.driver.runtime.inertial_scroll.as_ref().unwrap();
        assert_eq!(inertia.element_id, NodeId(9));
        let presented_at = inertia.started_at + Duration::from_millis(16);
        let before = engine.tree().get(&NodeId(9)).unwrap().layout.scroll_y;
        host.handle_present_timing(presented_at, presented_at + Duration::from_millis(16));
        settle(&mut host, &mut engine, policy);
        assert!(engine.tree().get(&NodeId(9)).unwrap().layout.scroll_y > before);
        assert!(recorder.events.lock().unwrap().iter().all(|(_, kind)| {
            !matches!(kind, ElementEventKind::Click | ElementEventKind::Press)
        }));
        assert!(
            !engine
                .tree()
                .get(&NodeId(10))
                .unwrap()
                .runtime
                .mouse_down_active
        );

        // A new touch must stop the parent fling, even when it lands on another row.
        host.handle_input(button(ACTION_PRESS, 20.0, 25.0));
        settle(&mut host, &mut engine, policy);
        assert!(host.driver.runtime.inertial_scroll.is_none());
    }
}

#[test]
fn tapping_a_scrollable_list_row_still_emits_its_press_once() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    for styled in [false, true] {
        let mut engine = TreeUpdateEngine::new(pressable_list(styled), 200, 100);
        let (mut host, recorder) = recorded_host();
        host.install_rebuild(response(
            &mut engine,
            vec![TreeMsg::RebuildRegistry],
            policy,
        ));
        host.handle_input(button(ACTION_PRESS, 20.0, 50.0));
        settle(&mut host, &mut engine, policy);
        host.handle_input(button(ACTION_RELEASE, 20.0, 50.0));
        settle(&mut host, &mut engine, policy);
        let presses: Vec<_> = recorder
            .events
            .lock()
            .unwrap()
            .iter()
            .copied()
            .filter(|(_, kind)| *kind == ElementEventKind::Press)
            .collect();
        assert_eq!(presses, [(NodeId(10), ElementEventKind::Press)]);
        assert_eq!(engine.tree().get(&NodeId(9)).unwrap().layout.scroll_y, 0.0);
        assert!(host.driver.runtime.inertial_scroll.is_none());
        assert!(
            !engine
                .tree()
                .get(&NodeId(10))
                .unwrap()
                .runtime
                .mouse_down_active
        );
    }
}

#[test]
fn libinput_primary_touch_preserves_row_taps_and_scroll_capture() {
    use crate::input::TouchPhase;
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for styled in [false, true] {
        let policy = TreeUpdateDecodePolicy::ReturnErr;
        let mut engine = TreeUpdateEngine::new(pressable_list(styled), 200, 100);
        let (mut host, recorder) = recorded_host();
        host.install_rebuild(response(
            &mut engine,
            vec![TreeMsg::RebuildRegistry],
            policy,
        ));
        let touch = |phase, y| InputEvent::Touch {
            id: 42,
            phase,
            x: 20.0,
            y,
        };
        host.handle_input(touch(TouchPhase::Down, 50.0));
        settle(&mut host, &mut engine, policy);
        host.handle_input(touch(TouchPhase::Up, 50.0));
        settle(&mut host, &mut engine, policy);
        assert_eq!(
            recorder
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, kind)| *kind == ElementEventKind::Press)
                .count(),
            1
        );
        host.handle_input(touch(TouchPhase::Down, 75.0));
        settle(&mut host, &mut engine, policy);
        host.handle_input(touch(TouchPhase::Move, 50.0));
        settle(&mut host, &mut engine, policy);
        host.handle_input(touch(TouchPhase::Move, 25.0));
        settle(&mut host, &mut engine, policy);
        assert_eq!(
            host.driver.runtime.drag_motion.as_ref().unwrap().element_id,
            NodeId(9)
        );
        assert!(
            !engine
                .tree()
                .get(&NodeId(10))
                .unwrap()
                .runtime
                .mouse_down_active
        );
        host.handle_input(touch(TouchPhase::Cancel, 25.0));
        settle(&mut host, &mut engine, policy);
        assert!(host.driver.runtime.inertial_scroll.is_none());
        assert_eq!(
            recorder
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, kind)| *kind == ElementEventKind::Press)
                .count(),
            1
        );
    }
}

#[test]
fn drag_from_a_pressable_row_at_the_top_overscrolls_the_list() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut engine = TreeUpdateEngine::new(pressable_list(false), 200, 100);
    let (mut host, recorder) = recorded_host();
    host.driver.runtime.set_scroll_bounce(true);
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        policy,
    ));
    host.handle_input(button(ACTION_PRESS, 20.0, 20.0));
    settle(&mut host, &mut engine, policy);
    // At the top only the opposite probe finds the list; it becomes the overscroll target.
    host.handle_input(InputEvent::CursorPos { x: 20.0, y: 35.0 });
    settle(&mut host, &mut engine, policy);
    assert_eq!(host.driver.runtime.drag_scroll_target, Some(NodeId(9)));
    host.handle_input(InputEvent::CursorPos { x: 20.0, y: 75.0 });
    settle(&mut host, &mut engine, policy);
    let overscroll = host.driver.runtime.overscroll.as_ref().unwrap();
    assert_eq!(overscroll.element_id, NodeId(9));
    let shown = f32::from_bits(
        engine
            .tree()
            .get(&NodeId(9))
            .unwrap()
            .runtime
            .scroll_overscroll[1],
    );
    assert!(shown < 0.0 && shown > -40.0, "{shown}");
    host.handle_input(button(ACTION_RELEASE, 20.0, 75.0));
    settle(&mut host, &mut engine, policy);
    assert!(
        recorder.events.lock().unwrap().iter().all(|(_, kind)| {
            !matches!(kind, ElementEventKind::Click | ElementEventKind::Press)
        })
    );
    let spring = host
        .driver
        .runtime
        .overscroll
        .as_ref()
        .and_then(|overscroll| overscroll.spring.as_ref())
        .expect("released overscroll springs back");
    let done = spring.started_at + Duration::from_millis(1_300);
    host.handle_present_timing(done, done + Duration::from_millis(16));
    settle(&mut host, &mut engine, policy);
    assert!(host.driver.runtime.overscroll.is_none());
    assert_eq!(
        engine
            .tree()
            .get(&NodeId(9))
            .unwrap()
            .runtime
            .scroll_overscroll,
        [0; 2]
    );
}

// Column 9 (200 x 100, scrollbar_y) with two pressable 30 px rows: the content fits.
fn fitting_list() -> ElementTree {
    let mut tree = ElementTree::new();
    tree.set_revision(1);
    tree.insert(encoded_node(
        NodeId(9),
        ElementKind::Column,
        &[7],
        200.0,
        100.0,
    ));
    for id in [10, 11] {
        tree.insert(encoded_node(
            NodeId(id),
            ElementKind::El,
            &[61],
            200.0,
            30.0,
        ));
    }
    tree.set_children(&NodeId(9), vec![NodeId(10), NodeId(11)])
        .unwrap();
    tree.set_root_id(NodeId(9));
    tree.stamp_all_mounted_at_revision(1);
    tree
}

fn fitting_host(
    tree: ElementTree,
    bounce: bool,
) -> (TreeUpdateEngine, HostEventRuntime, Arc<Recorder>) {
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut engine = TreeUpdateEngine::new(tree, 200, 100);
    let (mut host, recorder) = recorded_host();
    host.driver.runtime.set_scroll_bounce(bounce);
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        policy,
    ));
    (engine, host, recorder)
}

fn overscroll_of(engine: &TreeUpdateEngine, id: NodeId) -> f32 {
    f32::from_bits(engine.tree().get(&id).unwrap().runtime.scroll_overscroll[1])
}

fn drive(engine: &mut TreeUpdateEngine, host: &mut HostEventRuntime, event: InputEvent) {
    host.handle_input(event);
    settle(host, engine, TreeUpdateDecodePolicy::ReturnErr);
}

#[test]
fn drag_on_content_that_fits_overscrolls_with_resistance_and_springs_back() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, recorder) = fitting_host(fitting_list(), true);

    // Starts on a pressable row: past the dead zone the drag belongs to the column.
    drive(&mut engine, &mut host, button(ACTION_PRESS, 20.0, 15.0));
    drive(
        &mut engine,
        &mut host,
        InputEvent::CursorPos { x: 20.0, y: 40.0 },
    );
    drive(
        &mut engine,
        &mut host,
        InputEvent::CursorPos { x: 20.0, y: 75.0 },
    );
    let pulled = overscroll_of(&engine, NodeId(9));
    assert!(pulled < 0.0 && pulled > -60.0, "{pulled}");
    assert_eq!(engine.tree().get(&NodeId(9)).unwrap().layout.scroll_y, 0.0);

    drive(&mut engine, &mut host, button(ACTION_RELEASE, 20.0, 75.0));
    assert!(host.driver.runtime.inertial_scroll.is_none());
    let started_at = host
        .driver
        .runtime
        .overscroll
        .as_ref()
        .and_then(|overscroll| overscroll.spring.as_ref())
        .expect("spring running")
        .started_at;
    let at = started_at + Duration::from_millis(450);
    host.handle_present_timing(at, at + Duration::from_millis(16));
    settle(&mut host, &mut engine, TreeUpdateDecodePolicy::ReturnErr);
    assert_eq!(overscroll_of(&engine, NodeId(9)), 0.0);
    assert!(host.driver.runtime.overscroll.is_none());
    assert!(
        recorder
            .events
            .lock()
            .unwrap()
            .iter()
            .all(|(_, kind)| *kind != ElementEventKind::Press)
    );
}

#[test]
fn without_bounce_a_drag_on_content_that_fits_moves_nothing() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = fitting_host(fitting_list(), false);

    drive(&mut engine, &mut host, button(ACTION_PRESS, 20.0, 80.0));
    drive(
        &mut engine,
        &mut host,
        InputEvent::CursorPos { x: 20.0, y: 95.0 },
    );
    drive(
        &mut engine,
        &mut host,
        InputEvent::CursorPos { x: 20.0, y: 130.0 },
    );
    assert!(host.driver.runtime.overscroll.is_none());
    assert_eq!(overscroll_of(&engine, NodeId(9)), 0.0);
}

#[test]
fn tapping_a_row_in_content_that_fits_still_presses_it_with_bounce_on() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, recorder) = fitting_host(fitting_list(), true);

    drive(&mut engine, &mut host, button(ACTION_PRESS, 20.0, 15.0));
    drive(&mut engine, &mut host, button(ACTION_RELEASE, 20.0, 15.0));
    let presses: Vec<_> = recorder
        .events
        .lock()
        .unwrap()
        .iter()
        .copied()
        .filter(|(_, kind)| *kind == ElementEventKind::Press)
        .collect();
    assert_eq!(presses, [(NodeId(10), ElementEventKind::Press)]);
    assert!(host.driver.runtime.overscroll.is_none());
}

#[test]
fn a_scrolling_parent_takes_the_drag_before_a_child_whose_content_fits() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    // Column 9 scrolls (50 + 4 x 100 px of content); child column 20 fits (2 x 20 in 50 px).
    let mut tree = ElementTree::new();
    tree.set_revision(1);
    tree.insert(encoded_node(
        NodeId(9),
        ElementKind::Column,
        &[7],
        200.0,
        100.0,
    ));
    tree.insert(encoded_node(
        NodeId(20),
        ElementKind::Column,
        &[7],
        200.0,
        50.0,
    ));
    for id in [21, 22] {
        tree.insert(encoded_node(NodeId(id), ElementKind::El, &[], 200.0, 20.0));
    }
    for id in 10..14 {
        tree.insert(encoded_node(NodeId(id), ElementKind::El, &[], 200.0, 100.0));
    }
    tree.set_children(&NodeId(20), vec![NodeId(21), NodeId(22)])
        .unwrap();
    tree.set_children(
        &NodeId(9),
        [NodeId(20)]
            .into_iter()
            .chain((10..14).map(NodeId))
            .collect(),
    )
    .unwrap();
    tree.set_root_id(NodeId(9));
    tree.stamp_all_mounted_at_revision(1);
    let (mut engine, mut host, _) = fitting_host(tree, true);

    // Upward drag inside the child: the parent can scroll down and takes it.
    drive(&mut engine, &mut host, button(ACTION_PRESS, 20.0, 40.0));
    drive(
        &mut engine,
        &mut host,
        InputEvent::CursorPos { x: 20.0, y: 25.0 },
    );
    drive(
        &mut engine,
        &mut host,
        InputEvent::CursorPos { x: 20.0, y: 10.0 },
    );
    assert!(engine.tree().get(&NodeId(9)).unwrap().layout.scroll_y > 0.0);
    assert_eq!(overscroll_of(&engine, NodeId(20)), 0.0);
    assert!(
        host.driver
            .runtime
            .overscroll
            .as_ref()
            .is_none_or(|overscroll| overscroll.element_id != NodeId(20))
    );
}
