use super::*;

#[derive(Default)]
struct Changes(Mutex<Vec<String>>);
impl HostEventSink for Changes {
    fn send_raw_input(&self, _: &InputEvent) {}
    fn send_element_event(
        &self,
        _: &NodeId,
        kind: ElementEventKind,
        payload: Option<&registry_builder::ElixirEventPayload>,
    ) {
        if kind == ElementEventKind::Change
            && let Some(registry_builder::ElixirEventPayload::String(text)) = payload
        {
            self.0.lock().unwrap().push(text.clone());
        }
    }
}

fn attrs(text: &str) -> Vec<u8> {
    4u16.to_be_bytes()
        .into_iter()
        .chain([1, 2])
        .chain(200f64.to_be_bytes())
        .chain([2, 2])
        .chain(30f64.to_be_bytes())
        .chain([21])
        .chain((text.len() as u16).to_be_bytes())
        .chain(text.bytes())
        .chain([56, 1])
        .collect()
}

fn input_tree(kind: ElementKind, emit_change: bool) -> ElementTree {
    let mut raw = attrs("");
    *raw.last_mut().unwrap() = u8::from(emit_change);
    let mut input = Element::with_attrs(
        NodeId(9),
        kind,
        raw.clone(),
        crate::tree::attrs::decode_attrs(&raw).unwrap(),
    );
    input.runtime.text_input_focused = true;
    input.runtime.focused_active = true;
    input.runtime.text_input_cursor = Some(0);
    let mut tree = ElementTree::new();
    tree.set_revision(1);
    tree.insert(input);
    tree.set_root_id(NodeId(9));
    tree.stamp_all_mounted_at_revision(1);
    tree
}

fn setup(kind: ElementKind) -> (TreeUpdateEngine, HostEventRuntime, Arc<Changes>) {
    start(input_tree(kind, true))
}

fn start(tree: ElementTree) -> (TreeUpdateEngine, HostEventRuntime, Arc<Changes>) {
    let mut engine = TreeUpdateEngine::new(tree, 200, 100);
    let changes = Arc::new(Changes::default());
    let mut host = HostEventRuntime::new(false, 40.0, false, changes.clone(), None);
    // Use full frame preparation, as production UploadTree does. Registry-only
    // startup bypasses frame authority and hides stale-projection cursor clamps.
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
    (engine, host, changes)
}

fn settle(engine: &mut TreeUpdateEngine, host: &mut HostEventRuntime) {
    for _ in 0..32 {
        let messages = host.drain_tree_messages();
        if messages.is_empty() {
            assert!(!host.driver.runtime.listener_lane.is_stale());
            return;
        }
        host.install_rebuild(response(
            engine,
            messages,
            TreeUpdateDecodePolicy::ReturnErr,
        ));
    }
    panic!("text reconciliation did not settle");
}

fn commit(engine: &mut TreeUpdateEngine, host: &mut HostEventRuntime, text: &str) {
    host.handle_input(InputEvent::TextCommit {
        text: text.into(),
        mods: 0,
    });
    settle(engine, host);
    assert_native_edit_state(engine, host);
}

fn assert_native_edit_state(engine: &TreeUpdateEngine, host: &HostEventRuntime) {
    let live = &host.driver.runtime.text_states[&NodeId(9)];
    let node = engine.tree().get(&NodeId(9)).unwrap();
    assert_eq!(
        node.spec.declared.content.as_deref(),
        Some(live.content.as_str())
    );
    assert_eq!(
        node.layout.effective.content.as_deref(),
        Some(live.content.as_str())
    );
    assert_eq!(node.runtime.text_input_cursor, Some(live.cursor));
    assert_eq!(
        node.runtime.text_input_selection_anchor,
        live.selection_anchor
    );
    assert_eq!(node.runtime.text_input_preedit, live.preedit);
    assert_eq!(node.runtime.text_input_preedit_cursor, live.preedit_cursor);
}

fn app_patch(text: &str) -> TreeMsg {
    app_attrs_patch(attrs(text))
}

fn app_attrs_patch(raw: Vec<u8>) -> TreeMsg {
    TreeMsg::PatchTree {
        bytes: [1]
            .into_iter()
            .chain(NodeId(9).to_be_bytes())
            .chain((raw.len() as u32).to_be_bytes())
            .chain(raw)
            .collect(),
        submitted_at: None,
    }
}

fn echo(
    engine: &mut TreeUpdateEngine,
    host: &mut HostEventRuntime,
    text: &str,
) -> RegistryRebuildPayload {
    let rebuild = response(
        engine,
        vec![app_patch(text)],
        TreeUpdateDecodePolicy::ReturnErr,
    );
    host.install_rebuild(rebuild.clone());
    settle(engine, host);
    assert_native_edit_state(engine, host);
    rebuild
}

fn assert_text(host: &HostEventRuntime, text: &str, cursor: u32) {
    let state = &host.driver.runtime.text_states[&NodeId(9)];
    assert_eq!((state.content.as_str(), state.cursor), (text, cursor));
}

#[test]
fn multiline_word_wrap_updates_height_and_caret_before_controller_echoes() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let font = crate::renderer::make_font_with_style("default", 400, false, 16.0);
    let width = 3.0 * font.measure_str("a", None).0 + font.measure_str(" ", None).0;
    let (_, metrics) = font.metrics();
    let line_height = (metrics.ascent.abs() + metrics.descent).max(16.0);
    let raw_attrs = |text: &str| -> Vec<u8> {
        3u16.to_be_bytes()
            .into_iter()
            .chain([1, 2])
            .chain((width as f64).to_be_bytes())
            .chain([21])
            .chain((text.len() as u16).to_be_bytes())
            .chain(text.bytes())
            .chain([56, 1])
            .collect()
    };
    let mut tree = input_tree(ElementKind::Multiline, true);
    let node = tree.get_mut(&NodeId(9)).unwrap();
    node.spec.attrs_raw = raw_attrs("");
    node.spec.declared = crate::tree::attrs::decode_attrs(&node.spec.attrs_raw).unwrap();
    node.layout.effective = node.spec.declared.clone();
    let (mut engine, mut host, _) = start(tree);

    // All words fit. Each native edit must publish correct geometry without
    // waiting for any app proposal, including the first character on line three.
    for (text, lines) in [("a aaa ", 2), ("a", 3), ("a", 3)] {
        host.handle_input(InputEvent::TextCommit {
            text: text.into(),
            mods: 0,
        });
        let TreeUpdateEffect::Layout { output, .. } = engine
            .process_messages(
                host.drain_tree_messages(),
                TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
            )
            .unwrap()
        else {
            panic!("typing must publish a rendered scene");
        };
        let frame = engine.tree().get(&NodeId(9)).unwrap().layout.frame.unwrap();
        assert!((frame.height - line_height * lines as f32).abs() < 0.001);
        let (caret_x, caret_y, _, _) = output.ime_cursor_area.unwrap();
        assert!((caret_y - frame.y - line_height * (lines - 1) as f32).abs() < 0.001);
        assert!(caret_x <= frame.x + width + 0.001);
        host.install_rebuild(output.event_rebuild);
        settle(&mut engine, &mut host);
        assert_native_edit_state(&engine, &host);
    }
    assert_text(&host, "a aaa aa", 8);
    for text in ["a aaa ", "a aaa aa"] {
        host.install_rebuild(response(
            &mut engine,
            vec![app_attrs_patch(raw_attrs(text))],
            TreeUpdateDecodePolicy::ReturnErr,
        ));
        settle(&mut engine, &mut host);
        assert_text(&host, "a aaa aa", 8);
        let frame = engine.tree().get(&NodeId(9)).unwrap().layout.frame.unwrap();
        assert!((frame.height - 3.0 * line_height).abs() < 0.001);
    }
    for _ in 0..2 {
        assert!(host.handle_text_input_edit(TextInputEditRequest::Backspace));
        settle(&mut engine, &mut host);
    }
    assert_text(&host, "a aaa ", 6);
    let frame = engine.tree().get(&NodeId(9)).unwrap().layout.frame.unwrap();
    assert!((frame.height - 2.0 * line_height).abs() < 0.001);
    assert!(host.handle_text_input_edit(TextInputEditRequest::Insert("aa".into())));
    settle(&mut engine, &mut host);
    assert_native_edit_state(&engine, &host);
    assert_text(&host, "a aaa aa", 8);
    let frame = engine.tree().get(&NodeId(9)).unwrap().layout.frame.unwrap();
    assert!((frame.height - 3.0 * line_height).abs() < 0.001);
}

#[test]
fn delayed_fifo_controller_echoes_preserve_newer_native_edits() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for kind in [ElementKind::TextInput, ElementKind::Multiline] {
        let (mut engine, mut host, changes) = setup(kind);
        commit(&mut engine, &mut host, "a");
        commit(&mut engine, &mut host, "b");
        assert_eq!(*changes.0.lock().unwrap(), ["a", "ab"]);
        echo(&mut engine, &mut host, "a");
        assert_text(&host, "ab", 2);
        echo(&mut engine, &mut host, "ab");
        assert_text(&host, "ab", 2);
        commit(&mut engine, &mut host, "c");
        assert_text(&host, "abc", 3);
        assert_eq!(*changes.0.lock().unwrap(), ["a", "ab", "abc"]);
    }
}

#[test]
fn native_ack_and_app_patch_can_arrive_in_either_order() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for app_first in [false, true] {
        let (mut engine, mut host, _) = setup(ElementKind::TextInput);
        host.handle_input(InputEvent::TextCommit {
            text: "a".into(),
            mods: 0,
        });
        let native = host.drain_tree_messages();
        let messages = if app_first {
            std::iter::once(app_patch("a")).chain(native).collect()
        } else {
            native.into_iter().chain([app_patch("a")]).collect()
        };
        host.install_rebuild(response(
            &mut engine,
            messages,
            TreeUpdateDecodePolicy::ReturnErr,
        ));
        settle(&mut engine, &mut host);
        assert_text(&host, "a", 1);
        assert!(host.driver.runtime.expected_text_echoes.is_empty());
        assert_eq!(
            engine
                .tree()
                .get(&NodeId(9))
                .unwrap()
                .runtime
                .content_proposal
                .as_ref()
                .unwrap()
                .id,
            1
        );
    }
}

#[test]
fn coalesced_and_repeated_echo_values_preserve_latest_edit() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(ElementKind::TextInput);
    commit(&mut engine, &mut host, "a");
    assert!(host.handle_text_input_edit(TextInputEditRequest::Backspace));
    settle(&mut engine, &mut host);
    commit(&mut engine, &mut host, "a");
    commit(&mut engine, &mut host, "b");
    echo(&mut engine, &mut host, "a");
    assert_text(&host, "ab", 2);
    assert_eq!(
        host.driver.runtime.expected_text_echoes[&NodeId(9)].len(),
        3
    );
    echo(&mut engine, &mut host, "ab");
    assert!(host.driver.runtime.expected_text_echoes.is_empty());
    assert_text(&host, "ab", 2);
}

#[test]
fn processed_proposals_and_native_snapshots_never_resend_text() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(ElementKind::TextInput);
    commit(&mut engine, &mut host, "a");
    let old = echo(&mut engine, &mut host, "a");
    commit(&mut engine, &mut host, "b");
    echo(&mut engine, &mut host, "ab");
    commit(&mut engine, &mut host, "c");
    // Let the remaining expectation expire; proposal IDs still suppress replay.
    host.driver
        .runtime
        .expected_text_echoes
        .get_mut(&NodeId(9))
        .unwrap()[0]
        .expires_at = Instant::now();
    for _ in 0..8 {
        host.install_rebuild(old.clone());
        assert_text(&host, "abc", 3);
        assert!(host.drain_tree_messages().is_empty());
    }
    assert!(host.driver.runtime.expected_text_echoes.is_empty());
}

#[test]
fn style_only_patch_does_not_repropose_old_app_text() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(ElementKind::TextInput);
    commit(&mut engine, &mut host, "a");
    echo(&mut engine, &mut host, "a");
    commit(&mut engine, &mut host, "b");
    let raw: Vec<_> = 5u16
        .to_be_bytes()
        .into_iter()
        .chain(attrs("a").into_iter().skip(2))
        .chain([16])
        .chain(20f64.to_be_bytes())
        .collect();
    let bytes = [1]
        .into_iter()
        .chain(NodeId(9).to_be_bytes())
        .chain((raw.len() as u32).to_be_bytes())
        .chain(raw)
        .collect();
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::PatchTree {
            bytes,
            submitted_at: None,
        }],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    assert_text(&host, "ab", 2);
    assert!(host.drain_tree_messages().is_empty());
    let state = &host.driver.runtime.text_states[&NodeId(9)];
    assert_eq!(state.font_size, 20.0);
    assert_eq!(state.last_processed_proposal_id, 1);
    assert_eq!(
        engine
            .tree()
            .get(&NodeId(9))
            .unwrap()
            .runtime
            .content_proposal
            .as_ref()
            .unwrap()
            .id,
        1
    );
}

#[test]
fn app_replacement_publishes_once_and_later_old_echo_does_not_undo_it() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, changes) = setup(ElementKind::TextInput);
    commit(&mut engine, &mut host, "abc");
    let replacement = response(
        &mut engine,
        vec![app_patch("reset")],
        TreeUpdateDecodePolicy::ReturnErr,
    );
    host.install_rebuild(replacement.clone());
    let messages = host.drain_tree_messages();
    assert_eq!(
        messages
            .iter()
            .flat_map(TreeMsg::commands)
            .filter(|msg| matches!(msg, TreeMsg::SetTextInputContent { .. }))
            .count(),
        1
    );
    host.install_rebuild(response(
        &mut engine,
        messages,
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    host.install_rebuild(replacement);
    assert!(host.drain_tree_messages().is_empty());
    echo(&mut engine, &mut host, "abc");
    assert_text(&host, "reset", 3);
    assert_eq!(*changes.0.lock().unwrap(), ["abc"]);
}

#[test]
fn echo_preserves_unicode_selection_and_composition() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for kind in [ElementKind::TextInput, ElementKind::Multiline] {
        let (mut engine, mut host, _) = setup(kind);
        commit(&mut engine, &mut host, "é🙂");
        commit(&mut engine, &mut host, "z");
        assert!(host.handle_text_input_edit(TextInputEditRequest::MoveLeft {
            extend_selection: true
        }));
        settle(&mut engine, &mut host);
        host.handle_input(InputEvent::TextPreedit {
            text: "候".into(),
            cursor: Some((0, 1)),
        });
        settle(&mut engine, &mut host);
        let before = host.focused_text_state().unwrap();
        echo(&mut engine, &mut host, "é🙂");
        echo(&mut engine, &mut host, "é🙂z");
        let after = host.focused_text_state().unwrap();
        assert_eq!(
            (
                after.content,
                after.cursor,
                after.selection_anchor,
                after.preedit,
                after.preedit_cursor
            ),
            (
                before.content,
                before.cursor,
                before.selection_anchor,
                before.preedit,
                before.preedit_cursor
            )
        );
    }
}

#[test]
fn delayed_echo_crossing_blur_and_refocus_preserves_text() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(ElementKind::TextInput);
    commit(&mut engine, &mut host, "a");
    commit(&mut engine, &mut host, "b");
    // A lifecycle focus update does not acknowledge the controller's value.
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::SetFocusedActive {
            element_id: NodeId(9),
            active: false,
        }],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    assert!(!host.driver.runtime.text_states[&NodeId(9)].focused);
    echo(&mut engine, &mut host, "a");
    assert_text(&host, "ab", 2);
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::SetFocusedActive {
            element_id: NodeId(9),
            active: true,
        }],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    echo(&mut engine, &mut host, "ab");
    commit(&mut engine, &mut host, "c");
    assert_text(&host, "abc", 3);
}

#[test]
fn mount_replacement_drops_history_and_proposal_watermark() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(ElementKind::TextInput);
    commit(&mut engine, &mut host, "a");
    echo(&mut engine, &mut host, "a");
    commit(&mut engine, &mut host, "b");
    let fresh = setup(ElementKind::TextInput).0;
    let bytes = crate::tree::serialize::encode_tree(fresh.tree());
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::UploadTree {
            bytes,
            submitted_at: None,
        }],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    settle(&mut engine, &mut host);
    assert!(host.driver.runtime.expected_text_echoes.is_empty());
    assert_text(&host, "", 0);
    echo(&mut engine, &mut host, "new mount");
    assert_text(&host, "new mount", 0);
    assert_eq!(
        host.driver.runtime.text_states[&NodeId(9)].last_processed_proposal_id,
        1
    );
}

#[test]
fn echo_window_is_five_seconds_and_expires_at_its_boundary() {
    let id = NodeId(9);
    let now = Instant::now();
    let mut echoes = HashMap::new();
    enqueue_expected_text_echo_value(&mut echoes, id, "a".into(), now);
    assert_eq!(echoes[&id][0].expires_at, now + Duration::from_secs(5));
    assert!(consume_expected_text_echo(
        &mut echoes,
        &id,
        "a",
        now + Duration::from_millis(4999)
    ));
    enqueue_expected_text_echo_value(&mut echoes, id, "b".into(), now);
    assert!(!consume_expected_text_echo(
        &mut echoes,
        &id,
        "b",
        now + Duration::from_secs(5)
    ));
    assert!(echoes.is_empty());
    enqueue_expected_text_echo_value(&mut echoes, id, "unreturned".into(), now);
    enqueue_expected_text_echo_value(&mut echoes, id, "new".into(), now + Duration::from_secs(6));
    assert_eq!(echoes[&id].len(), 1);
    assert_eq!(echoes[&id][0].content, "new");
}

#[test]
fn first_seen_echo_after_expiry_is_an_app_replacement() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(ElementKind::TextInput);
    commit(&mut engine, &mut host, "a");
    commit(&mut engine, &mut host, "b");
    for entry in host
        .driver
        .runtime
        .expected_text_echoes
        .get_mut(&NodeId(9))
        .unwrap()
    {
        entry.expires_at = Instant::now();
    }
    echo(&mut engine, &mut host, "a");
    assert_text(&host, "a", 1);
    assert!(host.driver.runtime.expected_text_echoes.is_empty());
}

#[test]
fn inputs_without_change_handlers_ignore_native_text_snapshots() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, changes) = start(input_tree(ElementKind::TextInput, false));
    let old = response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        TreeUpdateDecodePolicy::ReturnErr,
    );
    commit(&mut engine, &mut host, "abc");
    assert!(host.driver.runtime.expected_text_echoes.is_empty());
    assert!(changes.0.lock().unwrap().is_empty());
    host.install_rebuild(old);
    assert_text(&host, "abc", 3);
    assert!(host.drain_tree_messages().is_empty());
}

#[test]
fn virtual_keys_share_the_controller_echo_rules() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut tree = input_tree(ElementKind::TextInput, true);
    tree.insert(Element::with_attrs(
        NodeId(10),
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(Length::Px(200.0)),
            height: Some(Length::Px(30.0)),
            virtual_key: Some(VirtualKeySpec {
                tap: VirtualKeyTapAction::Text("b".into()),
                hold: VirtualKeyHoldMode::None,
                hold_ms: 500,
                repeat_ms: 50,
            }),
            ..Default::default()
        },
    ));
    tree.insert(Element::with_attrs(
        NodeId(1),
        ElementKind::Column,
        vec![],
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            ..Default::default()
        },
    ));
    tree.set_children(&NodeId(1), vec![NodeId(9), NodeId(10)])
        .unwrap();
    tree.set_root_id(NodeId(1));
    tree.stamp_all_mounted_at_revision(1);
    let (mut engine, mut host, changes) = start(tree);
    for action in [ACTION_PRESS, ACTION_RELEASE] {
        host.handle_input(pointer::button(action, 10.0, 45.0));
        settle(&mut engine, &mut host);
    }
    // The very first virtual keystroke must advance the rendered caret too.
    assert_text(&host, "b", 1);
    assert_native_edit_state(&engine, &host);
    commit(&mut engine, &mut host, "a");
    echo(&mut engine, &mut host, "b");
    echo(&mut engine, &mut host, "ba");
    commit(&mut engine, &mut host, "c");
    assert_text(&host, "bac", 3);
    assert_eq!(*changes.0.lock().unwrap(), ["b", "ba", "bac"]);
}

#[test]
fn animation_geometry_updates_do_not_replay_processed_app_content() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    fn animated_attrs(text: &str) -> Vec<u8> {
        let animation: Vec<_> = 2u16
            .to_be_bytes()
            .into_iter()
            .chain([0f64, 30.0].into_iter().flat_map(|x| {
                11u32
                    .to_be_bytes()
                    .into_iter()
                    .chain(1u16.to_be_bytes())
                    .chain([31])
                    .chain(x.to_be_bytes())
            }))
            .chain(1000f64.to_be_bytes())
            .chain(6u16.to_be_bytes())
            .chain(*b"linear")
            .chain([0])
            .collect();
        5u16.to_be_bytes()
            .into_iter()
            .chain(attrs(text).into_iter().skip(2))
            .chain([65])
            .chain((animation.len() as u32).to_be_bytes())
            .chain(animation)
            .collect()
    }
    let mut tree = input_tree(ElementKind::TextInput, true);
    let input = tree.get_mut(&NodeId(9)).unwrap();
    input.spec.attrs_raw = animated_attrs("");
    input.spec.declared = crate::tree::attrs::decode_attrs(&input.spec.attrs_raw).unwrap();
    let (mut engine, mut host, changes) = start(tree);
    commit(&mut engine, &mut host, "a");
    host.install_rebuild(response(
        &mut engine,
        vec![app_attrs_patch(animated_attrs("a"))],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    settle(&mut engine, &mut host);
    commit(&mut engine, &mut host, "b");
    let before_transform = host.driver.runtime.text_states[&NodeId(9)].screen_to_local;
    let start = Instant::now();
    for ms in [16, 32, 64] {
        let effect = engine
            .process_messages(
                vec![TreeMsg::AnimationPulse {
                    presented_at: start + Duration::from_millis(ms),
                    predicted_next_present_at: start + Duration::from_millis(ms + 16),
                    trace: None,
                }],
                TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
            )
            .unwrap();
        match effect {
            TreeUpdateEffect::Layout { output, .. } if output.event_rebuild_changed => {
                host.install_rebuild(output.event_rebuild)
            }
            TreeUpdateEffect::RegistryUpdate { rebuild } => host.install_rebuild(rebuild),
            TreeUpdateEffect::Layout { .. } | TreeUpdateEffect::Skip => {}
            TreeUpdateEffect::Stop => panic!("unexpected stop"),
        }
        assert_text(&host, "ab", 2);
        assert!(host.drain_tree_messages().is_empty());
    }
    assert_eq!(*changes.0.lock().unwrap(), ["a", "ab"]);
    assert_ne!(
        host.driver.runtime.text_states[&NodeId(9)].screen_to_local,
        before_transform
    );
}

#[test]
fn actor_backed_fifo_echoes_do_not_publish_corrections_or_reorder_typing() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, _, _) = setup(ElementKind::TextInput);
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let initial = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
    let (event_tx, event_rx) = unbounded();
    let (tree_tx, tree_rx) = unbounded();
    let actor = spawn_event_actor(SpawnEventActorConfig {
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
    event_tx.send(EventMsg::registry_update(initial)).unwrap();
    for (key, expected) in [("a", "a"), ("b", "ab")] {
        event_tx
            .send(EventMsg::InputEvent(InputEvent::TextCommit {
                text: key.into(),
                mods: 0,
            }))
            .unwrap();
        let packet = tree_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            packet
                .commands()
                .iter()
                .any(|msg| matches!(msg, TreeMsg::ListenerBarrier(_)))
        );
        let rebuild = response(&mut engine, vec![packet], policy);
        assert_eq!(rebuild.text_inputs[&NodeId(9)].content, expected);
        event_tx.send(EventMsg::registry_update(rebuild)).unwrap();
    }
    // Controller responses are FIFO but arrive after both native acknowledgments.
    for value in ["a", "ab"] {
        let rebuild = response(&mut engine, vec![app_patch(value)], policy);
        event_tx.send(EventMsg::registry_update(rebuild)).unwrap();
        // A harmless release is an event-coalescing boundary, ensuring both
        // proposals are installed rather than just testing the latest echo.
        event_tx
            .send(EventMsg::InputEvent(InputEvent::Key {
                key: CanonicalKey::ArrowLeft,
                action: ACTION_RELEASE,
                mods: 0,
            }))
            .unwrap();
    }
    event_tx
        .send(EventMsg::InputEvent(InputEvent::TextCommit {
            text: "c".into(),
            mods: 0,
        }))
        .unwrap();
    let packet = tree_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let rebuild = response(&mut engine, vec![packet], policy);
    let state = &rebuild.text_inputs[&NodeId(9)];
    assert_eq!((state.content.as_str(), state.cursor), ("abc", 3));
    event_tx.send(EventMsg::registry_update(rebuild)).unwrap();
    event_tx.send(EventMsg::Stop).unwrap();
    actor.join().unwrap();
    assert!(tree_rx.is_empty());
}

#[test]
fn uploaded_input_renders_the_first_keystroke_with_caret_after_the_letter() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for kind in [ElementKind::TextInput, ElementKind::Multiline] {
        let mut engine = TreeUpdateEngine::new(ElementTree::new(), 200, 100);
        let changes = Arc::new(Changes::default());
        let mut host = HostEventRuntime::new(false, 40.0, false, changes.clone(), None);
        host.install_rebuild(response(
            &mut engine,
            vec![TreeMsg::UploadTree {
                bytes: crate::tree::serialize::encode_tree(&input_tree(kind, true)),
                submitted_at: None,
            }],
            TreeUpdateDecodePolicy::ReturnErr,
        ));
        settle(&mut engine, &mut host);
        // Focus through the real pointer path, not a pre-seeded focused fixture.
        for action in [ACTION_PRESS, ACTION_RELEASE] {
            host.handle_input(pointer::button(action, 10.0, 10.0));
            settle(&mut engine, &mut host);
        }
        assert!(host.focused_text_state().is_some());
        host.handle_input(InputEvent::TextCommit {
            text: "a".into(),
            mods: 0,
        });
        let TreeUpdateEffect::Layout { output, .. } = engine
            .process_messages(
                host.drain_tree_messages(),
                TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
            )
            .unwrap()
        else {
            panic!("typing must publish a rendered scene");
        };
        assert_eq!(output.ime_text_state.as_ref().unwrap().cursor, 1);
        let (caret_x, _, _, _) = output.ime_cursor_area.unwrap();
        let frame = engine.tree().get(&NodeId(9)).unwrap().layout.frame.unwrap();
        assert!(
            caret_x > frame.x + 1.0,
            "caret must be after 'a', not at the empty-text origin: {caret_x}"
        );
        host.install_rebuild(output.event_rebuild);
        assert_text(&host, "a", 1);
        assert_native_edit_state(&engine, &host);
        // No corrective round trip should be necessary to place the caret.
        assert!(host.drain_tree_messages().is_empty());
        echo(&mut engine, &mut host, "a");
        commit(&mut engine, &mut host, "b");
        assert!(host.handle_text_input_edit(TextInputEditRequest::MoveLeft {
            extend_selection: false,
        }));
        settle(&mut engine, &mut host);
        commit(&mut engine, &mut host, "X");
        assert_text(&host, "aXb", 2);
        assert_eq!(*changes.0.lock().unwrap(), ["a", "ab", "aXb"]);
    }
}
