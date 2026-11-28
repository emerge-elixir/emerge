use super::*;
use crate::{
    keys::CanonicalKey as Key,
    tree::{Navigation, Stop},
};

impl Driver {
    pub fn key(
        &mut self,
        s: &Scene,
        focus: Option<Stop>,
        key: Key,
        line: f32,
        cx: &Context<'_, '_>,
    ) {
        let axis = match key {
            Key::ArrowLeft | Key::ArrowRight => Some(0),
            Key::ArrowUp | Key::ArrowDown => Some(1),
            Key::PageUp | Key::PageDown | Key::Home | Key::End => None,
            _ => return,
        };
        // Prefer vertical paging. A horizontal strip can page across when
        // there is no vertical scroll range in this route.
        let mut horizontal = None;
        let mut vertical = false;
        let mut step = |p: ScrollHit| {
            vertical |= p.view.range[1] > 0.;
            if axis.is_none() && !vertical && horizontal.is_none() {
                horizontal = self.key_step(p, 0, key, line);
            }
            self.key_step(p, axis.unwrap_or(1), key, line)
        };
        let context = focus.zip(s.navigation.as_ref()).and_then(|(stop, nav)| {
            nav.enclosure(stop.key)
                .or_else(|| nav.enclosure(stop.parent?))
                .map(|p| (p, nav))
        });
        let chosen = if let Some((first, nav)) = context {
            std::iter::successors(Some(first), |(link, _)| nav.enclosure(link.parent?))
                .find_map(|(_, p)| step(p))
        } else if let Some([x, y]) = self.cursor {
            let first = s.hits.region_at(s.viewport, x, y).map(|(p, _)| p);
            s.hits.ancestors(first).find_map(step)
        } else {
            s.hits.keyboard_ports().find_map(step)
        };
        if let Some((p, next)) = chosen.or(horizontal.filter(|_| !vertical)) {
            self.interrupt();
            self.commit(p, next, cx);
        }
    }
    fn key_step(
        &self,
        p: ScrollHit,
        axis: usize,
        key: Key,
        line: f32,
    ) -> Option<(ScrollHit, [f64; 2])> {
        let range = f64::from(p.view.range[axis]);
        if range <= 0. {
            return None;
        }
        let mut next = self.axes(p).map(|a| a.pos);
        let page = f64::from([p.frame.width, p.frame.height][axis]);
        let delta = match key {
            Key::ArrowUp | Key::ArrowLeft => -f64::from(line),
            Key::ArrowDown | Key::ArrowRight => f64::from(line),
            Key::PageUp => -page,
            Key::PageDown => page,
            Key::Home => -range,
            Key::End => range,
            _ => return None,
        };
        let v = (next[axis] + delta / range).clamp(0., 1.);
        if v == next[axis] {
            return None;
        }
        next[axis] = v;
        Some((p, next))
    }
    pub fn reveal(&mut self, nav: &Navigation, stop: Stop, cx: &Context<'_, '_>) {
        let mut bounds = stop.bounds;
        let mut parent = stop.parent;
        while let Some((link, p)) = parent.and_then(|k| nav.enclosure(k)) {
            let current = self.axes(p).map(|a| a.pos);
            let v = link.viewport;
            let next = std::array::from_fn(|i| {
                reveal_axis(
                    current[i],
                    p.view.range[i],
                    [bounds.x, bounds.y][i],
                    [bounds.width, bounds.height][i],
                    [v.x, v.y][i],
                    [v.width, v.height][i],
                )
            });
            self.commit(p, next, cx);
            let [rx, ry] = p.view.range;
            bounds.x -= (next[0] * f64::from(rx)) as f32;
            bounds.y -= (next[1] * f64::from(ry)) as f32;
            bounds = link.outward(bounds);
            parent = link.parent;
        }
    }
}
fn reveal_axis(current: f64, range: f32, start: f32, size: f32, origin: f32, extent: f32) -> f64 {
    if range <= 0.
        || extent <= 0.
        || ![start, size, origin, extent]
            .into_iter()
            .all(f32::is_finite)
    {
        return current;
    }
    let [range, start, size, origin, extent] = [range, start, size, origin, extent].map(f64::from);
    let leading = start - origin;
    let trailing = leading + size - extent;
    let offset = current * range;
    // A larger-than-viewport box already spanning both edges stays put.
    let desired = offset.clamp(leading.min(trailing), leading.max(trailing));
    if desired == offset {
        current
    } else {
        (desired / range).clamp(0., 1.)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reveal_uses_nearest_edge_and_does_not_jitter_an_oversized_stop() {
        assert_eq!(reveal_axis(0., 100., 100., 20., 10., 60.), 0.5);
        assert_eq!(reveal_axis(0.8, 100., 65., 80., 0., 60.), 0.8);
        assert_eq!(reveal_axis(1., 100., 60., 10., 10., 40.), 0.5);
    }
}
