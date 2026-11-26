//! Only dirty jobs inside a proved Row charge run early. Descendants use the
//! usual Flow and Reflow; neither the paragraph nor its peers run twice.
use super::*;

#[derive(Default)]
pub(super) struct RowSplits {
    active: bool,
    turn: u64,
    // Classify only paths reached by scheduled jobs, once per settle. Moves
    // and attrs-only edits can change the boundary without changing depth.
    before: SecondaryMap<Key, (u64, bool)>,
}
impl RowSplits {
    pub(super) fn reserve(&mut self, keys: usize) -> Result<(), String> {
        self.before
            .try_set_capacity(keys)
            .map_err(pool::capacity_error)
    }
    pub(super) fn phase(&mut self, e: &Elements, k: Key, p: Pass) -> Pass {
        if self.active && matches!(p, Flow | Reflow) && self.before_split(e, k) {
            if p == Flow {
                RowChargeFlow
            } else {
                RowChargeReflow
            }
        } else {
            p
        }
    }
    fn before_split(&mut self, e: &Elements, k: Key) -> bool {
        if let Some(&(turn, before)) = self.before.get(k)
            && turn == self.turn
        {
            return before;
        }
        let before =
            e.is_row_charge(k) || e.slots[k].parent.is_some_and(|p| self.before_split(e, p));
        self.before.insert(k, (self.turn, before));
        before
    }
}
impl Layers {
    pub(in crate::tree) fn begin_width_schedule(&mut self, e: &Elements, root: Option<Key>) {
        let splits = &mut self.row_splits;
        splits.active = root.is_some_and(|k| e.slots[k].stats.own.contains_row_split);
        if splits.active {
            splits.turn += 1;
        }
    }
}
