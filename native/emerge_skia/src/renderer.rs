//! Backend-agnostic Skia renderer.
//!
//! This module contains:
//! - `RenderScene` / `RenderNode` scene graph types
//! - `RenderState` for holding scene data between frames
//! - `SceneRenderer` that executes scene nodes on backend-provided Skia surfaces
//! - Font cache for text rendering

use std::collections::{HashMap, HashSet};
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use resvg::usvg;
use skia_safe::{
    BlendMode, BlurStyle, Color, Data, FilterMode, Font, FontHinting, FontMgr, Image, MaskFilter,
    Matrix, MipmapMode, Paint, PaintStyle, PathBuilder, PathFillType, Point, RRect, Rect,
    SamplingOptions, Surface, TileMode, Typeface,
    canvas::{SaveLayerRec, SrcRectConstraint},
    color_filters, dash_path_effect,
    font::Edging as FontEdging,
    gpu,
    gradient::{Colors as GradientColors, Gradient, Interpolation},
    image::CachingHint,
    shaders,
};

use crate::render_scene::{
    DrawPrimitive, RenderCacheCandidate, RenderCacheCandidateKind, RenderNode, RenderScene,
};
use crate::tree::attrs::{BorderStyle, ImageFit};
use crate::tree::geometry::{ClipShape, CornerRadii, Rect as GeometryRect, clamp_radii};
use crate::tree::transform::Affine2;
use crate::video::{RendererVideoState, VideoSyncResult};

// ============================================================================
// Render State
// ============================================================================

pub struct RenderState {
    pub scene: RenderScene,
    pub clear_color: Color,
    pub render_version: u64,
    pub pipeline_submitted_at: Option<Instant>,
    pub pipeline_render_queued_at: Option<Instant>,
    pub animate: bool,
    pub has_cache_candidates: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderTimings {
    pub total: Duration,
    pub draw: Duration,
    pub draw_detail: Option<RenderDrawTimings>,
    pub flush: Duration,
    pub gpu_flush: Duration,
    pub submit: Duration,
    /// Keep cache stats absent on normal frames. Returning a larger inline empty
    /// stats value regressed the `raster_direct/mixed_ui_scene` benchmark, so
    /// disabled caches must not enlarge the hot render result.
    pub renderer_cache: Option<Box<RendererCacheFrameStats>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RendererCacheKind {
    #[default]
    Noop,
    CleanSubtree,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RendererCacheFrameStats {
    pub noop: RendererCacheKindFrameStats,
    pub clean_subtree: RendererCacheKindFrameStats,
}

impl RendererCacheFrameStats {
    pub fn is_empty(&self) -> bool {
        self.noop.is_empty() && self.clean_subtree.is_empty()
    }

    fn for_kind_mut(&mut self, kind: RendererCacheKind) -> &mut RendererCacheKindFrameStats {
        match kind {
            RendererCacheKind::Noop => &mut self.noop,
            RendererCacheKind::CleanSubtree => &mut self.clean_subtree,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RendererCacheKindFrameStats {
    pub candidates: u64,
    pub visible_candidates: u64,
    pub admitted: u64,
    pub hits: u64,
    pub misses: u64,
    pub stores: u64,
    pub evictions: u64,
    pub rejected: u64,
    pub current_entries: u64,
    pub current_bytes: u64,
    pub current_gpu_payloads: u64,
    pub current_cpu_payloads: u64,
    pub evicted_bytes: u64,
    pub gpu_payload_stores: u64,
    pub cpu_payload_stores: u64,
    pub prepare_successes: u64,
    pub prepare_failures: u64,
    pub direct_fallbacks_after_admission: u64,
    pub rejected_ineligible: u64,
    pub rejected_admission: u64,
    pub rejected_oversized: u64,
    pub rejected_payload_budget: u64,
    pub prepare_time: Duration,
    pub draw_hit_time: Duration,
}

impl RendererCacheKindFrameStats {
    pub fn is_empty(&self) -> bool {
        self.candidates == 0
            && self.visible_candidates == 0
            && self.admitted == 0
            && self.hits == 0
            && self.misses == 0
            && self.stores == 0
            && self.evictions == 0
            && self.rejected == 0
            && self.current_entries == 0
            && self.current_bytes == 0
            && self.current_gpu_payloads == 0
            && self.current_cpu_payloads == 0
            && self.evicted_bytes == 0
            && self.gpu_payload_stores == 0
            && self.cpu_payload_stores == 0
            && self.prepare_successes == 0
            && self.prepare_failures == 0
            && self.direct_fallbacks_after_admission == 0
            && self.rejected_ineligible == 0
            && self.rejected_admission == 0
            && self.rejected_oversized == 0
            && self.rejected_payload_budget == 0
            && self.prepare_time.is_zero()
            && self.draw_hit_time.is_zero()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderDrawTimings {
    pub clear: Duration,
    pub clips: Duration,
    pub relaxed_clips: Duration,
    pub transforms: Duration,
    pub alphas: Duration,
    pub rects: Duration,
    pub rounded_rects: Duration,
    pub borders: Duration,
    pub shadows: Duration,
    pub inset_shadows: Duration,
    pub texts: Duration,
    pub gradients: Duration,
    pub images: Duration,
    pub videos: Duration,
    pub image_placeholders: Duration,
    pub clip_detail: RenderClipDrawSummary,
    pub border_detail: RenderBorderDrawSummary,
    pub layer_detail: RenderLayerDrawSummary,
    pub shadow_details: Vec<RenderShadowDrawProfile>,
    pub image_details: Vec<RenderImageDrawProfile>,
}

impl RenderDrawTimings {
    pub fn attributed_total(&self) -> Duration {
        self.clear
            + self.clips
            + self.relaxed_clips
            + self.transforms
            + self.alphas
            + self.rects
            + self.rounded_rects
            + self.borders
            + self.shadows
            + self.inset_shadows
            + self.texts
            + self.gradients
            + self.images
            + self.videos
            + self.image_placeholders
    }

    pub fn unattributed(&self, draw: Duration) -> Duration {
        draw.saturating_sub(self.attributed_total())
    }

    fn record_primitive(&mut self, primitive: &DrawPrimitive, duration: Duration) {
        match primitive {
            DrawPrimitive::Rect(..) => self.rects += duration,
            DrawPrimitive::RoundedRect(..) => self.rounded_rects += duration,
            DrawPrimitive::Border(_, _, w, h, radius, width, _, style) => {
                self.borders += duration;
                self.border_detail.record(
                    *style,
                    [*radius; 4],
                    [*width; 4],
                    (*w).max(0.0) * (*h).max(0.0),
                );
            }
            DrawPrimitive::BorderCorners(_, _, w, h, tl, tr, br, bl, width, _, style) => {
                self.borders += duration;
                self.border_detail.record(
                    *style,
                    [*tl, *tr, *br, *bl],
                    [*width; 4],
                    (*w).max(0.0) * (*h).max(0.0),
                );
            }
            DrawPrimitive::BorderEdges(_, _, w, h, radius, top, right, bottom, left, _, style) => {
                self.borders += duration;
                self.border_detail.record(
                    *style,
                    [*radius; 4],
                    [*top, *right, *bottom, *left],
                    (*w).max(0.0) * (*h).max(0.0),
                );
            }
            DrawPrimitive::Shadow(..) => self.shadows += duration,
            DrawPrimitive::InsetShadow(..) => self.inset_shadows += duration,
            DrawPrimitive::TextWithFont(..) => self.texts += duration,
            DrawPrimitive::Gradient(..) => self.gradients += duration,
            DrawPrimitive::Image(..) => self.images += duration,
            DrawPrimitive::Video(..) => self.videos += duration,
            DrawPrimitive::ImageLoading(..) | DrawPrimitive::ImageFailed(..) => {
                self.image_placeholders += duration
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderClipDrawSummary {
    pub clip_scopes: u32,
    pub relaxed_clip_scopes: u32,
    pub empty_clip_scopes: u32,
    pub rect_shapes: u32,
    pub rounded_shapes: u32,
    pub shadow_escape_reapplications: u32,
}

impl RenderClipDrawSummary {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn record_clip_scope(&mut self, relaxed: bool, clips: &[ClipShape]) {
        if relaxed {
            self.relaxed_clip_scopes = self.relaxed_clip_scopes.saturating_add(1);
        } else {
            self.clip_scopes = self.clip_scopes.saturating_add(1);
        }

        if clips.is_empty() {
            self.empty_clip_scopes = self.empty_clip_scopes.saturating_add(1);
        }

        let rounded = clips.iter().filter(|clip| clip.radii.is_some()).count() as u32;
        let rect = clips.len() as u32 - rounded;
        self.rect_shapes = self.rect_shapes.saturating_add(rect);
        self.rounded_shapes = self.rounded_shapes.saturating_add(rounded);
    }

    fn record_shadow_escape_reapplication(&mut self) {
        self.shadow_escape_reapplications = self.shadow_escape_reapplications.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderBorderDrawSummary {
    pub total: u32,
    pub solid: u32,
    pub dashed: u32,
    pub dotted: u32,
    pub uniform_width: u32,
    pub asymmetric_width: u32,
    pub zero_radius: u32,
    pub rounded: u32,
    pub path_clip_candidates: u32,
    pub max_width: f32,
    pub max_area: f32,
}

impl RenderBorderDrawSummary {
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    fn record(&mut self, style: BorderStyle, corners: [f32; 4], insets: [f32; 4], area: f32) {
        self.total = self.total.saturating_add(1);
        match style {
            BorderStyle::Solid => self.solid = self.solid.saturating_add(1),
            BorderStyle::Dashed => self.dashed = self.dashed.saturating_add(1),
            BorderStyle::Dotted => self.dotted = self.dotted.saturating_add(1),
        }

        if insets
            .iter()
            .all(|width| approx_eq(*width, insets.first().copied().unwrap_or(0.0)))
        {
            self.uniform_width = self.uniform_width.saturating_add(1);
        } else {
            self.asymmetric_width = self.asymmetric_width.saturating_add(1);
        }

        if corners.iter().all(|radius| *radius <= 0.0) {
            self.zero_radius = self.zero_radius.saturating_add(1);
        } else {
            self.rounded = self.rounded.saturating_add(1);
        }

        self.path_clip_candidates = self
            .path_clip_candidates
            .saturating_add(border_path_clip_candidate_count(style, insets));
        self.max_width = self
            .max_width
            .max(insets.iter().copied().fold(0.0_f32, f32::max));
        self.max_area = self.max_area.max(area);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderLayerDrawSummary {
    pub alpha_layers: u32,
    pub alpha_children: u32,
    pub max_alpha_children: u32,
    pub tinted_image_layers: u32,
    pub tinted_image_area_px: u64,
    pub max_tinted_image_area_px: u64,
}

impl RenderLayerDrawSummary {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn record_alpha_layer(&mut self, child_count: usize) {
        let child_count = child_count.min(u32::MAX as usize) as u32;
        self.alpha_layers = self.alpha_layers.saturating_add(1);
        self.alpha_children = self.alpha_children.saturating_add(child_count);
        self.max_alpha_children = self.max_alpha_children.max(child_count);
    }

    fn record_tinted_image_layer(&mut self, width: u32, height: u32) {
        let area = u64::from(width).saturating_mul(u64::from(height));
        self.tinted_image_layers = self.tinted_image_layers.saturating_add(1);
        self.tinted_image_area_px = self.tinted_image_area_px.saturating_add(area);
        self.max_tinted_image_area_px = self.max_tinted_image_area_px.max(area);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderShadowDrawPath {
    #[default]
    MaskFilter,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderShadowDrawProfile {
    pub path: RenderShadowDrawPath,
    pub rect_x: f32,
    pub rect_y: f32,
    pub rect_width: f32,
    pub rect_height: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub size: f32,
    pub radius: f32,
    pub color: u32,
    pub total: Duration,
    pub prepare: Duration,
    pub clip: Duration,
    pub draw: Duration,
}

impl RenderShadowDrawProfile {
    fn new(spec: ShadowDrawSpec) -> Self {
        Self {
            rect_x: spec.rect.x,
            rect_y: spec.rect.y,
            rect_width: spec.rect.w,
            rect_height: spec.rect.h,
            offset_x: spec.offset_x,
            offset_y: spec.offset_y,
            blur: spec.blur,
            size: spec.size,
            radius: spec.radius,
            color: spec.color,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderImageDrawProfile {
    pub image_id: String,
    pub kind: RenderImageAssetKind,
    pub fit: ImageFit,
    pub tinted: bool,
    pub tint_layer_used: bool,
    pub source_width: u32,
    pub source_height: u32,
    pub draw_width: u32,
    pub draw_height: u32,
    pub total: Duration,
    pub asset_lookup: Duration,
    pub fit_compute: Duration,
    pub vector_cache_lookup: Duration,
    pub vector_cache_hit: Option<bool>,
    pub vector_rasterize: Duration,
    pub vector_cache_store: Duration,
    pub draw: Duration,
}

impl RenderImageDrawProfile {
    fn new(spec: ImageDrawSpec<'_>) -> Self {
        Self {
            image_id: spec.image_id.to_string(),
            fit: spec.fit,
            tinted: spec.svg_tint.is_some(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderImageAssetKind {
    #[default]
    Missing,
    Raster,
    Vector,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderFlushTimings {
    pub total: Duration,
    pub gpu_flush: Duration,
    pub submit: Duration,
}

const RELAXED_IMAGE_DRAW_BLEED_DEVICE_OUTSET: f32 = 1.0;

impl Default for RenderState {
    fn default() -> Self {
        Self {
            scene: RenderScene::default(),
            clear_color: Color::TRANSPARENT,
            render_version: 0,
            pipeline_submitted_at: None,
            pipeline_render_queued_at: None,
            animate: false,
            has_cache_candidates: false,
        }
    }
}

impl RenderState {
    pub fn new(scene: RenderScene, clear_color: Color, render_version: u64, animate: bool) -> Self {
        let has_cache_candidates = scene.has_cache_candidates();
        Self {
            scene,
            clear_color,
            render_version,
            pipeline_submitted_at: None,
            pipeline_render_queued_at: None,
            animate,
            has_cache_candidates,
        }
    }
}

// ============================================================================
// Font Cache
// ============================================================================

// Embedded default fonts (Inter, OFL licensed)
static DEFAULT_FONT_REGULAR: &[u8] = include_bytes!("fonts/Inter-Regular.ttf");
static DEFAULT_FONT_BOLD: &[u8] = include_bytes!("fonts/Inter-Bold.ttf");
static DEFAULT_FONT_ITALIC: &[u8] = include_bytes!("fonts/Inter-Italic.ttf");
static DEFAULT_FONT_BOLD_ITALIC: &[u8] = include_bytes!("fonts/Inter-BoldItalic.ttf");

/// Key for looking up fonts in the cache.
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct FontKey {
    pub family: String,
    pub weight: u16, // 100-900, 400=normal, 700=bold
    pub italic: bool,
}

impl FontKey {
    pub fn new(family: impl Into<String>, weight: u16, italic: bool) -> Self {
        Self {
            family: family.into(),
            weight,
            italic,
        }
    }

    pub fn default_regular() -> Self {
        Self::new("default", 400, false)
    }

    pub fn default_bold() -> Self {
        Self::new("default", 700, false)
    }

    pub fn default_italic() -> Self {
        Self::new("default", 400, true)
    }

    pub fn default_bold_italic() -> Self {
        Self::new("default", 700, true)
    }
}

impl Default for FontKey {
    fn default() -> Self {
        Self::default_regular()
    }
}

static FONT_CACHE: OnceLock<Mutex<HashMap<FontKey, Arc<Typeface>>>> = OnceLock::new();
static SYNTHETIC_LOGGED: OnceLock<Mutex<HashSet<FontKey>>> = OnceLock::new();
static RENDER_LOG_ENABLED: AtomicBool = AtomicBool::new(false);

fn default_font_cache() -> HashMap<FontKey, Arc<Typeface>> {
    let mut cache = HashMap::new();
    let font_mgr = FontMgr::new();

    if let Some(tf) = font_mgr.new_from_data(DEFAULT_FONT_REGULAR, 0) {
        cache.insert(FontKey::default_regular(), Arc::new(tf));
    }
    if let Some(tf) = font_mgr.new_from_data(DEFAULT_FONT_BOLD, 0) {
        cache.insert(FontKey::default_bold(), Arc::new(tf));
    }
    if let Some(tf) = font_mgr.new_from_data(DEFAULT_FONT_ITALIC, 0) {
        cache.insert(FontKey::default_italic(), Arc::new(tf));
    }
    if let Some(tf) = font_mgr.new_from_data(DEFAULT_FONT_BOLD_ITALIC, 0) {
        cache.insert(FontKey::default_bold_italic(), Arc::new(tf));
    }

    cache
}

fn get_font_cache() -> &'static Mutex<HashMap<FontKey, Arc<Typeface>>> {
    FONT_CACHE.get_or_init(|| Mutex::new(default_font_cache()))
}

fn get_synthetic_log_cache() -> &'static Mutex<HashSet<FontKey>> {
    SYNTHETIC_LOGGED.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn set_render_log_enabled(enabled: bool) {
    RENDER_LOG_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Get a typeface from the cache by key.
pub fn get_typeface(key: &FontKey) -> Option<Arc<Typeface>> {
    let cache = get_font_cache().lock().ok()?;
    cache.get(key).cloned()
}

fn resolve_typeface_with_fallback(
    family: &str,
    weight: u16,
    italic: bool,
) -> (Arc<Typeface>, bool) {
    let requested = FontKey::new(family, weight, italic);
    if let Some(tf) = get_typeface(&requested) {
        return (tf, true);
    }

    let default_requested = FontKey::new("default", weight, italic);
    if let Some(tf) = get_typeface(&default_requested) {
        return (tf, true);
    }

    let key = FontKey::new(family, 400, false);
    if let Some(tf) = get_typeface(&key) {
        return (tf, false);
    }

    let fallback = FontKey::default_regular();
    let tf = get_typeface(&fallback).expect("embedded default font must exist");
    (tf, false)
}

pub fn make_font_with_style(family: &str, weight: u16, italic: bool, size: f32) -> Font {
    let (typeface, exact) = resolve_typeface_with_fallback(family, weight, italic);
    let mut font = Font::new(&*typeface, size);
    configure_text_font(&mut font);

    if !exact {
        if weight >= 600 {
            font.set_embolden(true);
        }
        if italic {
            font.set_skew_x(-0.25);
        }

        if RENDER_LOG_ENABLED.load(Ordering::Relaxed) {
            let key = FontKey::new(family, weight, italic);
            if let Ok(mut cache) = get_synthetic_log_cache().lock()
                && cache.insert(key)
            {
                eprintln!(
                    "synthetic font style applied family={} weight={} italic={}",
                    family, weight, italic
                );
            }
        }
    }

    font
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextVisualMetrics {
    pub(crate) advance: f32,
    pub(crate) left_overhang: f32,
    pub(crate) visual_width: f32,
}

pub(crate) fn measure_text_visual_metrics_with_font(font: &Font, text: &str) -> TextVisualMetrics {
    if text.is_empty() {
        return TextVisualMetrics {
            advance: 0.0,
            left_overhang: 0.0,
            visual_width: 0.0,
        };
    }

    let (advance, bounds) = font.measure_str(text, None);
    let left_overhang = (-bounds.left()).max(0.0);
    let right_overhang = (bounds.right() - advance).max(0.0);

    TextVisualMetrics {
        advance,
        left_overhang,
        visual_width: (advance + left_overhang + right_overhang).max(0.0),
    }
}

pub(crate) fn measure_text_visual_metrics(
    family: &str,
    weight: u16,
    italic: bool,
    size: f32,
    text: &str,
) -> TextVisualMetrics {
    let font = make_font_with_style(family, weight, italic, size);
    measure_text_visual_metrics_with_font(&font, text)
}

fn configure_text_font(font: &mut Font) {
    font.set_subpixel(true);
    font.set_linear_metrics(true);
    font.set_baseline_snap(false);
    font.set_edging(FontEdging::AntiAlias);
    font.set_hinting(FontHinting::Slight);
}

/// Load a font from binary data and register it in the cache.
pub fn load_font(family: &str, weight: u16, italic: bool, data: &[u8]) -> Result<(), String> {
    let font_mgr = FontMgr::new();
    let typeface = font_mgr
        .new_from_data(data, 0)
        .ok_or_else(|| "Invalid font data".to_string())?;

    let cache = get_font_cache();
    let mut cache = cache.lock().map_err(|_| "Font cache lock poisoned")?;

    cache.insert(FontKey::new(family, weight, italic), Arc::new(typeface));
    bump_font_cache_generation();

    Ok(())
}

#[derive(Clone)]
enum CachedAssetKind {
    Raster(Image),
    Vector(Box<usvg::Tree>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Raster,
    Vector,
}

#[derive(Clone)]
struct CachedAsset {
    kind: CachedAssetKind,
    width: u32,
    height: u32,
    generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RenderedVectorKey {
    asset_id: String,
    width: u32,
    height: u32,
}

#[derive(Clone)]
struct RenderedVectorVariant {
    image: Image,
    bytes: usize,
    last_used: u64,
}

struct RenderedVectorCache {
    entries: HashMap<RenderedVectorKey, RenderedVectorVariant>,
    total_bytes: usize,
    access_clock: u64,
    max_entries: usize,
    max_bytes: usize,
}

const RENDERED_VECTOR_CACHE_MAX_ENTRIES: usize = 256;
const RENDERED_VECTOR_CACHE_MAX_BYTES: usize = 16 * 1024 * 1024;
const RENDERED_VECTOR_CACHE_MAX_VARIANT_BYTES: usize = 1024 * 1024;

impl Default for RenderedVectorCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            total_bytes: 0,
            access_clock: 0,
            max_entries: RENDERED_VECTOR_CACHE_MAX_ENTRIES,
            max_bytes: RENDERED_VECTOR_CACHE_MAX_BYTES,
        }
    }
}

static ASSET_CACHE: OnceLock<Mutex<HashMap<String, Arc<CachedAsset>>>> = OnceLock::new();
static RENDERED_VECTOR_CACHE: OnceLock<Mutex<RenderedVectorCache>> = OnceLock::new();
static ASSET_CACHE_GENERATION: AtomicU64 = AtomicU64::new(1);
static FONT_CACHE_GENERATION: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
static VECTOR_RASTERIZATION_COUNT: AtomicUsize = AtomicUsize::new(0);

fn get_asset_cache() -> &'static Mutex<HashMap<String, Arc<CachedAsset>>> {
    ASSET_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_rendered_vector_cache() -> &'static Mutex<RenderedVectorCache> {
    RENDERED_VECTOR_CACHE.get_or_init(|| Mutex::new(RenderedVectorCache::default()))
}

fn bump_asset_cache_generation() -> u64 {
    ASSET_CACHE_GENERATION.fetch_add(1, Ordering::Relaxed) + 1
}

fn font_cache_generation() -> u64 {
    FONT_CACHE_GENERATION.load(Ordering::Relaxed)
}

fn bump_font_cache_generation() -> u64 {
    FONT_CACHE_GENERATION.fetch_add(1, Ordering::Relaxed) + 1
}

#[cfg(not(test))]
pub fn clear_global_caches() {
    if let Some(cache) = FONT_CACHE.get()
        && let Ok(mut cache) = cache.lock()
    {
        *cache = default_font_cache();
    }

    if let Some(cache) = SYNTHETIC_LOGGED.get()
        && let Ok(mut cache) = cache.lock()
    {
        cache.clear();
    }

    if let Some(cache) = ASSET_CACHE.get()
        && let Ok(mut cache) = cache.lock()
    {
        cache.clear();
    }

    if let Some(cache) = RENDERED_VECTOR_CACHE.get()
        && let Ok(mut cache) = cache.lock()
    {
        *cache = RenderedVectorCache::default();
    }

    skia_safe::graphics::purge_all_caches();
    bump_asset_cache_generation();
    bump_font_cache_generation();
}

#[cfg(test)]
pub fn clear_global_caches() {}

fn cached_asset(id: &str) -> Option<Arc<CachedAsset>> {
    let cache = get_asset_cache().lock().ok()?;
    cache.get(id).cloned()
}

pub fn asset_dimensions(id: &str) -> Option<(u32, u32)> {
    cached_asset(id).map(|cached| (cached.width, cached.height))
}

pub fn asset_kind(id: &str) -> Option<AssetKind> {
    cached_asset(id).map(|cached| match &cached.kind {
        CachedAssetKind::Raster(_) => AssetKind::Raster,
        CachedAssetKind::Vector(_) => AssetKind::Vector,
    })
}

fn rendered_vector_key(asset_id: &str, width: u32, height: u32) -> RenderedVectorKey {
    RenderedVectorKey {
        asset_id: asset_id.to_string(),
        width,
        height,
    }
}

fn rendered_variant_bytes(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)
}

fn should_cache_rendered_variant(width: u32, height: u32) -> bool {
    rendered_variant_bytes(width, height)
        .map(|bytes| bytes <= RENDERED_VECTOR_CACHE_MAX_VARIANT_BYTES)
        .unwrap_or(false)
}

fn next_rendered_vector_access_stamp(cache: &mut RenderedVectorCache) -> u64 {
    cache.access_clock = cache.access_clock.wrapping_add(1);
    cache.access_clock
}

fn lookup_rendered_vector_variant(asset_id: &str, width: u32, height: u32) -> Option<Image> {
    let mut cache = get_rendered_vector_cache().lock().ok()?;
    let key = rendered_vector_key(asset_id, width, height);
    let stamp = next_rendered_vector_access_stamp(&mut cache);
    let variant = cache.entries.get_mut(&key)?;
    variant.last_used = stamp;
    Some(variant.image.clone())
}

fn evict_rendered_vector_variants_if_needed(cache: &mut RenderedVectorCache) {
    while cache.entries.len() > cache.max_entries || cache.total_bytes > cache.max_bytes {
        let Some(oldest_key) = cache
            .entries
            .iter()
            .min_by_key(|(_, variant)| variant.last_used)
            .map(|(key, _)| key.clone())
        else {
            break;
        };

        if let Some(variant) = cache.entries.remove(&oldest_key) {
            cache.total_bytes = cache.total_bytes.saturating_sub(variant.bytes);
        }
    }
}

fn store_rendered_vector_variant(asset_id: &str, width: u32, height: u32, image: &Image) {
    if !should_cache_rendered_variant(width, height) {
        return;
    }

    let Some(bytes) = rendered_variant_bytes(width, height) else {
        return;
    };

    let Ok(mut cache) = get_rendered_vector_cache().lock() else {
        return;
    };

    let key = rendered_vector_key(asset_id, width, height);
    if let Some(existing) = cache.entries.remove(&key) {
        cache.total_bytes = cache.total_bytes.saturating_sub(existing.bytes);
    }

    let stamp = next_rendered_vector_access_stamp(&mut cache);
    cache.entries.insert(
        key,
        RenderedVectorVariant {
            image: image.clone(),
            bytes,
            last_used: stamp,
        },
    );
    cache.total_bytes = cache.total_bytes.saturating_add(bytes);
    evict_rendered_vector_variants_if_needed(&mut cache);
}

fn clear_rendered_vector_variants(asset_id: &str) {
    let Ok(mut cache) = get_rendered_vector_cache().lock() else {
        return;
    };

    let mut retained = HashMap::with_capacity(cache.entries.len());
    let mut total_bytes = 0usize;

    for (key, variant) in cache.entries.drain() {
        if key.asset_id == asset_id {
            continue;
        }

        total_bytes = total_bytes.saturating_add(variant.bytes);
        retained.insert(key, variant);
    }

    cache.entries = retained;
    cache.total_bytes = total_bytes;
}

pub fn insert_raster_asset(id: &str, data: &[u8]) -> Result<(u32, u32), String> {
    let image = Image::from_encoded(Data::new_copy(data))
        .and_then(|image| {
            image.make_raster_image(None::<&mut gpu::DirectContext>, CachingHint::Allow)
        })
        .ok_or_else(|| "failed to decode image data".to_string())?;

    let width = image.width().max(0) as u32;
    let height = image.height().max(0) as u32;

    clear_rendered_vector_variants(id);

    let cache = get_asset_cache();
    let mut cache = cache.lock().map_err(|_| "image cache lock poisoned")?;
    let generation = bump_asset_cache_generation();
    cache.insert(
        id.to_string(),
        Arc::new(CachedAsset {
            kind: CachedAssetKind::Raster(image),
            width,
            height,
            generation,
        }),
    );

    Ok((width, height))
}

pub fn insert_vector_asset(id: &str, tree: usvg::Tree) -> Result<(u32, u32), String> {
    let width = tree.size().width().ceil().max(1.0) as u32;
    let height = tree.size().height().ceil().max(1.0) as u32;

    clear_rendered_vector_variants(id);

    let cache = get_asset_cache();
    let mut cache = cache.lock().map_err(|_| "asset cache lock poisoned")?;
    let generation = bump_asset_cache_generation();
    cache.insert(
        id.to_string(),
        Arc::new(CachedAsset {
            kind: CachedAssetKind::Vector(Box::new(tree)),
            width,
            height,
            generation,
        }),
    );

    Ok((width, height))
}

#[cfg(test)]
pub fn insert_test_raster_asset_rgba(
    id: &str,
    width: u32,
    height: u32,
    rgba_pixels: &[u8],
) -> Result<(), String> {
    let image = raster_image_from_rgba(width, height, rgba_pixels)
        .ok_or_else(|| "failed to create raster image from RGBA pixels".to_string())?;

    clear_rendered_vector_variants(id);

    let cache = get_asset_cache();
    let mut cache = cache.lock().map_err(|_| "asset cache lock poisoned")?;
    let generation = bump_asset_cache_generation();
    cache.insert(
        id.to_string(),
        Arc::new(CachedAsset {
            kind: CachedAssetKind::Raster(image),
            width,
            height,
            generation,
        }),
    );

    Ok(())
}

fn raster_image_from_rgba(width: u32, height: u32, rgba_pixels: &[u8]) -> Option<Image> {
    let info = skia_safe::ImageInfo::new(
        (width as i32, height as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let data = Data::new_copy(rgba_pixels);
    skia_safe::images::raster_from_data(&info, data, (width * 4) as usize)
}

#[cfg(test)]
fn remove_asset(id: &str) {
    if let Ok(mut cache) = get_asset_cache().lock() {
        cache.remove(id);
    }

    clear_rendered_vector_variants(id);
    bump_asset_cache_generation();
}

// ============================================================================
// Renderer
// ============================================================================

pub struct RenderFrame<'a> {
    surface: &'a mut Surface,
    direct_context: Option<&'a mut gpu::DirectContext>,
}

impl<'a> RenderFrame<'a> {
    pub fn new(
        surface: &'a mut Surface,
        direct_context: Option<&'a mut gpu::DirectContext>,
    ) -> Self {
        Self {
            surface,
            direct_context,
        }
    }

    pub fn surface_mut(&mut self) -> &mut Surface {
        self.surface
    }

    pub fn flush(&mut self) -> RenderFlushTimings {
        let started_at = Instant::now();
        let mut gpu_flush = Duration::ZERO;
        let mut submit = Duration::ZERO;

        if let Some(gr_context) = self.direct_context.as_deref_mut() {
            let flush_started_at = Instant::now();
            gr_context.flush(None);
            gpu_flush = flush_started_at.elapsed();

            let submit_started_at = Instant::now();
            gr_context.submit(gpu::SyncCpu::No);
            submit = submit_started_at.elapsed();
        }

        RenderFlushTimings {
            total: started_at.elapsed(),
            gpu_flush,
            submit,
        }
    }
}

const RENDERER_CACHE_DEFAULT_NEW_PAYLOADS_PER_FRAME: u32 = 1;
const CLEAN_SUBTREE_CACHE_MIN_VISIBLE_BEFORE_STORE: u64 = 2;
const CLEAN_SUBTREE_CACHE_MAX_ENTRIES: usize = 128;
const CLEAN_SUBTREE_CACHE_MAX_BYTES: u64 = 32 * 1024 * 1024;
const CLEAN_SUBTREE_CACHE_MAX_ENTRY_BYTES: u64 = 4 * 1024 * 1024;
const CLEAN_SUBTREE_CACHE_BYTES_PER_PIXEL: u64 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RendererCacheConfig {
    pub max_new_payloads_per_frame: u32,
    pub clean_subtree: CleanSubtreeCacheConfig,
}

impl Default for RendererCacheConfig {
    fn default() -> Self {
        Self {
            max_new_payloads_per_frame: RENDERER_CACHE_DEFAULT_NEW_PAYLOADS_PER_FRAME,
            clean_subtree: CleanSubtreeCacheConfig::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CleanSubtreeCacheConfig {
    pub max_entries: usize,
    pub max_bytes: u64,
    pub max_entry_bytes: u64,
}

impl Default for CleanSubtreeCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: CLEAN_SUBTREE_CACHE_MAX_ENTRIES,
            max_bytes: CLEAN_SUBTREE_CACHE_MAX_BYTES,
            max_entry_bytes: CLEAN_SUBTREE_CACHE_MAX_ENTRY_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CleanSubtreeContentKey {
    pub stable_id: u64,
    pub content_generation: u64,
    pub width_px: u32,
    pub height_px: u32,
    pub scale_bits: u32,
    pub resource_generation: u64,
}

impl CleanSubtreeContentKey {
    pub fn from_candidate(
        candidate: &RenderCacheCandidate,
        scale: f32,
        resource_generation: u64,
    ) -> Option<Self> {
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }

        let (width_px, height_px, _) = clean_subtree_bounds_size(candidate.bounds)?;
        Some(Self {
            stable_id: candidate.stable_id,
            content_generation: candidate.content_generation,
            width_px,
            height_px,
            scale_bits: scale.to_bits(),
            resource_generation,
        })
    }

    pub fn byte_len(self) -> Option<u64> {
        clean_subtree_byte_len(self.width_px, self.height_px)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CleanSubtreePlacement {
    pub x_px: i32,
    pub y_px: i32,
}

impl CleanSubtreePlacement {
    pub fn from_transform(transform: Affine2) -> Result<Self, CleanSubtreePlacementRejection> {
        if !approx_eq(transform.xx, 1.0)
            || !approx_eq(transform.yx, 0.0)
            || !approx_eq(transform.xy, 0.0)
            || !approx_eq(transform.yy, 1.0)
        {
            return Err(CleanSubtreePlacementRejection::UnsupportedTransform);
        }

        Self::from_translation(transform.tx, transform.ty)
    }

    pub fn from_translation(x: f32, y: f32) -> Result<Self, CleanSubtreePlacementRejection> {
        if !x.is_finite() || !y.is_finite() {
            return Err(CleanSubtreePlacementRejection::NonFiniteTranslation);
        }

        let rounded_x = x.round();
        let rounded_y = y.round();
        if !approx_eq(x, rounded_x) || !approx_eq(y, rounded_y) {
            return Err(CleanSubtreePlacementRejection::FractionalTranslation);
        }

        if rounded_x < i32::MIN as f32
            || rounded_x > i32::MAX as f32
            || rounded_y < i32::MIN as f32
            || rounded_y > i32::MAX as f32
        {
            return Err(CleanSubtreePlacementRejection::OutOfRangeTranslation);
        }

        Ok(Self {
            x_px: rounded_x as i32,
            y_px: rounded_y as i32,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanSubtreePlacementRejection {
    UnsupportedTransform,
    NonFiniteTranslation,
    FractionalTranslation,
    OutOfRangeTranslation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanSubtreeStoreRejection {
    AdmissionThreshold,
    OversizedEntry,
    PayloadBudget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererCachePayloadKind {
    GpuRenderTarget,
    CpuRaster,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererCacheRejectionReason {
    Ineligible,
    AdmissionThreshold,
    OversizedEntry,
    PayloadBudget,
}

impl From<CleanSubtreeStoreRejection> for RendererCacheRejectionReason {
    fn from(rejection: CleanSubtreeStoreRejection) -> Self {
        match rejection {
            CleanSubtreeStoreRejection::AdmissionThreshold => Self::AdmissionThreshold,
            CleanSubtreeStoreRejection::OversizedEntry => Self::OversizedEntry,
            CleanSubtreeStoreRejection::PayloadBudget => Self::PayloadBudget,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CleanSubtreeEntry {
    pub key: CleanSubtreeContentKey,
    pub bytes: u64,
    pub payload_kind: RendererCachePayloadKind,
    pub visible_count: u64,
    pub first_visible_frame: u64,
    pub last_visible_frame: u64,
    pub last_used_frame: u64,
    image: Option<Image>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CleanSubtreeAccess {
    visible_count: u64,
    first_visible_frame: u64,
    last_visible_frame: u64,
}

#[derive(Debug)]
struct CleanSubtreeCache {
    entries: HashMap<CleanSubtreeContentKey, CleanSubtreeEntry>,
    visible_accesses: HashMap<CleanSubtreeContentKey, CleanSubtreeAccess>,
    total_bytes: u64,
    max_entries: usize,
    max_bytes: u64,
    max_entry_bytes: u64,
    min_visible_before_store: u64,
}

impl Default for CleanSubtreeCache {
    fn default() -> Self {
        Self::with_config(CleanSubtreeCacheConfig::default())
    }
}

impl CleanSubtreeCache {
    fn with_config(config: CleanSubtreeCacheConfig) -> Self {
        Self {
            entries: HashMap::new(),
            visible_accesses: HashMap::new(),
            total_bytes: 0,
            max_entries: config.max_entries,
            max_bytes: config.max_bytes,
            max_entry_bytes: config.max_entry_bytes,
            min_visible_before_store: CLEAN_SUBTREE_CACHE_MIN_VISIBLE_BEFORE_STORE,
        }
    }
}

impl CleanSubtreeCache {
    fn clear(&mut self) {
        self.entries.clear();
        self.visible_accesses.clear();
        self.total_bytes = 0;
    }

    fn mark_visible(&mut self, key: CleanSubtreeContentKey, frame_index: u64) -> u64 {
        let access = self
            .visible_accesses
            .entry(key)
            .or_insert(CleanSubtreeAccess {
                visible_count: 0,
                first_visible_frame: frame_index,
                last_visible_frame: frame_index,
            });
        access.visible_count = access.visible_count.saturating_add(1);
        access.last_visible_frame = frame_index;

        if let Some(entry) = self.entries.get_mut(&key) {
            entry.visible_count = access.visible_count;
            entry.last_visible_frame = frame_index;
            entry.last_used_frame = frame_index;
        }

        access.visible_count
    }

    fn image(&mut self, key: CleanSubtreeContentKey, frame_index: u64) -> Option<Image> {
        let entry = self.entries.get_mut(&key)?;
        entry.last_used_frame = frame_index;
        entry.image.clone()
    }

    fn try_store_metadata(
        &mut self,
        key: CleanSubtreeContentKey,
        bytes: u64,
        frame_index: u64,
    ) -> Result<Vec<u64>, CleanSubtreeStoreRejection> {
        self.try_store_entry(
            key,
            bytes,
            frame_index,
            RendererCachePayloadKind::CpuRaster,
            None,
        )
    }

    fn try_store_payload(
        &mut self,
        key: CleanSubtreeContentKey,
        bytes: u64,
        frame_index: u64,
        payload_kind: RendererCachePayloadKind,
        image: Image,
    ) -> Result<Vec<u64>, CleanSubtreeStoreRejection> {
        self.try_store_entry(key, bytes, frame_index, payload_kind, Some(image))
    }

    fn try_store_entry(
        &mut self,
        key: CleanSubtreeContentKey,
        bytes: u64,
        frame_index: u64,
        payload_kind: RendererCachePayloadKind,
        image: Option<Image>,
    ) -> Result<Vec<u64>, CleanSubtreeStoreRejection> {
        if bytes > self.max_entry_bytes {
            return Err(CleanSubtreeStoreRejection::OversizedEntry);
        }

        let access = self
            .visible_accesses
            .get(&key)
            .copied()
            .ok_or(CleanSubtreeStoreRejection::AdmissionThreshold)?;
        if access.visible_count < self.min_visible_before_store {
            return Err(CleanSubtreeStoreRejection::AdmissionThreshold);
        }

        if let Some(existing) = self.entries.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(existing.bytes);
        }

        self.entries.insert(
            key,
            CleanSubtreeEntry {
                key,
                bytes,
                payload_kind,
                visible_count: access.visible_count,
                first_visible_frame: access.first_visible_frame,
                last_visible_frame: access.last_visible_frame,
                last_used_frame: frame_index,
                image,
            },
        );
        self.total_bytes = self.total_bytes.saturating_add(bytes);

        Ok(self.evict_if_needed())
    }

    fn evict_if_needed(&mut self) -> Vec<u64> {
        let mut evicted = Vec::new();
        while self.entries.len() > self.max_entries || self.total_bytes > self.max_bytes {
            let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used_frame)
                .map(|(key, _)| *key)
            else {
                break;
            };

            if let Some(entry) = self.entries.remove(&oldest_key) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.bytes);
                evicted.push(entry.bytes);
            }
        }
        evicted
    }

    fn entry_count(&self) -> u64 {
        self.entries.len() as u64
    }

    fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    fn payload_counts(&self) -> (u64, u64) {
        self.entries
            .values()
            .fold((0u64, 0u64), |(gpu, cpu), entry| match entry.payload_kind {
                RendererCachePayloadKind::GpuRenderTarget => (gpu.saturating_add(1), cpu),
                RendererCachePayloadKind::CpuRaster => (gpu, cpu.saturating_add(1)),
            })
    }
}

fn clean_subtree_bounds_size(bounds: GeometryRect) -> Option<(u32, u32, u64)> {
    if !bounds.x.is_finite()
        || !bounds.y.is_finite()
        || !bounds.width.is_finite()
        || !bounds.height.is_finite()
        || bounds.width <= 0.0
        || bounds.height <= 0.0
    {
        return None;
    }

    let width = bounds.width.ceil();
    let height = bounds.height.ceil();
    if width > u32::MAX as f32 || height > u32::MAX as f32 {
        return None;
    }

    let width_px = width as u32;
    let height_px = height as u32;
    let bytes = clean_subtree_byte_len(width_px, height_px)?;
    Some((width_px, height_px, bytes))
}

fn clean_subtree_byte_len(width_px: u32, height_px: u32) -> Option<u64> {
    u64::from(width_px)
        .checked_mul(u64::from(height_px))?
        .checked_mul(CLEAN_SUBTREE_CACHE_BYTES_PER_PIXEL)
}

fn clean_subtree_placement(
    candidate: &RenderCacheCandidate,
    current_transform: Affine2,
) -> Result<CleanSubtreePlacement, CleanSubtreePlacementRejection> {
    if !approx_eq(current_transform.xx, 1.0)
        || !approx_eq(current_transform.yx, 0.0)
        || !approx_eq(current_transform.xy, 0.0)
        || !approx_eq(current_transform.yy, 1.0)
    {
        return Err(CleanSubtreePlacementRejection::UnsupportedTransform);
    }

    CleanSubtreePlacement::from_translation(
        current_transform.tx + candidate.bounds.x,
        current_transform.ty + candidate.bounds.y,
    )
}

fn clean_subtree_children_are_cacheable(nodes: &[RenderNode]) -> bool {
    nodes.iter().all(|node| match node {
        RenderNode::ShadowPass { .. } => false,
        RenderNode::Clip { children, .. } | RenderNode::RelaxedClip { children, .. } => {
            clean_subtree_children_are_cacheable(children)
        }
        RenderNode::Transform { .. } | RenderNode::Alpha { .. } => false,
        RenderNode::CacheCandidate(candidate) => {
            clean_subtree_children_are_cacheable(&candidate.children)
        }
        RenderNode::Primitive(primitive) => match primitive {
            DrawPrimitive::Video(..)
            | DrawPrimitive::ImageLoading(..)
            | DrawPrimitive::ImageFailed(..) => false,
            DrawPrimitive::Rect(..)
            | DrawPrimitive::RoundedRect(..)
            | DrawPrimitive::Border(..)
            | DrawPrimitive::BorderCorners(..)
            | DrawPrimitive::BorderEdges(..)
            | DrawPrimitive::Shadow(..)
            | DrawPrimitive::InsetShadow(..)
            | DrawPrimitive::TextWithFont(..)
            | DrawPrimitive::Gradient(..)
            | DrawPrimitive::Image(..) => true,
        },
    })
}

fn clean_subtree_resource_generation(nodes: &[RenderNode]) -> Option<u64> {
    nodes.iter().try_fold(0u64, |generation, node| {
        let node_generation = match node {
            RenderNode::ShadowPass { .. } => return None,
            RenderNode::Clip { children, .. } | RenderNode::RelaxedClip { children, .. } => {
                clean_subtree_resource_generation(children)?
            }
            RenderNode::Transform { .. } | RenderNode::Alpha { .. } => return None,
            RenderNode::CacheCandidate(candidate) => {
                clean_subtree_resource_generation(&candidate.children)?
            }
            RenderNode::Primitive(primitive) => match primitive {
                DrawPrimitive::Video(..)
                | DrawPrimitive::ImageLoading(..)
                | DrawPrimitive::ImageFailed(..) => return None,
                DrawPrimitive::TextWithFont(..) => font_cache_generation(),
                DrawPrimitive::Image(_, _, _, _, image_id, _, _) => {
                    cached_asset(image_id).map(|asset| asset.generation)?
                }
                DrawPrimitive::Rect(..)
                | DrawPrimitive::RoundedRect(..)
                | DrawPrimitive::Border(..)
                | DrawPrimitive::BorderCorners(..)
                | DrawPrimitive::BorderEdges(..)
                | DrawPrimitive::Shadow(..)
                | DrawPrimitive::InsetShadow(..)
                | DrawPrimitive::Gradient(..) => 0,
            },
        };

        Some(
            generation
                .wrapping_mul(1_099_511_628_211)
                .wrapping_add(node_generation),
        )
    })
}

#[derive(Debug)]
pub struct RendererCacheManager {
    generation: u64,
    frame_index: u64,
    max_new_payloads_per_frame: u32,
    clean_subtree: CleanSubtreeCache,
}

impl Default for RendererCacheManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RendererCacheManager {
    pub fn new() -> Self {
        Self::with_config(RendererCacheConfig::default())
    }

    pub fn with_config(config: RendererCacheConfig) -> Self {
        Self {
            generation: 0,
            frame_index: 0,
            max_new_payloads_per_frame: config.max_new_payloads_per_frame,
            clean_subtree: CleanSubtreeCache::with_config(config.clean_subtree),
        }
    }

    pub fn begin_frame(&mut self) -> RendererCacheFrame {
        self.frame_index = self.frame_index.wrapping_add(1);
        RendererCacheFrame {
            generation: self.generation,
            frame_index: self.frame_index,
            new_payload_budget_remaining: self.max_new_payloads_per_frame,
            stats: RendererCacheFrameStats::default(),
        }
    }

    pub fn end_frame(&mut self, frame: RendererCacheFrame) -> RendererCacheFrameStats {
        debug_assert_eq!(frame.generation, self.generation);
        let mut stats = frame.stats;
        stats.clean_subtree.current_entries = self.clean_subtree.entry_count();
        stats.clean_subtree.current_bytes = self.clean_subtree.total_bytes();
        let (gpu_payloads, cpu_payloads) = self.clean_subtree.payload_counts();
        stats.clean_subtree.current_gpu_payloads = gpu_payloads;
        stats.clean_subtree.current_cpu_payloads = cpu_payloads;
        stats
    }

    pub fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.clean_subtree.clear();
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn mark_clean_subtree_visible(
        &mut self,
        frame: &mut RendererCacheFrame,
        key: CleanSubtreeContentKey,
    ) -> u64 {
        frame.mark_candidate(RendererCacheKind::CleanSubtree, true);
        self.clean_subtree.mark_visible(key, frame.frame_index)
    }

    pub fn clean_subtree_payload(
        &mut self,
        frame: &RendererCacheFrame,
        key: CleanSubtreeContentKey,
    ) -> Option<Image> {
        self.clean_subtree.image(key, frame.frame_index)
    }

    pub fn clean_subtree_visible_count_allows_store(&self, visible_count: u64) -> bool {
        visible_count >= self.clean_subtree.min_visible_before_store
    }

    pub fn try_store_clean_subtree_metadata(
        &mut self,
        frame: &mut RendererCacheFrame,
        key: CleanSubtreeContentKey,
        bytes: u64,
        prepare_time: Duration,
    ) -> Result<(), CleanSubtreeStoreRejection> {
        if bytes > self.clean_subtree.max_entry_bytes {
            frame.record_rejection(
                RendererCacheKind::CleanSubtree,
                RendererCacheRejectionReason::OversizedEntry,
            );
            return Err(CleanSubtreeStoreRejection::OversizedEntry);
        }

        let visible_count = self
            .clean_subtree
            .visible_accesses
            .get(&key)
            .map(|access| access.visible_count)
            .unwrap_or(0);
        if visible_count < self.clean_subtree.min_visible_before_store {
            frame.record_rejection(
                RendererCacheKind::CleanSubtree,
                RendererCacheRejectionReason::AdmissionThreshold,
            );
            return Err(CleanSubtreeStoreRejection::AdmissionThreshold);
        }

        frame.admit_candidate(RendererCacheKind::CleanSubtree);
        if !frame.try_consume_new_payload_budget(RendererCacheKind::CleanSubtree) {
            return Err(CleanSubtreeStoreRejection::PayloadBudget);
        }

        let evicted = self
            .clean_subtree
            .try_store_metadata(key, bytes, frame.frame_index)?;
        frame.record_store(
            RendererCacheKind::CleanSubtree,
            bytes,
            RendererCachePayloadKind::CpuRaster,
            prepare_time,
        );
        for bytes in evicted {
            frame.record_eviction(RendererCacheKind::CleanSubtree, bytes);
        }

        Ok(())
    }

    pub fn try_store_clean_subtree_payload(
        &mut self,
        frame: &mut RendererCacheFrame,
        key: CleanSubtreeContentKey,
        bytes: u64,
        payload_kind: RendererCachePayloadKind,
        image: Image,
        prepare_time: Duration,
    ) -> Result<(), CleanSubtreeStoreRejection> {
        if bytes > self.clean_subtree.max_entry_bytes {
            frame.record_rejection(
                RendererCacheKind::CleanSubtree,
                RendererCacheRejectionReason::OversizedEntry,
            );
            return Err(CleanSubtreeStoreRejection::OversizedEntry);
        }

        let visible_count = self
            .clean_subtree
            .visible_accesses
            .get(&key)
            .map(|access| access.visible_count)
            .unwrap_or(0);
        if visible_count < self.clean_subtree.min_visible_before_store {
            frame.record_rejection(
                RendererCacheKind::CleanSubtree,
                RendererCacheRejectionReason::AdmissionThreshold,
            );
            return Err(CleanSubtreeStoreRejection::AdmissionThreshold);
        }

        frame.admit_candidate(RendererCacheKind::CleanSubtree);
        if !frame.try_consume_new_payload_budget(RendererCacheKind::CleanSubtree) {
            return Err(CleanSubtreeStoreRejection::PayloadBudget);
        }

        let evicted = self.clean_subtree.try_store_payload(
            key,
            bytes,
            frame.frame_index,
            payload_kind,
            image,
        )?;
        frame.record_store(
            RendererCacheKind::CleanSubtree,
            bytes,
            payload_kind,
            prepare_time,
        );
        for bytes in evicted {
            frame.record_eviction(RendererCacheKind::CleanSubtree, bytes);
        }

        Ok(())
    }

    pub fn reserve_clean_subtree_payload_store(
        &mut self,
        frame: &mut RendererCacheFrame,
        key: CleanSubtreeContentKey,
        bytes: u64,
    ) -> Result<(), CleanSubtreeStoreRejection> {
        if bytes > self.clean_subtree.max_entry_bytes {
            frame.record_rejection(
                RendererCacheKind::CleanSubtree,
                RendererCacheRejectionReason::OversizedEntry,
            );
            return Err(CleanSubtreeStoreRejection::OversizedEntry);
        }

        let visible_count = self
            .clean_subtree
            .visible_accesses
            .get(&key)
            .map(|access| access.visible_count)
            .unwrap_or(0);
        if visible_count < self.clean_subtree.min_visible_before_store {
            frame.record_rejection(
                RendererCacheKind::CleanSubtree,
                RendererCacheRejectionReason::AdmissionThreshold,
            );
            return Err(CleanSubtreeStoreRejection::AdmissionThreshold);
        }

        frame.admit_candidate(RendererCacheKind::CleanSubtree);
        if !frame.try_consume_new_payload_budget(RendererCacheKind::CleanSubtree) {
            return Err(CleanSubtreeStoreRejection::PayloadBudget);
        }

        Ok(())
    }

    pub fn store_reserved_clean_subtree_payload(
        &mut self,
        frame: &mut RendererCacheFrame,
        key: CleanSubtreeContentKey,
        bytes: u64,
        payload_kind: RendererCachePayloadKind,
        image: Image,
        prepare_time: Duration,
    ) {
        match self.clean_subtree.try_store_payload(
            key,
            bytes,
            frame.frame_index,
            payload_kind,
            image,
        ) {
            Ok(evicted) => {
                frame.record_store(
                    RendererCacheKind::CleanSubtree,
                    bytes,
                    payload_kind,
                    prepare_time,
                );
                for bytes in evicted {
                    frame.record_eviction(RendererCacheKind::CleanSubtree, bytes);
                }
            }
            Err(rejection) => {
                frame.record_rejection(RendererCacheKind::CleanSubtree, rejection.into());
            }
        }
    }

    pub fn clean_subtree_entry_count(&self) -> u64 {
        self.clean_subtree.entry_count()
    }

    pub fn clean_subtree_total_bytes(&self) -> u64 {
        self.clean_subtree.total_bytes()
    }

    #[cfg(test)]
    fn configure_clean_subtree_limits_for_test(
        &mut self,
        max_entries: usize,
        max_bytes: u64,
        max_entry_bytes: u64,
    ) {
        self.clean_subtree.max_entries = max_entries;
        self.clean_subtree.max_bytes = max_bytes;
        self.clean_subtree.max_entry_bytes = max_entry_bytes;
    }
}

#[derive(Debug)]
pub struct RendererCacheFrame {
    generation: u64,
    frame_index: u64,
    new_payload_budget_remaining: u32,
    stats: RendererCacheFrameStats,
}

impl RendererCacheFrame {
    pub fn mark_candidate(&mut self, kind: RendererCacheKind, visible: bool) {
        let stats = self.stats.for_kind_mut(kind);
        stats.candidates = stats.candidates.saturating_add(1);
        if visible {
            stats.visible_candidates = stats.visible_candidates.saturating_add(1);
        }
    }

    pub fn admit_candidate(&mut self, kind: RendererCacheKind) {
        let stats = self.stats.for_kind_mut(kind);
        stats.admitted = stats.admitted.saturating_add(1);
    }

    pub fn record_hit(&mut self, kind: RendererCacheKind, draw_hit_time: Duration) {
        let stats = self.stats.for_kind_mut(kind);
        stats.hits = stats.hits.saturating_add(1);
        stats.draw_hit_time += draw_hit_time;
    }

    pub fn record_miss(&mut self, kind: RendererCacheKind) {
        let stats = self.stats.for_kind_mut(kind);
        stats.misses = stats.misses.saturating_add(1);
    }

    pub fn record_store(
        &mut self,
        kind: RendererCacheKind,
        bytes: u64,
        payload_kind: RendererCachePayloadKind,
        prepare_time: Duration,
    ) {
        let stats = self.stats.for_kind_mut(kind);
        stats.stores = stats.stores.saturating_add(1);
        stats.current_entries = stats.current_entries.saturating_add(1);
        stats.current_bytes = stats.current_bytes.saturating_add(bytes);
        match payload_kind {
            RendererCachePayloadKind::GpuRenderTarget => {
                stats.gpu_payload_stores = stats.gpu_payload_stores.saturating_add(1);
            }
            RendererCachePayloadKind::CpuRaster => {
                stats.cpu_payload_stores = stats.cpu_payload_stores.saturating_add(1);
            }
        }
        stats.prepare_successes = stats.prepare_successes.saturating_add(1);
        stats.prepare_time += prepare_time;
    }

    pub fn record_eviction(&mut self, kind: RendererCacheKind, bytes: u64) {
        let stats = self.stats.for_kind_mut(kind);
        stats.evictions = stats.evictions.saturating_add(1);
        stats.current_entries = stats.current_entries.saturating_sub(1);
        stats.current_bytes = stats.current_bytes.saturating_sub(bytes);
        stats.evicted_bytes = stats.evicted_bytes.saturating_add(bytes);
    }

    pub fn record_prepare_failure(&mut self, kind: RendererCacheKind) {
        let stats = self.stats.for_kind_mut(kind);
        stats.prepare_failures = stats.prepare_failures.saturating_add(1);
    }

    pub fn record_direct_fallback_after_admission(&mut self, kind: RendererCacheKind) {
        let stats = self.stats.for_kind_mut(kind);
        stats.direct_fallbacks_after_admission =
            stats.direct_fallbacks_after_admission.saturating_add(1);
    }

    pub fn record_rejection(
        &mut self,
        kind: RendererCacheKind,
        reason: RendererCacheRejectionReason,
    ) {
        let stats = self.stats.for_kind_mut(kind);
        stats.rejected = stats.rejected.saturating_add(1);
        match reason {
            RendererCacheRejectionReason::Ineligible => {
                stats.rejected_ineligible = stats.rejected_ineligible.saturating_add(1);
            }
            RendererCacheRejectionReason::AdmissionThreshold => {
                stats.rejected_admission = stats.rejected_admission.saturating_add(1);
            }
            RendererCacheRejectionReason::OversizedEntry => {
                stats.rejected_oversized = stats.rejected_oversized.saturating_add(1);
            }
            RendererCacheRejectionReason::PayloadBudget => {
                stats.rejected_payload_budget = stats.rejected_payload_budget.saturating_add(1);
            }
        }
    }

    pub fn try_consume_new_payload_budget(&mut self, kind: RendererCacheKind) -> bool {
        if self.new_payload_budget_remaining == 0 {
            self.record_rejection(kind, RendererCacheRejectionReason::PayloadBudget);
            return false;
        }

        self.new_payload_budget_remaining -= 1;
        true
    }
}

#[derive(Clone, Copy)]
struct RenderTraversalOptions<'a> {
    video_state: &'a RendererVideoState,
    image_bleed_device_outset: f32,
    solid_border_fast_paths: bool,
}

impl<'a> RenderTraversalOptions<'a> {
    fn unclipped(video_state: &'a RendererVideoState) -> Self {
        Self {
            video_state,
            image_bleed_device_outset: 0.0,
            solid_border_fast_paths: true,
        }
    }

    fn with_image_bleed_device_outset(self, image_bleed_device_outset: f32) -> Self {
        Self {
            image_bleed_device_outset,
            ..self
        }
    }

    fn with_solid_border_fast_paths(self, solid_border_fast_paths: bool) -> Self {
        Self {
            solid_border_fast_paths,
            ..self
        }
    }
}

struct RenderCacheTracking<'a> {
    renderer_cache: &'a mut RendererCacheManager,
    frame: &'a mut RendererCacheFrame,
    gpu_context: Option<&'a mut gpu::DirectContext>,
}

struct PreparedCleanSubtreePayload {
    image: Image,
    bytes: u64,
    payload_kind: RendererCachePayloadKind,
    prepare_time: Duration,
}

#[derive(Clone, Copy)]
struct CacheCandidateEligibility {
    current_transform: Affine2,
    paint_attributes_eligible: bool,
}

impl CacheCandidateEligibility {
    fn root() -> Self {
        Self {
            current_transform: Affine2::identity(),
            paint_attributes_eligible: true,
        }
    }

    fn with_transform(self, transform: Affine2) -> Self {
        Self {
            current_transform: self.current_transform.then(transform),
            ..self
        }
    }
}

pub struct SceneRenderer {
    video_state: RendererVideoState,
    renderer_cache: RendererCacheManager,
}

impl Default for SceneRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl SceneRenderer {
    pub fn new() -> Self {
        Self::with_cache_config(RendererCacheConfig::default())
    }

    pub fn with_cache_config(cache_config: RendererCacheConfig) -> Self {
        Self {
            video_state: RendererVideoState::default(),
            renderer_cache: RendererCacheManager::with_config(cache_config),
        }
    }

    pub fn sync_video_frames(
        &mut self,
        frame: &mut RenderFrame<'_>,
        registry: &Arc<crate::video::VideoRegistry>,
        ctx: Option<&crate::video::VideoImportContext>,
    ) -> Result<VideoSyncResult, String> {
        let Some(gr_context) = frame.direct_context.as_deref_mut() else {
            return Ok(VideoSyncResult::default());
        };

        let result = self.video_state.sync_pending(registry, gr_context, ctx)?;
        if result.resources_changed {
            gr_context.reset(None);
            self.renderer_cache.clear();
        }
        Ok(result)
    }

    /// Render the given state to the surface.
    pub fn render(&mut self, frame: &mut RenderFrame<'_>, state: &RenderState) -> RenderTimings {
        self.render_with_draw_profile(frame, state, false)
    }

    pub fn render_profiled(
        &mut self,
        frame: &mut RenderFrame<'_>,
        state: &RenderState,
    ) -> RenderTimings {
        self.render_with_draw_profile(frame, state, true)
    }

    fn render_with_draw_profile(
        &mut self,
        frame: &mut RenderFrame<'_>,
        state: &RenderState,
        profile_draw: bool,
    ) -> RenderTimings {
        let started_at = Instant::now();
        let draw_started_at = Instant::now();
        let canvas = frame.surface.canvas();

        // Keep no-candidate frames on the original renderer path. Routing every
        // frame through cache-tracking traversal regressed mixed_ui_scene by
        // 3.36%, so candidate tracking is only paid by candidate-bearing scenes.
        if !state.has_cache_candidates {
            let draw_detail = if profile_draw {
                let mut detail = RenderDrawTimings::default();
                let clear_started_at = Instant::now();
                canvas.clear(state.clear_color);
                detail.clear = clear_started_at.elapsed();
                Self::render_nodes_profiled(
                    canvas,
                    &state.scene.nodes,
                    &self.video_state,
                    0.0,
                    true,
                    &mut detail,
                );
                Some(detail)
            } else {
                canvas.clear(state.clear_color);
                Self::render_nodes(canvas, &state.scene.nodes, &self.video_state, 0.0, true);
                None
            };

            let draw = draw_started_at.elapsed();
            let flush = frame.flush();

            return RenderTimings {
                total: started_at.elapsed(),
                draw,
                draw_detail,
                flush: flush.total,
                gpu_flush: flush.gpu_flush,
                submit: flush.submit,
                renderer_cache: None,
            };
        }

        let (draw_detail, renderer_cache) = if profile_draw {
            let mut detail = RenderDrawTimings::default();
            let clear_started_at = Instant::now();
            canvas.clear(state.clear_color);
            detail.clear = clear_started_at.elapsed();

            let mut cache_frame = self.renderer_cache.begin_frame();
            let options = RenderTraversalOptions::unclipped(&self.video_state);
            let mut cache_tracking = RenderCacheTracking {
                renderer_cache: &mut self.renderer_cache,
                frame: &mut cache_frame,
                gpu_context: frame.direct_context.as_deref_mut(),
            };
            Self::render_nodes_with_cache_tracking(
                canvas,
                &state.scene.nodes,
                options,
                &mut cache_tracking,
                CacheCandidateEligibility::root(),
            );

            let stats = self.renderer_cache.end_frame(cache_frame);
            let renderer_cache = (!stats.is_empty()).then(|| Box::new(stats));

            (Some(detail), renderer_cache)
        } else {
            canvas.clear(state.clear_color);

            let mut cache_frame = self.renderer_cache.begin_frame();
            let options = RenderTraversalOptions::unclipped(&self.video_state);
            let mut cache_tracking = RenderCacheTracking {
                renderer_cache: &mut self.renderer_cache,
                frame: &mut cache_frame,
                gpu_context: frame.direct_context.as_deref_mut(),
            };
            Self::render_nodes_with_cache_tracking(
                canvas,
                &state.scene.nodes,
                options,
                &mut cache_tracking,
                CacheCandidateEligibility::root(),
            );
            let stats = self.renderer_cache.end_frame(cache_frame);
            let renderer_cache = (!stats.is_empty()).then(|| Box::new(stats));

            (None, renderer_cache)
        };

        let draw = draw_started_at.elapsed();
        let flush = frame.flush();

        RenderTimings {
            total: started_at.elapsed(),
            draw,
            draw_detail,
            flush: flush.total,
            gpu_flush: flush.gpu_flush,
            submit: flush.submit,
            renderer_cache,
        }
    }

    fn render_nodes_with_cache_tracking(
        canvas: &skia_safe::Canvas,
        nodes: &[RenderNode],
        options: RenderTraversalOptions<'_>,
        cache_tracking: &mut RenderCacheTracking<'_>,
        eligibility: CacheCandidateEligibility,
    ) {
        for node in nodes {
            match node {
                RenderNode::ShadowPass { children } => Self::render_nodes_with_cache_tracking(
                    canvas,
                    children,
                    options,
                    cache_tracking,
                    eligibility,
                ),
                RenderNode::Clip { clips, children } => Self::render_clip_node_with_cache_tracking(
                    canvas,
                    clips,
                    children,
                    options,
                    cache_tracking,
                    eligibility,
                ),
                RenderNode::RelaxedClip { clips, children } => {
                    Self::render_relaxed_clip_node_with_cache_tracking(
                        canvas,
                        clips,
                        children,
                        options,
                        cache_tracking,
                        eligibility,
                    )
                }
                RenderNode::Transform {
                    transform,
                    children,
                } => Self::render_transform_node_with_cache_tracking(
                    canvas,
                    *transform,
                    children,
                    options,
                    cache_tracking,
                    eligibility,
                ),
                RenderNode::Alpha { alpha, children } => {
                    Self::render_alpha_node_with_cache_tracking(
                        canvas,
                        *alpha,
                        children,
                        options,
                        cache_tracking,
                        eligibility,
                    )
                }
                RenderNode::CacheCandidate(candidate) => {
                    Self::render_clean_subtree_cache_candidate(
                        canvas,
                        candidate,
                        options,
                        cache_tracking,
                        eligibility,
                    );
                }
                RenderNode::Primitive(primitive) => Self::render_primitive(
                    canvas,
                    primitive,
                    options.video_state,
                    options.image_bleed_device_outset,
                    options.solid_border_fast_paths,
                ),
            }
        }
    }

    fn render_clean_subtree_cache_candidate(
        canvas: &skia_safe::Canvas,
        candidate: &RenderCacheCandidate,
        options: RenderTraversalOptions<'_>,
        cache_tracking: &mut RenderCacheTracking<'_>,
        eligibility: CacheCandidateEligibility,
    ) {
        match candidate.kind {
            RenderCacheCandidateKind::CleanSubtree => {
                let resource_generation = clean_subtree_resource_generation(&candidate.children);
                let key = CleanSubtreeContentKey::from_candidate(
                    candidate,
                    1.0,
                    resource_generation.unwrap_or_default(),
                );
                // Keep the first production cache to integer translation plus
                // root alpha composition. Rotate/scale stay in direct fallback
                // until their sampling behavior has parity coverage.
                let placement = clean_subtree_placement(candidate, eligibility.current_transform);

                if !eligibility.paint_attributes_eligible
                    || !clean_subtree_children_are_cacheable(&candidate.children)
                    || resource_generation.is_none()
                    || key.is_none()
                    || placement.is_err()
                {
                    cache_tracking
                        .frame
                        .mark_candidate(RendererCacheKind::CleanSubtree, true);
                    cache_tracking.frame.record_rejection(
                        RendererCacheKind::CleanSubtree,
                        RendererCacheRejectionReason::Ineligible,
                    );
                    Self::render_nodes_with_cache_tracking(
                        canvas,
                        &candidate.children,
                        options,
                        cache_tracking,
                        eligibility,
                    );
                    return;
                }

                let key = key.expect("clean-subtree key checked above");
                let visible_count = cache_tracking
                    .renderer_cache
                    .mark_clean_subtree_visible(cache_tracking.frame, key);

                if let Some(image) = cache_tracking
                    .renderer_cache
                    .clean_subtree_payload(cache_tracking.frame, key)
                {
                    let hit_started_at = Instant::now();
                    canvas.draw_image(&image, (candidate.bounds.x, candidate.bounds.y), None);
                    cache_tracking
                        .frame
                        .record_hit(RendererCacheKind::CleanSubtree, hit_started_at.elapsed());
                    return;
                }

                cache_tracking
                    .frame
                    .record_miss(RendererCacheKind::CleanSubtree);

                if !cache_tracking
                    .renderer_cache
                    .clean_subtree_visible_count_allows_store(visible_count)
                {
                    Self::render_nodes_with_cache_tracking(
                        canvas,
                        &candidate.children,
                        options,
                        cache_tracking,
                        eligibility,
                    );
                    return;
                }

                let Some(bytes) = key.byte_len() else {
                    cache_tracking.frame.record_rejection(
                        RendererCacheKind::CleanSubtree,
                        RendererCacheRejectionReason::OversizedEntry,
                    );
                    Self::render_nodes_with_cache_tracking(
                        canvas,
                        &candidate.children,
                        options,
                        cache_tracking,
                        eligibility,
                    );
                    return;
                };

                match cache_tracking
                    .renderer_cache
                    .reserve_clean_subtree_payload_store(cache_tracking.frame, key, bytes)
                {
                    Ok(()) => {
                        let prepared = if let Some(gr_context) = cache_tracking.gpu_context.as_mut()
                        {
                            Self::prepare_clean_subtree_payload(
                                candidate,
                                options,
                                Some(&mut **gr_context),
                            )
                        } else {
                            Self::prepare_clean_subtree_payload(candidate, options, None)
                        };

                        if let Some(prepared) = prepared {
                            canvas.draw_image(
                                &prepared.image,
                                (candidate.bounds.x, candidate.bounds.y),
                                None,
                            );
                            cache_tracking
                                .renderer_cache
                                .store_reserved_clean_subtree_payload(
                                    cache_tracking.frame,
                                    key,
                                    prepared.bytes,
                                    prepared.payload_kind,
                                    prepared.image,
                                    prepared.prepare_time,
                                );
                            return;
                        }

                        cache_tracking
                            .frame
                            .record_prepare_failure(RendererCacheKind::CleanSubtree);
                        cache_tracking.frame.record_direct_fallback_after_admission(
                            RendererCacheKind::CleanSubtree,
                        );
                    }
                    Err(CleanSubtreeStoreRejection::PayloadBudget) => {
                        cache_tracking.frame.record_direct_fallback_after_admission(
                            RendererCacheKind::CleanSubtree,
                        );
                    }
                    Err(_) => {}
                }

                Self::render_nodes_with_cache_tracking(
                    canvas,
                    &candidate.children,
                    options,
                    cache_tracking,
                    eligibility,
                );
            }
        }
    }

    fn prepare_clean_subtree_payload(
        candidate: &RenderCacheCandidate,
        options: RenderTraversalOptions<'_>,
        gpu_context: Option<&mut gpu::DirectContext>,
    ) -> Option<PreparedCleanSubtreePayload> {
        if let Some(gr_context) = gpu_context {
            return Self::prepare_clean_subtree_gpu_payload(candidate, options, gr_context);
        }

        Self::rasterize_clean_subtree_payload(candidate, options)
    }

    fn prepare_clean_subtree_gpu_payload(
        candidate: &RenderCacheCandidate,
        options: RenderTraversalOptions<'_>,
        gr_context: &mut gpu::DirectContext,
    ) -> Option<PreparedCleanSubtreePayload> {
        let (width_px, height_px, bytes) = clean_subtree_bounds_size(candidate.bounds)?;
        let info = skia_safe::ImageInfo::new(
            (width_px as i32, height_px as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut surface = gpu::surfaces::render_target(
            gr_context,
            gpu::Budgeted::Yes,
            &info,
            0,
            gpu::SurfaceOrigin::TopLeft,
            None,
            false,
            false,
        )?;
        let started_at = Instant::now();
        let canvas = surface.canvas();
        canvas.clear(Color::TRANSPARENT);
        canvas.save();
        canvas.translate((-candidate.bounds.x, -candidate.bounds.y));
        Self::render_nodes(
            canvas,
            &candidate.children,
            options.video_state,
            options.image_bleed_device_outset,
            options.solid_border_fast_paths,
        );
        canvas.restore();
        let image = surface.image_snapshot();

        Some(PreparedCleanSubtreePayload {
            image,
            bytes,
            payload_kind: RendererCachePayloadKind::GpuRenderTarget,
            prepare_time: started_at.elapsed(),
        })
    }

    fn rasterize_clean_subtree_payload(
        candidate: &RenderCacheCandidate,
        options: RenderTraversalOptions<'_>,
    ) -> Option<PreparedCleanSubtreePayload> {
        let (width_px, height_px, bytes) = clean_subtree_bounds_size(candidate.bounds)?;
        let info = skia_safe::ImageInfo::new(
            (width_px as i32, height_px as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut surface = skia_safe::surfaces::raster(&info, None, None)?;
        let started_at = Instant::now();
        let canvas = surface.canvas();
        canvas.clear(Color::TRANSPARENT);
        canvas.save();
        canvas.translate((-candidate.bounds.x, -candidate.bounds.y));
        Self::render_nodes(
            canvas,
            &candidate.children,
            options.video_state,
            options.image_bleed_device_outset,
            options.solid_border_fast_paths,
        );
        canvas.restore();
        let image = surface.image_snapshot();

        Some(PreparedCleanSubtreePayload {
            image,
            bytes,
            payload_kind: RendererCachePayloadKind::CpuRaster,
            prepare_time: started_at.elapsed(),
        })
    }

    fn render_nodes(
        canvas: &skia_safe::Canvas,
        nodes: &[RenderNode],
        video_state: &RendererVideoState,
        image_bleed_device_outset: f32,
        solid_border_fast_paths: bool,
    ) {
        for node in nodes {
            match node {
                RenderNode::ShadowPass { children } => Self::render_nodes(
                    canvas,
                    children,
                    video_state,
                    image_bleed_device_outset,
                    solid_border_fast_paths,
                ),
                RenderNode::Clip { clips, children } => Self::render_clip_node(
                    canvas,
                    clips,
                    children,
                    video_state,
                    image_bleed_device_outset,
                    solid_border_fast_paths,
                ),
                RenderNode::RelaxedClip { clips, children } => Self::render_relaxed_clip_node(
                    canvas,
                    clips,
                    children,
                    video_state,
                    solid_border_fast_paths,
                ),
                RenderNode::Transform {
                    transform,
                    children,
                } => Self::render_transform_node(
                    canvas,
                    *transform,
                    children,
                    video_state,
                    image_bleed_device_outset,
                    solid_border_fast_paths,
                ),
                RenderNode::Alpha { alpha, children } => Self::render_alpha_node(
                    canvas,
                    *alpha,
                    children,
                    video_state,
                    image_bleed_device_outset,
                    solid_border_fast_paths,
                ),
                RenderNode::CacheCandidate(candidate) => Self::render_nodes(
                    canvas,
                    &candidate.children,
                    video_state,
                    image_bleed_device_outset,
                    solid_border_fast_paths,
                ),
                RenderNode::Primitive(primitive) => Self::render_primitive(
                    canvas,
                    primitive,
                    video_state,
                    image_bleed_device_outset,
                    solid_border_fast_paths,
                ),
            }
        }
    }

    fn render_nodes_profiled(
        canvas: &skia_safe::Canvas,
        nodes: &[RenderNode],
        video_state: &RendererVideoState,
        image_bleed_device_outset: f32,
        solid_border_fast_paths: bool,
        draw_detail: &mut RenderDrawTimings,
    ) {
        for node in nodes {
            match node {
                RenderNode::ShadowPass { children } => Self::render_nodes_profiled(
                    canvas,
                    children,
                    video_state,
                    image_bleed_device_outset,
                    solid_border_fast_paths,
                    draw_detail,
                ),
                RenderNode::Clip { clips, children } => Self::render_clip_node_profiled(
                    canvas,
                    clips,
                    children,
                    video_state,
                    image_bleed_device_outset,
                    solid_border_fast_paths,
                    draw_detail,
                ),
                RenderNode::RelaxedClip { clips, children } => {
                    Self::render_relaxed_clip_node_profiled(
                        canvas,
                        clips,
                        children,
                        video_state,
                        solid_border_fast_paths,
                        draw_detail,
                    )
                }
                RenderNode::Transform {
                    transform,
                    children,
                } => Self::render_transform_node_profiled(
                    canvas,
                    *transform,
                    children,
                    video_state,
                    image_bleed_device_outset,
                    solid_border_fast_paths,
                    draw_detail,
                ),
                RenderNode::Alpha { alpha, children } => Self::render_alpha_node_profiled(
                    canvas,
                    *alpha,
                    children,
                    video_state,
                    image_bleed_device_outset,
                    solid_border_fast_paths,
                    draw_detail,
                ),
                RenderNode::CacheCandidate(candidate) => Self::render_nodes_profiled(
                    canvas,
                    &candidate.children,
                    video_state,
                    image_bleed_device_outset,
                    solid_border_fast_paths,
                    draw_detail,
                ),
                RenderNode::Primitive(primitive) => match primitive {
                    DrawPrimitive::Shadow(
                        x,
                        y,
                        w,
                        h,
                        offset_x,
                        offset_y,
                        blur,
                        size,
                        radius,
                        color,
                    ) => {
                        let profile = draw_outer_shadow_profiled(
                            canvas,
                            ShadowDrawSpec {
                                rect: RectSpec {
                                    x: *x,
                                    y: *y,
                                    w: *w,
                                    h: *h,
                                },
                                offset_x: *offset_x,
                                offset_y: *offset_y,
                                blur: *blur,
                                size: *size,
                                radius: *radius,
                                color: *color,
                            },
                        );
                        draw_detail.shadows += profile.total;
                        draw_detail.shadow_details.push(profile);
                    }
                    DrawPrimitive::Image(x, y, w, h, image_id, fit, svg_tint) => {
                        let profile = draw_cached_asset_with_fit_profiled(
                            canvas,
                            ImageDrawSpec {
                                rect: RectSpec {
                                    x: *x,
                                    y: *y,
                                    w: *w,
                                    h: *h,
                                },
                                image_id,
                                fit: *fit,
                                svg_tint: *svg_tint,
                            },
                            image_bleed_device_outset,
                        );
                        draw_detail.images += profile.total;
                        if profile.tint_layer_used {
                            draw_detail
                                .layer_detail
                                .record_tinted_image_layer(profile.draw_width, profile.draw_height);
                        }
                        draw_detail.image_details.push(profile);
                    }
                    _ => {
                        let started_at = Instant::now();
                        Self::render_primitive(
                            canvas,
                            primitive,
                            video_state,
                            image_bleed_device_outset,
                            solid_border_fast_paths,
                        );
                        draw_detail.record_primitive(primitive, started_at.elapsed());
                    }
                },
            }
        }
    }

    #[doc(hidden)]
    pub fn render_nodes_for_cache_candidate_benchmark(
        canvas: &skia_safe::Canvas,
        nodes: &[RenderNode],
    ) {
        Self::render_nodes(canvas, nodes, &RendererVideoState::default(), 0.0, true);
    }

    fn render_clip_node_profiled(
        canvas: &skia_safe::Canvas,
        clips: &[ClipShape],
        children: &[RenderNode],
        video_state: &RendererVideoState,
        image_bleed_device_outset: f32,
        solid_border_fast_paths: bool,
        draw_detail: &mut RenderDrawTimings,
    ) {
        if children.is_empty() {
            return;
        }

        draw_detail.clip_detail.record_clip_scope(false, clips);

        if clips.is_empty() {
            Self::render_nodes_profiled(
                canvas,
                children,
                video_state,
                image_bleed_device_outset,
                solid_border_fast_paths,
                draw_detail,
            );
            return;
        }

        let clip_started_at = Instant::now();
        canvas.save();
        for clip in clips {
            apply_clip_shape(canvas, clip);
        }
        draw_detail.clips += clip_started_at.elapsed();

        for child in children {
            match child {
                RenderNode::ShadowPass { children } => {
                    draw_detail.clip_detail.record_shadow_escape_reapplication();

                    let clip_started_at = Instant::now();
                    canvas.restore();
                    draw_detail.clips += clip_started_at.elapsed();

                    Self::render_nodes_profiled(
                        canvas,
                        children,
                        video_state,
                        image_bleed_device_outset,
                        solid_border_fast_paths,
                        draw_detail,
                    );

                    let clip_started_at = Instant::now();
                    canvas.save();
                    for clip in clips {
                        apply_clip_shape(canvas, clip);
                    }
                    draw_detail.clips += clip_started_at.elapsed();
                }
                _ => {
                    Self::render_nodes_profiled(
                        canvas,
                        std::slice::from_ref(child),
                        video_state,
                        image_bleed_device_outset,
                        false,
                        draw_detail,
                    );
                }
            }
        }

        let clip_started_at = Instant::now();
        canvas.restore();
        draw_detail.clips += clip_started_at.elapsed();
    }

    fn render_relaxed_clip_node_profiled(
        canvas: &skia_safe::Canvas,
        clips: &[ClipShape],
        children: &[RenderNode],
        video_state: &RendererVideoState,
        solid_border_fast_paths: bool,
        draw_detail: &mut RenderDrawTimings,
    ) {
        if children.is_empty() {
            return;
        }

        draw_detail.clip_detail.record_clip_scope(true, clips);

        if clips.is_empty() {
            Self::render_nodes_profiled(
                canvas,
                children,
                video_state,
                RELAXED_IMAGE_DRAW_BLEED_DEVICE_OUTSET,
                solid_border_fast_paths,
                draw_detail,
            );
            return;
        }

        let clip_started_at = Instant::now();
        canvas.save();
        for clip in clips {
            apply_relaxed_clip_shape(canvas, clip);
        }
        draw_detail.relaxed_clips += clip_started_at.elapsed();

        for child in children {
            match child {
                RenderNode::ShadowPass { children } => {
                    draw_detail.clip_detail.record_shadow_escape_reapplication();

                    let clip_started_at = Instant::now();
                    canvas.restore();
                    draw_detail.relaxed_clips += clip_started_at.elapsed();

                    Self::render_nodes_profiled(
                        canvas,
                        children,
                        video_state,
                        RELAXED_IMAGE_DRAW_BLEED_DEVICE_OUTSET,
                        solid_border_fast_paths,
                        draw_detail,
                    );

                    let clip_started_at = Instant::now();
                    canvas.save();
                    for clip in clips {
                        apply_relaxed_clip_shape(canvas, clip);
                    }
                    draw_detail.relaxed_clips += clip_started_at.elapsed();
                }
                _ => {
                    Self::render_nodes_profiled(
                        canvas,
                        std::slice::from_ref(child),
                        video_state,
                        RELAXED_IMAGE_DRAW_BLEED_DEVICE_OUTSET,
                        false,
                        draw_detail,
                    );
                }
            }
        }

        let clip_started_at = Instant::now();
        canvas.restore();
        draw_detail.relaxed_clips += clip_started_at.elapsed();
    }

    fn render_transform_node_profiled(
        canvas: &skia_safe::Canvas,
        transform: Affine2,
        children: &[RenderNode],
        video_state: &RendererVideoState,
        image_bleed_device_outset: f32,
        solid_border_fast_paths: bool,
        draw_detail: &mut RenderDrawTimings,
    ) {
        if children.is_empty() {
            return;
        }

        if transform.is_identity() {
            Self::render_nodes_profiled(
                canvas,
                children,
                video_state,
                image_bleed_device_outset,
                solid_border_fast_paths,
                draw_detail,
            );
            return;
        }

        let transform_started_at = Instant::now();
        canvas.save();
        let matrix = matrix_from_affine2(transform);
        canvas.concat(&matrix);
        draw_detail.transforms += transform_started_at.elapsed();

        Self::render_nodes_profiled(
            canvas,
            children,
            video_state,
            image_bleed_device_outset,
            solid_border_fast_paths,
            draw_detail,
        );

        let transform_started_at = Instant::now();
        canvas.restore();
        draw_detail.transforms += transform_started_at.elapsed();
    }

    fn render_alpha_node_profiled(
        canvas: &skia_safe::Canvas,
        alpha: f32,
        children: &[RenderNode],
        video_state: &RendererVideoState,
        image_bleed_device_outset: f32,
        solid_border_fast_paths: bool,
        draw_detail: &mut RenderDrawTimings,
    ) {
        if children.is_empty() {
            return;
        }

        if alpha >= 1.0 {
            Self::render_nodes_profiled(
                canvas,
                children,
                video_state,
                image_bleed_device_outset,
                solid_border_fast_paths,
                draw_detail,
            );
            return;
        }

        let clamped = alpha.clamp(0.0, 1.0);
        let alpha_u8 = (clamped * 255.0).round() as u8;

        if let [RenderNode::Primitive(primitive)] = children
            && {
                let started_at = Instant::now();
                let rendered = Self::render_primitive_with_alpha(canvas, primitive, clamped);
                if rendered {
                    draw_detail.record_primitive(primitive, started_at.elapsed());
                }
                rendered
            }
        {
            return;
        }

        let alpha_started_at = Instant::now();
        draw_detail.layer_detail.record_alpha_layer(children.len());
        canvas.save_layer_alpha(None, alpha_u8.into());
        draw_detail.alphas += alpha_started_at.elapsed();

        Self::render_nodes_profiled(
            canvas,
            children,
            video_state,
            image_bleed_device_outset,
            solid_border_fast_paths,
            draw_detail,
        );

        let alpha_started_at = Instant::now();
        canvas.restore();
        draw_detail.alphas += alpha_started_at.elapsed();
    }

    fn render_clip_node_with_cache_tracking(
        canvas: &skia_safe::Canvas,
        clips: &[ClipShape],
        children: &[RenderNode],
        options: RenderTraversalOptions<'_>,
        cache_tracking: &mut RenderCacheTracking<'_>,
        eligibility: CacheCandidateEligibility,
    ) {
        if children.is_empty() {
            return;
        }

        if clips.is_empty() {
            Self::render_nodes_with_cache_tracking(
                canvas,
                children,
                options,
                cache_tracking,
                eligibility,
            );
            return;
        }

        canvas.save();
        for clip in clips {
            apply_clip_shape(canvas, clip);
        }
        for child in children {
            match child {
                RenderNode::ShadowPass { children } => {
                    canvas.restore();
                    Self::render_nodes_with_cache_tracking(
                        canvas,
                        children,
                        options,
                        cache_tracking,
                        eligibility,
                    );
                    canvas.save();
                    for clip in clips {
                        apply_clip_shape(canvas, clip);
                    }
                }
                _ => {
                    Self::render_nodes_with_cache_tracking(
                        canvas,
                        std::slice::from_ref(child),
                        options.with_solid_border_fast_paths(false),
                        cache_tracking,
                        eligibility,
                    );
                }
            }
        }
        canvas.restore();
    }

    fn render_relaxed_clip_node_with_cache_tracking(
        canvas: &skia_safe::Canvas,
        clips: &[ClipShape],
        children: &[RenderNode],
        options: RenderTraversalOptions<'_>,
        cache_tracking: &mut RenderCacheTracking<'_>,
        eligibility: CacheCandidateEligibility,
    ) {
        if children.is_empty() {
            return;
        }

        let relaxed_options =
            options.with_image_bleed_device_outset(RELAXED_IMAGE_DRAW_BLEED_DEVICE_OUTSET);
        if clips.is_empty() {
            Self::render_nodes_with_cache_tracking(
                canvas,
                children,
                relaxed_options,
                cache_tracking,
                eligibility,
            );
            return;
        }

        canvas.save();
        for clip in clips {
            apply_relaxed_clip_shape(canvas, clip);
        }
        for child in children {
            match child {
                RenderNode::ShadowPass { children } => {
                    canvas.restore();
                    Self::render_nodes_with_cache_tracking(
                        canvas,
                        children,
                        relaxed_options,
                        cache_tracking,
                        eligibility,
                    );
                    canvas.save();
                    for clip in clips {
                        apply_relaxed_clip_shape(canvas, clip);
                    }
                }
                _ => {
                    Self::render_nodes_with_cache_tracking(
                        canvas,
                        std::slice::from_ref(child),
                        relaxed_options.with_solid_border_fast_paths(false),
                        cache_tracking,
                        eligibility,
                    );
                }
            }
        }
        canvas.restore();
    }

    fn render_transform_node_with_cache_tracking(
        canvas: &skia_safe::Canvas,
        transform: Affine2,
        children: &[RenderNode],
        options: RenderTraversalOptions<'_>,
        cache_tracking: &mut RenderCacheTracking<'_>,
        eligibility: CacheCandidateEligibility,
    ) {
        if children.is_empty() {
            return;
        }

        let next_eligibility = eligibility.with_transform(transform);
        if transform.is_identity() {
            Self::render_nodes_with_cache_tracking(
                canvas,
                children,
                options,
                cache_tracking,
                next_eligibility,
            );
            return;
        }

        canvas.save();
        let matrix = matrix_from_affine2(transform);
        canvas.concat(&matrix);
        Self::render_nodes_with_cache_tracking(
            canvas,
            children,
            options,
            cache_tracking,
            next_eligibility,
        );
        canvas.restore();
    }

    fn render_alpha_node_with_cache_tracking(
        canvas: &skia_safe::Canvas,
        alpha: f32,
        children: &[RenderNode],
        options: RenderTraversalOptions<'_>,
        cache_tracking: &mut RenderCacheTracking<'_>,
        eligibility: CacheCandidateEligibility,
    ) {
        if children.is_empty() {
            return;
        }

        if alpha >= 1.0 {
            Self::render_nodes_with_cache_tracking(
                canvas,
                children,
                options,
                cache_tracking,
                eligibility,
            );
            return;
        }

        let clamped = alpha.clamp(0.0, 1.0);
        let alpha_u8 = (clamped * 255.0).round() as u8;
        if let [RenderNode::Primitive(primitive)] = children
            && Self::render_primitive_with_alpha(canvas, primitive, clamped)
        {
            return;
        }

        canvas.save_layer_alpha(None, alpha_u8.into());
        Self::render_nodes_with_cache_tracking(
            canvas,
            children,
            options,
            cache_tracking,
            eligibility,
        );
        canvas.restore();
    }

    fn render_clip_node(
        canvas: &skia_safe::Canvas,
        clips: &[ClipShape],
        children: &[RenderNode],
        video_state: &RendererVideoState,
        image_bleed_device_outset: f32,
        solid_border_fast_paths: bool,
    ) {
        if children.is_empty() {
            return;
        }

        if clips.is_empty() {
            Self::render_nodes(
                canvas,
                children,
                video_state,
                image_bleed_device_outset,
                solid_border_fast_paths,
            );
            return;
        }

        canvas.save();
        for clip in clips {
            apply_clip_shape(canvas, clip);
        }
        for child in children {
            match child {
                RenderNode::ShadowPass { children } => {
                    canvas.restore();
                    Self::render_nodes(
                        canvas,
                        children,
                        video_state,
                        image_bleed_device_outset,
                        solid_border_fast_paths,
                    );
                    canvas.save();
                    for clip in clips {
                        apply_clip_shape(canvas, clip);
                    }
                }
                _ => {
                    // Solid border fast paths stay disabled inside active clips. The
                    // unclipped `draw_drrect` path wins, but `border_clip_heavy`
                    // did not prove a clipped fast-path win against the simpler
                    // path, so keep the conservative rendering here until a
                    // benchmark says otherwise.
                    Self::render_nodes(
                        canvas,
                        std::slice::from_ref(child),
                        video_state,
                        image_bleed_device_outset,
                        false,
                    );
                }
            }
        }
        canvas.restore();
    }

    fn render_relaxed_clip_node(
        canvas: &skia_safe::Canvas,
        clips: &[ClipShape],
        children: &[RenderNode],
        video_state: &RendererVideoState,
        solid_border_fast_paths: bool,
    ) {
        if children.is_empty() {
            return;
        }

        if clips.is_empty() {
            Self::render_nodes(
                canvas,
                children,
                video_state,
                RELAXED_IMAGE_DRAW_BLEED_DEVICE_OUTSET,
                solid_border_fast_paths,
            );
            return;
        }

        canvas.save();
        for clip in clips {
            apply_relaxed_clip_shape(canvas, clip);
        }
        for child in children {
            match child {
                RenderNode::ShadowPass { children } => {
                    canvas.restore();
                    Self::render_nodes(
                        canvas,
                        children,
                        video_state,
                        RELAXED_IMAGE_DRAW_BLEED_DEVICE_OUTSET,
                        solid_border_fast_paths,
                    );
                    canvas.save();
                    for clip in clips {
                        apply_relaxed_clip_shape(canvas, clip);
                    }
                }
                _ => {
                    // See the regular clip path above: clipped solid-border fast
                    // paths are intentionally not enabled without a measured win.
                    Self::render_nodes(
                        canvas,
                        std::slice::from_ref(child),
                        video_state,
                        RELAXED_IMAGE_DRAW_BLEED_DEVICE_OUTSET,
                        false,
                    );
                }
            }
        }
        canvas.restore();
    }

    fn render_transform_node(
        canvas: &skia_safe::Canvas,
        transform: Affine2,
        children: &[RenderNode],
        video_state: &RendererVideoState,
        image_bleed_device_outset: f32,
        solid_border_fast_paths: bool,
    ) {
        if children.is_empty() {
            return;
        }

        if transform.is_identity() {
            Self::render_nodes(
                canvas,
                children,
                video_state,
                image_bleed_device_outset,
                solid_border_fast_paths,
            );
            return;
        }

        canvas.save();
        let matrix = matrix_from_affine2(transform);
        canvas.concat(&matrix);
        Self::render_nodes(
            canvas,
            children,
            video_state,
            image_bleed_device_outset,
            solid_border_fast_paths,
        );
        canvas.restore();
    }

    fn render_alpha_node(
        canvas: &skia_safe::Canvas,
        alpha: f32,
        children: &[RenderNode],
        video_state: &RendererVideoState,
        image_bleed_device_outset: f32,
        solid_border_fast_paths: bool,
    ) {
        if children.is_empty() {
            return;
        }

        if alpha >= 1.0 {
            Self::render_nodes(
                canvas,
                children,
                video_state,
                image_bleed_device_outset,
                solid_border_fast_paths,
            );
            return;
        }

        let clamped = alpha.clamp(0.0, 1.0);
        let alpha_u8 = (clamped * 255.0).round() as u8;
        if let [RenderNode::Primitive(primitive)] = children
            && Self::render_primitive_with_alpha(canvas, primitive, clamped)
        {
            return;
        }

        canvas.save_layer_alpha(None, alpha_u8.into());
        Self::render_nodes(
            canvas,
            children,
            video_state,
            image_bleed_device_outset,
            solid_border_fast_paths,
        );
        canvas.restore();
    }

    fn render_primitive(
        canvas: &skia_safe::Canvas,
        primitive: &DrawPrimitive,
        video_state: &RendererVideoState,
        image_bleed_device_outset: f32,
        solid_border_fast_paths: bool,
    ) {
        match primitive {
            DrawPrimitive::Rect(x, y, w, h, fill) => {
                let rect = Rect::from_xywh(*x, *y, *w, *h);
                let mut paint = Paint::default();
                paint.set_color(color_from_u32(*fill));
                paint.set_anti_alias(true);
                canvas.draw_rect(rect, &paint);
            }

            DrawPrimitive::RoundedRect(x, y, w, h, radius, fill) => {
                let rect = Rect::from_xywh(*x, *y, *w, *h);
                let rrect = corner_rrect(rect, [*radius; 4]);
                let mut paint = Paint::default();
                paint.set_color(color_from_u32(*fill));
                paint.set_anti_alias(true);
                canvas.draw_rrect(rrect, &paint);
            }

            DrawPrimitive::Border(x, y, w, h, radius, width, color, style) => {
                draw_border_with_fast_path(
                    canvas,
                    BorderDrawSpec {
                        rect: RectSpec {
                            x: *x,
                            y: *y,
                            w: *w,
                            h: *h,
                        },
                        corners: [*radius, *radius, *radius, *radius],
                        insets: EdgeInsets::uniform(*width),
                        color: *color,
                        style: *style,
                    },
                    solid_border_fast_paths,
                );
            }

            DrawPrimitive::BorderCorners(x, y, w, h, tl, tr, br, bl, width, color, style) => {
                draw_border_with_fast_path(
                    canvas,
                    BorderDrawSpec {
                        rect: RectSpec {
                            x: *x,
                            y: *y,
                            w: *w,
                            h: *h,
                        },
                        corners: [*tl, *tr, *br, *bl],
                        insets: EdgeInsets::uniform(*width),
                        color: *color,
                        style: *style,
                    },
                    solid_border_fast_paths,
                );
            }

            DrawPrimitive::BorderEdges(
                x,
                y,
                w,
                h,
                radius,
                top,
                right,
                bottom,
                left,
                color,
                style,
            ) => {
                draw_border_with_fast_path(
                    canvas,
                    BorderDrawSpec {
                        rect: RectSpec {
                            x: *x,
                            y: *y,
                            w: *w,
                            h: *h,
                        },
                        corners: [*radius, *radius, *radius, *radius],
                        insets: EdgeInsets {
                            top: *top,
                            right: *right,
                            bottom: *bottom,
                            left: *left,
                        },
                        color: *color,
                        style: *style,
                    },
                    solid_border_fast_paths,
                );
            }

            DrawPrimitive::Shadow(x, y, w, h, offset_x, offset_y, blur, size, radius, color) => {
                draw_outer_shadow(
                    canvas,
                    ShadowDrawSpec {
                        rect: RectSpec {
                            x: *x,
                            y: *y,
                            w: *w,
                            h: *h,
                        },
                        offset_x: *offset_x,
                        offset_y: *offset_y,
                        blur: *blur,
                        size: *size,
                        radius: *radius,
                        color: *color,
                    },
                );
            }

            DrawPrimitive::InsetShadow(
                x,
                y,
                w,
                h,
                offset_x,
                offset_y,
                blur,
                size,
                radius,
                color,
            ) => {
                let bounds_rect = Rect::from_xywh(*x, *y, *w, *h);
                let bounds_rrect = if *radius > 0.0 {
                    RRect::new_rect_xy(bounds_rect, *radius, *radius)
                } else {
                    RRect::new_rect(bounds_rect)
                };

                canvas.save();
                canvas.clip_rrect(bounds_rrect, skia_safe::ClipOp::Intersect, true);

                let inset_x = *x + *offset_x + *size;
                let inset_y = *y + *offset_y + *size;
                let inset_w = *w - *size * 2.0;
                let inset_h = *h - *size * 2.0;
                let inset_radius = (*radius - *size).max(0.0);

                let inner_rect =
                    Rect::from_xywh(inset_x, inset_y, inset_w.max(0.0), inset_h.max(0.0));
                let inner_rrect = if inset_radius > 0.0 {
                    RRect::new_rect_xy(inner_rect, inset_radius, inset_radius)
                } else {
                    RRect::new_rect(inner_rect)
                };

                let margin = (*blur + *size) * 4.0 + 100.0;
                let outer_rect = Rect::from_xywh(
                    *x - margin,
                    *y - margin,
                    *w + margin * 2.0,
                    *h + margin * 2.0,
                );
                let mut builder = PathBuilder::new_with_fill_type(PathFillType::EvenOdd);
                builder.add_rect(outer_rect, None, None);
                builder.add_rrect(inner_rrect, None, None);
                let path = builder.detach();

                let mut paint = Paint::default();
                paint.set_color(color_from_u32(*color));
                paint.set_anti_alias(true);

                if *blur > 0.0 {
                    let sigma = *blur / 2.0;
                    if let Some(filter) = MaskFilter::blur(BlurStyle::Normal, sigma, false) {
                        paint.set_mask_filter(filter);
                    }
                }

                canvas.draw_path(&path, &paint);
                canvas.restore();
            }

            DrawPrimitive::TextWithFont(x, y, text, font_size, fill, family, weight, italic) => {
                let font = make_font_with_style(family, *weight, *italic, *font_size);
                let mut paint = Paint::default();
                paint.set_color(color_from_u32(*fill));
                paint.set_anti_alias(true);
                canvas.draw_str(text, (*x, *y), &font, &paint);
            }

            DrawPrimitive::Gradient(x, y, w, h, from, to, angle) => {
                let rect = Rect::from_xywh(*x, *y, *w, *h);

                let radians = angle.to_radians();
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                let half_diag = (w * w + h * h).sqrt() / 2.0;

                let start = (
                    cx - radians.cos() * half_diag,
                    cy - radians.sin() * half_diag,
                );
                let end = (
                    cx + radians.cos() * half_diag,
                    cy + radians.sin() * half_diag,
                );

                let colors = [color_from_u32(*from).into(), color_from_u32(*to).into()];
                let gradient_colors =
                    GradientColors::new_evenly_spaced(&colors, TileMode::Clamp, None);
                let gradient = Gradient::new(gradient_colors, Interpolation::default());

                if let Some(shader) = shaders::linear_gradient((start, end), &gradient, None) {
                    let mut paint = Paint::default();
                    paint.set_shader(shader);
                    paint.set_anti_alias(true);
                    canvas.draw_rect(rect, &paint);
                }
            }

            DrawPrimitive::Image(x, y, w, h, image_id, fit, svg_tint) => {
                draw_cached_asset_with_fit(
                    canvas,
                    ImageDrawSpec {
                        rect: RectSpec {
                            x: *x,
                            y: *y,
                            w: *w,
                            h: *h,
                        },
                        image_id,
                        fit: *fit,
                        svg_tint: *svg_tint,
                    },
                    image_bleed_device_outset,
                );
            }

            DrawPrimitive::Video(x, y, w, h, target_id, fit) => {
                if let Some((image, image_width, image_height)) = video_state.image(target_id) {
                    draw_image_with_fit(
                        canvas,
                        image,
                        image_width,
                        image_height,
                        ImageDrawSpec {
                            rect: RectSpec {
                                x: *x,
                                y: *y,
                                w: *w,
                                h: *h,
                            },
                            image_id: target_id,
                            fit: *fit,
                            svg_tint: None,
                        },
                        image_bleed_device_outset,
                    );
                }
            }

            DrawPrimitive::ImageLoading(x, y, w, h) => {
                let rect = maybe_expand_draw_rect(
                    canvas,
                    Rect::from_xywh(*x, *y, *w, *h),
                    image_bleed_device_outset,
                );
                draw_image_loading(canvas, rect.x(), rect.y(), rect.width(), rect.height());
            }

            DrawPrimitive::ImageFailed(x, y, w, h) => {
                let rect = maybe_expand_draw_rect(
                    canvas,
                    Rect::from_xywh(*x, *y, *w, *h),
                    image_bleed_device_outset,
                );
                draw_image_failed(canvas, rect.x(), rect.y(), rect.width(), rect.height());
            }
        }
    }

    fn render_primitive_with_alpha(
        canvas: &skia_safe::Canvas,
        primitive: &DrawPrimitive,
        alpha: f32,
    ) -> bool {
        // Keep this list narrow. Groups and primitives with more complex
        // sampling/blending semantics continue through `save_layer_alpha` until
        // a focused benchmark and pixel test prove a direct alpha path is better.
        match primitive {
            DrawPrimitive::Rect(x, y, w, h, fill) => {
                let rect = Rect::from_xywh(*x, *y, *w, *h);
                let mut paint = Paint::default();
                paint.set_color(color_from_u32(color_with_multiplied_alpha(*fill, alpha)));
                paint.set_anti_alias(true);
                canvas.draw_rect(rect, &paint);
                true
            }
            DrawPrimitive::RoundedRect(x, y, w, h, radius, fill) => {
                let rect = Rect::from_xywh(*x, *y, *w, *h);
                let rrect = corner_rrect(rect, [*radius; 4]);
                let mut paint = Paint::default();
                paint.set_color(color_from_u32(color_with_multiplied_alpha(*fill, alpha)));
                paint.set_anti_alias(true);
                canvas.draw_rrect(rrect, &paint);
                true
            }
            DrawPrimitive::TextWithFont(x, y, text, font_size, fill, family, weight, italic) => {
                let font = make_font_with_style(family, *weight, *italic, *font_size);
                let mut paint = Paint::default();
                paint.set_color(color_from_u32(color_with_multiplied_alpha(*fill, alpha)));
                paint.set_anti_alias(true);
                canvas.draw_str(text, (*x, *y), &font, &paint);
                true
            }
            DrawPrimitive::Border(..)
            | DrawPrimitive::BorderCorners(..)
            | DrawPrimitive::BorderEdges(..)
            | DrawPrimitive::Shadow(..)
            | DrawPrimitive::InsetShadow(..)
            | DrawPrimitive::Gradient(..)
            | DrawPrimitive::Image(..)
            | DrawPrimitive::Video(..)
            | DrawPrimitive::ImageLoading(..)
            | DrawPrimitive::ImageFailed(..) => false,
        }
    }

    #[cfg(all(feature = "drm", target_os = "linux"))]
    /// Flush the GPU context after manual drawing.
    pub fn flush(&mut self, frame: &mut RenderFrame<'_>) {
        frame.flush();
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq)]
struct RectSpec {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct EdgeInsets {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

impl EdgeInsets {
    fn uniform(width: f32) -> Self {
        Self {
            top: width,
            right: width,
            bottom: width,
            left: width,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ImageDrawSpec<'a> {
    rect: RectSpec,
    image_id: &'a str,
    fit: ImageFit,
    svg_tint: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
struct BorderDrawSpec {
    rect: RectSpec,
    corners: [f32; 4],
    insets: EdgeInsets,
    color: u32,
    style: BorderStyle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ShadowDrawSpec {
    rect: RectSpec,
    offset_x: f32,
    offset_y: f32,
    blur: f32,
    size: f32,
    radius: f32,
    color: u32,
}

struct PreparedOuterShadow {
    shadow_rrect: RRect,
    bounds_rrect: RRect,
    paint: Paint,
}

fn matrix_from_affine2(transform: Affine2) -> Matrix {
    Matrix::new_all(
        transform.xx,
        transform.xy,
        transform.tx,
        transform.yx,
        transform.yy,
        transform.ty,
        0.0,
        0.0,
        1.0,
    )
}

fn apply_clip_shape(canvas: &skia_safe::Canvas, clip: &ClipShape) {
    let rect = Rect::from_xywh(clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height);
    match clip.radii {
        None => {
            canvas.clip_rect(rect, skia_safe::ClipOp::Intersect, true);
        }
        Some(CornerRadii { tl, tr, br, bl }) => {
            let radii = [
                Point::new(tl, tl),
                Point::new(tr, tr),
                Point::new(br, br),
                Point::new(bl, bl),
            ];
            let rrect = RRect::new_rect_radii(rect, &radii);
            canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);
        }
    }
}

fn apply_relaxed_clip_shape(canvas: &skia_safe::Canvas, clip: &ClipShape) {
    apply_clip_shape(canvas, &relax_clip_shape_to_device(canvas, *clip));
}

fn relax_clip_shape_to_device(canvas: &skia_safe::Canvas, clip: ClipShape) -> ClipShape {
    let rect = Rect::from_xywh(clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height);
    let expanded = outset_rect_in_device_space(canvas, rect, 0.5);
    let (outset_x, outset_y) = rect_outset_amount(rect, expanded);
    let outset = outset_x.max(outset_y);

    let expanded_rect = crate::tree::geometry::Rect {
        x: expanded.left(),
        y: expanded.top(),
        width: expanded.width(),
        height: expanded.height(),
    };

    let radii = clip.radii.map(|radii| {
        clamp_radii(
            expanded_rect,
            CornerRadii {
                tl: radii.tl + outset,
                tr: radii.tr + outset,
                br: radii.br + outset,
                bl: radii.bl + outset,
            },
        )
    });

    ClipShape {
        rect: expanded_rect,
        radii,
    }
}

fn outset_rect_in_device_space(canvas: &skia_safe::Canvas, rect: Rect, device_outset: f32) -> Rect {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return rect;
    }

    let matrix = canvas.local_to_device_as_3x3();
    let (device_rect, _) = matrix.map_rect(rect);
    if !device_rect.left().is_finite()
        || !device_rect.top().is_finite()
        || !device_rect.right().is_finite()
        || !device_rect.bottom().is_finite()
    {
        return rect;
    }

    let expanded_device = Rect::from_ltrb(
        device_rect.left() - device_outset,
        device_rect.top() - device_outset,
        device_rect.right() + device_outset,
        device_rect.bottom() + device_outset,
    );

    let Some(inv) = matrix.invert() else {
        return rect;
    };

    let (mapped_back, _) = inv.map_rect(expanded_device);
    if !mapped_back.left().is_finite()
        || !mapped_back.top().is_finite()
        || !mapped_back.right().is_finite()
        || !mapped_back.bottom().is_finite()
    {
        return rect;
    }

    Rect::from_ltrb(
        mapped_back.left().min(rect.left()),
        mapped_back.top().min(rect.top()),
        mapped_back.right().max(rect.right()),
        mapped_back.bottom().max(rect.bottom()),
    )
}

fn draw_cached_asset_with_fit(
    canvas: &skia_safe::Canvas,
    spec: ImageDrawSpec<'_>,
    image_bleed_device_outset: f32,
) {
    let RectSpec { w, h, .. } = spec.rect;

    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let Some(cached) = cached_asset(spec.image_id) else {
        return;
    };

    match &cached.kind {
        CachedAssetKind::Raster(image) => draw_image_with_fit(
            canvas,
            image,
            cached.width,
            cached.height,
            spec,
            image_bleed_device_outset,
        ),
        CachedAssetKind::Vector(tree) => draw_vector_asset_with_fit(
            canvas,
            spec.image_id,
            tree,
            cached.width,
            cached.height,
            spec,
            image_bleed_device_outset,
        ),
    }
}

fn draw_cached_asset_with_fit_profiled(
    canvas: &skia_safe::Canvas,
    spec: ImageDrawSpec<'_>,
    image_bleed_device_outset: f32,
) -> RenderImageDrawProfile {
    let started_at = Instant::now();
    let mut profile = RenderImageDrawProfile::new(spec);
    let RectSpec { w, h, .. } = spec.rect;

    if w > 0.0 && h > 0.0 {
        let lookup_started_at = Instant::now();
        let cached = cached_asset(spec.image_id);
        profile.asset_lookup = lookup_started_at.elapsed();

        if let Some(cached) = cached {
            profile.source_width = cached.width;
            profile.source_height = cached.height;

            match &cached.kind {
                CachedAssetKind::Raster(image) => {
                    profile.kind = RenderImageAssetKind::Raster;
                    draw_image_with_fit_profiled(
                        canvas,
                        image,
                        cached.width,
                        cached.height,
                        spec,
                        image_bleed_device_outset,
                        &mut profile,
                    );
                }
                CachedAssetKind::Vector(tree) => {
                    profile.kind = RenderImageAssetKind::Vector;
                    draw_vector_asset_with_fit_profiled(
                        canvas,
                        tree,
                        cached.width,
                        cached.height,
                        spec,
                        image_bleed_device_outset,
                        &mut profile,
                    );
                }
            }
        }
    }

    profile.total = started_at.elapsed();
    profile
}

fn draw_image_with_fit(
    canvas: &skia_safe::Canvas,
    image: &Image,
    image_width: u32,
    image_height: u32,
    spec: ImageDrawSpec<'_>,
    image_bleed_device_outset: f32,
) {
    let RectSpec { x, y, w, h } = spec.rect;

    match spec.fit {
        ImageFit::Contain | ImageFit::Cover => {
            let src_w = image_width as f32;
            let src_h = image_height as f32;
            let Some(rects) = compute_image_fit_rects(src_w, src_h, x, y, w, h, spec.fit) else {
                return;
            };

            let mut paint = Paint::default();
            paint.set_anti_alias(false);
            let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);

            let src_rect = Rect::from_xywh(rects.src_x, rects.src_y, rects.src_w, rects.src_h);
            let dst_rect = maybe_expand_fit_dst_rect(
                canvas,
                Rect::from_xywh(rects.dst_x, rects.dst_y, rects.dst_w, rects.dst_h),
                Rect::from_xywh(x, y, w, h),
                spec.fit,
                image_bleed_device_outset,
            );
            draw_image_rect_with_optional_template_tint(
                canvas,
                image,
                Some((&src_rect, SrcRectConstraint::Strict)),
                dst_rect,
                sampling,
                &paint,
                spec.svg_tint,
            );
        }
        ImageFit::Repeat | ImageFit::RepeatX | ImageFit::RepeatY => {
            draw_tiled_image(
                canvas,
                image,
                Rect::from_xywh(x, y, w, h),
                spec.fit,
                spec.svg_tint,
            );
        }
    }
}

fn draw_image_with_fit_profiled(
    canvas: &skia_safe::Canvas,
    image: &Image,
    image_width: u32,
    image_height: u32,
    spec: ImageDrawSpec<'_>,
    image_bleed_device_outset: f32,
    profile: &mut RenderImageDrawProfile,
) {
    let RectSpec { x, y, w, h } = spec.rect;

    match spec.fit {
        ImageFit::Contain | ImageFit::Cover => {
            let fit_started_at = Instant::now();
            let src_w = image_width as f32;
            let src_h = image_height as f32;
            let Some(rects) = compute_image_fit_rects(src_w, src_h, x, y, w, h, spec.fit) else {
                profile.fit_compute += fit_started_at.elapsed();
                return;
            };

            let mut paint = Paint::default();
            paint.set_anti_alias(false);
            let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);

            let src_rect = Rect::from_xywh(rects.src_x, rects.src_y, rects.src_w, rects.src_h);
            let dst_rect = maybe_expand_fit_dst_rect(
                canvas,
                Rect::from_xywh(rects.dst_x, rects.dst_y, rects.dst_w, rects.dst_h),
                Rect::from_xywh(x, y, w, h),
                spec.fit,
                image_bleed_device_outset,
            );
            profile.draw_width = dst_rect.width().ceil().max(0.0) as u32;
            profile.draw_height = dst_rect.height().ceil().max(0.0) as u32;
            profile.fit_compute += fit_started_at.elapsed();

            let draw_started_at = Instant::now();
            if let Some(tint) = spec.svg_tint {
                profile.tint_layer_used |= draw_image_rect_with_template_tint_direct(
                    canvas,
                    image,
                    Some((&src_rect, SrcRectConstraint::Strict)),
                    dst_rect,
                    sampling,
                    &paint,
                    tint,
                );
            } else {
                draw_image_rect_with_optional_template_tint(
                    canvas,
                    image,
                    Some((&src_rect, SrcRectConstraint::Strict)),
                    dst_rect,
                    sampling,
                    &paint,
                    None,
                );
            }
            profile.draw += draw_started_at.elapsed();
        }
        ImageFit::Repeat | ImageFit::RepeatX | ImageFit::RepeatY => {
            let fit_started_at = Instant::now();
            profile.draw_width = w.ceil().max(0.0) as u32;
            profile.draw_height = h.ceil().max(0.0) as u32;
            profile.fit_compute += fit_started_at.elapsed();

            let draw_started_at = Instant::now();
            profile.tint_layer_used |= draw_tiled_image(
                canvas,
                image,
                Rect::from_xywh(x, y, w, h),
                spec.fit,
                spec.svg_tint,
            );
            profile.draw += draw_started_at.elapsed();
        }
    }
}

fn draw_image_fill_rect(canvas: &skia_safe::Canvas, image: &Image, x: f32, y: f32, w: f32, h: f32) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let mut paint = Paint::default();
    paint.set_anti_alias(false);
    let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);
    let dst_rect = Rect::from_xywh(x, y, w, h);
    draw_image_rect_with_optional_template_tint(
        canvas, image, None, dst_rect, sampling, &paint, None,
    );
}

fn draw_image_fill_rect_tinted(
    canvas: &skia_safe::Canvas,
    image: &Image,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tint: Option<u32>,
) -> bool {
    if w <= 0.0 || h <= 0.0 {
        return false;
    }

    if tint.is_none() {
        draw_image_fill_rect(canvas, image, x, y, w, h);
        return false;
    }

    let mut paint = Paint::default();
    paint.set_anti_alias(false);
    let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);
    let dst_rect = Rect::from_xywh(x, y, w, h);
    if let Some(tint) = tint {
        draw_image_rect_with_template_tint_direct(
            canvas, image, None, dst_rect, sampling, &paint, tint,
        )
    } else {
        draw_image_rect_with_optional_template_tint(
            canvas, image, None, dst_rect, sampling, &paint, None,
        );
        false
    }
}

fn draw_image_rect_with_optional_template_tint(
    canvas: &skia_safe::Canvas,
    image: &Image,
    src: Option<(&Rect, SrcRectConstraint)>,
    dst_rect: Rect,
    sampling: SamplingOptions,
    paint: &Paint,
    tint: Option<u32>,
) {
    if let Some(tint) = tint {
        draw_image_rect_with_template_tint_direct(
            canvas, image, src, dst_rect, sampling, paint, tint,
        );
    } else {
        canvas.draw_image_rect_with_sampling_options(image, src, dst_rect, sampling, paint);
    }
}

fn draw_image_rect_with_template_tint_direct(
    canvas: &skia_safe::Canvas,
    image: &Image,
    src: Option<(&Rect, SrcRectConstraint)>,
    dst_rect: Rect,
    sampling: SamplingOptions,
    paint: &Paint,
    tint: u32,
) -> bool {
    if let Some(tinted_paint) = paint_with_template_tint(paint, tint) {
        canvas.draw_image_rect_with_sampling_options(image, src, dst_rect, sampling, &tinted_paint);
        false
    } else {
        draw_with_template_tint(canvas, dst_rect, tint, |canvas| {
            canvas.draw_image_rect_with_sampling_options(image, src, dst_rect, sampling, paint);
        });
        true
    }
}

fn maybe_expand_fit_dst_rect(
    canvas: &skia_safe::Canvas,
    dst_rect: Rect,
    container_rect: Rect,
    fit: ImageFit,
    image_bleed_device_outset: f32,
) -> Rect {
    if image_bleed_device_outset <= 0.0 {
        return dst_rect;
    }

    match fit {
        ImageFit::Cover => {
            maybe_expand_draw_rect_axes(canvas, dst_rect, image_bleed_device_outset, true, true)
        }
        ImageFit::Contain => {
            let epsilon = 0.01;
            let expand_x = (dst_rect.left() - container_rect.left()).abs() <= epsilon
                && (dst_rect.right() - container_rect.right()).abs() <= epsilon;
            let expand_y = (dst_rect.top() - container_rect.top()).abs() <= epsilon
                && (dst_rect.bottom() - container_rect.bottom()).abs() <= epsilon;

            maybe_expand_draw_rect_axes(
                canvas,
                dst_rect,
                image_bleed_device_outset,
                expand_x,
                expand_y,
            )
        }
        ImageFit::Repeat | ImageFit::RepeatX | ImageFit::RepeatY => dst_rect,
    }
}

fn maybe_expand_draw_rect(
    canvas: &skia_safe::Canvas,
    rect: Rect,
    image_bleed_device_outset: f32,
) -> Rect {
    maybe_expand_draw_rect_axes(canvas, rect, image_bleed_device_outset, true, true)
}

fn maybe_expand_draw_rect_axes(
    canvas: &skia_safe::Canvas,
    rect: Rect,
    image_bleed_device_outset: f32,
    expand_x: bool,
    expand_y: bool,
) -> Rect {
    if image_bleed_device_outset <= 0.0 || (!expand_x && !expand_y) {
        return rect;
    }

    let expanded = outset_rect_in_device_space(canvas, rect, image_bleed_device_outset);
    Rect::from_ltrb(
        if expand_x {
            expanded.left()
        } else {
            rect.left()
        },
        if expand_y { expanded.top() } else { rect.top() },
        if expand_x {
            expanded.right()
        } else {
            rect.right()
        },
        if expand_y {
            expanded.bottom()
        } else {
            rect.bottom()
        },
    )
}

fn draw_with_template_tint<F>(canvas: &skia_safe::Canvas, bounds: Rect, tint: u32, draw: F)
where
    F: FnOnce(&skia_safe::Canvas),
{
    let layer_rec = SaveLayerRec::default().bounds(&bounds);
    canvas.save_layer(&layer_rec);
    draw(canvas);

    let mut tint_paint = Paint::default();
    tint_paint.set_color(color_from_u32(tint));
    tint_paint.set_blend_mode(BlendMode::SrcIn);
    canvas.draw_rect(bounds, &tint_paint);
    canvas.restore();
}

fn paint_with_template_tint(paint: &Paint, tint: u32) -> Option<Paint> {
    let filter = color_filters::blend(color_from_u32(tint), BlendMode::SrcIn)?;
    let mut tinted = paint.clone();
    tinted.set_color_filter(filter);
    Some(tinted)
}

fn get_or_rasterize_vector_variant(
    asset_id: &str,
    tree: &usvg::Tree,
    width: u32,
    height: u32,
) -> Option<Image> {
    if let Some(image) = lookup_rendered_vector_variant(asset_id, width, height) {
        return Some(image);
    }

    let image = rasterize_vector_tree(tree, width, height)?;
    store_rendered_vector_variant(asset_id, width, height, &image);
    Some(image)
}

fn get_or_rasterize_vector_variant_profiled(
    asset_id: &str,
    tree: &usvg::Tree,
    width: u32,
    height: u32,
    profile: &mut RenderImageDrawProfile,
) -> Option<Image> {
    let lookup_started_at = Instant::now();
    let cached = lookup_rendered_vector_variant(asset_id, width, height);
    profile.vector_cache_lookup += lookup_started_at.elapsed();

    if let Some(image) = cached {
        profile.vector_cache_hit = Some(true);
        return Some(image);
    }

    profile.vector_cache_hit = Some(false);
    let rasterize_started_at = Instant::now();
    let image = rasterize_vector_tree(tree, width, height);
    profile.vector_rasterize += rasterize_started_at.elapsed();
    let image = image?;

    let store_started_at = Instant::now();
    store_rendered_vector_variant(asset_id, width, height, &image);
    profile.vector_cache_store += store_started_at.elapsed();
    Some(image)
}

fn draw_vector_asset_with_fit(
    canvas: &skia_safe::Canvas,
    asset_id: &str,
    tree: &usvg::Tree,
    asset_width: u32,
    asset_height: u32,
    spec: ImageDrawSpec<'_>,
    image_bleed_device_outset: f32,
) {
    let RectSpec { x, y, w, h } = spec.rect;

    match spec.fit {
        ImageFit::Contain | ImageFit::Cover => {
            let src_w = asset_width as f32;
            let src_h = asset_height as f32;
            let Some((draw_x, draw_y, draw_w, draw_h)) =
                compute_vector_fit_rect(src_w, src_h, x, y, w, h, spec.fit)
            else {
                return;
            };

            let dst_rect = maybe_expand_fit_dst_rect(
                canvas,
                Rect::from_xywh(draw_x, draw_y, draw_w, draw_h),
                Rect::from_xywh(x, y, w, h),
                spec.fit,
                image_bleed_device_outset,
            );

            let raster_width = dst_rect.width().ceil().max(1.0) as u32;
            let raster_height = dst_rect.height().ceil().max(1.0) as u32;
            let Some(image) =
                get_or_rasterize_vector_variant(asset_id, tree, raster_width, raster_height)
            else {
                return;
            };

            canvas.save();
            if matches!(spec.fit, ImageFit::Cover) {
                let clip = Rect::from_xywh(x, y, w, h);
                canvas.clip_rect(clip, skia_safe::ClipOp::Intersect, true);
            }
            draw_image_fill_rect_tinted(
                canvas,
                &image,
                dst_rect.x(),
                dst_rect.y(),
                dst_rect.width(),
                dst_rect.height(),
                spec.svg_tint,
            );
            canvas.restore();
        }
        ImageFit::Repeat | ImageFit::RepeatX | ImageFit::RepeatY => {
            let Some(image) =
                get_or_rasterize_vector_variant(asset_id, tree, asset_width, asset_height)
            else {
                return;
            };

            draw_tiled_image(
                canvas,
                &image,
                Rect::from_xywh(x, y, w, h),
                spec.fit,
                spec.svg_tint,
            );
        }
    }
}

fn draw_vector_asset_with_fit_profiled(
    canvas: &skia_safe::Canvas,
    tree: &usvg::Tree,
    asset_width: u32,
    asset_height: u32,
    spec: ImageDrawSpec<'_>,
    image_bleed_device_outset: f32,
    profile: &mut RenderImageDrawProfile,
) {
    let RectSpec { x, y, w, h } = spec.rect;

    match spec.fit {
        ImageFit::Contain | ImageFit::Cover => {
            let fit_started_at = Instant::now();
            let src_w = asset_width as f32;
            let src_h = asset_height as f32;
            let Some((draw_x, draw_y, draw_w, draw_h)) =
                compute_vector_fit_rect(src_w, src_h, x, y, w, h, spec.fit)
            else {
                profile.fit_compute += fit_started_at.elapsed();
                return;
            };

            let dst_rect = maybe_expand_fit_dst_rect(
                canvas,
                Rect::from_xywh(draw_x, draw_y, draw_w, draw_h),
                Rect::from_xywh(x, y, w, h),
                spec.fit,
                image_bleed_device_outset,
            );

            let raster_width = dst_rect.width().ceil().max(1.0) as u32;
            let raster_height = dst_rect.height().ceil().max(1.0) as u32;
            profile.draw_width = raster_width;
            profile.draw_height = raster_height;
            profile.fit_compute += fit_started_at.elapsed();

            let Some(image) = get_or_rasterize_vector_variant_profiled(
                spec.image_id,
                tree,
                raster_width,
                raster_height,
                profile,
            ) else {
                return;
            };

            let draw_started_at = Instant::now();
            canvas.save();
            if matches!(spec.fit, ImageFit::Cover) {
                let clip = Rect::from_xywh(x, y, w, h);
                canvas.clip_rect(clip, skia_safe::ClipOp::Intersect, true);
            }
            profile.tint_layer_used |= draw_image_fill_rect_tinted(
                canvas,
                &image,
                dst_rect.x(),
                dst_rect.y(),
                dst_rect.width(),
                dst_rect.height(),
                spec.svg_tint,
            );
            canvas.restore();
            profile.draw += draw_started_at.elapsed();
        }
        ImageFit::Repeat | ImageFit::RepeatX | ImageFit::RepeatY => {
            let fit_started_at = Instant::now();
            profile.draw_width = w.ceil().max(0.0) as u32;
            profile.draw_height = h.ceil().max(0.0) as u32;
            profile.fit_compute += fit_started_at.elapsed();

            let Some(image) = get_or_rasterize_vector_variant_profiled(
                spec.image_id,
                tree,
                asset_width,
                asset_height,
                profile,
            ) else {
                return;
            };

            let draw_started_at = Instant::now();
            profile.tint_layer_used |= draw_tiled_image(
                canvas,
                &image,
                Rect::from_xywh(x, y, w, h),
                spec.fit,
                spec.svg_tint,
            );
            profile.draw += draw_started_at.elapsed();
        }
    }
}

fn draw_tiled_image(
    canvas: &skia_safe::Canvas,
    image: &Image,
    bounds: Rect,
    fit: ImageFit,
    tint: Option<u32>,
) -> bool {
    let Some(tile_modes) = tile_modes_for_fit(fit) else {
        return false;
    };

    let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);
    let local_matrix = Matrix::translate((bounds.x(), bounds.y()));
    let Some(shader) = image.to_shader(Some(tile_modes), sampling, Some(&local_matrix)) else {
        return false;
    };

    let mut paint = Paint::default();
    paint.set_anti_alias(false);
    paint.set_shader(shader);

    let dst_rect = bounds;
    if let Some(tint) = tint {
        if let Some(filter) = color_filters::blend(color_from_u32(tint), BlendMode::SrcIn) {
            paint.set_color_filter(filter);
            canvas.draw_rect(dst_rect, &paint);
            false
        } else {
            draw_with_template_tint(canvas, dst_rect, tint, |canvas| {
                canvas.draw_rect(dst_rect, &paint);
            });
            true
        }
    } else {
        canvas.draw_rect(dst_rect, &paint);
        false
    }
}

fn rasterize_vector_tree(tree: &usvg::Tree, width: u32, height: u32) -> Option<Image> {
    if width == 0 || height == 0 {
        return None;
    }

    #[cfg(test)]
    VECTOR_RASTERIZATION_COUNT.fetch_add(1, Ordering::Relaxed);

    let src_w = tree.size().width();
    let src_h = tree.size().height();
    if src_w <= 0.0 || src_h <= 0.0 {
        return None;
    }

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let transform =
        resvg::tiny_skia::Transform::from_scale(width as f32 / src_w, height as f32 / src_h);
    let mut pixmap_mut = pixmap.as_mut();
    resvg::render(tree, transform, &mut pixmap_mut);

    raster_image_from_rgba(width, height, pixmap.data())
}

#[cfg(test)]
fn clear_rendered_vector_cache() {
    if let Ok(mut cache) = get_rendered_vector_cache().lock() {
        *cache = RenderedVectorCache::default();
    }
}

#[cfg(test)]
fn rendered_vector_cache_entry_count() -> usize {
    get_rendered_vector_cache()
        .lock()
        .map(|cache| cache.entries.len())
        .unwrap_or(0)
}

#[cfg(test)]
fn reset_vector_rasterization_count() {
    VECTOR_RASTERIZATION_COUNT.store(0, Ordering::Relaxed);
}

#[cfg(test)]
fn vector_rasterization_count() -> usize {
    VECTOR_RASTERIZATION_COUNT.load(Ordering::Relaxed)
}

fn compute_vector_fit_rect(
    src_w: f32,
    src_h: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fit: ImageFit,
) -> Option<(f32, f32, f32, f32)> {
    if src_w <= 0.0 || src_h <= 0.0 || w <= 0.0 || h <= 0.0 {
        return None;
    }

    let scale_x = w / src_w;
    let scale_y = h / src_h;
    if !scale_x.is_finite() || !scale_y.is_finite() {
        return None;
    }

    let scale = match fit {
        ImageFit::Contain => scale_x.min(scale_y),
        ImageFit::Cover => scale_x.max(scale_y),
        ImageFit::Repeat | ImageFit::RepeatX | ImageFit::RepeatY => return None,
    };

    let draw_w = src_w * scale;
    let draw_h = src_h * scale;
    let draw_x = x + (w - draw_w) * 0.5;
    let draw_y = y + (h - draw_h) * 0.5;

    Some((draw_x, draw_y, draw_w, draw_h))
}

fn tile_modes_for_fit(fit: ImageFit) -> Option<(TileMode, TileMode)> {
    match fit {
        ImageFit::Repeat => Some((TileMode::Repeat, TileMode::Repeat)),
        ImageFit::RepeatX => Some((TileMode::Repeat, TileMode::Decal)),
        ImageFit::RepeatY => Some((TileMode::Decal, TileMode::Repeat)),
        ImageFit::Contain | ImageFit::Cover => None,
    }
}

#[cfg(test)]
fn snap_outset_rect_to_device(canvas: &skia_safe::Canvas, rect: Rect) -> Rect {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return rect;
    }

    let matrix = canvas.local_to_device_as_3x3();
    let (device_rect, _) = matrix.map_rect(rect);
    if !device_rect.left().is_finite()
        || !device_rect.top().is_finite()
        || !device_rect.right().is_finite()
        || !device_rect.bottom().is_finite()
    {
        return rect;
    }

    let snapped_device = Rect::from_ltrb(
        device_rect.left().floor(),
        device_rect.top().floor(),
        device_rect.right().ceil(),
        device_rect.bottom().ceil(),
    );

    let Some(inv) = matrix.invert() else {
        return rect;
    };

    let (mapped_back, _) = inv.map_rect(snapped_device);
    if !mapped_back.left().is_finite()
        || !mapped_back.top().is_finite()
        || !mapped_back.right().is_finite()
        || !mapped_back.bottom().is_finite()
    {
        return rect;
    }

    Rect::from_ltrb(
        mapped_back.left().min(rect.left()),
        mapped_back.top().min(rect.top()),
        mapped_back.right().max(rect.right()),
        mapped_back.bottom().max(rect.bottom()),
    )
}

fn rect_outset_amount(original: Rect, expanded: Rect) -> (f32, f32) {
    let outset_x = (original.left() - expanded.left())
        .max(expanded.right() - original.right())
        .max(0.0);
    let outset_y = (original.top() - expanded.top())
        .max(expanded.bottom() - original.bottom())
        .max(0.0);
    (outset_x, outset_y)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ImageFitRects {
    src_x: f32,
    src_y: f32,
    src_w: f32,
    src_h: f32,
    dst_x: f32,
    dst_y: f32,
    dst_w: f32,
    dst_h: f32,
}

fn compute_image_fit_rects(
    src_w: f32,
    src_h: f32,
    dst_x: f32,
    dst_y: f32,
    dst_w: f32,
    dst_h: f32,
    fit: ImageFit,
) -> Option<ImageFitRects> {
    let values = [src_w, src_h, dst_x, dst_y, dst_w, dst_h];
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }

    if src_w <= 0.0 || src_h <= 0.0 || dst_w <= 0.0 || dst_h <= 0.0 {
        return None;
    }

    match fit {
        ImageFit::Contain => {
            let scale = (dst_w / src_w).min(dst_h / src_h);
            if !scale.is_finite() || scale <= 0.0 {
                return None;
            }

            let draw_w = (src_w * scale).clamp(0.0, dst_w);
            let draw_h = (src_h * scale).clamp(0.0, dst_h);
            let draw_x = dst_x + (dst_w - draw_w) * 0.5;
            let draw_y = dst_y + (dst_h - draw_h) * 0.5;

            Some(ImageFitRects {
                src_x: 0.0,
                src_y: 0.0,
                src_w,
                src_h,
                dst_x: draw_x,
                dst_y: draw_y,
                dst_w: draw_w,
                dst_h: draw_h,
            })
        }
        ImageFit::Cover => {
            let scale = (dst_w / src_w).max(dst_h / src_h);
            if !scale.is_finite() || scale <= 0.0 {
                return None;
            }

            let crop_w = (dst_w / scale).clamp(0.0, src_w);
            let crop_h = (dst_h / scale).clamp(0.0, src_h);
            if crop_w <= 0.0 || crop_h <= 0.0 {
                return None;
            }

            let crop_x = ((src_w - crop_w) * 0.5).clamp(0.0, src_w - crop_w);
            let crop_y = ((src_h - crop_h) * 0.5).clamp(0.0, src_h - crop_h);

            Some(ImageFitRects {
                src_x: crop_x,
                src_y: crop_y,
                src_w: crop_w,
                src_h: crop_h,
                dst_x,
                dst_y,
                dst_w,
                dst_h,
            })
        }
        ImageFit::Repeat | ImageFit::RepeatX | ImageFit::RepeatY => None,
    }
}

fn draw_image_loading(canvas: &skia_safe::Canvas, x: f32, y: f32, w: f32, h: f32) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let rect = Rect::from_xywh(x, y, w, h);

    let mut bg = Paint::default();
    bg.set_anti_alias(true);
    bg.set_color(Color::from_argb(255, 238, 242, 247));
    canvas.draw_rect(rect, &bg);

    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as f32)
        .unwrap_or(0.0);

    let period = 1400.0;
    let phase = (millis % period) / period;
    let band_w = (w * 0.35).max(24.0);
    let band_x = x - band_w + (w + band_w * 2.0) * phase;

    let shimmer_rect = Rect::from_xywh(band_x, y, band_w, h);
    let mut shimmer = Paint::default();
    shimmer.set_anti_alias(true);
    shimmer.set_color(Color::from_argb(170, 248, 250, 252));

    canvas.save();
    canvas.clip_rect(rect, skia_safe::ClipOp::Intersect, true);
    canvas.draw_rect(shimmer_rect, &shimmer);
    canvas.restore();

    draw_image_placeholder_glyph(canvas, rect, Color::from_argb(180, 148, 163, 184));
}

fn draw_image_failed(canvas: &skia_safe::Canvas, x: f32, y: f32, w: f32, h: f32) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let rect = Rect::from_xywh(x, y, w, h);

    let mut bg = Paint::default();
    bg.set_anti_alias(true);
    bg.set_color(Color::from_argb(255, 254, 242, 242));
    canvas.draw_rect(rect, &bg);

    draw_image_placeholder_glyph(canvas, rect, Color::from_argb(210, 248, 113, 113));

    let stroke = (w.min(h) * 0.08).clamp(1.0, 6.0);

    let mut line = Paint::default();
    line.set_anti_alias(true);
    line.set_style(PaintStyle::Stroke);
    line.set_stroke_width(stroke);
    line.set_color(Color::from_argb(230, 220, 38, 38));

    let inset = stroke * 1.6;
    let x0 = x + inset;
    let y0 = y + inset;
    let x1 = x + w - inset;
    let y1 = y + h - inset;

    canvas.draw_line((x0, y0), (x1, y1), &line);
    canvas.draw_line((x1, y0), (x0, y1), &line);
}

fn draw_image_placeholder_glyph(canvas: &skia_safe::Canvas, rect: Rect, color: Color) {
    let min_side = rect.width().min(rect.height());
    if min_side < 28.0 {
        return;
    }

    let icon_w = (min_side * 0.38).clamp(18.0, 52.0);
    let icon_h = icon_w * 0.72;
    let icon_x = rect.x() + (rect.width() - icon_w) * 0.5;
    let icon_y = rect.y() + (rect.height() - icon_h) * 0.5;
    let icon_rect = Rect::from_xywh(icon_x, icon_y, icon_w, icon_h);

    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_style(PaintStyle::Stroke);
    paint.set_stroke_width((min_side * 0.018).clamp(1.0, 2.0));
    paint.set_color(color);

    canvas.draw_rrect(RRect::new_rect_xy(icon_rect, 3.0, 3.0), &paint);

    let dot_radius = (icon_w * 0.055).max(1.2);
    canvas.draw_circle(
        (icon_x + icon_w * 0.72, icon_y + icon_h * 0.28),
        dot_radius,
        &paint,
    );

    let mut path = PathBuilder::new();
    path.move_to((icon_x + icon_w * 0.18, icon_y + icon_h * 0.74));
    path.line_to((icon_x + icon_w * 0.40, icon_y + icon_h * 0.52));
    path.line_to((icon_x + icon_w * 0.54, icon_y + icon_h * 0.65));
    path.line_to((icon_x + icon_w * 0.68, icon_y + icon_h * 0.48));
    path.line_to((icon_x + icon_w * 0.84, icon_y + icon_h * 0.74));
    let path = path.detach();
    canvas.draw_path(&path, &paint);
}

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1.0e-3
}

fn border_path_clip_candidate_count(style: BorderStyle, insets: [f32; 4]) -> u32 {
    match style {
        BorderStyle::Solid => 0,
        BorderStyle::Dashed | BorderStyle::Dotted => {
            let non_zero_edges = insets.iter().filter(|width| **width > 0.0).count() as u32;
            if insets
                .iter()
                .all(|width| approx_eq(*width, insets.first().copied().unwrap_or(0.0)))
            {
                u32::from(non_zero_edges > 0)
            } else {
                non_zero_edges
            }
        }
    }
}

fn resolve_inset_pair(start: f32, end: f32, total: f32) -> (f32, f32) {
    let start = start.max(0.0);
    let end = end.max(0.0);
    let total = total.max(0.0);

    let sum = start + end;
    if sum <= total || sum <= f32::EPSILON {
        (start, end)
    } else {
        let scale = total / sum;
        (start * scale, end * scale)
    }
}

fn corner_rrect(rect: Rect, corners: [f32; 4]) -> RRect {
    let max_rx = rect.width().max(0.0) * 0.5;
    let max_ry = rect.height().max(0.0) * 0.5;
    let clamp = |corner: f32| corner.max(0.0).min(max_rx).min(max_ry);

    let radii = [
        Point::new(clamp(corners[0]), clamp(corners[0])),
        Point::new(clamp(corners[1]), clamp(corners[1])),
        Point::new(clamp(corners[2]), clamp(corners[2])),
        Point::new(clamp(corners[3]), clamp(corners[3])),
    ];

    if radii.iter().all(|p| p.x <= 0.0 && p.y <= 0.0) {
        RRect::new_rect(rect)
    } else {
        RRect::new_rect_radii(rect, &radii)
    }
}

fn border_band_path(outer_rrect: RRect, inner_rrect: Option<RRect>) -> skia_safe::Path {
    let mut builder = PathBuilder::new_with_fill_type(PathFillType::EvenOdd);
    builder.add_rrect(outer_rrect, None, None);
    if let Some(inner) = inner_rrect {
        builder.add_rrect(inner, None, None);
    }
    builder.detach()
}

fn prepare_outer_shadow(spec: ShadowDrawSpec) -> PreparedOuterShadow {
    let RectSpec { x, y, w, h } = spec.rect;
    let shadow_x = x + spec.offset_x - spec.size;
    let shadow_y = y + spec.offset_y - spec.size;
    let shadow_w = w + spec.size * 2.0;
    let shadow_h = h + spec.size * 2.0;
    let shadow_radius = (spec.radius + spec.size).max(0.0);

    let shadow_rrect = corner_rrect(
        Rect::from_xywh(shadow_x, shadow_y, shadow_w, shadow_h),
        [shadow_radius; 4],
    );
    let bounds_rrect = corner_rrect(Rect::from_xywh(x, y, w, h), [spec.radius; 4]);

    let mut paint = Paint::default();
    paint.set_color(color_from_u32(spec.color));
    paint.set_anti_alias(true);

    if spec.blur > 0.0 {
        let sigma = spec.blur / 2.0;
        if let Some(filter) = MaskFilter::blur(BlurStyle::Normal, sigma, false) {
            paint.set_mask_filter(filter);
        }
    }

    PreparedOuterShadow {
        shadow_rrect,
        bounds_rrect,
        paint,
    }
}

fn draw_prepared_outer_shadow(canvas: &skia_safe::Canvas, prepared: &PreparedOuterShadow) {
    // `Canvas::draw_shadow` was kept as a benchmark-only candidate. It did not
    // beat this mask-filter path in `native/renderer/direct_candidates` and is
    // not a semantic match for Emerge's CSS-like spread/transparent-center
    // shadow model, so the renderer keeps the simpler proven implementation.
    canvas.save();
    canvas.clip_rrect(prepared.bounds_rrect, skia_safe::ClipOp::Difference, true);
    canvas.draw_rrect(prepared.shadow_rrect, &prepared.paint);
    canvas.restore();
}

fn draw_outer_shadow(canvas: &skia_safe::Canvas, spec: ShadowDrawSpec) {
    let prepared = prepare_outer_shadow(spec);
    draw_prepared_outer_shadow(canvas, &prepared);
}

fn draw_outer_shadow_profiled(
    canvas: &skia_safe::Canvas,
    spec: ShadowDrawSpec,
) -> RenderShadowDrawProfile {
    let total_started_at = Instant::now();
    let mut profile = RenderShadowDrawProfile::new(spec);

    let prepare_started_at = Instant::now();
    let prepared = prepare_outer_shadow(spec);
    profile.prepare = prepare_started_at.elapsed();

    let clip_started_at = Instant::now();
    canvas.save();
    canvas.clip_rrect(prepared.bounds_rrect, skia_safe::ClipOp::Difference, true);
    profile.clip += clip_started_at.elapsed();

    let draw_started_at = Instant::now();
    canvas.draw_rrect(prepared.shadow_rrect, &prepared.paint);
    profile.draw = draw_started_at.elapsed();

    let clip_started_at = Instant::now();
    canvas.restore();
    profile.clip += clip_started_at.elapsed();

    profile.total = total_started_at.elapsed();
    profile
}

fn quad_path(quad: [(f32, f32); 4]) -> skia_safe::Path {
    PathBuilder::new()
        .move_to(Point::new(quad[0].0, quad[0].1))
        .line_to(Point::new(quad[1].0, quad[1].1))
        .line_to(Point::new(quad[2].0, quad[2].1))
        .line_to(Point::new(quad[3].0, quad[3].1))
        .close()
        .detach()
}

#[cfg(test)]
fn draw_border(canvas: &skia_safe::Canvas, spec: BorderDrawSpec) {
    draw_border_with_fast_path(canvas, spec, true);
}

fn draw_border_with_fast_path(
    canvas: &skia_safe::Canvas,
    spec: BorderDrawSpec,
    solid_border_fast_paths: bool,
) {
    let RectSpec { x, y, w, h } = spec.rect;
    let corners = spec.corners;
    let EdgeInsets {
        top,
        right,
        bottom,
        left,
    } = spec.insets;
    let color = spec.color;
    let style = spec.style;

    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let (left, right) = resolve_inset_pair(left, right, w);
    let (top, bottom) = resolve_inset_pair(top, bottom, h);
    let resolved_insets = EdgeInsets {
        top,
        right,
        bottom,
        left,
    };

    if top <= 0.0 && right <= 0.0 && bottom <= 0.0 && left <= 0.0 {
        return;
    }

    if solid_border_fast_paths
        && style == BorderStyle::Solid
        && corners.iter().all(|corner| *corner <= 0.0)
    {
        let fill_paint = solid_border_paint(color);
        if draw_single_edge_solid_border_rect(canvas, spec.rect, resolved_insets, &fill_paint) {
            return;
        }
    }

    let outer_rect = Rect::from_xywh(x, y, w, h);
    let outer_rrect = corner_rrect(outer_rect, corners);

    let outer_tl = outer_rrect.radii(skia_safe::rrect::Corner::UpperLeft);
    let outer_tr = outer_rrect.radii(skia_safe::rrect::Corner::UpperRight);
    let outer_br = outer_rrect.radii(skia_safe::rrect::Corner::LowerRight);
    let outer_bl = outer_rrect.radii(skia_safe::rrect::Corner::LowerLeft);

    let inner_x = x + left;
    let inner_y = y + top;
    let inner_w = (w - left - right).max(0.0);
    let inner_h = (h - top - bottom).max(0.0);

    let inner_rrect = if inner_w > 0.0 && inner_h > 0.0 {
        let inner_rect = Rect::from_xywh(inner_x, inner_y, inner_w, inner_h);
        let max_rx = inner_w * 0.5;
        let max_ry = inner_h * 0.5;
        let radii = [
            Point::new(
                (outer_tl.x - left).max(0.0).min(max_rx),
                (outer_tl.y - top).max(0.0).min(max_ry),
            ),
            Point::new(
                (outer_tr.x - right).max(0.0).min(max_rx),
                (outer_tr.y - top).max(0.0).min(max_ry),
            ),
            Point::new(
                (outer_br.x - right).max(0.0).min(max_rx),
                (outer_br.y - bottom).max(0.0).min(max_ry),
            ),
            Point::new(
                (outer_bl.x - left).max(0.0).min(max_rx),
                (outer_bl.y - bottom).max(0.0).min(max_ry),
            ),
        ];

        Some(if radii.iter().all(|p| p.x <= 0.0 && p.y <= 0.0) {
            RRect::new_rect(inner_rect)
        } else {
            RRect::new_rect_radii(inner_rect, &radii)
        })
    } else {
        None
    };

    match style {
        BorderStyle::Solid => {
            let fill_paint = solid_border_paint(color);
            if solid_border_fast_paths {
                draw_solid_border_rrect(canvas, outer_rrect, inner_rrect, &fill_paint);
            } else {
                let band_path = border_band_path(outer_rrect, inner_rrect);
                canvas.draw_path(&band_path, &fill_paint);
            }
        }
        BorderStyle::Dashed | BorderStyle::Dotted => {
            let band_path = border_band_path(outer_rrect, inner_rrect);
            let mut stroke_paint = Paint::default();
            stroke_paint.set_color(color_from_u32(color));
            stroke_paint.set_style(PaintStyle::Stroke);
            stroke_paint.set_anti_alias(true);

            let uniform =
                approx_eq(top, right) && approx_eq(right, bottom) && approx_eq(bottom, left);

            if uniform {
                let width = top;
                if width <= 0.0 {
                    return;
                }
                stroke_paint.set_stroke_width(width * 2.0);
                apply_border_style(&mut stroke_paint, style, width);
                canvas.save();
                canvas.clip_path(&band_path, skia_safe::ClipOp::Intersect, true);
                canvas.draw_rrect(outer_rrect, &stroke_paint);
                canvas.restore();
            } else {
                let edge_clips = border_edge_clip_quads(spec.rect, spec.insets);
                for (width, quad) in &edge_clips {
                    if *width <= 0.0 {
                        continue;
                    }

                    let edge_path = quad_path(*quad);

                    canvas.save();
                    canvas.clip_path(&band_path, skia_safe::ClipOp::Intersect, true);
                    canvas.clip_path(&edge_path, skia_safe::ClipOp::Intersect, false);
                    stroke_paint.set_stroke_width(*width * 2.0);
                    apply_border_style(&mut stroke_paint, style, *width);
                    canvas.draw_rrect(outer_rrect, &stroke_paint);
                    canvas.restore();
                }
            }
        }
    }
}

fn solid_border_paint(color: u32) -> Paint {
    let mut paint = Paint::default();
    paint.set_color(color_from_u32(color));
    paint.set_anti_alias(true);
    paint
}

fn draw_solid_border_rrect(
    canvas: &skia_safe::Canvas,
    outer_rrect: RRect,
    inner_rrect: Option<RRect>,
    paint: &Paint,
) {
    if let Some(inner_rrect) = inner_rrect {
        canvas.draw_drrect(outer_rrect, inner_rrect, paint);
    } else {
        canvas.draw_rrect(outer_rrect, paint);
    }
}

fn draw_single_edge_solid_border_rect(
    canvas: &skia_safe::Canvas,
    rect: RectSpec,
    insets: EdgeInsets,
    paint: &Paint,
) -> bool {
    let RectSpec { x, y, w, h } = rect;
    let EdgeInsets {
        top,
        right,
        bottom,
        left,
    } = insets;

    let non_zero_edges = [top, right, bottom, left]
        .into_iter()
        .filter(|width| *width > 0.0)
        .count();

    if non_zero_edges != 1 {
        return false;
    }

    let edge_rect = if top > 0.0 {
        Rect::from_xywh(x, y, w, top)
    } else if right > 0.0 {
        Rect::from_xywh(x + w - right, y, right, h)
    } else if bottom > 0.0 {
        Rect::from_xywh(x, y + h - bottom, w, bottom)
    } else {
        Rect::from_xywh(x, y, left, h)
    };

    canvas.draw_rect(edge_rect, paint);
    true
}

fn apply_border_style(paint: &mut Paint, style: BorderStyle, stroke_width: f32) {
    match style {
        BorderStyle::Solid => {}
        BorderStyle::Dashed => {
            let segment = (stroke_width * 3.0).max(4.0);
            let gap = (stroke_width * 2.0).max(3.0);
            if let Some(effect) = dash_path_effect::new(&[segment, gap], 0.0) {
                paint.set_path_effect(effect);
            }
        }
        BorderStyle::Dotted => {
            let dot = stroke_width.max(1.0);
            let gap = (stroke_width * 1.5).max(2.0);
            if let Some(effect) = dash_path_effect::new(&[dot, gap], 0.0) {
                paint.set_path_effect(effect);
            }
        }
    }
}

pub fn color_from_u32(c: u32) -> Color {
    // RGBA format: 0xRRGGBBAA
    let r = ((c >> 24) & 0xFF) as u8;
    let g = ((c >> 16) & 0xFF) as u8;
    let b = ((c >> 8) & 0xFF) as u8;
    let a = (c & 0xFF) as u8;
    Color::from_argb(a, r, g, b)
}

fn color_with_multiplied_alpha(c: u32, alpha: f32) -> u32 {
    let rgb = c & 0xFFFF_FF00;
    let source_alpha = (c & 0xFF) as f32;
    let alpha = (source_alpha * alpha.clamp(0.0, 1.0)).round() as u32;
    rgb | alpha.min(0xFF)
}

/// Compute the four clip polygons used by `BorderEdges` rendering.
///
/// The clips are quads that split corners at the CSS-style inner-join points
/// derived from per-edge insets.
///
/// Returns `[(stroke_width, [(x,y); 4]); 4]` in top/right/bottom/left order.
fn border_edge_clip_quads(rect: RectSpec, insets: EdgeInsets) -> [(f32, [(f32, f32); 4]); 4] {
    let RectSpec { x, y, w, h } = rect;
    let EdgeInsets {
        top,
        right,
        bottom,
        left,
    } = insets;

    let (left, right) = resolve_inset_pair(left, right, w);
    let (top, bottom) = resolve_inset_pair(top, bottom, h);

    let join_tl = (x + left, y + top);
    let join_tr = (x + w - right, y + top);
    let join_br = (x + w - right, y + h - bottom);
    let join_bl = (x + left, y + h - bottom);

    let margin = top.max(right).max(bottom).max(left) * 2.0 + 20.0;

    [
        (
            top,
            [
                (x - margin, y - margin),
                (x + w + margin, y - margin),
                join_tr,
                join_tl,
            ],
        ),
        (
            right,
            [
                (x + w + margin, y - margin),
                (x + w + margin, y + h + margin),
                join_br,
                join_tr,
            ],
        ),
        (
            bottom,
            [
                (x + w + margin, y + h + margin),
                (x - margin, y + h + margin),
                join_bl,
                join_br,
            ],
        ),
        (
            left,
            [
                (x - margin, y + h + margin),
                (x - margin, y - margin),
                join_tl,
                join_bl,
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point_in_convex_polygon(p: (f32, f32), vertices: &[(f32, f32)]) -> bool {
        const EPS: f32 = 1.0e-4;

        let mut sign = 0i8;
        for i in 0..vertices.len() {
            let a = vertices[i];
            let b = vertices[(i + 1) % vertices.len()];
            let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);

            if cross.abs() <= EPS {
                continue;
            }

            let current_sign = if cross > 0.0 { 1 } else { -1 };
            if sign == 0 {
                sign = current_sign;
            } else if sign != current_sign {
                return false;
            }
        }

        true
    }

    fn render_commands_to_pixels(
        width: u32,
        height: u32,
        primitives: Vec<DrawPrimitive>,
    ) -> Vec<u8> {
        render_scene_graph_to_pixels(
            width,
            height,
            RenderScene {
                nodes: primitives.into_iter().map(RenderNode::Primitive).collect(),
            },
        )
    }

    fn render_scene_graph_to_pixels(width: u32, height: u32, scene: RenderScene) -> Vec<u8> {
        render_scene_graph_to_pixels_and_timings(width, height, scene).0
    }

    fn render_scene_graph_to_pixels_and_timings(
        width: u32,
        height: u32,
        scene: RenderScene,
    ) -> (Vec<u8>, RenderTimings) {
        let mut renderer = SceneRenderer::new();
        render_scene_graph_to_pixels_and_timings_with_renderer(&mut renderer, width, height, scene)
    }

    fn render_scene_graph_to_pixels_and_timings_with_renderer(
        renderer: &mut SceneRenderer,
        width: u32,
        height: u32,
        scene: RenderScene,
    ) -> (Vec<u8>, RenderTimings) {
        let info = skia_safe::ImageInfo::new(
            (width as i32, height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut surface = skia_safe::surfaces::raster(&info, None, None)
            .expect("raster surface should be created for renderer test");

        let state = RenderState::new(scene, Color::TRANSPARENT, 1, false);
        let timings = {
            let mut frame = RenderFrame::new(&mut surface, None);
            renderer.render(&mut frame, &state)
        };

        let mut pixels = vec![0u8; (width * height * 4) as usize];
        surface.read_pixels(&info, pixels.as_mut_slice(), (width * 4) as usize, (0, 0));
        (pixels, timings)
    }

    fn render_scene_graph_profiled(width: u32, height: u32, scene: RenderScene) -> RenderTimings {
        let info = skia_safe::ImageInfo::new(
            (width as i32, height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut surface = skia_safe::surfaces::raster(&info, None, None)
            .expect("raster surface should be created for renderer test");

        let mut renderer = SceneRenderer::new();
        let state = RenderState::new(scene, Color::TRANSPARENT, 1, false);
        let mut frame = RenderFrame::new(&mut surface, None);
        renderer.render_profiled(&mut frame, &state)
    }

    #[test]
    fn renderer_cache_manager_uses_configured_limits() {
        let config = RendererCacheConfig {
            max_new_payloads_per_frame: 0,
            clean_subtree: CleanSubtreeCacheConfig {
                max_entries: 2,
                max_bytes: 1024,
                max_entry_bytes: 128,
            },
        };

        let mut cache = RendererCacheManager::with_config(config);
        assert_eq!(cache.max_new_payloads_per_frame, 0);
        assert_eq!(cache.clean_subtree.max_entries, 2);
        assert_eq!(cache.clean_subtree.max_bytes, 1024);
        assert_eq!(cache.clean_subtree.max_entry_bytes, 128);

        let frame = cache.begin_frame();
        assert_eq!(frame.new_payload_budget_remaining, 0);

        let renderer = SceneRenderer::with_cache_config(config);
        assert_eq!(renderer.renderer_cache.max_new_payloads_per_frame, 0);
        assert_eq!(renderer.renderer_cache.clean_subtree.max_entries, 2);
        assert_eq!(renderer.renderer_cache.clean_subtree.max_bytes, 1024);
        assert_eq!(renderer.renderer_cache.clean_subtree.max_entry_bytes, 128);
    }

    #[test]
    fn renderer_cache_lifecycle_tracks_budget_stats_and_generation_clear() {
        let mut cache = RendererCacheManager::new();
        let initial_generation = cache.generation();

        let mut frame = cache.begin_frame();
        frame.mark_candidate(RendererCacheKind::Noop, false);
        frame.mark_candidate(RendererCacheKind::Noop, true);
        frame.admit_candidate(RendererCacheKind::Noop);
        assert!(frame.try_consume_new_payload_budget(RendererCacheKind::Noop));
        assert!(!frame.try_consume_new_payload_budget(RendererCacheKind::Noop));
        frame.record_miss(RendererCacheKind::Noop);
        frame.record_store(
            RendererCacheKind::Noop,
            128,
            RendererCachePayloadKind::CpuRaster,
            Duration::from_micros(20),
        );
        frame.record_hit(RendererCacheKind::Noop, Duration::from_micros(8));
        frame.record_eviction(RendererCacheKind::Noop, 128);

        let stats = cache.end_frame(frame);
        assert_eq!(stats.noop.candidates, 2);
        assert_eq!(stats.noop.visible_candidates, 1);
        assert_eq!(stats.noop.admitted, 1);
        assert_eq!(stats.noop.rejected, 1);
        assert_eq!(stats.noop.misses, 1);
        assert_eq!(stats.noop.stores, 1);
        assert_eq!(stats.noop.hits, 1);
        assert_eq!(stats.noop.evictions, 1);
        assert_eq!(stats.noop.current_entries, 0);
        assert_eq!(stats.noop.current_bytes, 0);
        assert_eq!(stats.noop.evicted_bytes, 128);
        assert_eq!(stats.noop.prepare_time, Duration::from_micros(20));
        assert_eq!(stats.noop.draw_hit_time, Duration::from_micros(8));

        cache.clear();
        assert_ne!(cache.generation(), initial_generation);
    }

    #[test]
    fn clean_subtree_key_separates_content_from_integer_placement() {
        let candidate = RenderCacheCandidate {
            kind: crate::render_scene::RenderCacheCandidateKind::CleanSubtree,
            stable_id: 42,
            content_generation: 7,
            bounds: GeometryRect {
                x: 0.0,
                y: 0.0,
                width: 120.2,
                height: 40.1,
            },
            children: Vec::new(),
        };
        let shifted_candidate = RenderCacheCandidate {
            bounds: GeometryRect {
                x: 300.0,
                y: -12.0,
                ..candidate.bounds
            },
            ..candidate.clone()
        };

        let key = CleanSubtreeContentKey::from_candidate(&candidate, 1.0, 99)
            .expect("valid candidate should produce a content key");
        let shifted_key = CleanSubtreeContentKey::from_candidate(&shifted_candidate, 1.0, 99)
            .expect("local x/y placement should not affect content key");
        assert_eq!(key, shifted_key);
        assert_eq!(key.width_px, 121);
        assert_eq!(key.height_px, 41);
        assert_eq!(key.byte_len(), Some(121 * 41 * 4));

        let moved_left = CleanSubtreePlacement::from_transform(Affine2::translation(-300.0, 0.0))
            .expect("integer move_x should be a reusable placement");
        let moved_right = CleanSubtreePlacement::from_transform(Affine2::translation(300.0, 0.0))
            .expect("integer move_x should be a reusable placement");
        assert_ne!(moved_left, moved_right);

        let next_generation = RenderCacheCandidate {
            content_generation: candidate.content_generation + 1,
            ..candidate.clone()
        };
        let next_key = CleanSubtreeContentKey::from_candidate(&next_generation, 1.0, 99)
            .expect("content generation should produce a valid key");
        assert_ne!(key, next_key);

        let different_scale = CleanSubtreeContentKey::from_candidate(&candidate, 2.0, 99)
            .expect("scale should be part of the content key");
        assert_ne!(key, different_scale);

        let different_resource_generation =
            CleanSubtreeContentKey::from_candidate(&candidate, 1.0, 100)
                .expect("resource generation should be part of the content key");
        assert_ne!(key, different_resource_generation);
    }

    #[test]
    fn clean_subtree_placement_rejects_fractional_and_non_translation_transforms() {
        assert_eq!(
            CleanSubtreePlacement::from_translation(10.5, 0.0),
            Err(CleanSubtreePlacementRejection::FractionalTranslation)
        );
        assert_eq!(
            CleanSubtreePlacement::from_translation(f32::NAN, 0.0),
            Err(CleanSubtreePlacementRejection::NonFiniteTranslation)
        );
        assert_eq!(
            CleanSubtreePlacement::from_transform(Affine2::scale(1.1, 1.1)),
            Err(CleanSubtreePlacementRejection::UnsupportedTransform)
        );
    }

    #[test]
    fn clean_subtree_cache_requires_repeated_visibility_and_tracks_eviction_stats() {
        let key_a = CleanSubtreeContentKey {
            stable_id: 1,
            content_generation: 1,
            width_px: 8,
            height_px: 4,
            scale_bits: 1.0_f32.to_bits(),
            resource_generation: 1,
        };
        let key_b = CleanSubtreeContentKey {
            stable_id: 2,
            ..key_a
        };
        let key_c = CleanSubtreeContentKey {
            stable_id: 3,
            ..key_a
        };

        let mut cache = RendererCacheManager::new();
        cache.configure_clean_subtree_limits_for_test(1, 256, 256);

        let mut first_frame = cache.begin_frame();
        assert_eq!(cache.mark_clean_subtree_visible(&mut first_frame, key_a), 1);
        assert_eq!(
            cache.try_store_clean_subtree_metadata(
                &mut first_frame,
                key_a,
                128,
                Duration::from_micros(5)
            ),
            Err(CleanSubtreeStoreRejection::AdmissionThreshold)
        );
        let first_stats = cache.end_frame(first_frame);
        assert_eq!(first_stats.clean_subtree.visible_candidates, 1);
        assert_eq!(first_stats.clean_subtree.rejected, 1);
        assert_eq!(first_stats.clean_subtree.stores, 0);

        let mut second_frame = cache.begin_frame();
        assert_eq!(
            cache.mark_clean_subtree_visible(&mut second_frame, key_a),
            2
        );
        assert_eq!(
            cache.try_store_clean_subtree_metadata(
                &mut second_frame,
                key_a,
                128,
                Duration::from_micros(7)
            ),
            Ok(())
        );
        let second_stats = cache.end_frame(second_frame);
        assert_eq!(second_stats.clean_subtree.admitted, 1);
        assert_eq!(second_stats.clean_subtree.stores, 1);
        assert_eq!(second_stats.clean_subtree.current_entries, 1);
        assert_eq!(second_stats.clean_subtree.current_bytes, 128);
        assert_eq!(cache.clean_subtree_entry_count(), 1);
        assert_eq!(cache.clean_subtree_total_bytes(), 128);

        let mut third_frame = cache.begin_frame();
        cache.mark_clean_subtree_visible(&mut third_frame, key_b);
        cache.mark_clean_subtree_visible(&mut third_frame, key_b);
        cache.mark_clean_subtree_visible(&mut third_frame, key_c);
        cache.mark_clean_subtree_visible(&mut third_frame, key_c);

        assert_eq!(
            cache.try_store_clean_subtree_metadata(
                &mut third_frame,
                key_b,
                160,
                Duration::from_micros(11)
            ),
            Ok(())
        );
        assert_eq!(
            cache.try_store_clean_subtree_metadata(
                &mut third_frame,
                key_c,
                160,
                Duration::from_micros(13)
            ),
            Err(CleanSubtreeStoreRejection::PayloadBudget)
        );

        let third_stats = cache.end_frame(third_frame);
        assert_eq!(third_stats.clean_subtree.visible_candidates, 4);
        assert_eq!(third_stats.clean_subtree.admitted, 2);
        assert_eq!(third_stats.clean_subtree.stores, 1);
        assert_eq!(third_stats.clean_subtree.evictions, 1);
        assert_eq!(third_stats.clean_subtree.rejected, 1);
        assert_eq!(third_stats.clean_subtree.evicted_bytes, 128);
        assert_eq!(third_stats.clean_subtree.current_entries, 1);
        assert_eq!(third_stats.clean_subtree.current_bytes, 160);

        cache.clear();
        assert_eq!(cache.clean_subtree_entry_count(), 0);
        assert_eq!(cache.clean_subtree_total_bytes(), 0);
    }

    #[test]
    fn renderer_cache_lifecycle_is_empty_when_no_cache_candidates_are_marked() {
        let timings = render_scene_graph_profiled(
            16,
            16,
            RenderScene {
                nodes: vec![RenderNode::Primitive(DrawPrimitive::Rect(
                    0.0, 0.0, 8.0, 8.0, 0xFF0000FF,
                ))],
            },
        );

        assert!(timings.renderer_cache.is_none());
    }

    #[test]
    fn cache_candidate_node_renders_children_as_direct_fallback() {
        let children = vec![RenderNode::Clip {
            clips: vec![ClipShape {
                rect: crate::tree::geometry::Rect {
                    x: 2.0,
                    y: 2.0,
                    width: 18.0,
                    height: 14.0,
                },
                radii: Some(CornerRadii {
                    tl: 4.0,
                    tr: 4.0,
                    br: 4.0,
                    bl: 4.0,
                }),
            }],
            children: vec![
                RenderNode::Primitive(DrawPrimitive::RoundedRect(
                    2.0, 2.0, 18.0, 14.0, 4.0, 0x2F80EDFF,
                )),
                RenderNode::Primitive(DrawPrimitive::TextWithFont(
                    5.0,
                    11.0,
                    "cache".to_string(),
                    8.0,
                    0xFFFFFFFF,
                    "default".to_string(),
                    700,
                    false,
                )),
            ],
        }];

        let direct = render_scene_graph_to_pixels(
            28,
            22,
            RenderScene {
                nodes: children.clone(),
            },
        );
        let candidate = render_scene_graph_to_pixels(
            28,
            22,
            RenderScene {
                nodes: vec![RenderNode::CacheCandidate(
                    crate::render_scene::RenderCacheCandidate {
                        kind: crate::render_scene::RenderCacheCandidateKind::CleanSubtree,
                        stable_id: 42,
                        content_generation: 7,
                        bounds: crate::tree::geometry::Rect {
                            x: 2.0,
                            y: 2.0,
                            width: 18.0,
                            height: 14.0,
                        },
                        children,
                    },
                )],
            },
        );

        assert_eq!(candidate, direct);
    }

    fn clean_subtree_test_children() -> Vec<RenderNode> {
        vec![
            RenderNode::Primitive(DrawPrimitive::Rect(0.0, 0.0, 22.0, 16.0, 0x2F80EDFF)),
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                4.0, 4.0, 14.0, 8.0, 3.0, 0xFFFFFFFF,
            )),
        ]
    }

    fn clean_subtree_test_candidate(children: Vec<RenderNode>) -> RenderCacheCandidate {
        clean_subtree_test_candidate_with_generation(children, 3)
    }

    fn clean_subtree_test_candidate_with_generation(
        children: Vec<RenderNode>,
        content_generation: u64,
    ) -> RenderCacheCandidate {
        RenderCacheCandidate {
            kind: RenderCacheCandidateKind::CleanSubtree,
            stable_id: 99,
            content_generation,
            bounds: crate::tree::geometry::Rect {
                x: 0.0,
                y: 0.0,
                width: 22.0,
                height: 16.0,
            },
            children,
        }
    }

    fn assert_cache_candidate_fallback_matches_direct(
        width: u32,
        height: u32,
        direct_node: RenderNode,
        candidate_node: RenderNode,
    ) -> RenderTimings {
        let direct = render_scene_graph_to_pixels(
            width,
            height,
            RenderScene {
                nodes: vec![direct_node],
            },
        );
        let (candidate, timings) = render_scene_graph_to_pixels_and_timings(
            width,
            height,
            RenderScene {
                nodes: vec![candidate_node],
            },
        );

        assert_eq!(candidate, direct);
        timings
    }

    fn translated_candidate_scene(content_generation: u64) -> RenderScene {
        RenderScene {
            nodes: vec![RenderNode::Transform {
                transform: Affine2::translation(8.0, 5.0),
                children: vec![RenderNode::CacheCandidate(
                    clean_subtree_test_candidate_with_generation(
                        clean_subtree_test_children(),
                        content_generation,
                    ),
                )],
            }],
        }
    }

    fn translated_direct_scene() -> RenderScene {
        RenderScene {
            nodes: vec![RenderNode::Transform {
                transform: Affine2::translation(8.0, 5.0),
                children: clean_subtree_test_children(),
            }],
        }
    }

    #[test]
    fn cache_candidate_traversal_records_eligible_direct_miss_stats() {
        let children = clean_subtree_test_children();
        let transform = Affine2::translation(8.0, 5.0);

        let timings = assert_cache_candidate_fallback_matches_direct(
            48,
            32,
            RenderNode::Transform {
                transform,
                children: children.clone(),
            },
            RenderNode::Transform {
                transform,
                children: vec![RenderNode::CacheCandidate(clean_subtree_test_candidate(
                    children,
                ))],
            },
        );

        let cache_stats = timings
            .renderer_cache
            .expect("eligible cache candidate should produce cache stats");
        assert_eq!(cache_stats.clean_subtree.candidates, 1);
        assert_eq!(cache_stats.clean_subtree.visible_candidates, 1);
        assert_eq!(cache_stats.clean_subtree.misses, 1);
        assert_eq!(cache_stats.clean_subtree.rejected, 0);
        assert_eq!(cache_stats.clean_subtree.stores, 0);
        assert_eq!(cache_stats.clean_subtree.current_entries, 0);
    }

    #[test]
    fn cache_candidate_traversal_rejects_fractional_translation_without_changing_pixels() {
        let children = clean_subtree_test_children();
        let transform = Affine2::translation(8.5, 5.0);

        let timings = assert_cache_candidate_fallback_matches_direct(
            48,
            32,
            RenderNode::Transform {
                transform,
                children: children.clone(),
            },
            RenderNode::Transform {
                transform,
                children: vec![RenderNode::CacheCandidate(clean_subtree_test_candidate(
                    children,
                ))],
            },
        );

        let cache_stats = timings
            .renderer_cache
            .expect("rejected cache candidate should produce cache stats");
        assert_eq!(cache_stats.clean_subtree.candidates, 1);
        assert_eq!(cache_stats.clean_subtree.visible_candidates, 1);
        assert_eq!(cache_stats.clean_subtree.misses, 0);
        assert_eq!(cache_stats.clean_subtree.rejected, 1);
        assert_eq!(cache_stats.clean_subtree.stores, 0);
    }

    #[test]
    fn cache_candidate_traversal_rejects_rotate_and_scale_without_changing_pixels() {
        let cases = [
            (
                RenderNode::Transform {
                    transform: Affine2::rotation_degrees(8.0),
                    children: clean_subtree_test_children(),
                },
                RenderNode::Transform {
                    transform: Affine2::rotation_degrees(8.0),
                    children: vec![RenderNode::CacheCandidate(clean_subtree_test_candidate(
                        clean_subtree_test_children(),
                    ))],
                },
            ),
            (
                RenderNode::Transform {
                    transform: Affine2::scale(1.08, 1.08),
                    children: clean_subtree_test_children(),
                },
                RenderNode::Transform {
                    transform: Affine2::scale(1.08, 1.08),
                    children: vec![RenderNode::CacheCandidate(clean_subtree_test_candidate(
                        clean_subtree_test_children(),
                    ))],
                },
            ),
        ];

        for (direct_node, candidate_node) in cases {
            let timings =
                assert_cache_candidate_fallback_matches_direct(48, 32, direct_node, candidate_node);
            let cache_stats = timings
                .renderer_cache
                .expect("rejected cache candidate should produce cache stats");
            assert_eq!(cache_stats.clean_subtree.candidates, 1);
            assert_eq!(cache_stats.clean_subtree.visible_candidates, 1);
            assert_eq!(cache_stats.clean_subtree.misses, 0);
            assert_eq!(cache_stats.clean_subtree.rejected, 1);
            assert_eq!(cache_stats.clean_subtree.stores, 0);
        }
    }

    #[test]
    fn cache_candidate_traversal_allows_root_alpha_as_composition_state() {
        let alpha = 0.72;
        let timings = assert_cache_candidate_fallback_matches_direct(
            48,
            32,
            RenderNode::Alpha {
                alpha,
                children: clean_subtree_test_children(),
            },
            RenderNode::Alpha {
                alpha,
                children: vec![RenderNode::CacheCandidate(clean_subtree_test_candidate(
                    clean_subtree_test_children(),
                ))],
            },
        );

        let cache_stats = timings
            .renderer_cache
            .expect("root alpha should not reject the cache candidate");
        assert_eq!(cache_stats.clean_subtree.candidates, 1);
        assert_eq!(cache_stats.clean_subtree.visible_candidates, 1);
        assert_eq!(cache_stats.clean_subtree.misses, 1);
        assert_eq!(cache_stats.clean_subtree.rejected, 0);
        assert_eq!(cache_stats.clean_subtree.stores, 0);
    }

    #[test]
    fn clean_subtree_cache_reuses_payload_across_root_alpha_changes() {
        let direct_scene = |alpha| RenderScene {
            nodes: vec![RenderNode::Alpha {
                alpha,
                children: clean_subtree_test_children(),
            }],
        };
        let candidate_scene = |alpha| RenderScene {
            nodes: vec![RenderNode::Alpha {
                alpha,
                children: vec![RenderNode::CacheCandidate(clean_subtree_test_candidate(
                    clean_subtree_test_children(),
                ))],
            }],
        };
        let mut renderer = SceneRenderer::new();

        let first_direct = render_scene_graph_to_pixels(48, 32, direct_scene(0.48));
        let (first_pixels, first_timings) = render_scene_graph_to_pixels_and_timings_with_renderer(
            &mut renderer,
            48,
            32,
            candidate_scene(0.48),
        );
        assert_eq!(first_pixels, first_direct);
        let first_stats = first_timings
            .renderer_cache
            .expect("first alpha candidate frame should produce cache stats");
        assert_eq!(first_stats.clean_subtree.misses, 1);
        assert_eq!(first_stats.clean_subtree.stores, 0);
        assert_eq!(first_stats.clean_subtree.rejected, 0);

        let second_direct = render_scene_graph_to_pixels(48, 32, direct_scene(0.72));
        let (second_pixels, second_timings) =
            render_scene_graph_to_pixels_and_timings_with_renderer(
                &mut renderer,
                48,
                32,
                candidate_scene(0.72),
            );
        assert_eq!(second_pixels, second_direct);
        let second_stats = second_timings
            .renderer_cache
            .expect("second alpha candidate frame should store a payload");
        assert_eq!(second_stats.clean_subtree.misses, 1);
        assert_eq!(second_stats.clean_subtree.stores, 1);
        assert_eq!(second_stats.clean_subtree.hits, 0);
        assert_eq!(second_stats.clean_subtree.current_entries, 1);

        let third_direct = render_scene_graph_to_pixels(48, 32, direct_scene(0.36));
        let (third_pixels, third_timings) = render_scene_graph_to_pixels_and_timings_with_renderer(
            &mut renderer,
            48,
            32,
            candidate_scene(0.36),
        );
        assert_eq!(third_pixels, third_direct);
        let third_stats = third_timings
            .renderer_cache
            .expect("third alpha candidate frame should hit the cached payload");
        assert_eq!(third_stats.clean_subtree.hits, 1);
        assert_eq!(third_stats.clean_subtree.misses, 0);
        assert_eq!(third_stats.clean_subtree.stores, 0);
        assert_eq!(third_stats.clean_subtree.current_entries, 1);
    }

    #[test]
    fn clean_subtree_cache_stores_after_repeated_visibility_and_hits_later_frames() {
        let direct = render_scene_graph_to_pixels(48, 32, translated_direct_scene());
        let mut renderer = SceneRenderer::new();

        let (first_pixels, first_timings) = render_scene_graph_to_pixels_and_timings_with_renderer(
            &mut renderer,
            48,
            32,
            translated_candidate_scene(3),
        );
        assert_eq!(first_pixels, direct);
        let first_stats = first_timings
            .renderer_cache
            .expect("first candidate frame should produce cache stats");
        assert_eq!(first_stats.clean_subtree.misses, 1);
        assert_eq!(first_stats.clean_subtree.stores, 0);
        assert_eq!(first_stats.clean_subtree.hits, 0);

        let (second_pixels, second_timings) =
            render_scene_graph_to_pixels_and_timings_with_renderer(
                &mut renderer,
                48,
                32,
                translated_candidate_scene(3),
            );
        assert_eq!(second_pixels, direct);
        let second_stats = second_timings
            .renderer_cache
            .expect("second candidate frame should produce cache stats");
        assert_eq!(second_stats.clean_subtree.misses, 1);
        assert_eq!(second_stats.clean_subtree.stores, 1);
        assert_eq!(second_stats.clean_subtree.hits, 0);
        assert_eq!(second_stats.clean_subtree.current_entries, 1);

        let (third_pixels, third_timings) = render_scene_graph_to_pixels_and_timings_with_renderer(
            &mut renderer,
            48,
            32,
            translated_candidate_scene(3),
        );
        assert_eq!(third_pixels, direct);
        let third_stats = third_timings
            .renderer_cache
            .expect("third candidate frame should produce cache stats");
        assert_eq!(third_stats.clean_subtree.hits, 1);
        assert_eq!(third_stats.clean_subtree.misses, 0);
        assert_eq!(third_stats.clean_subtree.stores, 0);
        assert_eq!(third_stats.clean_subtree.current_entries, 1);
        assert!(third_stats.clean_subtree.draw_hit_time > Duration::ZERO);
    }

    #[test]
    fn clean_subtree_cache_clear_and_content_generation_force_miss() {
        let direct = render_scene_graph_to_pixels(48, 32, translated_direct_scene());
        let mut renderer = SceneRenderer::new();

        render_scene_graph_to_pixels_and_timings_with_renderer(
            &mut renderer,
            48,
            32,
            translated_candidate_scene(3),
        );
        render_scene_graph_to_pixels_and_timings_with_renderer(
            &mut renderer,
            48,
            32,
            translated_candidate_scene(3),
        );

        let (_, hit_timings) = render_scene_graph_to_pixels_and_timings_with_renderer(
            &mut renderer,
            48,
            32,
            translated_candidate_scene(3),
        );
        assert_eq!(
            hit_timings
                .renderer_cache
                .as_ref()
                .expect("hit frame should produce cache stats")
                .clean_subtree
                .hits,
            1
        );

        let (new_generation_pixels, new_generation_timings) =
            render_scene_graph_to_pixels_and_timings_with_renderer(
                &mut renderer,
                48,
                32,
                translated_candidate_scene(4),
            );
        assert_eq!(new_generation_pixels, direct);
        let new_generation_stats = new_generation_timings
            .renderer_cache
            .expect("new generation should produce cache stats");
        assert_eq!(new_generation_stats.clean_subtree.hits, 0);
        assert_eq!(new_generation_stats.clean_subtree.misses, 1);

        renderer.renderer_cache.clear();
        let (after_clear_pixels, after_clear_timings) =
            render_scene_graph_to_pixels_and_timings_with_renderer(
                &mut renderer,
                48,
                32,
                translated_candidate_scene(3),
            );
        assert_eq!(after_clear_pixels, direct);
        let after_clear_stats = after_clear_timings
            .renderer_cache
            .expect("after clear should produce cache stats");
        assert_eq!(after_clear_stats.clean_subtree.hits, 0);
        assert_eq!(after_clear_stats.clean_subtree.misses, 1);
        assert_eq!(after_clear_stats.clean_subtree.current_entries, 0);
    }

    #[test]
    fn clean_subtree_cache_asset_generation_change_forces_miss() {
        let image_id = "clean_subtree_cache_asset_generation_change";
        let red = vec![
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ];
        cache_test_image(image_id, 2, 2, red);

        let image_children = || {
            vec![RenderNode::Primitive(DrawPrimitive::Image(
                0.0,
                0.0,
                12.0,
                12.0,
                image_id.to_string(),
                ImageFit::Cover,
                None,
            ))]
        };
        let image_scene = || RenderScene {
            nodes: vec![RenderNode::CacheCandidate(
                clean_subtree_test_candidate_with_generation(image_children(), 7),
            )],
        };

        let mut renderer = SceneRenderer::new();
        render_scene_graph_to_pixels_and_timings_with_renderer(
            &mut renderer,
            24,
            24,
            image_scene(),
        );
        render_scene_graph_to_pixels_and_timings_with_renderer(
            &mut renderer,
            24,
            24,
            image_scene(),
        );
        let (red_hit_pixels, red_hit_timings) =
            render_scene_graph_to_pixels_and_timings_with_renderer(
                &mut renderer,
                24,
                24,
                image_scene(),
            );
        assert_eq!(rgba_at(&red_hit_pixels, 24, 6, 6), (255, 0, 0, 255));
        assert_eq!(
            red_hit_timings
                .renderer_cache
                .as_ref()
                .expect("asset hit should produce cache stats")
                .clean_subtree
                .hits,
            1
        );

        let blue = vec![
            0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255,
        ];
        cache_test_image(image_id, 2, 2, blue);
        let (blue_pixels, blue_timings) = render_scene_graph_to_pixels_and_timings_with_renderer(
            &mut renderer,
            24,
            24,
            image_scene(),
        );
        assert_eq!(rgba_at(&blue_pixels, 24, 6, 6), (0, 0, 255, 255));
        let blue_stats = blue_timings
            .renderer_cache
            .expect("asset generation change should produce cache stats");
        assert_eq!(blue_stats.clean_subtree.hits, 0);
        assert_eq!(blue_stats.clean_subtree.misses, 1);

        remove_asset(image_id);
    }

    fn render_with_canvas_to_pixels(
        width: u32,
        height: u32,
        draw: impl FnOnce(&skia_safe::Canvas),
    ) -> Vec<u8> {
        let info = skia_safe::ImageInfo::new(
            (width as i32, height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut surface = skia_safe::surfaces::raster(&info, None, None)
            .expect("raster surface should be created for renderer test");
        let canvas = surface.canvas();
        canvas.clear(Color::TRANSPARENT);
        draw(canvas);

        let mut pixels = vec![0u8; (width * height * 4) as usize];
        surface.read_pixels(&info, pixels.as_mut_slice(), (width * 4) as usize, (0, 0));
        pixels
    }

    fn render_single_command_to_pixels(
        width: u32,
        height: u32,
        primitive: DrawPrimitive,
    ) -> Vec<u8> {
        render_commands_to_pixels(width, height, vec![primitive])
    }

    fn rgba_at(pixels: &[u8], width: u32, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let idx = ((y * width + x) * 4) as usize;
        (
            pixels[idx],
            pixels[idx + 1],
            pixels[idx + 2],
            pixels[idx + 3],
        )
    }

    #[test]
    fn image_loading_placeholder_uses_light_neutral_surface() {
        let pixels = render_single_command_to_pixels(
            80,
            60,
            DrawPrimitive::ImageLoading(0.0, 0.0, 80.0, 60.0),
        );
        let (r, g, b, a) = rgba_at(&pixels, 80, 4, 4);

        assert_eq!(a, 255);
        assert!(
            r >= 220 && g >= 220 && b >= 220,
            "expected loading placeholder to be a light neutral surface, got rgba({r}, {g}, {b}, {a})"
        );
    }

    #[test]
    fn image_failed_placeholder_uses_soft_error_surface() {
        let pixels = render_single_command_to_pixels(
            80,
            60,
            DrawPrimitive::ImageFailed(0.0, 0.0, 80.0, 60.0),
        );
        let (r, g, b, a) = rgba_at(&pixels, 80, 4, 4);

        assert_eq!(a, 255);
        assert!(
            r > g && r > b && g >= 200 && b >= 200,
            "expected failed placeholder to be a soft error surface, got rgba({r}, {g}, {b}, {a})"
        );
    }

    fn cache_test_image(id: &str, width: u32, height: u32, rgba_pixels: Vec<u8>) {
        let info = skia_safe::ImageInfo::new(
            (width as i32, height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let data = Data::new_copy(&rgba_pixels);
        let image = skia_safe::images::raster_from_data(&info, data, (width * 4) as usize)
            .expect("test image should be created from RGBA pixels");

        let mut cache = get_asset_cache()
            .lock()
            .expect("asset cache lock for test image insertion");
        let generation = bump_asset_cache_generation();
        cache.insert(
            id.to_string(),
            Arc::new(CachedAsset {
                kind: CachedAssetKind::Raster(image),
                width,
                height,
                generation,
            }),
        );
    }

    fn cache_test_svg_asset(id: &str, width: u32, height: u32, svg: &str) {
        let mut options = usvg::Options::default();
        options.fontdb_mut().load_system_fonts();

        let tree = usvg::Tree::from_data_nested(svg.as_bytes(), &options)
            .expect("test SVG should parse into a vector tree");
        assert_eq!(tree.size().width().ceil() as u32, width);
        assert_eq!(tree.size().height().ceil() as u32, height);

        insert_vector_asset(id, tree).expect("test SVG should insert into asset cache");
    }

    fn reset_vector_cache_test_state() {
        clear_rendered_vector_cache();
        reset_vector_rasterization_count();
    }

    fn vector_cache_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static VECTOR_CACHE_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

        VECTOR_CACHE_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("vector cache test lock")
    }

    fn max_alpha_in_region(pixels: &[u8], width: u32, x0: u32, y0: u32, x1: u32, y1: u32) -> u8 {
        let mut max_alpha = 0u8;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let idx = ((y * width + x) * 4 + 3) as usize;
                max_alpha = max_alpha.max(pixels[idx]);
            }
        }
        max_alpha
    }

    fn assert_close(actual: f32, expected: f32, label: &str) {
        assert!(
            approx_eq(actual, expected),
            "{} expected {:.6}, got {:.6}",
            label,
            expected,
            actual
        );
    }

    #[test]
    fn test_render_clip_scope_restores_before_following_sibling() {
        let pixels = render_scene_graph_to_pixels(
            40,
            10,
            RenderScene {
                nodes: vec![
                    RenderNode::Clip {
                        clips: vec![ClipShape {
                            rect: crate::tree::geometry::Rect {
                                x: 0.0,
                                y: 0.0,
                                width: 10.0,
                                height: 10.0,
                            },
                            radii: None,
                        }],
                        children: vec![RenderNode::Primitive(DrawPrimitive::Rect(
                            0.0, 0.0, 10.0, 10.0, 0xFF0000FF,
                        ))],
                    },
                    RenderNode::Primitive(DrawPrimitive::Rect(20.0, 0.0, 10.0, 10.0, 0x0000FFFF)),
                ],
            },
        );

        assert_eq!(rgba_at(&pixels, 40, 5, 5), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 40, 25, 5), (0, 0, 255, 255));
    }

    #[test]
    fn test_render_transform_scope_restores_before_following_sibling() {
        let pixels = render_scene_graph_to_pixels(
            50,
            10,
            RenderScene {
                nodes: vec![
                    RenderNode::Transform {
                        transform: Affine2::translation(10.0, 0.0),
                        children: vec![RenderNode::Primitive(DrawPrimitive::Rect(
                            0.0, 0.0, 10.0, 10.0, 0xFF0000FF,
                        ))],
                    },
                    RenderNode::Primitive(DrawPrimitive::Rect(20.0, 0.0, 10.0, 10.0, 0x0000FFFF)),
                ],
            },
        );

        assert_eq!(rgba_at(&pixels, 50, 15, 5), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 50, 25, 5), (0, 0, 255, 255));
        assert_eq!(rgba_at(&pixels, 50, 35, 5).3, 0);
    }

    #[test]
    fn test_render_alpha_scope_restores_before_following_sibling() {
        let pixels = render_scene_graph_to_pixels(
            40,
            10,
            RenderScene {
                nodes: vec![
                    RenderNode::Alpha {
                        alpha: 0.5,
                        children: vec![RenderNode::Primitive(DrawPrimitive::Rect(
                            0.0, 0.0, 10.0, 10.0, 0xFF0000FF,
                        ))],
                    },
                    RenderNode::Primitive(DrawPrimitive::Rect(20.0, 0.0, 10.0, 10.0, 0x0000FFFF)),
                ],
            },
        );

        let red = rgba_at(&pixels, 40, 5, 5);
        let blue = rgba_at(&pixels, 40, 25, 5);

        assert!(red.3 > 0 && red.3 < 255);
        assert_eq!(blue, (0, 0, 255, 255));
    }

    #[test]
    fn test_single_primitive_alpha_profile_avoids_layer() {
        let timings = render_scene_graph_profiled(
            40,
            24,
            RenderScene {
                nodes: vec![RenderNode::Alpha {
                    alpha: 0.5,
                    children: vec![RenderNode::Primitive(DrawPrimitive::RoundedRect(
                        4.0, 4.0, 24.0, 12.0, 4.0, 0x336699FF,
                    ))],
                }],
            },
        );
        let detail = timings
            .draw_detail
            .expect("profiled render should include draw detail");

        assert_eq!(detail.layer_detail.alpha_layers, 0);
        assert!(detail.rounded_rects > Duration::ZERO);
    }

    #[test]
    fn test_text_alpha_profile_avoids_layer() {
        let timings = render_scene_graph_profiled(
            120,
            32,
            RenderScene {
                nodes: vec![RenderNode::Alpha {
                    alpha: 0.5,
                    children: vec![RenderNode::Primitive(DrawPrimitive::TextWithFont(
                        8.0,
                        20.0,
                        "alpha text".to_string(),
                        16.0,
                        0x203040FF,
                        "default".to_string(),
                        400,
                        false,
                    ))],
                }],
            },
        );
        let detail = timings
            .draw_detail
            .expect("profiled render should include draw detail");

        assert_eq!(detail.layer_detail.alpha_layers, 0);
        assert!(detail.texts > Duration::ZERO);
    }

    #[test]
    fn test_group_alpha_profile_keeps_layer_for_overlap() {
        let timings = render_scene_graph_profiled(
            48,
            32,
            RenderScene {
                nodes: vec![RenderNode::Alpha {
                    alpha: 0.5,
                    children: vec![
                        RenderNode::Primitive(DrawPrimitive::Rect(
                            4.0, 4.0, 24.0, 20.0, 0x336699FF,
                        )),
                        RenderNode::Primitive(DrawPrimitive::Rect(
                            16.0, 8.0, 24.0, 20.0, 0xCC5544FF,
                        )),
                    ],
                }],
            },
        );
        let detail = timings
            .draw_detail
            .expect("profiled render should include draw detail");

        assert_eq!(detail.layer_detail.alpha_layers, 1);
        assert_eq!(detail.layer_detail.max_alpha_children, 2);
    }

    #[test]
    fn test_image_alpha_profile_keeps_layer_fallback() {
        let image_id = "test_image_alpha_profile_keeps_layer_fallback";
        cache_test_image(image_id, 1, 1, vec![255, 255, 255, 255]);

        let timings = render_scene_graph_profiled(
            16,
            16,
            RenderScene {
                nodes: vec![RenderNode::Alpha {
                    alpha: 0.5,
                    children: vec![RenderNode::Primitive(DrawPrimitive::Image(
                        2.0,
                        2.0,
                        8.0,
                        8.0,
                        image_id.to_string(),
                        ImageFit::Cover,
                        None,
                    ))],
                }],
            },
        );
        let detail = timings
            .draw_detail
            .expect("profiled render should include draw detail");

        assert_eq!(detail.layer_detail.alpha_layers, 1);
        assert_eq!(detail.layer_detail.max_alpha_children, 1);

        remove_asset(image_id);
    }

    #[test]
    fn test_clipped_single_primitive_alpha_preserves_clip_without_layer() {
        let scene = RenderScene {
            nodes: vec![RenderNode::Clip {
                clips: vec![ClipShape {
                    rect: crate::tree::geometry::Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 8.0,
                        height: 16.0,
                    },
                    radii: None,
                }],
                children: vec![RenderNode::Alpha {
                    alpha: 0.5,
                    children: vec![RenderNode::Primitive(DrawPrimitive::Rect(
                        0.0, 0.0, 16.0, 16.0, 0xFF0000FF,
                    ))],
                }],
            }],
        };

        let pixels = render_scene_graph_to_pixels(16, 16, scene.clone());
        assert!(rgba_at(&pixels, 16, 4, 8).3 > 0);
        assert_eq!(rgba_at(&pixels, 16, 12, 8).3, 0);

        let timings = render_scene_graph_profiled(16, 16, scene);
        let detail = timings
            .draw_detail
            .expect("profiled render should include draw detail");
        assert_eq!(detail.layer_detail.alpha_layers, 0);
    }

    #[test]
    fn test_nested_render_scopes_restore_before_following_sibling() {
        let pixels = render_scene_graph_to_pixels(
            60,
            10,
            RenderScene {
                nodes: vec![
                    RenderNode::Clip {
                        clips: vec![ClipShape {
                            rect: crate::tree::geometry::Rect {
                                x: 0.0,
                                y: 0.0,
                                width: 20.0,
                                height: 10.0,
                            },
                            radii: None,
                        }],
                        children: vec![RenderNode::Transform {
                            transform: Affine2::translation(10.0, 0.0),
                            children: vec![RenderNode::Alpha {
                                alpha: 0.5,
                                children: vec![RenderNode::Primitive(DrawPrimitive::Rect(
                                    0.0, 0.0, 10.0, 10.0, 0xFF0000FF,
                                ))],
                            }],
                        }],
                    },
                    RenderNode::Primitive(DrawPrimitive::Rect(30.0, 0.0, 10.0, 10.0, 0x0000FFFF)),
                ],
            },
        );

        let red = rgba_at(&pixels, 60, 15, 5);
        let blue = rgba_at(&pixels, 60, 35, 5);

        assert!(red.3 > 0 && red.3 < 255);
        assert_eq!(blue, (0, 0, 255, 255));
    }

    #[test]
    fn test_make_font_with_style_uses_stable_text_settings() {
        let font = make_font_with_style("default", 400, false, 16.0);

        assert!(font.is_subpixel());
        assert!(font.is_linear_metrics());
        assert!(!font.is_baseline_snap());
        assert_eq!(font.edging(), FontEdging::AntiAlias);
        assert_eq!(font.hinting(), FontHinting::Slight);
    }

    #[test]
    fn test_measure_text_visual_metrics_account_for_synthetic_overhang() {
        let font_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../priv/test_assets/Lobster-Regular.ttf");
        let data = std::fs::read(&font_path).expect("lobster font asset should exist");

        load_font("lobster-test", 400, false, &data).expect("lobster font should load");

        let metrics =
            measure_text_visual_metrics("lobster-test", 700, true, 22.0, "Asset Fonts 123");

        assert!(metrics.visual_width > metrics.advance);
    }

    fn point_in_rounded_rect(point: (f32, f32), rect: RectSpec, radius: f32) -> bool {
        let (px, py) = point;
        let RectSpec { x, y, w, h } = rect;
        if w <= 0.0 || h <= 0.0 {
            return false;
        }

        let r = radius.max(0.0).min((w * 0.5).min(h * 0.5));
        let left = x;
        let right = x + w;
        let top = y;
        let bottom = y + h;

        if px < left || px > right || py < top || py > bottom {
            return false;
        }

        if r <= 0.0 {
            return true;
        }

        if (px >= left + r && px <= right - r) || (py >= top + r && py <= bottom - r) {
            return true;
        }

        let cx = if px < left + r { left + r } else { right - r };
        let cy = if py < top + r { top + r } else { bottom - r };
        let dx = px - cx;
        let dy = py - cy;
        dx * dx + dy * dy <= r * r
    }

    fn point_in_inset_rounded_rect(
        point: (f32, f32),
        rect: RectSpec,
        radius: f32,
        inset: f32,
    ) -> bool {
        let RectSpec { x, y, w, h } = rect;
        let inset = inset.max(0.0);
        let inset_rect = RectSpec {
            x: x + inset,
            y: y + inset,
            w: (w - inset * 2.0).max(0.0),
            h: (h - inset * 2.0).max(0.0),
        };
        point_in_rounded_rect(point, inset_rect, (radius - inset).max(0.0))
    }

    #[test]
    fn test_outer_shadow_on_transparent_rect_keeps_center_transparent() {
        let pixels = render_single_command_to_pixels(
            48,
            48,
            DrawPrimitive::Shadow(12.0, 12.0, 24.0, 24.0, 0.0, 0.0, 6.0, 2.0, 0.0, 0xC75A5AFF),
        );

        assert_eq!(max_alpha_in_region(&pixels, 48, 20, 20, 27, 27), 0);
        assert!(
            max_alpha_in_region(&pixels, 48, 7, 20, 10, 27) > 0,
            "expected shadow halo outside the rect"
        );
    }

    #[test]
    fn test_outer_shadow_on_transparent_rounded_rect_keeps_center_transparent() {
        let pixels = render_single_command_to_pixels(
            48,
            48,
            DrawPrimitive::Shadow(12.0, 12.0, 24.0, 24.0, 0.0, 0.0, 6.0, 2.0, 8.0, 0xC75A5AFF),
        );

        assert_eq!(max_alpha_in_region(&pixels, 48, 20, 20, 27, 27), 0);
        assert!(
            max_alpha_in_region(&pixels, 48, 7, 20, 10, 27) > 0,
            "expected rounded shadow halo outside the rect"
        );
    }

    #[test]
    fn test_compute_image_fit_rects_contain_wide_frame() {
        let rects =
            compute_image_fit_rects(640.0, 420.0, 0.0, 0.0, 280.0, 120.0, ImageFit::Contain)
                .expect("contain fit rects");

        assert_close(rects.src_x, 0.0, "src_x");
        assert_close(rects.src_y, 0.0, "src_y");
        assert_close(rects.src_w, 640.0, "src_w");
        assert_close(rects.src_h, 420.0, "src_h");
        assert_close(rects.dst_w, 182.85715, "dst_w");
        assert_close(rects.dst_h, 120.0, "dst_h");
        assert_close(rects.dst_x, 48.571426, "dst_x");
        assert_close(rects.dst_y, 0.0, "dst_y");
    }

    #[test]
    fn test_compute_image_fit_rects_cover_wide_frame() {
        let rects = compute_image_fit_rects(640.0, 420.0, 0.0, 0.0, 280.0, 120.0, ImageFit::Cover)
            .expect("cover fit rects");

        assert_close(rects.src_x, 0.0, "src_x");
        assert_close(rects.src_y, 72.85715, "src_y");
        assert_close(rects.src_w, 640.0, "src_w");
        assert_close(rects.src_h, 274.2857, "src_h");
        assert_close(rects.dst_x, 0.0, "dst_x");
        assert_close(rects.dst_y, 0.0, "dst_y");
        assert_close(rects.dst_w, 280.0, "dst_w");
        assert_close(rects.dst_h, 120.0, "dst_h");
    }

    #[test]
    fn test_compute_image_fit_rects_contain_tall_frame() {
        let rects =
            compute_image_fit_rects(640.0, 420.0, 0.0, 0.0, 140.0, 240.0, ImageFit::Contain)
                .expect("contain fit rects");

        assert_close(rects.dst_x, 0.0, "dst_x");
        assert_close(rects.dst_y, 74.0625, "dst_y");
        assert_close(rects.dst_w, 140.0, "dst_w");
        assert_close(rects.dst_h, 91.875, "dst_h");
    }

    #[test]
    fn test_compute_image_fit_rects_cover_tall_frame() {
        let rects = compute_image_fit_rects(640.0, 420.0, 0.0, 0.0, 140.0, 240.0, ImageFit::Cover)
            .expect("cover fit rects");

        assert_close(rects.src_x, 197.5, "src_x");
        assert_close(rects.src_y, 0.0, "src_y");
        assert_close(rects.src_w, 245.0, "src_w");
        assert_close(rects.src_h, 420.0, "src_h");
        assert_close(rects.dst_w, 140.0, "dst_w");
        assert_close(rects.dst_h, 240.0, "dst_h");
    }

    #[test]
    fn test_compute_image_fit_rects_cover_square_frame() {
        let rects = compute_image_fit_rects(640.0, 420.0, 0.0, 0.0, 180.0, 180.0, ImageFit::Cover)
            .expect("cover fit rects");

        assert_close(rects.src_x, 110.0, "src_x");
        assert_close(rects.src_y, 0.0, "src_y");
        assert_close(rects.src_w, 420.0, "src_w");
        assert_close(rects.src_h, 420.0, "src_h");
        assert_close(rects.dst_w, 180.0, "dst_w");
        assert_close(rects.dst_h, 180.0, "dst_h");
    }

    #[test]
    fn test_compute_image_fit_rects_rejects_invalid_dimensions() {
        assert!(
            compute_image_fit_rects(0.0, 420.0, 0.0, 0.0, 180.0, 180.0, ImageFit::Contain)
                .is_none()
        );
        assert!(
            compute_image_fit_rects(640.0, 420.0, 0.0, 0.0, 0.0, 180.0, ImageFit::Contain)
                .is_none()
        );
        assert!(
            compute_image_fit_rects(640.0, 420.0, f32::NAN, 0.0, 180.0, 180.0, ImageFit::Cover)
                .is_none()
        );
    }

    #[test]
    fn test_cover_fit_avoids_edge_bleed_from_outside_crop() {
        let image_id = "test_cover_edge_bleed";

        // 8x4 image: red | green-center | blue
        let mut src = vec![0u8; 8 * 4 * 4];
        for y in 0..4u32 {
            for x in 0..8u32 {
                let i = ((y * 8 + x) * 4) as usize;
                let (r, g, b) = if x < 2 {
                    (255, 0, 0)
                } else if x < 6 {
                    (0, 220, 0)
                } else {
                    (0, 0, 255)
                };
                src[i] = r;
                src[i + 1] = g;
                src[i + 2] = b;
                src[i + 3] = 255;
            }
        }

        cache_test_image(image_id, 8, 4, src);

        // Square destination forces cover crop to center 4 columns (green-only).
        let pixels = render_commands_to_pixels(
            32,
            32,
            vec![DrawPrimitive::Image(
                8.0,
                8.0,
                15.0,
                15.0,
                image_id.to_string(),
                ImageFit::Cover,
                None,
            )],
        );

        for (x, y) in &[(8, 14), (8, 16), (8, 18), (22, 14), (22, 16), (22, 18)] {
            let (r, g, b, a) = rgba_at(&pixels, 32, *x, *y);
            assert!(
                g >= 150,
                "expected green edge pixel at ({}, {}) without crop bleed, got rgba({}, {}, {}, {})",
                x,
                y,
                r,
                g,
                b,
                a
            );
            assert!(
                r <= 90 && b <= 90,
                "expected low red/blue crop bleed at ({}, {}), got rgba({}, {}, {}, {})",
                x,
                y,
                r,
                g,
                b,
                a
            );
        }

        remove_asset(image_id);
    }

    #[test]
    fn test_repeat_fit_tiles_both_axes() {
        let image_id = "test_repeat_fit_tiles_both_axes";

        // 2x2 source pattern:
        // [red, green]
        // [blue, yellow]
        let src = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ];
        cache_test_image(image_id, 2, 2, src);

        let pixels = render_commands_to_pixels(
            8,
            8,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                8.0,
                8.0,
                image_id.to_string(),
                ImageFit::Repeat,
                None,
            )],
        );

        assert_eq!(rgba_at(&pixels, 8, 0, 0), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 8, 1, 0), (0, 255, 0, 255));
        assert_eq!(rgba_at(&pixels, 8, 0, 1), (0, 0, 255, 255));
        assert_eq!(rgba_at(&pixels, 8, 1, 1), (255, 255, 0, 255));

        // Repeat period is source dimensions (2x2)
        assert_eq!(rgba_at(&pixels, 8, 0, 0), rgba_at(&pixels, 8, 2, 0));
        assert_eq!(rgba_at(&pixels, 8, 0, 0), rgba_at(&pixels, 8, 0, 2));
        assert_eq!(rgba_at(&pixels, 8, 1, 1), rgba_at(&pixels, 8, 3, 3));

        remove_asset(image_id);
    }

    #[test]
    fn test_repeat_x_fit_tiles_horizontally_only() {
        let image_id = "test_repeat_x_fit_tiles_horizontally_only";

        let src = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ];
        cache_test_image(image_id, 2, 2, src);

        let pixels = render_commands_to_pixels(
            20,
            20,
            vec![DrawPrimitive::Image(
                4.0,
                5.0,
                8.0,
                8.0,
                image_id.to_string(),
                ImageFit::RepeatX,
                None,
            )],
        );

        // Horizontal repetition exists in the top tile row.
        assert_eq!(rgba_at(&pixels, 20, 4, 5), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 20, 6, 5), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 20, 5, 6), (255, 255, 0, 255));

        // Outside image height should be transparent (no Y repeat).
        assert_eq!(rgba_at(&pixels, 20, 5, 9).3, 0);
        assert_eq!(rgba_at(&pixels, 20, 11, 12).3, 0);

        remove_asset(image_id);
    }

    #[test]
    fn test_repeat_y_fit_tiles_vertically_only() {
        let image_id = "test_repeat_y_fit_tiles_vertically_only";

        let src = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ];
        cache_test_image(image_id, 2, 2, src);

        let pixels = render_commands_to_pixels(
            20,
            20,
            vec![DrawPrimitive::Image(
                4.0,
                5.0,
                8.0,
                8.0,
                image_id.to_string(),
                ImageFit::RepeatY,
                None,
            )],
        );

        // Vertical repetition exists in the left tile column.
        assert_eq!(rgba_at(&pixels, 20, 4, 5), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 20, 4, 7), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 20, 5, 8), (255, 255, 0, 255));

        // Outside image width should be transparent (no X repeat).
        assert_eq!(rgba_at(&pixels, 20, 7, 6).3, 0);
        assert_eq!(rgba_at(&pixels, 20, 12, 12).3, 0);

        remove_asset(image_id);
    }

    #[test]
    fn test_svg_cover_fit_draws_vector_asset() {
        let _guard = vector_cache_test_lock();
        let image_id = "test_svg_cover_fit_draws_vector_asset";
        let svg = r##"
            <svg xmlns="http://www.w3.org/2000/svg" width="8" height="4" viewBox="0 0 8 4">
                <rect x="0" y="0" width="4" height="4" fill="#ff0000"/>
                <rect x="4" y="0" width="4" height="4" fill="#00ff00"/>
            </svg>
        "##;

        cache_test_svg_asset(image_id, 8, 4, svg);

        let pixels = render_commands_to_pixels(
            4,
            4,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                4.0,
                4.0,
                image_id.to_string(),
                ImageFit::Cover,
                None,
            )],
        );

        assert_eq!(rgba_at(&pixels, 4, 0, 2), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 4, 3, 2), (0, 255, 0, 255));

        remove_asset(image_id);
    }

    #[test]
    fn test_svg_repeat_fit_tiles_both_axes() {
        let _guard = vector_cache_test_lock();
        let image_id = "test_svg_repeat_fit_tiles_both_axes";
        let svg = r##"
            <svg xmlns="http://www.w3.org/2000/svg" width="2" height="2" viewBox="0 0 2 2">
                <rect x="0" y="0" width="1" height="1" fill="#ff0000"/>
                <rect x="1" y="0" width="1" height="1" fill="#00ff00"/>
                <rect x="0" y="1" width="1" height="1" fill="#0000ff"/>
                <rect x="1" y="1" width="1" height="1" fill="#ffff00"/>
            </svg>
        "##;

        cache_test_svg_asset(image_id, 2, 2, svg);

        let pixels = render_commands_to_pixels(
            8,
            8,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                8.0,
                8.0,
                image_id.to_string(),
                ImageFit::Repeat,
                None,
            )],
        );

        assert_eq!(rgba_at(&pixels, 8, 0, 0), (255, 0, 0, 255));
        assert_eq!(rgba_at(&pixels, 8, 1, 0), (0, 255, 0, 255));
        assert_eq!(rgba_at(&pixels, 8, 0, 1), (0, 0, 255, 255));
        assert_eq!(rgba_at(&pixels, 8, 1, 1), (255, 255, 0, 255));
        assert_eq!(rgba_at(&pixels, 8, 0, 0), rgba_at(&pixels, 8, 2, 0));
        assert_eq!(rgba_at(&pixels, 8, 0, 0), rgba_at(&pixels, 8, 0, 2));

        remove_asset(image_id);
    }

    #[test]
    fn test_svg_cover_fit_template_tint_flattens_visible_pixels() {
        let _guard = vector_cache_test_lock();
        let image_id = "test_svg_cover_fit_template_tint_flattens_visible_pixels";
        let svg = r##"
            <svg xmlns="http://www.w3.org/2000/svg" width="2" height="2" viewBox="0 0 2 2">
                <rect x="0" y="0" width="1" height="1" fill="#ff0000"/>
                <rect x="1" y="0" width="1" height="1" fill="#00ff00"/>
                <rect x="0" y="1" width="1" height="1" fill="#0000ff"/>
                <rect x="1" y="1" width="1" height="1" fill="#ffff00"/>
            </svg>
        "##;

        cache_test_svg_asset(image_id, 2, 2, svg);

        let pixels = render_commands_to_pixels(
            8,
            8,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                8.0,
                8.0,
                image_id.to_string(),
                ImageFit::Cover,
                Some(0xFFFFFFFF),
            )],
        );

        assert_eq!(rgba_at(&pixels, 8, 1, 1), (255, 255, 255, 255));
        assert_eq!(rgba_at(&pixels, 8, 6, 1), (255, 255, 255, 255));
        assert_eq!(rgba_at(&pixels, 8, 1, 6), (255, 255, 255, 255));
        assert_eq!(rgba_at(&pixels, 8, 6, 6), (255, 255, 255, 255));

        remove_asset(image_id);
    }

    #[test]
    fn test_svg_repeat_fit_template_tint_flattens_tiled_pixels() {
        let _guard = vector_cache_test_lock();
        let image_id = "test_svg_repeat_fit_template_tint_flattens_tiled_pixels";
        let svg = r##"
            <svg xmlns="http://www.w3.org/2000/svg" width="2" height="2" viewBox="0 0 2 2">
                <rect x="0" y="0" width="1" height="1" fill="#ff0000"/>
                <rect x="1" y="0" width="1" height="1" fill="#00ff00"/>
                <rect x="0" y="1" width="1" height="1" fill="#0000ff"/>
                <rect x="1" y="1" width="1" height="1" fill="#ffff00"/>
            </svg>
        "##;

        cache_test_svg_asset(image_id, 2, 2, svg);

        let pixels = render_commands_to_pixels(
            8,
            8,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                8.0,
                8.0,
                image_id.to_string(),
                ImageFit::Repeat,
                Some(0x00FFFFFF),
            )],
        );

        assert_eq!(rgba_at(&pixels, 8, 0, 0), (0, 255, 255, 255));
        assert_eq!(rgba_at(&pixels, 8, 1, 0), (0, 255, 255, 255));
        assert_eq!(rgba_at(&pixels, 8, 0, 1), (0, 255, 255, 255));
        assert_eq!(rgba_at(&pixels, 8, 2, 2), (0, 255, 255, 255));

        remove_asset(image_id);
    }

    #[test]
    fn test_raster_template_tint_preserves_source_alpha_without_layer() {
        let image_id = "test_raster_template_tint_preserves_source_alpha_without_layer";
        cache_test_image(
            image_id,
            2,
            2,
            vec![
                255, 0, 0, 255, // opaque source
                0, 255, 0, 0, // transparent source
                0, 0, 255, 128, // semi-transparent source
                255, 255, 255, 64, // low-alpha source
            ],
        );

        let pixels = render_commands_to_pixels(
            2,
            2,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                2.0,
                2.0,
                image_id.to_string(),
                ImageFit::Cover,
                Some(0x112233FF),
            )],
        );

        assert_eq!(rgba_at(&pixels, 2, 0, 0), (17, 34, 51, 255));
        assert_eq!(rgba_at(&pixels, 2, 1, 0).3, 0);

        let semi = rgba_at(&pixels, 2, 0, 1);
        assert!((126..=129).contains(&semi.3));
        assert!((8..=9).contains(&semi.0));
        assert!((16..=18).contains(&semi.1));
        assert!((25..=27).contains(&semi.2));

        let low = rgba_at(&pixels, 2, 1, 1);
        assert!((62..=65).contains(&low.3));
        assert!((4..=5).contains(&low.0));
        assert!((8..=9).contains(&low.1));
        assert!((12..=13).contains(&low.2));

        remove_asset(image_id);
    }

    #[test]
    fn test_template_tint_profile_reports_direct_tint_without_layer() {
        let image_id = "test_template_tint_profile_reports_direct_tint_without_layer";
        cache_test_image(image_id, 1, 1, vec![255, 255, 255, 255]);

        let timings = render_scene_graph_profiled(
            4,
            4,
            RenderScene {
                nodes: vec![RenderNode::Primitive(DrawPrimitive::Image(
                    0.0,
                    0.0,
                    4.0,
                    4.0,
                    image_id.to_string(),
                    ImageFit::Cover,
                    Some(0x336699FF),
                ))],
            },
        );
        let detail = timings
            .draw_detail
            .expect("profiled render should include draw detail");

        assert_eq!(detail.layer_detail.tinted_image_layers, 0);
        assert_eq!(detail.image_details.len(), 1);
        assert!(detail.image_details[0].tinted);
        assert!(!detail.image_details[0].tint_layer_used);

        remove_asset(image_id);
    }

    #[test]
    fn test_svg_cover_fit_reuses_cached_rendered_variant() {
        let _guard = vector_cache_test_lock();
        let image_id = "test_svg_cover_fit_reuses_cached_rendered_variant";
        let svg = r##"
            <svg xmlns="http://www.w3.org/2000/svg" width="8" height="4" viewBox="0 0 8 4">
                <rect x="0" y="0" width="4" height="4" fill="#ff0000"/>
                <rect x="4" y="0" width="4" height="4" fill="#00ff00"/>
            </svg>
        "##;

        reset_vector_cache_test_state();
        cache_test_svg_asset(image_id, 8, 4, svg);

        let first = render_commands_to_pixels(
            4,
            4,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                4.0,
                4.0,
                image_id.to_string(),
                ImageFit::Cover,
                None,
            )],
        );
        let second = render_commands_to_pixels(
            4,
            4,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                4.0,
                4.0,
                image_id.to_string(),
                ImageFit::Cover,
                None,
            )],
        );

        assert_eq!(first, second);
        assert_eq!(vector_rasterization_count(), 1);
        assert_eq!(rendered_vector_cache_entry_count(), 1);

        remove_asset(image_id);
    }

    #[test]
    fn test_svg_different_sizes_cache_separate_variants() {
        let _guard = vector_cache_test_lock();
        let image_id = "test_svg_different_sizes_cache_separate_variants";
        let svg = r##"
            <svg xmlns="http://www.w3.org/2000/svg" width="8" height="4" viewBox="0 0 8 4">
                <rect x="0" y="0" width="8" height="4" fill="#00aaff"/>
            </svg>
        "##;

        reset_vector_cache_test_state();
        cache_test_svg_asset(image_id, 8, 4, svg);

        render_commands_to_pixels(
            4,
            4,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                4.0,
                4.0,
                image_id.to_string(),
                ImageFit::Cover,
                None,
            )],
        );
        render_commands_to_pixels(
            8,
            8,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                8.0,
                8.0,
                image_id.to_string(),
                ImageFit::Cover,
                None,
            )],
        );

        assert_eq!(vector_rasterization_count(), 2);
        assert_eq!(rendered_vector_cache_entry_count(), 2);

        remove_asset(image_id);
    }

    #[test]
    fn test_svg_repeat_fit_reuses_cached_tile_variant() {
        let _guard = vector_cache_test_lock();
        let image_id = "test_svg_repeat_fit_reuses_cached_tile_variant";
        let svg = r##"
            <svg xmlns="http://www.w3.org/2000/svg" width="2" height="2" viewBox="0 0 2 2">
                <rect x="0" y="0" width="1" height="1" fill="#ff0000"/>
                <rect x="1" y="0" width="1" height="1" fill="#00ff00"/>
                <rect x="0" y="1" width="1" height="1" fill="#0000ff"/>
                <rect x="1" y="1" width="1" height="1" fill="#ffff00"/>
            </svg>
        "##;

        reset_vector_cache_test_state();
        cache_test_svg_asset(image_id, 2, 2, svg);

        let first = render_commands_to_pixels(
            8,
            8,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                8.0,
                8.0,
                image_id.to_string(),
                ImageFit::Repeat,
                None,
            )],
        );
        let second = render_commands_to_pixels(
            8,
            8,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                8.0,
                8.0,
                image_id.to_string(),
                ImageFit::Repeat,
                None,
            )],
        );

        assert_eq!(first, second);
        assert_eq!(vector_rasterization_count(), 1);
        assert_eq!(rendered_vector_cache_entry_count(), 1);

        remove_asset(image_id);
    }

    #[test]
    fn test_svg_replacing_asset_id_invalidates_rendered_variants() {
        let _guard = vector_cache_test_lock();
        let image_id = "test_svg_replacing_asset_id_invalidates_rendered_variants";
        let red_svg = r##"
            <svg xmlns="http://www.w3.org/2000/svg" width="4" height="4" viewBox="0 0 4 4">
                <rect x="0" y="0" width="4" height="4" fill="#ff0000"/>
            </svg>
        "##;
        let blue_svg = r##"
            <svg xmlns="http://www.w3.org/2000/svg" width="4" height="4" viewBox="0 0 4 4">
                <rect x="0" y="0" width="4" height="4" fill="#0000ff"/>
            </svg>
        "##;

        reset_vector_cache_test_state();
        cache_test_svg_asset(image_id, 4, 4, red_svg);

        let red_pixels = render_commands_to_pixels(
            4,
            4,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                4.0,
                4.0,
                image_id.to_string(),
                ImageFit::Cover,
                None,
            )],
        );

        assert_eq!(vector_rasterization_count(), 1);
        assert_eq!(rendered_vector_cache_entry_count(), 1);
        assert_eq!(rgba_at(&red_pixels, 4, 1, 1), (255, 0, 0, 255));

        cache_test_svg_asset(image_id, 4, 4, blue_svg);
        assert_eq!(rendered_vector_cache_entry_count(), 0);

        let blue_pixels = render_commands_to_pixels(
            4,
            4,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                4.0,
                4.0,
                image_id.to_string(),
                ImageFit::Cover,
                None,
            )],
        );

        assert_eq!(vector_rasterization_count(), 2);
        assert_eq!(rendered_vector_cache_entry_count(), 1);
        assert_eq!(rgba_at(&blue_pixels, 4, 1, 1), (0, 0, 255, 255));

        remove_asset(image_id);
    }

    #[test]
    fn test_svg_large_variant_skips_render_cache() {
        let _guard = vector_cache_test_lock();
        let image_id = "test_svg_large_variant_skips_render_cache";
        let svg = r##"
            <svg xmlns="http://www.w3.org/2000/svg" width="513" height="513" viewBox="0 0 513 513">
                <rect x="0" y="0" width="513" height="513" fill="#ff5500"/>
            </svg>
        "##;

        reset_vector_cache_test_state();
        cache_test_svg_asset(image_id, 513, 513, svg);

        render_commands_to_pixels(
            513,
            513,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                513.0,
                513.0,
                image_id.to_string(),
                ImageFit::Repeat,
                None,
            )],
        );
        render_commands_to_pixels(
            513,
            513,
            vec![DrawPrimitive::Image(
                0.0,
                0.0,
                513.0,
                513.0,
                image_id.to_string(),
                ImageFit::Repeat,
                None,
            )],
        );

        assert_eq!(vector_rasterization_count(), 2);
        assert_eq!(rendered_vector_cache_entry_count(), 0);

        remove_asset(image_id);
    }

    #[test]
    fn test_fractional_scale_cover_border_has_no_dark_inner_hairline() {
        let image_id = "test_fractional_cover_hairline";

        // Opaque, high-contrast source color to make dark seams visible.
        let mut src = vec![0u8; 24 * 16 * 4];
        for px in src.chunks_exact_mut(4) {
            px[0] = 36;
            px[1] = 216;
            px[2] = 72;
            px[3] = 255;
        }
        cache_test_image(image_id, 24, 16, src);

        // Fractional geometry mirrors a 1.5x-scaled layout.
        // Choose values that keep the inner clip on half-pixel boundaries.
        let outer_x: f32 = 12.0;
        let outer_y: f32 = 8.0;
        let outer_w: f32 = 54.0;
        let outer_h: f32 = 38.0;
        let border: f32 = 1.5;
        let radius: f32 = 9.0;

        let inner_x = outer_x + border;
        let inner_y = outer_y + border;
        let inner_w = outer_w - border * 2.0;
        let inner_h = outer_h - border * 2.0;
        let inner_r = (radius - border).max(0.0);

        let render_scene = |background_color: u32| {
            render_with_canvas_to_pixels(80, 60, |canvas| {
                let outer_rect = Rect::from_xywh(outer_x, outer_y, outer_w, outer_h);
                let outer_rrect = RRect::new_rect_xy(outer_rect, radius, radius);

                let mut background_paint = Paint::default();
                background_paint.set_color(color_from_u32(background_color));
                background_paint.set_anti_alias(true);
                canvas.draw_rrect(outer_rrect, &background_paint);

                canvas.save();
                canvas.clip_rrect(outer_rrect, skia_safe::ClipOp::Intersect, true);

                let inner_rect = Rect::from_xywh(inner_x, inner_y, inner_w, inner_h);
                let expanded = snap_outset_rect_to_device(canvas, inner_rect);
                let (outset_x, outset_y) = rect_outset_amount(inner_rect, expanded);
                let expanded_radius = (inner_r + outset_x.max(outset_y)).max(0.0);
                let inner_rrect = RRect::new_rect_xy(expanded, expanded_radius, expanded_radius);

                canvas.save();
                canvas.clip_rrect(inner_rrect, skia_safe::ClipOp::Intersect, false);
                draw_cached_asset_with_fit(
                    canvas,
                    ImageDrawSpec {
                        rect: RectSpec {
                            x: inner_x,
                            y: inner_y,
                            w: inner_w,
                            h: inner_h,
                        },
                        image_id,
                        fit: ImageFit::Cover,
                        svg_tint: None,
                    },
                    0.0,
                );
                canvas.restore();

                draw_border(
                    canvas,
                    BorderDrawSpec {
                        rect: RectSpec {
                            x: outer_x,
                            y: outer_y,
                            w: outer_w,
                            h: outer_h,
                        },
                        corners: [radius, radius, radius, radius],
                        insets: EdgeInsets::uniform(border),
                        color: 0xD6DCECDD,
                        style: BorderStyle::Solid,
                    },
                );
                canvas.restore();
            })
        };

        let dark_bg_pixels = render_scene(0x05070BFF);
        let bright_bg_pixels = render_scene(0xF5E87AFF);
        let inner_rect = RectSpec {
            x: inner_x,
            y: inner_y,
            w: inner_w,
            h: inner_h,
        };

        let mut band_count = 0usize;
        let mut changed_count = 0usize;
        let mut max_channel_diff = 0u8;

        for y in 0..60u32 {
            for x in 0..80u32 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;

                // Only inspect a thin band *inside* the inner clip edge.
                let in_inner_near_edge =
                    point_in_inset_rounded_rect((px, py), inner_rect, inner_r, 0.05);
                let in_inner_deep =
                    point_in_inset_rounded_rect((px, py), inner_rect, inner_r, 1.25);

                if !in_inner_near_edge || in_inner_deep {
                    continue;
                }

                let (dr, dg, db, _da) = rgba_at(&dark_bg_pixels, 80, x, y);
                let (br, bg, bb, _ba) = rgba_at(&bright_bg_pixels, 80, x, y);

                let d_r = dr.abs_diff(br);
                let d_g = dg.abs_diff(bg);
                let d_b = db.abs_diff(bb);
                let local_max = d_r.max(d_g).max(d_b);
                max_channel_diff = max_channel_diff.max(local_max);

                band_count += 1;
                if local_max > 8 {
                    changed_count += 1;
                }
            }
        }

        assert!(band_count > 0, "expected non-empty inner edge band");
        assert!(
            max_channel_diff <= 8,
            "expected inner edge pixels to be background-invariant (no hairline leak), max channel diff was {}",
            max_channel_diff
        );
        assert!(
            changed_count <= 2,
            "expected <=2 significantly changed inner-edge pixels, got {} of {}",
            changed_count,
            band_count
        );

        remove_asset(image_id);
    }

    #[test]
    fn test_border_edge_clips_do_not_overlap_at_corners() {
        // Element at (10, 20) with size 200x100 and asymmetric widths
        let clips = border_edge_clip_quads(
            RectSpec {
                x: 10.0,
                y: 20.0,
                w: 200.0,
                h: 100.0,
            },
            EdgeInsets {
                top: 4.0,
                right: 1.0,
                bottom: 4.0,
                left: 1.0,
            },
        );

        // Sample points deep inside each corner quadrant of the element
        let top_left = (20.0, 25.0); // near top-left
        let top_right = (200.0, 25.0); // near top-right
        let bottom_right = (200.0, 115.0); // near bottom-right
        let bottom_left = (20.0, 115.0); // near bottom-left

        let corner_points = [top_left, top_right, bottom_right, bottom_left];

        for point in &corner_points {
            let mut hit_count = 0;
            for (width, quad) in &clips {
                if *width > 0.0 && point_in_convex_polygon(*point, quad) {
                    hit_count += 1;
                }
            }
            assert!(
                hit_count <= 1,
                "corner point {:?} is inside {} clip regions (expected at most 1)",
                point,
                hit_count,
            );
        }
    }

    #[test]
    fn test_border_edge_clips_cover_all_edge_midpoints() {
        let clips = border_edge_clip_quads(
            RectSpec {
                x: 10.0,
                y: 20.0,
                w: 200.0,
                h: 100.0,
            },
            EdgeInsets {
                top: 4.0,
                right: 1.0,
                bottom: 3.0,
                left: 2.0,
            },
        );

        // Midpoints of each edge (on the border, not inside)
        let edge_midpoints: [(f32, f32, &str); 4] = [
            (110.0, 20.0, "top"),     // top midpoint
            (210.0, 70.0, "right"),   // right midpoint
            (110.0, 120.0, "bottom"), // bottom midpoint
            (10.0, 70.0, "left"),     // left midpoint
        ];

        for (i, (px, py, label)) in edge_midpoints.iter().enumerate() {
            let (width, quad) = &clips[i];
            assert!(
                *width > 0.0 && point_in_convex_polygon((*px, *py), quad),
                "{} edge midpoint ({}, {}) should be inside clip region {}",
                label,
                px,
                py,
                i,
            );
        }
    }

    #[test]
    fn test_border_edge_clips_asymmetric_top_right_corner_prefers_top() {
        // Regression: thick top (4) and thin right (1) should keep near-corner
        // top-band pixels owned by the top edge clip.
        let clips = border_edge_clip_quads(
            RectSpec {
                x: 10.0,
                y: 20.0,
                w: 200.0,
                h: 100.0,
            },
            EdgeInsets {
                top: 4.0,
                right: 1.0,
                bottom: 4.0,
                left: 1.0,
            },
        );

        // Very close to the top-right outer corner, but still inside top band.
        let p = (209.0, 21.8);

        let top_hit = point_in_convex_polygon(p, &clips[0].1);
        let right_hit = point_in_convex_polygon(p, &clips[1].1);

        assert!(
            top_hit,
            "top clip should include near-corner top-band point"
        );
        assert!(
            !right_hit,
            "right clip should not steal near-corner top-band point"
        );
    }

    #[test]
    fn test_border_edge_clips_bottom_only_covers_near_corners() {
        // bottom-only border: top=0, right=0, bottom=3, left=0
        let clips = border_edge_clip_quads(
            RectSpec {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 50.0,
            },
            EdgeInsets {
                top: 0.0,
                right: 0.0,
                bottom: 3.0,
                left: 0.0,
            },
        );

        assert_eq!(clips[0].0, 0.0, "top width should be 0");
        assert_eq!(clips[1].0, 0.0, "right width should be 0");
        assert_eq!(clips[2].0, 3.0, "bottom width should be 3");
        assert_eq!(clips[3].0, 0.0, "left width should be 0");

        // Bottom midpoint and near-corner points should stay in bottom clip.
        let bottom_mid = (50.0, 50.0);
        let bottom_left = (1.0, 49.0);
        let bottom_right = (99.0, 49.0);
        assert!(
            point_in_convex_polygon(bottom_mid, &clips[2].1),
            "bottom edge midpoint should be inside the bottom clip region",
        );
        assert!(
            point_in_convex_polygon(bottom_left, &clips[2].1),
            "bottom-left near-corner point should be inside the bottom clip region",
        );
        assert!(
            point_in_convex_polygon(bottom_right, &clips[2].1),
            "bottom-right near-corner point should be inside the bottom clip region",
        );
    }

    #[test]
    fn test_solid_border_edges_asymmetric_keeps_top_right_corner_covered() {
        // Regression: with top=4 and right=1 on rounded corners, a point near
        // the outer top-right arc should remain filled (no corner gap).
        let pixels = render_single_command_to_pixels(
            160,
            100,
            DrawPrimitive::BorderEdges(
                20.0,
                20.0,
                100.0,
                40.0,
                8.0,
                4.0,
                1.0,
                4.0,
                1.0,
                0x78C8A0FF,
                BorderStyle::Solid,
            ),
        );

        // Near top-right arc location inside the border band.
        let corner_alpha = max_alpha_in_region(&pixels, 160, 116, 22, 117, 23);
        assert!(
            corner_alpha >= 96,
            "expected top-right arc coverage alpha >= 96, got {}",
            corner_alpha
        );

        // Ensure interior remains unfilled for border-only drawing.
        let interior_alpha = max_alpha_in_region(&pixels, 160, 60, 38, 61, 39);
        assert!(
            interior_alpha <= 8,
            "expected interior alpha <= 8, got {}",
            interior_alpha
        );
    }

    #[test]
    fn test_solid_single_edge_border_draws_only_requested_edge() {
        let pixels = render_single_command_to_pixels(
            72,
            48,
            DrawPrimitive::BorderEdges(
                12.0,
                10.0,
                40.0,
                24.0,
                0.0,
                0.0,
                3.0,
                0.0,
                0.0,
                0x335577FF,
                BorderStyle::Solid,
            ),
        );

        assert_eq!(rgba_at(&pixels, 72, 50, 22), (51, 85, 119, 255));
        assert_eq!(max_alpha_in_region(&pixels, 72, 14, 12, 44, 32), 0);
        assert_eq!(max_alpha_in_region(&pixels, 72, 12, 8, 52, 9), 0);
        assert_eq!(max_alpha_in_region(&pixels, 72, 53, 10, 55, 34), 0);
    }

    #[test]
    fn test_translucent_square_solid_border_does_not_overdraw_corners() {
        let pixels = render_single_command_to_pixels(
            72,
            48,
            DrawPrimitive::Border(
                12.0,
                10.0,
                40.0,
                24.0,
                0.0,
                4.0,
                0x33669980,
                BorderStyle::Solid,
            ),
        );

        let corner_alpha = rgba_at(&pixels, 72, 14, 12).3;
        let edge_alpha = rgba_at(&pixels, 72, 28, 12).3;
        assert!(
            (110..=140).contains(&corner_alpha),
            "expected single-pass translucent corner alpha, got {}",
            corner_alpha
        );
        assert!(
            (110..=140).contains(&edge_alpha),
            "expected single-pass translucent edge alpha, got {}",
            edge_alpha
        );
        assert_eq!(max_alpha_in_region(&pixels, 72, 20, 18, 44, 28), 0);
    }

    #[test]
    fn test_all_border_styles_stay_inside_border_box() {
        for style in [BorderStyle::Solid, BorderStyle::Dashed, BorderStyle::Dotted] {
            let pixels = render_single_command_to_pixels(
                180,
                120,
                DrawPrimitive::BorderEdges(
                    20.0, 20.0, 100.0, 40.0, 8.0, 4.0, 1.0, 4.0, 1.0, 0x78C8A0FF, style,
                ),
            );

            let outside_right_alpha = max_alpha_in_region(&pixels, 180, 121, 24, 123, 56);
            assert!(
                outside_right_alpha <= 8,
                "expected no paint outside right edge for {:?}, got alpha {}",
                style,
                outside_right_alpha
            );

            let outside_top_alpha = max_alpha_in_region(&pixels, 180, 24, 17, 116, 19);
            assert!(
                outside_top_alpha <= 8,
                "expected no paint outside top edge for {:?}, got alpha {}",
                style,
                outside_top_alpha
            );

            let interior_alpha = max_alpha_in_region(&pixels, 180, 68, 36, 72, 40);
            assert!(
                interior_alpha <= 8,
                "expected no interior fill for {:?}, got alpha {}",
                style,
                interior_alpha
            );
        }
    }
}
