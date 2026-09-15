use super::*;

#[test]
fn queued_text_and_selection_commands_cannot_modify_a_replacement() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for edit in [false, true] {
        for policy in [
            TreeUpdateDecodePolicy::ReturnErr,
            TreeUpdateDecodePolicy::LogAndContinue,
        ] {
            let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
            let (mut host, _) = recorded_host();
            host.install_rebuild(response(
                &mut engine,
                vec![TreeMsg::RebuildRegistry],
                policy,
            ));
            if edit {
                host.handle_input(InputEvent::TextCommit {
                    text: "X".into(),
                    mods: 0,
                });
            } else {
                host.handle_input(button(ACTION_PRESS, 0.0, 10.0));
                host.handle_input(InputEvent::CursorPos { x: 100.0, y: 10.0 });
            }
            let queued = host.drain_tree_messages();
            assert!(!queued.is_empty());
            let replacement = encoded_tree(ElementKind::TextInput, &[56]);
            let replaced = response(&mut engine, vec![upload(&replacement)], policy);
            let expected = replaced.text_inputs[&NodeId(9)].clone();
            let latest = response(&mut engine, queued, policy);
            assert_eq!(
                latest.text_inputs[&NodeId(9)],
                expected,
                "old event commands must not mutate the new mount (edit={edit})"
            );
            host.install_rebuild(latest);
            settle(&mut host, &mut engine, policy);
            assert!(host.driver.awaiting_registry.is_none());
            assert_eq!(host.driver.runtime.text_states[&NodeId(9)].content, "");
        }
    }
}

fn queue_actions(host: &mut HostEventRuntime, messages: Vec<TreeMsg>) -> Vec<TreeMsg> {
    host.driver.dispatch(|runtime, tx, log| {
        runtime.apply_listener_actions(
            messages
                .into_iter()
                .map(registry_builder::ListenerAction::TreeMsg)
                .collect(),
            tx,
            log,
            DispatchMode::Normal,
        )
    });
    host.drain_tree_messages()
}
fn scroller() -> ElementTree {
    let mut tree = encoded_tree(ElementKind::Column, &[7, 8, 43, 44]);
    let node = tree.get_mut(&NodeId(9)).unwrap();
    node.spec.attrs_raw = 8u16
        .to_be_bytes()
        .into_iter()
        .chain(node.spec.attrs_raw[2..].iter().copied())
        .chain([46, 0, 0, 0, 0, 60, 0, 0, 0, 0])
        .collect();
    node.spec.declared = crate::tree::attrs::decode_attrs(&node.spec.attrs_raw).unwrap();
    node.layout.effective = node.spec.declared.clone();
    tree.insert(encoded_node(
        NodeId(10),
        ElementKind::El,
        &[40],
        400.0,
        200.0,
    ));
    tree.set_children(&NodeId(9), vec![NodeId(10)]).unwrap();
    tree.stamp_all_mounted_at_revision(1);
    tree
}
fn deferred(index: usize) -> TreeMsg {
    let element_id = NodeId(9);
    match index {
        0 => TreeMsg::ScrollRequest {
            element_id,
            dx: -8.0,
            dy: -12.0,
        },
        1 => TreeMsg::ScrollbarThumbDragX {
            element_id,
            dx: 8.0,
        },
        2 => TreeMsg::ScrollbarThumbDragY {
            element_id,
            dy: 12.0,
        },
        3 => TreeMsg::SetScrollbarXHover {
            element_id,
            hovered: true,
        },
        4 => TreeMsg::SetScrollbarYHover {
            element_id,
            hovered: true,
        },
        5 => TreeMsg::SetMouseOverActive {
            element_id,
            active: true,
        },
        6 => TreeMsg::SetMouseDownActive {
            element_id,
            active: true,
        },
        _ => TreeMsg::SetFocusedActive {
            element_id,
            active: true,
        },
    }
}
#[test]
fn deferred_event_mutations_do_not_cross_remount_in_either_tree_batch_order() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for policy in [
        TreeUpdateDecodePolicy::ReturnErr,
        TreeUpdateDecodePolicy::LogAndContinue,
    ] {
        for before in [false, true] {
            for index in 0..8 {
                let tree = scroller();
                let mut engine = TreeUpdateEngine::new(tree.clone(), 200, 30);
                let (mut host, _) = recorded_host();
                host.install_rebuild(response(
                    &mut engine,
                    vec![TreeMsg::RebuildRegistry],
                    policy,
                ));
                let queued = queue_actions(&mut host, vec![deferred(index)]);
                let messages = if before {
                    queued.into_iter().chain([upload(&tree)]).collect()
                } else {
                    std::iter::once(upload(&tree)).chain(queued).collect()
                };
                let latest = response(&mut engine, messages, policy);
                let node = engine.tree().get(&NodeId(9)).unwrap();
                assert_eq!(
                    (node.layout.scroll_x, node.layout.scroll_y),
                    (0.0, 0.0),
                    "before={before} index={index}"
                );
                assert!(!node.runtime.mouse_over_active);
                assert!(!node.runtime.mouse_down_active);
                assert!(!node.runtime.focused_active);
                assert!(node.runtime.scrollbar_hover_axis.is_none());
                host.install_rebuild(latest);
                assert!(host.driver.awaiting_registry.is_none());
            }
        }
    }
}

fn replace_child(tree: &ElementTree) -> TreeMsg {
    let mut child = ElementTree::new();
    child.insert(tree.get(&NodeId(10)).unwrap().clone());
    child.set_root_id(NodeId(10));
    let bytes = crate::tree::serialize::encode_tree(&child);
    let encoded = [4]
        .into_iter()
        .chain(NodeId(10).to_be_bytes())
        .chain([3])
        .chain(NodeId(9).to_be_bytes())
        .chain(0u16.to_be_bytes())
        .chain((bytes.len() as u32).to_be_bytes())
        .chain(bytes)
        .collect();
    TreeMsg::PatchTree {
        bytes: encoded,
        submitted_at: None,
    }
}
#[test]
fn one_stale_target_rejects_the_whole_event_operation_but_not_controls() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut tree = scroller();
    tree.get_mut(&NodeId(9)).unwrap().runtime.focused_active = true;
    let mut engine = TreeUpdateEngine::new(tree.clone(), 200, 30);
    let (mut host, _) = recorded_host();
    let first = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
    let root_mount = first.base_registry.source_mount(&NodeId(9));
    host.install_rebuild(first);
    let queued = queue_actions(
        &mut host,
        vec![
            TreeMsg::SetFocusedActive {
                element_id: NodeId(9),
                active: false,
            },
            TreeMsg::SetFocusedActive {
                element_id: NodeId(10),
                active: true,
            },
            TreeMsg::Resize {
                width: 250.0,
                height: 40.0,
                scale: 1.0,
            },
        ],
    );
    let replaced = response(&mut engine, vec![replace_child(&tree)], policy);
    assert_eq!(replaced.base_registry.source_mount(&NodeId(9)), root_mount);
    let latest = response(&mut engine, queued, policy);
    assert!(
        engine
            .tree()
            .get(&NodeId(9))
            .unwrap()
            .runtime
            .focused_active,
        "a stale focus target must not blur the still-valid previous target"
    );
    assert!(
        !engine
            .tree()
            .get(&NodeId(10))
            .unwrap()
            .runtime
            .focused_active
    );
    assert_eq!(engine.size(), (250.0, 40.0));
    host.install_rebuild(latest);
    assert!(host.driver.awaiting_registry.is_none());
}

#[test]
fn same_mount_commands_still_apply_after_unrelated_revision_changes() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let tree = scroller();
    let mut engine = TreeUpdateEngine::new(tree.clone(), 200, 30);
    let (mut host, _) = recorded_host();
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        policy,
    ));
    let queued = queue_actions(
        &mut host,
        vec![
            TreeMsg::ScrollRequest {
                element_id: NodeId(9),
                dx: 0.0,
                dy: -15.0,
            },
            TreeMsg::SetMouseDownActive {
                element_id: NodeId(9),
                active: true,
            },
        ],
    );
    response(&mut engine, vec![replace_child(&tree)], policy);
    let latest = response(&mut engine, queued, policy);
    assert_eq!(engine.tree().get(&NodeId(9)).unwrap().layout.scroll_y, 15.0);
    assert!(
        engine
            .tree()
            .get(&NodeId(9))
            .unwrap()
            .runtime
            .mouse_down_active
    );
    host.install_rebuild(latest);
    assert!(host.driver.awaiting_registry.is_none());
}

#[test]
fn malformed_or_missing_event_evidence_fails_closed_and_stop_remains_control() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    for case in 0..4 {
        let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
        let first = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
        let mount = first.base_registry.source_mount(&NodeId(9));
        let targets = match case {
            0 => vec![],
            1 => vec![(NodeId(9), None)],
            _ => vec![(NodeId(9), mount)],
        };
        let command = TreeMsg::SetTextInputContent {
            element_id: NodeId(9),
            content: "bad".into(),
        };
        let messages = match case {
            2 => vec![upload(&encoded_tree(ElementKind::TextInput, &[])), command],
            3 => vec![TreeMsg::Batch(vec![command])],
            _ => vec![command],
        };
        let latest = response(
            &mut engine,
            vec![TreeMsg::EventBatch {
                target_mounts: targets.into_boxed_slice(),
                messages,
            }],
            policy,
        );
        assert_eq!(latest.text_inputs[&NodeId(9)].content, "abc");
        assert!(matches!(
            engine
                .process_messages(
                    vec![TreeMsg::EventBatch {
                        target_mounts: vec![(NodeId(9), None)].into_boxed_slice(),
                        messages: vec![TreeMsg::Stop]
                    }],
                    TreeUpdateOptions::new(None, policy)
                )
                .unwrap(),
            TreeUpdateEffect::Stop
        ));
    }
}

#[test]
fn real_event_actor_commands_queued_before_replacement_are_rejected_on_delivery() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
    let initial = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
    let (event_tx, event_rx) = bounded(16);
    let (tree_tx, tree_rx) = bounded(16);
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
    event_tx
        .send(EventMsg::RegistryUpdate { rebuild: initial })
        .unwrap();
    event_tx
        .send(EventMsg::InputEvent(InputEvent::TextCommit {
            text: "X".into(),
            mods: 0,
        }))
        .unwrap();
    let queued = tree_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(queued, TreeMsg::EventBatch { .. }));
    let replacement = response(
        &mut engine,
        vec![upload(&encoded_tree(ElementKind::TextInput, &[56]))],
        policy,
    );
    event_tx
        .send(EventMsg::RegistryUpdate {
            rebuild: replacement,
        })
        .unwrap();
    let latest = response(&mut engine, vec![queued], policy);
    assert_eq!(latest.text_inputs[&NodeId(9)].content, "");
    assert!(!latest.text_inputs[&NodeId(9)].focused);
    event_tx
        .send(EventMsg::RegistryUpdate { rebuild: latest })
        .unwrap();
    event_tx.send(EventMsg::Stop).unwrap();
    handle.join().unwrap();
    assert!(tree_rx.is_empty());
}

#[test]
fn queued_slider_value_cannot_overwrite_a_replacement() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for policy in [
        TreeUpdateDecodePolicy::ReturnErr,
        TreeUpdateDecodePolicy::LogAndContinue,
    ] {
        let mut tree = encoded_tree(ElementKind::Slider, &[56]);
        tree.insert(encoded_node(NodeId(10), ElementKind::El, &[], 200.0, 10.0));
        tree.insert(encoded_node(NodeId(11), ElementKind::El, &[], 20.0, 20.0));
        tree.set_children(&NodeId(9), vec![NodeId(10), NodeId(11)])
            .unwrap();
        tree.stamp_all_mounted_at_revision(1);
        let mut engine = TreeUpdateEngine::new(tree.clone(), 200, 30);
        let (mut host, _) = recorded_host();
        host.install_rebuild(response(
            &mut engine,
            vec![TreeMsg::RebuildRegistry],
            policy,
        ));
        host.handle_input(button(ACTION_PRESS, 50.0, 15.0));
        let queued = host.drain_tree_messages();
        assert!(
            queued
                .iter()
                .flat_map(TreeMsg::commands)
                .any(|message| matches!(message, TreeMsg::SetSliderValue { .. }))
        );
        response(&mut engine, vec![upload(&tree)], policy);
        host.install_rebuild(response(&mut engine, queued, policy));
        settle(&mut host, &mut engine, policy);
        assert_eq!(host.driver.runtime.slider_states[&NodeId(9)].value, 0.0);
        assert!(host.driver.runtime.runtime_overlay.slider_drag.is_none());
    }
}

#[test]
fn focused_listenerless_nodes_still_provide_blur_target_evidence() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut tree = encoded_tree(ElementKind::El, &[]);
    tree.get_mut(&NodeId(9)).unwrap().runtime.focused_active = true;
    let mut engine = TreeUpdateEngine::new(tree, 200, 30);
    let rebuild = response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        TreeUpdateDecodePolicy::ReturnErr,
    );
    assert_eq!(rebuild.base_registry.source_mount(&NodeId(9)), Some(1));
}
