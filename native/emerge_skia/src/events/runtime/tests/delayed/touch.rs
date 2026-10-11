use super::*;
use crate::input::TouchPhase;

#[derive(Default)]
struct Recorder(Mutex<Vec<ElementEventKind>>, Mutex<Vec<InputEvent>>);
impl HostEventSink for Recorder {
    fn send_raw_input(&self, event: &InputEvent) {
        self.1.lock().unwrap().push(event.clone());
    }
    fn send_element_event(
        &self,
        _: &NodeId,
        kind: ElementEventKind,
        _: Option<&registry_builder::ElixirEventPayload>,
    ) {
        self.0.lock().unwrap().push(kind);
    }
}
fn setup(
    styled: bool,
    hold: VirtualKeyHoldMode,
) -> (TreeUpdateEngine, HostEventRuntime, Arc<Recorder>) {
    let mut tree = ElementTree::new();
    tree.set_revision(1);
    let node = |id, kind, attrs: Attrs| {
        let px = |tag, length: Option<Length>| {
            let Some(Length::Px(value)) = length else {
                panic!("fixture length")
            };
            [tag, 2]
                .into_iter()
                .chain(value.to_be_bytes())
                .collect::<Vec<_>>()
        };
        let sized = [px(1, attrs.width.clone()), px(2, attrs.height.clone())];
        let content = attrs.content.as_ref().map(|text| {
            [21].into_iter()
                .chain((text.len() as u16).to_be_bytes())
                .chain(text.bytes())
                .collect::<Vec<_>>()
        });
        let change = attrs.on_change.map(|value| vec![56, u8::from(value)]);
        let key = attrs.virtual_key.as_ref().map(|key| {
            let VirtualKeyTapAction::Text(text) = &key.tap else {
                panic!("fixture text")
            };
            let hold = match key.hold {
                VirtualKeyHoldMode::None => 0,
                VirtualKeyHoldMode::Repeat => 1,
                VirtualKeyHoldMode::Event => 2,
            };
            let payload: Vec<_> = [0, hold]
                .into_iter()
                .chain(key.hold_ms.to_be_bytes())
                .chain(key.repeat_ms.to_be_bytes())
                .chain((text.len() as u16).to_be_bytes())
                .chain(text.bytes())
                .collect();
            [75].into_iter()
                .chain((payload.len() as u32).to_be_bytes())
                .chain(payload)
                .collect::<Vec<_>>()
        });
        let style = attrs.mouse_down.as_ref().map(|_| {
            let payload: Vec<_> = [0, 1, 35]
                .into_iter()
                .chain(0.7_f64.to_be_bytes())
                .collect();
            [60].into_iter()
                .chain((payload.len() as u32).to_be_bytes())
                .chain(payload)
                .collect::<Vec<_>>()
        });
        let entries: Vec<_> = sized
            .into_iter()
            .chain(content)
            .chain(change)
            .chain(key)
            .chain(style)
            .collect();
        let raw = (entries.len() as u16)
            .to_be_bytes()
            .into_iter()
            .chain(entries.into_iter().flatten())
            .collect();
        Element::with_attrs(NodeId(id), kind, raw, attrs)
    };
    tree.insert(node(
        1,
        ElementKind::Column,
        Attrs {
            width: Some(Length::Px(200.0)),
            height: Some(Length::Px(100.0)),
            ..Default::default()
        },
    ));
    let mut field = node(
        9,
        ElementKind::TextInput,
        Attrs {
            width: Some(Length::Px(200.0)),
            height: Some(Length::Px(30.0)),
            content: Some(String::new()),
            on_change: Some(true),
            ..Default::default()
        },
    );
    field.runtime.text_input_focused = true;
    field.runtime.focused_active = true;
    field.runtime.text_input_cursor = Some(0);
    tree.insert(field);
    tree.insert(node(
        2,
        ElementKind::Row,
        Attrs {
            width: Some(Length::Px(200.0)),
            height: Some(Length::Px(40.0)),
            ..Default::default()
        },
    ));
    for (id, text) in [(10, "a"), (11, "b")] {
        tree.insert(node(
            id,
            ElementKind::El,
            Attrs {
                width: Some(Length::Px(90.0)),
                height: Some(Length::Px(40.0)),
                mouse_down: styled.then_some(MouseOverAttrs {
                    alpha: Some(0.7),
                    ..Default::default()
                }),
                virtual_key: Some(VirtualKeySpec {
                    tap: VirtualKeyTapAction::Text(text.into()),
                    hold,
                    hold_ms: 350,
                    repeat_ms: 40,
                }),
                ..Default::default()
            },
        ));
    }
    tree.set_children(&NodeId(2), vec![NodeId(10), NodeId(11)])
        .unwrap();
    tree.set_children(&NodeId(1), vec![NodeId(9), NodeId(2)])
        .unwrap();
    tree.set_root_id(NodeId(1));
    tree.stamp_all_mounted_at_revision(1);
    let mut engine = TreeUpdateEngine::new(tree, 200, 100);
    let recorder = Arc::new(Recorder::default());
    let mut host = HostEventRuntime::new(false, 40.0, false, recorder.clone(), None);
    host.install_rebuild(response(
        &mut engine,
        vec![
            TreeMsg::Resize {
                width: 200.0,
                height: 100.0,
                scale: 1.0,
            },
            TreeMsg::RebuildRegistry,
        ],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    settle(&mut engine, &mut host);
    (engine, host, recorder)
}
fn settle(engine: &mut TreeUpdateEngine, host: &mut HostEventRuntime) {
    for _ in 0..64 {
        let messages = host.drain_tree_messages();
        if messages.is_empty() {
            assert!(!host.driver.runtime.listener_lane.is_stale());
            return;
        }
        match engine
            .process_messages(
                messages,
                TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
            )
            .unwrap()
        {
            TreeUpdateEffect::RegistryUpdate { rebuild } => host.install_rebuild(rebuild),
            TreeUpdateEffect::Layout { output, .. } if output.event_rebuild_changed => {
                host.install_rebuild(output.event_rebuild)
            }
            TreeUpdateEffect::Layout { .. } | TreeUpdateEffect::Skip => {}
            TreeUpdateEffect::Stop => panic!("unexpected stop"),
        }
    }
    panic!("touch reconciliation did not settle");
}
fn touch(id: u64, phase: TouchPhase, right: bool) -> InputEvent {
    InputEvent::Touch {
        id,
        phase,
        x: if right { 120.0 } else { 20.0 },
        y: 50.0,
    }
}
fn send(
    engine: &mut TreeUpdateEngine,
    host: &mut HostEventRuntime,
    id: u64,
    phase: TouchPhase,
    right: bool,
) {
    host.handle_input(touch(id, phase, right));
    settle(engine, host);
}
fn text(host: &HostEventRuntime) -> String {
    host.driver.runtime.text_states[&NodeId(9)].content.clone()
}
fn pressed(engine: &TreeUpdateEngine, id: u64) -> bool {
    engine
        .tree()
        .get(&NodeId(id))
        .unwrap()
        .runtime
        .mouse_down_active
}

#[test]
fn overlapping_keys_commit_on_their_own_release_in_either_order() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for styled in [false, true] {
        for count in [2, 4] {
            for reverse in [false, true] {
                let (mut engine, mut host, _) = setup(styled, VirtualKeyHoldMode::None);
                // Queue all contacts before the first pressed-style receipt.
                for id in 1..=count {
                    host.handle_input(touch(id, TouchPhase::Down, id % 2 == 0));
                }
                settle(&mut engine, &mut host);
                assert_eq!(host.driver.runtime.touch_keys.len(), count as usize);
                assert_eq!(text(&host), "");
                if styled {
                    assert!(pressed(&engine, 10) && pressed(&engine, 11));
                }
                let mut releases: Vec<_> = (1..=count).collect();
                if reverse {
                    releases.reverse();
                }
                let expected: String = releases
                    .iter()
                    .map(|id| if id % 2 == 0 { 'b' } else { 'a' })
                    .collect();
                for id in releases {
                    host.handle_input(touch(id, TouchPhase::Up, id % 2 == 0));
                }
                settle(&mut engine, &mut host);
                assert_eq!(text(&host), expected);
                assert!(host.driver.runtime.touch_keys.is_empty());
                assert!(!pressed(&engine, 10) && !pressed(&engine, 11));
            }
        }
    }
}
#[test]
fn same_key_contacts_share_feedback_but_not_taps() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(true, VirtualKeyHoldMode::None);
    send(&mut engine, &mut host, 1, TouchPhase::Down, false);
    send(&mut engine, &mut host, 2, TouchPhase::Down, false);
    send(&mut engine, &mut host, 1, TouchPhase::Up, false);
    assert_eq!(text(&host), "a");
    assert!(pressed(&engine, 10));
    send(&mut engine, &mut host, 2, TouchPhase::Up, false);
    assert_eq!(text(&host), "aa");
    assert!(!pressed(&engine, 10));
}
#[test]
fn slide_off_cancels_only_its_contact_even_if_it_returns_inside() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(true, VirtualKeyHoldMode::Repeat);
    send(&mut engine, &mut host, 1, TouchPhase::Down, false);
    send(&mut engine, &mut host, 2, TouchPhase::Down, true);
    send(&mut engine, &mut host, 1, TouchPhase::Move, true);
    assert!(!pressed(&engine, 10));
    assert!(pressed(&engine, 11));
    send(&mut engine, &mut host, 1, TouchPhase::Up, false);
    send(&mut engine, &mut host, 2, TouchPhase::Up, true);
    assert_eq!(text(&host), "b");
}
#[test]
fn cancel_and_contact_id_reuse_do_not_activate_stale_keys() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(true, VirtualKeyHoldMode::None);
    send(&mut engine, &mut host, 1, TouchPhase::Down, false);
    send(&mut engine, &mut host, 1, TouchPhase::Cancel, false);
    assert_eq!(text(&host), "");
    assert!(!pressed(&engine, 10));
    send(&mut engine, &mut host, 1, TouchPhase::Down, true);
    send(&mut engine, &mut host, 1, TouchPhase::Up, true);
    assert_eq!(text(&host), "b");
}
#[test]
fn hold_timers_are_independent_and_release_does_not_add_a_tap() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for hold in [VirtualKeyHoldMode::Repeat, VirtualKeyHoldMode::Event] {
        let (mut engine, mut host, recorder) = setup(true, hold);
        send(&mut engine, &mut host, 1, TouchPhase::Down, false);
        send(&mut engine, &mut host, 2, TouchPhase::Down, true);
        host.driver.dispatch(|runtime, tx, log| {
            runtime.handle_touch_key_timers(Instant::now() + Duration::from_secs(1), tx, log)
        });
        settle(&mut engine, &mut host);
        assert_eq!(
            text(&host),
            if hold == VirtualKeyHoldMode::Repeat {
                "ab"
            } else {
                ""
            }
        );
        send(&mut engine, &mut host, 1, TouchPhase::Up, false);
        send(&mut engine, &mut host, 2, TouchPhase::Up, true);
        assert_eq!(
            text(&host),
            if hold == VirtualKeyHoldMode::Repeat {
                "ab"
            } else {
                ""
            }
        );
        if hold == VirtualKeyHoldMode::Event {
            assert_eq!(
                recorder
                    .0
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|kind| **kind == ElementEventKind::VirtualKeyHold)
                    .count(),
                2
            );
        }
        assert!(host.driver.runtime.touch_key_deadline().is_none());
    }
}
#[test]
fn window_blur_cancels_all_touch_keys_and_feedback() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(true, VirtualKeyHoldMode::Repeat);
    send(&mut engine, &mut host, 1, TouchPhase::Down, false);
    send(&mut engine, &mut host, 2, TouchPhase::Down, true);
    host.handle_input(InputEvent::Focused { focused: false });
    settle(&mut engine, &mut host);
    send(&mut engine, &mut host, 1, TouchPhase::Up, false);
    send(&mut engine, &mut host, 2, TouchPhase::Up, true);
    assert_eq!(text(&host), "");
    assert!(host.driver.runtime.touch_keys.is_empty());
    assert!(!pressed(&engine, 10) && !pressed(&engine, 11));
}

#[test]
fn primary_touch_cancellation_does_not_cancel_an_independent_key() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(true, VirtualKeyHoldMode::None);
    host.handle_input(InputEvent::Touch {
        id: 100,
        phase: TouchPhase::Down,
        x: 20.0,
        y: 10.0,
    });
    settle(&mut engine, &mut host);
    send(&mut engine, &mut host, 1, TouchPhase::Down, false);
    host.handle_input(InputEvent::Touch {
        id: 100,
        phase: TouchPhase::Cancel,
        x: 20.0,
        y: 10.0,
    });
    settle(&mut engine, &mut host);
    assert_eq!(host.driver.runtime.touch_keys.len(), 1);
    assert!(pressed(&engine, 10));
    send(&mut engine, &mut host, 1, TouchPhase::Up, false);
    assert_eq!(text(&host), "a");
}

#[test]
fn buffered_release_cannot_type_into_a_replacement_keyboard_or_field() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(true, VirtualKeyHoldMode::None);
    let replacement = crate::tree::serialize::encode_tree(engine.tree());
    send(&mut engine, &mut host, 1, TouchPhase::Down, false);
    send(&mut engine, &mut host, 2, TouchPhase::Down, true);
    host.handle_input(touch(1, TouchPhase::Up, false));
    assert!(host.driver.awaiting_registry.is_some());
    host.handle_input(touch(2, TouchPhase::Up, true));
    assert!(!host.driver.runtime.listener_lane.buffered_inputs.is_empty());
    let messages = host
        .drain_tree_messages()
        .into_iter()
        .chain([TreeMsg::UploadTree {
            bytes: replacement,
            submitted_at: None,
        }])
        .collect();
    host.install_rebuild(response(
        &mut engine,
        messages,
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    settle(&mut engine, &mut host);
    assert!(host.driver.runtime.touch_keys.is_empty());
    assert_eq!(text(&host), "");
    assert!(!pressed(&engine, 10) && !pressed(&engine, 11));
}

#[test]
fn disabling_input_clears_contact_lifetimes_without_replaying_taps_after_resume() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Gated(Arc<AtomicBool>);
    impl crate::backend::wake::BackendWake for Gated {
        fn input_enabled(&self) -> bool {
            self.0.load(Ordering::Relaxed)
        }
        fn request_stop(&self) {}
        fn request_redraw(&self) {}
        fn notify_video_frame(&self) {}
    }
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(true, VirtualKeyHoldMode::Repeat);
    let enabled = Arc::new(AtomicBool::new(true));
    host.driver.runtime.backend_wake = BackendWakeHandle::new(Gated(enabled.clone()));
    send(&mut engine, &mut host, 1, TouchPhase::Down, false);
    send(&mut engine, &mut host, 2, TouchPhase::Down, true);
    enabled.store(false, Ordering::Relaxed);
    host.driver
        .dispatch(|runtime, tx, log| runtime.handle_timers(tx, log));
    settle(&mut engine, &mut host);
    assert!(host.driver.runtime.touch_keys.is_empty());
    assert!(!pressed(&engine, 10) && !pressed(&engine, 11));
    enabled.store(true, Ordering::Relaxed);
    send(&mut engine, &mut host, 1, TouchPhase::Up, false);
    send(&mut engine, &mut host, 2, TouchPhase::Up, true);
    assert_eq!(text(&host), "");
}

#[test]
fn mouse_and_touch_on_the_same_key_keep_independent_capture_and_feedback() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(true, VirtualKeyHoldMode::None);
    let mouse = |action| InputEvent::CursorButton {
        button: "left".into(),
        action,
        mods: 0,
        x: 20.0,
        y: 50.0,
    };
    host.handle_input(mouse(ACTION_PRESS));
    settle(&mut engine, &mut host);
    assert!(host.driver.runtime.runtime_overlay.virtual_key.is_some());
    send(&mut engine, &mut host, 1, TouchPhase::Down, false);
    assert!(host.driver.runtime.runtime_overlay.virtual_key.is_some());
    host.handle_input(mouse(ACTION_RELEASE));
    settle(&mut engine, &mut host);
    assert_eq!(text(&host), "a");
    assert!(pressed(&engine, 10));
    send(&mut engine, &mut host, 1, TouchPhase::Up, false);
    assert_eq!(text(&host), "aa");
    assert!(!pressed(&engine, 10));
}

#[test]
fn stale_touch_fifo_preserves_overlapping_release_order_and_observers_are_not_replayed() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, recorder) = setup(true, VirtualKeyHoldMode::None);
    host.set_input_mask(crate::input::INPUT_MASK_TOUCH);
    host.driver.runtime.listener_lane.mark_stale();
    let events: Vec<_> = [
        (1, TouchPhase::Down, false),
        (2, TouchPhase::Down, true),
        (1, TouchPhase::Move, false),
        (2, TouchPhase::Up, true),
        (1, TouchPhase::Up, false),
    ]
    .into_iter()
    .map(|(id, phase, second)| InputEvent::Touch {
        id,
        phase,
        x: if second { 110.0 } else { 20.0 },
        y: 50.0,
    })
    .collect();
    for event in &events {
        host.handle_input(event.clone());
    }
    assert_eq!(
        host.driver.runtime.listener_lane.buffered_inputs.len(),
        events.len()
    );
    assert_eq!(text(&host), "");
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    settle(&mut engine, &mut host);
    assert_eq!(text(&host), "ba");
    assert_eq!(*recorder.1.lock().unwrap(), events);
    assert!(host.driver.runtime.touch_keys.is_empty());
    assert!(host.driver.runtime.listener_lane.buffered_inputs.is_empty());
}
