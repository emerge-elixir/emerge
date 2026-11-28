//! Tab stops are declarations, not visible hit rectangles. Pack their boxes in
//! unscrolled port space. A controller replacement can refresh port receipts
//! without reopening tab order; ordinary native scrolling shares both banks.
use super::*;
use skia_safe::Matrix;

#[derive(Clone, Copy)]
pub(crate) struct Stop {
    pub key: Attachment,
    pub id: u64,
    pub actions: Actions,
    pub parent: Option<Attachment>,
    pub bounds: Rect, // in the nearest port's axes
}
#[derive(Clone, Copy)]
pub(crate) struct Enclosure {
    key: Attachment,
    id: u64,
    frame: Rect,
    pub viewport: Rect,
    pub parent: Option<Attachment>,
    to_parent: Matrix,
}
impl Enclosure {
    pub fn outward(self, r: Rect) -> Rect {
        mapped(self.to_parent, r)
    }
}
fn mapped(m: Matrix, r: Rect) -> Rect {
    let b = m.map_rect(sk(r)).0;
    Rect {
        x: b.left,
        y: b.top,
        width: b.width(),
        height: b.height(),
    }
}
#[derive(Default)]
struct Order {
    stops: Vec<Stop>,
    by_key: HashMap<Attachment, usize>,
    ports: Vec<Enclosure>,
    by_port: HashMap<Attachment, usize>,
    autofocus: Option<(u64, Attachment)>,
}
#[derive(Clone)]
pub(crate) struct Navigation {
    order: Arc<Order>,
    // Same ordinals as order.ports; no lookup or pose copying to refresh these.
    views: Arc<Views>,
}
impl Navigation {
    pub fn same_receipts(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.views, &other.views)
    }
    pub fn autofocus(&self) -> Option<(u64, Attachment)> {
        self.order.autofocus
    }
    pub fn enclosure(&self, key: Attachment) -> Option<(Enclosure, ScrollHit)> {
        let &i = self.order.by_port.get(&key)?;
        let p = self.order.ports[i];
        Some((
            p,
            ScrollHit::unprojected(key, p.id, self.views.0[i], p.frame),
        ))
    }
    pub fn get(&self, key: Attachment) -> Option<Stop> {
        self.order.by_key.get(&key).map(|&i| self.order.stops[i])
    }
    pub fn tab(&self, current: Option<Attachment>, back: bool) -> Option<Stop> {
        let stops = &self.order.stops;
        if stops.is_empty() {
            return None;
        }
        let i = current.and_then(|k| self.order.by_key.get(&k).copied());
        let next = if back {
            i.unwrap_or(0).checked_sub(1).unwrap_or(stops.len() - 1)
        } else {
            i.map_or(0, |i| (i + 1) % stops.len())
        };
        Some(stops[next])
    }
    #[cfg(test)]
    pub(in crate::tree) fn same_order(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.order, &other.order)
    }
}
impl Order {
    fn visit(
        &mut self,
        e: &Elements,
        l: &Layers,
        k: Key,
        parent: Option<Attachment>,
        matrix: Matrix,
    ) {
        let m = &e.slots[k];
        if m.stats.tab_stops == 0 {
            return;
        }
        let a = &e.attrs[k];
        let ctx = l.paint[k]; // stable, NOT the current visible projection
        let f = ctx.frame;
        let matrix = if a.posed() {
            let local = a.pose.matrix(f);
            if local.invert().is_none() {
                return;
            } // same usable plane as hit packing
            matrix * local
        } else {
            matrix
        };
        let bounds = mapped(matrix, f);
        if !ctx.zoom.is_finite() || ctx.zoom == 0. || !matrix.is_finite() || !sk(bounds).is_finite()
        {
            return;
        }
        let key = Attachment(k, m.attachment);
        if a.actions.focusable() && f.width > 0. && f.height > 0. {
            self.by_key.insert(key, self.stops.len());
            self.stops.push(Stop {
                key,
                id: m.id,
                actions: a.actions,
                bounds,
                parent,
            });
            let mount = if a.autofocus {
                e.attrs
                    .version
                    .as_ref()
                    .and_then(|v| v.get(k))
                    .map_or(0, |r| r.autofocus_mount)
            } else {
                0
            };
            if mount > self.autofocus.map_or(0, |(v, _)| v) {
                self.autofocus = Some((mount, key));
            }
        }
        let port = a.scroll.port();
        if port {
            self.by_port.insert(key, self.ports.len());
            self.ports.push(Enclosure {
                key,
                id: m.id,
                frame: f,
                viewport: a.decor.inner_rect(f, ctx.scale),
                parent,
                to_parent: matrix,
            });
        }
        if m.stats.child_stops != 0 {
            for (slot, c) in e.order(k).owned() {
                if port && slot.is_none() {
                    self.visit(e, l, c, Some(key), Matrix::new_identity());
                } else {
                    self.visit(e, l, c, parent, matrix);
                }
            }
        }
    }
}
impl buffers::Packed<2> for Order {
    fn capacities(&self) -> [usize; 2] {
        [
            self.stops.capacity().min(self.by_key.capacity()),
            self.ports.capacity().min(self.by_port.capacity()),
        ]
    }
    fn reserve(&mut self, [stops, ports]: [usize; 2]) -> Result<(), String> {
        pool::reserve(&mut self.stops, stops)?;
        pool::reserve(&mut self.ports, ports)?;
        self.by_key
            .try_reserve(stops.saturating_sub(self.by_key.len()))
            .map_err(pool::capacity_error)?;
        self.by_port
            .try_reserve(ports.saturating_sub(self.by_port.len()))
            .map_err(pool::capacity_error)
    }
    fn clear(&mut self) {
        self.stops.clear();
        self.by_key.clear();
        self.ports.clear();
        self.by_port.clear();
        self.autofocus = None;
    }
}
#[derive(Default)]
struct Views(Vec<scroll::View>);
impl buffers::Packed<1> for Views {
    fn capacities(&self) -> [usize; 1] {
        [self.0.capacity()]
    }
    fn reserve(&mut self, [ports]: [usize; 1]) -> Result<(), String> {
        pool::reserve(&mut self.0, ports)
    }
    fn clear(&mut self) {
        self.0.clear();
    }
}
pub(super) struct Bank {
    order: Buffers<Order, 2>,
    views: Buffers<Views, 1>,
}
impl Bank {
    pub fn available(&self, l: &Layers) -> bool {
        (!l.navigation_changed || self.order.available())
            && (!(l.navigation_changed || l.focus_ports_changed) || self.views.available())
    }
    pub fn prepare(&mut self, e: &Elements, l: &mut Layers, root: Key) {
        debug_assert!(self.available(l));
        if l.navigation_changed {
            self.order
                .build
                .visit(e, l, root, None, Matrix::new_identity());
            self.order.publish();
            l.navigation_changed = false;
            l.focus_ports_changed = true;
        }
        if std::mem::take(&mut l.focus_ports_changed) {
            self.views.build.0.extend(
                self.order
                    .current()
                    .ports
                    .iter()
                    .map(|p| l.scroll.view(p.key.0)),
            );
            self.views.publish();
        }
    }
    pub fn snapshot(&self) -> Navigation {
        Navigation {
            order: self.order.current().clone(),
            views: self.views.current().clone(),
        }
    }
}
impl Output {
    pub(in crate::tree) fn reserve_navigation(
        &mut self,
        stops: usize,
        ports: usize,
        b: Budget,
    ) -> Result<(), String> {
        if stops > 0 {
            let bank = self.navigation.get_or_insert_with(|| Bank {
                order: Buffers::new(self.hits.readers()),
                views: Buffers::new(self.hits.readers()),
            });
            bank.order.reserve([stops, ports], b)?;
            bank.views.reserve([ports], b)?;
        }
        Ok(())
    }
}
