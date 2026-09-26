use super::*;

fn selection_drag_stops(kind: ElementKind, styled: bool, end: InputEvent, delayed: bool) {
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut tree = editable_tree(false);
    let input = tree.get_mut(&NodeId(9)).unwrap();
    input.spec.kind = kind;
    input.spec.declared.content = Some("Style showcase input".into());
    input.spec.declared.mouse_over = styled.then(MouseOverAttrs::default);
    input.spec.declared.focused = styled.then(MouseOverAttrs::default);
    input.spec.declared.mouse_down = styled.then_some(MouseOverAttrs {
        move_y: Some(1.0),
        ..Default::default()
    });
    // Like the showcase field, this input has no Elixir on_change callback.
    input.spec.declared.on_change = None;
    input.spec.declared.on_mouse_up = styled.then_some(true);
    input.runtime.text_input_focused = false;
    input.runtime.focused_active = false;
    let mut engine = TreeUpdateEngine::new(tree, 200, 30);
    let (mut host, recorder) = recorded_host();
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        policy,
    ));
    host.handle_input(button(ACTION_PRESS, 1.0, 12.0));
    assert!(host.driver.runtime.runtime_overlay.text_drag.is_some());
    assert_eq!(
        host.driver.runtime.runtime_overlay.click_press.is_some(),
        styled
    );
    if !delayed {
        settle(&mut host, &mut engine, policy);
    }
    host.handle_input(InputEvent::CursorPos { x: 95.0, y: 12.0 });
    if !delayed {
        settle(&mut host, &mut engine, policy);
        let state = &host.driver.runtime.text_states[&NodeId(9)];
        assert!(
            state
                .selection_anchor
                .is_some_and(|anchor| anchor != state.cursor)
        );
    }
    // Releasing another button must not terminate the held left-button drag.
    host.handle_input(InputEvent::CursorButton {
        button: "right".into(),
        action: ACTION_RELEASE,
        mods: 0,
        x: 95.0,
        y: 12.0,
    });
    if !delayed {
        settle(&mut host, &mut engine, policy);
        assert!(host.driver.runtime.runtime_overlay.text_drag.is_some());
    }
    host.handle_input(end.clone());
    settle(&mut host, &mut engine, policy);
    assert!(
        host.driver.runtime.runtime_overlay.text_drag.is_none(),
        "{kind:?}, styled={styled}, delayed={delayed}: {end:?} must end selection dragging"
    );
    assert!(host.driver.runtime.runtime_overlay.click_press.is_none());
    assert!(
        !engine
            .tree()
            .get(&NodeId(9))
            .unwrap()
            .runtime
            .mouse_down_active
    );
    let expect_mouse_up = styled
        && matches!(
            &end,
            InputEvent::CursorButton { x, .. } if *x < 200.0
        );
    assert_eq!(
        recorder
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, kind)| *kind == ElementEventKind::MouseUp)
            .count(),
        usize::from(expect_mouse_up),
        "the normal release callback must still run exactly once"
    );

    let state = &host.driver.runtime.text_states[&NodeId(9)];
    let selection = (state.cursor, state.selection_anchor);
    assert!(selection.1.is_some_and(|anchor| anchor != selection.0));
    host.handle_input(InputEvent::CursorPos { x: 150.0, y: 12.0 });
    settle(&mut host, &mut engine, policy);
    let state = &host.driver.runtime.text_states[&NodeId(9)];
    assert_eq!((state.cursor, state.selection_anchor), selection);
}

#[test]
fn styled_text_selection_stops_on_release_inside_and_outside() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for kind in [ElementKind::TextInput, ElementKind::Multiline] {
        for styled in [false, true] {
            for x in [95.0, 250.0] {
                for delayed in [false, true] {
                    selection_drag_stops(kind, styled, button(ACTION_RELEASE, x, 12.0), delayed);
                }
            }
        }
    }
}

#[test]
fn styled_text_selection_stops_on_window_leave_and_blur() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for kind in [ElementKind::TextInput, ElementKind::Multiline] {
        for styled in [false, true] {
            for end in [
                InputEvent::CursorEntered { entered: false },
                InputEvent::Focused { focused: false },
            ] {
                for delayed in [false, true] {
                    selection_drag_stops(kind, styled, end.clone(), delayed);
                }
            }
        }
    }
}

#[test]
fn styled_slider_drag_stops_when_the_press_ends() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    for end in [
        button(ACTION_RELEASE, 150.0, 15.0),
        button(ACTION_RELEASE, 250.0, 15.0),
        InputEvent::CursorEntered { entered: false },
        InputEvent::Focused { focused: false },
    ] {
        for delayed in [false, true] {
            let mut tree = encoded_tree(ElementKind::Slider, &[56]);
            tree.get_mut(&NodeId(9)).unwrap().spec.declared.mouse_down = Some(MouseOverAttrs {
                move_y: Some(1.0),
                ..Default::default()
            });
            tree.insert(encoded_node(NodeId(10), ElementKind::El, &[], 200.0, 10.0));
            tree.insert(encoded_node(NodeId(11), ElementKind::El, &[], 20.0, 20.0));
            tree.set_children(&NodeId(9), vec![NodeId(10), NodeId(11)])
                .unwrap();
            tree.stamp_all_mounted_at_revision(1);
            let mut engine = TreeUpdateEngine::new(tree, 200, 30);
            let mut host = host();
            host.install_rebuild(response(
                &mut engine,
                vec![TreeMsg::RebuildRegistry],
                policy,
            ));
            host.handle_input(button(ACTION_PRESS, 50.0, 15.0));
            assert!(host.driver.runtime.runtime_overlay.slider_drag.is_some());
            assert!(host.driver.runtime.runtime_overlay.click_press.is_some());
            if !delayed {
                settle(&mut host, &mut engine, policy);
            }
            host.handle_input(InputEvent::CursorPos { x: 150.0, y: 15.0 });
            if !delayed {
                settle(&mut host, &mut engine, policy);
            }
            host.handle_input(end.clone());
            settle(&mut host, &mut engine, policy);
            assert!(
                host.driver.runtime.runtime_overlay.slider_drag.is_none(),
                "{end:?}"
            );
            assert!(host.driver.runtime.runtime_overlay.click_press.is_none());
            assert!(
                !engine
                    .tree()
                    .get(&NodeId(9))
                    .unwrap()
                    .runtime
                    .mouse_down_active
            );
            let value = host.driver.runtime.slider_states[&NodeId(9)].value;
            assert_eq!(value, 0.75);
            host.handle_input(InputEvent::CursorPos { x: 20.0, y: 15.0 });
            settle(&mut host, &mut engine, policy);
            assert_eq!(host.driver.runtime.slider_states[&NodeId(9)].value, value);
        }
    }
}
