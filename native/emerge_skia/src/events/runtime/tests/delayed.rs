use super::*;

fn editable_tree(mount_focus: bool) -> ElementTree {
    let mut tree = ElementTree::new();
    tree.set_revision(1);
    let mut input = Element::with_attrs(
        NodeId(9),
        ElementKind::TextInput,
        vec![],
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Px(30.0)),
            content: Some("abc".into()),
            on_change: Some(true),
            focus_on_mount: Some(mount_focus),
            ..Default::default()
        },
    );
    input.runtime.text_input_focused = !mount_focus;
    input.runtime.focused_active = !mount_focus;
    tree.insert(input);
    tree.set_root_id(NodeId(9));
    tree.stamp_all_mounted_at_revision(1);
    tree
}
fn host() -> HostEventRuntime {
    HostEventRuntime::new(false, 40.0, false, Arc::new(NoopHostEventSink), None)
}
fn response(
    engine: &mut TreeUpdateEngine,
    messages: Vec<TreeMsg>,
    policy: TreeUpdateDecodePolicy,
) -> RegistryRebuildPayload {
    match engine
        .process_messages(messages, TreeUpdateOptions::new(None, policy))
        .unwrap()
    {
        TreeUpdateEffect::Layout { output, .. } => output.event_rebuild,
        TreeUpdateEffect::RegistryUpdate { rebuild } => rebuild,
        _ => panic!("expected a registry response"),
    }
}
fn native_rebuild(tree: &mut ElementTree) -> RegistryRebuildPayload {
    crate::tree::layout::layout_and_refresh_default(tree, Constraint::new(200.0, 30.0), 1.0)
        .event_rebuild
}

#[test]
fn queued_pre_edit_registry_does_not_release_buffered_keyboard_input() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for policy in [
        TreeUpdateDecodePolicy::ReturnErr,
        TreeUpdateDecodePolicy::LogAndContinue,
    ] {
        for queued in [1, 8, 64] {
            let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
            let initial = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
            assert!(initial.mount_focus_targets.is_none());
            let mut host = host();
            host.install_rebuild(initial.clone());
            assert!(host.drain_tree_messages().is_empty());
            host.handle_input(InputEvent::TextCommit {
                text: "X".into(),
                mods: 0,
            });
            assert!(host.driver.runtime.listener_lane.is_stale());
            host.handle_input(InputEvent::TextCommit {
                text: "Y".into(),
                mods: 0,
            });
            for _ in 0..queued {
                host.install_rebuild(initial.clone());
                assert_eq!(
                    host.driver.runtime.listener_lane.buffered_inputs.len(),
                    1,
                    "a frame queued before the edit cannot acknowledge it or replay Y"
                );
            }
            let mut foreign = initial.clone();
            foreign.listener_barrier = Some(crate::actors::ListenerBarrier::default());
            host.install_rebuild(foreign);
            assert_eq!(host.driver.runtime.listener_lane.buffered_inputs.len(), 1);
            let first = response(&mut engine, host.drain_tree_messages(), policy);
            host.install_rebuild(first.clone());
            assert!(
                host.driver.runtime.listener_lane.is_stale(),
                "replaying Y needs its own response"
            );
            let content = host.focused_text_state().unwrap().content;
            assert!(content.contains("XY"));
            assert_eq!(content.replace("XY", ""), "abc");
            host.install_rebuild(first);
            assert!(
                host.driver.runtime.listener_lane.is_stale(),
                "X's response cannot acknowledge Y"
            );
            let last = response(&mut engine, host.drain_tree_messages(), policy);
            host.install_rebuild(last);
            assert!(!host.driver.runtime.listener_lane.is_stale());
            assert!(host.drain_tree_messages().is_empty());
            assert_eq!(host.focused_text_state().unwrap().content, content);
        }
    }
}

#[test]
fn event_coalescing_rebinds_pending_mount_focus_only_to_the_same_eligible_mount() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for change in 0..4 {
        let mut tree = editable_tree(true);
        let first = native_rebuild(&mut tree);
        assert!(first.focus_on_mount.is_some());
        tree.set_revision(2);
        match change {
            0 => {}
            1 => {
                tree.remove_node(&NodeId(9));
            }
            2 => {
                tree.remove_node(&NodeId(9));
                let mut replacement = editable_tree(true);
                replacement.stamp_all_mounted_at_revision(2);
                replacement.set_revision(3);
                tree = replacement;
            }
            _ => {
                tree.get_mut(&NodeId(9))
                    .unwrap()
                    .spec
                    .declared
                    .focus_on_mount = Some(false);
                tree.mark_measure_dirty(&NodeId(9));
            }
        }
        let next = native_rebuild(&mut tree);
        assert!(next.focus_on_mount.is_none());
        let (tx, rx) = bounded(4);
        tx.send(EventMsg::RegistryUpdate { rebuild: next }).unwrap();
        let mut pending = None;
        let (result, count) = coalesce_registry_updates(first, &rx, &mut pending);
        assert_eq!(count, 1);
        assert_eq!(
            result.focus_on_mount.is_some(),
            change == 0,
            "change {change}"
        );
    }
}

#[test]
fn deferred_mount_focus_survives_a_causal_wait_but_not_remount_or_removal() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for change in 0..4 {
        let mut tree = editable_tree(true);
        let first = native_rebuild(&mut tree);
        let mut host = host();
        // Isolate the install protocol with an outstanding native receipt.
        let fence = crate::actors::ListenerBarrier::default();
        host.driver.awaiting_registry = Some(fence.clone());
        host.install_rebuild(first);
        assert!(host.driver.pending_mount_focus.is_some());
        assert!(host.drain_tree_messages().is_empty());
        tree.set_revision(2);
        match change {
            0 => {}
            1 => {
                tree.remove_node(&NodeId(9));
            }
            2 => {
                tree.stamp_all_mounted_at_revision(2);
                tree.set_revision(3);
            }
            _ => {
                tree.get_mut(&NodeId(9))
                    .unwrap()
                    .spec
                    .declared
                    .focus_on_mount = Some(false);
                tree.mark_measure_dirty(&NodeId(9));
            }
        }
        let mut next = native_rebuild(&mut tree);
        next.listener_barrier = Some(fence);
        host.install_rebuild(next);
        assert!(host.driver.pending_mount_focus.is_none());
        assert_eq!(
            host.focused_text_state().is_some(),
            change == 0,
            "change {change}"
        );
        assert_eq!(
            host.driver.runtime.last_focus_on_mount_revision,
            if change == 0 { 1 } else { 0 }
        );
    }
}

#[test]
fn cached_registry_acknowledges_noop_requests_without_constructing_a_scene() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
    response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        TreeUpdateDecodePolicy::ReturnErr,
    );
    for _ in 0..3 {
        let fence = crate::actors::ListenerBarrier::default();
        let effect = engine
            .process_messages(
                vec![TreeMsg::ListenerBarrier(fence.clone())],
                TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
            )
            .unwrap();
        let TreeUpdateEffect::RegistryUpdate { rebuild } = effect else {
            panic!("no-op fence must not construct a scene")
        };
        assert!(fence.matches(rebuild.listener_barrier.as_ref().unwrap()));
    }
}

#[test]
fn event_registry_draining_is_bounded_and_stop_does_not_wait_for_a_receipt() {
    let (tx, rx) = unbounded();
    for _ in 0..129 {
        tx.send(EventMsg::RegistryUpdate {
            rebuild: RegistryRebuildPayload::default(),
        })
        .unwrap();
    }
    tx.send(EventMsg::Stop).unwrap();
    let mut pending = None;
    let (_, count) =
        coalesce_registry_updates(RegistryRebuildPayload::default(), &rx, &mut pending);
    assert_eq!(count, 63);
    assert_eq!(rx.len(), 67);
    let mut host = host();
    host.driver.awaiting_registry = Some(crate::actors::ListenerBarrier::default());
    assert!(
        !host
            .driver
            .handle_actor_message(EventMsg::Stop, &rx, &mut pending)
    );
}

#[test]
fn real_event_actor_waits_for_its_own_response_and_can_stop_with_buffered_input() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for recover in [false, true] {
        let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
        let initial = response(
            &mut engine,
            vec![TreeMsg::RebuildRegistry],
            TreeUpdateDecodePolicy::ReturnErr,
        );
        let (event_tx, event_rx) = bounded(16);
        let (tree_tx, tree_rx) = bounded(64);
        event_tx
            .send(EventMsg::RegistryUpdate {
                rebuild: initial.clone(),
            })
            .unwrap();
        event_tx
            .send(EventMsg::InputEvent(InputEvent::TextCommit {
                text: "X".into(),
                mods: 0,
            }))
            .unwrap();
        event_tx
            .send(EventMsg::RegistryUpdate { rebuild: initial })
            .unwrap();
        event_tx
            .send(EventMsg::InputEvent(InputEvent::TextCommit {
                text: "Y".into(),
                mods: 0,
            }))
            .unwrap();
        if !recover {
            event_tx.send(EventMsg::Stop).unwrap();
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
        if recover {
            for expected in ["X", "XY"] {
                let mut messages = Vec::new();
                loop {
                    let message = tree_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    let fence = message
                        .commands()
                        .iter()
                        .any(|msg| matches!(msg, TreeMsg::ListenerBarrier(_)));
                    push_tree_msg_flat(message, &mut messages);
                    if fence {
                        break;
                    }
                }
                let rebuild = response(&mut engine, messages, TreeUpdateDecodePolicy::ReturnErr);
                let content = &rebuild.text_inputs[&NodeId(9)].content;
                assert!(content.contains(expected), "{content}");
                assert_eq!(content.replace(expected, ""), "abc");
                event_tx.send(EventMsg::RegistryUpdate { rebuild }).unwrap();
            }
            event_tx.send(EventMsg::Stop).unwrap();
            handle.join().unwrap();
            assert!(tree_rx.is_empty());
        } else {
            handle.join().unwrap();
            let messages = drain_msgs(&tree_rx);
            let rebuild = response(&mut engine, messages, TreeUpdateDecodePolicy::ReturnErr);
            let content = &rebuild.text_inputs[&NodeId(9)].content;
            assert!(content.contains('X'));
            assert!(!content.contains('Y'));
        }
    }
}

#[test]
fn mount_focus_metadata_is_shared_and_released_with_its_native_owners() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let first = {
        let mut tree = editable_tree(true);
        native_rebuild(&mut tree)
    };
    let weak = Arc::downgrade(first.mount_focus_targets.as_ref().unwrap());
    let second = first.clone();
    assert!(Arc::ptr_eq(
        first.mount_focus_targets.as_ref().unwrap(),
        second.mount_focus_targets.as_ref().unwrap()
    ));
    assert_eq!(first.mount_focus_targets.as_ref().unwrap().len(), 1);
    drop(first);
    assert!(weak.upgrade().is_some());
    drop(second);
    assert!(weak.upgrade().is_none());
}

mod pointer;

pub(crate) mod stalled;

mod yielding;

mod outbound;
