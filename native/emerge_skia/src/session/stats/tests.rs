use super::*;

#[test]
fn closing_a_window_resets_samples_but_keeps_gauges() {
    let mut w = Window::new(Some(Duration::from_millis(10)));
    w.layout = [5, 2, 4];
    w.packed = [6, 12, 3];
    for ms in [4, 1, 7] {
        w.counts
            .timings
            .record(Metric::Present, Duration::from_millis(ms));
    }
    let end = w.start + Duration::from_millis(500);
    for cmd in [StatsCommandNif::Peek, StatsCommandNif::Take] {
        let r = w.close(cmd, end);
        let t = r.counts.timings.present_submit;
        assert_eq!((t.count, t.min_ms, t.avg_ms, t.max_ms), (3, 1., 4., 7.));
        assert_eq!(r.frames().fps, 6.);
    }
    let idle = w.close(StatsCommandNif::Take, end + PERIOD);
    assert_eq!(idle.frames().frame_count, 0);
    assert_eq!(idle.frames().display_fps, 100.);
    assert_eq!((idle.layout, idle.packed), ([5, 2, 4], [6, 12, 3]));
    assert!(Display::next_report(false).is_none());
}

#[test]
fn a_slow_report_requires_logs_and_is_limited_per_stage_per_session() {
    let quiet = Collector::new(true, false, None);
    assert!(!quiet.log_slow(0, SLOW));
    let stats = Collector::new(false, true, None);
    assert!(!stats.log_slow(0, Duration::ZERO));
    assert!(stats.log_slow(0, SLOW));
    assert!(!stats.log_slow(0, SLOW));
    assert!(stats.log_slow(1, SLOW));
}
