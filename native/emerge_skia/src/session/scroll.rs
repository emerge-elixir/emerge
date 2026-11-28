//! The display interprets an operation against installed geometry. Absolute
//! receipts coalesce per attachment; a slow layout/draw never replays a wheel
//! delta or becomes the source of a callback. No mutable tree crosses stages.
use super::{dispatch::deliver, *};
use crate::tree::{
    Attachment, Scene, ScrollHit,
    scroll::{Progress, Update},
};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct Inbox(Mutex<HashMap<Attachment, Update>>);
impl Inbox {
    fn post(
        &self,
        key: Attachment,
        axes: [Progress; 2],
        moved: [bool; 2],
        callback: Option<Instant>,
    ) {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(key)
            .or_default()
            .record(axes, moved, callback);
    }
    fn hover(&self, old: Option<(Attachment, usize)>, next: Option<(Attachment, usize)>) {
        let mut inbox = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((key, _)) = old {
            inbox.entry(key).or_default().hover = Some(None);
        }
        if let Some((key, axis)) = next {
            inbox.entry(key).or_default().hover = Some(Some(axis));
        }
    }
    pub fn collect(&self, tree: &mut Tree, scratch: &mut HashMap<Attachment, Update>) -> bool {
        {
            let mut pending = self.0.lock().unwrap_or_else(|p| p.into_inner());
            if pending.is_empty() {
                return false;
            }
            std::mem::swap(&mut *pending, scratch);
        }
        scratch.drain().fold(false, |changed, (key, update)| {
            tree.scroll_input(key, update) || changed
        })
    }
}
#[derive(Clone, Copy)]
enum Capture {
    Thumb {
        key: Attachment,
        axis: usize,
        epoch: u64,
        grip: f32,
    },
    Pan {
        key: Attachment,
        epochs: [u64; 2],
        start: [f32; 2],
        last: [f32; 2],
        active: bool,
    },
}
#[derive(Default)]
pub(super) struct Driver {
    capture: Option<Capture>,
    cursor: Option<[f32; 2]>,
    hover: Option<(Attachment, usize)>,
    predicted: HashMap<Attachment, [Progress; 2]>,
    serial: u64,
}
struct Dispatch<'a, 'env> {
    owner: &'a Owner,
    env: Env<'env>,
    pid: Option<LocalPid>,
    epoch: u64,
}
impl Driver {
    pub fn cancel(&mut self) {
        self.capture = None;
    }
    fn axes(&self, p: ScrollHit) -> [Progress; 2] {
        let installed = p.view.axes;
        self.predicted.get(&p.key).map_or(installed, |pending| {
            std::array::from_fn(|i| {
                if pending[i].epoch == installed[i].epoch && pending[i].input > installed[i].input {
                    pending[i]
                } else {
                    installed[i]
                }
            })
        })
    }
    fn hover(&mut self, owner: &Owner, scene: &Scene) {
        let hover = self
            .cursor
            .and_then(|[x, y]| scene.hits.thumb_at(scene.viewport, x, y));
        if hover != self.hover {
            owner.scroll.hover(self.hover, hover);
            owner.actor.notify();
            self.hover = hover;
        }
    }
    pub fn installed(&mut self, owner: &Owner, scene: &Scene) {
        self.hover(owner, scene);
        self.predicted.retain(|key, pending| {
            scene.hits.scroll_port(*key).is_some_and(|p| {
                (0..2).any(|i| {
                    pending[i].epoch == p.view.axes[i].epoch
                        && pending[i].input > p.view.axes[i].input
                })
            })
        });
        if !self.capture.is_none_or(|c| match c {
            Capture::Thumb {
                key, axis, epoch, ..
            } => scene.hits.scroll_port(key).is_some_and(|p| {
                p.view.axes[axis].epoch == epoch && p.bar(axis).is_some_and(|b| b.travel > 0.)
            }),
            Capture::Pan { key, epochs, .. } => scene
                .hits
                .scroll_port(key)
                .is_some_and(|p| p.view.axes.map(|p| p.epoch) == epochs),
        }) {
            self.cancel();
        }
    }
    fn commit(&mut self, p: ScrollHit, next: [f64; 2], cx: &Dispatch<'_, '_>) {
        let mut axes = self.axes(p);
        let moved = std::array::from_fn(|i| next[i].is_finite() && axes[i].pos != next[i]);
        if moved == [false; 2] {
            return;
        }
        self.serial += 1;
        for (i, a) in axes.iter_mut().enumerate().filter(|(i, _)| moved[*i]) {
            a.pos = next[i];
            a.input = self.serial;
        }
        self.predicted.insert(p.key, axes);
        let to = cx.pid.filter(|_| p.view.emit);
        cx.owner
            .scroll
            .post(p.key, axes, moved, to.map(|_| Instant::now()));
        cx.owner.actor.notify();
        if let Some(pid) = to {
            deliver(cx.env, pid, cx.epoch, p.id, scroll(), axes.map(|p| p.pos));
        }
    }
    fn moved(&self, p: ScrollHit, delta: [f32; 2]) -> [f64; 2] {
        let axes = self.axes(p);
        std::array::from_fn(|i| {
            if p.view.range[i] > 0. && delta[i].is_finite() {
                (axes[i].pos - f64::from(delta[i]) / f64::from(p.view.range[i])).clamp(0., 1.)
            } else {
                axes[i].pos
            }
        })
    }
    fn wheel(&mut self, s: &Scene, at: [f32; 2], delta: [f32; 2], cx: &Dispatch<'_, '_>) {
        // Route the two device components independently (e.g. a horizontal
        // strip inside a vertical page); combine them before notifying a port.
        let [x, y] = at;
        let Some((first, _)) = s.hits.region_at(s.viewport, x, y) else {
            return;
        };
        let mut changes: [Option<(ScrollHit, [f32; 2])>; 2] = [None, None];
        for (i, d) in delta.into_iter().enumerate().filter(|(_, d)| *d != 0.) {
            for p in s.hits.ancestors(Some(first)) {
                let local = if i == 0 {
                    p.vector(d, 0.)
                } else {
                    p.vector(0., d)
                };
                if self.moved(p, local) != self.axes(p).map(|a| a.pos) {
                    if let Some((_, first)) = changes[0].as_mut().filter(|(q, _)| q.key == p.key) {
                        first[0] += local[0];
                        first[1] += local[1];
                    } else {
                        changes[i] = Some((p, local));
                    }
                    break;
                }
            }
        }
        for (p, d) in changes.into_iter().flatten() {
            self.commit(p, self.moved(p, d), cx);
        }
    }
    fn press(&mut self, s: &Scene, x: f32, y: f32, cx: &Dispatch<'_, '_>) -> bool {
        self.cancel();
        let Some((p, axis)) = s.hits.region_at(s.viewport, x, y) else {
            return false;
        };
        if let Some(axis) = axis {
            let bar = p.bar(axis).unwrap();
            let point = p.point(x, y)[axis];
            let mut next = self.axes(p).map(|a| a.pos);
            // Identify a thumb press from the displayed thumb, not its as-yet
            // uninstalled successor. A track press centers it on the pointer.
            let grip = (point - bar.start - (p.view.axes[axis].pos * f64::from(bar.travel)) as f32)
                / bar.length;
            let grip = if (0.0..=1.0).contains(&grip) {
                grip
            } else {
                if let Some(f) = bar.fraction(point, 0.5) {
                    next[axis] = f;
                    self.commit(p, next, cx);
                }
                0.5
            };
            self.capture = Some(Capture::Thumb {
                key: p.key,
                axis,
                epoch: p.view.axes[axis].epoch,
                grip,
            });
            return true;
        }
        self.capture = Some(Capture::Pan {
            key: p.key,
            epochs: p.view.axes.map(|a| a.epoch),
            start: [x, y],
            last: [x, y],
            active: false,
        });
        false
    }
    fn motion(&mut self, s: &Scene, x: f32, y: f32, cx: &Dispatch<'_, '_>) -> bool {
        match self.capture {
            Some(Capture::Thumb {
                key, axis, grip, ..
            }) => {
                if let Some(p) = s.hits.scroll_port(key)
                    && let Some(f) = p
                        .bar(axis)
                        .and_then(|b| b.fraction(p.point(x, y)[axis], grip))
                {
                    let mut next = self.axes(p).map(|a| a.pos);
                    next[axis] = f;
                    self.commit(p, next, cx);
                }
                true
            }
            Some(Capture::Pan {
                key,
                start,
                last,
                active,
                ..
            }) => {
                let Some(p) = s.hits.scroll_port(key) else {
                    self.cancel();
                    return false;
                };
                if !active && (x - start[0]).hypot(y - start[1]) < 8. {
                    return false;
                }
                // Choose the accepting ancestor only after the gesture has a
                // direction. Once captured it remains the owner up to release.
                let Some((p, next)) = s
                    .hits
                    .ancestors(Some(p))
                    .take(if active { 1 } else { usize::MAX })
                    .find_map(|p| {
                        let next = self.moved(p, p.vector(x - last[0], y - last[1]));
                        (active || next != self.axes(p).map(|a| a.pos)).then_some((p, next))
                    })
                else {
                    return false;
                };
                self.commit(p, next, cx);
                self.capture = Some(Capture::Pan {
                    key: p.key,
                    epochs: p.view.axes.map(|a| a.epoch),
                    start,
                    last: [x, y],
                    active: true,
                });
                true
            }
            None => false,
        }
    }
    /// True means this edge/motion belongs to scrolling, not a click.
    pub fn dispatch(
        &mut self,
        owner: &Owner,
        env: Env,
        pid: Option<LocalPid>,
        f: &Bundle,
        event: &input::InputEvent,
    ) -> bool {
        let cx = Dispatch {
            owner,
            env,
            pid,
            epoch: f.route.epoch,
        };
        use input::InputEvent::*;
        match event {
            CursorScroll { dx, dy, x, y } => {
                self.wheel(&f.scene, [*x, *y], [*dx, *dy], &cx);
                false
            }
            CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == input::pointer::BUTTON_LEFT => {
                if *action == input::ACTION_PRESS {
                    self.press(&f.scene, *x, *y, &cx)
                } else {
                    matches!(
                        self.capture.take(),
                        Some(Capture::Thumb { .. } | Capture::Pan { active: true, .. })
                    )
                }
            }
            CursorPos { x, y } => {
                self.cursor = Some([*x, *y]);
                self.hover(owner, &f.scene);
                self.motion(&f.scene, *x, *y, &cx)
            }
            Focused { focused: false } | CursorEntered { entered: false } => {
                self.cursor = None;
                self.hover(owner, &f.scene);
                self.cancel();
                false
            }
            _ => false,
        }
    }
}
rustler::atoms! {scroll}
