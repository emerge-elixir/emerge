//! Incoming prose edges, including moves of closed El → Text branches. Size
//! feedback is checked when folding the final group (Subtree::with_layout),
//! including resident ancestors outside an attrs-only patch target.
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
    pub(super) fn check_layout_sites(&self, e: &Elements) -> Result<()> {
        for &i in &self.post {
            let s = &self.entries[i];
            if matches!(s.shape, Shape::Text | Shape::El) {
                self.check_text_site(e, s.id)?;
            }
            if s.shape == Shape::El {
                if self.declaration(e, s.id).1.has_art() && self.inline_text(e, s.id) {
                    return Err("file backgrounds require a block box, not a paragraph span".into());
                }
                for c in self.layout_children(e, s.id) {
                    self.check_text_site(e, c)?;
                }
            }
        }
        Ok(())
    }
}
