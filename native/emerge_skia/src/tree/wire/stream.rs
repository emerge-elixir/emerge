//! Incremental live wire input. Keep attribute blocks and decoded child IDs,
//! not a concatenation of the transport. Even an unaligned BEAM sub-binary
//! is inspected only a bounded piece at a time after writer admission.
use super::*;
use crate::raster_assets::{Digest, Media, digest};
pub(crate) const CHUNK: usize = 4096;
#[derive(Default)]
struct Row {
    id: u64,
    tag: u8,
    attrs: Option<Vec<u8>>,
    order: Option<Vec<u64>>,
}
pub(crate) struct Rows {
    pub(super) document: storage::Allowance,
    media: storage::Allowance,
    _wire: storage::Grant,
    _rows: Option<storage::Grant>,
    header: Option<(u64, u64)>,
    records: Vec<Row>,
    pub(super) defs: media::Definitions,
}
enum Field {
    Header,
    Record,
    RawSize,
    Raw,
    Count,
    Child,
    Nearby,
    Done,
}
pub(crate) struct Stream {
    rows: Rows,
    size: usize,
    offset: usize,
    field: Field,
    small: [u8; 25],
    used: usize,
    left: usize,
    count: usize,
    order: bool,
}
impl Stream {
    pub fn bounded(size: usize, storage: &storage::Storage) -> Result<Self> {
        if size > MAX_BYTES {
            return Err("edit exceeds storage limit".into());
        }
        let mut document = storage.document.allowance();
        Ok(Self {
            rows: Rows {
                _wire: document.reserve(size + CHUNK)?,
                document,
                media: storage.media.allowance(),
                _rows: None,
                header: None,
                records: Vec::new(),
                defs: Default::default(),
            },
            size,
            offset: 0,
            field: if size == 0 {
                Field::Done
            } else {
                Field::Header
            },
            small: [0; 25],
            used: 0,
            left: 0,
            count: 0,
            order: false,
        })
    }
    fn row_end(&mut self) {
        self.field = if self.rows.records.len() == self.count {
            Field::Done
        } else {
            Field::Record
        };
    }
    fn after_raw(&mut self) {
        if self.order {
            self.field = Field::Count;
        } else {
            self.row_end();
        }
    }
    pub fn push(&mut self, offset: usize, mut data: &[u8]) -> Result<()> {
        if offset != self.offset
            || data.len() > CHUNK
            || data.is_empty()
            || data.len() > self.size - self.offset
        {
            return Err("invalid edit slice".into());
        }
        while !data.is_empty() {
            let need = match self.field {
                Field::Header => 25,
                Field::Record => 10,
                Field::RawSize => 4,
                Field::Count | Field::Nearby => 2,
                Field::Child => 8,
                Field::Done => return Err("trailing patch bytes".into()),
                Field::Raw => {
                    let n = self.left.min(data.len());
                    self.rows
                        .records
                        .last_mut()
                        .unwrap()
                        .attrs
                        .as_mut()
                        .unwrap()
                        .extend_from_slice(&data[..n]);
                    self.left -= n;
                    self.offset += n;
                    data = &data[n..];
                    if self.left == 0 {
                        self.after_raw();
                    }
                    continue;
                }
            };
            let n = (need - self.used).min(data.len());
            self.small[self.used..self.used + n].copy_from_slice(&data[..n]);
            self.used += n;
            self.offset += n;
            data = &data[n..];
            if self.used != need {
                continue;
            }
            self.used = 0;
            let mut r = Reader(&self.small[..need]);
            match self.field {
                Field::Header => {
                    if r.take(5)? != b"EMRG\x0a" {
                        return Err("expected EMRG version 10".into());
                    }
                    self.rows.header = Some((r.id()?, r.id()?));
                    self.count = r.long()?;
                    if self.count > 1_000_000 || self.count > (self.size - 25) / 10 {
                        return Err("invalid record count".into());
                    }
                    self.rows._rows = Some(
                        self.rows
                            .document
                            .reserve(self.count * (size_of::<Row>() + 32))?,
                    );
                    self.rows
                        .records
                        .try_reserve_exact(self.count)
                        .map_err(pool::capacity_error)?;
                    self.row_end();
                }
                Field::Record => {
                    let id = r.id()?;
                    let tag = r.byte()?;
                    let flags = r.byte()?;
                    if flags > 3 {
                        return Err("invalid record flags".into());
                    }
                    self.rows.records.push(Row {
                        id,
                        tag,
                        ..Default::default()
                    });
                    self.order = flags & 2 != 0;
                    if flags & 1 != 0 {
                        self.field = Field::RawSize;
                    } else {
                        self.after_raw();
                    }
                }
                Field::RawSize => {
                    let len = r.long()?;
                    if len > self.size - self.offset {
                        return Err("truncated EMRG data".into());
                    }
                    let mut raw = Vec::new();
                    raw.try_reserve_exact(len).map_err(pool::capacity_error)?;
                    self.rows.records.last_mut().unwrap().attrs = Some(raw);
                    self.left = len;
                    if len != 0 {
                        self.field = Field::Raw;
                    } else {
                        self.after_raw();
                    }
                }
                Field::Count => {
                    self.left = r.count()?;
                    if self.left > (self.size - self.offset) / 8 {
                        return Err("truncated EMRG data".into());
                    }
                    let mut ids = Vec::new();
                    ids.try_reserve_exact(self.left)
                        .map_err(pool::capacity_error)?;
                    self.rows.records.last_mut().unwrap().order = Some(ids);
                    self.field = if self.left == 0 {
                        Field::Nearby
                    } else {
                        Field::Child
                    };
                }
                Field::Child => {
                    self.rows
                        .records
                        .last_mut()
                        .unwrap()
                        .order
                        .as_mut()
                        .unwrap()
                        .push(r.id()?);
                    self.left -= 1;
                    if self.left == 0 {
                        self.field = Field::Nearby
                    };
                }
                Field::Nearby => {
                    if r.count()? != 0 {
                        return Err("Nearby is not in the first slice".into());
                    }
                    self.row_end();
                }
                _ => unreachable!(),
            }
        }
        Ok(())
    }
    pub fn finish(self) -> Result<Rows> {
        if self.offset == self.size && matches!(self.field, Field::Done) {
            Ok(self.rows)
        } else {
            Err("truncated EMRG data".into())
        }
    }
}
impl Rows {
    pub(super) fn raw(&self, r: &Range<usize>) -> &[u8] {
        self.records[r.start]
            .attrs
            .as_deref()
            .expect("staged attributes")
    }
    pub(super) fn take(&mut self, r: &Range<usize>) -> Box<[u8]> {
        self.records[r.start]
            .attrs
            .take()
            .unwrap()
            .into_boxed_slice()
    }
    pub(super) fn validate(
        &mut self,
        b: &mut batch::Batch,
        e: &Elements,
        prev: Option<Key>,
        budget: Budget,
    ) -> Result<()> {
        let Some((target, root)) = self.header else {
            b.rebase(prev.map(|k| e.slots[k].id));
            return Ok(());
        };
        let target = b.begin(e, prev, target, self.records.len(), budget)?;
        for (index, row) in self.records.iter_mut().enumerate() {
            let shape = Shape::from_tag(row.tag)?;
            let flags = u8::from(row.attrs.is_some()) + 2 * u8::from(row.order.is_some());
            let i = b.entry(e, row.id, shape, flags, budget)?;
            if let Some(raw) = &mut row.attrs {
                let mut a = attrs(raw, shape)?;
                if shape == Shape::Image {
                    let src = a.image_src.get(raw);
                    if src[0] == 3 {
                        let id: Digest = src[1..33].try_into().unwrap();
                        let data = &src[37..];
                        if digest(data) != id {
                            return Err("inline image digest mismatch".into());
                        }
                        let span = range(raw, src);
                        let payload = span.start + 37..span.end;
                        if let Some(existing) =
                            self.defs.get(&id).or_else(|| e.media.find(&id)).cloned()
                        {
                            if existing.bytes() != data {
                                return Err("conflicting inline image definitions".into());
                            }
                            self.defs.entry(id).or_insert(existing);
                            // Deduplicate without allocating a second large attribute block.
                            raw[span.start] = 4;
                            raw.copy_within(span.end.., span.start + 33);
                            raw.truncate(raw.len() - (span.len() - 33));
                        } else {
                            let lease = self.media.reserve(raw.capacity() + 256)?;
                            let mut compact = Vec::with_capacity(raw.len() - (span.len() - 33));
                            compact.extend_from_slice(&raw[..span.start]);
                            compact.push(4);
                            compact.extend(id);
                            compact.extend_from_slice(&raw[span.end..]);
                            let original = std::mem::replace(raw, compact);
                            self.defs.insert(id, Media::adopt(original, payload, lease));
                        }
                        a = attrs(raw, shape)?;
                    }
                }
                // The live batch names an owned record block. Detached batches
                // use the same proof with byte ranges in their caller's wire.
                b.entries[i].attrs = Some((a, index..index + 1));
            }
            if let Some(order) = &row.order {
                shape.check_children(order.len())?;
                b.order(i, order.len(), order.iter().copied(), budget)?;
            }
        }
        b.prove(e, prev, target, (root != 0).then_some(root), budget)
    }
}
