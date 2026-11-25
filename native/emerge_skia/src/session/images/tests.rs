use super::*;

#[test]
fn stop_while_worker_owns_a_ticket_does_not_publish_or_wake() {
    for name in [
        "test_assets/single_axis_wide.png",
        "test_assets/single_axis_wide.svg",
    ] {
        stop_before_publication(name);
    }
}

fn stop_before_publication(name: &'static str) {
    let mut raster = Raster::default();
    raster
        .tree
        .upload(&crate::tree::image_fixture(name))
        .unwrap();
    let owner = Arc::new(Owner {
        lifecycle: Default::default(),
        revision: Default::default(),
        api: "raster".into(),
        selected: Default::default(),
        #[cfg(all(
            target_os = "linux",
            feature = "wayland",
            any(feature = "opengl", feature = "vulkan")
        ))]
        readbacks: Default::default(),
        live: Mutex::new(Some(Live {
            #[cfg(all(target_os = "linux", feature = "wayland"))]
            toplevel: None,
            assets: None,
            raster,
            input: Default::default(),
            route_target: None,
            log: None,
            stats: None,
            width: 80,
            height: 100,
            request: false,
            last: None,
            interval: None,
        })),
    });
    let (wake, events) = bounded(1);
    let (entered, ticket) = bounded(0);
    let (resume, release) = bounded(0);
    let w = Worker::start(
        Arc::downgrade(&owner),
        Wake::Binary(wake),
        move |request, publish| {
            let mut loader = Loader::new(crate::raster_assets::Settings {
                sources: vec![format!("{}/../../priv", env!("CARGO_MANIFEST_DIR"))],
                ..Default::default()
            })
            .unwrap();
            loader.fulfill(request, |s| {
                assert!(matches!(s, Source::Ready(_) | Source::Picture(_)));
                entered.send(()).unwrap(); // Actual bitmap/picture ready; not yet bound.
                release.recv().unwrap();
                assert!(!publish(s));
                false
            });
        },
    )
    .unwrap();
    w.notify.send(()).unwrap();
    ticket.recv_timeout(Duration::from_secs(3)).unwrap();
    owner.lifecycle.request_stop();
    resume.send(()).unwrap();
    w.stop();
    assert!(!owner.live.lock().unwrap().as_ref().unwrap().request);
    assert!(events.try_recv().is_err());
}
