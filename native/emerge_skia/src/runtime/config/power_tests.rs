use super::*;
use crossbeam_channel::{Receiver, bounded};
use std::time::Duration;

type Reply = ((u64, u64), Result<(), String>);

fn power() -> LiveConfig {
    LiveConfig {
        title: None,
        target_fps: None,
        input_mask: 3,
        drm_power: Some(DrmPower {
            display_on: false,
            brightness: Some(0.5),
        }),
    }
}

fn enqueue(control: &Arc<ConfigControl>) -> (ConfigRequest, Receiver<Reply>) {
    let (tx, rx) = bounded(1);
    let (reply_tx, reply_rx) = bounded(2);
    control
        .submit_to(power(), CompletionTarget::Test(reply_tx), |request| {
            tx.try_send(request).map_err(|e| e.to_string())
        })
        .unwrap();
    (rx.recv().unwrap(), reply_rx)
}

#[test]
fn either_owner_can_finish_first_but_both_are_required() {
    for input_first in [false, true] {
        let control = ConfigControl::new(21, Arc::new(AtomicBool::new(false)));
        let (request, replies) = enqueue(&control);
        if input_first {
            request.finish_input(Ok(()));
        } else {
            request.finish_output(Ok(()));
        }
        assert_eq!(control.status().pending_revision, Some(1));
        assert!(replies.try_recv().is_err());
        if input_first {
            request.finish_output(Ok(()));
        } else {
            request.finish_input(Ok(()));
        }
        assert_eq!(replies.recv().unwrap(), ((21, 1), Ok(())));
        request.finish_input(Ok(()));
        request.finish_output(Err("late duplicate".into()));
        assert!(replies.try_recv().is_err());
        assert_eq!(control.status().applied_revision, Some(1));
    }
}

#[test]
fn concurrent_owner_completions_send_exactly_one_reply() {
    for _ in 0..32 {
        let control = ConfigControl::new(22, Arc::new(AtomicBool::new(false)));
        let (request, replies) = enqueue(&control);
        let input = request.clone();
        let input = std::thread::spawn(move || input.finish_input(Ok(())));
        request.finish_output(Ok(()));
        input.join().unwrap();
        assert_eq!(
            replies.recv_timeout(Duration::from_secs(1)).unwrap(),
            ((22, 1), Ok(()))
        );
        assert!(replies.try_recv().is_err());
    }
}

#[test]
fn owner_failure_cannot_become_success() {
    for input_fails in [false, true] {
        let control = ConfigControl::new(23, Arc::new(AtomicBool::new(false)));
        let (request, replies) = enqueue(&control);
        if input_fails {
            request.finish_input(Err("owner failed".into()));
        } else {
            request.finish_output(Err("owner failed".into()));
        }
        assert!(replies.recv().unwrap().1.is_err());

        request.finish_input(Ok(()));
        request.finish_output(Ok(()));
        assert_eq!(control.status().failed_revision, Some(1));
        assert_eq!(control.status().applied_revision, None);
        assert!(replies.try_recv().is_err());
    }
}

#[test]
fn stop_between_owner_acknowledgements_fails_the_join() {
    let stop = Arc::new(AtomicBool::new(false));
    let control = ConfigControl::new(24, stop.clone());
    let (request, replies) = enqueue(&control);
    request.finish_input(Ok(()));
    stop.store(true, Ordering::Release);
    request.finish_output(Ok(()));
    assert_eq!(
        replies.recv().unwrap().1,
        Err("renderer is stopping".into())
    );
    assert_eq!(control.status().applied_revision, None);
}

#[test]
fn superseded_join_cannot_acknowledge_the_new_revision() {
    let control = ConfigControl::new(25, Arc::new(AtomicBool::new(false)));
    let (old, old_replies) = enqueue(&control);
    old.finish_input(Ok(()));
    let (new, new_replies) = enqueue(&control);
    old.finish_output(Ok(()));
    assert!(old_replies.recv().unwrap().1.is_err());
    assert_eq!(control.status().pending_revision, Some(2));
    new.finish_output(Ok(()));
    assert!(new_replies.try_recv().is_err());
    new.finish_input(Ok(()));
    assert_eq!(new_replies.recv().unwrap(), ((25, 2), Ok(())));
}

#[test]
fn power_admission_does_not_require_space_in_the_input_queue() {
    let (input_tx, input_rx) = bounded(1);
    input_tx.send(EventMsg::Stop).unwrap();
    let control = ConfigControl::new(26, Arc::new(AtomicBool::new(false)));
    let (request, replies) = enqueue(&control);
    request.finish_output(Ok(()));
    assert!(
        input_tx
            .try_send(EventMsg::ConfigureInput(request.clone()))
            .is_err()
    );
    assert!(replies.try_recv().is_err());
    input_rx.recv().unwrap();
    request.finish_input(Ok(()));
    assert_eq!(replies.recv().unwrap(), ((26, 1), Ok(())));
}

#[test]
fn brightness_is_validated_before_admission() {
    for value in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
        let control = ConfigControl::new(27, Arc::new(AtomicBool::new(false)));
        let mut config = power();
        config.drm_power.as_mut().unwrap().brightness = Some(value);
        let (reply, _) = bounded(1);
        assert!(
            control
                .submit_to(config, CompletionTarget::Test(reply), |_| panic!(
                    "invalid config enqueued"
                ))
                .is_err()
        );
        assert_eq!(control.status().desired_revision, 0);
    }
    for brightness in [None, Some(0.0), Some(1.0)] {
        assert!(
            DrmPower {
                display_on: true,
                brightness
            }
            .validate()
            .is_ok()
        );
    }
}

#[test]
fn taking_a_mailbox_request_does_not_acknowledge_owner_application() {
    let control = ConfigControl::new(28, Arc::new(AtomicBool::new(false)));
    let (inbox, owner) = ConfigInbox::pair();
    let (old, old_replies) = enqueue(&control);
    inbox.submit(old).unwrap();
    let (new, replies) = enqueue(&control);
    inbox.submit(new).unwrap();
    assert!(old_replies.recv().unwrap().1.is_err());
    let taken = owner.take().unwrap();
    assert!(owner.take().is_none());
    assert_eq!(control.status().pending_revision, Some(2));
    assert!(replies.try_recv().is_err());
    taken.finish_input(Ok(()));
    taken.finish_output(Ok(()));
    assert_eq!(replies.recv().unwrap(), ((28, 2), Ok(())));
}
