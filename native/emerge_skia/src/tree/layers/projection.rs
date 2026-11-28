//! Projection is the last, shallow-first part of paint preparation. A receipt
//! changes a port, not the inherited inputs or the composed reach of every row.
use super::*;

impl paint::PaintContext {
    pub(super) fn geometry(
        mut self,
        e: &Elements,
        k: Key,
        mut f: Rect,
        viewport: Rect,
        parent: Option<Self>,
        project: bool,
    ) -> Self {
        let a = &e.attrs[k];
        let shape = e.slots[k].shape;
        let ordinary = e.slots[k].slot.is_none();
        self.offset = std::array::from_fn(|i| {
            parent.map_or(0., |p| {
                p.offset[i]
                    - if project && ordinary {
                        p.content_scroll[i]
                    } else {
                        0.
                    }
            }) + pixels(a.offset[i], self.scale)
        });
        f.x += self.offset[0];
        f.y += self.offset[1];
        self.frame = f;
        if !project {
            // Own contributions, never clipped by today's viewport. Descendant
            // overflow (including exports from an empty owner) is composed separately.
            self.clip = f;
            self.child_clip = f;
            return self;
        }
        let escape = parent.map_or(viewport, |p| {
            if ordinary {
                p.content_escape
            } else {
                p.escape_clip
            }
        });
        let clip = if e.slots[k].slot.is_some_and(Slot::escapes) {
            escape
        } else {
            parent.map_or(viewport, |p| p.child_clip)
        };
        // A borrowed media mask turns with its image; earlier clips stay put.
        let borrowed = e.layout_parent(k).filter(|&p| {
            shape == Shape::Image
                && a.posed()
                && !a.image_host_clip
                && !e.attrs[p].scroll.port()
                && e.attrs[p].clips_media()
        });
        let outer = borrowed.map_or(clip, |p| {
            let host = parent.unwrap();
            e.slots[p].shape.child_clip(host.clip, host.frame)
        });
        let clip = if a.posed() && shape != Shape::Image {
            a.pose.local_clip(f, outer)
        } else {
            outer
        };
        let escape_clip = if !a.posed() {
            escape
        } else if outer == escape && shape != Shape::Image {
            clip
        } else {
            a.pose.local_clip(f, escape)
        };
        let inner = a.decor.inner_rect(f, self.scale);
        self.escape_boundary = escape;
        self.escape_clip = if a.clip_nearby {
            escape_clip.intersection(inner).unwrap_or_default()
        } else {
            escape_clip
        };
        self.content_escape = if self.scrollport {
            self.escape_clip.intersection(inner).unwrap_or_default()
        } else {
            self.escape_clip
        };
        self.boundary = outer;
        self.clip = clip;
        self.child_clip = if shape == Shape::Image && a.posed() {
            borrowed.map_or(f, |p| {
                let h = parent.unwrap();
                e.attrs[p].decor.inner_rect(h.frame, h.scale)
            })
        } else if shape == Shape::Image {
            if ordinary {
                clip.media_clip(parent)
            } else {
                clip
            }
        } else if a.clips_media() || self.scrollport {
            clip.intersection(inner).unwrap_or_default()
        } else {
            shape.child_clip(clip, f)
        };
        self
    }
}
impl Layers {
    pub(in crate::tree) fn reserve_projection(
        &mut self,
        scrolls: usize,
        keys: usize,
    ) -> Result<(), String> {
        self.scroll.reserve(scrolls, keys)?;
        if scrolls != 0 {
            self.jobs[Project as usize].reserve(keys, self.owners)?;
            self.projected
                .try_set_capacity(keys)
                .map_err(pool::capacity_error)?;
        }
        Ok(())
    }
    pub(in crate::tree) fn projection(&self, k: Key) -> Option<&paint::Projected> {
        self.projected.get(k)?.as_ref()
    }
    pub(in crate::tree) fn painted(&self, k: Key) -> paint::PaintContext {
        self.projection(k).map_or(self.paint[k], |p| p.ctx)
    }
    pub(super) fn project(&mut self, e: &Elements, k: Key) {
        let stable = self.paint[k];
        let ctx = if stable.content.is_some() {
            let p = e.slots[k].parent.expect("content edge");
            // A nested port may have a pending receipt while its branch is hidden.
            // An unprojected parent cannot currently expose it. Reveal projects it.
            if self.paint[p].content.is_some() && self.projection(p).is_none() {
                return;
            }
            let ctx = stable.geometry(
                e,
                k,
                self.local_frames[k],
                self.viewport,
                Some(self.painted(p)),
                true,
            );
            self.projected[k] = Some(paint::Projected::new(e, k, ctx, self.runs.get(k)));
            ctx
        } else {
            stable
        };
        if let Some(port) = scroll::order_port(e, k) {
            let window = self.painted(port);
            self.scroll.select(
                e,
                k,
                port,
                &self.parts,
                scroll::Query::new(e, (port, window), (k, ctx), stable),
            );
        }
        // Do not re-open the order from either collector, or project descendants
        // of an invisible row. Port-owned mounts are fixed on a position-only turn.
        let work = &mut self.jobs[Project as usize];
        let depth = e.depth(k) + 1;
        for &c in self.scroll.children_rev(e, k) {
            work.push(depth, c, Outputs::Both);
        }
        if stable.content.is_some() {
            for (_, c) in e.nearby(k) {
                work.push(depth, c, Outputs::Both);
            }
        }
    }
}
