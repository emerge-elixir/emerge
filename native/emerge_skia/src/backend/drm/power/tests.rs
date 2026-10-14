use super::*;

fn control(sequence: Arc<AtomicU64>) -> PresenterControl {
    PresenterControl {
        // No DRM ioctls: only exercise the shared presenter handshake.
        card: Arc::new(Card(std::fs::File::open("/dev/null").unwrap(), None)),
        sequence,
        presented: Arc::new(AtomicBool::new(false)),
        retiring: Arc::new(AtomicBool::new(false)),
        dark_ack: Arc::new(AtomicBool::new(false)),
        wake: EventFd::new().unwrap(),
    }
}

#[test]
fn recreated_presenters_share_frame_sequence_but_not_presentation_or_dark_ack() {
    let sequence = Arc::new(AtomicU64::new(0));
    let (tx, rx) = crossbeam_channel::bounded(1);
    let stop = AtomicBool::new(false);
    for request in 1..=100 {
        let old = control(sequence.clone());
        assert_eq!(old.request_frame(&tx, (640, 480), &stop).unwrap(), request);
        assert!(matches!(rx.recv().unwrap(), TreeMsg::Batch(messages)
            if matches!(messages.as_slice(), [TreeMsg::Resize { .. }, TreeMsg::RequestFrame { request: id }] if *id == request)));
        old.presented();
        old.dark_ack.store(true, Ordering::Release);
        let next = control(sequence.clone());
        assert!(!next.presented.load(Ordering::Acquire));
        assert!(!next.dark_ack.load(Ordering::Acquire));
        assert!(!next.retiring.load(Ordering::Acquire));
        // Even a late notification from the old child cannot mark next visible.
        old.presented();
        assert!(!next.presented.load(Ordering::Acquire));
    }
}

#[test]
fn detach_waits_for_parent_suppression_acknowledgement() {
    let control = control(Arc::default());
    let child = control.clone();
    let (done_tx, done_rx) = crossbeam_channel::bounded(1);
    let handle = thread::spawn(move || {
        child.before_detach();
        done_tx.send(()).unwrap();
    });
    let mut fds = [libc::pollfd {
        fd: control.wake.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    }];
    poll_fds(&mut fds, Some(Duration::from_secs(1))).unwrap();
    assert_ne!(fds[0].revents & libc::POLLIN, 0);
    assert!(control.retiring.load(Ordering::Acquire));
    assert!(done_rx.try_recv().is_err());
    // Like the real owner, acknowledge the suppression attempt before waiting
    // for child completion. The child itself performs no backlight I/O.
    control.dark_ack.store(true, Ordering::Release);
    done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    handle.join().unwrap();
    control.before_detach(); // Repeated retirement cannot strand the child.
}

#[test]
fn frame_request_cancels_on_stop_or_disconnected_tree() {
    let control = control(Arc::default());
    let (tx, rx) = crossbeam_channel::bounded(1);
    assert!(
        control
            .request_frame(&tx, (1, 1), &AtomicBool::new(true))
            .is_err()
    );
    assert!(rx.try_recv().is_err());
    drop(rx);
    assert!(
        control
            .request_frame(&tx, (1, 1), &AtomicBool::new(false))
            .is_err()
    );
}

#[test]
fn cancelled_child_startup_does_not_fail_the_persistent_owner() {
    assert!(
        presenter_readiness(Err("DRM Vulkan startup aborted".into()), true)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        presenter_readiness(Err("DRM Vulkan startup failed".into()), false),
        Err(error) if error == "DRM Vulkan startup failed"
    ));
}

#[test]
fn successful_readiness_still_preserves_initial_off_capabilities() {
    for stopped in [false, true] {
        assert!(
            presenter_readiness(Ok(DrmBackendStartupInfo::raster()), stopped)
                .unwrap()
                .is_some()
        );
    }
}
