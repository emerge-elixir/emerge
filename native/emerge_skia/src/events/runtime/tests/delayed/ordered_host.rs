//! Host admission shares raw input's dependency queue. No claim of async local
//! publication yet: this closes the overtaking hole before relaxing edit waits.
use super::*;

fn ready() -> (TreeUpdateEngine, HostEventRuntime) {
    let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
    let mut host = host();
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    (engine, host)
}
fn commit(host: &mut HostEventRuntime, text: &str) {
    host.handle_input(InputEvent::TextCommit {
        text: text.into(),
        mods: 0,
    });
}
fn settle(engine: &mut TreeUpdateEngine, host: &mut HostEventRuntime) -> usize {
    for round in 0..256 {
        let messages = host.drain_tree_messages();
        if messages.is_empty() {
            assert!(!host.driver.runtime.listener_lane.is_stale());
            assert!(host.driver.runtime.listener_lane.buffered_inputs.is_empty());
            return round;
        }
        host.install_rebuild(response(
            engine,
            messages,
            TreeUpdateDecodePolicy::ReturnErr,
        ));
    }
    panic!("host did not settle");
}

fn two_inputs() -> (TreeUpdateEngine, HostEventRuntime) {
    let mut tree = editable_tree(false);
    tree.insert(Element::with_attrs(
        NodeId(10),
        ElementKind::TextInput,
        vec![],
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Px(30.0)),
            content: Some("other".into()),
            on_change: Some(true),
            ..Default::default()
        },
    ));
    tree.insert(Element::with_attrs(
        NodeId(1),
        ElementKind::Row,
        vec![],
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Px(30.0)),
            ..Default::default()
        },
    ));
    tree.set_children(&NodeId(1), vec![NodeId(9), NodeId(10)])
        .unwrap();
    tree.set_root_id(NodeId(1));
    tree.stamp_all_mounted_at_revision(1);
    let mut engine = TreeUpdateEngine::new(tree, 200, 30);
    let mut host = host();
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    (engine, host)
}

#[test]
fn host_edit_does_not_overtake_raw_input_or_replace_its_receipt() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host) = ready();
    let recorder = Arc::new(pointer::Recorder::default());
    host.driver.runtime.host_event_sink = Some(recorder.clone());
    commit(&mut host, "X");
    let first_barrier = host.driver.awaiting_registry.clone().unwrap();
    let first = host.drain_tree_messages();
    commit(&mut host, "Y");
    let after_x = host.focused_text_state().unwrap().content;
    assert!(host.handle_text_input_edit(TextInputEditRequest::Insert("Z".into())));
    assert_eq!(
        host.focused_text_state().unwrap().content,
        after_x,
        "a host edit must not overtake the raw edit waiting for geometry"
    );
    assert!(
        host.driver
            .awaiting_registry
            .as_ref()
            .unwrap()
            .matches(&first_barrier)
    );
    assert_eq!(host.driver.runtime.listener_lane.buffered_inputs.len(), 2);
    assert!(host.drain_tree_messages().is_empty());
    assert_eq!(
        recorder.events.lock().unwrap().len(),
        1,
        "no premature callback"
    );
    let obsolete = response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        TreeUpdateDecodePolicy::ReturnErr,
    );
    host.install_rebuild(obsolete);
    assert_eq!(host.driver.runtime.listener_lane.buffered_inputs.len(), 2);
    assert_eq!(host.focused_text_state().unwrap().content, after_x);
    host.install_rebuild(response(
        &mut engine,
        first,
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    settle(&mut engine, &mut host);
    assert_eq!(
        host.focused_text_state()
            .unwrap()
            .content
            .replace("XYZ", ""),
        "abc"
    );
    assert_eq!(recorder.events.lock().unwrap().len(), 3);
    assert_eq!(
        recorder.raw.lock().unwrap().len(),
        2,
        "host replay is not raw input"
    );
}

#[test]
fn host_selection_and_insert_are_deferred_as_an_ordered_pair() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host) = ready();
    commit(&mut host, "X");
    let current = host.focused_text_state().unwrap();
    assert!(host.handle_text_input_command(TextInputCommandRequest::SelectAll));
    assert!(host.handle_text_input_edit(TextInputEditRequest::Insert("Y".into())));
    assert_eq!(
        host.focused_text_state().unwrap(),
        current,
        "selection and insertion must wait, not mutate the old text immediately"
    );
    settle(&mut engine, &mut host);
    assert_eq!(host.focused_text_state().unwrap().content, "Y");
    assert_eq!(
        host.driver
            .runtime
            .clipboard
            .get_text(ClipboardTarget::Primary),
        Some(current.content)
    );
}

#[test]
fn host_edit_after_a_queued_click_uses_the_focus_decided_by_that_click() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host) = two_inputs();
    assert_eq!(host.driver.runtime.focused_id, Some(NodeId(9)));
    commit(&mut host, "A");
    host.handle_input(pointer::button(ACTION_PRESS, 150.0, 15.0));
    assert!(host.prepare_text_input_replacement_range(Some((0, 5))));
    assert!(host.handle_text_input_edit(TextInputEditRequest::Insert("B".into())));
    assert!(!host.focused_text_state().unwrap().content.contains('B'));
    settle(&mut engine, &mut host);
    assert_eq!(host.driver.runtime.focused_id, Some(NodeId(10)));
    let old = &host.driver.runtime.text_states[&NodeId(9)].content;
    let new = &host.driver.runtime.text_states[&NodeId(10)].content;
    assert_eq!(old.replace('A', ""), "abc");
    assert_eq!(new.replace('B', ""), "other");
}

#[test]
fn deferred_replacement_range_is_mount_scoped_but_a_later_key_uses_current_focus() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for remount in [false, true] {
        let (mut engine, mut host) = ready();
        commit(&mut host, "X");
        let first = host.drain_tree_messages();
        assert!(host.prepare_text_input_replacement_range(Some((0, 1))));
        assert!(host.handle_text_input_edit(TextInputEditRequest::Insert("Z".into())));
        if remount {
            let mut tree = editable_tree(false);
            tree.set_revision(2);
            tree.stamp_all_mounted_at_revision(2);
            engine = TreeUpdateEngine::new(tree, 200, 30);
        }
        host.install_rebuild(response(
            &mut engine,
            first,
            TreeUpdateDecodePolicy::ReturnErr,
        ));
        settle(&mut engine, &mut host);
        let content = host.focused_text_state().unwrap().content;
        assert_eq!(
            content.matches('Z').count(),
            1,
            "the queued insertion must run once"
        );
        if remount {
            assert_eq!(
                content.replace('Z', ""),
                "abc",
                "old range must not select the replacement"
            );
        } else {
            assert_eq!(
                content.replace(['X', 'Z'], ""),
                "bc",
                "same mount retains the requested range"
            );
        }
    }
}

#[test]
fn host_operations_are_coalescing_boundaries_and_replay_tail_moves_in_order() {
    let mut lane = ListenerLaneState::initially_stale();
    lane.buffer_input(InputEvent::CursorPos { x: 1.0, y: 0.0 });
    lane.buffer_input(InputEvent::CursorPos { x: 2.0, y: 0.0 });
    lane.buffer_pending(PendingInput::Command(TextInputCommandRequest::Copy));
    lane.buffer_input(InputEvent::CursorPos { x: 3.0, y: 0.0 });
    lane.buffer_input(InputEvent::CursorPos { x: 4.0, y: 0.0 });
    let mut text = String::with_capacity(4096);
    text.push_str("payload");
    let pointer = text.as_ptr();
    lane.buffer_pending(PendingInput::Edit(TextInputEditRequest::Insert(text)));
    let tail = lane.mark_fresh_and_take_buffered();
    assert_eq!(tail.len(), 4);
    lane.restore_replay_tail(tail);
    let mut restored = lane.mark_fresh_and_take_buffered();
    assert_eq!(
        restored.pop_front(),
        Some(PendingInput::Raw(InputEvent::CursorPos { x: 2.0, y: 0.0 }))
    );
    assert_eq!(
        restored.pop_front(),
        Some(PendingInput::Command(TextInputCommandRequest::Copy))
    );
    assert_eq!(
        restored.pop_front(),
        Some(PendingInput::Raw(InputEvent::CursorPos { x: 4.0, y: 0.0 }))
    );
    let Some(PendingInput::Edit(TextInputEditRequest::Insert(text))) = restored.pop_front() else {
        panic!("insert");
    };
    assert_eq!(
        text.as_ptr(),
        pointer,
        "queue movement must not clone the payload"
    );
    drop(text);
    lane.restore_replay_tail(restored);
    assert_eq!(lane.buffered_inputs.capacity(), 0);
}

#[test]
fn a_long_host_command_queue_yields_with_the_existing_native_quantum() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host) = ready();
    commit(&mut host, "X");
    for _ in 0..4096 {
        assert!(host.handle_text_input_command(TextInputCommandRequest::Copy));
    }
    let first = host.drain_tree_messages();
    host.install_rebuild(response(
        &mut engine,
        first,
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    assert_eq!(
        host.driver.runtime.listener_lane.buffered_inputs.len(),
        4096 - INPUT_BATCH_LIMIT
    );
    assert_eq!(settle(&mut engine, &mut host), 63);
    assert_eq!(
        host.driver.runtime.listener_lane.buffered_inputs.capacity(),
        0
    );
}

#[test]
fn host_input_before_initial_registry_is_accepted_without_requesting_fallback() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
    let mut host = host();
    assert!(host.focused_text_state().is_none());
    assert!(host.handle_text_input_edit(TextInputEditRequest::Insert("X".into())));
    assert!(host.drain_tree_messages().is_empty());
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    settle(&mut engine, &mut host);
    assert_eq!(host.focused_text_state().unwrap().content, "abcX");
}

#[test]
fn failed_tree_update_does_not_dispatch_host_input_until_successful_recovery() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host) = ready();
    commit(&mut host, "X");
    let first = host.drain_tree_messages();
    assert!(host.handle_text_input_edit(TextInputEditRequest::Insert("Y".into())));
    // The native edit/fence is consumed before a malformed external patch. This
    // is a failed batch, not an acknowledgment of the queued host operation.
    let messages = first
        .into_iter()
        .chain([TreeMsg::PatchTree {
            bytes: vec![1],
            submitted_at: None,
        }])
        .collect();
    assert!(
        engine
            .process_messages(
                messages,
                TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr)
            )
            .is_err()
    );
    assert_eq!(host.focused_text_state().unwrap().content, "abcX");
    assert_eq!(host.driver.runtime.listener_lane.buffered_inputs.len(), 1);
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    settle(&mut engine, &mut host);
    assert_eq!(host.focused_text_state().unwrap().content, "abcXY");
}

#[test]
fn delayed_range_cannot_address_text_changed_by_an_earlier_queued_edit() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host) = ready();
    commit(&mut host, "X");
    commit(&mut host, "Y");
    assert!(host.prepare_text_input_replacement_range(Some((0, 1))));
    assert!(host.handle_text_input_edit(TextInputEditRequest::Insert("Z".into())));
    settle(&mut engine, &mut host);
    assert_eq!(host.focused_text_state().unwrap().content, "abcXYZ");
}

#[test]
fn delayed_range_does_not_survive_focus_away_and_back_to_the_same_mount() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host) = two_inputs();
    commit(&mut host, "X");
    for x in [150.0, 95.0] {
        host.handle_input(pointer::button(ACTION_PRESS, x, 15.0));
        host.handle_input(pointer::button(crate::input::ACTION_RELEASE, x, 15.0));
    }
    assert!(host.prepare_text_input_replacement_range(Some((0, 1))));
    assert!(host.handle_text_input_edit(TextInputEditRequest::Insert("Z".into())));
    settle(&mut engine, &mut host);
    assert_eq!(host.driver.runtime.focused_id, Some(NodeId(9)));
    assert_eq!(host.focused_text_state().unwrap().content, "abcXZ");
}

#[test]
fn accepted_external_value_invalidates_old_range_and_destruction_releases_tokens() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host) = ready();
    commit(&mut host, "X");
    let first = host.drain_tree_messages();
    assert!(host.prepare_text_input_replacement_range(Some((0, 1))));
    assert!(host.handle_text_input_edit(TextInputEditRequest::Insert("Z".into())));
    let old = Arc::downgrade(&host.driver.runtime.range_generation.0);
    let mut rebuild = response(&mut engine, first, TreeUpdateDecodePolicy::ReturnErr);
    // Reconciliation unit: inject the metadata carried by an authoritative
    // focused-value replacement, not a claim of a public codec/BEAM trace.
    rebuild
        .text_inputs
        .get_mut(&NodeId(9))
        .unwrap()
        .patch_content = Some("reset".into());
    host.install_rebuild(rebuild);
    settle(&mut engine, &mut host);
    let content = host.focused_text_state().unwrap().content;
    assert_eq!(content.matches('Z').count(), 1);
    assert_eq!(content.replace('Z', ""), "reset");
    assert!(
        old.upgrade().is_none(),
        "invalidated range token has no retained history"
    );
    commit(&mut host, "Q");
    assert!(host.prepare_text_input_replacement_range(Some((0, 1))));
    let pending = Arc::downgrade(&host.driver.runtime.range_generation.0);
    drop(host);
    assert!(
        pending.upgrade().is_none(),
        "host destruction drops pending ranges synchronously"
    );
}
