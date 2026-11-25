use super::*;

#[test]
fn shutdown_with_a_completed_unacknowledged_ticket_does_not_bind_or_deadlock() {
    for name in [
        "test_assets/single_axis_wide.png",
        "test_assets/single_axis_wide.svg",
    ] {
        let mut tree = Tree::default();
        tree.upload(&crate::tree::image_fixture(name)).unwrap();
        let signal = Signal::default();
        let w = Worker::start(signal.clone(), move |request, publish| {
            let mut loader = Loader::new(crate::raster_assets::Settings {
                sources: vec![format!("{}/../../priv", env!("CARGO_MANIFEST_DIR"))],
                ..Default::default()
            })
            .unwrap();
            loader.fulfill(request, |s| {
                assert!(matches!(s, Source::Ready(_) | Source::Picture(_)));
                assert!(!publish(s));
                false
            });
        })
        .unwrap();
        w.jobs.try_send(tree.next_image().unwrap()).ok().unwrap();
        signal.wait(Some(Instant::now() + Duration::from_secs(5)));
        assert!(w.results.try_recv().is_ok());
        w.stop();
    }
}
