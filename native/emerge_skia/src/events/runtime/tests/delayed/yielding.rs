use super::*;
use crate::runtime::host_feedback::process_host_feedback;

#[test]
fn one_registry_replay_yields_before_consuming_an_entire_nonstaling_burst() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut host = host();
    let mut tree = editable_tree(false);
    for _ in 0..128 {
        host.handle_input(InputEvent::Key {
            key: CanonicalKey::A,
            action: crate::input::ACTION_RELEASE,
            mods: 0,
        });
    }
    host.install_rebuild(native_rebuild(&mut tree));
    assert_eq!(
        host.driver.runtime.listener_lane.buffered_inputs.len(),
        64,
        "one registry install must yield after 64 replayed inputs"
    );
    assert!(host.driver.awaiting_registry.is_some());
}

struct Feedback {
    engine: TreeUpdateEngine,
    host: HostEventRuntime,
    steps: usize,
    scenes: usize,
}
impl Feedback {
    fn new() -> Self {
        let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
        let mut host = host();
        host.install_rebuild(response(
            &mut engine,
            vec![TreeMsg::RebuildRegistry],
            TreeUpdateDecodePolicy::ReturnErr,
        ));
        Self {
            engine,
            host,
            steps: 0,
            scenes: 0,
        }
    }
    fn step(&mut self, messages: Vec<TreeMsg>) -> Result<bool, String> {
        self.steps += 1;
        match self.engine.process_messages(
            messages,
            TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
        )? {
            TreeUpdateEffect::RegistryUpdate { rebuild } => self.host.install_rebuild(rebuild),
            TreeUpdateEffect::Layout { output, .. } => {
                self.scenes += 1;
                self.host.install_rebuild(output.event_rebuild);
            }
            TreeUpdateEffect::Skip => {}
            TreeUpdateEffect::Stop => return Ok(false),
        };
        Ok(true)
    }
    fn tick(&mut self, messages: Vec<TreeMsg>) -> Result<(), String> {
        process_host_feedback(self, messages, Self::step, |state| {
            state.host.drain_tree_messages()
        })
    }
}

#[test]
fn host_feedback_budget_must_not_drain_the_request_for_the_next_tick() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut feedback = Feedback::new();
    for _ in 0..20 {
        feedback.host.handle_input(InputEvent::TextCommit {
            text: "x".into(),
            mods: 0,
        });
    }
    let messages = feedback.host.drain_tree_messages();
    feedback.tick(messages).unwrap();
    assert_eq!(feedback.steps, 8);
    let next = feedback.host.drain_tree_messages();
    assert!(
        !next.is_empty(),
        "the ninth request must remain queued, not disappear at the budget boundary"
    );
}

#[test]
fn fresh_cursor_drain_stops_at_the_actor_quantum_without_consuming_stop() {
    let (tx, rx) = crossbeam_channel::unbounded();
    for index in 1..=128 {
        tx.send(EventMsg::InputEvent(InputEvent::CursorPos {
            x: index as f32,
            y: 0.0,
        }))
        .unwrap();
    }
    tx.send(EventMsg::Stop).unwrap();
    let mut pending = None;
    let first =
        drain_fresh_input_events(InputEvent::CursorPos { x: 0.0, y: 0.0 }, &rx, &mut pending);
    assert_eq!(first, vec![InputEvent::CursorPos { x: 63.0, y: 0.0 }]);
    assert!(pending.is_none());
    assert_eq!(rx.len(), 66);
    let EventMsg::InputEvent(initial) = rx.recv().unwrap() else {
        panic!("input");
    };
    assert_eq!(
        drain_fresh_input_events(initial, &rx, &mut pending),
        vec![InputEvent::CursorPos { x: 127.0, y: 0.0 }]
    );
    assert!(pending.is_none());
    let EventMsg::InputEvent(initial) = rx.recv().unwrap() else {
        panic!("input");
    };
    assert_eq!(
        drain_fresh_input_events(initial, &rx, &mut pending),
        vec![InputEvent::CursorPos { x: 128.0, y: 0.0 }]
    );
    assert!(matches!(pending, Some(EventMsg::Stop)));
    assert!(rx.is_empty());
}

#[test]
fn cached_yield_receipts_bound_host_ticks_without_constructing_scenes() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut state = Feedback::new();
    state.host.handle_input(InputEvent::Resized {
        width: 200,
        height: 30,
        scale_factor: 1.0,
    });
    for _ in 0..4096 {
        state.host.handle_input(InputEvent::Key {
            key: CanonicalKey::A,
            action: crate::input::ACTION_RELEASE,
            mods: 0,
        });
    }
    for tick in 1..=8 {
        let messages = state.host.drain_tree_messages();
        assert!(!messages.is_empty());
        state.tick(messages).unwrap();
        assert_eq!(state.steps, 8 * tick);
        assert_eq!(state.scenes, 0);
        assert_eq!(
            state
                .host
                .driver
                .runtime
                .listener_lane
                .buffered_inputs
                .len(),
            4096 - 512 * tick
        );
    }
    assert!(state.host.driver.awaiting_registry.is_none());
    assert_eq!(
        state
            .host
            .driver
            .runtime
            .listener_lane
            .buffered_inputs
            .capacity(),
        0
    );
    assert!(state.host.drain_tree_messages().is_empty());
}

#[test]
fn composition_and_new_input_stay_ordered_across_successive_yield_receipts() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut state = Feedback::new();
    state.host.handle_input(InputEvent::Resized {
        width: 200,
        height: 30,
        scale_factor: 1.0,
    });
    for index in 0..129 {
        state.host.handle_input(InputEvent::TextPreedit {
            text: format!("{index}"),
            cursor: None,
        });
    }
    let first = state.host.drain_tree_messages();
    state.step(first).unwrap();
    assert_eq!(
        state.host.focused_text_state().unwrap().preedit.as_deref(),
        Some("63")
    );
    state.host.handle_input(InputEvent::TextCommit {
        text: "Z".into(),
        mods: 0,
    });
    assert!(
        !state
            .host
            .focused_text_state()
            .unwrap()
            .content
            .contains('Z')
    );
    assert_eq!(
        state
            .host
            .driver
            .runtime
            .listener_lane
            .buffered_inputs
            .len(),
        66
    );
    let second = state.host.drain_tree_messages();
    let old = response(&mut state.engine, second, policy);
    state.host.install_rebuild(old.clone());
    assert_eq!(
        state.host.focused_text_state().unwrap().preedit.as_deref(),
        Some("127")
    );
    state.host.install_rebuild(old);
    assert_eq!(
        state
            .host
            .driver
            .runtime
            .listener_lane
            .buffered_inputs
            .len(),
        2,
        "a previous yield cannot acknowledge its successor"
    );
    let third = state.host.drain_tree_messages();
    state.step(third).unwrap();
    let text = state.host.focused_text_state().unwrap();
    assert!(text.preedit.is_none());
    assert_eq!(text.content.replace('Z', ""), "abc");
    assert_eq!(text.content.matches('Z').count(), 1);
    let final_ack = state.host.drain_tree_messages();
    state.step(final_ack).unwrap();
    assert!(state.host.driver.awaiting_registry.is_none());
    assert!(state.host.drain_tree_messages().is_empty());
}

#[test]
fn pointer_release_after_a_yield_preserves_selection_or_cancels_on_remount() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    for remount in [false, true] {
        let mut state = Feedback::new();
        state
            .host
            .handle_input(InputEvent::CursorEntered { entered: true });
        state.host.handle_input(InputEvent::CursorButton {
            button: "left".into(),
            action: ACTION_PRESS,
            mods: 0,
            x: 0.0,
            y: 10.0,
        });
        state.host.handle_input(InputEvent::Resized {
            width: 200,
            height: 30,
            scale_factor: 1.0,
        });
        for _ in 0..63 {
            state.host.handle_input(InputEvent::Key {
                key: CanonicalKey::A,
                action: crate::input::ACTION_RELEASE,
                mods: 0,
            });
        }
        state
            .host
            .handle_input(InputEvent::CursorPos { x: 150.0, y: 10.0 });
        state.host.handle_input(InputEvent::CursorButton {
            button: "left".into(),
            action: crate::input::ACTION_RELEASE,
            mods: 0,
            x: 150.0,
            y: 10.0,
        });
        let initial = state.host.drain_tree_messages();
        state.step(initial).unwrap();
        assert_eq!(
            state
                .host
                .driver
                .runtime
                .listener_lane
                .buffered_inputs
                .len(),
            1
        );
        assert!(
            state
                .host
                .driver
                .runtime
                .runtime_overlay
                .text_drag
                .is_some()
        );
        let selected = state.host.focused_text_state().unwrap();
        assert_eq!(selected.cursor, 3);
        assert_eq!(selected.selection_anchor, Some(0));
        let pending = state.host.drain_tree_messages();
        if remount {
            let raw: Vec<_> = 2u16
                .to_be_bytes()
                .into_iter()
                .chain([1, 2])
                .chain(200f64.to_be_bytes())
                .chain([2, 2])
                .chain(30f64.to_be_bytes())
                .collect();
            let attrs = crate::tree::attrs::decode_attrs(&raw).unwrap();
            let mut tree = ElementTree::new();
            tree.insert(Element::with_attrs(
                NodeId(9),
                ElementKind::TextInput,
                raw,
                attrs,
            ));
            tree.set_root_id(NodeId(9));
            let replacement = response(
                &mut state.engine,
                vec![TreeMsg::UploadTree {
                    bytes: crate::tree::serialize::encode_tree(&tree),
                    submitted_at: None,
                }],
                policy,
            );
            state.host.install_rebuild(replacement);
        }
        state.tick(pending).unwrap();
        assert!(
            state
                .host
                .driver
                .runtime
                .runtime_overlay
                .text_drag
                .is_none()
        );
        assert!(state.host.driver.awaiting_registry.is_none());
        state
            .host
            .handle_input(InputEvent::CursorPos { x: 0.0, y: 10.0 });
        if remount {
            assert!(state.host.focused_text_state().is_none());
        } else {
            assert_eq!(state.host.focused_text_state().unwrap().cursor, 3);
        }
    }
}

#[test]
fn feedback_error_or_stop_does_not_drain_undelivered_runtime_work() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for stop in [false, true] {
        let mut state = Feedback::new();
        state.host.handle_input(InputEvent::TextCommit {
            text: "X".into(),
            mods: 0,
        });
        let result = state.tick(vec![if stop {
            TreeMsg::Stop
        } else {
            TreeMsg::PatchTree {
                bytes: vec![255],
                submitted_at: None,
            }
        }]);
        assert_eq!(result.is_ok(), stop);
        let pending = state.host.drain_tree_messages();
        assert!(!pending.is_empty());
        if !stop {
            state.tick(pending).unwrap();
            assert!(state.host.driver.awaiting_registry.is_none());
            assert!(
                state
                    .host
                    .focused_text_state()
                    .unwrap()
                    .content
                    .contains('X')
            );
        }
    }
}

#[test]
fn real_actor_recovers_yielded_bursts_or_stops_without_a_yield_receipt() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for recover in [false, true] {
        let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
        let initial = response(
            &mut engine,
            vec![TreeMsg::RebuildRegistry],
            TreeUpdateDecodePolicy::ReturnErr,
        );
        let (event_tx, event_rx) = bounded(4096);
        let (tree_tx, tree_rx) = bounded(64);
        for _ in 0..2048 {
            event_tx
                .send(EventMsg::InputEvent(InputEvent::Key {
                    key: CanonicalKey::A,
                    action: crate::input::ACTION_RELEASE,
                    mods: 0,
                }))
                .unwrap();
        }
        event_tx
            .send(EventMsg::RegistryUpdate { rebuild: initial })
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
            for _ in 0..31 {
                let packet = tree_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                let TreeUpdateEffect::RegistryUpdate { rebuild } = engine
                    .process_messages(
                        vec![packet],
                        TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
                    )
                    .unwrap()
                else {
                    panic!("cached ack must not construct a scene");
                };
                event_tx.send(EventMsg::RegistryUpdate { rebuild }).unwrap();
            }
            event_tx.send(EventMsg::Stop).unwrap();
        }
        let (done, stopped) = bounded(1);
        let join = std::thread::spawn(move || {
            handle.join().unwrap();
            done.send(()).unwrap();
        });
        stopped.recv_timeout(Duration::from_secs(5)).unwrap();
        join.join().unwrap();
        let messages: Vec<_> = tree_rx.try_iter().collect();
        if recover {
            assert!(messages.is_empty());
        } else {
            assert_eq!(messages.len(), 1);
            let commands = messages[0].commands();
            assert!(
                commands
                    .iter()
                    .any(|msg| matches!(msg, TreeMsg::RebuildRegistry))
            );
            assert!(
                commands
                    .iter()
                    .any(|msg| matches!(msg, TreeMsg::ListenerBarrier(_)))
            );
        }
    }
}
