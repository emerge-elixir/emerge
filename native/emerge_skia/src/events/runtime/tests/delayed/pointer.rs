use super::*;

#[derive(Default)]
pub(super) struct Recorder {
    pub(super) events: Mutex<Vec<(NodeId, ElementEventKind)>>,
    pub(super) payloads: Mutex<
        Vec<(
            NodeId,
            ElementEventKind,
            Option<registry_builder::ElixirEventPayload>,
        )>,
    >,
    pub(super) raw: Mutex<Vec<InputEvent>>,
}
impl HostEventSink for Recorder {
    fn send_raw_input(&self, event: &InputEvent) {
        self.raw.lock().unwrap().push(event.clone());
    }
    fn send_element_event(
        &self,
        id: &NodeId,
        kind: ElementEventKind,
        payload: Option<&registry_builder::ElixirEventPayload>,
    ) {
        self.events.lock().unwrap().push((*id, kind));
        self.payloads
            .lock()
            .unwrap()
            .push((*id, kind, payload.cloned()));
    }
}
fn recorded_host() -> (HostEventRuntime, Arc<Recorder>) {
    let recorder = Arc::new(Recorder::default());
    let mut host = HostEventRuntime::new(false, 40.0, false, recorder.clone(), None);
    host.set_input_mask(u32::MAX);
    (host, recorder)
}
pub(super) fn button(action: u8, x: f32, y: f32) -> InputEvent {
    InputEvent::CursorButton {
        button: "left".into(),
        action,
        mods: 0,
        x,
        y,
    }
}
fn encoded_node(id: NodeId, kind: ElementKind, flags: &[u8], width: f64, height: f64) -> Element {
    let raw: Vec<u8> = ((2 + flags.len()) as u16)
        .to_be_bytes()
        .into_iter()
        .chain([1, 2])
        .chain(width.to_be_bytes())
        .chain([2, 2])
        .chain(height.to_be_bytes())
        .chain(flags.iter().flat_map(|tag| [*tag, 1]))
        .collect();
    let attrs = crate::tree::attrs::decode_attrs(&raw).unwrap();
    Element::with_attrs(id, kind, raw, attrs)
}
fn encoded_tree(kind: ElementKind, flags: &[u8]) -> ElementTree {
    let mut tree = ElementTree::new();
    tree.set_revision(1);
    tree.insert(encoded_node(NodeId(9), kind, flags, 200.0, 30.0));
    tree.set_root_id(NodeId(9));
    tree.stamp_all_mounted_at_revision(1);
    tree
}

fn upload(tree: &ElementTree) -> TreeMsg {
    TreeMsg::UploadTree {
        bytes: crate::tree::serialize::encode_tree(tree),
        submitted_at: None,
    }
}
fn settle(
    host: &mut HostEventRuntime,
    engine: &mut TreeUpdateEngine,
    policy: TreeUpdateDecodePolicy,
) {
    for _ in 0..32 {
        let messages = host.drain_tree_messages();
        if messages.is_empty() {
            return;
        }
        match engine
            .process_messages(messages, TreeUpdateOptions::new(None, policy))
            .unwrap()
        {
            TreeUpdateEffect::RegistryUpdate { rebuild } => host.install_rebuild(rebuild),
            TreeUpdateEffect::Layout { output, .. } => {
                if output.event_rebuild_changed {
                    host.install_rebuild(output.event_rebuild);
                }
            }
            TreeUpdateEffect::Skip => assert!(host.driver.awaiting_registry.is_none()),
            TreeUpdateEffect::Stop => panic!("unexpected stop"),
        }
    }
    panic!("pointer feedback did not settle");
}

#[test]
fn buffered_release_cannot_click_a_replacement_with_the_same_numeric_id() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for policy in [
        TreeUpdateDecodePolicy::ReturnErr,
        TreeUpdateDecodePolicy::LogAndContinue,
    ] {
        let tree = encoded_tree(ElementKind::El, &[40, 41]);
        let mut engine = TreeUpdateEngine::new(tree.clone(), 200, 30);
        let (mut host, recorder) = recorded_host();
        let initial = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
        host.install_rebuild(initial.clone());
        host.handle_input(button(ACTION_PRESS, 10.0, 10.0));
        assert!(host.driver.runtime.runtime_overlay.click_press.is_some());
        assert!(host.driver.awaiting_registry.is_some());
        host.handle_input(button(ACTION_RELEASE, 10.0, 10.0));
        host.install_rebuild(initial);
        assert_eq!(host.driver.runtime.listener_lane.buffered_inputs.len(), 1);
        let messages = host
            .drain_tree_messages()
            .into_iter()
            .chain([upload(&tree)])
            .collect();
        host.install_rebuild(response(&mut engine, messages, policy));
        settle(&mut host, &mut engine, policy);
        assert!(
            !recorder
                .events
                .lock()
                .unwrap()
                .iter()
                .any(|(_, kind)| *kind == ElementEventKind::Click),
            "a release must not activate the new mount"
        );
        assert!(host.driver.runtime.runtime_overlay.click_press.is_none());
        assert_eq!(
            recorder.raw.lock().unwrap().len(),
            2,
            "replay must not forward raw input twice"
        );
    }
}

#[test]
fn text_drag_does_not_transfer_to_a_replacement_during_a_causal_wait() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
    let (mut host, _) = recorded_host();
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        policy,
    ));
    host.handle_input(button(ACTION_PRESS, 0.0, 10.0));
    assert!(host.driver.runtime.runtime_overlay.text_drag.is_some());
    host.handle_input(InputEvent::TextCommit {
        text: "X".into(),
        mods: 0,
    });
    assert!(host.driver.awaiting_registry.is_some());
    host.handle_input(InputEvent::CursorPos { x: 100.0, y: 10.0 });
    let replacement = encoded_tree(ElementKind::TextInput, &[56]);
    let messages = host
        .drain_tree_messages()
        .into_iter()
        .chain([upload(&replacement)])
        .collect();
    host.install_rebuild(response(&mut engine, messages, policy));
    assert!(
        host.driver.runtime.runtime_overlay.text_drag.is_none(),
        "the old anchor cannot select the new mount"
    );
    settle(&mut host, &mut engine, policy);
    let state = &host.driver.runtime.text_states[&NodeId(9)];
    assert_eq!(state.content, "");
    assert!(state.selection_anchor.is_none());
    assert!(!state.focused);
}

fn animate_move(tree: &mut ElementTree, positions: &[f64]) {
    tree.get_mut(&NodeId(9)).unwrap().spec.declared.animate = Some(AnimationSpec {
        keyframes: positions
            .iter()
            .map(|x| Attrs {
                move_x: Some(*x),
                ..Default::default()
            })
            .collect(),
        duration_ms: 1000.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Once,
    });
}
fn pulse(start: Instant, ms: u64) -> TreeMsg {
    TreeMsg::AnimationPulse {
        presented_at: start + Duration::from_millis(ms),
        predicted_next_present_at: start + Duration::from_millis(ms),
        trace: None,
    }
}
fn resize() -> InputEvent {
    InputEvent::Resized {
        width: 200,
        height: 30,
        scale_factor: 1.0,
    }
}

#[test]
fn stationary_hover_recovers_current_geometry_without_replaying_obsolete_crossings() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    for final_inside in [false, true] {
        let mut tree = encoded_tree(ElementKind::El, &[43, 44]);
        // Shift a 200px target past a fixed pointer, then back.
        animate_move(&mut tree, &[100.0, 0.0, 100.0]);
        let mut engine = TreeUpdateEngine::new(tree, 200, 30);
        let start = Instant::now();
        let (mut host, recorder) = recorded_host();
        host.install_rebuild(response(&mut engine, vec![pulse(start, 0)], policy));
        host.handle_input(InputEvent::CursorEntered { entered: true });
        host.handle_input(InputEvent::CursorPos { x: 10.0, y: 10.0 });
        assert!(recorder.events.lock().unwrap().is_empty());
        let crossing = response(&mut engine, vec![pulse(start, 500)], policy);
        let last = if final_inside {
            crossing.clone()
        } else {
            response(&mut engine, vec![pulse(start, 1000)], policy)
        };
        host.handle_input(resize());
        assert!(host.driver.awaiting_registry.is_some());
        host.install_rebuild(crossing);
        host.install_rebuild(last);
        assert!(
            recorder.events.lock().unwrap().is_empty(),
            "obsolete geometry has no callback authority"
        );
        settle(&mut host, &mut engine, policy);
        assert_eq!(
            recorder.events.lock().unwrap().as_slice(),
            if final_inside {
                &[(NodeId(9), ElementEventKind::MouseEnter)][..]
            } else {
                &[]
            }
        );
        if final_inside {
            host.install_rebuild(response(&mut engine, vec![pulse(start, 1000)], policy));
            settle(&mut host, &mut engine, policy);
            assert_eq!(
                *recorder.events.lock().unwrap(),
                vec![
                    (NodeId(9), ElementEventKind::MouseEnter),
                    (NodeId(9), ElementEventKind::MouseLeave)
                ]
            );
        }
        assert_eq!(recorder.raw.lock().unwrap().len(), 3);
    }
}

#[test]
fn stationary_hover_enters_a_new_mount_instead_of_inheriting_the_old_hover_stack() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let tree = encoded_tree(ElementKind::El, &[43, 44]);
    let mut engine = TreeUpdateEngine::new(tree.clone(), 200, 30);
    let (mut host, recorder) = recorded_host();
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        policy,
    ));
    host.handle_input(InputEvent::CursorEntered { entered: true });
    host.handle_input(InputEvent::CursorPos { x: 10.0, y: 10.0 });
    settle(&mut host, &mut engine, policy);
    assert_eq!(recorder.events.lock().unwrap().len(), 1);
    host.handle_input(resize());
    let messages = host
        .drain_tree_messages()
        .into_iter()
        .chain([upload(&tree)])
        .collect();
    host.install_rebuild(response(&mut engine, messages, policy));
    settle(&mut host, &mut engine, policy);
    assert_eq!(
        *recorder.events.lock().unwrap(),
        vec![(NodeId(9), ElementEventKind::MouseEnter); 2]
    );
}

#[test]
fn same_mount_release_hit_testing_uses_the_accepted_animated_geometry() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    for release_x in [10.0, 60.0] {
        let mut tree = encoded_tree(ElementKind::El, &[40, 41]);
        animate_move(&mut tree, &[0.0, 100.0]);
        let mut engine = TreeUpdateEngine::new(tree, 200, 30);
        let start = Instant::now();
        let (mut host, recorder) = recorded_host();
        host.install_rebuild(response(&mut engine, vec![pulse(start, 0)], policy));
        host.handle_input(button(ACTION_PRESS, 10.0, 10.0));
        let old = response(&mut engine, vec![pulse(start, 500)], policy);
        host.handle_input(InputEvent::CursorPos {
            x: release_x,
            y: 10.0,
        });
        host.handle_input(button(ACTION_RELEASE, release_x, 10.0));
        host.install_rebuild(old);
        assert!(host.driver.runtime.runtime_overlay.click_press.is_some());
        settle(&mut host, &mut engine, policy);
        let clicks = recorder
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, kind)| *kind == ElementEventKind::Click)
            .count();
        assert_eq!(clicks, usize::from(release_x == 60.0));
        if release_x == 60.0 {
            let payloads = recorder.payloads.lock().unwrap();
            let (_, _, payload) = payloads
                .iter()
                .find(|(_, kind, _)| *kind == ElementEventKind::Click)
                .unwrap();
            assert_eq!(
                *payload,
                Some(registry_builder::ElixirEventPayload::Position(
                    crate::events::PointerPosition { x: 10.0, y: 10.0 }
                ))
            );
        }
        assert!(!host.driver.runtime.has_active_pointer_overlay());
        assert_eq!(recorder.raw.lock().unwrap().len(), 3);
    }
}

#[test]
fn buffered_text_selection_uses_current_transform_and_preserves_release_order() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut tree = editable_tree(false);
    animate_move(&mut tree, &[0.0, 100.0]);
    let mut engine = TreeUpdateEngine::new(tree, 200, 30);
    let start = Instant::now();
    let (mut host, recorder) = recorded_host();
    host.install_rebuild(response(&mut engine, vec![pulse(start, 0)], policy));
    host.handle_input(button(ACTION_PRESS, 0.0, 10.0));
    host.handle_input(InputEvent::TextCommit {
        text: "X".into(),
        mods: 0,
    });
    let old = response(&mut engine, vec![pulse(start, 500)], policy);
    host.handle_input(InputEvent::CursorPos { x: 150.0, y: 10.0 });
    host.handle_input(InputEvent::CursorPos { x: 51.0, y: 10.0 });
    host.handle_input(button(ACTION_RELEASE, 51.0, 10.0));
    host.install_rebuild(old);
    settle(&mut host, &mut engine, policy);
    assert_eq!(
        host.focused_text_state().unwrap().cursor,
        0,
        "51px is near the start after the native 50px translation"
    );
    assert!(host.driver.runtime.runtime_overlay.text_drag.is_none());
    assert_eq!(recorder.raw.lock().unwrap().len(), 5);
}

#[test]
fn scroll_capture_and_thumb_capture_do_not_survive_remount_but_same_mount_release_settles() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for thumb in [false, true] {
        for remount in [false, true] {
            let policy = TreeUpdateDecodePolicy::ReturnErr;
            let mut tree = encoded_tree(ElementKind::Column, &[7]);
            tree.insert(encoded_node(NodeId(10), ElementKind::El, &[], 200.0, 200.0));
            tree.set_children(&NodeId(9), vec![NodeId(10)]).unwrap();
            tree.stamp_all_mounted_at_revision(1);
            let mut engine = TreeUpdateEngine::new(tree.clone(), 200, 30);
            let (mut host, recorder) = recorded_host();
            let initial = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
            let node = initial.scrollbars[&(NodeId(9), ScrollbarAxis::Y)];
            let (x, y) = if thumb {
                (
                    node.thumb_rect.x + node.thumb_rect.width / 2.0,
                    node.thumb_rect.y + node.thumb_rect.height / 2.0,
                )
            } else {
                (10.0, 25.0)
            };
            host.install_rebuild(initial.clone());
            host.handle_input(button(ACTION_PRESS, x, y));
            // Thumb press itself may request native focus/hover reconciliation.
            settle(&mut host, &mut engine, policy);
            host.handle_input(InputEvent::CursorPos {
                x,
                y: if thumb { y + 5.0 } else { 0.0 },
            });
            if !thumb {
                host.handle_input(InputEvent::CursorPos { x, y: -10.0 });
            }
            assert!(
                host.driver.awaiting_registry.is_some(),
                "thumb={thumb} remount={remount} node={node:?} overlay={:?}",
                host.driver.runtime.runtime_overlay
            );
            host.handle_input(InputEvent::CursorPos {
                x,
                y: if thumb { y + 10.0 } else { -25.0 },
            });
            host.install_rebuild(initial);
            let messages = host
                .drain_tree_messages()
                .into_iter()
                .chain(remount.then(|| upload(&tree)))
                .collect();
            host.install_rebuild(response(&mut engine, messages, policy));
            if remount {
                assert!(!host.driver.runtime.has_active_pointer_overlay());
            }
            host.handle_input(button(
                ACTION_RELEASE,
                x,
                if thumb { y + 10.0 } else { -25.0 },
            ));
            settle(&mut host, &mut engine, policy);
            assert!(!host.driver.runtime.has_active_pointer_overlay());
            if remount {
                assert!(host.driver.runtime.inertial_scroll.is_none());
                assert_eq!(engine.tree().get(&NodeId(9)).unwrap().layout.scroll_y, 0.0);
            } else {
                assert!(engine.tree().get(&NodeId(9)).unwrap().layout.scroll_y > 0.0);
            }
            assert_eq!(
                recorder.raw.lock().unwrap().len(),
                if thumb { 4 } else { 5 }
            );
        }
    }
}

#[test]
fn real_event_actor_drops_remounted_click_capture_but_preserves_same_mount_release() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for remount in [false, true] {
        let tree = encoded_tree(ElementKind::El, &[40, 41]);
        let mut engine = TreeUpdateEngine::new(tree.clone(), 200, 30);
        let initial = response(
            &mut engine,
            vec![TreeMsg::RebuildRegistry],
            TreeUpdateDecodePolicy::ReturnErr,
        );
        let (event_tx, event_rx) = bounded(16);
        let (tree_tx, tree_rx) = bounded(64);
        for msg in [
            EventMsg::registry_update(initial.clone()),
            EventMsg::InputEvent(button(ACTION_PRESS, 10.0, 10.0)),
            EventMsg::registry_update(initial),
            EventMsg::InputEvent(button(ACTION_RELEASE, 10.0, 10.0)),
        ] {
            event_tx.send(msg).unwrap();
        }
        let handle = spawn_event_actor(SpawnEventActorConfig {
            event_rx,
            tree_tx,
            backend_cursor_tx: None,
            backend_wake: BackendWakeHandle::noop(),
            scroll_line_pixels: 40.0,
            log_render: false,
            native_log: Arc::new(NativeLogRelay::default()),
            system_clipboard: false,
            stats: None,
            asset_context: assets.context(),
        });
        let mut messages = Vec::new();
        loop {
            let msg = tree_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let fence = msg
                .commands()
                .iter()
                .any(|msg| matches!(msg, TreeMsg::ListenerBarrier(_)));
            push_tree_msg_flat(msg, &mut messages);
            if fence {
                break;
            }
        }
        let messages = messages
            .into_iter()
            .chain(remount.then(|| upload(&tree)))
            .collect();
        let rebuild = response(&mut engine, messages, TreeUpdateDecodePolicy::ReturnErr);
        event_tx.send(EventMsg::registry_update(rebuild)).unwrap();
        event_tx.send(EventMsg::Stop).unwrap();
        handle.join().unwrap();
        let outgoing = drain_msgs(&tree_rx);
        assert_eq!(
            outgoing
                .iter()
                .filter(|msg| matches!(msg, TreeMsg::ListenerBarrier(_)))
                .count(),
            usize::from(!remount),
            "only a legitimate click requests another native response"
        );
    }
}

#[test]
fn slider_drag_and_pending_value_belong_to_the_original_mount() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for remount in [false, true] {
        let policy = TreeUpdateDecodePolicy::ReturnErr;
        let mut tree = encoded_tree(ElementKind::Slider, &[56]);
        tree.insert(encoded_node(NodeId(10), ElementKind::El, &[], 200.0, 10.0));
        tree.insert(encoded_node(NodeId(11), ElementKind::El, &[], 20.0, 20.0));
        tree.set_children(&NodeId(9), vec![NodeId(10), NodeId(11)])
            .unwrap();
        tree.stamp_all_mounted_at_revision(1);
        let mut engine = TreeUpdateEngine::new(tree.clone(), 200, 30);
        let (mut host, recorder) = recorded_host();
        let initial = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
        host.install_rebuild(initial.clone());
        host.handle_input(button(ACTION_PRESS, 50.0, 15.0));
        assert!(host.driver.runtime.runtime_overlay.slider_drag.is_some());
        assert!(host.driver.awaiting_registry.is_some());
        host.handle_input(InputEvent::CursorPos { x: 150.0, y: 15.0 });
        host.install_rebuild(initial);
        let messages = host
            .drain_tree_messages()
            .into_iter()
            .chain(remount.then(|| upload(&tree)))
            .collect();
        host.install_rebuild(response(&mut engine, messages, policy));
        host.handle_input(button(ACTION_RELEASE, 150.0, 15.0));
        settle(&mut host, &mut engine, policy);
        assert!(host.driver.runtime.runtime_overlay.slider_drag.is_none());
        let value = host.driver.runtime.slider_states[&NodeId(9)].value;
        assert_eq!(value, if remount { 0.0 } else { 0.75 });
        assert!(host.driver.runtime.pending_slider_patches.is_empty() || !remount);
        assert_eq!(recorder.raw.lock().unwrap().len(), 3);
    }
}

mod inflight;
mod scroll;
mod styled;

#[test]
fn window_leave_uses_listener_order_position_not_a_later_ingress_sample() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut engine = TreeUpdateEngine::new(encoded_tree(ElementKind::El, &[43, 44]), 200, 30);
    let (mut host, recorder) = recorded_host();
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        policy,
    ));
    host.handle_input(InputEvent::CursorPos { x: 71.0, y: 10.25 });
    settle(&mut host, &mut engine, policy);
    host.handle_input(resize());
    assert!(host.driver.awaiting_registry.is_some());
    host.handle_input(InputEvent::CursorEntered { entered: false });
    host.handle_input(InputEvent::CursorPos { x: 180.0, y: 15.0 });
    settle(&mut host, &mut engine, policy);
    let payloads = recorder.payloads.lock().unwrap();
    let (_, _, payload) = payloads
        .iter()
        .find(|(_, kind, _)| *kind == ElementEventKind::MouseLeave)
        .unwrap();
    assert_eq!(
        *payload,
        Some(registry_builder::ElixirEventPayload::Position(
            crate::events::PointerPosition { x: 71.0, y: 10.25 }
        ))
    );
    assert_eq!(recorder.raw.lock().unwrap().len(), 4);
}

#[test]
fn buffered_mouse_moves_deliver_latest_position_before_release_without_synthetic_duplicates() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut engine = TreeUpdateEngine::new(encoded_tree(ElementKind::El, &[41, 42, 45]), 200, 30);
    let (mut host, recorder) = recorded_host();
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        policy,
    ));
    host.handle_input(button(ACTION_PRESS, 20.0, 10.0));
    assert!(host.driver.awaiting_registry.is_some());
    host.handle_input(InputEvent::CursorPos { x: 50.0, y: 11.0 });
    host.handle_input(InputEvent::CursorPos { x: 71.5, y: 12.25 });
    host.handle_input(button(ACTION_RELEASE, 71.5, 12.25));
    settle(&mut host, &mut engine, policy);

    use crate::events::PointerPosition;
    use registry_builder::ElixirEventPayload::Position;
    assert_eq!(
        *recorder.payloads.lock().unwrap(),
        vec![
            (
                NodeId(9),
                ElementEventKind::MouseDown,
                Some(Position(PointerPosition { x: 20.0, y: 10.0 }))
            ),
            (
                NodeId(9),
                ElementEventKind::MouseMove,
                Some(Position(PointerPosition { x: 71.5, y: 12.25 }))
            ),
            (
                NodeId(9),
                ElementEventKind::MouseUp,
                Some(Position(PointerPosition { x: 71.5, y: 12.25 }))
            ),
        ],
        "real coalesced movement must be delivered; stationary revalidation must not duplicate it"
    );
    assert_eq!(
        recorder.raw.lock().unwrap().len(),
        4,
        "raw input is not forwarded again on replay"
    );
}

fn drag_samples(recorder: &Recorder) -> Vec<crate::events::DragPayload> {
    recorder
        .payloads
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, _, payload)| match payload {
            Some(registry_builder::ElixirEventPayload::Drag(value)) => Some(*value),
            _ => None,
        })
        .collect()
}

#[test]
fn captured_drag_replays_real_moves_once_and_ends_outside_the_source() {
    use crate::events::DragPhase;
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    for scale in [1.0, 1.6, 2.0] {
        let mut tree = encoded_tree(ElementKind::El, &[88]);
        tree.get_mut(&NodeId(9)).unwrap().spec.declared.layout_scale = Some(scale);
        let mut engine = TreeUpdateEngine::new(tree, 800, 300);
        let (mut host, recorder) = recorded_host();
        host.install_rebuild(response(
            &mut engine,
            vec![TreeMsg::RebuildRegistry],
            policy,
        ));
        let scale = scale as f32;
        host.handle_input(button(ACTION_PRESS, 35.5 * scale, 8.25 * scale));
        assert!(
            host.driver
                .runtime
                .runtime_overlay
                .pointer_capture
                .is_some()
        );
        host.handle_input(InputEvent::CursorPos {
            x: 60.0 * scale,
            y: 50.0 * scale,
        });
        host.handle_input(InputEvent::CursorPos {
            x: 95.5 * scale,
            y: -12.25 * scale,
        });
        host.handle_input(button(ACTION_RELEASE, 201.5 * scale, 80.25 * scale));
        settle(&mut host, &mut engine, policy);
        let samples = drag_samples(&recorder);
        assert_eq!(
            samples.len(),
            3,
            "no stationary or duplicate replay samples: {samples:?}"
        );
        for (sample, (phase, x, y)) in samples.iter().zip([
            (DragPhase::Start, 35.5, 8.25),
            (DragPhase::Move, 95.5, -12.25),
            (DragPhase::End, 201.5, 80.25),
        ]) {
            assert_eq!(sample.phase, phase);
            assert!((sample.x - x).abs() < 0.0001, "{sample:?}");
            assert!((sample.y - y).abs() < 0.0001, "{sample:?}");
        }
        assert!(
            host.driver
                .runtime
                .runtime_overlay
                .pointer_capture
                .is_none()
        );
        host.handle_input(InputEvent::CursorPos { x: 40.0, y: 10.0 });
        settle(&mut host, &mut engine, policy);
        assert_eq!(
            drag_samples(&recorder).len(),
            3,
            "hover must not resume dragging"
        );
        assert_eq!(recorder.raw.lock().unwrap().len(), 5);
    }
}

#[test]
fn captured_drag_cancel_uses_the_ordered_last_position_and_preserves_hover_leave() {
    use crate::events::DragPhase;
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    for cancel in [
        InputEvent::Focused { focused: false },
        InputEvent::CursorEntered { entered: false },
    ] {
        let mut engine =
            TreeUpdateEngine::new(encoded_tree(ElementKind::El, &[88, 43, 44]), 500, 300);
        let (mut host, recorder) = recorded_host();
        host.install_rebuild(response(
            &mut engine,
            vec![TreeMsg::RebuildRegistry],
            policy,
        ));
        host.handle_input(InputEvent::CursorPos { x: 35.5, y: 8.25 });
        settle(&mut host, &mut engine, policy);
        host.handle_input(button(ACTION_PRESS, 35.5, 8.25));
        host.handle_input(cancel.clone());
        host.handle_input(InputEvent::CursorPos { x: 400.0, y: 250.0 });
        settle(&mut host, &mut engine, policy);
        let samples = drag_samples(&recorder);
        assert_eq!(samples.len(), 2, "{cancel:?}: {samples:?}");
        assert_eq!(
            samples[1],
            crate::events::DragPayload {
                phase: DragPhase::Cancel,
                x: 35.5,
                y: 8.25
            }
        );
        assert!(
            host.driver
                .runtime
                .runtime_overlay
                .pointer_capture
                .is_none()
        );
        if matches!(cancel, InputEvent::CursorEntered { .. }) {
            assert!(
                recorder
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(_, kind)| *kind == ElementEventKind::MouseLeave)
            );
        }
    }
}

#[test]
fn captured_drag_does_not_transfer_to_a_replacement_mount_or_removed_handler() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    for remount in [false, true] {
        let mut tree = encoded_tree(ElementKind::El, &[88, 41]);
        let mut engine = TreeUpdateEngine::new(tree.clone(), 500, 300);
        let (mut host, recorder) = recorded_host();
        host.install_rebuild(response(
            &mut engine,
            vec![TreeMsg::RebuildRegistry],
            policy,
        ));
        host.handle_input(button(ACTION_PRESS, 35.5, 8.25));
        settle(&mut host, &mut engine, policy);
        assert!(
            host.driver
                .runtime
                .runtime_overlay
                .pointer_capture
                .is_some()
        );
        if remount {
            host.install_rebuild(response(&mut engine, vec![upload(&tree)], policy));
        } else {
            tree.set_revision(2);
            tree.get_mut(&NodeId(9)).unwrap().spec.declared.on_drag = None;
            host.install_rebuild(native_rebuild(&mut tree));
        }
        assert!(
            host.driver
                .runtime
                .runtime_overlay
                .pointer_capture
                .is_none()
        );
        host.handle_input(button(ACTION_RELEASE, 250.0, 100.0));
        settle(&mut host, &mut engine, policy);
        assert_eq!(drag_samples(&recorder).len(), 1);
    }
}

#[test]
fn stale_hover_snapshots_do_not_revive_a_left_probe_between_sibling_boxes() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut tree = ElementTree::new();
    for (id, kind, flags, width, height) in [
        (1, ElementKind::Row, vec![], 500.0, 100.0),
        (9, ElementKind::El, vec![43, 44, 45], 100.0, 60.0),
        (2, ElementKind::El, vec![], 50.0, 60.0),
        (10, ElementKind::El, vec![43, 44, 45], 100.0, 60.0),
    ] {
        tree.insert(encoded_node(NodeId(id), kind, &flags, width, height));
    }
    tree.set_children(&NodeId(1), vec![NodeId(9), NodeId(2), NodeId(10)])
        .unwrap();
    tree.set_root_id(NodeId(1));
    tree.set_revision(1);
    tree.stamp_all_mounted_at_revision(1);
    let mut engine = TreeUpdateEngine::new(tree, 500, 100);
    let (mut host, recorder) = recorded_host();
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        policy,
    ));
    host.handle_input(InputEvent::CursorPos { x: 10.0, y: 10.0 });
    settle(&mut host, &mut engine, policy);
    assert!(
        engine
            .tree()
            .get(&NodeId(9))
            .unwrap()
            .runtime
            .mouse_over_active
    );
    // A legitimate registry published while A is hovered. Paint-only reuse or
    // an already queued update can republish it after the event runtime left A.
    let hovered_a = registry_builder::build_registry_rebuild(engine.tree());
    host.install_rebuild(hovered_a.clone());
    let leaves = |id| {
        recorder
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(source, kind)| *source == id && *kind == ElementEventKind::MouseLeave)
            .count()
    };
    for (x, y, owner) in [
        (300.0, 80.0, None),
        (320.0, 85.0, None),
        (170.0, 20.0, Some(NodeId(10))),
        (180.0, 25.0, Some(NodeId(10))),
        (330.0, 85.0, None),
        (350.0, 90.0, None),
    ] {
        host.handle_input(InputEvent::CursorPos { x, y });
        settle(&mut host, &mut engine, policy);
        let before = recorder.events.lock().unwrap().len();
        host.install_rebuild(hovered_a.clone());
        settle(&mut host, &mut engine, policy);
        assert_eq!(
            recorder.events.lock().unwrap().len(),
            before,
            "a cached registry must not synthesize another leave at ({x}, {y})"
        );
        assert_eq!(
            host.driver
                .runtime
                .hover_stack
                .last()
                .map(|tracker| tracker.element_id),
            owner
        );
        assert_eq!(
            leaves(NodeId(9)),
            1,
            "A must stay left even when B is left later"
        );
    }
    assert_eq!(leaves(NodeId(10)), 1);
}

#[test]
fn cached_hover_flags_cannot_reactivate_a_probe_after_window_leave() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut engine = TreeUpdateEngine::new(encoded_tree(ElementKind::El, &[43, 44, 45]), 500, 100);
    let (mut host, recorder) = recorded_host();
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        policy,
    ));
    host.handle_input(InputEvent::CursorPos { x: 10.0, y: 10.0 });
    settle(&mut host, &mut engine, policy);
    let hovered = registry_builder::build_registry_rebuild(engine.tree());
    host.handle_input(InputEvent::CursorEntered { entered: false });
    // Install the snapshot before the queued native hover-clear reaches the
    // tree, modeling an application render racing immediate Elixir delivery.
    host.install_rebuild(hovered.clone());
    assert!(host.driver.runtime.hover_stack.is_empty());
    settle(&mut host, &mut engine, policy);
    host.handle_input(InputEvent::CursorEntered { entered: true });
    host.handle_input(InputEvent::CursorPos { x: 300.0, y: 80.0 });
    host.install_rebuild(hovered);
    settle(&mut host, &mut engine, policy);
    assert!(host.driver.runtime.hover_stack.is_empty());
    let events = recorder.events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|(_, kind)| *kind == ElementEventKind::MouseLeave)
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|(_, kind)| *kind == ElementEventKind::MouseEnter)
            .count(),
        1
    );
    assert_eq!(recorder.raw.lock().unwrap().len(), 4);
}
