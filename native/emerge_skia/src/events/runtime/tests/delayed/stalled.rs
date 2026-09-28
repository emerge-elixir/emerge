use super::*;
thread_local! {static VISITS:std::cell::Cell<usize>=const {std::cell::Cell::new(0)};}
pub(crate) fn visit() {
    VISITS.set(VISITS.get() + 1);
}
fn reset_work() {
    VISITS.set(0);
}
fn work() -> usize {
    VISITS.get()
}

#[test]
fn stalled_enqueue_only_visits_the_new_record_not_the_retained_history() {
    let mut lane = ListenerLaneState::initially_stale();
    reset_work();
    for _ in 0..2000 {
        lane.buffer_input(InputEvent::TextCommit {
            text: "x".into(),
            mods: 0,
        });
    }
    assert_eq!(lane.buffered_inputs.len(), 2000);
    assert_eq!(work(), 2000, "enqueue must not rescan the stalled history");
}

#[test]
fn successive_receipts_do_not_reinsert_the_untouched_replay_tail() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
    let mut host = host();
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::RebuildRegistry],
        policy,
    ));
    for _ in 0..128 {
        host.handle_input(InputEvent::TextCommit {
            text: "x".into(),
            mods: 0,
        });
    }
    reset_work();
    for _ in 0..128 {
        let queued = host.drain_tree_messages();
        assert!(!queued.is_empty());
        host.install_rebuild(response(&mut engine, queued, policy));
    }
    assert_eq!(
        host.focused_text_state().unwrap().content.replace('x', ""),
        "abc"
    );
    assert_eq!(
        host.focused_text_state()
            .unwrap()
            .content
            .matches('x')
            .count(),
        128
    );
    assert!(host.driver.awaiting_registry.is_none());
    assert!(host.driver.runtime.listener_lane.buffered_inputs.is_empty());
    assert_eq!(
        work(),
        0,
        "the already coalesced tail must transfer without reinsertion"
    );
}

#[derive(Debug)]
struct QueueCharge {
    inline_queue_bytes: usize,
    records: usize,
    slot_bytes: usize,
    string_capacity_bytes: usize,
}
fn charge(lane: &ListenerLaneState) -> QueueCharge {
    QueueCharge {
        inline_queue_bytes: std::mem::size_of::<VecDeque<PendingInput>>(),
        records: lane.buffered_inputs.len(),
        slot_bytes: lane.buffered_inputs.capacity() * std::mem::size_of::<PendingInput>(),
        string_capacity_bytes: lane
            .buffered_inputs
            .iter()
            .map(|event| match event {
                PendingInput::Edit(TextInputEditRequest::Insert(text)) => text.capacity(),
                PendingInput::Command(_)
                | PendingInput::Edit(_)
                | PendingInput::ReplacementRange { .. } => 0,
                PendingInput::Raw(event) => match event {
                    InputEvent::TextCommit { text, .. } | InputEvent::TextPreedit { text, .. } => {
                        text.capacity()
                    }
                    InputEvent::CursorButton { button, .. } => button.capacity(),
                    InputEvent::CursorPos { .. }
                    | InputEvent::CursorScroll { .. }
                    | InputEvent::CursorScrollLines { .. }
                    | InputEvent::Key { .. }
                    | InputEvent::TextPreeditClear
                    | InputEvent::DeleteSurrounding { .. }
                    | InputEvent::CursorEntered { .. }
                    | InputEvent::Resized { .. }
                    | InputEvent::Focused { .. } => 0,
                },
            })
            .sum(),
    }
}

#[test]
fn finite_burst_charges_distinguish_coalescing_from_lossless_payload_retention() {
    let mut lane = ListenerLaneState::initially_stale();
    reset_work();
    for index in 0..100_000 {
        lane.buffer_input(InputEvent::CursorPos {
            x: index as f32,
            y: 0.0,
        });
    }
    let pointer = charge(&lane);
    println!("pointer_burst=100000 {pointer:?} visits={}", work());
    assert_eq!(
        pointer.inline_queue_bytes,
        std::mem::size_of::<VecDeque<PendingInput>>()
    );
    assert_eq!(pointer.records, 1);
    assert_eq!(pointer.string_capacity_bytes, 0);
    assert!(pointer.slot_bytes <= 8 * std::mem::size_of::<PendingInput>());
    drop(lane.mark_fresh_and_take_buffered());
    assert_eq!(charge(&lane).slot_bytes, 0);
    lane.mark_stale();
    reset_work();
    let pointers: Vec<_> = (0..4096)
        .map(|_| {
            let mut text = String::with_capacity(256);
            text.push_str("日本語");
            let pointer = text.as_ptr() as usize;
            lane.buffer_input(InputEvent::TextCommit { text, mods: 0 });
            pointer
        })
        .collect();
    let peak = charge(&lane);
    println!("commit_burst=4096 {peak:?} visits={}", work());
    assert_eq!(peak.records, 4096);
    assert_eq!(peak.string_capacity_bytes, 4096 * 256);
    assert_eq!(work(), 4096);
    let mut queue = lane.mark_fresh_and_take_buffered();
    for pointer in &pointers[..4095] {
        let PendingInput::Raw(InputEvent::TextCommit { text, .. }) = queue.pop_front().unwrap()
        else {
            panic!("commit");
        };
        assert_eq!(
            text.as_ptr() as usize,
            *pointer,
            "moving queue ownership must not clone strings"
        );
    }
    lane.restore_replay_tail(queue);
    let tail = charge(&lane);
    println!("remaining_tail {tail:?}");
    assert_eq!(tail.records, 1);
    assert_eq!(tail.string_capacity_bytes, 256);
    assert_eq!(
        tail.slot_bytes, peak.slot_bytes,
        "capacity persists until the queue is drained, not until each input is consumed"
    );
    let mut queue = lane.mark_fresh_and_take_buffered();
    drop(queue.pop_front().unwrap());
    lane.restore_replay_tail(queue);
    assert_eq!(charge(&lane).slot_bytes, 0);
    assert_eq!(charge(&lane).string_capacity_bytes, 0);
}

#[test]
fn adjacent_coalescing_preserves_key_composition_button_and_resize_boundaries() {
    let mut lane = ListenerLaneState::initially_stale();
    let key = InputEvent::Key {
        key: CanonicalKey::A,
        action: ACTION_PRESS,
        mods: 0,
    };
    let preedit = InputEvent::TextPreedit {
        text: "é界".into(),
        cursor: Some((1, 1)),
    };
    let button = InputEvent::CursorButton {
        button: "left".into(),
        action: crate::input::ACTION_RELEASE,
        mods: 0,
        x: 2.0,
        y: 3.0,
    };
    let resize = |width| InputEvent::Resized {
        width,
        height: 30,
        scale_factor: 2.0,
    };
    let source = vec![
        InputEvent::CursorPos { x: 0.0, y: 0.0 },
        InputEvent::CursorPos { x: 2.0, y: 3.0 },
        key.clone(),
        InputEvent::CursorScrollLines {
            dx: 1.0,
            dy: -1.0,
            x: 0.0,
            y: 0.0,
        },
        InputEvent::CursorScroll {
            dx: 2.0,
            dy: -2.0,
            x: 2.0,
            y: 3.0,
        },
        preedit.clone(),
        InputEvent::TextPreeditClear,
        InputEvent::TextCommit {
            text: "é".into(),
            mods: 0,
        },
        button.clone(),
        resize(100),
        resize(200),
        key.clone(),
        resize(300),
    ];
    for event in source {
        lane.buffer_input(event);
    }
    let actual: Vec<_> = lane
        .mark_fresh_and_take_buffered()
        .into_iter()
        .map(|event| {
            let PendingInput::Raw(event) = event else {
                panic!("expected raw input");
            };
            event
        })
        .collect();
    assert_eq!(
        actual,
        vec![
            InputEvent::CursorPos { x: 2.0, y: 3.0 },
            key.clone(),
            InputEvent::CursorScroll {
                dx: 32.0,
                dy: -32.0,
                x: 2.0,
                y: 3.0
            },
            preedit,
            InputEvent::TextPreeditClear,
            InputEvent::TextCommit {
                text: "é".into(),
                mods: 0
            },
            button,
            resize(200),
            key,
            resize(300)
        ]
    );
}

#[test]
fn replay_tail_join_coalesces_only_the_boundary_and_preserves_existing_order() {
    let mut lane = ListenerLaneState::initially_stale();
    for index in 0..64 {
        lane.buffer_input(InputEvent::Key {
            key: CanonicalKey::A,
            action: (index % 2) as u8,
            mods: 0,
        });
    }
    let mut tail = lane.mark_fresh_and_take_buffered();
    // Force a wrapped deque before appending the normalized motion/commit suffix.
    for _ in 0..48 {
        tail.pop_front();
    }
    for _ in 0..48 {
        tail.push_back(PendingInput::Raw(InputEvent::Key {
            key: CanonicalKey::B,
            action: ACTION_PRESS,
            mods: 0,
        }));
    }
    let expected: Vec<_> = tail.iter().cloned().collect();
    tail.push_front(PendingInput::Raw(InputEvent::CursorPos { x: 9.0, y: 9.0 }));
    lane.buffer_input(InputEvent::CursorPos { x: 1.0, y: 1.0 });
    reset_work();
    lane.restore_replay_tail(tail);
    assert_eq!(work(), 1);
    let queue = lane.mark_fresh_and_take_buffered();
    assert_eq!(
        queue.front(),
        Some(&PendingInput::Raw(InputEvent::CursorPos { x: 9.0, y: 9.0 }))
    );
    assert_eq!(queue.into_iter().skip(1).collect::<Vec<_>>(), expected);
}

#[derive(Default)]
struct RawCount(std::sync::atomic::AtomicUsize);
impl HostEventSink for RawCount {
    fn send_raw_input(&self, _: &InputEvent) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn send_element_event(
        &self,
        _: &NodeId,
        _: ElementEventKind,
        _: Option<&registry_builder::ElixirEventPayload>,
    ) {
    }
}
#[test]
fn large_stall_keeps_raw_observers_once_and_remount_recovery_releases_the_fifo() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for policy in [
        TreeUpdateDecodePolicy::ReturnErr,
        TreeUpdateDecodePolicy::LogAndContinue,
    ] {
        let sink = Arc::new(RawCount::default());
        let weak = Arc::downgrade(&sink);
        let mut host = HostEventRuntime::new(false, 40.0, false, sink.clone(), None);
        host.set_input_mask(u32::MAX);
        let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
        let initial = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
        host.install_rebuild(initial.clone());
        host.handle_input(InputEvent::TextCommit {
            text: "X".into(),
            mods: 0,
        });
        let queued = host.drain_tree_messages();
        assert!(!queued.is_empty());
        for _ in 0..4096 {
            host.handle_input(InputEvent::TextCommit {
                text: "界".repeat(64),
                mods: 0,
            });
        }
        let peak = charge(&host.driver.runtime.listener_lane);
        assert_eq!(peak.records, 4096);
        for _ in 0..1024 {
            host.install_rebuild(initial.clone());
        }
        assert_eq!(
            charge(&host.driver.runtime.listener_lane).string_capacity_bytes,
            peak.string_capacity_bytes
        );
        assert!(host.drain_tree_messages().is_empty());
        let raw: Vec<_> = 2u16
            .to_be_bytes()
            .into_iter()
            .chain([1, 2])
            .chain(200f64.to_be_bytes())
            .chain([2, 2])
            .chain(30f64.to_be_bytes())
            .collect();
        let attrs = crate::tree::attrs::decode_attrs(&raw).unwrap();
        let mut replacement = ElementTree::new();
        replacement.insert(Element::with_attrs(
            NodeId(9),
            ElementKind::TextInput,
            raw,
            attrs,
        ));
        replacement.set_root_id(NodeId(9));
        let new = response(
            &mut engine,
            vec![TreeMsg::UploadTree {
                bytes: crate::tree::serialize::encode_tree(&replacement),
                submitted_at: None,
            }],
            policy,
        );
        host.install_rebuild(new);
        assert_eq!(charge(&host.driver.runtime.listener_lane).records, 4096);
        host.install_rebuild(response(&mut engine, queued, policy));
        // Bounded replay leaves each continuation request queued for its own ack.
        for _ in 0..64 {
            let messages = host.drain_tree_messages();
            if messages.is_empty() {
                break;
            }
            host.install_rebuild(response(&mut engine, messages, policy));
        }
        assert!(host.driver.awaiting_registry.is_none());
        assert!(host.focused_text_state().is_none());
        assert_eq!(charge(&host.driver.runtime.listener_lane).slot_bytes, 0);
        assert_eq!(
            sink.0.load(std::sync::atomic::Ordering::Relaxed),
            4097,
            "replay is not raw observer input"
        );
        assert!(
            engine
                .tree()
                .get(&NodeId(9))
                .unwrap()
                .spec
                .declared
                .content
                .as_deref()
                .unwrap_or_default()
                .is_empty()
        );
        drop(sink);
        drop(host);
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn real_event_actor_handles_burst_recovery_or_stop_without_a_registry_ack() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    for recover in [false, true] {
        let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
        let initial = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
        let (event_tx, event_rx) = bounded(64);
        let (tree_tx, tree_rx) = bounded(64);
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
            .send(EventMsg::RegistryUpdate {
                rebuild: initial.clone(),
            })
            .unwrap();
        let count = if recover { 256 } else { 4096 };
        for _ in 0..count {
            event_tx
                .send(EventMsg::InputEvent(InputEvent::TextCommit {
                    text: "界".into(),
                    mods: 0,
                }))
                .unwrap();
        }
        event_tx
            .send(EventMsg::RegistryUpdate { rebuild: initial })
            .unwrap();
        if recover {
            for index in 1..=count {
                let packet = tree_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                assert!(
                    packet
                        .commands()
                        .iter()
                        .any(|msg| matches!(msg, TreeMsg::ListenerBarrier(_)))
                );
                let rebuild = response(&mut engine, vec![packet], policy);
                assert_eq!(
                    rebuild.text_inputs[&NodeId(9)]
                        .content
                        .matches('界')
                        .count(),
                    index
                );
                event_tx.send(EventMsg::RegistryUpdate { rebuild }).unwrap();
            }
        }
        event_tx.send(EventMsg::Stop).unwrap();
        let (done, stopped) = bounded(1);
        let join = std::thread::spawn(move || {
            handle.join().unwrap();
            done.send(()).unwrap();
        });
        stopped.recv_timeout(Duration::from_secs(5)).unwrap();
        join.join().unwrap();
        if recover {
            assert!(tree_rx.is_empty());
        } else {
            let messages: Vec<_> = tree_rx.try_iter().collect();
            let latest = response(&mut engine, messages, policy);
            assert_eq!(
                latest.text_inputs[&NodeId(9)].content.matches('界').count(),
                1,
                "Stop must not require replaying the retained burst"
            );
        }
    }
}
