use super::*;

#[test]
fn full_outbound_tree_channel_does_not_prevent_event_actor_stop() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut tree = editable_tree(false);
    let initial = native_rebuild(&mut tree);
    let (event_tx, event_rx) = bounded(16);
    let (tree_tx, tree_rx) = bounded(1);
    tree_tx.send(TreeMsg::RebuildRegistry).unwrap();
    event_tx
        .send(EventMsg::RegistryUpdate { rebuild: initial })
        .unwrap();
    event_tx
        .send(EventMsg::InputEvent(InputEvent::TextCommit {
            text: "X".into(),
            mods: 0,
        }))
        .unwrap();
    event_tx.send(EventMsg::Stop).unwrap();
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
    let (done, stopped) = bounded(1);
    let join = std::thread::spawn(move || {
        handle.join().unwrap();
        done.send(()).unwrap();
    });
    let result = stopped.recv_timeout(Duration::from_secs(5));
    // Unblock the old implementation before asserting, so a failed regression
    // still joins its native threads rather than stranding the test process.
    drop(tree_rx);
    if result.is_err() {
        stopped.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    join.join().unwrap();
    assert!(
        result.is_ok(),
        "a full outbound channel must not prevent Stop"
    );
}

fn make_driver(tree_tx: Sender<TreeMsg>) -> EventRuntimeDriver {
    EventRuntimeDriver::new(
        false,
        None,
        BackendWakeHandle::noop(),
        tree_tx,
        false,
        Arc::new(NativeLogRelay::default()),
        None,
    )
}
fn barrier(packet: &TreeMsg) -> &crate::actors::ListenerBarrier {
    packet
        .commands()
        .into_iter()
        .find_map(|message| match message {
            TreeMsg::ListenerBarrier(barrier) => Some(barrier),
            _ => None,
        })
        .unwrap()
}

#[test]
fn host_drains_channel_then_deferred_operations_without_clones_or_overtaking() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for policy in [
        TreeUpdateDecodePolicy::ReturnErr,
        TreeUpdateDecodePolicy::LogAndContinue,
    ] {
        let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
        let initial = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
        let mut host = host();
        host.install_rebuild(initial);
        for _ in 0..1280 {
            assert!(
                host.handle_text_input_edit(crate::events::TextInputEditRequest::Insert(
                    "X".into()
                ))
            );
        }
        assert_eq!(host.tree_rx.len(), 512);
        assert_eq!(host.driver.outbox.len(), 768);
        let first = barrier(host.driver.outbox.front().unwrap()).weak_identity();
        let pointer = host
            .driver
            .outbox
            .front()
            .unwrap()
            .commands()
            .into_iter()
            .find_map(|msg| match msg {
                TreeMsg::SetTextInputContent { content, .. } => Some(content.as_ptr()),
                _ => None,
            })
            .unwrap();
        println!(
            "host packets: channel={} outbox={} outbox_slot_capacity={} slot_bytes={} (excludes nested allocations/allocator overhead)",
            host.tree_rx.len(),
            host.driver.outbox.len(),
            host.driver.outbox.capacity(),
            host.driver.outbox.capacity() * std::mem::size_of::<TreeMsg>()
        );
        // Free a channel slot, but a newer operation must still join the tail.
        let prefix = host.tree_rx.recv().unwrap();
        assert!(
            host.handle_text_input_edit(crate::events::TextInputEditRequest::Insert("Y".into()))
        );
        assert_eq!(host.tree_rx.len(), 511);
        assert_eq!(host.driver.outbox.len(), 769);
        let batches: Vec<_> = (0..3).map(|_| host.drain_tree_messages()).collect();
        assert_eq!(
            batches.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![512, 512, 256]
        );
        assert!(host.drain_tree_messages().is_empty());
        let packets: Vec<_> = std::iter::once(prefix)
            .chain(batches.into_iter().flatten())
            .collect();
        assert_eq!(packets.len(), 1281);
        assert_eq!(host.driver.outbox.capacity(), 0);
        let contents: Vec<_> = packets
            .iter()
            .flat_map(TreeMsg::commands)
            .filter_map(|msg| match msg {
                TreeMsg::SetTextInputContent { content, .. } => Some(content),
                _ => None,
            })
            .collect();
        assert_eq!(
            contents
                .iter()
                .map(|content| content.len())
                .collect::<Vec<_>>(),
            (4..=1284).collect::<Vec<_>>()
        );
        assert_eq!(
            contents[512].as_ptr(),
            pointer,
            "payload ownership moves without cloning"
        );
        assert!(first.upgrade().is_some());
        drop(contents);
        host.install_rebuild(response(&mut engine, packets, policy));
        assert!(
            first.upgrade().is_none(),
            "earlier receipt not retained as outbox history"
        );
        assert!(host.driver.awaiting_registry.is_none());
        assert!(host.drain_tree_messages().is_empty());
        let text = host.focused_text_state().unwrap().content;
        assert_eq!(text.matches('X').count(), 1280);
        assert_eq!(text.matches('Y').count(), 1);
    }
}

#[test]
fn queued_packet_receipt_cannot_be_acknowledged_before_delivery() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
    let initial = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
    let (tx, rx) = bounded(1);
    tx.send(TreeMsg::RebuildRegistry).unwrap();
    let mut driver = make_driver(tx);
    driver.install_rebuild(initial.clone());
    driver.handle_input(InputEvent::TextCommit {
        text: "X".into(),
        mods: 0,
    });
    driver.handle_input(InputEvent::TextCommit {
        text: "Y".into(),
        mods: 0,
    });
    assert_eq!(driver.outbox.len(), 1);
    let receipt = barrier(driver.outbox.front().unwrap()).clone();
    driver.install_rebuild(initial);
    assert!(driver.awaiting_registry.as_ref().unwrap().matches(&receipt));
    assert_eq!(driver.runtime.listener_lane.buffered_inputs.len(), 1);
    rx.recv().unwrap();
    let packet = driver.outbox.pop_front().unwrap();
    driver.install_rebuild(response(&mut engine, vec![packet], policy));
    assert_eq!(driver.runtime.listener_lane.buffered_inputs.len(), 0);
    let next = rx.recv().unwrap();
    assert!(!barrier(&next).matches(&receipt));
    driver.install_rebuild(response(&mut engine, vec![next], policy));
    assert!(driver.awaiting_registry.is_none());
}

struct RawSignal(Sender<InputEvent>);
impl HostEventSink for RawSignal {
    fn send_raw_input(&self, event: &InputEvent) {
        self.0.send(event.clone()).unwrap();
    }
    fn send_element_event(&self, _: &NodeId, _: ElementEventKind, _: Option<&ElixirEventPayload>) {}
}

#[test]
fn real_outbound_recovery_preserves_input_order_and_rejects_old_mount_packets() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for policy in [
        TreeUpdateDecodePolicy::ReturnErr,
        TreeUpdateDecodePolicy::LogAndContinue,
    ] {
        for remount in [false, true] {
            let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
            let initial = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
            let (event_tx, event_rx) = bounded(16);
            let (tree_tx, tree_rx) = bounded(1);
            tree_tx.send(TreeMsg::RebuildRegistry).unwrap();
            let (raw_tx, raw_rx) = crossbeam_channel::unbounded();
            let context = assets.context();
            let handle = std::thread::spawn(move || {
                let _assets = context.enter();
                let mut driver = make_driver(tree_tx);
                driver.runtime.host_event_sink = Some(Arc::new(RawSignal(raw_tx)));
                driver.set_input_mask(u32::MAX);
                run_event_actor(driver, event_rx);
            });
            event_tx
                .send(EventMsg::RegistryUpdate { rebuild: initial })
                .unwrap();
            for text in ["X", "Y"] {
                event_tx
                    .send(EventMsg::InputEvent(InputEvent::TextCommit {
                        text: text.into(),
                        mods: 0,
                    }))
                    .unwrap();
            }
            // Seeing Y proves dispatch returned from X while the outbound slot was
            // still full. A sleep or a timeout-only assertion would not prove this.
            for text in ["X", "Y"] {
                assert_eq!(
                    raw_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
                    InputEvent::TextCommit {
                        text: text.into(),
                        mods: 0
                    }
                );
            }
            assert_eq!(tree_rx.len(), 1);
            tree_rx.recv().unwrap();
            let packet = tree_rx.recv_timeout(Duration::from_secs(5)).unwrap();
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
                let rebuild = response(
                    &mut engine,
                    vec![TreeMsg::UploadTree {
                        bytes: crate::tree::serialize::encode_tree(&tree),
                        submitted_at: None,
                    }],
                    policy,
                );
                event_tx.send(EventMsg::RegistryUpdate { rebuild }).unwrap();
            }
            let rebuild = response(&mut engine, vec![packet], policy);
            event_tx.send(EventMsg::RegistryUpdate { rebuild }).unwrap();
            if !remount {
                let next = tree_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                let rebuild = response(&mut engine, vec![next], policy);
                event_tx.send(EventMsg::RegistryUpdate { rebuild }).unwrap();
                assert_eq!(
                    engine
                        .tree()
                        .get(&NodeId(9))
                        .unwrap()
                        .spec
                        .declared
                        .content
                        .as_ref()
                        .unwrap()
                        .replace("XY", ""),
                    "abc"
                );
            } else {
                assert!(
                    engine
                        .tree()
                        .get(&NodeId(9))
                        .unwrap()
                        .spec
                        .declared
                        .content
                        .is_none()
                );
            }
            event_tx.send(EventMsg::Stop).unwrap();
            handle.join().unwrap();
            assert!(tree_rx.is_empty());
            assert!(raw_rx.is_empty(), "replay is not a second raw observation");
        }
    }
}

#[test]
fn stop_and_both_peer_disconnections_drop_pending_packets_and_input_synchronously() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for mode in 0..3 {
        let mut tree = editable_tree(false);
        let initial = native_rebuild(&mut tree);
        let context = assets.context();
        let (event_tx, event_rx) = bounded(1);
        let (tree_tx, tree_rx) = bounded(1);
        tree_tx.send(TreeMsg::RebuildRegistry).unwrap();
        let (ready, weak_rx) = bounded(1);
        let (done, stopped) = bounded(1);
        let handle = std::thread::spawn(move || {
            let _assets = context.enter();
            let mut driver = make_driver(tree_tx);
            driver.install_rebuild(initial);
            driver.handle_input(InputEvent::TextCommit {
                text: "X".into(),
                mods: 0,
            });
            for _ in 0..1024 {
                driver.handle_input(InputEvent::TextCommit {
                    text: "界".repeat(64),
                    mods: 0,
                });
            }
            assert_eq!(driver.outbox.len(), 1);
            assert_eq!(driver.runtime.listener_lane.buffered_inputs.len(), 1024);
            ready
                .send(barrier(driver.outbox.front().unwrap()).weak_identity())
                .unwrap();
            run_event_actor(driver, event_rx);
            done.send(()).unwrap();
        });
        let weak = weak_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(weak.upgrade().is_some());
        let mut event_tx = Some(event_tx);
        let mut tree_rx = Some(tree_rx);
        match mode {
            0 => event_tx.as_ref().unwrap().send(EventMsg::Stop).unwrap(),
            1 => {
                event_tx.take();
            }
            _ => {
                tree_rx.take();
            }
        }
        let result = stopped.recv_timeout(Duration::from_secs(5));
        if result.is_err() {
            event_tx.take();
            tree_rx.take();
            stopped.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        handle.join().unwrap();
        assert!(result.is_ok());
        assert!(
            weak.upgrade().is_none(),
            "pending packet and awaiting receipt released before thread completion"
        );
    }
}

#[test]
fn real_actor_sends_a_nonstaling_outbox_fifo_without_overtaking_or_replaying_raw_input() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
    let initial = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
    let (event_tx, event_rx) = bounded(256);
    let (tree_tx, tree_rx) = bounded(1);
    tree_tx.send(TreeMsg::RebuildRegistry).unwrap();
    let (raw_tx, raw_rx) = crossbeam_channel::unbounded();
    let context = assets.context();
    let handle = std::thread::spawn(move || {
        let _assets = context.enter();
        let mut driver = make_driver(tree_tx);
        driver.runtime.host_event_sink = Some(Arc::new(RawSignal(raw_tx)));
        driver.set_input_mask(u32::MAX);
        run_event_actor(driver, event_rx);
    });
    event_tx
        .send(EventMsg::RegistryUpdate { rebuild: initial })
        .unwrap();
    for index in 0..128 {
        event_tx
            .send(EventMsg::InputEvent(InputEvent::TextPreedit {
                text: format!("{index}"),
                cursor: None,
            }))
            .unwrap();
    }
    for index in 0..128 {
        assert_eq!(
            raw_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            InputEvent::TextPreedit {
                text: format!("{index}"),
                cursor: None
            }
        );
    }
    tree_rx.recv().unwrap();
    let packets: Vec<_> = (0..128)
        .map(|_| tree_rx.recv_timeout(Duration::from_secs(5)).unwrap())
        .collect();
    let preedits: Vec<_> = packets
        .iter()
        .flat_map(TreeMsg::commands)
        .filter_map(|msg| match msg {
            TreeMsg::SetTextInputRuntime { preedit, .. } => preedit.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(
        preedits,
        (0..128).map(|index| format!("{index}")).collect::<Vec<_>>()
    );
    let rebuild = response(&mut engine, packets, policy);
    event_tx.send(EventMsg::RegistryUpdate { rebuild }).unwrap();
    event_tx.send(EventMsg::Stop).unwrap();
    handle.join().unwrap();
    assert!(tree_rx.is_empty());
    assert!(raw_rx.is_empty());
}

struct HoldSignal(Sender<()>);
impl HostEventSink for HoldSignal {
    fn send_raw_input(&self, _: &InputEvent) {}
    fn send_element_event(
        &self,
        _: &NodeId,
        kind: ElementEventKind,
        _: Option<&ElixirEventPayload>,
    ) {
        if matches!(kind, ElementEventKind::VirtualKeyHold) {
            self.0.send(()).unwrap();
        }
    }
}
#[test]
fn due_virtual_key_timer_and_stop_remain_selectable_while_output_is_full() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut tree = ElementTree::new();
    tree.insert(Element::with_attrs(
        NodeId(9),
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Px(30.0)),
            virtual_key: Some(VirtualKeySpec {
                tap: VirtualKeyTapAction::Text("x".into()),
                hold: VirtualKeyHoldMode::Event,
                hold_ms: 350,
                repeat_ms: 40,
            }),
            ..Attrs::default()
        },
    ));
    tree.set_root_id(NodeId(9));
    tree.stamp_all_mounted_at_revision(1);
    let initial = native_rebuild(&mut tree);
    let (event_tx, event_rx) = bounded(1);
    let (tree_tx, tree_rx) = bounded(1);
    tree_tx.send(TreeMsg::RebuildRegistry).unwrap();
    let (held, hold_rx) = bounded(1);
    let (done, stopped) = bounded(1);
    let context = assets.context();
    let handle = std::thread::spawn(move || {
        let _assets = context.enter();
        let mut driver = make_driver(tree_tx);
        driver.runtime.host_event_sink = Some(Arc::new(HoldSignal(held)));
        driver.install_rebuild(initial);
        driver.handle_input(InputEvent::CursorButton {
            button: "left".into(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });
        assert!(driver.runtime.runtime_overlay.virtual_key.is_some());
        driver.queue_tree_packet(TreeMsg::RebuildRegistry);
        driver.runtime.virtual_key_deadline = Some(Instant::now());
        run_event_actor(driver, event_rx);
        done.send(()).unwrap();
    });
    let held = hold_rx.recv_timeout(Duration::from_secs(5));
    event_tx.send(EventMsg::Stop).unwrap();
    let result = stopped.recv_timeout(Duration::from_secs(5));
    drop(tree_rx);
    if result.is_err() {
        stopped.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    handle.join().unwrap();
    assert!(held.is_ok());
    assert!(result.is_ok());
}

#[test]
fn host_feedback_preserves_deferred_work_across_both_budgets_and_keeps_new_input_behind_it() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let policy = TreeUpdateDecodePolicy::ReturnErr;
    let mut engine = TreeUpdateEngine::new(editable_tree(false), 200, 30);
    let initial = response(&mut engine, vec![TreeMsg::RebuildRegistry], policy);
    let mut host = host();
    host.install_rebuild(initial);
    for index in 0..4097 {
        assert!(
            host.handle_text_input_edit(crate::events::TextInputEditRequest::Insert("X".into()))
        );
        if index == 0 {
            assert_eq!(
                host.driver.outbox.capacity(),
                0,
                "immediate send needs no outbox allocation"
            );
        }
    }
    let rounds = std::cell::Cell::new(0);
    let first = host.drain_tree_messages();
    crate::runtime::host_feedback::process_host_feedback(
        &mut (&mut host, &mut engine),
        first,
        |(host, engine), messages| {
            rounds.set(rounds.get() + 1);
            host.install_rebuild(response(engine, messages, policy));
            Ok::<_, String>(true)
        },
        |(host, _)| host.drain_tree_messages(),
    )
    .unwrap();
    assert_eq!(rounds.get(), 8);
    assert_eq!(host.driver.outbox.len(), 1);
    assert!(host.tree_rx.is_empty());
    assert_eq!(
        engine
            .tree()
            .get(&NodeId(9))
            .unwrap()
            .spec
            .declared
            .content
            .as_ref()
            .unwrap()
            .matches('X')
            .count(),
        4096
    );
    assert!(host.driver.awaiting_registry.is_some());
    host.handle_input(InputEvent::TextCommit {
        text: "Z".into(),
        mods: 0,
    });
    assert_eq!(host.driver.runtime.listener_lane.buffered_inputs.len(), 1);
    assert!(!host.focused_text_state().unwrap().content.contains('Z'));
    let next = host.drain_tree_messages();
    assert_eq!(next.len(), 1);
    crate::runtime::host_feedback::process_host_feedback(
        &mut (&mut host, &mut engine),
        next,
        |(host, engine), messages| {
            rounds.set(rounds.get() + 1);
            host.install_rebuild(response(engine, messages, policy));
            Ok::<_, String>(true)
        },
        |(host, _)| host.drain_tree_messages(),
    )
    .unwrap();
    assert_eq!(rounds.get(), 10);
    assert!(host.driver.awaiting_registry.is_none());
    assert_eq!(host.driver.outbox.capacity(), 0);
    assert_eq!(
        host.focused_text_state()
            .unwrap()
            .content
            .replace(&format!("{}Z", "X".repeat(4097)), ""),
        "abc"
    );
}
