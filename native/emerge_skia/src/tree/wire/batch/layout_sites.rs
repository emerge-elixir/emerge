//! Check the final incoming edge, including moves of closed branches. A
//! height-dependent packed width is usable at placement, but cannot be an
//! input to an earlier width split. Never substitute last frame's geometry.
use super::*;

impl Batch {
    fn declaration<'a>(&'a self, e: &'a Elements, id: u64) -> (Shape, &'a Attrs) {
        let edit = self.get(id);
        let shape = edit.map_or_else(|| e.slots[e.ids[&id]].shape, |s| s.shape);
        let attrs = edit
            .and_then(|s| s.attrs.as_ref().map(|(a, _)| a))
            .unwrap_or_else(|| &e.attrs[e.ids[&id]]);
        (shape, attrs)
    }
    // Borrow either a replacement order or its kept counterpart, never both.
    fn layout_children<'a>(&'a self, e: &'a Elements, id: u64) -> impl Iterator<Item = u64> + 'a {
        let (wire, resident): (&[u64], &[Key]) = match self.get(id).and_then(|s| s.order) {
            Some(order) => (self.orders.get(&order), &[]),
            None => (&[], e.children(e.ids[&id])),
        };
        wire.iter()
            .copied()
            .chain(resident.iter().map(|c| e.slots[*c].id))
    }
    fn site_parent(&self, e: &Elements, id: u64) -> Option<u64> {
        self.get(id).map_or_else(
            || e.slots[e.ids[&id]].parent.map(|p| e.slots[p].id),
            |s| s.parent,
        )
    }
    fn needs_early_width(&self, e: &Elements, id: u64) -> bool {
        let (_, a) = self.declaration(e, id);
        if matches!(a.sizing.width, Length::Px(_)) || !a.sizing.height.limits_column_height() {
            return false;
        }
        // A mixed cross-axis policy itself reads packed content in Flow.
        a.sizing.width.reads_fill_content()
            || self.site_parent(e, id).is_some_and(|p| {
                let (shape, parent) = self.declaration(e, p);
                !(matches!(parent.sizing.width, Length::Px(_))
                    && (matches!(shape, Shape::El | Shape::Column | Shape::WrappedColumn)
                        || (shape == Shape::Row && a.sizing.width.weight().is_some())))
            })
    }
    fn check_column_basis(&self, e: &Elements, id: u64) -> Result<()> {
        if !self.needs_early_width(e, id) {
            return Ok(());
        }
        let (_, a) = self.declaration(e, id);
        if a.sizing.height.weight().is_some()
            && self.site_parent(e, id).is_some_and(|p| {
                let (shape, attrs) = self.declaration(e, p);
                shape.cross_height_px(attrs.sizing).is_none()
                    && !(shape == Shape::Column && matches!(attrs.sizing.height, Length::Px(_)))
            })
        {
            return Err("WrappedColumn packed width is needed before its height is assigned; declare a pixel width".into());
        }
        let fixed = |child| {
            let (shape, a) = self.declaration(e, child);
            shape.has_intrinsic_height(a.sizing)
        };
        if !self.layout_children(e, id).all(fixed) {
            return Err(early_heights());
        }
        Ok(())
    }
    // Check the shared group once, not once for each packed-width child.
    // A sparse target may change a slot without naming its enclosing Column.
    fn check_declared_group(&self, e: &Elements, id: u64) -> Result<()> {
        let (shape, column) = self.declaration(e, id);
        if shape != Shape::Column || !matches!(column.sizing.height, Length::Px(_)) {
            return Ok(());
        }
        let inspect = |(reads, known), c| {
            let (kind, a) = self.declaration(e, c);
            (
                reads || kind.reads_group_height(a.sizing, column.sizing),
                known && a.sizing.height.declared_height_slot(),
            )
        };
        let facts = self.layout_children(e, id).fold((false, true), inspect);
        if facts == (true, false) {
            return Err("WrappedColumn packed width is needed before its height is assigned; a Column share depends on reflow. Declare a pixel width or width-independent height slots".into());
        }
        Ok(())
    }
    pub(super) fn check_layout_sites(
        &self,
        e: &Elements,
        outside_parent: Option<Key>,
    ) -> Result<()> {
        if let Some(p) = outside_parent.filter(|&p| {
            e.slots[p].shape == Shape::Column && matches!(e.attrs[p].sizing.height, Length::Px(_))
        }) {
            let id = e.slots[p].id;
            // A paint edit, or one declared height replacing another, cannot
            // make an already admitted group depend on width reflow.
            let changed = self.post.first().is_some_and(|&i| {
                let s = &self.entries[i];
                let Some((a, _)) = &s.attrs else { return false };
                e.live(s.id).is_none_or(|c| {
                    let old = e.attrs[c].sizing;
                    let new = a.sizing;
                    let column = e.attrs[p].sizing;
                    (old.height.declared_height_slot() && !new.height.declared_height_slot())
                        || (!s.shape.reads_group_height(old, column)
                            && s.shape.reads_group_height(new, column))
                })
            });
            if changed || self.get(id).is_some_and(|s| s.order.is_some()) {
                self.check_declared_group(e, id)?;
            }
        }
        for &i in &self.post {
            let s = &self.entries[i];
            let (shape, a) = self.declaration(e, s.id);
            if shape == Shape::Column
                && (!s.closed
                    || s.attrs.as_ref().is_some_and(|(a, _)| {
                        e.live(s.id).is_none_or(|k| {
                            let old = e.attrs[k].sizing;
                            old.width != a.sizing.width || old.height != a.sizing.height
                        })
                    }))
            {
                self.check_declared_group(e, s.id)?;
            }
            if shape == Shape::WrappedColumn {
                self.check_column_basis(e, s.id)?;
            }
            if let Some(p) = s.parent
                && !shape.has_intrinsic_height(a.sizing)
                && self.declaration(e, p).0 == Shape::WrappedColumn
                && self.needs_early_width(e, p)
            {
                return Err(early_heights());
            }
            // An attrs-only replacement can remove the boundary that made a
            // retained child's late width safe. Its order need not be resent.
            if s.order.is_none()
                && let Some(k) = e.live(s.id)
                && ((matches!(
                    shape,
                    Shape::El | Shape::Column | Shape::Row | Shape::WrappedColumn
                ) && matches!(e.attrs[k].sizing.width, Length::Px(_))
                    && !matches!(a.sizing.width, Length::Px(_)))
                    || (shape.cross_height_px(e.attrs[k].sizing).is_some()
                        && shape.cross_height_px(a.sizing).is_none())
                    || (shape == Shape::Column
                        && matches!(e.attrs[k].sizing.height, Length::Px(_))
                        && !matches!(a.sizing.height, Length::Px(_))))
            {
                for &c in e.children(k) {
                    if e.slots[c].shape == Shape::WrappedColumn {
                        self.check_column_basis(e, e.slots[c].id)?;
                    }
                }
            }
        }
        Ok(())
    }
}
fn early_heights() -> String {
    "WrappedColumn packed width is needed before child reflow; declare a pixel width or use leaves or pixel-height children".into()
}
