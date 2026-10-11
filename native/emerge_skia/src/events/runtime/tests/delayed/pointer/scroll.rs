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
