//! One physical source owns a contact. Mouse and touch share decisions, not
//! synthetic raw events. Only a touch pan can pass velocity to the scroll driver.
use super::*;
use crate::{
    input::TouchPhase,
    session::scroll::{Driver, PAN_SLOP, Velocity},
    tree::Listener,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    Mouse,
    Touch(u64),
}
#[derive(Clone, Copy)]
enum Mode {
    Pending,
    Scroll,
    Drag,
    Swipe(usize),
    Suppressed,
}
#[derive(Clone, Copy)]
pub(super) struct Gesture {
    source: Source,
    mode: Mode,
    listener: Option<Listener>,
    port: bool,
    tap: bool,
    origin: [f32; 2],
    last: [f32; 2],
    start: Instant,
    velocity: Velocity,
}
rustler::atoms! {drag, start, end_phase="end", swipe_left, swipe_right, swipe_up, swipe_down}
#[derive(rustler::NifMap)]
struct Drag {
    phase: Atom,
    x: f64,
    y: f64,
}
#[derive(rustler::NifMap)]
struct Swipe {
    start: Position,
    end: Position,
    duration_ms: f64,
}
fn drag_event(cx: &Context<'_, '_>, t: Listener, phase: Atom) {
    let [x, y] = t.local;
    cx.payload(t.id, drag(), Drag { phase, x, y });
}
fn swipe_direction(axis: usize, d: f32) -> (u16, Atom) {
    match (axis, d > 0.) {
        (0, false) => (Actions::LEFT, swipe_left()),
        (0, true) => (Actions::RIGHT, swipe_right()),
        (_, false) => (Actions::UP, swipe_up()),
        (_, true) => (Actions::DOWN, swipe_down()),
    }
}
const SWIPE_SLOP: f32 = 10.;
// With a directional listener, wait for intent rather than assigning a
// diagonal wobble to either the scrollport or the swipe.
fn direction([dx, dy]: [f32; 2]) -> Option<usize> {
    let (x, y) = (dx.abs(), dy.abs());
    if x >= 1.25 * y && x - y >= 6. {
        Some(0)
    } else if y >= 1.25 * x && y - x >= 6. {
        Some(1)
    } else {
        None
    }
}
impl Gesture {
    pub(super) fn suppress(&mut self) {
        self.mode = Mode::Suppressed;
        self.listener = None;
    }
    fn target(self, s: &Scene, at: [f32; 2]) -> Option<Listener> {
        let old = self.listener?;
        s.hits
            .captured(old.key, at[0], at[1])
            .filter(|h| match self.mode {
                Mode::Drag => h.actions.drag(),
                Mode::Swipe(_) => h.actions.swipes(),
                _ => h.actions == old.actions,
            })
    }
    pub(super) fn valid(self, s: &Scene, driver: &Driver) -> bool {
        match self.mode {
            Mode::Scroll => driver.has_capture(),
            Mode::Pending => {
                (!self.port || driver.has_capture())
                    && (self.listener.is_none() || self.target(s, self.last).is_some())
            }
            Mode::Swipe(_) | Mode::Drag => self.target(s, self.last).is_some(),
            Mode::Suppressed => true,
        }
    }
}
impl Dispatcher {
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    pub fn remap(
        &mut self,
        map: impl Fn([f32; 2]) -> [f32; 2],
        contact: impl FnOnce(u64) -> Option<[f32; 2]>,
    ) {
        self.scroll.remap(&map);
        if let Some(g) = &mut self.gesture {
            let last = match g.source {
                Source::Touch(id) => contact(id),
                Source::Mouse => None,
            };
            g.origin = map(g.origin);
            g.last = last.unwrap_or_else(|| map(g.last));
            g.velocity.remap(map);
        }
    }
    pub(super) fn cancel_contact(
        &mut self,
        s: &Scene,
        source: Option<Source>,
        cx: &Context<'_, '_>,
    ) {
        if self
            .gesture
            .is_some_and(|g| source.is_some_and(|k| k != g.source))
        {
            return;
        }
        if let Some(g) = self.gesture.take()
            && matches!(g.mode, Mode::Drag)
            && let Some(h) = g.target(s, g.last)
        {
            drag_event(cx, h, input::cancel());
        }
        self.scroll.cancel();
    }
    pub(super) fn contact(
        &mut self,
        s: &Scene,
        source: Source,
        phase: TouchPhase,
        at: [f32; 2],
        now: Instant,
        cx: &Context<'_, '_>,
    ) {
        use TouchPhase::*;
        let [x, y] = at;
        if phase == Down {
            if self.gesture.is_some() {
                return;
            }
            let chrome = self.scroll.press(s, x, y, cx);
            let listener = s.hits.listener_at(s.viewport, x, y);
            if !chrome {
                let focus = listener.and_then(|t| s.navigation.as_ref()?.get(t.key));
                self.focus_to(s, focus, cx);
            }
            let mode = if chrome {
                Mode::Scroll
            } else if let Some(t) = listener.filter(|t| t.actions.drag()) {
                self.scroll.cancel();
                drag_event(cx, t, start());
                Mode::Drag
            } else {
                Mode::Pending
            };
            self.gesture = Some(Gesture {
                source,
                mode,
                port: self.scroll.has_capture(),
                tap: true,
                listener,
                origin: at,
                last: at,
                start: now,
                velocity: Velocity::new(now, at),
            });
            return;
        }
        let Some(mut g) = self.gesture.filter(|g| g.source == source) else {
            return;
        };
        if phase == Cancel {
            self.cancel_contact(s, Some(source), cx);
            return;
        }
        // A contact up may carry its last displacement without a separate move.
        // Never synthesize a drag :move or a hover from that release.
        if at != g.last && (phase == Move || source != Source::Mouse && phase == Up) {
            g.velocity.moved(now, at);
            match g.mode {
                Mode::Pending | Mode::Scroll => {
                    let swipes = g.listener.is_some_and(|t| t.actions.swipes());
                    let [dx, dy] = [x - g.origin[0], y - g.origin[1]];
                    if (swipes || g.port || source != Source::Mouse) && dx.hypot(dy) >= PAN_SLOP {
                        g.tap = false;
                    }
                    let intent = direction([dx, dy]);
                    if matches!(g.mode, Mode::Scroll) || !swipes || intent.is_some() {
                        if self.scroll.motion(s, x, y, swipes, cx) {
                            g.mode = Mode::Scroll;
                        } else if !g.tap {
                            let axis = intent.unwrap_or(usize::from(dy.abs() >= dx.abs()));
                            let mask = if axis == 0 {
                                Actions::LEFT | Actions::RIGHT
                            } else {
                                Actions::UP | Actions::DOWN
                            };
                            g.mode = if g.listener.is_some_and(|t| t.actions.has(mask)) {
                                Mode::Swipe(axis)
                            } else {
                                Mode::Suppressed
                            };
                            self.scroll.cancel();
                        }
                    }
                }
                Mode::Drag if phase == Move => {
                    if let Some(t) = g.target(s, at) {
                        drag_event(cx, t, input::move_touch());
                    }
                }
                _ => (),
            }
            g.last = at;
        }
        if phase != Up {
            self.gesture = Some(g);
            return;
        }
        self.gesture = None;
        match g.mode {
            Mode::Scroll => {
                let velocity = if source == Source::Mouse {
                    [0.; 2]
                } else {
                    g.velocity.released(now)
                };
                self.scroll.release(s, velocity, now);
            }
            Mode::Drag => {
                if let Some(t) = g.target(s, at) {
                    drag_event(cx, t, end_phase());
                }
            }
            Mode::Swipe(axis) => {
                let d = at[axis] - g.origin[axis];
                let (flag, kind) = swipe_direction(axis, d);
                if d.abs() >= SWIPE_SLOP
                    && let Some(start) = g.listener.filter(|t| t.actions.has(flag))
                    && let Some(t) = g.target(s, at).filter(|t| t.actions.has(flag))
                {
                    cx.payload(
                        t.id,
                        kind,
                        Swipe {
                            start: start.local.into(),
                            end: t.local.into(),
                            duration_ms: now.saturating_duration_since(g.start).as_secs_f64()
                                * 1000.,
                        },
                    );
                }
            }
            Mode::Pending => {
                self.scroll.cancel();
                if g.tap
                    && let Some((a, b)) = g.listener.zip(s.hits.listener_at(s.viewport, x, y))
                    && a.key == b.key
                {
                    if b.actions.has(Actions::CLICK) {
                        cx.payload(b.id, input::click(), Position::from(b.local));
                    }
                    if b.actions.has(Actions::PRESS) {
                        cx.bare(b.id, press());
                    }
                }
            }
            Mode::Suppressed => (),
        }
    }
}

#[cfg(all(test, target_os = "linux", feature = "wayland"))]
#[test]
fn mapping_change_preserves_the_last_surface_sample_not_a_new_displacement() {
    let at = Instant::now();
    let mut velocity = Velocity::new(at, [5., 0.]);
    velocity.moved(at + std::time::Duration::from_millis(20), [10., 16.]);
    let mut d = Dispatcher {
        gesture: Some(Gesture {
            source: Source::Touch(9),
            mode: Mode::Pending,
            listener: None,
            port: false,
            tap: true,
            origin: [5., 0.],
            last: [10., 16.],
            start: at,
            velocity,
        }),
        ..Default::default()
    };
    // The protocol adapter still has the surface position at f64 precision.
    let exact = [15.000001, 24.];
    d.remap(|p| p.map(|x| 1.5 * x), |id| (id == 9).then_some(exact));
    let g = d.gesture.unwrap();
    assert_eq!((g.origin, g.last), ([7.5, 0.], exact));
    assert_eq!(
        g.velocity
            .released(at + std::time::Duration::from_millis(25)),
        [375., 1200.]
    );
}
