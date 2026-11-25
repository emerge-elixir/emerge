//! Split EMRG image definitions from the small, retained attribute blocks.
//! The prepass has no dependency on mutable session state. Non-media edits
//! borrow the original buffer; only definitions require copying or hashing.
use super::*;
use crate::raster_assets::{Digest, InputBudget, Media, digest};
use std::{borrow::Cow, collections::HashSet, sync::Arc};
use storage::Grant as Charge;

pub(super) type Definitions = HashMap<Digest, Arc<Media>>;
pub(crate) struct Staged<'a> {
    pub(super) bytes: Cow<'a, [u8]>,
    definitions: Definitions,
    _scratch: Option<Charge>,
}

// Non-definition edits borrow their wire. For mixed edits charge only the
// compact buffer as it grows, including the old allocation during relocation.
#[derive(Default)]
struct Compact {
    bytes: Vec<u8>,
    lease: Option<Charge>,
}
impl Compact {
    fn append(&mut self, parts: &[&[u8]], budget: &InputBudget, limit: usize) -> Result<()> {
        let need = self.bytes.len() + parts.iter().map(|s| s.len()).sum::<usize>();
        if need > self.bytes.capacity() {
            let cap = need.max(2 * self.bytes.capacity()).min(limit);
            let charge = budget.reserve(cap)?;
            self.bytes
                .try_reserve_exact(cap - self.bytes.len())
                .map_err(pool::capacity_error)?;
            self.lease = Some(charge);
        }
        for part in parts {
            self.bytes.extend_from_slice(part);
        }
        Ok(())
    }
}
impl<'a> Staged<'a> {
    pub fn new(bytes: &'a [u8], budget: &InputBudget) -> Result<Self> {
        let mut definitions = Definitions::new();
        let mut compact = Compact::default();
        let mut copied = 0;
        if bytes.len() > MAX_BYTES {
            return Err("edit exceeds storage limit".into());
        }
        if !bytes.is_empty() {
            let mut r = Reader(bytes);
            if r.take(5)? != b"EMRG\x0a" {
                return Err("expected EMRG version 10".into());
            }
            r.take(16)?;
            let count = r.long()?;
            if count > 1_000_000 || count > r.0.len() / 10 {
                return Err("invalid record count".into());
            }
            for _ in 0..count {
                r.take(8)?;
                let kind = r.byte()?;
                let flags = r.byte()?;
                if flags > 3 {
                    return Err("invalid record flags".into());
                }
                if flags & 1 != 0 {
                    let raw = r.block()?;
                    if kind == Shape::Image as u8 {
                        let a = attrs(raw, Shape::Image)?;
                        let src = a.image_src.get(raw);
                        if src[0] == 3 {
                            let id: Digest = src[1..33].try_into().unwrap();
                            let payload = &src[37..];
                            if digest(payload) != id {
                                return Err("inline image digest mismatch".into());
                            }
                            match definitions.entry(id) {
                                std::collections::hash_map::Entry::Occupied(e)
                                    if e.get().bytes() != payload =>
                                {
                                    return Err("conflicting inline image definitions".into());
                                }
                                std::collections::hash_map::Entry::Occupied(_) => (),
                                std::collections::hash_map::Entry::Vacant(e) => {
                                    e.insert(budget.copy(payload)?);
                                }
                            }
                            let at = range(bytes, raw).start;
                            let span = range(raw, src);
                            let size = ((raw.len() - 4 - payload.len()) as u32).to_be_bytes();
                            compact.append(
                                &[&bytes[copied..at - 4], &size, &raw[..span.start], &[4], &id],
                                budget,
                                bytes.len(),
                            )?;
                            copied = at + span.end;
                        }
                    }
                }
                if flags & 2 != 0 {
                    r.children()?;
                    let n = r.count()?;
                    r.take(n * 9)?;
                }
            }
            if !r.0.is_empty() {
                return Err("trailing patch bytes".into());
            }
        }
        let bytes = if copied == 0 {
            Cow::Borrowed(bytes)
        } else {
            compact.append(&[&bytes[copied..]], budget, bytes.len())?;
            Cow::Owned(compact.bytes)
        };
        Ok(Self {
            bytes,
            definitions,
            _scratch: compact.lease,
        })
    }
}

#[derive(Default)]
pub(in crate::tree) struct Declarations {
    live: HashMap<Digest, (usize, Arc<Media>)>,
    bytes: usize,
    extent: usize,
}
#[derive(Default)]
pub(super) struct Changes {
    counts: HashMap<Digest, isize>,
    bytes: usize,
    pub(super) extent: usize,
}
fn key<'a>(attrs: &Attrs, raw: &'a [u8]) -> Option<&'a Digest> {
    attrs.image_src.get(raw).strip_prefix(&[4])?.try_into().ok()
}
impl Staged<'_> {
    pub(super) fn changes(&self, e: &Elements, b: &batch::Batch) -> Result<Changes> {
        Changes::for_batch(e, b, |r| &self.bytes[r.clone()], &self.definitions)
    }
    pub(super) fn commit(&self, changes: Changes, e: &mut Elements) {
        changes.install(&self.definitions, e);
    }
}
impl Changes {
    pub(super) fn for_batch<'a>(
        e: &Elements,
        b: &batch::Batch,
        raw: impl Fn(&Range<usize>) -> &'a [u8],
        defs: &Definitions,
    ) -> Result<Self> {
        let mut counts = HashMap::new();
        for s in b
            .entries
            .iter()
            .filter(|s| s.shape == Shape::Image && (!s.attached || s.attrs.is_some() || s.inserted))
        {
            let k = e.live(s.id);
            let old = k.and_then(|k| key(&e.attrs[k], e.raw(k)));
            let new = if s.attached {
                match &s.attrs {
                    Some((a, r)) => key(a, raw(r)),
                    None => old,
                }
            } else {
                None
            };
            if old != new {
                if let Some(id) = old {
                    *counts.entry(*id).or_default() -= 1;
                }
                if let Some(id) = new {
                    *counts.entry(*id).or_default() += 1;
                }
            }
        }
        let (mut bytes, mut extent) = (e.media.bytes, e.media.extent);
        for (id, &delta) in &counts {
            if let Some((n, data)) = e.media.live.get(id) {
                if n.checked_add_signed(delta).ok_or("invalid source count")? == 0 {
                    bytes -= data.bytes().len() + 4;
                    extent -= data.extent();
                }
            } else {
                let data = defs.get(id).ok_or("undefined inline image reference")?;
                extent = extent
                    .checked_add(data.extent())
                    .ok_or("inline source limit")?;
                bytes = bytes
                    .checked_add(data.bytes().len() + 4)
                    .ok_or("inline source limit")?;
            }
        }
        Ok(Changes {
            counts,
            bytes,
            extent,
        })
    }
    pub(super) fn install(self, defs: &Definitions, e: &mut Elements) {
        for (id, delta) in self.counts {
            if let Some((n, _)) = e.media.live.get_mut(&id) {
                *n = n.checked_add_signed(delta).unwrap();
                if *n == 0 {
                    e.media.live.remove(&id);
                }
            } else {
                e.media.live.insert(id, (delta as usize, defs[&id].clone()));
            }
        }
        e.media.bytes = self.bytes;
        e.media.extent = self.extent;
    }
}
impl Changes {
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}
impl Declarations {
    pub(super) fn find(&self, id: &Digest) -> Option<&Arc<Media>> {
        self.live.get(id).map(|d| &d.1)
    }
    pub(in crate::tree) fn get(&self, id: &Digest) -> &Arc<Media> {
        &self.live[id].1
    }
    pub(super) fn write(
        &self,
        attrs: &Attrs,
        raw: &[u8],
        seen: &mut HashSet<Digest>,
        out: &mut Vec<u8>,
    ) {
        if let Some(id) = key(attrs, raw)
            && seen.insert(*id)
        {
            let payload = self.get(id).bytes();
            let at = attrs.image_src.start as usize;
            out.extend(((raw.len() + 4 + payload.len()) as u32).to_be_bytes());
            out.extend_from_slice(&raw[..at]);
            out.push(3);
            out.extend_from_slice(id);
            out.extend((payload.len() as u32).to_be_bytes());
            out.extend_from_slice(payload);
            out.extend_from_slice(&raw[at + 33..]);
        } else {
            out.extend((raw.len() as u32).to_be_bytes());
            out.extend_from_slice(raw);
        }
    }
}
