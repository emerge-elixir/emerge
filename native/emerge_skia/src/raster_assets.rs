//! Renderer-local source preparation. SVGs are immutable command pictures;
//! raster formats use the fitted-pixel pipeline; SVG retains a picture.
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

mod files;
mod format;
mod input;
mod sized;
mod svg;
mod usage;
pub(crate) use files::{valid_absolute, valid_logical};
use input::AssetKey;
pub(crate) use input::{Digest, Input, InputBudget, Media, digest};
pub(crate) use sized::{Record, covers};
pub(crate) use svg::Pictures;
pub(crate) use usage::Usage;
#[cfg(test)]
mod tests;
use format::Format;
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

#[derive(rustler::NifMap, Default)]
pub(crate) struct Settings {
    pub sources: Vec<String>,
    pub runtime_enabled: bool,
    pub allowlist: Vec<String>,
    pub follow_symlinks: bool,
    pub max_file_size: usize,
    pub extensions: Vec<String>,
    pub svg_tree_max_entries: usize,
    pub svg_tree_max_bytes: usize,
    pub decode_at_size: bool,
    pub cache_max_entries: usize,
    pub cache_max_bytes: usize,
}

#[derive(Debug)]
pub(crate) struct Charge {
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
    Picture(svg::Art),
    Failed,
}
impl Source {
    pub fn dimensions(&self) -> Option<(f32, f32)> {
        match self {
            Self::Ready(b) => Some((b.image().width() as f32, b.image().height() as f32)),
            Self::Sized(r) => Some((r.size().width as f32, r.size().height as f32)),
            Self::Picture(art) => Some(art.size()),
            _ => None,
        }
    }
}

pub(crate) enum Request {
    Open(Input, bool),
    Fit(Record, ISize),
}
struct Entry {
    name: AssetKey,
    record: Option<Record>,
    bitmap: BitmapRef,
}
pub(crate) struct Loader {
    files: files::Files,
    pub pictures: Pictures,
    settings: Settings,
    cache: VecDeque<Entry>,
    records: HashMap<AssetKey, std::sync::Weak<sized::Encoded>>,
    encoded_bytes: Arc<AtomicUsize>,
    clock: Arc<std::sync::atomic::AtomicU64>,
    #[cfg(test)]
    decodes: Vec<(ISize, ISize)>,
    cached_bytes: usize,
    // Only maintained for opted-in periodic reports; never retains an asset.
    usage: Option<usage::CacheGauge>,
    resident: Arc<AtomicUsize>,
}
impl Loader {
    pub fn new(settings: Settings) -> Result<Self, String> {
        Ok(Self {
            files: files::Files::new(&settings)?,
            pictures: Pictures::new(&settings),
            settings,
            cache: Default::default(),
            records: Default::default(),
            encoded_bytes: Default::default(),
            clock: Default::default(),
            #[cfg(test)]
            decodes: Default::default(),
            cached_bytes: 0,
            usage: None,
            resident: Default::default(),
        })
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
        self.note_cache_size();
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
    pub fn fulfill(&mut self, req: Request, publish: impl FnOnce(Source) -> bool) -> bool {
        match req {
            Request::Open(input, svg_only) => {
                publish(self.open_input(input, svg_only).unwrap_or(Source::Failed))
            }
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
    #[cfg(test)]
    pub fn load(&mut self, name: String) -> Source {
        self.open_input(Input::Logical(name), false)
            .unwrap_or(Source::Failed)
    }
    fn open_input(&mut self, input: Input, svg_only: bool) -> Option<Source> {
        let (format, key, data) = match input {
            Input::Logical(name) => {
                let format = Format::logical(&name)?;
                if svg_only && format != Format::Svg {
                    return None;
                }
                if format == Format::Svg
                    && let Some(art) = self.pictures.named(&name)
                {
                    return Some(Source::Picture(art));
                }
                let name: Arc<str> = name.into();
                let key = AssetKey::Name(name.clone());
                if format != Format::Svg
                    && let Some(hit) = self.cached(&key)
                {
                    return Some(hit);
                }
                let (_, data) = self.files.read(&name, false, &self.settings)?;
                (format, key, data)
            }
            Input::Runtime(name) => {
                let (format, data) = self.files.read(&name, true, &self.settings)?;
                (format, AssetKey::Content(digest(&data)), data)
            }
            Input::Bytes(id, bytes) => {
                let format = Format::inline(bytes.bytes());
                if bytes.bytes().len() > format.limit() {
                    return None;
                }
                // SkData retains the already-owned input, not a BEAM term.
                struct Owner(Arc<Media>);
                impl AsRef<[u8]> for Owner {
                    fn as_ref(&self) -> &[u8] {
                        self.0.bytes()
                    }
                }
                (
                    format,
                    AssetKey::Content(id),
                    Data::new_with_owner(Owner(bytes)),
                )
            }
        };
        if format == Format::Svg {
            return self.pictures.load(key, &data).map(Source::Picture);
        }
        if svg_only {
            return None;
        }
        if matches!(key, AssetKey::Content(_))
            && let Some(hit) = self.cached(&key)
        {
            return Some(hit);
        }
        if self.settings.decode_at_size {
            return self.metadata(key, format, data).map(Source::Sized);
        }
        let codec = format.codec(data)?;
        let size = sized::intrinsic(&codec, format);
        let image = self.decode_for(codec, format, size)?;
        self.retain(key, None, &image);
        Some(Source::Ready(image))
    }
    fn cached(&mut self, name: &AssetKey) -> Option<Source> {
        if self.settings.decode_at_size {
            return self
                .records
                .get(name)?
                .upgrade()
                .map(|r| Source::Sized(Record(r)));
        }
        if let Some(i) = self.cache.iter().position(|e| &e.name == name) {
            let entry = self.cache.remove(i).unwrap();
            let image = entry.bitmap.clone();
            self.cache.push_back(entry);
            return Some(Source::Ready(image));
        }
        None
    }
    fn retain(&mut self, name: AssetKey, record: Option<Record>, image: &BitmapRef) {
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
        self.note_cache_size();
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
