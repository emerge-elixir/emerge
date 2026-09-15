//! Isolated release probe. Counters are test-only and included in measured work.
use super::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Probe {
    ready: AtomicBool,
    idle: AtomicBool,
    finished: AtomicBool,
    raw: AtomicUsize,
    callbacks: AtomicUsize,
    buffered: AtomicUsize,
    buffered_peak: AtomicUsize,
    slot_peak: AtomicUsize,
    outbox_peak: AtomicUsize,
    event_peak: AtomicUsize,
    pause: Receiver<(Duration, Sender<()>)>,
}
impl Probe {
    fn reset(&self) {
        for value in [
            &self.raw,
            &self.callbacks,
            &self.buffered_peak,
            &self.slot_peak,
            &self.outbox_peak,
            &self.event_peak,
        ] {
            value.store(0, Ordering::Relaxed);
        }
    }
}
impl HostEventSink for Probe {
    fn send_raw_input(&self, _: &InputEvent) {
        self.raw.fetch_add(1, Ordering::Relaxed);
    }
    fn send_element_event(&self, _: &NodeId, _: ElementEventKind, _: Option<&ElixirEventPayload>) {
        self.callbacks.fetch_add(1, Ordering::Relaxed);
    }
}
thread_local! {static PROBE:RefCell<Option<Arc<Probe>>>=const{RefCell::new(None)};}
pub(in crate::events::runtime) fn sample(driver: &EventRuntimeDriver, rx: &Receiver<EventMsg>) {
    PROBE.with(|slot| {
        let slot = slot.borrow();
        let Some(probe) = slot.as_ref() else {
            return;
        };
        let lane = &driver.runtime.listener_lane;
        probe
            .buffered
            .store(lane.buffered_inputs.len(), Ordering::Relaxed);
        probe
            .buffered_peak
            .fetch_max(lane.buffered_inputs.len(), Ordering::Relaxed);
        probe.slot_peak.fetch_max(
            lane.buffered_inputs.capacity() * std::mem::size_of::<InputEvent>(),
            Ordering::Relaxed,
        );
        probe
            .outbox_peak
            .fetch_max(driver.outbox.len(), Ordering::Relaxed);
        probe.event_peak.fetch_max(rx.len(), Ordering::Relaxed);
        probe.ready.store(
            driver.runtime.focused_id == Some(NodeId(9)),
            Ordering::Relaxed,
        );
        probe.idle.store(
            rx.is_empty()
                && lane.buffered_inputs.is_empty()
                && driver.outbox.is_empty()
                && driver.awaiting_registry.is_none(),
            Ordering::Relaxed,
        );
        probe.finished.store(
            probe.idle.load(Ordering::Relaxed)
                && !driver
                    .runtime
                    .input_handler
                    .accepts(&InputEvent::CursorPos { x: 0.0, y: 0.0 }),
            Ordering::Relaxed,
        );
        if let Ok((duration, ack)) = probe.pause.try_recv() {
            ack.send(()).unwrap();
            std::thread::sleep(duration);
        }
    });
}

fn fixture(nodes: usize) -> ElementTree {
    let mut tree = editable_tree(false);
    tree.get_mut(&NodeId(9))
        .unwrap()
        .spec
        .declared
        .on_mouse_move = Some(true);
    if nodes > 1 {
        tree.insert(Element::with_attrs(
            NodeId(1),
            ElementKind::Column,
            vec![],
            Attrs {
                width: Some(Length::Fill),
                height: Some(Length::Fill),
                ..Attrs::default()
            },
        ));
        for id in 10..(nodes as u64 + 8) {
            tree.insert(Element::with_attrs(
                NodeId(id),
                ElementKind::El,
                vec![],
                Attrs {
                    width: Some(Length::Px(8.0)),
                    height: Some(Length::Px(8.0)),
                    on_mouse_move: Some(true),
                    ..Attrs::default()
                },
            ));
        }
        tree.set_children(
            &NodeId(1),
            std::iter::once(NodeId(9))
                .chain((10..nodes as u64 + 8).map(NodeId))
                .collect(),
        )
        .unwrap();
        tree.set_root_id(NodeId(1));
    }
    tree.stamp_all_mounted_at_revision(1);
    tree
}
fn input(kind: &str, index: usize) -> InputEvent {
    match kind {
        "edit" if index.is_multiple_of(2) => InputEvent::TextCommit {
            text: "x".into(),
            mods: 0,
        },
        "edit" => InputEvent::Key {
            key: CanonicalKey::Backspace,
            action: ACTION_PRESS,
            mods: 0,
        },
        "pointer" => InputEvent::CursorPos {
            x: 10.0 + (index % 20) as f32,
            y: 10.0,
        },
        "ime" => InputEvent::TextPreedit {
            text: format!("{index:032}"),
            cursor: None,
        },
        _ => panic!("unknown input kind"),
    }
}
fn wait_until(predicate: impl Fn() -> bool, timeout: Duration) -> bool {
    let end = Instant::now() + timeout;
    while !predicate() {
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    true
}

#[test]
#[ignore = "exclusive-lock release measurement only"]
fn isolated_event_pressure_probe() {
    let args = std::env::var("EMERGE_EVENT_PROBE").expect("nodes,kind,rate,count,stall,ms");
    let args: Vec<_> = args.split(',').collect();
    let nodes: usize = args[0].parse().unwrap();
    let kind = args[1];
    let rate: usize = args[2].parse().unwrap();
    let count: usize = args[3].parse().unwrap();
    let stall = args[4];
    let stall_ms: u64 = args[5].parse().unwrap();
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut tree = fixture(nodes);
    let initial = crate::tree::layout::layout_and_refresh_default(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
    )
    .event_rebuild;
    let (event_tx, event_rx) = bounded(4096);
    let (tree_tx, tree_rx) = bounded(512);
    let (inner_tx, inner_rx) = bounded(512);
    let (render_tx, renders) = bounded(1);
    let tree_handle = crate::runtime::tree_actor::spawn_tree_actor_with_initial_tree(
        inner_rx,
        crate::runtime::tree_actor::TreeActorConfig {
            render_sender: crate::RenderSender {
                tx: render_tx,
                drop_rx: renders.clone(),
                log_render: false,
            },
            event_tx: event_tx.clone(),
            render_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            stats: None,
            log_input: false,
            window_wake: BackendWakeHandle::noop(),
            initial_width: 800,
            initial_height: 600,
            asset_context: assets.context(),
        },
        tree,
    );
    let (gate_tx, gate_rx) = bounded::<(Duration, Sender<()>)>(1);
    let forwarder = std::thread::spawn(move || {
        loop {
            crossbeam_channel::select! {
                recv(gate_rx)->pause=>match pause {Ok((duration,ack))=>{ack.send(()).unwrap();std::thread::sleep(duration);},Err(_)=>return},
                recv(tree_rx)->packet=>match packet {Ok(packet)=>{let stop=matches!(packet,TreeMsg::Stop);if inner_tx.send(packet).is_err()||stop{return;}},Err(_)=>return},
            }
        }
    });
    let (pause_tx, pause_rx) = bounded(1);
    let probe = Arc::new(Probe {
        ready: AtomicBool::new(false),
        idle: AtomicBool::new(false),
        finished: AtomicBool::new(false),
        raw: AtomicUsize::new(0),
        callbacks: AtomicUsize::new(0),
        buffered: AtomicUsize::new(0),
        buffered_peak: AtomicUsize::new(0),
        slot_peak: AtomicUsize::new(0),
        outbox_peak: AtomicUsize::new(0),
        event_peak: AtomicUsize::new(0),
        pause: pause_rx,
    });
    let actor_probe = probe.clone();
    let context = assets.context();
    let actor_tx = tree_tx.clone();
    let actor = std::thread::spawn(move || {
        let _assets = context.enter();
        PROBE.with(|slot| *slot.borrow_mut() = Some(actor_probe.clone()));
        let mut driver = EventRuntimeDriver::new(
            false,
            None,
            BackendWakeHandle::noop(),
            actor_tx,
            false,
            Arc::new(NativeLogRelay::default()),
            None,
        );
        driver.runtime.host_event_sink = Some(actor_probe);
        driver.set_input_mask(u32::MAX);
        driver.install_rebuild(initial);
        run_event_actor(driver, event_rx);
        PROBE.with(|slot| slot.borrow_mut().take());
    });
    assert!(wait_until(
        || probe.ready.load(Ordering::Relaxed),
        Duration::from_secs(10)
    ));
    for index in 0..20 {
        event_tx
            .send(EventMsg::InputEvent(input("edit", index)))
            .unwrap();
    }
    assert!(wait_until(
        || probe.raw.load(Ordering::Relaxed) >= 20 && probe.idle.load(Ordering::Relaxed),
        Duration::from_secs(10)
    ));
    probe.reset();
    let (ack, acked) = bounded(1);
    match stall {
        "tree" => gate_tx
            .send((Duration::from_millis(stall_ms), ack))
            .unwrap(),
        "event" => {
            pause_tx
                .send((Duration::from_millis(stall_ms), ack))
                .unwrap();
            event_tx.send(EventMsg::SetInputMask(u32::MAX)).unwrap();
        }
        "none" => ack.send(()).unwrap(),
        _ => panic!("unknown stall"),
    }
    acked.recv().unwrap();
    // Force a genuine causal wait for the stalled-tree pointer case. Otherwise
    // ordinary MouseMove does not need the tree at all.
    let bootstrap = usize::from(stall == "tree" && kind == "pointer");
    if bootstrap == 1 {
        event_tx
            .send(EventMsg::InputEvent(input("edit", 0)))
            .unwrap();
    }
    let started = Instant::now();
    let mut accepted = 0usize;
    let mut full = 0usize;
    for index in 0..count {
        if rate > 0 {
            let due = started + Duration::from_secs_f64(index as f64 / rate as f64);
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            }
        }
        match event_tx.try_send(EventMsg::InputEvent(input(kind, index))) {
            Ok(()) => accepted += 1,
            Err(TrySendError::Full(_)) => full += 1,
            Err(TrySendError::Disconnected(_)) => panic!("actor disconnected"),
        }
        probe
            .event_peak
            .fetch_max(event_tx.len(), Ordering::Relaxed);
    }
    let produce_seconds = started.elapsed().as_secs_f64();
    let buffered_at_end = probe.buffered.load(Ordering::Relaxed);
    // A final FIFO control disables raw observation. The loop samples idle only
    // after consuming it, avoiding an old idle flag racing the final dispatch.
    let settling = Instant::now();
    event_tx.send(EventMsg::SetInputMask(0)).unwrap();
    let settled = wait_until(
        || probe.finished.load(Ordering::Relaxed),
        Duration::from_secs(2).saturating_sub(settling.elapsed()),
    );
    let settle_seconds = settling.elapsed().as_secs_f64();
    let remaining = probe.buffered.load(Ordering::Relaxed);
    let shutdown = Instant::now();
    event_tx.send(EventMsg::Stop).unwrap();
    actor.join().unwrap();
    tree_tx.send(TreeMsg::Stop).unwrap();
    forwarder.join().unwrap();
    tree_handle.join().unwrap();
    let shutdown_seconds = shutdown.elapsed().as_secs_f64();
    println!(
        "PROBE {{\"nodes\":{nodes},\"kind\":\"{kind}\",\"rate\":{rate},\"count\":{count},\"stall\":\"{stall}\",\"stall_ms\":{stall_ms},\"accepted\":{accepted},\"full\":{full},\"produce_seconds\":{produce_seconds},\"raw\":{},\"callbacks\":{},\"event_queue_peak\":{},\"buffered_peak\":{},\"buffered_slots_bytes_peak\":{},\"outbox_peak\":{},\"buffered_at_producer_end\":{buffered_at_end},\"settled\":{settled},\"settle_seconds\":{settle_seconds},\"buffered_at_recovery_cutoff\":{remaining},\"shutdown_seconds\":{shutdown_seconds}}}",
        probe.raw.load(Ordering::Relaxed),
        probe.callbacks.load(Ordering::Relaxed),
        probe.event_peak.load(Ordering::Relaxed),
        probe.buffered_peak.load(Ordering::Relaxed),
        probe.slot_peak.load(Ordering::Relaxed),
        probe.outbox_peak.load(Ordering::Relaxed)
    );
}
