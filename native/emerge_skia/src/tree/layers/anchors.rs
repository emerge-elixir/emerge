//! Test the attachment against the exposed scroll window, not the panel's
//! ink against the port. This decision controls admission to the common FIFO;
//! a sealed scene needs neither the geometry used here nor a live-tree lookup.
use super::*;
use skia_safe::Matrix;
mod window;
use window::{Area, Window};

impl Layers {
    pub(in crate::tree) fn shows_anchor(&self, e: &Elements, k: Key) -> bool {
        if self.scroll.exhaustive() {
            Geometry::of(self).visible(e, k)
        } else {
            self.projected.get(k).is_none_or(|p| !p.anchor_hidden)
        }
    }
    pub(in crate::tree) fn outer_window_changed(&mut self, e: &Elements, k: Key) {
        // An edit above the port can change its exposed shape without changing
        // its rectangular bounds. Stop at the ordinary content edge: the port
        // projects its selected branches. A scroll receipt never walks this path.
        if !self.projected.is_empty() && self.paint.get(k).is_some_and(|p| p.content.is_none()) {
            self.enqueue_exposed_ports(e, k);
        }
    }
    fn enqueue_exposed_ports(&mut self, e: &Elements, k: Key) {
        if e.slots[k].stats.escapes == 0 {
            return;
        }
        if e.attrs[k].scroll.port() {
            self.enqueue(Project, k, e);
            for (_, c) in e.nearby(k) {
                self.enqueue_exposed_ports(e, c);
            }
        } else {
            for &c in e.owned(k) {
                self.enqueue_exposed_ports(e, c);
            }
        }
    }
    pub(super) fn refresh_anchor(&mut self, e: &Elements, k: Key) -> bool {
        let hidden = !Geometry::of(self).visible(e, k);
        let changed = hidden != std::mem::replace(&mut self.projected[k].anchor_hidden, hidden);
        self.edge_epoch += u64::from(changed);
        changed && !hidden
    }
    pub(super) fn refresh_scroll_bindings(&mut self, e: &Elements, host: Option<Key>, port: Key) {
        let jobs = &mut self.jobs[Project as usize];
        let geometry = Geometry {
            frames: &self.local_frames,
            paint: &self.paint,
            viewport: self.viewport,
        };
        self.scroll.bindings(
            e,
            host,
            port,
            &self.parts,
            self.paint[port]
                .content
                .map(|_| !self.projected[port].anchor_hidden),
            |k| {
                let port = e.attrs[k].scroll.port();
                let hidden = !port && !geometry.visible(e, k);
                if port {
                    jobs.push(e.depth(k), k, Outputs::Both);
                } else if hidden != self.projected[k].anchor_hidden {
                    self.projected[k].anchor_hidden = hidden;
                    self.edge_epoch += 1;
                    // The host is already projected in this stable run. Only
                    // its escaping roots may be missing on first exposure.
                    if !hidden {
                        for (slot, c) in e.nearby(k) {
                            if slot.escapes() {
                                jobs.push(e.depth(c), c, Outputs::Both);
                            }
                        }
                    }
                }
                !hidden
            },
        );
    }
}
struct Geometry<'a> {
    frames: &'a SecondaryMap<Key, Rect>,
    paint: &'a SecondaryMap<Key, paint::PaintContext>,
    viewport: Rect,
}
impl<'a> Geometry<'a> {
    fn of(l: &'a Layers) -> Self {
        Self {
            frames: &l.local_frames,
            paint: &l.paint,
            viewport: l.viewport,
        }
    }
    fn visible(&self, e: &Elements, host: Key) -> bool {
        self.exposed(
            e,
            host,
            Scope::Anchor,
            Matrix::new_identity(),
            Area::new(self.frames[host]),
        )
    }
    fn exposed(
        &self,
        e: &Elements,
        mut k: Key,
        mut scope: Scope,
        mut m: Matrix,
        area: Area<'_>,
    ) -> bool {
        loop {
            let node = &e.slots[k];
            let a = &e.attrs[k];
            let f = self.frames[k];
            let ctx = self.paint[k];
            let base = node.parent.map_or(Rect::default(), |p| self.frames[p]);
            if a.posed() {
                m = a.pose.matrix(Rect { x: 0., y: 0., ..f }) * m;
            }
            let scroll = node
                .parent
                .filter(|_| node.slot.is_none())
                .map_or([0.; 2], |p| self.paint[p].content_scroll);
            m.post_translate((
                f.x - base.x + pixels(a.offset[0], ctx.scale) - scroll[0],
                f.y - base.y + pixels(a.offset[1], ctx.scale) - scroll[1],
            ));
            let Some(p) = node.parent else {
                return matches!(scope, Scope::Anchor)
                    || area.within(Window::new(m, self.viewport, [0.; 4]), |a| a.visible());
            };
            let parent = &e.attrs[p];
            let escape = node.slot.is_some_and(Slot::escapes);
            scope = match scope {
                // Submenus of an already admitted panel do not have to reach
                // its original port. A new ordinary scroll edge starts a test.
                Scope::Anchor if escape => return true,
                Scope::Anchor if node.slot.is_none() && parent.scroll.port() => Scope::Ordinary,
                _ if escape => Scope::Escape,
                s => s,
            };
            let clips = match scope {
                Scope::Anchor => false,
                Scope::Ordinary => {
                    parent.scroll.port()
                        || parent.clips_media()
                        || e.slots[p].shape.clips_children()
                }
                Scope::Escape => parent.clip_nearby,
            };
            if clips {
                let f = Rect {
                    x: 0.,
                    y: 0.,
                    ..base
                };
                let scale = self.paint[p].scale;
                let r = parent.decor.inner_rect(f, scale);
                let radii = parent
                    .decor
                    .rounded_inner(f, scale)
                    .map_or([0.; 4], |c| c.radii);
                return area.within(Window::new(m, r, radii), |area| {
                    self.exposed(e, p, scope, m, area)
                });
            }
            k = p;
        }
    }
}
#[derive(Clone, Copy)]
enum Scope {
    Anchor,   // below the first ordinary scroll edge: no port has gated this panel
    Ordinary, // above it: use the portion of the port actually on screen
    Escape,   // an enclosing panel escaped; only explicit guards remain
}
