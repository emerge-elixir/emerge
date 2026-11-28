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
        let escape = parent.map_or(viewport, |p| p.escape_clip);
        let clip = if e.slots[k].slot.is_some_and(Slot::escapes) {
            escape
        } else {
            parent.map_or(viewport, |p| p.child_clip)
        };
        // A borrowed media mask turns with its image; earlier clips stay put.
        let borrowed = parent.and_then(|_| e.layout_parent(k)).filter(|&p| {
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
    pub(in crate::tree) fn drop_recordings(&mut self, k: Key) {
        self.scroll.remove(k);
        if let Some(s) = self.projected.get_mut(k) {
            *s = Default::default();
        }
    }
    pub(in crate::tree) fn projection(&self, k: Key) -> Option<&paint::Projected> {
        self.projected.get(k)?.projection.as_ref()
    }
    pub(in crate::tree) fn painted(&self, k: Key) -> paint::PaintContext {
        self.projection(k).map_or(self.paint[k], |p| p.ctx)
    }
    // Populate the existing projection slots only while compiling a missing
    // local recording. The viewport and the current scroll offset are not
    // recording inputs. The caller supplies the local coordinate origin.
    pub(in crate::tree) fn record_geometry(
        &mut self,
        e: &Elements,
        k: Key,
        origin: [f32; 2],
        parent: Option<paint::PaintContext>,
        descend: bool,
        outputs: Outputs,
    ) {
        let mut f = self.local_frames[k];
        f.x -= origin[0];
        f.y -= origin[1];
        let open = Rect {
            x: -1e30,
            y: -1e30,
            width: 2e30,
            height: 2e30,
        };
        if parent.is_none() {
            f.x = -pixels(e.attrs[k].offset[0], self.paint[k].scale);
            f.y = -pixels(e.attrs[k].offset[1], self.paint[k].scale);
        }
        let mut ctx = paint::PaintContext {
            transformed: true,
            ..self.paint[k]
        }
        .geometry(e, k, f, open, parent, true);
        if parent.is_none() {
            ctx.offset = [0.; 2];
        }
        self.projected[k].projection = Some(paint::Projected::new(e, k, ctx, self.runs.get(k)));
        let origin = if parent.is_none() {
            [self.local_frames[k].x, self.local_frames[k].y]
        } else {
            origin
        };
        let kids = if descend { e.children(k) } else { &[] };
        for c in kids.iter().copied().chain(
            e.nearby(k)
                .filter_map(|(s, c)| (s == Slot::Behind).then_some(c)),
        ) {
            self.projected[c].absorbed(outputs);
            self.record_geometry(e, c, origin, Some(ctx), true, outputs);
        }
    }
    pub(in crate::tree) fn inline_record_geometry(
        &mut self,
        e: &Elements,
        k: Key,
        host: Key,
        outputs: Outputs,
    ) -> paint::PaintContext {
        // Fragment positions and continuation pivots share paragraph space.
        // The stationary mask/pose belongs to the reference, not to this span.
        let f = self.local_frames[host];
        let open = Rect {
            x: -1e30,
            y: -1e30,
            width: 2e30,
            height: 2e30,
        };
        let ctx = paint::PaintContext {
            frame: Rect { x: 0., y: 0., ..f },
            clip: open,
            boundary: open,
            child_clip: open,
            escape_clip: open,
            offset: [0.; 2],
            content_scroll: [0.; 2],
            ..self.paint[host]
        };
        self.record_geometry(e, k, [f.x, f.y], Some(ctx), true, outputs);
        ctx
    }
    // Visibility consumes the actual exposure. Banded output is recorded in
    // its establishing plane instead: never clip it to one scroll sample.
    pub(in crate::tree) fn exposure(&self, e: &Elements, k: Key) -> paint::PaintContext {
        let s = self.paint[k];
        if s.content.is_none() {
            return s;
        }
        let parent = e.slots[k].parent.map(|p| self.exposure(e, p));
        s.geometry(e, k, self.local_frames[k], self.viewport, parent, true)
    }
    pub(in crate::tree) fn band_owner(&self, e: &Elements, k: Key) -> bool {
        self.paint[k]
            .content
            .and_then(|p| self.scroll.recorded(p))
            .is_some_and(|w| w.banded && !w.hosts(k))
            && self.parts[k].separate_body
            && !(e.slots[k].shape == Shape::Image
                && e.layout_parent(k)
                    .is_some_and(|p| e.attrs[p].clips_media() && !e.attrs[p].scroll.port()))
    }
    fn enqueue_bands(&mut self, e: &Elements, host: Key, port: Key) {
        let jobs = &mut self.jobs[Project as usize];
        let d = e.depth(host) + 1;
        self.scroll
            .bands_rev(e, host, port, &self.parts, |section| {
                if let scroll::BandPart::Branch(c) = section {
                    jobs.push(d, c, Outputs::Both);
                }
            });
        if host != port {
            for (_, c) in e.nearby(host) {
                jobs.push(d, c, Outputs::Both);
            }
        }
    }
    pub(super) fn project(&mut self, e: &Elements, k: Key, out: &mut paint::Output) {
        let stable = self.paint[k];
        let ctx = if stable.content.is_some() {
            let p = e.slots[k].parent.expect("content edge");
            // A nested port may have a pending receipt while its branch is hidden.
            // An unprojected parent cannot currently expose it. Reveal projects it.
            if self.paint[p].content.is_some() && self.projection(p).is_none() {
                return;
            }
            if e.slots[k].slot.is_some_and(Slot::escapes) && !self.shows_anchor(e, p) {
                return;
            }
            let mut parent = self.painted(p);
            if e.slots[k].slot.is_none() && self.scroll.recorded(p).is_some_and(|w| w.banded) {
                parent.content_scroll = [0.; 2];
                parent.child_clip = Rect::UNBOUNDED;
                parent.escape_clip = Rect::UNBOUNDED;
                parent.clip = Rect::UNBOUNDED;
            }
            let ctx = stable.geometry(
                e,
                k,
                self.local_frames[k],
                self.viewport,
                Some(parent),
                true,
            );
            self.projected[k].projection = Some(paint::Projected::new(e, k, ctx, self.runs.get(k)));
            ctx
        } else {
            stable
        };
        let revealed =
            stable.content.is_some() && self.parts[k].anchors() && self.refresh_anchor(e, k);
        if self.band_owner(e, k) {
            let in_body = e.slots[k].slot.is_none_or(|s| s == Slot::Behind)
                && e.slots[k].parent.is_some_and(|p| self.band_owner(e, p));
            if !in_body {
                out.prepare_owner(e, self, k);
            }
            if self.parts[k].facts().exports != [false; 2] {
                for &c in e.owned(k) {
                    if e.slots[c].slot.is_some_and(Slot::escapes)
                        || self.parts[c].facts().exports != [false; 2]
                    {
                        self.enqueue(Project, c, e);
                    }
                }
            }
            return;
        }
        if let Some(port) = scroll::order_port(e, k)
            .filter(|&p| p != k && self.scroll.recorded(p).is_some_and(|w| w.banded))
        {
            self.enqueue_bands(e, k, port);
            return;
        }
        let delegated = stable.scrollport.then(|| scroll::delegated(e, k)).flatten();
        if let Some((host, port)) = delegated
            .map(|h| (h, k))
            .or_else(|| scroll::order_port(e, k).map(|p| (k, p)))
        {
            let window = self.exposure(e, port);
            let hs = self.paint[host];
            let hc = if host == port {
                window
            } else {
                hs.geometry(
                    e,
                    host,
                    self.local_frames[host],
                    self.viewport,
                    Some(window),
                    true,
                )
            };
            self.scroll.select(
                e,
                host,
                port,
                &self.parts,
                scroll::Query::new(e, (port, window), (host, hc), hs),
            );
            if port == k
                && let Some(range) = self.scroll.selection(e, host, port)
            {
                let mut w = std::mem::take(self.scroll.recording(k).unwrap());
                let structure = out.prepare_window(e, self, k, host, range, &mut w);
                let fixed = w.stationary(self.parts[k].sides(), ctx.frame);
                let banded = w.banded;
                *self.scroll.recording(k).unwrap() = w;
                self.edge_epoch += u64::from(structure);
                if (!fixed || revealed) && stable.content.is_some() {
                    for (_, c) in e.nearby(k) {
                        self.enqueue(Project, c, e);
                    }
                }
                if banded && structure {
                    if k == host {
                        self.enqueue_bands(e, k, k);
                    } else {
                        self.enqueue(Project, host, e);
                    }
                }
                let kept = (banded && !structure).then_some(host);
                if kept.is_some() || stable.content.is_some() {
                    self.refresh_scroll_bindings(e, kept, k);
                }
                return;
            }
        }
        if let Some(w) = self.scroll.recording(k)
            && w.key.is_some()
        {
            self.edge_epoch += 1;
            *w = Default::default();
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
