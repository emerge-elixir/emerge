use super::*;
use crate::tree::motion::Property;
use std::time::{Duration, Instant};
fn tree() -> Tree {
    let mut t = Tree::for_session(0);
    let a = block(&[px(1, 8.), px(2, 8.), color([255, 0, 0]), vec![40, 1]]);
    let wire = upload(&[
        node(1, 1, block(&[px(1, 96.), px(2, 12.)]), &[2]),
        node(2, 4, a, &[]),
    ]);
    Publisher::new(0).apply(&wire, 1.).unwrap().install(&mut t);
    t.refresh(96., 12., 1.).unwrap();
    t
}
#[test]
fn absolute_samples_move_pixels_and_hits_together_without_an_authored_revision() {
    let mut t = tree();
    let mut m = Motion::default();
    let start = Instant::now();
    let key = t.elements.ids[&2];
    let version = t
        .elements
        .attrs
        .version
        .as_ref()
        .unwrap()
        .get(key)
        .unwrap()
        .clone();
    m.linear(
        key,
        Property::OffsetX(0., 48.),
        start,
        Duration::from_millis(48),
        false,
    );
    let installed = t.seal().unwrap();
    for ms in [16, 16, 48] {
        m.sample(&mut t, start + Duration::from_millis(ms));
        t.refresh(96., 12., 1.).unwrap();
        let next = t.seal().unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &version,
            t.elements.attrs.version.as_ref().unwrap().get(key).unwrap()
        ));
        assert!(!t.layers.trace.iter().any(|&(p, _, _)| p == Measure));
        let x = ms as usize;
        assert_eq!(
            &next.rasterize().unwrap()[4 * x..4 * x + 4],
            &[255, 0, 0, 255]
        );
        assert!(
            next.hits
                .target_at(next.viewport, x as f32 + 1., 2.)
                .is_some()
        );
        assert!(
            installed
                .hits
                .target_at(installed.viewport, x as f32 + 1., 2.)
                .is_none()
        );
    }
    let lost = m.endpoints();
    drop(lost);
    assert!(m.active());
    let observed = m.endpoints();
    let at = start + Duration::from_millis(60);
    observed.observe(at);
    observed.observe(at + Duration::from_secs(1));
    assert!(!m.active());
}
#[test]
fn an_unsampled_frame_cannot_acknowledge_either_endpoint() {
    let start = Instant::now();
    let mut t = tree();
    let key = t.elements.ids[&2];
    let mut m = Motion::default();
    m.linear(key, Property::Width(12., 32.), start, Duration::ZERO, true);
    m.endpoints().observe(start); // An authored frame, before the first sample.
    assert!(m.sample(&mut t, start));
    t.refresh(96., 12., 1.).unwrap();
    assert_eq!(t.layers.frames[key].width, 12.);
    assert!(!m.sample(&mut t, start + Duration::from_secs(1)));
    let first = m.endpoints();
    first.observe(start);
    assert!(m.active());
    assert!(m.sample(&mut t, start));
    t.refresh(96., 12., 1.).unwrap();
    assert_eq!(t.layers.frames[key].width, 32.);
    first.observe(start);
    assert!(m.active());
    m.endpoints().observe(start);
    assert!(!m.active());
}
#[test]
fn first_pose_anchors_only_at_progress_and_width_not_background_reads_measure() {
    let start = Instant::now();
    for (property, layout) in [
        (Property::Width(8., 32.), true),
        (
            Property::Background([255, 0, 0, 255], [0, 0, 255, 255]),
            false,
        ),
    ] {
        let mut t = tree();
        let key = t.elements.ids[&2];
        let mut m = Motion::default();
        m.linear(key, property, start, Duration::from_millis(24), true);
        m.sample(&mut t, start);
        t.refresh(96., 12., 1.).unwrap();
        let first = m.endpoints();
        assert!(!m.sample(&mut t, start + Duration::from_secs(1)));
        first.observe(start + Duration::from_secs(2));
        first.observe(start + Duration::from_secs(3));
        m.sample(&mut t, start + Duration::from_millis(2012));
        t.refresh(96., 12., 1.).unwrap();
        assert_eq!(t.layers.trace.iter().any(|&(p, _, _)| p == Measure), layout);
        if layout {
            assert_eq!(t.layers.frames[key].width, 20.);
        } else {
            assert_eq!(t.elements.attrs[key].background, Some(0x800080ff));
        }
    }
}
