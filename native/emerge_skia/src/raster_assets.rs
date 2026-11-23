//! Logical PNG and JPEG images. Each loader owns its directory capability, decoded cache
//! and pixel budget. No process-global source names or lazy decode at replay.
use rustix::fs::{Mode, OFlags, openat};
use skia_safe::{
    AlphaType, Bitmap, Codec, ColorType, Data, EncodedOrigin, ISize, Image, ImageInfo, codec,
    images, surfaces,
};
use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    io::Read,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

mod format;
mod sized;
pub(crate) use sized::{Record, covers};
#[cfg(test)]
mod tests;
pub(crate) use format::Format;
const MIB: usize = 1024 * 1024;
const MAX_FILE: usize = 64 * MIB;
const MAX_BITMAP: usize = 64 * MIB;
const MAX_RESIDENT: usize = 256 * MIB;

fn bitmap_bytes(w: i32, h: i32) -> Option<usize> {
    if !(1..=32768).contains(&w) || !(1..=32768).contains(&h) {
        return None;
    }
    let bytes = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
    (bytes <= MAX_BITMAP).then_some(bytes)
}

#[derive(rustler::NifMap)]
pub(crate) struct Settings {
    pub sources: Vec<String>,
    pub decode_at_size: bool,
    pub cache_max_entries: usize,
    pub cache_max_bytes: usize,
}

#[derive(Debug)]
struct Charge {
    resident: Arc<AtomicUsize>,
    bytes: usize,
}
impl Charge {
    fn new(resident: &Arc<AtomicUsize>, bytes: usize) -> Self {
        resident.fetch_add(bytes, Ordering::Relaxed);
        Self {
            resident: resident.clone(),
            bytes,
        }
    }
}
impl Drop for Charge {
    fn drop(&mut self) {
        self.resident.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}
#[derive(Debug)]
struct Decoded {
    image: Image,
    charge: Charge,
}
#[derive(Clone, Debug)]
pub(crate) struct BitmapRef(Arc<Decoded>);
impl PartialEq for BitmapRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl BitmapRef {
    pub fn image(&self) -> &Image {
        &self.0.image
    }
    #[cfg(test)]
    pub fn snapshot(image: Image) -> Self {
        Self(Arc::new(Decoded {
            image,
            charge: Charge {
                bytes: 0,
                resident: Default::default(),
            },
        }))
    }
}
#[derive(Default, Clone, Debug, PartialEq)]
pub(crate) enum Source {
    #[default]
    Pending,
    Ready(BitmapRef),
    Sized(Record),
    Failed,
}
impl Source {
    pub fn dimensions(&self) -> Option<(i32, i32)> {
        match self {
            Self::Ready(b) => Some((b.image().width(), b.image().height())),
            Self::Sized(r) => Some((r.size().width, r.size().height)),
            _ => None,
        }
    }
}

pub(crate) enum Request {
    Open(String),
    Fit(Record, ISize),
}
struct Entry {
    name: Arc<str>,
    record: Option<Record>,
    bitmap: BitmapRef,
}
pub(crate) struct Loader {
    root: Option<File>,
    settings: Settings,
    cache: VecDeque<Entry>,
    records: HashMap<Arc<str>, std::sync::Weak<sized::Encoded>>,
    encoded_bytes: Arc<AtomicUsize>,
    clock: Arc<std::sync::atomic::AtomicU64>,
    #[cfg(test)]
    decodes: Vec<(ISize, ISize)>,
    cached_bytes: usize,
    resident: Arc<AtomicUsize>,
}
impl Loader {
    pub fn new(settings: Settings) -> Result<Self, String> {
        let [root] = settings.sources.as_slice() else {
            return Err("expected one application priv directory".into());
        };
        if !Path::new(root).is_absolute() {
            return Err("expected an absolute priv directory".into());
        }
        // Mix itself may symlink priv/. Descendants are opened relative to this
        // directory handle with NOFOLLOW, not checked and re-opened by path.
        let root = rustix::fs::open(
            root.as_str(),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .ok()
        .map(File::from);
        Ok(Self {
            root,
            settings,
            cache: Default::default(),
            records: Default::default(),
            encoded_bytes: Default::default(),
            clock: Default::default(),
            #[cfg(test)]
            decodes: Default::default(),
            cached_bytes: 0,
            resident: Default::default(),
        })
    }
    fn open(&self, name: &str) -> Option<(Format, File, usize)> {
        let format = Format::logical(name)?;
        let mut dir = self.root.as_ref()?.try_clone().ok()?;
        let mut components = name.split('/').peekable();
        while let Some(component) = components.next() {
            let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
            let flags = if components.peek().is_some() {
                flags | OFlags::DIRECTORY
            } else {
                flags
            };
            dir = openat(dir, component, flags, Mode::empty()).ok()?.into();
        }
        let meta = dir.metadata().ok()?;
        let len = usize::try_from(meta.len()).ok()?;
        if !meta.is_file() || !(1..=MAX_FILE).contains(&len) {
            return None;
        }
        Some((format, dir, len))
    }
    fn evict(&mut self) -> bool {
        let i = if self.settings.decode_at_size {
            self.cache
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.record.as_ref().map(Record::stamp))
                .map(|(i, _)| i)
                .unwrap_or(0)
        } else {
            0
        };
        self.remove(i).is_some()
    }
    fn remove(&mut self, i: usize) -> Option<Entry> {
        let entry = self.cache.remove(i)?;
        self.cached_bytes -= entry.bitmap.0.charge.bytes;
        Some(entry)
    }
    fn charge(&mut self, bytes: usize) -> Option<Charge> {
        let available = MAX_RESIDENT.checked_sub(bytes)?;
        while self.resident.load(Ordering::Relaxed) > available {
            if !self.evict() {
                return None;
            }
        }
        // This worker is the only incrementer. Installed and sealed drawings
        // can release their individual leases on other threads.
        Some(Charge::new(&self.resident, bytes))
    }
    fn decode(&mut self, name: &str) -> Option<BitmapRef> {
        let (format, file, len) = self.open(name)?;
        let codec = format.codec(Data::from_stream(file.take(len as u64), len)?)?;
        let display = sized::intrinsic(&codec, format);
        self.decode_for(codec, format, display)
    }
    pub fn fulfill(&mut self, req: Request, publish: impl FnOnce(Source) -> bool) -> bool {
        match req {
            Request::Open(name) => publish(self.load(name)),
            Request::Fit(r, size) => {
                let Some(b) = self.fit(&r, size) else {
                    return publish(Source::Failed);
                };
                if !publish(Source::Ready(b.clone())) {
                    return false;
                }
                // A superseded preparation must not grow the LRU. Retention
                // and eviction happen on the worker, after releasing the session.
                self.retain(r.name(), Some(r), &b);
                true
            }
        }
    }
    pub fn load(&mut self, name: String) -> Source {
        if self.settings.decode_at_size {
            return self.metadata(name).map_or(Source::Failed, Source::Sized);
        }
        if let Some(i) = self.cache.iter().position(|e| e.name.as_ref() == name) {
            let entry = self.cache.remove(i).unwrap();
            let image = entry.bitmap.clone();
            self.cache.push_back(entry);
            return Source::Ready(image);
        }
        let Some(image) = self.decode(&name) else {
            return Source::Failed;
        };
        self.retain(name.into(), None, &image);
        Source::Ready(image)
    }
    fn retain(&mut self, name: Arc<str>, record: Option<Record>, image: &BitmapRef) {
        let bytes = image.0.charge.bytes;
        let limit = self.settings.cache_max_bytes.min(MAX_RESIDENT);
        if self.settings.cache_max_entries == 0 || bytes > limit {
            return;
        }
        if let Some(i) = self.cache.iter().position(|e| e.name == name) {
            self.remove(i);
        }
        while self.cache.len() >= self.settings.cache_max_entries
            || self.cached_bytes > limit - bytes
        {
            self.evict();
        }
        self.cached_bytes += bytes;
        self.cache.push_back(Entry {
            name,
            record,
            bitmap: image.clone(),
        });
    }
}

// Drop the codec (including its workspace) before orienting or resampling.
fn decode_pixels(mut codec: Codec<'_>, size: ISize) -> Option<Image> {
    let info = ImageInfo::new(
        size,
        ColorType::RGBA8888,
        AlphaType::Premul,
        codec.info().color_space(),
    );
    let mut bitmap = Bitmap::new();
    let stride = info.min_row_bytes();
    if !bitmap.try_alloc_pixels_info(&info, stride) {
        return None;
    }
    let options = codec::Options {
        max_decode_memory: Some(MAX_BITMAP),
        ..Default::default()
    };
    let mut pixels = bitmap.peek_pixels()?;
    if codec.get_pixels_with_options(&info, pixels.bytes_mut()?, stride, Some(&options))
        != codec::Result::Success
    {
        return None;
    }
    bitmap.set_immutable();
    images::raster_from_bitmap(&bitmap)
}

// Transform already-decoded pixels once, without asking the codec for an
// incorrectly transposed decode size. This step is a texel permutation.
fn orient(image: Image, origin: EncodedOrigin) -> Option<Image> {
    let info = image.image_info();
    let info = if origin.swaps_width_height() {
        codec::pixmap_utils::swap_width_height(info)
    } else {
        info.clone()
    };
    let mut dst = surfaces::raster(&info, info.min_row_bytes(), None)?;
    let canvas = dst.canvas();
    canvas.concat(&origin.to_matrix(info.dimensions()));
    canvas.draw_image(&image, (0., 0.), None);
    // The surface owns its pixels and is dropped without any further writes.
    dst.make_temporary_image()
}
