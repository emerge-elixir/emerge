//! A shared compressed source is not a decoded bitmap. Only the loader owns
//! the LRU; a record's weak link also allows reuse of pixels pinned by a frame.
use super::*;
use std::sync::{Mutex, Weak, atomic::AtomicU64};

#[derive(Debug)]
pub(super) struct Encoded {
    name: AssetKey,
    format: Format,
    data: Data,
    display: ISize,
    _lease: Charge,
    latest: Mutex<Weak<Decoded>>,
    clock: Arc<AtomicU64>,
    stamp: AtomicU64,
}
#[derive(Debug, Clone)]
pub(crate) struct Record(pub(super) Arc<Encoded>);
impl PartialEq for Record {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Record {
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
    pub fn size(&self) -> ISize {
        self.0.display
    }
    pub(super) fn name(&self) -> AssetKey {
        self.0.name.clone()
    }
    pub fn publish(&self, bitmap: &BitmapRef) {
        if let Ok(mut latest) = self.0.latest.lock() {
            *latest = Arc::downgrade(&bitmap.0);
        }
        self.touch();
    }
    fn touch(&self) {
        self.0.stamp.fetch_max(
            self.0.clock.fetch_add(1, Ordering::Relaxed),
            Ordering::Relaxed,
        );
    }
    pub(super) fn stamp(&self) -> u64 {
        self.0.stamp.load(Ordering::Relaxed)
    }
    pub fn current(&self, target: ISize) -> Option<BitmapRef> {
        let b = BitmapRef(self.0.latest.lock().ok()?.upgrade()?);
        covers(b.image().dimensions(), target).then(|| {
            self.touch();
            b
        })
    }
}
pub(crate) fn covers(size: ISize, target: ISize) -> bool {
    size.width >= target.width && size.height >= target.height
}
fn swapped(size: ISize) -> ISize {
    (size.height, size.width).into()
}
fn display_size(size: ISize, origin: EncodedOrigin) -> ISize {
    if origin.swaps_width_height() {
        swapped(size)
    } else {
        size
    }
}
pub(super) fn intrinsic(codec: &Codec, format: Format) -> ISize {
    display_size(codec.dimensions(), origin(codec, format))
}
fn origin(codec: &Codec, format: Format) -> EncodedOrigin {
    if matches!(format, Format::Jpeg | Format::Webp) {
        codec.origin()
    } else {
        EncodedOrigin::TopLeft
    }
}

impl Loader {
    pub(super) fn metadata(
        &mut self,
        name: AssetKey,
        format: Format,
        data: Data,
    ) -> Option<Record> {
        let len = data.len();
        let room = MAX_RESIDENT.checked_sub(len)?;
        while self.encoded_bytes.load(Ordering::Relaxed) > room {
            if !self.evict() {
                return None;
            }
        }
        let lease = Charge::new(&self.encoded_bytes, len);
        let codec = format.codec(data.clone())?;
        let display = intrinsic(&codec, format);
        // Same source acceptance limits in both modes, even for tiny draws.
        bitmap_bytes(display.width, display.height)?;
        let record = Record(Arc::new(Encoded {
            name: name.clone(),
            format,
            display,
            _lease: lease,
            data,
            latest: Default::default(),
            clock: self.clock.clone(),
            stamp: Default::default(),
        }));
        // Amortized cleanup; the index must not retain every name ever mounted.
        if self.records.len() == self.records.capacity() {
            self.records.retain(|_, r| r.strong_count() != 0);
        }
        self.records.insert(name, Arc::downgrade(&record.0));
        Some(record)
    }
    pub(super) fn fit(&mut self, record: &Record, target: ISize) -> Option<BitmapRef> {
        if let Some(b) = record.current(target) {
            return Some(b);
        }
        // An oversized live draw may have expired without displacing a
        // smaller eligible LRU entry. It can still satisfy the next request.
        if let Some(entry) = self.cache.iter().find(|e| {
            e.record.as_ref() == Some(record) && covers(e.bitmap.image().dimensions(), target)
        }) {
            return Some(entry.bitmap.clone());
        }
        let r = &record.0;
        self.decode_for(r.format.reopen(r.data.clone())?, r.format, target)
    }
    pub(super) fn decode_for(
        &mut self,
        codec: Codec,
        format: Format,
        target: ISize,
    ) -> Option<BitmapRef> {
        let origin = origin(&codec, format);
        let raw_target = display_size(target, origin);
        if !covers(codec.dimensions(), raw_target) {
            return None;
        }
        let stage = codec_size(&codec, raw_target);
        let bytes = bitmap_bytes(target.width, target.height)?;
        let staging = bitmap_bytes(stage.width, stage.height)?;
        let turn = origin != EncodedOrigin::TopLeft;
        let resize = stage != raw_target;
        // Raw + oriented staging, then oriented staging + final. Never retain
        // the codec-size image as the result of a smaller exact-size request.
        let peak =
            (staging * if turn { 2 } else { 1 }).max(staging + if resize { bytes } else { 0 });
        let charge = self.charge(bytes)?;
        let _scratch = if peak > bytes {
            Some(self.charge(peak - bytes)?)
        } else {
            None
        };
        #[cfg(test)]
        {
            self.decodes.push((stage, target));
        }
        let raw = decode_pixels(codec, stage)?;
        let image = if turn { orient(raw, origin)? } else { raw };
        let image = if resize {
            image.make_scaled(
                &image.image_info().with_dimensions(target),
                skia_safe::SamplingOptions::new(
                    skia_safe::FilterMode::Linear,
                    skia_safe::MipmapMode::None,
                ),
            )?
        } else {
            image
        };
        Some(BitmapRef(Arc::new(Decoded { image, charge })))
    }
}
// Skia rounds to a supported scale (e.g. JPEG 1/8). Round *up* when its
// initial suggestion would require enlargement on either axis, as on main.
fn codec_size(codec: &Codec, target: ISize) -> ISize {
    let full = codec.dimensions();
    if target == full {
        return full;
    }
    let mut lo =
        (target.width as f32 / full.width as f32).max(target.height as f32 / full.height as f32);
    let initial = codec.get_scaled_dimensions(lo);
    if covers(initial, target) {
        return initial;
    }
    let (mut hi, mut best) = (1., full);
    for _ in 0..16 {
        let mid = (lo + hi) * 0.5;
        let size = codec.get_scaled_dimensions(mid);
        if covers(size, target) {
            best = size;
            hi = mid;
        } else {
            lo = mid;
        }
    }
    best
}
