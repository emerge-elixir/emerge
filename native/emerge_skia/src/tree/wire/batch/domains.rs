//! Stage only changed resource domains. Named records are already in parent-first
//! order. A closed branch is inspected only when moving or rescaling it really
//! changes its inputs. Nothing here enqueues a calculation or changes a mount.
use super::*;
use scale::Inputs;
use text::{StrikeDelta, StrikeKey};

#[derive(Default)]
pub(super) struct Domains {
    stack: Vec<(u64, Inputs)>,
    changed: Vec<(u64, Inputs)>,
    deltas: Vec<StrikeDelta>,
    added: Vec<StrikeKey>,
}
impl Domains {
    pub(super) fn reserve(&mut self, n: usize) -> Result<()> {
        pool::reserve(&mut self.stack, n)?;
        pool::reserve(&mut self.changed, n)?;
        pool::reserve(&mut self.deltas, 2 * n)?;
        pool::reserve(&mut self.added, n)
    }
    pub(super) fn clear(&mut self) {
        self.stack.clear();
        self.changed.clear();
        self.deltas.clear();
        self.added.clear();
    }
}
impl Batch {
    pub(in crate::tree::wire) fn rebase(&mut self, root: Option<u64>) {
        self.reset();
        self.root = root;
    }
    pub(in crate::tree::wire) fn prepare_domains(
        &mut self,
        e: &Elements,
        strikes: &mut std::sync::Arc<text::Strikes>,
        display: f32,
        declarations: usize,
    ) -> Result<()> {
        let base = Inputs::display(display);
        if let Some(root) = self.root {
            let attrs = self
                .get(root)
                .and_then(|s| s.attrs.as_ref().map(|(a, _)| a))
                .unwrap_or_else(|| &e.attrs[e.ids[&root]]);
            let nested = declarations > usize::from(attrs.layout_scale.is_some());
            self.walk_domains(e, root, base, nested)?;
            for j in 0..self.post.len() {
                let edit = &self.entries[self.post[j]];
                if edit.inputs.is_none() {
                    let parent = edit.parent.map_or(base, |id| {
                        self.get(id)
                            .and_then(|s| s.inputs)
                            .unwrap_or_else(|| e.slots[e.ids[&id]].admitted)
                    });
                    self.walk_domains(e, edit.id, parent, nested)?;
                }
            }
        }
        for s in self
            .entries
            .iter()
            .filter(|s| !s.attached && s.shape.uses_glyphs())
        {
            let old = e.slots[e.ids[&s.id]].admitted;
            self.domains.deltas.push((Some(old.into()), None));
        }
        if self.domains.deltas.is_empty() {
            return Ok(());
        }
        std::sync::Arc::make_mut(strikes).prepare(&self.domains.deltas, &mut self.domains.added)
    }
    fn walk_domains(
        &mut self,
        e: &Elements,
        root: u64,
        inherited: Inputs,
        nested: bool,
    ) -> Result<()> {
        self.domains.stack.push((root, inherited));
        while let Some((id, parent)) = self.domains.stack.pop() {
            let old = e.live(id);
            let old_inputs = old.map(|k| e.slots[k].admitted);
            let i = self.position(id);
            let s = i.map(|i| &self.entries[i]);
            let a = s
                .and_then(|s| s.attrs.as_ref().map(|(a, _)| a))
                .unwrap_or_else(|| &e.attrs[e.ids[&id]]);
            let shape = s.map_or_else(|| e.slots[old.unwrap()].shape, |s| s.shape);
            let next = parent.child(a, shape, nested)?;
            let order = s.and_then(|s| s.order);
            if let Some(i) = i {
                self.entries[i].inputs = Some(next);
            }
            if old_inputs == Some(next) {
                continue;
            }
            self.domains.changed.push((id, next));
            if shape.uses_glyphs() {
                let delta = (old_inputs.map(StrikeKey::from), Some(next.into()));
                if delta.0 != delta.1 {
                    self.domains.deltas.push(delta);
                }
            }
            if let Some(order) = order {
                self.domains
                    .stack
                    .extend(self.orders.get(order).keys.iter().map(|&id| (id, next)));
            } else {
                self.domains
                    .stack
                    .extend(e.owned(old.unwrap()).iter().map(|&k| (e.slots[k].id, next)));
            }
        }
        Ok(())
    }
    pub(in crate::tree::wire) fn changed_inputs(&self) -> impl Iterator<Item = (u64, Inputs)> + '_ {
        self.domains.changed.iter().copied()
    }
    pub(in crate::tree::wire) fn commit_strikes(
        &self,
        strikes: &mut std::sync::Arc<text::Strikes>,
    ) {
        if !self.domains.deltas.is_empty() {
            std::sync::Arc::make_mut(strikes).commit(&self.domains.deltas);
        }
    }
    pub(in crate::tree::wire) fn publish_domains(&self, e: &mut Elements, l: &mut Layers) {
        self.commit_strikes(&mut l.text.strikes);
        for &(id, next) in &self.domains.changed {
            e.slots[e.ids[&id]].admitted = next;
        }
    }
}
