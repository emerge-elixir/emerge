use super::*;

fn control_loop() -> (EventLoop<'static, usize>, WaylandWake, FramePacer) {
    let event_loop = EventLoop::try_new().unwrap();
    let (control, rx) = FrameRateControl::new(Some(1));
    let (ping, source) = calloop::ping::make_ping().unwrap();
    event_loop
        .handle()
        .insert_source(source, move |(), _, count| {
            let _ = rx.try_recv();
            *count += 1;
        })
        .unwrap();
    let running_flag = Arc::new(AtomicBool::new(true));
    let running = running_flag.clone();
    let (tx, rx) = calloop::channel::channel();
    event_loop
        .handle()
        .insert_source(rx, move |event, _, count| {
            if let calloop::channel::Event::Msg(WakeAction::Stop) = event {
                running.store(false, Ordering::Release);
                *count += 1000;
            }
        })
        .unwrap();
    let wake = WaylandWake {
        config_inbox: ConfigInbox::default(),
        tx,
        frame_rate: control.clone(),
        frame_rate_ping: ping,
        running_flag,
    };
    (event_loop, wake, FramePacer::new(control))
}

#[test]
fn cap_wakes_coalesce_and_leave_idle_presenters_idle() {
    let (mut event_loop, wake, mut pacer) = control_loop();
    let now = Instant::now();
    pacer.started(now);
    for n in 1..=1000 {
        wake.set_frame_rate(Some(n)).unwrap();
    }
    wake.set_frame_rate(Some(60)).unwrap();
    let mut count = 0;
    event_loop
        .dispatch(Some(Duration::ZERO), &mut count)
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(pacer.render_at(now), now + Duration::from_nanos(16_666_667));
    assert!(wake.set_frame_rate(Some(0)).is_err());
    wake.set_frame_rate(None).unwrap();
    event_loop
        .dispatch(Some(Duration::ZERO), &mut count)
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(pacer.render_at(now), now);
    assert_eq!(
        pacer.dispatch_timeout(false, now, WAYLAND_DISPATCH_STOP_POLL_INTERVAL),
        WAYLAND_DISPATCH_STOP_POLL_INTERVAL
    );
    wake.request_stop();
    event_loop
        .dispatch(Some(Duration::ZERO), &mut count)
        .unwrap();
    assert_eq!(count, 1002);
    assert!(wake.set_frame_rate(Some(30)).is_err());
}

#[test]
fn cap_changes_wake_sleeping_dispatch_and_closed_sources_reject_updates() {
    let (mut event_loop, wake, pacer) = control_loop();
    let sender = wake.clone();
    let thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        sender.set_frame_rate(None).unwrap();
    });
    let mut count = 0;
    event_loop
        .dispatch(Some(Duration::from_secs(2)), &mut count)
        .unwrap();
    thread.join().unwrap();
    assert_eq!(count, 1);
    let now = Instant::now();
    assert_eq!(pacer.render_at(now), now);
    drop(event_loop);
    assert!(wake.set_frame_rate(Some(30)).is_err());
}
