//! Binary rendering orchestration, independent of Rustler delivery and GPU setup.
use super::*;

/// Retry draw/readback, not the tree transaction or its animation clock. Backoff
/// bounds autonomous retries; a new scene is an immediate external recovery input.
#[derive(Default)]
struct RetryDelay(Option<Duration>);
impl RetryDelay {
    fn next(&mut self, now: Instant) -> Instant {
        let delay = self.0.unwrap_or(Duration::from_millis(16));
        self.0 = Some((delay * 2).min(Duration::from_millis(250)));
        now + delay
    }
}

pub(super) fn run_binary_loop(
    render_rx: &Receiver<RenderMsg>,
    tree_tx: &Sender<TreeMsg>,
    running: &AtomicBool,
    pixel_format: &str,
    frame_interval: Duration,
    mut render: impl FnMut(&RenderState, Instant) -> Result<(), String>,
) {
    let mut sequence = 0u64;
    let mut animation_tick = never();
    let mut next_animation_at = None;
    let mut retry_tick = never();
    let mut retry_delay = RetryDelay::default();
    let mut pending = None;
    while running.load(Ordering::Relaxed) {
        let attempt = select! {
            recv(animation_tick)->_=>{
                animation_tick=never();
                maybe_send_animation_pulse(tree_tx,true,frame_interval);
                None
            }
            recv(retry_tick)->_=>{
                retry_tick=never();
                pending.take().map(|state|(state,Instant::now()))
            }
            recv(render_rx)->message=>{
                let Ok(message)=message else {break;};
                match message {
                    RenderMsg::Stop=>break,
                    RenderMsg::Scene{scene,animate,pipeline_submitted_at,pipeline_render_queued_at,..}=>{
                        let started=Instant::now();
                        // Dispose the superseded scene synchronously, inside attempt timing.
                        pending=None;
                        retry_tick=never();
                        let clear=if matches!(pixel_format,"bw1"|"gray2") {Color::WHITE}else{Color::TRANSPARENT};
                        let mut state=RenderState::new(*scene,clear,sequence,animate);
                        state.pipeline_submitted_at=pipeline_submitted_at;
                        state.pipeline_render_queued_at=pipeline_render_queued_at;
                        Some((state,started))
                    }
                }
            }
        };
        let Some((state, started)) = attempt else {
            continue;
        };
        if !running.load(Ordering::Relaxed) {
            break;
        }
        match render(&state, started) {
            Ok(()) => {
                sequence = sequence.wrapping_add(1);
                retry_delay = RetryDelay::default();
                animation_tick = animation_tick_receiver(
                    state.animate,
                    frame_interval,
                    Instant::now(),
                    &mut next_animation_at,
                );
            }
            Err(error) => {
                eprintln!("headless render failed: {error}");
                // Do not keep driving new animation samples through a failed output.
                // Successful recovery resumes native wall-clock sampling normally.
                animation_tick = never();
                next_animation_at = None;
                pending = Some(state);
                retry_tick = at(retry_delay.next(Instant::now()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn message(animate: bool) -> RenderMsg {
        RenderMsg::Scene {
            scene: Box::default(),
            version: 1,
            pipeline_submitted_at: None,
            pipeline_render_queued_at: None,
            animation_trace: None,
            animate,
            ime_enabled: false,
            ime_cursor_area: None,
            ime_text_state: Box::new(None),
        }
    }
    #[test]
    fn failed_terminal_binary_frame_recovers_without_another_tree_message() {
        let (tx, rx) = bounded(4);
        let (tree_tx, tree_rx) = bounded(4);
        let (output, frames) = bounded(4);
        let (attempt_tx, attempts) = bounded(4);
        let handle = thread::spawn(move || {
            let running = AtomicBool::new(true);
            let mut fail = true;
            run_binary_loop(
                &rx,
                &tree_tx,
                &running,
                "rgba",
                Duration::from_millis(16),
                |_, _| {
                    attempt_tx.send(()).unwrap();
                    if std::mem::take(&mut fail) {
                        Err("controlled readback failure".into())
                    } else {
                        output.send(()).unwrap();
                        Ok(())
                    }
                },
            );
        });
        tx.send(message(false)).unwrap();
        attempts.recv_timeout(Duration::from_secs(5)).unwrap();
        let recovered = frames.recv_timeout(Duration::from_millis(600));
        tx.send(RenderMsg::Stop).unwrap();
        handle.join().unwrap();
        assert!(
            recovered.is_ok(),
            "a failed terminal frame must retry without a new tree message"
        );
        assert!(
            tree_rx.is_empty(),
            "a static output retry must not invent an animation pulse"
        );
    }
    #[test]
    fn autonomous_retry_delay_caps_without_busy_spinning_and_resets_after_success() {
        let start = Instant::now();
        let mut delay = RetryDelay::default();
        let mut next = start;
        let attempts = (0..1000)
            .filter(|ms| {
                let now = start + Duration::from_millis(*ms);
                if now < next {
                    return false;
                }
                next = delay.next(now);
                true
            })
            .count();
        assert_eq!(attempts, 8);
        assert_eq!(delay.next(next) - next, Duration::from_millis(250));
        delay = RetryDelay::default();
        assert_eq!(delay.next(start) - start, Duration::from_millis(16));
    }

    fn tracked_message(
        id: u64,
    ) -> (
        RenderMsg,
        std::sync::Weak<crate::render_scene::RenderPaintLayerContent>,
    ) {
        use crate::render_scene::*;
        let layer = RenderPaintLayer::from_children(
            id,
            crate::tree::geometry::Rect::default(),
            PaintLayerPlacement::Fixed,
            PaintLayerPolicy::DirectOnly,
            PaintLayerReason::Root,
            0,
            vec![],
        );
        let weak = Arc::downgrade(&layer.content);
        let mut msg = message(false);
        if let RenderMsg::Scene { scene, .. } = &mut msg {
            scene.nodes = vec![RenderNode::PaintLayer(layer)];
        }
        (msg, weak)
    }

    #[test]
    fn superseded_failed_scene_drops_synchronously_and_success_leaves_no_retry_history() {
        let (tx, rx) = bounded(4);
        let (tree_tx, _) = unbounded();
        let (attempt_tx, attempts) = unbounded();
        let fail = Arc::new(AtomicBool::new(true));
        let failing = fail.clone();
        let handle = thread::spawn(move || {
            run_binary_loop(
                &rx,
                &tree_tx,
                &AtomicBool::new(true),
                "rgba",
                Duration::from_millis(16),
                |state, _| {
                    let crate::render_scene::RenderNode::PaintLayer(layer) = &state.scene.nodes[0]
                    else {
                        panic!("layer");
                    };
                    let failed = failing.load(Ordering::SeqCst);
                    attempt_tx
                        .send((layer.id.node_id, state.render_version))
                        .unwrap();
                    if failed {
                        Err("controlled draw failure".into())
                    } else {
                        Ok(())
                    }
                },
            )
        });
        let mut previous = None;
        for id in 1..=64 {
            let (msg, weak) = tracked_message(id);
            tx.send(msg).unwrap();
            loop {
                let (observed, sequence) = attempts.recv_timeout(Duration::from_secs(5)).unwrap();
                assert_eq!(sequence, 0, "failures do not advance output sequence");
                if observed == id {
                    break;
                }
            }
            if let Some(previous) = previous {
                let previous: std::sync::Weak<crate::render_scene::RenderPaintLayerContent> =
                    previous;
                assert!(previous.upgrade().is_none());
            }
            assert_eq!(weak.strong_count(), 1);
            previous = Some(weak);
        }
        fail.store(false, Ordering::SeqCst);
        // A new external scene bypasses accumulated backoff, then a FIFO probe
        // proves the successfully rendered scene was dropped without waiting for Stop.
        let (success, success_weak) = tracked_message(65);
        tx.send(success).unwrap();
        loop {
            if attempts.recv_timeout(Duration::from_secs(5)).unwrap().0 == 65 {
                break;
            }
        }
        let (probe, _) = tracked_message(66);
        tx.send(probe).unwrap();
        loop {
            let (id, sequence) = attempts.recv_timeout(Duration::from_secs(5)).unwrap();
            if id == 66 {
                assert!(sequence >= 1);
                break;
            }
        }
        assert!(previous.unwrap().upgrade().is_none());
        assert!(success_weak.upgrade().is_none());
        tx.send(RenderMsg::Stop).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn stop_and_disconnect_release_pending_frames_without_waiting_for_retry() {
        for disconnect in [false, true] {
            let (tx, rx) = bounded(4);
            let (tree_tx, _) = unbounded();
            let (attempt_tx, attempts) = bounded(4);
            let (gate, release) = bounded(1);
            let (done, stopped) = bounded(1);
            let handle = thread::spawn(move || {
                run_binary_loop(
                    &rx,
                    &tree_tx,
                    &AtomicBool::new(true),
                    "rgba",
                    Duration::from_millis(16),
                    |_, _| {
                        attempt_tx.send(()).unwrap();
                        release.recv().unwrap();
                        Err("controlled persistent failure".into())
                    },
                );
                done.send(()).unwrap();
            });
            let (msg, weak) = tracked_message(1);
            tx.send(msg).unwrap();
            attempts.recv_timeout(Duration::from_secs(5)).unwrap();
            if disconnect {
                drop(tx);
            } else {
                tx.send(RenderMsg::Stop).unwrap();
            }
            gate.send(()).unwrap();
            stopped.recv_timeout(Duration::from_secs(5)).unwrap();
            handle.join().unwrap();
            assert!(attempts.is_empty());
            assert!(weak.upgrade().is_none());
        }
    }

    #[test]
    fn recovered_animated_frame_resumes_native_pulses_without_restarting_its_scene() {
        let (tx, rx) = bounded(4);
        let (tree_tx, tree_rx) = unbounded();
        let (attempt_tx, attempts) = unbounded();
        let (gate, release) = bounded(1);
        let submitted = Instant::now() - Duration::from_secs(1);
        let handle = thread::spawn(move || {
            let mut first = true;
            run_binary_loop(
                &rx,
                &tree_tx,
                &AtomicBool::new(true),
                "rgba",
                Duration::from_millis(16),
                |state, _| {
                    assert_eq!(state.pipeline_submitted_at, Some(submitted));
                    assert_eq!(state.render_version, 0);
                    attempt_tx.send(()).unwrap();
                    if std::mem::take(&mut first) {
                        release.recv().unwrap();
                        Err("controlled failure".into())
                    } else {
                        Ok(())
                    }
                },
            );
        });
        let mut msg = message(true);
        if let RenderMsg::Scene {
            pipeline_submitted_at,
            ..
        } = &mut msg
        {
            *pipeline_submitted_at = Some(submitted);
        }
        tx.send(msg).unwrap();
        attempts.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(tree_rx.is_empty());
        gate.send(()).unwrap();
        attempts.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            tree_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            TreeMsg::AnimationPulse { .. }
        ));
        tx.send(RenderMsg::Stop).unwrap();
        handle.join().unwrap();
    }
}
