//! The source product is a single immutable picture in intrinsic coordinates.
//! Only the catalog is shared with the session; no catalog lock spans I/O,
//! parsing, recording or replay. The raster size is never a cache key.
use super::*;
use emerge_skia_painter::{SkiaSceneCache, SkiaScenePainter};
use skia_safe::{Picture, PictureRecorder, Rect};
use std::sync::{Mutex, OnceLock, Weak};
mod estimate;
mod policy;
#[cfg(test)]
mod tests;
const MAX_EFFECT_PIXELS: f64 = 16. * 1024. * 1024.;

#[derive(Debug)]
struct Recorded {
    picture: Picture,
    size: (f32, f32),
    effects: f64,
    lease: Charge,
}
#[derive(Clone, Debug)]
pub(crate) struct Art(Arc<Recorded>);
impl PartialEq for Art {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Art {
    pub fn size(&self) -> (f32, f32) {
        self.0.size
    }
    pub fn picture(&self) -> &Picture {
        &self.0.picture
    }
    // A picture shader needs a full intrinsic tile even when the box is tiny.
    pub fn fits_tile(&self, zoom: f32) -> bool {
        let (w, h) = self.size();
        let zoom = f64::from(zoom.max(1.));
        (f64::from(w) * zoom).ceil() * (f64::from(h) * zoom).ceil() <= MAX_EFFECT_PIXELS
    }
    pub fn fits(&self, sx: f32, sy: f32) -> bool {
        sx.is_finite()
            && sy.is_finite()
            && sx > 0.
            && sy > 0.
            && self.0.effects * f64::from(sx) * f64::from(sy) <= MAX_EFFECT_PIXELS
    }
}
struct Alias {
    art: Weak<Recorded>,
    _lease: Charge,
}
struct Catalog {
    names: HashMap<Arc<str>, Alias>,
    by_digest: HashMap<Digest, Alias>,
    idle: VecDeque<Art>,
    bytes: usize,
    painter_headroom: usize,
    max_entries: usize,
    max_bytes: usize,
    resident: Arc<AtomicUsize>,
    #[cfg(test)]
    records: usize,
    #[cfg(test)]
    parses: usize,
}
impl Catalog {
    fn evict(&mut self) -> bool {
        if let Some(a) = self.idle.pop_front() {
            self.bytes -= a.0.lease.bytes;
            true
        } else {
            false
        }
    }
    fn charge(&mut self, bytes: usize) -> Option<Charge> {
        let room = MAX_RESIDENT
            .checked_sub(self.painter_headroom)?
            .checked_sub(bytes)?;
        while self.resident.load(Ordering::Relaxed) > room {
            if !self.evict() {
                self.names.retain(|_, a| a.art.strong_count() != 0);
                self.by_digest.retain(|_, a| a.art.strong_count() != 0);
                if self.resident.load(Ordering::Relaxed) > room {
                    return None;
                }
            }
        }
        Some(Charge::new(&self.resident, bytes))
    }
    fn touch(&mut self, art: &Arc<Recorded>) {
        let bytes = art.lease.bytes;
        if self.max_entries == 0 || bytes > self.max_bytes {
            return;
        }
        if self.idle.back().is_some_and(|a| Arc::ptr_eq(art, &a.0)) {
            return;
        }
        if let Some(i) = self.idle.iter().rposition(|a| Arc::ptr_eq(art, &a.0)) {
            self.idle.remove(i);
        } else {
            while self.idle.len() >= self.max_entries || self.bytes > self.max_bytes - bytes {
                self.evict();
            }
            self.bytes += bytes;
        }
        self.idle.push_back(Art(art.clone()));
    }
    fn name(&mut self, key: AssetKey, art: &Arc<Recorded>) -> Option<()> {
        if let AssetKey::Name(name) = key {
            if self.names.len() == self.names.capacity() || self.names.len() >= 8192 {
                self.names.retain(|_, a| a.art.strong_count() != 0);
            }
            // Each alias is bounded to 4096 bytes. Do not let a single live
            // picture pin unbounded filenames in the weak index.
            if self.names.len() >= 8192 && !self.names.contains_key(&name) {
                return None;
            }
            let lease = self.charge(name.len() + 128)?;
            self.names.insert(
                name,
                Alias {
                    art: Arc::downgrade(art),
                    _lease: lease,
                },
            );
        }
        Some(())
    }
}
#[derive(Clone)]
pub(crate) struct Pictures(Arc<Mutex<Catalog>>);
impl Pictures {
    pub(super) fn new(s: &Settings) -> Self {
        Self(Arc::new(Mutex::new(Catalog {
            names: Default::default(),
            by_digest: Default::default(),
            idle: Default::default(),
            bytes: 0,
            painter_headroom: 0,
            max_entries: s.svg_tree_max_entries.min(8192),
            max_bytes: s.svg_tree_max_bytes.min(MAX_RESIDENT),
            resident: Default::default(),
            #[cfg(test)]
            records: 0,
            #[cfg(test)]
            parses: 0,
        })))
    }
    pub fn named(&self, name: &str) -> Option<Art> {
        let mut c = self.0.lock().ok()?;
        let a = c.names.get(name)?.art.upgrade()?;
        c.touch(&a);
        Some(Art(a))
    }
    pub fn known(&self, id: &Digest) -> Option<Art> {
        let mut c = self.0.lock().ok()?;
        let a = c.by_digest.get(id)?.art.upgrade()?;
        c.touch(&a);
        Some(Art(a))
    }
    pub(super) fn load(&self, key: AssetKey, bytes: &[u8]) -> Option<Art> {
        if bytes.len() > Format::Svg.limit() {
            return None;
        }
        let id = match &key {
            AssetKey::Content(id) => *id,
            AssetKey::Name(_) => digest(bytes),
        };
        if let Some(a) = self.known(&id) {
            self.0.lock().ok()?.name(key, &a.0)?;
            return Some(a);
        }
        // One worker per renderer. These separate leases cover the input and
        // XML document, expanded scene/path scratch, then installed pictures.
        let _xml = self.0.lock().ok()?.charge(4 * MIB + bytes.len() * 16)?;
        let xml = std::str::from_utf8(bytes).ok()?;
        policy::framing(xml)?;
        #[cfg(test)]
        {
            self.0.lock().ok()?.parses += 1;
        }
        let doc = usvg::roxmltree::Document::parse_with_options(
            xml,
            usvg::roxmltree::ParsingOptions {
                allow_dtd: false,
                nodes_limit: policy::NODES,
                ..Default::default()
            },
        )
        .ok()?;
        let work = policy::check(&doc)?;
        let _conversion = self.0.lock().ok()?.charge(work * 512)?;
        let tree = usvg::Tree::from_xmltree(&doc, &options()).ok()?;
        let size = (tree.size().width(), tree.size().height());
        if ![size.0, size.1].into_iter().all(|v| v > 0. && v <= 32768.) {
            return None;
        }
        let estimate = estimate::Estimate::new(&tree)?;
        let effects = estimate.area;
        let _expanded = self
            .0
            .lock()
            .ok()?
            .charge(estimate.work.saturating_sub(work) * 512)?;
        let work = work.max(estimate.work);
        {
            // The stock painter may retain previous/current pattern resources,
            // its last paint, and high-water scratch buffers between recordings.
            // Keep their allowance separate from live/idle picture ownership.
            let mut c = self.0.lock().ok()?;
            let peak = 3 * (work * 128 + bytes.len() + 256);
            if peak > c.painter_headroom {
                let extra = peak - c.painter_headroom;
                drop(c.charge(extra)?);
                c.painter_headroom = peak;
            }
        }
        #[cfg(test)]
        {
            self.0.lock().ok()?.records += 1;
        }
        let picture = record(&tree)?;
        // Approximate picture bytes omit resources referenced by recorded ops.
        // Charge a conservative source/work allowance as well, not pixel area.
        let bytes = picture
            .approximate_bytes_used()
            .checked_add(bytes.len() + work * 128 + 256)?;
        let mut c = self.0.lock().ok()?;
        let a = Art(Arc::new(Recorded {
            size,
            effects,
            picture,
            lease: c.charge(bytes)?,
        }));
        c.name(key, &a.0)?;
        if c.by_digest.len() == c.by_digest.capacity() {
            c.by_digest.retain(|_, a| a.art.strong_count() != 0);
        }
        let lease = c.charge(128)?;
        c.by_digest.insert(
            id,
            Alias {
                art: Arc::downgrade(&a.0),
                _lease: lease,
            },
        );
        c.touch(&a.0);
        Some(a)
    }
    #[cfg(test)]
    pub fn counts(&self) -> (usize, usize) {
        let c = self.0.lock().unwrap();
        (c.parses, c.records)
    }
}
fn options() -> usvg::Options<'static> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    let fonts = DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        for &font in &crate::tree::BUNDLED_FONTS {
            db.load_font_source(usvg::fontdb::Source::Binary(Arc::new(font)));
        }
        db.set_sans_serif_family("Inter");
        db.set_serif_family("Inter");
        db.set_monospace_family("JetBrains Mono NL");
        Arc::new(db)
    });
    usvg::Options {
        dpi: 96.,
        font_family: "Inter".into(),
        fontdb: fonts.clone(),
        max_depth: policy::DEPTH,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    }
}
fn record(tree: &usvg::Tree) -> Option<Picture> {
    // Live and one-shot workers each own one lazy painter cache. In particular
    // a CoreText cache never has to be Send. Only the finished picture travels.
    thread_local! {
        static CACHE: std::cell::RefCell<SkiaSceneCache> = std::cell::RefCell::new(SkiaSceneCache::new());
    }
    let mut recorder = PictureRecorder::new();
    let canvas = recorder.begin_recording(
        Rect::from_wh(tree.size().width(), tree.size().height()),
        false,
    );
    let mut unsupported = false;
    CACHE.with_borrow_mut(|cache| {
        cache.next_gen();
        let mut painter = SkiaScenePainter::new(canvas, cache);
        anyrender_svg::render_svg_tree_with(&mut painter, tree, Default::default(), &mut |_, _| {
            unsupported = true
        });
    });
    if unsupported {
        return None;
    }
    recorder.finish_recording_as_picture(None)
}
