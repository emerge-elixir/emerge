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
                        || (shape == Shape::TextColumn
                            && !matches!(a.align[0], Some(Alignment::Start | Alignment::End)))
                        || (shape == Shape::Row && a.sizing.width.weight().is_some())))
            })
    }
    fn inline_text(&self, e: &Elements, id: u64) -> bool {
        let inline = |id| {
            let (_, a) = self.declaration(e, id);
            !matches!(a.align[0], Some(Alignment::Start | Alignment::End))
                && self
                    .site_parent(e, id)
                    .is_some_and(|p| self.declaration(e, p).0 == Shape::Paragraph)
        };
        inline(id)
            || self
                .site_parent(e, id)
                .is_some_and(|p| self.declaration(e, p).0 == Shape::El && inline(p))
    }
    // A closed El can move (or switch float side) without naming its Text.
    // No other unvisited descendant changes whether it supplies prose.
    pub(in crate::tree::wire) fn paragraph_words(&self, e: &Elements, old: usize) -> usize {
        let (removed, added) = self.entries.iter().fold((0, 0), |(removed, added), s| {
            let before = e.live(s.id).map_or(0, |k| match e.slots[k].shape {
                Shape::Text if e.inline_host(k).is_some() => e.attrs[k].words.max(1),
                Shape::El if e.inline_host(k).is_some() => e
                    .children(k)
                    .first()
                    .filter(|&&c| {
                        e.slots[c].shape == Shape::Text && self.get(e.slots[c].id).is_none()
                    })
                    .map_or(0, |c| e.attrs[*c].words.max(1)),
                _ => 0,
            });
            let next = if !s.attached {
                0
            } else {
                match s.shape {
                    Shape::Text if self.inline_text(e, s.id) => {
                        self.declaration(e, s.id).1.words.max(1)
                    }
                    Shape::El => self
                        .layout_children(e, s.id)
                        .filter(|c| {
                            self.get(*c).is_none()
                                && e.slots[e.ids[c]].shape == Shape::Text
                                && self.inline_text(e, *c)
                        })
                        .map(|c| e.attrs[e.ids[&c]].words.max(1))
                        .sum(),
                    _ => 0,
                }
            };
            (removed + before, added + next)
        });
        old - removed + added
    }
    fn check_text_site(&self, e: &Elements, id: u64) -> Result<()> {
        let (_, a) = self.declaration(e, id);
        if a.rotation.mixes_axes() && self.inline_text(e, id) {
            return Err("layout rotation requires a block box, not a paragraph span".into());
        }
        if !a.multiline || self.inline_text(e, id) {
            Ok(())
        } else {
            Err("multiline text is not in the first slice".into())
        }
    }
    // This proof uses final declarations only. A cross-axis offer is known
    // down an El/Row chain; a Column share is a separate, group-wide proof.
    fn independent_height(&self, e: &Elements, mut id: u64) -> bool {
        loop {
            let h = self.declaration(e, id).1.sizing.height;
            if matches!(h, Length::Px(_)) {
                return true;
            }
            if !h.independent_fill() {
                return false;
            }
            let Some(p) = self.site_parent(e, id) else {
                return true;
            };
            if !self.declaration(e, p).0.passes_cross_height() {
                return false;
            }
            id = p;
        }
    }
    fn check_height_projection(
        &self,
        e: &Elements,
        id: u64,
        shape: Shape,
        a: &Attrs,
    ) -> Result<()> {
        if !shape.projects_height(a) {
            return Ok(());
        }
        let Some(p) = self.site_parent(e, id) else {
            return Ok(());
        };
        let (host, parent) = self.declaration(e, p);
        // Neither Flow nor line membership reads a child's width here. Place
        // resolves the real height and also uses the projected width to align.
        if host.late_image_width(parent.sizing)
            || ((host.passes_cross_height() || host == Shape::Column)
                && self.independent_height(e, p))
        {
            Ok(())
        } else if a.rotation.mixes_axes() {
            Err("rotated width is needed before an independent height is assigned".into())
        } else {
            Err("automatic width from fill height is needed before an independent height is assigned".into())
        }
    }
    fn check_kept_projections(&self, e: &Elements, k: Key) -> Result<()> {
        let m = &e.slots[k];
        if !m.stats.own.height_reader {
            return Ok(());
        }
        self.check_height_projection(e, m.id, m.shape, &e.attrs[k])?;
        for &c in e.children(k) {
            self.check_kept_projections(e, c)?;
        }
        Ok(())
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
            shape.has_intrinsic_height(a)
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
        if shape != Shape::Column {
            return Ok(());
        }
        let inspect = |(reads, known), c| {
            let (kind, a) = self.declaration(e, c);
            (
                reads
                    || ((kind.projects_height(a) || matches!(column.sizing.height, Length::Px(_)))
                        && kind.reads_group_height(a, column.sizing)),
                known && kind.independent_height_slot(a),
            )
        };
        let facts = self.layout_children(e, id).fold((false, true), inspect);
        if facts == (true, false) {
            return Err("packed or automatic width is needed before its height is assigned; a Column share depends on reflow. Declare a pixel width or width-independent height slots".into());
        }
        Ok(())
    }
    pub(super) fn check_layout_sites(
        &self,
        e: &Elements,
        outside_parent: Option<Key>,
    ) -> Result<()> {
        if let Some(p) = outside_parent.filter(|&p| e.slots[p].shape == Shape::Column) {
            let id = e.slots[p].id;
            // A paint edit, or one declared height replacing another, cannot
            // make an already admitted group depend on width reflow.
            let changed = self.post.first().is_some_and(|&i| {
                let s = &self.entries[i];
                let Some((a, _)) = &s.attrs else { return false };
                e.live(s.id).is_none_or(|c| {
                    let old = &e.attrs[c];
                    let new = a;
                    let column = e.attrs[p].sizing;
                    (s.shape.independent_height_slot(old) && !s.shape.independent_height_slot(new))
                        || (!s.shape.reads_group_height(old, column)
                            && s.shape.reads_group_height(new, column))
                })
            });
            if changed || self.get(id).is_some_and(|s| s.order.is_some()) {
                self.check_declared_group(e, id)?;
            }
        }
        let mut changed_path = [false; MAX_DEPTH + 1];
        for &i in &self.post {
            let s = &self.entries[i];
            let (shape, a) = self.declaration(e, s.id);
            let inherited_change = s.depth > 0 && changed_path[s.depth - 1];
            // A different number, formula offset or raw block does not change
            // the proof. Only compare the capabilities a kept branch relies on.
            let basis = |b: BoxStyle| {
                (
                    matches!(b.width, Length::Px(_)),
                    matches!(b.height, Length::Px(_)),
                    b.height.independent_fill(),
                )
            };
            changed_path[s.depth] = s.stats.own.height_reader
                && (inherited_change
                    || e.live(s.id).is_none_or(|k| {
                        let old = e.attrs[k].sizing;
                        e.slots[k].parent.map(|p| e.slots[p].id) != s.parent
                            || basis(old) != basis(a.sizing)
                    }));
            if s.closed && changed_path[s.depth] && shape != Shape::Image {
                for &c in e.children(e.ids[&s.id]) {
                    self.check_kept_projections(e, c)?;
                }
            }
            if matches!(shape, Shape::Text | Shape::El) {
                self.check_text_site(e, s.id)?;
            }
            if shape == Shape::El {
                if a.has_art() && self.inline_text(e, s.id) {
                    return Err("file backgrounds require a block box, not a paragraph span".into());
                }
                // Moving a kept wrapper can turn its text back into a single line.
                for c in self.layout_children(e, s.id) {
                    self.check_text_site(e, c)?;
                }
            }
            if shape == Shape::Column
                && (!s.closed
                    || s.attrs.as_ref().is_some_and(|(a, _)| {
                        e.live(s.id).is_none_or(|k| {
                            let old = e.attrs[k].sizing;
                            basis(old) != basis(a.sizing)
                        })
                    }))
            {
                self.check_declared_group(e, s.id)?;
            }
            self.check_height_projection(e, s.id, shape, a)?;
            if shape == Shape::WrappedColumn {
                self.check_column_basis(e, s.id)?;
            }
            if let Some(p) = s.parent
                && !shape.has_intrinsic_height(a)
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
                    Shape::El
                        | Shape::Column
                        | Shape::Row
                        | Shape::WrappedColumn
                        | Shape::TextColumn
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
