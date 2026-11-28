//! Constant-size gauges, not an inventory of files or a new cache. A reporter
//! does not pin any picture, encoded source or pixel variant.
use super::*;
use std::{fmt, sync::Mutex};

pub(super) type CacheGauge = Arc<Mutex<(usize, usize)>>;
pub(crate) struct Usage {
    encoded: Arc<AtomicUsize>,
    pixels: Arc<AtomicUsize>,
    cached: CacheGauge,
    limits: (usize, usize),
    pictures: svg::PictureGauge,
}
impl Loader {
    pub(super) fn note_cache_size(&self) {
        if let Some(g) = &self.usage {
            *g.lock().unwrap_or_else(|p| p.into_inner()) = (self.cache.len(), self.cached_bytes);
        }
    }
    pub(crate) fn usage(&mut self) -> Usage {
        let cached = self
            .usage
            .get_or_insert_with(|| Arc::new(Mutex::new((self.cache.len(), self.cached_bytes))))
            .clone();
        Usage {
            encoded: self.encoded_bytes.clone(),
            pixels: self.resident.clone(),
            cached,
            limits: (
                self.settings.cache_max_entries,
                self.settings.cache_max_bytes.min(MAX_RESIDENT),
            ),
            pictures: self.pictures.gauge(),
        }
    }
}
impl fmt::Display for Usage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (entries, bytes) = *self.cached.lock().unwrap_or_else(|p| p.into_inner());
        let (max_entries, max_bytes) = self.limits;
        write!(
            f,
            "sized raster sources: encoded_bytes={}\n    raster cache: entries={entries} bytes={bytes} limits=entries:{max_entries} bytes:{max_bytes}\n    live charged raster bytes={} (includes cached pixels)\n    SVG pictures: ",
            self.encoded.load(Ordering::Relaxed),
            self.pixels.load(Ordering::Relaxed)
        )?;
        if let Some((n, b, max_n, max_b, live)) = self.pictures.read() {
            write!(
                f,
                "cached_entries={n} estimated_bytes={b} limits=entries:{max_n} bytes:{max_b} live_charged_bytes={live}"
            )
        } else {
            f.write_str("catalog unavailable")
        }
    }
}
