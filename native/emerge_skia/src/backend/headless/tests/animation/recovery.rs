use super::*;

struct FaultRenderer {
    tx: Sender<RenderMsg>,
    fail: Arc<AtomicBool>,
    attempts: Receiver<(bool, usize)>,
    outputs: Receiver<Vec<u8>>,
    pulses: Receiver<TreeMsg>,
    handle: Option<thread::JoinHandle<()>>,
}
impl FaultRenderer {
    fn new(context: assets::AssetContext, after_paint: bool) -> Self {
        let (tx, rx) = bounded(64);
        let (tree_tx, pulses) = unbounded();
        let (output, outputs) = unbounded();
        let (attempt, attempts) = unbounded();
        let fail = Arc::new(AtomicBool::new(false));
        let failing = fail.clone();
        let handle = thread::spawn(move || {
            let _assets = context.enter();
            let mut renderer = raster();
            let running = AtomicBool::new(true);
            binary::run_binary_loop(
                &rx,
                &tree_tx,
                &running,
                "rgba",
                Duration::from_millis(16),
                |state, _| {
                    let fail = failing.load(Ordering::SeqCst);
                    let frame = (!fail || after_paint).then(|| renderer.render(state));
                    if fail {
                        attempt
                            .send((state.animate, state.scene.summary().text_bytes))
                            .unwrap();
                        Err("controlled before/after native raster draw failure".into())
                    } else {
                        output.send(frame.unwrap().data).unwrap();
                        Ok(())
                    }
                },
            );
        });
        Self {
            tx,
            fail,
            attempts,
            outputs,
            pulses,
            handle: Some(handle),
        }
    }
}
impl Drop for FaultRenderer {
    fn drop(&mut self) {
        let _ = self.tx.send(RenderMsg::Stop);
        self.handle.take().unwrap().join().unwrap();
    }
}

#[test]
fn interaction_and_remount_during_failed_output_recover_only_latest_native_pixels() {
    let assets = assets::AssetRuntime::new();
    let _assets = assets.enter();
    for after_paint in [false, true] {
        for backlog in [1, 8, 32] {
            for remount in [false, true] {
                let worker = FaultRenderer::new(assets.context(), after_paint);
                let mut h = Harness::new(assets.context());
                h.render_tap = Some(worker.tx.clone());
                let now = Instant::now();
                let pulse = |us| TreeMsg::AnimationPulse {
                    presented_at: now + Duration::from_micros(us),
                    predicted_next_present_at: now + Duration::from_micros(us),
                    trace: None,
                };
                h.round_trip(vec![pulse(0)]);
                let initial = (0..h.frames)
                    .fold(None, |_, _| {
                        Some(worker.outputs.recv_timeout(Duration::from_secs(5)).unwrap())
                    })
                    .unwrap();
                assert_eq!(initial, h.last_pixels);
                worker.fail.store(true, Ordering::SeqCst);
                h.input(InputEvent::CursorEntered { entered: true });
                h.input(InputEvent::CursorPos { x: 10.0, y: 10.0 });
                h.input(InputEvent::CursorButton {
                    button: "left".into(),
                    action: crate::input::ACTION_PRESS,
                    mods: 0,
                    x: 10.0,
                    y: 10.0,
                });
                h.direct_events.handle_input(InputEvent::TextCommit {
                    text: "X".into(),
                    mods: 0,
                });
                h.actor_events.handle_input(InputEvent::TextCommit {
                    text: "X".into(),
                    mods: 0,
                });
                let queued = h.direct_events.drain_tree_messages();
                let other = h.actor_events.drain_tree_messages();
                assert!(!queued.is_empty());
                for sample in 0..backlog {
                    h.round_trip(vec![pulse(500_000 + sample * 10_000)]);
                }
                if remount {
                    let raw: Vec<_> = 3u16
                        .to_be_bytes()
                        .into_iter()
                        .chain([1, 2])
                        .chain(200f64.to_be_bytes())
                        .chain([2, 2])
                        .chain(30f64.to_be_bytes())
                        .chain([56, 1])
                        .collect();
                    let attrs = crate::tree::attrs::decode_attrs(&raw).unwrap();
                    let mut replacement = ElementTree::new();
                    replacement.insert(Element::with_attrs(
                        NodeId(3),
                        ElementKind::TextInput,
                        raw,
                        attrs,
                    ));
                    replacement.set_root_id(NodeId(3));
                    h.round_trip(vec![TreeMsg::UploadTree {
                        bytes: crate::tree::serialize::encode_tree(&replacement),
                        submitted_at: None,
                    }]);
                }
                h.round_trip_pair(queued, other);
                h.input(InputEvent::CursorPos { x: 150.0, y: 10.0 });
                h.input(InputEvent::CursorButton {
                    button: "left".into(),
                    action: crate::input::ACTION_RELEASE,
                    mods: 0,
                    x: 150.0,
                    y: 10.0,
                });
                h.input(InputEvent::CursorPos { x: 410.0, y: 10.0 });
                h.round_trip(vec![pulse(1_000_000)]);
                h.round_trip(vec![pulse(1_100_000)]);
                if remount {
                    assert!(h.direct_events.focused_text_state().is_none());
                    assert_eq!(
                        h.engine
                            .tree()
                            .get(&NodeId(3))
                            .unwrap()
                            .spec
                            .declared
                            .content
                            .as_deref()
                            .unwrap_or_default(),
                        ""
                    );
                } else {
                    assert!(
                        h.direct_events
                            .focused_text_state()
                            .unwrap()
                            .content
                            .contains('X')
                    );
                    assert!(
                        h.engine
                            .tree()
                            .get(&NodeId(2))
                            .unwrap()
                            .layout
                            .dimension_samples
                            .is_none()
                    );
                }
                assert_eq!(*h.sinks[0].0.lock().unwrap(), *h.sinks[1].0.lock().unwrap());
                // Registry/input progress above is explicitly not successful drawing.
                assert!(worker.outputs.is_empty());
                loop {
                    let (animate, bytes) = worker
                        .attempts
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                    if !animate && bytes == if remount { 0 } else { 7 } {
                        break;
                    }
                }
                while worker.pulses.try_recv().is_ok() {}
                worker.fail.store(false, Ordering::SeqCst);
                let recovered = worker.outputs.recv_timeout(Duration::from_secs(5)).unwrap();
                assert_eq!(
                    recovered, h.last_pixels,
                    "after_paint={after_paint}, backlog={backlog}, remount={remount}"
                );
                assert_ne!(initial, recovered);
                assert!(
                    worker.pulses.is_empty(),
                    "static recovery does not fabricate a tree pulse"
                );
                let (old, pixels) = h.retained.take().unwrap();
                assert_eq!(
                    raster()
                        .render(&RenderState::new(old, Color::TRANSPARENT, 0, false))
                        .data,
                    pixels
                );
            }
        }
    }
}
