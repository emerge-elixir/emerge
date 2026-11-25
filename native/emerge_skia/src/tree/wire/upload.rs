use super::*;

impl Tree {
    pub(super) fn reserve_owners(&mut self, n: usize) -> Result<()> {
        let cap = self.budget.capacity(n);
        let e = &mut self.elements;
        if n > e.slots.capacity() {
            e.slots
                .try_reserve(cap - e.slots.len())
                .map_err(pool::capacity_error)?
        }
        if n > e.ids.capacity() {
            e.ids
                .try_reserve(cap - e.ids.len())
                .map_err(pool::capacity_error)?
        }
        let key_cap = e.slots.capacity();
        e.attrs
            .try_set_capacity(key_cap)
            .map_err(pool::capacity_error)?;
        let owners = if n > self.layers.owners {
            cap
        } else {
            self.layers.owners
        };
        pool::reserve(&mut e.touched, owners)?;
        pool::reserve(&mut e.detached, owners)?;
        pool::reserve(&mut e.free_indices, owners)?;
        self.layers.reserve(key_cap, owners)
    }
    pub fn upload(&mut self, data: &[u8]) -> Result<()> {
        self.patch(data)
    }
    pub(crate) fn upload_scaled(&mut self, data: &[u8], scale: f32) -> Result<()> {
        self.apply(&Staged::new(data, &self.input_budget)?, scale)
    }
}
