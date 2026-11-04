use super::*;
use crossbeam_channel::bounded;

fn asleep(gesture: Option<WakeGesture>) -> LiveConfig {
    LiveConfig {
        title: None,
        target_fps: None,
        input_mask: 0x100,
        drm_power: Some(DrmPower {
            display_on: false,
            brightness: Some(0.0),
            wake_on_touch: gesture,
        }),
    }
}

#[test]
fn initial_off_wake_is_an_ordinary_joined_revision_and_preserves_other_settings() {
    let control = ConfigControl::new(17, Arc::default());
    control.initialize(asleep(Some(WakeGesture::DoubleTap)));
    assert_eq!(control.off_policy(), Some((0, WakeGesture::DoubleTap)));
    assert!(
        asleep(None).validate().is_ok(),
        "touch subscriptions are valid live controls"
    );
    let (tx, rx) = bounded(1);
    let (reply, replies) = bounded(1);
    let token = control
        .submit_wake_to(
            0,
            WakeGesture::DoubleTap,
            CompletionTarget::Test(reply),
            |req| tx.try_send(req).map_err(|e| e.to_string()),
        )
        .unwrap();
    assert_eq!(token, (17, 1));
    assert_eq!(control.off_policy(), None);
    let request = rx.recv().unwrap();
    let power = request.config.drm_power.as_ref().unwrap();
    assert!(power.display_on);
    assert_eq!(power.brightness, Some(0.0));
    assert_eq!(power.wake_on_touch, Some(WakeGesture::DoubleTap));
    assert_eq!(request.config.input_mask, 0x100);
    assert_eq!(request.wake_gesture, Some(WakeGesture::DoubleTap));
    request.finish_input(Ok(()));
    assert!(replies.try_recv().is_err());
    request.finish_output(Ok(()));
    request.finish_output(Ok(()));
    assert_eq!(replies.recv().unwrap(), (token, Ok(())));
    assert!(replies.try_recv().is_err());
    assert_eq!(control.status().applied_revision, Some(1));
}

#[test]
fn disabled_stale_pending_stopped_and_rejected_requests_do_not_change_intent() {
    let stop = Arc::new(AtomicBool::new(false));
    let control = ConfigControl::new(18, stop.clone());
    control.initialize(asleep(None));
    let attempt = |expected, gesture, fail| {
        let (reply, _) = bounded(1);
        control.submit_wake_to(expected, gesture, CompletionTarget::Test(reply), |_| {
            if fail {
                Err("queue full".into())
            } else {
                Ok(())
            }
        })
    };
    assert!(attempt(0, WakeGesture::SingleTap, false).is_err());
    let (tx, rx) = bounded(1);
    let (reply, _) = bounded(1);
    control
        .submit_to(
            asleep(Some(WakeGesture::SingleTap)),
            CompletionTarget::Test(reply),
            |r| tx.try_send(r).map_err(|e| e.to_string()),
        )
        .unwrap();
    assert_eq!(control.off_policy(), None);
    assert!(attempt(1, WakeGesture::SingleTap, false).is_err());
    rx.recv().unwrap().finish(Ok(()));
    assert_eq!(control.off_policy(), Some((1, WakeGesture::SingleTap)));
    assert!(attempt(0, WakeGesture::SingleTap, false).is_err());
    assert!(attempt(1, WakeGesture::DoubleTap, false).is_err());
    assert!(attempt(1, WakeGesture::SingleTap, true).is_err());
    assert_eq!(control.status().desired_revision, 1);
    assert_eq!(control.off_policy(), Some((1, WakeGesture::SingleTap)));
    stop.store(true, Ordering::Release);
    assert_eq!(control.off_policy(), None);
    assert!(attempt(1, WakeGesture::SingleTap, false).is_err());
}

#[test]
fn a_later_off_supersedes_a_queued_wake_without_replaying_stale_settings() {
    let control = ConfigControl::new(19, Arc::default());
    control.initialize(asleep(Some(WakeGesture::LongPress)));
    let (tx, rx) = bounded(2);
    let (reply, replies) = bounded(2);
    control
        .submit_wake_to(
            0,
            WakeGesture::LongPress,
            CompletionTarget::Test(reply.clone()),
            |r| tx.try_send(r).map_err(|e| e.to_string()),
        )
        .unwrap();
    let old = rx.recv().unwrap();
    let mut updated = asleep(None);
    updated.drm_power.as_mut().unwrap().brightness = Some(0.7);
    control
        .submit_to(updated, CompletionTarget::Test(reply), |r| {
            tx.try_send(r).map_err(|e| e.to_string())
        })
        .unwrap();
    assert!(old.apply(|| panic!("stale wake must not execute")).is_err());
    old.finish(Ok(()));
    assert!(replies.recv().unwrap().1.is_err());
    let latest = rx.recv().unwrap();
    assert_eq!(
        latest.config.drm_power.as_ref().unwrap().brightness,
        Some(0.7)
    );
    latest.finish(Ok(()));
    assert_eq!(control.off_policy(), None);
}
