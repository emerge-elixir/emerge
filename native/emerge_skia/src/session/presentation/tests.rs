//! Adapter contract, not a DRM implementation. The real selector sees only
//! fenced images; the fake CRTC owns scanout references and flip ordering.
use super::*;
use std::sync::Arc;
struct Image {
    pixels: u64,
    route: u64,
    _pin: Arc<()>,
}
struct Crtc {
    generation: u64,
    ready: Latest<Image>,
    queued: Option<Frame<Image>>,
    scanning: Option<Frame<Image>>,
    installed: Option<(u64, u64)>,
}
impl Crtc {
    fn new() -> Self {
        let mut ready = Latest::new();
        ready.reset(1);
        Self {
            generation: 1,
            ready,
            queued: None,
            scanning: None,
            installed: None,
        }
    }
    fn commit(&mut self, busy: bool) {
        assert!(self.queued.is_none());
        if let Some(claim) = self.ready.take() {
            if busy {
                self.ready.retry(claim);
            } else {
                self.queued = Some(claim);
            }
        }
    }
    fn flip(&mut self, crtc: u32, primary: bool, id: Id) {
        if crtc != 7 || !primary || self.queued.as_ref().is_none_or(|c| c.id != id) {
            return;
        }
        self.scanning = self.queued.take();
        if id.generation == self.generation {
            let i = &self.scanning.as_ref().unwrap().value;
            self.installed = Some((i.pixels, i.route));
        }
    }
    fn reset(&mut self) {
        self.generation += 1;
        self.ready.reset(self.generation);
    }
}
#[test]
fn three_or_four_targets_never_overwrite_scanout_queued_ready_or_gpu_readers() {
    for count in [3, 4] {
        let bank: Vec<_> = (0..count).map(|_| Arc::new(())).collect();
        let mut adapter = Crtc::new();
        let draw = |n| {
            bank.iter()
                .find(|p| Arc::strong_count(p) == 1)
                .map(|p| Frame {
                    id: Id {
                        generation: 1,
                        serial: n,
                    },
                    earliest: 0,
                    value: Image {
                        pixels: n * 100,
                        route: n,
                        _pin: p.clone(),
                    },
                })
        };
        let a = draw(1).unwrap();
        let aid = a.id;
        adapter.ready.offer(a);
        adapter.commit(false);
        adapter.flip(7, true, aid);
        let capture = adapter.scanning.as_ref().unwrap().value._pin.clone();
        let b = draw(2).unwrap();
        let bid = b.id;
        adapter.ready.offer(b);
        adapter.commit(true);
        assert_eq!(adapter.installed, Some((100, 1)));
        adapter.commit(false);
        let c = draw(3).unwrap();
        let cid = c.id;
        adapter.ready.offer(c);
        let unfinished = draw(4);
        assert_eq!(unfinished.is_some(), count == 4);
        assert!(draw(5).is_none());
        for (crtc, primary, id) in [(8, true, bid), (7, false, bid), (7, true, aid)] {
            adapter.flip(crtc, primary, id);
            assert_eq!(adapter.installed, Some((100, 1)));
            assert!(adapter.queued.is_some());
        }
        adapter.flip(7, true, bid);
        assert_eq!(adapter.installed, Some((200, 2)));
        assert!(draw(5).is_none()); // old capture still pins A
        drop(capture);
        assert!(draw(5).is_some());
        adapter.commit(false);
        assert_eq!(adapter.queued.as_ref().unwrap().id, cid); // don't wait for D's fence
        if let Some(done) = unfinished {
            adapter.ready.offer(done);
        }
        adapter.reset();
        assert!(!adapter.ready.pending());
        adapter.flip(7, true, bid);
        assert_eq!(adapter.installed, Some((200, 2)));
        adapter.flip(7, true, cid);
        assert_eq!(adapter.installed, Some((200, 2))); // retires the old queued lease only
        adapter.scanning.take(); // proven blocking detach, not a new installation
        assert!(bank.iter().all(|p| Arc::strong_count(p) == 1));
    }
}
#[test]
fn a_future_only_pose_does_not_displace_an_eligible_completion_or_its_retry() {
    let mut s = Latest::new();
    s.reset(2);
    let f = |serial, earliest| Frame {
        id: Id {
            generation: 2,
            serial,
        },
        earliest,
        value: serial,
    };
    s.offer(f(3, 2));
    s.offer(f(2, 1));
    let b = s.take().unwrap();
    assert_eq!(b.value, 2);
    s.retry(b);
    assert!(s.offer(f(4, 4)).is_some());
    assert_eq!(s.advance(2).unwrap().value, 2);
    assert_eq!(s.take().unwrap().value, 3);
    s.offer(f(4, 3));
    s.offer(f(5, 2));
    assert_eq!(s.advance(3).unwrap().value, 4);
    assert_eq!(s.take().unwrap().value, 5);
}
