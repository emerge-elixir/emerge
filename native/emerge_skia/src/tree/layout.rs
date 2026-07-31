//! Layout engine for Emerge element trees.
//!
//! Three-pass algorithm:
//! 0. Scale: Apply scale factor to all attributes
//! 1. Measurement (bottom-up): Compute intrinsic sizes
//! 2. Resolution (top-down): Assign frames with constraints

use super::animation::{
    AnimationFrameSamples, AnimationOverlayResult, AnimationRuntime, apply_sample_attrs,
    sample_animation_overlays, sample_animation_overlays_for_ids, scale_animation_spec,
};
use super::attrs::{
    AlignX, AlignY, Attrs, BorderWidth, Color, Font, Length, MouseOverAttrs, Padding, TextAlign,
    TextFragment, effective_scrollbar_x, effective_scrollbar_y,
};
use super::element::{
    Element, ElementKind, ElementTree, Frame, InheritedMeasureFontKey, IntrinsicMeasureCache,
    IntrinsicMeasureCacheKey, NearbyConstraintKind, NearbyMount, NearbySlot, NodeId, NodeIx,
    ResolveAttrs, ResolveAvailableSpaceKey, ResolveCache, ResolveCacheKey, ResolveConstraintKey,
    ResolveExtent, SubtreeMeasureAttrs, SubtreeMeasureCache, SubtreeMeasureCacheKey,
    TopologyDependencyKey,
};
use super::geometry::Rect;
use super::invalidation::TreeInvalidation;
use super::render::DEFAULT_TEXT_COLOR;
#[cfg(any(test, feature = "bench-diagnostics"))]
use super::render::{
    reset_render_traversal_diagnostics_for_benchmark,
    take_render_traversal_diagnostics_for_benchmark,
};
use super::text_layout::{TextLayoutStyle, layout_text_lines};
use crate::assets;
#[cfg(any(test, feature = "bench-diagnostics"))]
use crate::events::registry_builder::{
    reset_registry_build_diagnostics_for_benchmark, take_registry_build_diagnostics_for_benchmark,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

// =============================================================================
// Layout Types
// =============================================================================

/// Available space for layout, following elm-ui semantics.
/// More expressive than a simple f32 constraint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AvailableSpace {
    /// Definite constraint - a fixed amount of available space (px).
    Definite(f32),
    /// Minimize to content - use minimum size needed to fit content.
    /// Equivalent to elm-ui's `shrink` or `content` when space is tight.
    MinContent,
    /// Maximize to content - expand to fit all content without constraint.
    /// Equivalent to elm-ui's `content` when space is plentiful.
    MaxContent,
}

impl AvailableSpace {
    /// Convert to a definite f32 value, using the provided default for content modes.
    pub fn resolve(&self, default: f32) -> f32 {
        match self {
            AvailableSpace::Definite(px) => *px,
            AvailableSpace::MinContent => default,
            AvailableSpace::MaxContent => default,
        }
    }

    /// Check if this is a definite constraint.
    pub fn is_definite(&self) -> bool {
        matches!(self, AvailableSpace::Definite(_))
    }
}

impl From<f32> for AvailableSpace {
    fn from(value: f32) -> Self {
        AvailableSpace::Definite(value)
    }
}

/// Constraint passed down during layout resolution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constraint {
    pub width: AvailableSpace,
    pub height: AvailableSpace,
}

impl Constraint {
    /// Create a constraint with definite values (most common case).
    pub fn new(max_width: f32, max_height: f32) -> Self {
        Self {
            width: AvailableSpace::Definite(max_width),
            height: AvailableSpace::Definite(max_height),
        }
    }

    /// Create a constraint with custom available space.
    pub fn with_space(width: AvailableSpace, height: AvailableSpace) -> Self {
        Self { width, height }
    }

    /// Get max_width, resolving content modes to the provided default.
    pub fn max_width(&self, default: f32) -> f32 {
        self.width.resolve(default)
    }

    /// Get max_height, resolving content modes to the provided default.
    pub fn max_height(&self, default: f32) -> f32 {
        self.height.resolve(default)
    }
}

/// Intrinsic (natural) size computed during measurement pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct IntrinsicSize {
    pub width: f32,
    pub height: f32,
}

// MeasuredElement reserved for future layout caching.

// =============================================================================
// Text Measurement
// =============================================================================

/// Trait for measuring text dimensions.
pub trait TextMeasurer {
    /// Measure text with custom font and return (width, height).
    fn measure_with_font(
        &self,
        text: &str,
        font_size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> (f32, f32);

    /// Measure the visual width needed to paint the text without clipping.
    fn measure_visual_width_with_font(
        &self,
        text: &str,
        font_size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> f32 {
        self.measure_with_font(text, font_size, family, weight, italic)
            .0
    }

    fn measure_text_layout_with_font(
        &self,
        text: &str,
        font_size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> (f32, f32) {
        let width = self.measure_visual_width_with_font(text, font_size, family, weight, italic);
        let (_, height) = self.measure_with_font(text, font_size, family, weight, italic);
        (width, height)
    }

    /// Return (ascent, descent) for a given font configuration.
    fn font_metrics(&self, font_size: f32, family: &str, weight: u16, italic: bool) -> (f32, f32);
}

/// Default text measurer using Skia.
pub struct SkiaTextMeasurer;

impl TextMeasurer for SkiaTextMeasurer {
    fn measure_with_font(
        &self,
        text: &str,
        font_size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> (f32, f32) {
        use crate::renderer::make_font_with_style;

        let font = make_font_with_style(family, weight, italic, font_size);
        let (width, _bounds) = font.measure_str(text, None);
        let (_, metrics) = font.metrics();
        let height = metrics.ascent.abs() + metrics.descent;

        (width, height)
    }

    fn measure_visual_width_with_font(
        &self,
        text: &str,
        font_size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> f32 {
        use crate::renderer::measure_text_visual_metrics;

        measure_text_visual_metrics(family, weight, italic, font_size, text).visual_width
    }

    fn measure_text_layout_with_font(
        &self,
        text: &str,
        font_size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> (f32, f32) {
        use crate::renderer::make_font_with_style;

        let font = make_font_with_style(family, weight, italic, font_size);
        let metrics = crate::renderer::measure_text_visual_metrics_cached_with_font(
            &font, family, weight, italic, font_size, text,
        );
        let (_, font_metrics) = font.metrics();
        let height = font_metrics.ascent.abs() + font_metrics.descent;

        (metrics.visual_width, height)
    }

    fn font_metrics(&self, font_size: f32, family: &str, weight: u16, italic: bool) -> (f32, f32) {
        use crate::renderer::make_font_with_style;

        let font = make_font_with_style(family, weight, italic, font_size);
        let (_, metrics) = font.metrics();
        (metrics.ascent.abs(), metrics.descent)
    }
}

/// Font context inherited from ancestors during measurement and rendering.
#[derive(Clone, Debug, Default)]
pub struct FontContext {
    pub font_family: Option<String>,
    pub font_weight: Option<u16>,
    pub font_italic: Option<bool>,
    pub font_size: Option<f32>,
    pub font_color: Option<u32>,
    pub font_underline: Option<bool>,
    pub font_strike: Option<bool>,
    pub font_letter_spacing: Option<f32>,
    pub font_word_spacing: Option<f32>,
    pub text_align: Option<TextAlign>,
}

impl FontContext {
    /// Merge parent context with element's own attrs (element attrs win).
    pub fn merge_with_attrs(&self, attrs: &Attrs) -> FontContext {
        FontContext {
            font_family: attrs
                .font
                .as_ref()
                .map(|f| match f {
                    Font::Atom(s) | Font::String(s) => s.clone(),
                })
                .or_else(|| self.font_family.clone()),
            font_weight: attrs
                .font_weight
                .as_ref()
                .map(|w| parse_weight(&w.0))
                .or(self.font_weight),
            font_italic: attrs
                .font_style
                .as_ref()
                .map(|s| s.0 == "italic")
                .or(self.font_italic),
            font_size: attrs.font_size.map(|s| s as f32).or(self.font_size),
            font_color: attrs
                .font_color
                .as_ref()
                .map(color_to_u32)
                .or(self.font_color),
            font_underline: attrs.font_underline.or(self.font_underline),
            font_strike: attrs.font_strike.or(self.font_strike),
            font_letter_spacing: attrs
                .font_letter_spacing
                .map(|s| s as f32)
                .or(self.font_letter_spacing),
            font_word_spacing: attrs
                .font_word_spacing
                .map(|s| s as f32)
                .or(self.font_word_spacing),
            text_align: attrs.text_align.or(self.text_align),
        }
    }
}

fn measure_text_width_with_spacing<M: TextMeasurer>(
    measurer: &M,
    text: &str,
    font_size: f32,
    family: &str,
    weight: u16,
    italic: bool,
    spacing: (f32, f32),
) -> f32 {
    let (letter_spacing, word_spacing) = spacing;

    if text.is_empty() {
        return 0.0;
    }

    if letter_spacing == 0.0 && word_spacing == 0.0 {
        return measurer.measure_visual_width_with_font(text, font_size, family, weight, italic);
    }

    let mut total = 0.0;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        let glyph = ch.to_string();
        let (glyph_width, _glyph_height) =
            measurer.measure_with_font(&glyph, font_size, family, weight, italic);
        total += glyph_width;

        if chars.peek().is_some() {
            total += letter_spacing;
            if ch.is_whitespace() {
                total += word_spacing;
            }
        }
    }

    total
}

struct TextFontSpec<'a> {
    font_size: f32,
    family: &'a str,
    weight: u16,
    italic: bool,
}

fn multiline_text_layout<M: TextMeasurer>(
    measurer: &M,
    text: &str,
    font: TextFontSpec<'_>,
    spacing: (f32, f32),
    wrap_width: Option<f32>,
) -> crate::tree::text_layout::TextLayout {
    let (letter_spacing, word_spacing) = spacing;
    layout_text_lines(
        text,
        wrap_width,
        measurer.font_metrics(font.font_size, font.family, font.weight, font.italic),
        TextLayoutStyle {
            font_size: font.font_size,
            letter_spacing,
            word_spacing,
        },
        |ch| {
            measurer
                .measure_with_font(
                    &ch.to_string(),
                    font.font_size,
                    font.family,
                    font.weight,
                    font.italic,
                )
                .0
        },
    )
}

/// Convert a Color to u32 RGBA format.
fn color_to_u32(color: &Color) -> u32 {
    match color {
        Color::Rgb { r, g, b } => {
            ((*r as u32) << 24) | ((*g as u32) << 16) | ((*b as u32) << 8) | 0xFF
        }
        Color::Rgba { r, g, b, a } => {
            ((*r as u32) << 24) | ((*g as u32) << 16) | ((*b as u32) << 8) | (*a as u32)
        }
        Color::Named(name) => named_color(name),
    }
}

/// Map named colors to u32 RGBA values.
fn named_color(name: &str) -> u32 {
    match name {
        "white" => 0xFFFFFFFF,
        "black" => 0x000000FF,
        "red" => 0xFF0000FF,
        "green" => 0x00FF00FF,
        "blue" => 0x0000FFFF,
        "cyan" => 0x00FFFFFF,
        "magenta" => 0xFF00FFFF,
        "yellow" => 0xFFFF00FF,
        "orange" => 0xFFA500FF,
        "purple" => 0x800080FF,
        "pink" => 0xFFC0CBFF,
        "gray" | "grey" => 0x808080FF,
        "navy" => 0x000080FF,
        "teal" => 0x008080FF,
        _ => 0xFFFFFFFF,
    }
}

/// Parse font weight string to numeric value.
fn parse_weight(w: &str) -> u16 {
    match w {
        "bold" => 700,
        "normal" | "regular" => 400,
        "light" => 300,
        "thin" => 100,
        "extra_light" | "extralight" => 200,
        "medium" => 500,
        "semibold" | "semi_bold" => 600,
        "extrabold" | "extra_bold" => 800,
        "black" => 900,
        _ => w.parse().unwrap_or(400),
    }
}

/// Extract font info using inherited context for missing values.
pub fn font_info_with_inheritance(attrs: &Attrs, inherited: &FontContext) -> (String, u16, bool) {
    let family = attrs
        .font
        .as_ref()
        .map(|f| match f {
            Font::Atom(s) | Font::String(s) => s.clone(),
        })
        .or_else(|| inherited.font_family.clone())
        .unwrap_or_else(|| "default".to_string());

    let weight = attrs
        .font_weight
        .as_ref()
        .map(|w| parse_weight(&w.0))
        .or(inherited.font_weight)
        .unwrap_or(400);

    let italic = attrs
        .font_style
        .as_ref()
        .map(|s| s.0 == "italic")
        .or(inherited.font_italic)
        .unwrap_or(false);

    (family, weight, italic)
}

// =============================================================================
// Layout Engine
// =============================================================================

/// Main layout function: scale, measure, and resolve the tree.
pub fn layout_tree<M: TextMeasurer>(
    tree: &mut ElementTree,
    constraint: Constraint,
    scale: f32,
    measurer: &M,
) {
    layout_tree_with_context(tree, constraint, scale, measurer, &FontContext::default());
}

/// Layout using an explicit inherited font context for the root element.
pub fn layout_tree_with_context<M: TextMeasurer>(
    tree: &mut ElementTree,
    constraint: Constraint,
    scale: f32,
    measurer: &M,
    inherited: &FontContext,
) {
    let _ = layout_tree_with_context_and_animation(
        tree, constraint, scale, measurer, inherited, None, None,
    );
}

pub fn layout_tree_default_with_animation(
    tree: &mut ElementTree,
    constraint: Constraint,
    scale: f32,
    runtime: &AnimationRuntime,
    sample_time: Instant,
) -> bool {
    layout_tree_with_context_and_animation(
        tree,
        constraint,
        scale,
        &SkiaTextMeasurer,
        &FontContext::default(),
        Some(runtime),
        Some(sample_time),
    )
}

fn layout_tree_with_context_and_animation<M: TextMeasurer>(
    tree: &mut ElementTree,
    constraint: Constraint,
    scale: f32,
    measurer: &M,
    inherited: &FontContext,
    animation_runtime: Option<&AnimationRuntime>,
    sample_time: Option<Instant>,
) -> bool {
    tree.reset_layout_cache_stats();
    tree.reset_scroll_cache_context_for_layout();

    let Some(root_id) = tree.root_id() else {
        return false;
    };

    let animation_result = prepare_frame_attrs(tree, scale, animation_runtime, sample_time);
    run_layout_passes(
        tree,
        &root_id,
        constraint,
        measurer,
        inherited,
        &animation_result,
    );

    animation_result.active
}

#[derive(Clone, Debug)]
pub(crate) struct FrameAttrsPreparation {
    pub(crate) root_id: Option<NodeId>,
    pub(crate) animation_result: AnimationOverlayResult,
}

pub(crate) fn prepare_frame_attrs_for_update(
    tree: &mut ElementTree,
    scale: f32,
    animation_runtime: Option<&AnimationRuntime>,
    sample_time: Option<Instant>,
) -> FrameAttrsPreparation {
    tree.reset_layout_cache_stats();

    FrameAttrsPreparation {
        root_id: tree.root_id(),
        animation_result: prepare_frame_attrs(tree, scale, animation_runtime, sample_time),
    }
}

pub(crate) fn prepare_animation_frame_attrs_for_update(
    tree: &mut ElementTree,
    scale: f32,
    animation_runtime: &AnimationRuntime,
    sample_time: Option<Instant>,
) -> FrameAttrsPreparation {
    tree.reset_layout_cache_stats();
    tree.ensure_topology();
    tree.set_current_scale(scale);

    let active_ids = animation_runtime.active_node_ids();
    let frame_samples =
        sample_animation_overlays_for_ids(tree, animation_runtime, &active_ids, sample_time);
    let layout_scale_roots =
        prepare_active_attrs_for_frame(tree, scale, &active_ids, &frame_samples);
    let animation_result = frame_samples.result;
    mark_animation_refresh_effects_dirty(tree, &animation_result);
    if layout_scale_roots.is_empty() {
        apply_interaction_styles_for_ids(tree, &active_ids);
    } else {
        apply_interaction_styles_for_subtrees(tree, &layout_scale_roots);
        apply_interaction_styles_for_ids(tree, &active_ids);
    }

    FrameAttrsPreparation {
        root_id: tree.root_id(),
        animation_result,
    }
}

pub(crate) fn prepare_dirty_frame_attrs_for_update(
    tree: &mut ElementTree,
    scale: f32,
    animation_runtime: Option<&AnimationRuntime>,
    sample_time: Option<Instant>,
    dirty_ids: &[NodeId],
) -> FrameAttrsPreparation {
    prepare_dirty_frame_attrs_with_subtrees_for_update(
        tree,
        scale,
        animation_runtime,
        sample_time,
        dirty_ids,
        &[],
    )
}

pub(crate) fn prepare_dirty_frame_attrs_with_subtrees_for_update(
    tree: &mut ElementTree,
    scale: f32,
    animation_runtime: Option<&AnimationRuntime>,
    sample_time: Option<Instant>,
    dirty_ids: &[NodeId],
    dirty_subtree_roots: &[NodeId],
) -> FrameAttrsPreparation {
    tree.reset_layout_cache_stats();
    tree.ensure_topology();
    tree.set_current_scale(scale);

    let active_ids = animation_runtime
        .map(AnimationRuntime::active_node_ids)
        .unwrap_or_default();
    let frame_samples = animation_runtime
        .map(|runtime| sample_animation_overlays_for_ids(tree, runtime, &active_ids, sample_time))
        .unwrap_or_default();
    let prepared_ids = unique_frame_attr_prepare_ids(&active_ids, dirty_ids);
    let mut layout_scale_roots =
        prepare_active_attrs_for_frame(tree, scale, &prepared_ids, &frame_samples);

    for id in dirty_subtree_roots {
        if !layout_scale_roots.contains(id) {
            let inherited_scale =
                inherited_layout_scale_for_node(tree, id, scale, &frame_samples.samples);
            prepare_attrs_for_subtree(tree, *id, inherited_scale, &frame_samples.samples);
            layout_scale_roots.push(*id);
        }
    }

    let animation_result = frame_samples.result;

    mark_animation_refresh_effects_dirty(tree, &animation_result);
    if layout_scale_roots.is_empty() {
        apply_interaction_styles_for_ids(tree, &prepared_ids);
    } else {
        apply_interaction_styles_for_subtrees(tree, &layout_scale_roots);
        apply_interaction_styles_for_ids(tree, &prepared_ids);
    }

    FrameAttrsPreparation {
        root_id: tree.root_id(),
        animation_result,
    }
}

pub(crate) fn prepared_root_has_frame(
    tree: &ElementTree,
    preparation: &FrameAttrsPreparation,
) -> bool {
    preparation
        .root_id
        .and_then(|root_id| tree.get(&root_id).and_then(|element| element.layout.frame))
        .is_some()
}

fn prepare_frame_attrs(
    tree: &mut ElementTree,
    scale: f32,
    animation_runtime: Option<&AnimationRuntime>,
    sample_time: Option<Instant>,
) -> AnimationOverlayResult {
    tree.ensure_topology();
    tree.set_current_scale(scale);

    // Pass 0: Scale all attributes (base_attrs -> attrs with scale applied)
    let animation_result = prepare_attrs_for_frame(tree, scale, animation_runtime, sample_time);
    mark_animation_refresh_effects_dirty(tree, &animation_result);
    apply_interaction_styles(tree);

    animation_result
}

fn run_layout_passes<M: TextMeasurer>(
    tree: &mut ElementTree,
    root_id: &NodeId,
    constraint: Constraint,
    measurer: &M,
    inherited: &FontContext,
    animation_result: &AnimationOverlayResult,
) {
    mark_animation_layout_effects_dirty(tree, animation_result);
    tree.refresh_registry_subtree_affects_cache();
    let registry_geometry_before =
        (!tree.has_registry_refresh_damage()).then(|| capture_registry_geometry_snapshots(tree));

    // Pass 1: Measure (bottom-up) - uses pre-scaled attrs
    measure_element(tree, root_id, measurer, inherited, true);

    let root_constraint = tree
        .get(root_id)
        .map(|root| root_logical_constraint(&root.layout.effective, constraint))
        .unwrap_or(constraint);

    // Pass 2: Resolve (top-down) - uses pre-scaled attrs
    resolve_element(
        tree,
        root_id,
        ResolvePlacement {
            constraint: root_constraint,
            x: 0.0,
            y: 0.0,
            inherited,
            use_resolve_cache: true,
        },
        measurer,
    );

    if let Some(before) = registry_geometry_before {
        if registry_geometry_changed_since(tree, &before) {
            if !tree.has_registry_refresh_damage() {
                tree.mark_registry_refresh_dirty(root_id);
            }
        } else {
            tree.clear_registry_refresh_dirty();
        }
    }
}

pub(crate) fn mark_animation_effects_dirty_for_update(
    tree: &mut ElementTree,
    animation_result: &AnimationOverlayResult,
) {
    mark_animation_refresh_effects_dirty(tree, animation_result);
    mark_animation_layout_effects_dirty(tree, animation_result);
}

fn mark_animation_refresh_effects_dirty(
    tree: &mut ElementTree,
    animation_result: &AnimationOverlayResult,
) {
    for effect in &animation_result.effects {
        let registry_refresh = (effect.registry_refresh
            || effect.invalidation.requires_recompute())
            && tree.subtree_affects_registry(&effect.id);
        let refresh_invalidation = if effect.invalidation.requires_recompute() && !registry_refresh
        {
            TreeInvalidation::Paint
        } else {
            effect.invalidation
        };

        if effect.transform_only && refresh_invalidation == TreeInvalidation::Paint {
            tree.mark_transform_animation_refresh_dirty(&effect.id, registry_refresh);
            continue;
        }

        tree.mark_refresh_dirty_for_invalidation(&effect.id, refresh_invalidation);

        if registry_refresh {
            tree.mark_registry_refresh_dirty(&effect.id);
        }
    }
}

fn mark_animation_layout_effects_dirty(
    tree: &mut ElementTree,
    animation_result: &AnimationOverlayResult,
) {
    animation_result
        .effects
        .iter()
        .filter(|effect| effect.invalidation.requires_recompute())
        .for_each(|effect| {
            if effect.layout_scale_dirty {
                tree.mark_layout_scale_dirty_for_animation(&effect.id);
            } else {
                tree.mark_layout_dirty_for_invalidation(&effect.id, effect.invalidation);
            }
        });
}

/// Layout with default Skia text measurer.
pub fn layout_tree_default(tree: &mut ElementTree, constraint: Constraint, scale: f32) {
    layout_tree(tree, constraint, scale, &SkiaTextMeasurer);
}

// =============================================================================
// Pass 0: Scale Attributes
// =============================================================================

/// Apply scale factor to all elements, preserve runtime attrs, and overlay animations.
fn prepare_attrs_for_frame(
    tree: &mut ElementTree,
    scale: f32,
    animation_runtime: Option<&AnimationRuntime>,
    sample_time: Option<Instant>,
) -> AnimationOverlayResult {
    let frame_samples = sample_animation_overlays(tree, animation_runtime, sample_time);
    prepare_all_attrs_for_frame(tree, scale, &frame_samples.samples);
    frame_samples.result
}

fn prepare_all_attrs_for_frame(
    tree: &mut ElementTree,
    scale: f32,
    samples: &HashMap<NodeId, super::animation::AnimationSample>,
) {
    let root_id = tree.root_id();
    let root_layout_scale = root_id
        .and_then(|id| tree.get(&id))
        .and_then(|root| valid_layout_scale(frame_layout_scale(root, samples)));
    let flat_scale = (scale as f64 * root_layout_scale.unwrap_or(1.0)) as f32;
    let has_descendant_layout_scale = prepare_attrs_flat(tree, flat_scale, root_id, samples);

    if has_descendant_layout_scale && let Some(root_id) = root_id {
        prepare_attrs_for_subtree(tree, root_id, scale, samples);
    }
}

fn valid_layout_scale(scale: Option<f64>) -> Option<f64> {
    scale.filter(|scale| scale.is_finite() && *scale > 0.0)
}

fn prepare_attrs_flat(
    tree: &mut ElementTree,
    scale: f32,
    root_id: Option<NodeId>,
    samples: &HashMap<NodeId, super::animation::AnimationSample>,
) -> bool {
    let mut has_descendant_layout_scale = false;
    for element in tree.iter_nodes_mut() {
        let frame_attrs = frame_declared_attrs(element, samples);
        has_descendant_layout_scale |=
            root_id != Some(element.id) && valid_layout_scale(frame_attrs.layout_scale).is_some();
        let scale_factor = match element.lifecycle.ghost_capture_scale {
            Some(capture_scale) => scale / capture_scale.max(f32::EPSILON),
            None => scale,
        };
        element.layout.effective = scale_attrs(&frame_attrs, scale_factor);
        element.normalize_extracted_state();
    }
    has_descendant_layout_scale
}

fn prepare_active_attrs_for_frame(
    tree: &mut ElementTree,
    scale: f32,
    active_ids: &[NodeId],
    frame_samples: &AnimationFrameSamples,
) -> Vec<NodeId> {
    let layout_scale_roots: Vec<NodeId> = frame_samples
        .samples
        .iter()
        .filter_map(|(id, sample)| sample.attrs.layout_scale.is_some().then_some(*id))
        .collect();

    if tree
        .root_id()
        .is_some_and(|root_id| layout_scale_roots.contains(&root_id))
    {
        prepare_all_attrs_for_frame(tree, scale, &frame_samples.samples);
        return layout_scale_roots;
    }

    for id in &layout_scale_roots {
        let inherited_scale =
            inherited_layout_scale_for_node(tree, id, scale, &frame_samples.samples);
        prepare_attrs_for_subtree(tree, *id, inherited_scale, &frame_samples.samples);
    }

    for id in active_ids {
        let scale_factor =
            effective_layout_scale_for_node_with_samples(tree, id, scale, &frame_samples.samples);
        prepare_attrs_for_single_node(tree, id, scale_factor, &frame_samples.samples);
    }

    layout_scale_roots
}

fn unique_frame_attr_prepare_ids(active_ids: &[NodeId], dirty_ids: &[NodeId]) -> Vec<NodeId> {
    active_ids
        .iter()
        .chain(dirty_ids.iter())
        .copied()
        .fold(Vec::new(), |mut ids, id| {
            if !ids.contains(&id) {
                ids.push(id);
            }
            ids
        })
}

fn prepare_attrs_for_subtree(
    tree: &mut ElementTree,
    id: NodeId,
    inherited_scale: f32,
    samples: &HashMap<NodeId, super::animation::AnimationSample>,
) {
    if let Some(ix) = tree.ix_of(&id) {
        prepare_attrs_for_subtree_ix(tree, ix, inherited_scale, samples);
    }
}

fn prepare_attrs_for_subtree_ix(
    tree: &mut ElementTree,
    ix: NodeIx,
    inherited_scale: f32,
    samples: &HashMap<NodeId, super::animation::AnimationSample>,
) {
    let Some(next_scale) = prepare_attrs_for_node_ix(tree, ix, inherited_scale, samples) else {
        return;
    };

    let child_ixs = tree.child_ixs(ix);
    let nearby_ixs: Vec<NodeIx> = tree
        .nearby_ixs(ix)
        .into_iter()
        .map(|mount| mount.ix)
        .collect();

    for child_ix in child_ixs.into_iter().chain(nearby_ixs) {
        prepare_attrs_for_subtree_ix(tree, child_ix, next_scale, samples);
    }
}

fn prepare_attrs_for_node_ix(
    tree: &mut ElementTree,
    ix: NodeIx,
    inherited_scale: f32,
    samples: &HashMap<NodeId, super::animation::AnimationSample>,
) -> Option<f32> {
    let local_scale = tree
        .get_ix(ix)
        .and_then(|element| frame_layout_scale(element, samples))
        .and_then(|scale| valid_layout_scale(Some(scale)))
        .unwrap_or(1.0) as f32;
    let next_scale = (inherited_scale * local_scale).max(f32::EPSILON);
    let scale_factor = tree
        .get_ix(ix)
        .and_then(|element| element.lifecycle.ghost_capture_scale)
        .map(|capture_scale| next_scale / capture_scale.max(f32::EPSILON))
        .unwrap_or(next_scale);

    let element = tree.get_ix_mut(ix)?;
    let frame_attrs = frame_declared_attrs(element, samples);
    element.layout.effective = scale_attrs(&frame_attrs, scale_factor);
    element.normalize_extracted_state();

    Some(next_scale)
}

fn prepare_attrs_for_single_node(
    tree: &mut ElementTree,
    id: &NodeId,
    scale_factor: f32,
    samples: &HashMap<NodeId, super::animation::AnimationSample>,
) {
    if let Some(element) = tree.get_mut(id) {
        let frame_attrs = frame_declared_attrs(element, samples);
        element.layout.effective = scale_attrs(&frame_attrs, scale_factor);
        element.normalize_extracted_state();
    }
}

fn frame_declared_attrs(
    element: &Element,
    samples: &HashMap<NodeId, super::animation::AnimationSample>,
) -> Attrs {
    let mut attrs = element.spec.declared.clone();
    if let Some(sample) = samples.get(&element.id) {
        apply_sample_attrs(&mut attrs, &sample.attrs);
    }
    attrs
}

fn frame_layout_scale(
    element: &Element,
    samples: &HashMap<NodeId, super::animation::AnimationSample>,
) -> Option<f64> {
    samples
        .get(&element.id)
        .and_then(|sample| sample.attrs.layout_scale)
        .or(element.spec.declared.layout_scale)
}

pub(crate) fn effective_layout_scale_for_node(
    tree: &ElementTree,
    id: &NodeId,
    global_scale: f32,
) -> f32 {
    let Some(mut ix) = tree.ix_of(id) else {
        return global_scale;
    };

    let mut lineage = Vec::new();
    loop {
        lineage.push(ix);
        let Some(parent_ix) = tree
            .parent_link_of(ix)
            .and_then(|parent_link| super::element::parent_ix_from_link(Some(parent_link)))
        else {
            break;
        };
        ix = parent_ix;
    }

    lineage
        .into_iter()
        .rev()
        .fold((global_scale, global_scale), |(inherited, _factor), ix| {
            tree.get_ix(ix)
                .map(|element| {
                    let local = element
                        .spec
                        .declared
                        .layout_scale
                        .and_then(|scale| valid_layout_scale(Some(scale)))
                        .unwrap_or(1.0) as f32;
                    let current_total = (inherited * local).max(f32::EPSILON);
                    let factor = element
                        .lifecycle
                        .ghost_capture_scale
                        .map(|capture_scale| current_total / capture_scale.max(f32::EPSILON))
                        .unwrap_or(current_total);
                    (current_total, factor)
                })
                .unwrap_or((inherited, inherited))
        })
        .1
}

fn inherited_layout_scale_for_node(
    tree: &ElementTree,
    id: &NodeId,
    global_scale: f32,
    samples: &HashMap<NodeId, super::animation::AnimationSample>,
) -> f32 {
    let Some(mut ix) = tree.ix_of(id) else {
        return global_scale;
    };

    let mut lineage = Vec::new();
    while let Some(parent_ix) = tree
        .parent_link_of(ix)
        .and_then(|parent_link| super::element::parent_ix_from_link(Some(parent_link)))
    {
        lineage.push(parent_ix);
        ix = parent_ix;
    }

    lineage.into_iter().rev().fold(global_scale, |scale, ix| {
        tree.get_ix(ix)
            .map(|element| {
                let local = frame_layout_scale(element, samples)
                    .and_then(|scale| valid_layout_scale(Some(scale)))
                    .unwrap_or(1.0) as f32;
                (scale * local).max(f32::EPSILON)
            })
            .unwrap_or(scale)
    })
}

fn effective_layout_scale_for_node_with_samples(
    tree: &ElementTree,
    id: &NodeId,
    global_scale: f32,
    samples: &HashMap<NodeId, super::animation::AnimationSample>,
) -> f32 {
    let Some(mut ix) = tree.ix_of(id) else {
        return global_scale;
    };

    let mut lineage = Vec::new();
    loop {
        lineage.push(ix);
        let Some(parent_ix) = tree
            .parent_link_of(ix)
            .and_then(|parent_link| super::element::parent_ix_from_link(Some(parent_link)))
        else {
            break;
        };
        ix = parent_ix;
    }

    lineage
        .into_iter()
        .rev()
        .fold((global_scale, global_scale), |(inherited, _factor), ix| {
            tree.get_ix(ix)
                .map(|element| {
                    let local = frame_layout_scale(element, samples)
                        .and_then(|scale| valid_layout_scale(Some(scale)))
                        .unwrap_or(1.0) as f32;
                    let current_total = (inherited * local).max(f32::EPSILON);
                    let factor = element
                        .lifecycle
                        .ghost_capture_scale
                        .map(|capture_scale| current_total / capture_scale.max(f32::EPSILON))
                        .unwrap_or(current_total);
                    (current_total, factor)
                })
                .unwrap_or((inherited, inherited))
        })
        .1
}

/// Scale all pixel-based attributes in an Attrs struct.
fn scale_attrs(attrs: &Attrs, scale: f32) -> Attrs {
    let scale_f64 = scale as f64;
    Attrs {
        width: attrs.width.as_ref().map(|l| scale_length(l, scale)),
        height: attrs.height.as_ref().map(|l| scale_length(l, scale)),
        layout_scale: attrs.layout_scale,
        layout_rotate: attrs.layout_rotate,
        padding: attrs.padding.as_ref().map(|p| scale_padding(p, scale)),
        spacing: attrs.spacing.map(|s| s * scale_f64),
        spacing_x: attrs.spacing_x.map(|s| s * scale_f64),
        spacing_y: attrs.spacing_y.map(|s| s * scale_f64),
        align_x: attrs.align_x,
        align_y: attrs.align_y,
        scrollbar_y: attrs.scrollbar_y,
        scrollbar_x: attrs.scrollbar_x,
        ghost_scrollbar_y: attrs.ghost_scrollbar_y,
        ghost_scrollbar_x: attrs.ghost_scrollbar_x,
        #[cfg(test)]
        scrollbar_hover_axis: attrs.scrollbar_hover_axis,
        scroll_x: attrs.scroll_x.map(|v| v * scale_f64),
        scroll_y: attrs.scroll_y.map(|v| v * scale_f64),
        #[cfg(test)]
        scroll_x_max: None,
        #[cfg(test)]
        scroll_y_max: None,
        on_click: attrs.on_click,
        on_mouse_down: attrs.on_mouse_down,
        on_mouse_up: attrs.on_mouse_up,
        on_mouse_enter: attrs.on_mouse_enter,
        on_mouse_leave: attrs.on_mouse_leave,
        on_mouse_move: attrs.on_mouse_move,
        on_press: attrs.on_press,
        on_swipe_up: attrs.on_swipe_up,
        on_swipe_down: attrs.on_swipe_down,
        on_swipe_left: attrs.on_swipe_left,
        on_swipe_right: attrs.on_swipe_right,
        on_change: attrs.on_change,
        on_focus: attrs.on_focus,
        on_blur: attrs.on_blur,
        focus_on_mount: attrs.focus_on_mount,
        clip_nearby: attrs.clip_nearby,
        on_key_down: attrs.on_key_down.clone(),
        on_key_up: attrs.on_key_up.clone(),
        on_key_press: attrs.on_key_press.clone(),
        virtual_key: attrs.virtual_key.clone(),
        mouse_over: attrs
            .mouse_over
            .as_ref()
            .map(|hover| scale_mouse_over_attrs(hover, scale_f64)),
        focused: attrs
            .focused
            .as_ref()
            .map(|style| scale_mouse_over_attrs(style, scale_f64)),
        mouse_down: attrs
            .mouse_down
            .as_ref()
            .map(|style| scale_mouse_over_attrs(style, scale_f64)),
        #[cfg(test)]
        mouse_over_active: None,
        #[cfg(test)]
        mouse_down_active: None,
        #[cfg(test)]
        focused_active: None,
        #[cfg(test)]
        text_input_focused: None,
        #[cfg(test)]
        text_input_cursor: None,
        #[cfg(test)]
        text_input_selection_anchor: None,
        #[cfg(test)]
        text_input_preedit: None,
        #[cfg(test)]
        text_input_preedit_cursor: None,
        background: attrs.background.clone(),
        border_radius: attrs
            .border_radius
            .as_ref()
            .map(|r| scale_border_radius(r, scale_f64)),
        border_width: attrs
            .border_width
            .as_ref()
            .map(|w| scale_border_width(w, scale_f64)),
        border_style: attrs.border_style,
        border_color: attrs.border_color.clone(),
        box_shadows: attrs.box_shadows.as_ref().map(|shadows| {
            shadows
                .iter()
                .map(|s| super::attrs::BoxShadow {
                    offset_x: s.offset_x * scale_f64,
                    offset_y: s.offset_y * scale_f64,
                    blur: s.blur * scale_f64,
                    size: s.size * scale_f64,
                    color: s.color.clone(),
                    inset: s.inset,
                })
                .collect()
        }),
        font_size: attrs.font_size.map(|s| s * scale_f64),
        font_color: attrs.font_color.clone(),
        svg_color: attrs.svg_color.clone(),
        svg_expected: attrs.svg_expected,
        font: attrs.font.clone(),
        font_weight: attrs.font_weight.clone(),
        font_style: attrs.font_style.clone(),
        font_underline: attrs.font_underline,
        font_strike: attrs.font_strike,
        font_letter_spacing: attrs.font_letter_spacing.map(|s| s * scale_f64),
        font_word_spacing: attrs.font_word_spacing.map(|s| s * scale_f64),
        image_src: attrs.image_src.clone(),
        image_fit: attrs.image_fit,
        image_size: attrs
            .image_size
            .map(|(w, h)| (w * scale_f64, h * scale_f64)),
        slider_min: attrs.slider_min,
        slider_max: attrs.slider_max,
        slider_value: attrs.slider_value,
        slider_step: attrs.slider_step,
        video_target: attrs.video_target.clone(),
        text_align: attrs.text_align,
        content: attrs.content.clone(),
        #[cfg(test)]
        paragraph_fragments: None,
        snap_layout: attrs.snap_layout,
        snap_text_metrics: attrs.snap_text_metrics,
        move_x: attrs.move_x.map(|v| v * scale_f64),
        move_y: attrs.move_y.map(|v| v * scale_f64),
        rotate: attrs.rotate,
        scale: attrs.scale,
        alpha: attrs.alpha,
        animate: attrs
            .animate
            .as_ref()
            .map(|spec| scale_animation_spec(spec, scale_f64)),
        animate_enter: attrs
            .animate_enter
            .as_ref()
            .map(|spec| scale_animation_spec(spec, scale_f64)),
        animate_exit: attrs
            .animate_exit
            .as_ref()
            .map(|spec| scale_animation_spec(spec, scale_f64)),
        space_evenly: attrs.space_evenly,
    }
}

fn scale_mouse_over_attrs(attrs: &MouseOverAttrs, scale: f64) -> MouseOverAttrs {
    MouseOverAttrs {
        background: attrs.background.clone(),
        border_radius: attrs
            .border_radius
            .as_ref()
            .map(|radius| scale_border_radius(radius, scale)),
        border_width: attrs
            .border_width
            .as_ref()
            .map(|width| scale_border_width(width, scale)),
        border_style: attrs.border_style,
        border_color: attrs.border_color.clone(),
        box_shadows: attrs.box_shadows.as_ref().map(|shadows| {
            shadows
                .iter()
                .map(|shadow| super::attrs::BoxShadow {
                    offset_x: shadow.offset_x * scale,
                    offset_y: shadow.offset_y * scale,
                    blur: shadow.blur * scale,
                    size: shadow.size * scale,
                    color: shadow.color.clone(),
                    inset: shadow.inset,
                })
                .collect()
        }),
        font: attrs.font.clone(),
        font_weight: attrs.font_weight.clone(),
        font_style: attrs.font_style.clone(),
        font_color: attrs.font_color.clone(),
        svg_color: attrs.svg_color.clone(),
        font_size: attrs.font_size.map(|v| v * scale),
        font_underline: attrs.font_underline,
        font_strike: attrs.font_strike,
        font_letter_spacing: attrs.font_letter_spacing.map(|v| v * scale),
        font_word_spacing: attrs.font_word_spacing.map(|v| v * scale),
        text_align: attrs.text_align,
        move_x: attrs.move_x.map(|v| v * scale),
        move_y: attrs.move_y.map(|v| v * scale),
        rotate: attrs.rotate,
        scale: attrs.scale,
        alpha: attrs.alpha,
    }
}

fn apply_interaction_styles(tree: &mut ElementTree) {
    for element in tree.iter_nodes_mut() {
        apply_interaction_style_to_element(element);
    }
}

fn apply_interaction_styles_for_ids(tree: &mut ElementTree, ids: &[NodeId]) {
    for id in ids {
        if let Some(element) = tree.get_mut(id) {
            apply_interaction_style_to_element(element);
        }
    }
}

fn apply_interaction_styles_for_subtrees(tree: &mut ElementTree, ids: &[NodeId]) {
    for id in ids {
        if let Some(ix) = tree.ix_of(id) {
            apply_interaction_styles_for_subtree_ix(tree, ix);
        }
    }
}

fn apply_interaction_styles_for_subtree_ix(tree: &mut ElementTree, ix: NodeIx) {
    if let Some(element) = tree.get_ix_mut(ix) {
        apply_interaction_style_to_element(element);
    }

    let child_ixs = tree.child_ixs(ix);
    let nearby_ixs: Vec<NodeIx> = tree
        .nearby_ixs(ix)
        .into_iter()
        .map(|mount| mount.ix)
        .collect();

    for child_ix in child_ixs.into_iter().chain(nearby_ixs) {
        apply_interaction_styles_for_subtree_ix(tree, child_ix);
    }
}

fn apply_interaction_style_to_element(element: &mut Element) {
    if element.runtime.mouse_over_active
        && let Some(mouse_over) = element.layout.effective.mouse_over.clone()
    {
        apply_decorative_style(&mut element.layout.effective, &mouse_over);
    }

    if element.runtime.focused_active
        && let Some(focused) = element.layout.effective.focused.clone()
    {
        apply_decorative_style(&mut element.layout.effective, &focused);
    }

    if element.runtime.mouse_down_active
        && let Some(mouse_down) = element.layout.effective.mouse_down.clone()
    {
        apply_decorative_style(&mut element.layout.effective, &mouse_down);
    }
}

fn apply_decorative_style(attrs: &mut Attrs, style: &MouseOverAttrs) {
    if let Some(background) = style.background.clone() {
        attrs.background = Some(background);
    }
    if let Some(border_radius) = style.border_radius.clone() {
        attrs.border_radius = Some(border_radius);
    }
    if let Some(border_width) = style.border_width.clone() {
        attrs.border_width = Some(border_width);
    }
    if let Some(border_style) = style.border_style {
        attrs.border_style = Some(border_style);
    }
    if let Some(border_color) = style.border_color.clone() {
        attrs.border_color = Some(border_color);
    }
    if let Some(box_shadows) = style.box_shadows.clone() {
        attrs.box_shadows = Some(box_shadows);
    }
    if let Some(font) = style.font.clone() {
        attrs.font = Some(font);
    }
    if let Some(font_weight) = style.font_weight.clone() {
        attrs.font_weight = Some(font_weight);
    }
    if let Some(font_style) = style.font_style.clone() {
        attrs.font_style = Some(font_style);
    }
    if let Some(font_color) = style.font_color.clone() {
        attrs.font_color = Some(font_color);
    }
    if let Some(svg_color) = style.svg_color.clone() {
        attrs.svg_color = Some(svg_color);
    }
    if let Some(font_size) = style.font_size {
        attrs.font_size = Some(font_size);
    }
    if let Some(font_underline) = style.font_underline {
        attrs.font_underline = Some(font_underline);
    }
    if let Some(font_strike) = style.font_strike {
        attrs.font_strike = Some(font_strike);
    }
    if let Some(font_letter_spacing) = style.font_letter_spacing {
        attrs.font_letter_spacing = Some(font_letter_spacing);
    }
    if let Some(font_word_spacing) = style.font_word_spacing {
        attrs.font_word_spacing = Some(font_word_spacing);
    }
    if let Some(text_align) = style.text_align {
        attrs.text_align = Some(text_align);
    }
    if let Some(move_x) = style.move_x {
        attrs.move_x = Some(move_x);
    }
    if let Some(move_y) = style.move_y {
        attrs.move_y = Some(move_y);
    }
    if let Some(rotate) = style.rotate {
        attrs.rotate = Some(rotate);
    }
    if let Some(scale) = style.scale {
        attrs.scale = Some(scale);
    }
    if let Some(alpha) = style.alpha {
        attrs.alpha = Some(alpha);
    }
}

fn scale_border_width(width: &super::attrs::BorderWidth, scale: f64) -> super::attrs::BorderWidth {
    use super::attrs::BorderWidth;

    match width {
        BorderWidth::Uniform(value) => BorderWidth::Uniform(*value * scale),
        BorderWidth::Sides {
            top,
            right,
            bottom,
            left,
        } => BorderWidth::Sides {
            top: *top * scale,
            right: *right * scale,
            bottom: *bottom * scale,
            left: *left * scale,
        },
    }
}

fn scale_border_radius(
    radius: &super::attrs::BorderRadius,
    scale: f64,
) -> super::attrs::BorderRadius {
    use super::attrs::BorderRadius;

    match radius {
        BorderRadius::Uniform(value) => BorderRadius::Uniform(*value * scale),
        BorderRadius::Corners { tl, tr, br, bl } => BorderRadius::Corners {
            tl: *tl * scale,
            tr: *tr * scale,
            br: *br * scale,
            bl: *bl * scale,
        },
    }
}

/// Scale pixel values within a Length, recursively handling nested min/max.
fn scale_length(length: &Length, scale: f32) -> Length {
    let scale_f64 = scale as f64;
    match length {
        Length::Px(val) => Length::Px(*val * scale_f64),
        Length::Min(left, right) => Length::Min(
            Box::new(scale_length(left, scale)),
            Box::new(scale_length(right, scale)),
        ),
        Length::Max(left, right) => Length::Max(
            Box::new(scale_length(left, scale)),
            Box::new(scale_length(right, scale)),
        ),
        Length::Fill => Length::Fill,
        Length::Content => Length::Content,
        Length::FillWeighted(weight) => Length::FillWeighted(*weight),
    }
}

/// Scale padding values.
fn scale_padding(padding: &Padding, scale: f32) -> Padding {
    let scale_f64 = scale as f64;
    match padding {
        Padding::Uniform(val) => Padding::Uniform(*val * scale_f64),
        Padding::Sides {
            top,
            right,
            bottom,
            left,
        } => Padding::Sides {
            top: *top * scale_f64,
            right: *right * scale_f64,
            bottom: *bottom * scale_f64,
            left: *left * scale_f64,
        },
    }
}

// =============================================================================
// Pass 1: Measurement (Bottom-Up)
// =============================================================================

/// Measure an element and its children, computing intrinsic sizes.
/// Reads from pre-scaled attrs. Inherits font context from ancestors.
fn measure_element<M: TextMeasurer>(
    tree: &mut ElementTree,
    id: &NodeId,
    measurer: &M,
    inherited: &FontContext,
    use_subtree_cache: bool,
) -> IntrinsicSize {
    let Some((kind, attrs, measure_dirty, measure_descendant_dirty)) =
        tree.get(id).map(|element| {
            (
                element.spec.kind,
                element.layout.effective.clone(),
                element.layout.measure_dirty,
                element.layout.measure_descendant_dirty,
            )
        })
    else {
        return IntrinsicSize::default();
    };

    let element_context = inherited.merge_with_attrs(&attrs);
    let child_ids = tree.child_ids(id);
    let nearby_mounts = tree.nearby_mounts_for(id);
    let topology_key = tree.measure_topology_dependency_key_for(id);
    let subtree_cache_key =
        use_subtree_cache.then(|| subtree_measure_cache_key(kind, &attrs, inherited, topology_key));

    if !use_subtree_cache || measure_dirty {
        tree.record_layout_cache_stats(|stats| stats.record_subtree_measure_miss());
    } else if !measure_descendant_dirty
        && let Some(key) = subtree_cache_key.as_ref()
        && let Some(intrinsic) = try_reuse_subtree_measure_cache(tree, id, key)
    {
        return intrinsic;
    }

    // First measure all children with merged font context.
    let child_sizes: Vec<IntrinsicSize> = child_ids
        .iter()
        .map(|child_id| {
            restore_clean_subtree_measure_cache(tree, child_id, &element_context).unwrap_or_else(
                || {
                    measure_element(
                        tree,
                        child_id,
                        measurer,
                        &element_context,
                        use_subtree_cache,
                    )
                },
            )
        })
        .collect();

    for nearby_id in nearby_mounts.iter().map(|mount| mount.id) {
        if restore_clean_subtree_measure_cache(tree, &nearby_id, &element_context).is_none() {
            let _ = measure_element(
                tree,
                &nearby_id,
                measurer,
                &element_context,
                use_subtree_cache,
            );
        }
    }

    if use_subtree_cache
        && !measure_dirty
        && measure_descendant_dirty
        && let Some(key) = subtree_cache_key.as_ref()
        && let Some(intrinsic) = try_reuse_subtree_measure_cache(tree, id, key)
    {
        return intrinsic;
    }

    // Read from pre-scaled attrs
    let insets = LayoutInsets::from_attrs(&attrs);
    let spacing_x = spacing_x(&attrs);
    let spacing_y = spacing_y(&attrs);
    let cache_key = intrinsic_measure_cache_key(kind, &attrs, inherited);

    if let Some(key) = cache_key.as_ref()
        && let Some(intrinsic) = try_reuse_intrinsic_measure_cache(tree, id, key)
    {
        return intrinsic;
    }

    let intrinsic = match kind {
        ElementKind::Text | ElementKind::TextInput => {
            let content = attrs.content.as_deref().unwrap_or("");
            // Use inherited font context for missing values
            let font_size = attrs
                .font_size
                .map(|s| s as f32)
                .or(inherited.font_size)
                .unwrap_or(16.0);
            let (family, weight, italic) = font_info_with_inheritance(&attrs, inherited);
            let letter_spacing = attrs
                .font_letter_spacing
                .map(|s| s as f32)
                .or(inherited.font_letter_spacing)
                .unwrap_or(0.0);
            let word_spacing = attrs
                .font_word_spacing
                .map(|s| s as f32)
                .or(inherited.font_word_spacing)
                .unwrap_or(0.0);
            let (text_width, text_height) = if letter_spacing == 0.0 && word_spacing == 0.0 {
                measurer.measure_text_layout_with_font(content, font_size, &family, weight, italic)
            } else {
                (
                    measure_text_width_with_spacing(
                        measurer,
                        content,
                        font_size,
                        &family,
                        weight,
                        italic,
                        (letter_spacing, word_spacing),
                    ),
                    measurer
                        .measure_with_font(content, font_size, &family, weight, italic)
                        .1,
                )
            };
            IntrinsicSize {
                width: resolve_outer_intrinsic_length(
                    attrs.width.as_ref(),
                    text_width,
                    insets.horizontal(),
                ),
                height: resolve_outer_intrinsic_length(
                    attrs.height.as_ref(),
                    text_height,
                    insets.vertical(),
                ),
            }
        }

        ElementKind::Multiline => {
            let content = attrs.content.as_deref().unwrap_or("");
            let font_size = attrs
                .font_size
                .map(|s| s as f32)
                .or(inherited.font_size)
                .unwrap_or(16.0);
            let (family, weight, italic) = font_info_with_inheritance(&attrs, inherited);
            let letter_spacing = attrs
                .font_letter_spacing
                .map(|s| s as f32)
                .or(inherited.font_letter_spacing)
                .unwrap_or(0.0);
            let word_spacing = attrs
                .font_word_spacing
                .map(|s| s as f32)
                .or(inherited.font_word_spacing)
                .unwrap_or(0.0);
            let layout = multiline_text_layout(
                measurer,
                content,
                TextFontSpec {
                    font_size,
                    family: &family,
                    weight,
                    italic,
                },
                (letter_spacing, word_spacing),
                None,
            );
            IntrinsicSize {
                width: resolve_outer_intrinsic_length(
                    attrs.width.as_ref(),
                    layout.max_width,
                    insets.horizontal(),
                ),
                height: resolve_outer_intrinsic_length(
                    attrs.height.as_ref(),
                    layout.total_height,
                    insets.vertical(),
                ),
            }
        }

        ElementKind::Image | ElementKind::Video => {
            let (image_width, image_height) = if let Some((w, h)) = attrs.image_size {
                (w, h)
            } else if let Some(source) = attrs.image_src.as_ref() {
                assets::ensure_source(source);
                match assets::source_dimensions(source) {
                    Some((w, h)) => (w as f64, h as f64),
                    None => (64.0, 64.0),
                }
            } else {
                (0.0, 0.0)
            };

            IntrinsicSize {
                width: resolve_outer_intrinsic_length(
                    attrs.width.as_ref(),
                    image_width as f32,
                    insets.horizontal(),
                ),
                height: resolve_outer_intrinsic_length(
                    attrs.height.as_ref(),
                    image_height as f32,
                    insets.vertical(),
                ),
            }
        }

        ElementKind::El | ElementKind::None | ElementKind::Slider => {
            // Single child container: intrinsic = max child size + padding + border
            let max_child_width = child_sizes.iter().map(|s| s.width).fold(0.0, f32::max);
            let max_child_height = child_sizes.iter().map(|s| s.height).fold(0.0, f32::max);

            IntrinsicSize {
                width: resolve_outer_intrinsic_length(
                    attrs.width.as_ref(),
                    max_child_width,
                    insets.horizontal(),
                ),
                height: resolve_outer_intrinsic_length(
                    attrs.height.as_ref(),
                    max_child_height,
                    insets.vertical(),
                ),
            }
        }

        ElementKind::Row | ElementKind::WrappedRow => {
            // Row: sum widths + spacing + padding + border
            let total_spacing = if child_sizes.len() > 1 {
                spacing_x * (child_sizes.len() - 1) as f32
            } else {
                0.0
            };
            let sum_width: f32 = child_sizes.iter().map(|s| s.width).sum();
            let max_height = child_sizes.iter().map(|s| s.height).fold(0.0, f32::max);

            IntrinsicSize {
                width: resolve_outer_intrinsic_length(
                    attrs.width.as_ref(),
                    sum_width + total_spacing,
                    insets.horizontal(),
                ),
                height: resolve_outer_intrinsic_length(
                    attrs.height.as_ref(),
                    max_height,
                    insets.vertical(),
                ),
            }
        }

        ElementKind::Column | ElementKind::TextColumn => {
            // Column: sum heights + spacing + padding + border
            let total_spacing = if child_sizes.len() > 1 {
                spacing_y * (child_sizes.len() - 1) as f32
            } else {
                0.0
            };
            let max_width = child_sizes.iter().map(|s| s.width).fold(0.0, f32::max);
            let sum_height: f32 = child_sizes.iter().map(|s| s.height).sum();

            IntrinsicSize {
                width: resolve_outer_intrinsic_length(
                    attrs.width.as_ref(),
                    max_width,
                    insets.horizontal(),
                ),
                height: resolve_outer_intrinsic_length(
                    attrs.height.as_ref(),
                    sum_height + total_spacing,
                    insets.vertical(),
                ),
            }
        }

        ElementKind::Paragraph => {
            // Paragraph: sum child widths (unwrapped single-line), single line height
            let sum_width: f32 = child_sizes.iter().map(|s| s.width).sum();
            let max_height = child_sizes.iter().map(|s| s.height).fold(0.0, f32::max);

            IntrinsicSize {
                width: resolve_outer_intrinsic_length(
                    attrs.width.as_ref(),
                    sum_width,
                    insets.horizontal(),
                ),
                height: resolve_outer_intrinsic_length(
                    attrs.height.as_ref(),
                    max_height,
                    insets.vertical(),
                ),
            }
        }
    };

    // Store intrinsic size separately; resolve owns the retained frame positions.
    let measured_render_frame = Frame {
        x: 0.0,
        y: 0.0,
        width: intrinsic.width,
        height: intrinsic.height,
        content_width: intrinsic.width,
        content_height: intrinsic.height,
    };
    let measured_frame = layout_frame_for_rotation(measured_render_frame, &attrs);
    let intrinsic_measure_cache = cache_key.map(|key| {
        tree.record_layout_cache_stats(|stats| stats.record_intrinsic_measure_store());
        IntrinsicMeasureCache {
            key,
            frame: measured_frame,
            render_frame: measured_render_frame,
        }
    });
    let subtree_measure_cache = if use_subtree_cache {
        subtree_cache_key.map(|key| {
            tree.record_layout_cache_stats(|stats| stats.record_subtree_measure_store());
            SubtreeMeasureCache {
                key,
                frame: measured_frame,
                render_frame: measured_render_frame,
            }
        })
    } else {
        None
    };

    if let Some(element) = tree.get_mut(id) {
        element.layout.measured_frame = Some(measured_frame);
        element.layout.measured_render_frame =
            distinct_render_frame(measured_frame, measured_render_frame);
        element.layout.intrinsic_measure_cache = intrinsic_measure_cache;
        if use_subtree_cache {
            element.layout.subtree_measure_cache = subtree_measure_cache;
            element.layout.measure_dirty = false;
            element.layout.measure_descendant_dirty = false;
        }
    }

    IntrinsicSize {
        width: measured_frame.width,
        height: measured_frame.height,
    }
}

fn subtree_measure_cache_key(
    kind: ElementKind,
    attrs: &Attrs,
    inherited: &FontContext,
    topology: TopologyDependencyKey,
) -> SubtreeMeasureCacheKey {
    SubtreeMeasureCacheKey {
        kind,
        attrs: subtree_measure_attrs(attrs),
        inherited: inherited_measure_font_key(inherited),
        topology,
    }
}

fn subtree_measure_attrs(attrs: &Attrs) -> SubtreeMeasureAttrs {
    SubtreeMeasureAttrs {
        width: attrs.width.clone(),
        height: attrs.height.clone(),
        layout_scale: attrs.layout_scale,
        layout_rotate: attrs.layout_rotate,
        padding: attrs.padding.clone(),
        border_width: attrs.border_width.clone(),
        spacing: attrs.spacing,
        spacing_x: attrs.spacing_x,
        spacing_y: attrs.spacing_y,
        scrollbar_y: attrs.scrollbar_y,
        scrollbar_x: attrs.scrollbar_x,
        ghost_scrollbar_y: attrs.ghost_scrollbar_y,
        ghost_scrollbar_x: attrs.ghost_scrollbar_x,
        scroll_x: attrs.scroll_x,
        scroll_y: attrs.scroll_y,
        clip_nearby: attrs.clip_nearby,
        content: attrs.content.clone(),
        font_size: attrs.font_size,
        font: attrs.font.clone(),
        font_weight: attrs.font_weight.clone(),
        font_style: attrs.font_style.clone(),
        font_letter_spacing: attrs.font_letter_spacing,
        font_word_spacing: attrs.font_word_spacing,
        image_src: attrs.image_src.clone(),
        image_fit: attrs.image_fit,
        image_size: attrs.image_size,
        text_align: attrs.text_align,
        snap_layout: attrs.snap_layout,
        snap_text_metrics: attrs.snap_text_metrics,
        space_evenly: attrs.space_evenly,
        has_animation_attrs: attrs.animate.is_some()
            || attrs.animate_enter.is_some()
            || attrs.animate_exit.is_some(),
    }
}

fn inherited_measure_font_key(inherited: &FontContext) -> InheritedMeasureFontKey {
    InheritedMeasureFontKey {
        family: inherited.font_family.clone(),
        weight: inherited.font_weight,
        italic: inherited.font_italic,
        font_size: inherited.font_size,
        letter_spacing: inherited.font_letter_spacing,
        word_spacing: inherited.font_word_spacing,
    }
}

fn try_reuse_subtree_measure_cache(
    tree: &mut ElementTree,
    id: &NodeId,
    key: &SubtreeMeasureCacheKey,
) -> Option<IntrinsicSize> {
    let frame = tree
        .get(id)
        .and_then(|element| element.layout.subtree_measure_cache.as_ref())
        .filter(|cache| &cache.key == key)
        .map(|cache| (cache.frame, cache.render_frame));

    let Some((frame, render_frame)) = frame else {
        tree.record_layout_cache_stats(|stats| stats.record_subtree_measure_miss());
        return None;
    };

    tree.record_layout_cache_stats(|stats| stats.record_subtree_measure_hit());

    if let Some(element) = tree.get_mut(id) {
        element.layout.measured_frame = Some(frame);
        element.layout.measured_render_frame = distinct_render_frame(frame, render_frame);
        element.layout.measure_dirty = false;
        element.layout.measure_descendant_dirty = false;
    }

    Some(IntrinsicSize {
        width: frame.width,
        height: frame.height,
    })
}

fn restore_clean_subtree_measure_cache(
    tree: &mut ElementTree,
    id: &NodeId,
    inherited: &FontContext,
) -> Option<IntrinsicSize> {
    let (frame, render_frame) = tree.get(id).and_then(|element| {
        if element.layout.measure_dirty || element.layout.measure_descendant_dirty {
            return None;
        }

        let key = subtree_measure_cache_key(
            element.spec.kind,
            &element.layout.effective,
            inherited,
            tree.measure_topology_dependency_key_for(id),
        );
        let cache = element.layout.subtree_measure_cache.as_ref()?;
        (cache.key == key).then_some((cache.frame, cache.render_frame))
    })?;

    tree.record_layout_cache_stats(|stats| stats.record_subtree_measure_hit());

    if let Some(element) = tree.get_mut(id) {
        element.layout.measured_frame = Some(frame);
        element.layout.measured_render_frame = distinct_render_frame(frame, render_frame);
        element.layout.measure_dirty = false;
        element.layout.measure_descendant_dirty = false;
    }

    Some(IntrinsicSize {
        width: frame.width,
        height: frame.height,
    })
}

fn intrinsic_measure_cache_key(
    kind: ElementKind,
    attrs: &Attrs,
    inherited: &FontContext,
) -> Option<IntrinsicMeasureCacheKey> {
    match kind {
        ElementKind::Text | ElementKind::TextInput | ElementKind::Multiline => {
            let font_size = attrs
                .font_size
                .map(|s| s as f32)
                .or(inherited.font_size)
                .unwrap_or(16.0);
            let (family, weight, italic) = font_info_with_inheritance(attrs, inherited);
            let letter_spacing = attrs
                .font_letter_spacing
                .map(|s| s as f32)
                .or(inherited.font_letter_spacing)
                .unwrap_or(0.0);
            let word_spacing = attrs
                .font_word_spacing
                .map(|s| s as f32)
                .or(inherited.font_word_spacing)
                .unwrap_or(0.0);

            Some(IntrinsicMeasureCacheKey::Text {
                kind,
                content: attrs.content.clone(),
                width: attrs.width.clone(),
                height: attrs.height.clone(),
                padding: attrs.padding.clone(),
                border_width: attrs.border_width.clone(),
                family,
                weight,
                italic,
                font_size,
                letter_spacing,
                word_spacing,
            })
        }
        ElementKind::Image | ElementKind::Video => {
            let resolved_source_size = if attrs.image_size.is_none() {
                attrs.image_src.as_ref().and_then(|source| {
                    assets::ensure_source(source);
                    assets::source_dimensions(source)
                })
            } else {
                None
            };

            Some(IntrinsicMeasureCacheKey::Media {
                kind,
                width: attrs.width.clone(),
                height: attrs.height.clone(),
                padding: attrs.padding.clone(),
                border_width: attrs.border_width.clone(),
                image_src: attrs.image_src.clone(),
                image_size: attrs.image_size,
                resolved_source_size,
            })
        }
        _ => None,
    }
}

fn try_reuse_intrinsic_measure_cache(
    tree: &mut ElementTree,
    id: &NodeId,
    key: &IntrinsicMeasureCacheKey,
) -> Option<IntrinsicSize> {
    let frame = tree
        .get(id)
        .and_then(|element| element.layout.intrinsic_measure_cache.as_ref())
        .filter(|cache| &cache.key == key)
        .map(|cache| (cache.frame, cache.render_frame));

    let Some((frame, render_frame)) = frame else {
        tree.record_layout_cache_stats(|stats| stats.record_intrinsic_measure_miss());
        return None;
    };

    tree.record_layout_cache_stats(|stats| stats.record_intrinsic_measure_hit());

    if let Some(element) = tree.get_mut(id) {
        element.layout.measured_frame = Some(frame);
        element.layout.measured_render_frame = distinct_render_frame(frame, render_frame);
        element.layout.measure_dirty = false;
        element.layout.measure_descendant_dirty = false;
    }

    Some(IntrinsicSize {
        width: frame.width,
        height: frame.height,
    })
}

/// Resolve intrinsic length from attribute.
fn resolve_intrinsic_length(length: Option<&Length>, intrinsic: f32) -> f32 {
    match length {
        Some(Length::Px(px)) => *px as f32,
        Some(Length::Content) | None => intrinsic,
        Some(Length::Fill) | Some(Length::FillWeighted(_)) => intrinsic, // Will expand in resolve
        Some(Length::Min(left, right)) => resolve_intrinsic_length(Some(left), intrinsic)
            .min(resolve_intrinsic_length(Some(right), intrinsic)),
        Some(Length::Max(left, right)) => resolve_intrinsic_length(Some(left), intrinsic)
            .max(resolve_intrinsic_length(Some(right), intrinsic)),
    }
}

fn resolve_outer_intrinsic_length(length: Option<&Length>, content_size: f32, insets: f32) -> f32 {
    resolve_intrinsic_length(length, content_size + insets)
}

fn layout_rotate_degrees(attrs: &Attrs) -> Option<f32> {
    attrs
        .layout_rotate
        .map(|degrees| degrees as f32)
        .filter(|degrees| degrees.is_finite())
        .map(normalize_degrees)
        .filter(|degrees| degrees.abs() > f32::EPSILON)
}

fn normalize_degrees(degrees: f32) -> f32 {
    let mut normalized = degrees % 360.0;
    if normalized > 180.0 {
        normalized -= 360.0;
    } else if normalized <= -180.0 {
        normalized += 360.0;
    }
    normalized
}

fn quarter_turns(degrees: f32) -> Option<i32> {
    let normalized = normalize_degrees(degrees);
    let turns = (normalized / 90.0).round();
    ((normalized - turns * 90.0).abs() <= 0.0001).then_some(turns as i32)
}

fn rotated_aabb_size(width: f32, height: f32, degrees: f32) -> (f32, f32) {
    match quarter_turns(degrees).map(|turns| turns.rem_euclid(4)) {
        Some(0) => (width, height),
        Some(1 | 3) => (height, width),
        Some(2) => (width, height),
        _ => {
            let radians = degrees.to_radians();
            let sin = radians.sin().abs();
            let cos = radians.cos().abs();
            (cos * width + sin * height, sin * width + cos * height)
        }
    }
}

fn layout_frame_for_rotation(unrotated: Frame, attrs: &Attrs) -> Frame {
    let Some(degrees) = layout_rotate_degrees(attrs) else {
        return unrotated;
    };
    let (width, height) = rotated_aabb_size(unrotated.width, unrotated.height, degrees);
    let (content_width, content_height) =
        rotated_aabb_size(unrotated.content_width, unrotated.content_height, degrees);

    Frame {
        width,
        height,
        content_width,
        content_height,
        ..unrotated
    }
}

fn render_frame_inside_layout_frame(layout_frame: Frame, unrotated: Frame) -> Frame {
    Frame {
        x: layout_frame.x + (layout_frame.width - unrotated.width) / 2.0,
        y: layout_frame.y + (layout_frame.height - unrotated.height) / 2.0,
        ..unrotated
    }
}

fn distinct_render_frame(layout_frame: Frame, render_frame: Frame) -> Option<Frame> {
    (layout_frame != render_frame).then_some(render_frame)
}

fn root_logical_constraint(attrs: &Attrs, physical: Constraint) -> Constraint {
    let Some(degrees) = layout_rotate_degrees(attrs) else {
        return physical;
    };

    match quarter_turns(degrees).map(|turns| turns.rem_euclid(4)) {
        Some(1 | 3) => Constraint::with_space(physical.height, physical.width),
        _ => physical,
    }
}

// =============================================================================
// Pass 2: Resolution (Top-Down)
// =============================================================================

#[derive(Clone, Copy, Debug)]
struct ElementSizing {
    available_width: AvailableSpace,
    available_height: AvailableSpace,
    width: f32,
    height: f32,
}

fn resolve_element_sizing(
    kind: ElementKind,
    attrs: &Attrs,
    inherited: &FontContext,
    intrinsic: IntrinsicSize,
    constraint: Constraint,
    prefer_fill_width: bool,
    prefer_fill_height: bool,
) -> ElementSizing {
    // For text elements with non-Left alignment (direct or inherited), fill width.
    let text_should_fill_width = kind == ElementKind::Text
        && attrs.width.is_none()
        && attrs
            .text_align
            .or(inherited.text_align)
            .is_some_and(|align| align != TextAlign::Left);

    // Resolve final dimensions.
    // Use intrinsic size as default for content-based constraints.
    let available_width = if text_should_fill_width
        || prefer_fill_width
        || length_requests_fill(attrs.width.as_ref())
    {
        // Text with alignment should fill available width.
        constraint.width
    } else if kind == ElementKind::Paragraph && is_content_length(attrs.width.as_ref()) {
        // Paragraphs wrap text within parent's available width (like <p> in HTML).
        constraint.width
    } else if is_content_length(attrs.width.as_ref()) {
        AvailableSpace::MaxContent
    } else {
        constraint.width
    };

    let available_height = if prefer_fill_height || length_requests_fill(attrs.height.as_ref()) {
        constraint.height
    } else if is_content_length(attrs.height.as_ref()) {
        AvailableSpace::MaxContent
    } else {
        constraint.height
    };

    let effective_constraint = Constraint::with_space(available_width, available_height);
    let max_width = effective_constraint.max_width(intrinsic.width);
    let max_height = effective_constraint.max_height(intrinsic.height);

    // For text with alignment, use fill behavior for width.
    let width = if text_should_fill_width || prefer_fill_width {
        max_width
    } else {
        resolve_length(attrs.width.as_ref(), intrinsic.width, max_width)
    };
    let height = resolve_length(attrs.height.as_ref(), intrinsic.height, max_height);

    ElementSizing {
        available_width,
        available_height,
        width,
        height,
    }
}

fn length_requests_fill(length: Option<&Length>) -> bool {
    match length {
        Some(Length::Fill) | Some(Length::FillWeighted(_)) => true,
        Some(Length::Min(left, right)) | Some(Length::Max(left, right)) => {
            length_requests_fill(Some(left)) || length_requests_fill(Some(right))
        }
        _ => false,
    }
}

fn container_prefers_fill_width(
    tree: &ElementTree,
    kind: ElementKind,
    attrs: &Attrs,
    child_ids: &[NodeId],
    constraint: Constraint,
) -> bool {
    attrs.width.is_none()
        && constraint.width.is_definite()
        && matches!(kind, ElementKind::Column | ElementKind::TextColumn)
        && child_ids.iter().any(|child_id| {
            tree.get(child_id)
                .map(|child| length_requests_fill(child.layout.effective.width.as_ref()))
                .unwrap_or(false)
        })
}

fn container_prefers_fill_height(
    tree: &ElementTree,
    kind: ElementKind,
    attrs: &Attrs,
    child_ids: &[NodeId],
    constraint: Constraint,
) -> bool {
    attrs.height.is_none()
        && constraint.height.is_definite()
        && matches!(
            kind,
            ElementKind::Row | ElementKind::WrappedRow | ElementKind::El
        )
        && child_ids.iter().any(|child_id| {
            tree.get(child_id)
                .map(|child| length_requests_fill(child.layout.effective.height.as_ref()))
                .unwrap_or(false)
        })
}

#[derive(Clone, Copy, Debug)]
struct ContentRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

struct ResolvePassParams<'a> {
    id: &'a NodeId,
    attrs: &'a Attrs,
    child_ids: &'a [NodeId],
    content: ContentRect,
    insets: LayoutInsets,
    is_scrollable: bool,
    scroll_x_enabled: bool,
    scroll_y_enabled: bool,
    spacing_x: f32,
    spacing_y: f32,
    align_x: AlignX,
    align_y: AlignY,
    available_width: AvailableSpace,
    available_height: AvailableSpace,
    use_resolve_cache: bool,
}

fn resolve_el_kind<M: TextMeasurer>(
    tree: &mut ElementTree,
    params: &ResolvePassParams<'_>,
    element_context: &FontContext,
    measurer: &M,
) {
    if params.child_ids.is_empty() {
        return;
    }

    let (actual_cw, actual_ch) = resolve_el_children(
        tree,
        params.child_ids,
        params.content,
        ElChildrenOptions {
            parent_align_x: params.align_x,
            parent_align_y: params.align_y,
            scroll_x_enabled: params.scroll_x_enabled,
            scroll_y_enabled: params.scroll_y_enabled,
        },
        element_context,
        measurer,
        params.use_resolve_cache,
    );

    if actual_ch > params.content.height
        && !params.is_scrollable
        && length_allows_content_expansion(params.attrs.height.as_ref())
    {
        expand_frame_height_to_content(tree, params.id, actual_ch, params.insets);
        set_frame_content_width(tree, params.id, actual_cw, params.insets);
    } else {
        set_frame_content_size(tree, params.id, actual_cw, actual_ch, params.insets);
    }
}

fn resolve_slider_kind<M: TextMeasurer>(
    tree: &mut ElementTree,
    params: &ResolvePassParams<'_>,
    element_context: &FontContext,
    measurer: &M,
) {
    if params.child_ids.is_empty() {
        set_frame_content_size(
            tree,
            params.id,
            params.content.width,
            params.content.height,
            params.insets,
        );
        return;
    }

    let ratio = slider_ratio(params.attrs);
    let content_width = params.content.width.max(0.0);
    let content_height = params.content.height.max(0.0);
    let thumb_frame = params.child_ids.get(2).and_then(|thumb_id| {
        resolve_child_with_placement(
            tree,
            thumb_id,
            ResolvePlacement {
                constraint: Constraint::new(content_width, content_height),
                x: 0.0,
                y: 0.0,
                inherited: element_context,
                use_resolve_cache: params.use_resolve_cache,
            },
            measurer,
        )
        .map(|frame| (*thumb_id, frame))
    });
    let thumb_width = thumb_frame
        .as_ref()
        .map(|(_, frame)| frame.width.max(0.0))
        .unwrap_or(0.0);
    let track_x = params.content.x + thumb_width / 2.0;
    let track_width = (content_width - thumb_width).max(0.0);
    let filled_width = track_width * ratio;
    let mut max_child_height = thumb_frame
        .as_ref()
        .map(|(_, frame)| frame.height.max(0.0))
        .unwrap_or(0.0);

    if let Some(track_id) = params.child_ids.first() {
        force_child_width(tree, track_id, track_width);
        if let Some(frame) = resolve_child_with_placement(
            tree,
            track_id,
            ResolvePlacement {
                constraint: Constraint::new(track_width, content_height),
                x: 0.0,
                y: 0.0,
                inherited: element_context,
                use_resolve_cache: params.use_resolve_cache,
            },
            measurer,
        ) {
            max_child_height = max_child_height.max(frame.height);
            shift_subtree(
                tree,
                track_id,
                track_x - frame.x,
                params.content.y + (content_height - frame.height) / 2.0 - frame.y,
            );
        }
    }

    if let Some(filled_track_id) = params.child_ids.get(1) {
        force_child_width(tree, filled_track_id, filled_width);
        if let Some(frame) = resolve_child_with_placement(
            tree,
            filled_track_id,
            ResolvePlacement {
                constraint: Constraint::new(filled_width, content_height),
                x: 0.0,
                y: 0.0,
                inherited: element_context,
                use_resolve_cache: params.use_resolve_cache,
            },
            measurer,
        ) {
            max_child_height = max_child_height.max(frame.height);
            shift_subtree(
                tree,
                filled_track_id,
                track_x - frame.x,
                params.content.y + (content_height - frame.height) / 2.0 - frame.y,
            );
        }
    }

    if let Some((thumb_id, frame)) = thumb_frame {
        let thumb_center_x = track_x + filled_width;
        shift_subtree(
            tree,
            &thumb_id,
            thumb_center_x - frame.width / 2.0 - frame.x,
            params.content.y + (content_height - frame.height) / 2.0 - frame.y,
        );
    }

    set_frame_content_size(
        tree,
        params.id,
        content_width,
        content_height.max(max_child_height),
        params.insets,
    );
}

fn resolve_row_kind<M: TextMeasurer>(
    tree: &mut ElementTree,
    params: &ResolvePassParams<'_>,
    element_context: &FontContext,
    measurer: &M,
) {
    if params.child_ids.is_empty() {
        return;
    }

    let allow_fill_width = params.available_width.is_definite();
    let space_evenly = params.attrs.space_evenly.unwrap_or(false) && allow_fill_width;
    let (actual_cw, actual_ch) = resolve_row_children(
        tree,
        params.child_ids,
        params.content,
        RowChildrenOptions {
            spacing: params.spacing_x,
            allow_fill_width,
            space_evenly,
        },
        element_context,
        measurer,
        params.use_resolve_cache,
    );

    if actual_ch > params.content.height
        && !params.is_scrollable
        && length_allows_content_expansion(params.attrs.height.as_ref())
    {
        expand_frame_height_to_content(tree, params.id, actual_ch, params.insets);
        set_frame_content_width(tree, params.id, actual_cw, params.insets);
    } else {
        set_frame_content_size(tree, params.id, actual_cw, actual_ch, params.insets);
    }
}

fn resolve_wrapped_row_kind<M: TextMeasurer>(
    tree: &mut ElementTree,
    params: &ResolvePassParams<'_>,
    element_context: &FontContext,
    measurer: &M,
) {
    let actual_content_height = resolve_wrapped_row_children(
        tree,
        params.child_ids,
        params.content,
        WrappedRowChildrenOptions {
            spacing_x: params.spacing_x,
            spacing_y: params.spacing_y,
        },
        element_context,
        measurer,
        params.use_resolve_cache,
    );

    if actual_content_height > params.content.height
        && !params.is_scrollable
        && length_allows_content_expansion(params.attrs.height.as_ref())
    {
        expand_frame_height_to_content(tree, params.id, actual_content_height, params.insets);
    } else {
        set_frame_content_height(tree, params.id, actual_content_height, params.insets);
    }
}

fn resolve_column_kind<M: TextMeasurer>(
    tree: &mut ElementTree,
    params: &ResolvePassParams<'_>,
    element_context: &FontContext,
    measurer: &M,
) {
    let allow_fill_height = params.available_height.is_definite();
    let space_evenly = params.attrs.space_evenly.unwrap_or(false) && allow_fill_height;
    let mut actual_content_height = resolve_column_children(
        tree,
        params.child_ids,
        params.content,
        ColumnChildrenOptions {
            spacing: params.spacing_y,
            allow_fill_height,
            space_evenly,
            is_scrollable: params.is_scrollable,
        },
        element_context,
        measurer,
        params.use_resolve_cache,
    );

    if actual_content_height > params.content.height
        && !params.is_scrollable
        && length_allows_content_expansion(params.attrs.height.as_ref())
    {
        // For content-height columns, a first pass can expand children and increase
        // total height. Re-resolve once using the expanded height so bottom/center
        // aligned children are positioned against the final content box.
        if !allow_fill_height {
            actual_content_height = resolve_column_children(
                tree,
                params.child_ids,
                ContentRect {
                    x: params.content.x,
                    y: params.content.y,
                    width: params.content.width,
                    height: actual_content_height,
                },
                ColumnChildrenOptions {
                    spacing: params.spacing_y,
                    allow_fill_height,
                    space_evenly,
                    is_scrollable: params.is_scrollable,
                },
                element_context,
                measurer,
                params.use_resolve_cache,
            );
        }

        expand_frame_height_to_content(tree, params.id, actual_content_height, params.insets);
    } else {
        set_frame_content_height(tree, params.id, actual_content_height, params.insets);
    }
}

fn resolve_text_column_kind<M: TextMeasurer>(
    tree: &mut ElementTree,
    params: &ResolvePassParams<'_>,
    element_context: &FontContext,
    measurer: &M,
) {
    let actual_content_height = resolve_text_column_children(
        tree,
        params.child_ids,
        TextFlowLayoutContext {
            content: params.content,
            spacing_x: params.spacing_x,
            spacing_y: params.spacing_y,
            inherited: element_context,
        },
        measurer,
        params.use_resolve_cache,
    );

    if actual_content_height > params.content.height
        && !params.is_scrollable
        && length_allows_content_expansion(params.attrs.height.as_ref())
    {
        expand_frame_height_to_content(tree, params.id, actual_content_height, params.insets);
    } else {
        set_frame_content_height(tree, params.id, actual_content_height, params.insets);
    }
}

fn resolve_paragraph_kind<M: TextMeasurer>(
    tree: &mut ElementTree,
    params: &ResolvePassParams<'_>,
    element_context: &FontContext,
    measurer: &M,
) {
    let mut paragraph_floats = Vec::new();
    let (fragments, actual_content_height) = resolve_paragraph_children(
        tree,
        params.child_ids,
        TextFlowLayoutContext {
            content: params.content,
            spacing_x: params.spacing_x,
            spacing_y: params.spacing_y,
            inherited: element_context,
        },
        measurer,
        &mut paragraph_floats,
        params.use_resolve_cache,
    );

    if let Some(element) = tree.get_mut(params.id) {
        element.layout.paragraph_fragments = Some(fragments);
    }

    if actual_content_height > params.content.height
        && !params.is_scrollable
        && length_allows_content_expansion(params.attrs.height.as_ref())
    {
        expand_frame_height_to_content(tree, params.id, actual_content_height, params.insets);
    } else {
        set_frame_content_height(tree, params.id, actual_content_height, params.insets);
    }
}

fn resolve_multiline_kind<M: TextMeasurer>(
    tree: &mut ElementTree,
    params: &ResolvePassParams<'_>,
    element_context: &FontContext,
    measurer: &M,
) {
    let content = params.attrs.content.as_deref().unwrap_or("");
    let font_size = params
        .attrs
        .font_size
        .map(|s| s as f32)
        .or(element_context.font_size)
        .unwrap_or(16.0);
    let (family, weight, italic) = font_info_with_inheritance(params.attrs, element_context);
    let letter_spacing = params
        .attrs
        .font_letter_spacing
        .map(|s| s as f32)
        .or(element_context.font_letter_spacing)
        .unwrap_or(0.0);
    let word_spacing = params
        .attrs
        .font_word_spacing
        .map(|s| s as f32)
        .or(element_context.font_word_spacing)
        .unwrap_or(0.0);
    let layout = multiline_text_layout(
        measurer,
        content,
        TextFontSpec {
            font_size,
            family: &family,
            weight,
            italic,
        },
        (letter_spacing, word_spacing),
        Some(params.content.width.max(0.0)),
    );

    if layout.total_height > params.content.height
        && !params.is_scrollable
        && length_allows_content_expansion(params.attrs.height.as_ref())
    {
        expand_frame_height_to_content(tree, params.id, layout.total_height, params.insets);
        set_frame_content_width(tree, params.id, layout.max_width, params.insets);
    } else {
        set_frame_content_size(
            tree,
            params.id,
            layout.max_width,
            layout.total_height,
            params.insets,
        );
    }
}

/// Resolve an element's frame given constraints and position.
/// Reads from pre-scaled attrs.
fn resolve_element<M: TextMeasurer>(
    tree: &mut ElementTree,
    id: &NodeId,
    placement: ResolvePlacement<'_>,
    measurer: &M,
) {
    let ResolvePlacement {
        constraint,
        x,
        y,
        inherited,
        use_resolve_cache,
    } = placement;

    let Some(element) = tree.get(id) else {
        return;
    };

    // Read from pre-scaled attrs
    let attrs = element.layout.effective.clone();
    let kind = element.spec.kind;
    let measured_frame = element.layout.measured_frame;
    let resolve_dirty = element.layout.resolve_dirty;
    let resolve_descendant_dirty = element.layout.resolve_descendant_dirty;
    let intrinsic = element
        .layout
        .measured_render_frame
        .or(element.layout.render_frame)
        .or(element.layout.measured_frame)
        .or(element.layout.frame)
        .map(|f| IntrinsicSize {
            width: f.width,
            height: f.height,
        })
        .unwrap_or_default();
    let child_ids = tree.child_ids(id);
    let nearby_mounts = tree.nearby_mounts_for(id);
    let topology_key = tree.topology_dependency_key_for(id);

    // Merge inherited font context with this element's attrs
    let element_context = inherited.merge_with_attrs(&attrs);
    let resolve_kind_eligible = resolve_cache_kind_eligible(kind);
    let cache_eligible = use_resolve_cache && resolve_kind_eligible;

    let cache_key = cache_eligible.then(|| {
        resolve_cache_key(
            kind,
            &attrs,
            inherited,
            measured_frame,
            constraint,
            topology_key,
        )
    });

    if resolve_descendant_dirty
        && !resolve_dirty
        && let Some(key) = cache_key.as_ref()
        && try_reuse_resolve_cache_with_dirty_descendants(tree, id, key, placement, measurer)
    {
        return;
    }

    if !use_resolve_cache || !resolve_kind_eligible || resolve_dirty || resolve_descendant_dirty {
        tree.record_layout_cache_stats(|stats| stats.record_resolve_miss());
    } else if let Some(key) = cache_key.as_ref()
        && try_reuse_resolve_cache(tree, id, key, x, y)
    {
        return;
    }

    let insets = LayoutInsets::from_attrs(&attrs);
    let spacing_x = spacing_x(&attrs);
    let spacing_y = spacing_y(&attrs);
    let align_x = attrs.align_x.unwrap_or_default();
    let align_y = attrs.align_y.unwrap_or_default();

    let scroll_x_enabled = effective_scrollbar_x(&attrs);
    let scroll_y_enabled = effective_scrollbar_y(&attrs);
    // Check if this element is scrollable (scrollbars only)
    let is_scrollable = scroll_x_enabled || scroll_y_enabled;

    let prefer_fill_width =
        container_prefers_fill_width(tree, kind, &attrs, &child_ids, constraint);
    let prefer_fill_height =
        container_prefers_fill_height(tree, kind, &attrs, &child_ids, constraint);

    let sizing = resolve_element_sizing(
        kind,
        &attrs,
        inherited,
        intrinsic,
        constraint,
        prefer_fill_width,
        prefer_fill_height,
    );
    let available_width = sizing.available_width;
    let available_height = sizing.available_height;
    let width = sizing.width;
    let height = sizing.height;

    // Update frame (content size will be updated after children are resolved)
    let before_geometry = registry_geometry_snapshot(tree, id);
    if let Some(element) = tree.get_mut(id) {
        element.layout.frame = Some(Frame {
            x,
            y,
            width,
            height,
            content_width: width,
            content_height: height,
        });
    }
    mark_registry_dirty_if_geometry_changed(tree, id, before_geometry);

    // Content area for children (inset by both padding and border).
    let (content_x, content_y, content_width, content_height) =
        insets.content_rect(x, y, width, height);

    let params = ResolvePassParams {
        id,
        attrs: &attrs,
        child_ids: &child_ids,
        content: ContentRect {
            x: content_x,
            y: content_y,
            width: content_width,
            height: content_height,
        },
        insets,
        is_scrollable,
        scroll_x_enabled,
        scroll_y_enabled,
        spacing_x,
        spacing_y,
        align_x,
        align_y,
        available_width,
        available_height,
        use_resolve_cache,
    };

    match kind {
        ElementKind::Text
        | ElementKind::TextInput
        | ElementKind::Image
        | ElementKind::Video
        | ElementKind::None => {}
        ElementKind::El => resolve_el_kind(tree, &params, &element_context, measurer),
        ElementKind::Slider => resolve_slider_kind(tree, &params, &element_context, measurer),
        ElementKind::Row => resolve_row_kind(tree, &params, &element_context, measurer),
        ElementKind::WrappedRow => {
            resolve_wrapped_row_kind(tree, &params, &element_context, measurer)
        }
        ElementKind::Column => resolve_column_kind(tree, &params, &element_context, measurer),
        ElementKind::TextColumn => {
            resolve_text_column_kind(tree, &params, &element_context, measurer)
        }
        ElementKind::Paragraph => resolve_paragraph_kind(tree, &params, &element_context, measurer),
        ElementKind::Multiline => resolve_multiline_kind(tree, &params, &element_context, measurer),
    }

    apply_layout_rotation_to_resolved_element(tree, id, &attrs);
    update_paint_children(tree, id, kind);
    update_scroll_state(tree, id);
    resolve_nearby_mounts(tree, id, &element_context, measurer, use_resolve_cache);

    if use_resolve_cache
        && can_store_resolve_cache(tree, kind, &child_ids, &nearby_mounts)
        && let Some((frame, render_frame)) = tree.get(id).and_then(|element| {
            let frame = element.layout.frame?;
            Some((frame, element.layout.render_frame.unwrap_or(frame)))
        })
    {
        let key = resolve_cache_key(
            kind,
            &attrs,
            inherited,
            measured_frame,
            constraint,
            topology_key,
        );

        tree.record_layout_cache_stats(|stats| stats.record_resolve_store());

        if let Some(element) = tree.get_mut(id) {
            element.layout.resolve_cache = Some(ResolveCache {
                key,
                extent: resolve_extent(frame, render_frame),
            });
            element.layout.resolve_dirty = false;
            element.layout.resolve_descendant_dirty = false;
        }
    }
}

fn apply_layout_rotation_to_resolved_element(tree: &mut ElementTree, id: &NodeId, attrs: &Attrs) {
    let Some(unrotated_frame) = tree.get(id).and_then(|element| element.layout.frame) else {
        return;
    };

    if layout_rotate_degrees(attrs).is_none() {
        let before_geometry = registry_geometry_snapshot(tree, id);
        if let Some(element) = tree.get_mut(id) {
            element.layout.render_frame = None;
        }
        mark_registry_dirty_if_geometry_changed(tree, id, before_geometry);
        return;
    }

    let layout_frame = layout_frame_for_rotation(unrotated_frame, attrs);
    let render_frame = render_frame_inside_layout_frame(layout_frame, unrotated_frame);
    let dx = render_frame.x - unrotated_frame.x;
    let dy = render_frame.y - unrotated_frame.y;

    let before_geometry = registry_geometry_snapshot(tree, id);
    if let Some(element) = tree.get_mut(id) {
        element.layout.frame = Some(layout_frame);
        element.layout.render_frame = Some(render_frame);
    }
    mark_registry_dirty_if_geometry_changed(tree, id, before_geometry);

    if dx != 0.0 || dy != 0.0 {
        let mut child_ids = tree.child_ids(id);
        child_ids.extend(tree.nearby_mounts_for(id).into_iter().map(|mount| mount.id));
        for child_id in child_ids {
            shift_subtree(tree, &child_id, dx, dy);
        }
    }
}

fn resolve_cache_key(
    kind: ElementKind,
    attrs: &Attrs,
    inherited: &FontContext,
    measured_frame: Option<Frame>,
    constraint: Constraint,
    topology: TopologyDependencyKey,
) -> ResolveCacheKey {
    ResolveCacheKey {
        kind,
        attrs: resolve_attrs(attrs),
        inherited: inherited_measure_font_key(inherited),
        measured_frame,
        constraint: resolve_constraint_key(constraint),
        topology,
    }
}

fn resolve_extent(frame: Frame, render_frame: Frame) -> ResolveExtent {
    ResolveExtent {
        width: frame.width,
        height: frame.height,
        content_width: frame.content_width,
        content_height: frame.content_height,
        render_x: render_frame.x - frame.x,
        render_y: render_frame.y - frame.y,
        render_width: render_frame.width,
        render_height: render_frame.height,
        render_content_width: render_frame.content_width,
        render_content_height: render_frame.content_height,
    }
}

fn frames_from_resolve_extent(extent: ResolveExtent, x: f32, y: f32) -> (Frame, Frame) {
    let frame = Frame {
        x,
        y,
        width: extent.width,
        height: extent.height,
        content_width: extent.content_width,
        content_height: extent.content_height,
    };
    let render_frame = Frame {
        x: x + extent.render_x,
        y: y + extent.render_y,
        width: extent.render_width,
        height: extent.render_height,
        content_width: extent.render_content_width,
        content_height: extent.render_content_height,
    };
    (frame, render_frame)
}

fn resolve_attrs(attrs: &Attrs) -> ResolveAttrs {
    ResolveAttrs {
        width: attrs.width.clone(),
        height: attrs.height.clone(),
        layout_scale: attrs.layout_scale,
        layout_rotate: attrs.layout_rotate,
        padding: attrs.padding.clone(),
        border_width: attrs.border_width.clone(),
        spacing: attrs.spacing,
        spacing_x: attrs.spacing_x,
        spacing_y: attrs.spacing_y,
        align_x: attrs.align_x,
        align_y: attrs.align_y,
        scrollbar_y: attrs.scrollbar_y,
        scrollbar_x: attrs.scrollbar_x,
        ghost_scrollbar_y: attrs.ghost_scrollbar_y,
        ghost_scrollbar_x: attrs.ghost_scrollbar_x,
        scroll_x: attrs.scroll_x,
        scroll_y: attrs.scroll_y,
        clip_nearby: attrs.clip_nearby,
        content: attrs.content.clone(),
        font_size: attrs.font_size,
        font: attrs.font.clone(),
        font_weight: attrs.font_weight.clone(),
        font_style: attrs.font_style.clone(),
        font_letter_spacing: attrs.font_letter_spacing,
        font_word_spacing: attrs.font_word_spacing,
        image_src: attrs.image_src.clone(),
        image_fit: attrs.image_fit,
        image_size: attrs.image_size,
        slider_min: attrs.slider_min,
        slider_max: attrs.slider_max,
        slider_value: attrs.slider_value,
        slider_step: attrs.slider_step,
        text_align: attrs.text_align,
        snap_layout: attrs.snap_layout,
        snap_text_metrics: attrs.snap_text_metrics,
        space_evenly: attrs.space_evenly,
        has_animation_attrs: attrs.animate.is_some()
            || attrs.animate_enter.is_some()
            || attrs.animate_exit.is_some(),
    }
}

fn resolve_constraint_key(constraint: Constraint) -> ResolveConstraintKey {
    ResolveConstraintKey {
        width: resolve_available_space_key(constraint.width),
        height: resolve_available_space_key(constraint.height),
    }
}

fn resolve_available_space_key(space: AvailableSpace) -> ResolveAvailableSpaceKey {
    match space {
        AvailableSpace::Definite(value) => ResolveAvailableSpaceKey::Definite(value),
        AvailableSpace::MinContent => ResolveAvailableSpaceKey::MinContent,
        AvailableSpace::MaxContent => ResolveAvailableSpaceKey::MaxContent,
    }
}

fn try_reuse_resolve_cache(
    tree: &mut ElementTree,
    id: &NodeId,
    key: &ResolveCacheKey,
    x: f32,
    y: f32,
) -> bool {
    let cached_frames = tree.get(id).and_then(|element| {
        let cache = element.layout.resolve_cache.as_ref()?;
        if &cache.key != key {
            return None;
        }

        Some((
            frames_from_resolve_extent(cache.extent, x, y),
            element.layout.frame?,
        ))
    });

    let Some(((target_frame, target_render_frame), current_frame)) = cached_frames else {
        tree.record_layout_cache_stats(|stats| stats.record_resolve_miss());
        return false;
    };

    tree.record_layout_cache_stats(|stats| stats.record_resolve_hit());

    shift_subtree(
        tree,
        id,
        target_frame.x - current_frame.x,
        target_frame.y - current_frame.y,
    );

    let before_geometry = registry_geometry_snapshot(tree, id);
    if let Some(element) = tree.get_mut(id) {
        element.layout.frame = Some(target_frame);
        element.layout.render_frame = distinct_render_frame(target_frame, target_render_frame);
        element.layout.resolve_dirty = false;
        element.layout.resolve_descendant_dirty = false;
    }
    mark_registry_dirty_if_geometry_changed(tree, id, before_geometry);

    true
}

fn try_reuse_resolve_cache_with_dirty_descendants<M: TextMeasurer>(
    tree: &mut ElementTree,
    id: &NodeId,
    key: &ResolveCacheKey,
    placement: ResolvePlacement<'_>,
    measurer: &M,
) -> bool {
    let ResolvePlacement {
        x,
        y,
        inherited,
        use_resolve_cache,
        ..
    } = placement;

    let snapshot = tree.get(id).and_then(|element| {
        Some((
            element.layout.resolve_cache.as_ref()?.clone(),
            element.layout.frame?,
            element.layout.effective.clone(),
            element.spec.kind,
            element.layout.measured_frame,
            element.layout.resolve_dirty,
            element.layout.resolve_descendant_dirty,
            tree.child_ids(id),
            tree.nearby_mounts_for(id),
        ))
    });

    let Some((
        cache,
        current_frame,
        attrs,
        kind,
        _measured_frame,
        resolve_dirty,
        resolve_descendant_dirty,
        child_ids,
        nearby_mounts,
    )) = snapshot
    else {
        return false;
    };

    if resolve_dirty
        || !resolve_descendant_dirty
        || !resolve_cache_key_matches_with_nearby_boundary(&cache.key, key)
    {
        return false;
    }

    let dirty_child_ids: Vec<NodeId> = child_ids
        .iter()
        .filter(|child_id| node_needs_resolve_traversal(tree, child_id))
        .copied()
        .collect();

    let full_key_match = cache.key == *key;
    let (target_frame, target_render_frame) = frames_from_resolve_extent(cache.extent, x, y);
    tree.record_layout_cache_stats(|stats| stats.record_resolve_hit());
    shift_subtree(
        tree,
        id,
        target_frame.x - current_frame.x,
        target_frame.y - current_frame.y,
    );

    let before_geometry = registry_geometry_snapshot(tree, id);
    if let Some(element) = tree.get_mut(id) {
        element.layout.frame = Some(target_frame);
        element.layout.render_frame = distinct_render_frame(target_frame, target_render_frame);
        element.layout.resolve_dirty = false;
    }
    mark_registry_dirty_if_geometry_changed(tree, id, before_geometry);

    let element_context = inherited.merge_with_attrs(&attrs);

    for child_id in &dirty_child_ids {
        let Some((child_key, child_x, child_y)) =
            resolve_cache_key_for_existing_frame(tree, child_id, &element_context)
        else {
            return false;
        };

        resolve_element(
            tree,
            child_id,
            ResolvePlacement {
                constraint: constraint_from_resolve_constraint_key(child_key.constraint),
                x: child_x,
                y: child_y,
                inherited: &element_context,
                use_resolve_cache,
            },
            measurer,
        );
    }

    let nearby_needs_traversal = !full_key_match
        || nearby_mounts
            .iter()
            .any(|mount| node_needs_resolve_traversal(tree, &mount.id));

    if nearby_needs_traversal {
        resolve_nearby_mounts(tree, id, &element_context, measurer, use_resolve_cache);
    }

    if use_resolve_cache
        && !full_key_match
        && can_store_resolve_cache(tree, kind, &child_ids, &nearby_mounts)
    {
        tree.record_layout_cache_stats(|stats| stats.record_resolve_store());
        if let Some((frame, render_frame)) = tree.get(id).and_then(|element| {
            let frame = element.layout.frame?;
            Some((frame, element.layout.render_frame.unwrap_or(frame)))
        }) && let Some(element) = tree.get_mut(id)
        {
            element.layout.resolve_cache = Some(ResolveCache {
                key: key.clone(),
                extent: resolve_extent(frame, render_frame),
            });
        }
    }

    if let Some(element) = tree.get_mut(id) {
        element.layout.resolve_dirty = false;
        element.layout.resolve_descendant_dirty = false;
    }

    true
}

fn resolve_cache_key_for_existing_frame(
    tree: &ElementTree,
    id: &NodeId,
    inherited: &FontContext,
) -> Option<(ResolveCacheKey, f32, f32)> {
    let element = tree.get(id)?;
    let frame = element.layout.frame?;
    let cached_constraint = element.layout.resolve_cache.as_ref()?.key.constraint;
    let key = resolve_cache_key(
        element.spec.kind,
        &element.layout.effective,
        inherited,
        element.layout.measured_frame,
        constraint_from_resolve_constraint_key(cached_constraint),
        tree.topology_dependency_key_for(id),
    );

    Some((key, frame.x, frame.y))
}

fn constraint_from_resolve_constraint_key(key: ResolveConstraintKey) -> Constraint {
    Constraint {
        width: available_space_from_resolve_key(key.width),
        height: available_space_from_resolve_key(key.height),
    }
}

fn available_space_from_resolve_key(key: ResolveAvailableSpaceKey) -> AvailableSpace {
    match key {
        ResolveAvailableSpaceKey::Definite(value) => AvailableSpace::Definite(value),
        ResolveAvailableSpaceKey::MinContent => AvailableSpace::MinContent,
        ResolveAvailableSpaceKey::MaxContent => AvailableSpace::MaxContent,
    }
}

fn node_needs_resolve_traversal(tree: &ElementTree, id: &NodeId) -> bool {
    tree.get(id).is_some_and(|element| {
        element.layout.resolve_dirty || element.layout.resolve_descendant_dirty
    })
}

fn resolve_cache_key_matches_with_nearby_boundary(
    cached: &ResolveCacheKey,
    current: &ResolveCacheKey,
) -> bool {
    cached.kind == current.kind
        && cached.attrs == current.attrs
        && cached.inherited == current.inherited
        && cached.measured_frame == current.measured_frame
        && cached.constraint == current.constraint
        && cached.topology.children_version == current.topology.children_version
        && cached.topology.child_count == current.topology.child_count
}

fn can_store_resolve_cache(
    tree: &ElementTree,
    kind: ElementKind,
    child_ids: &[NodeId],
    nearby: &[NearbyMount],
) -> bool {
    resolve_cache_kind_eligible(kind)
        && child_ids
            .iter()
            .all(|child_id| child_can_be_restored_by_parent_resolve_cache(tree, kind, child_id))
        && nearby.iter().all(|mount| {
            tree.get(&mount.id)
                .is_some_and(|child| child.layout.resolve_cache.is_some())
        })
}

fn child_can_be_restored_by_parent_resolve_cache(
    tree: &ElementTree,
    parent_kind: ElementKind,
    child_id: &NodeId,
) -> bool {
    let Some(child) = tree.get(child_id) else {
        return false;
    };

    if parent_kind == ElementKind::Paragraph && paragraph_owns_inline_child_layout(child) {
        return true;
    }

    if parent_kind == ElementKind::TextColumn && child.spec.kind == ElementKind::Paragraph {
        return true;
    }

    child.layout.resolve_cache.is_some()
}

fn paragraph_owns_inline_child_layout(child: &Element) -> bool {
    !matches!(
        child.layout.effective.align_x,
        Some(AlignX::Left | AlignX::Right)
    )
}

fn resolve_cache_kind_eligible(kind: ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Text
            | ElementKind::TextInput
            | ElementKind::Image
            | ElementKind::Video
            | ElementKind::None
            | ElementKind::El
            | ElementKind::Slider
            | ElementKind::Row
            | ElementKind::Column
            | ElementKind::Multiline
            | ElementKind::WrappedRow
            | ElementKind::TextColumn
            | ElementKind::Paragraph
    )
}

fn update_paint_children(tree: &mut ElementTree, id: &NodeId, kind: ElementKind) {
    if tree.get(id).is_none() {
        return;
    }

    let source_children = tree.child_ids(id);
    let mut ordered: Vec<(usize, NodeId, f32, f32)> = source_children
        .iter()
        .enumerate()
        .filter_map(|(index, child_id)| {
            tree.get(child_id)
                .and_then(|child| child.layout.frame)
                .map(|frame| (index, *child_id, frame.x, frame.y))
        })
        .collect();

    match kind {
        ElementKind::Row => {
            ordered.sort_by(|left, right| {
                left.2
                    .partial_cmp(&right.2)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| left.0.cmp(&right.0))
            });
        }
        ElementKind::Column | ElementKind::TextColumn => {
            ordered.sort_by(|left, right| {
                left.3
                    .partial_cmp(&right.3)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| left.0.cmp(&right.0))
            });
        }
        ElementKind::WrappedRow => {
            ordered.sort_by(|left, right| {
                left.3
                    .partial_cmp(&right.3)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        left.2
                            .partial_cmp(&right.2)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| left.0.cmp(&right.0))
            });
        }
        _ => {}
    }

    let paint_children = if matches!(
        kind,
        ElementKind::Row | ElementKind::Column | ElementKind::TextColumn | ElementKind::WrappedRow
    ) {
        ordered
            .into_iter()
            .map(|(_, child_id, _, _)| child_id)
            .collect()
    } else {
        source_children
    };

    let _ = tree.set_paint_children(id, paint_children);
}

/// Resolve final length from attribute, intrinsic, and constraint.
fn resolve_length(length: Option<&Length>, intrinsic: f32, constraint: f32) -> f32 {
    match length {
        Some(Length::Px(px)) => *px as f32,
        Some(Length::Content) | None => intrinsic.min(constraint),
        Some(Length::Fill) => constraint,
        Some(Length::FillWeighted(_)) => constraint, // Simplified: treat as fill
        Some(Length::Min(left, right)) => resolve_length(Some(left), intrinsic, constraint)
            .min(resolve_length(Some(right), intrinsic, constraint)),
        Some(Length::Max(left, right)) => resolve_length(Some(left), intrinsic, constraint)
            .max(resolve_length(Some(right), intrinsic, constraint)),
    }
}

fn resolve_nearby_mounts<M: TextMeasurer>(
    tree: &mut ElementTree,
    host_id: &NodeId,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) {
    let Some(host_frame) = tree.get(host_id).and_then(|element| {
        let frame = element.layout.frame?;
        Some(element.layout.render_frame.unwrap_or(frame))
    }) else {
        return;
    };

    let nearby_roots: Vec<(NearbySlot, NodeId)> = tree
        .nearby_mounts_for(host_id)
        .into_iter()
        .map(|mount| (mount.slot, mount.id))
        .collect();

    for (slot, nearby_id) in nearby_roots {
        let constraint = nearby_constraint(host_frame, slot);
        resolve_element(
            tree,
            &nearby_id,
            ResolvePlacement {
                constraint,
                x: host_frame.x,
                y: host_frame.y,
                inherited,
                use_resolve_cache,
            },
            measurer,
        );

        let Some((nearby_frame, align_x, align_y)) = tree.get(&nearby_id).and_then(|element| {
            element.layout.frame.map(|frame| {
                (
                    frame,
                    element.layout.effective.align_x.unwrap_or_default(),
                    element.layout.effective.align_y.unwrap_or_default(),
                )
            })
        }) else {
            continue;
        };

        let target_x = nearby_origin_x(host_frame, nearby_frame, slot, align_x);
        let target_y = nearby_origin_y(host_frame, nearby_frame, slot, align_y);
        shift_subtree(
            tree,
            &nearby_id,
            target_x - nearby_frame.x,
            target_y - nearby_frame.y,
        );
    }
}

pub(crate) fn layout_nearby_mounts_for_refresh(tree: &mut ElementTree, host_id: &NodeId) -> bool {
    tree.ensure_topology();

    if tree
        .get(host_id)
        .and_then(|element| element.layout.frame)
        .is_none()
    {
        return false;
    }

    let Some(inherited) = font_context_for_resolved_node(tree, host_id) else {
        return false;
    };

    let nearby_roots = tree.nearby_mounts_for(host_id);
    let samples = HashMap::new();

    for mount in &nearby_roots {
        let inherited_scale =
            inherited_layout_scale_for_node(tree, &mount.id, tree.current_scale(), &samples);
        prepare_attrs_for_subtree(tree, mount.id, inherited_scale, &samples);
        let _ = measure_element(tree, &mount.id, &SkiaTextMeasurer, &inherited, true);
    }

    resolve_nearby_mounts(tree, host_id, &inherited, &SkiaTextMeasurer, true);
    tree.recompute_layout_descendant_dirty();
    true
}

fn font_context_for_resolved_node(tree: &ElementTree, id: &NodeId) -> Option<FontContext> {
    let mut ix = tree.ix_of(id)?;
    let mut lineage = Vec::new();

    loop {
        lineage.push(ix);
        let Some(parent_ix) = tree
            .parent_link_of(ix)
            .and_then(|parent_link| super::element::parent_ix_from_link(Some(parent_link)))
        else {
            break;
        };
        ix = parent_ix;
    }

    lineage
        .into_iter()
        .rev()
        .try_fold(FontContext::default(), |context, ix| {
            tree.get_ix(ix)
                .map(|element| context.merge_with_attrs(&element.layout.effective))
        })
}

fn nearby_constraint(parent_frame: Frame, slot: NearbySlot) -> Constraint {
    match slot.spec().constraint_kind {
        NearbyConstraintKind::Box => Constraint::new(parent_frame.width, parent_frame.height),
        NearbyConstraintKind::WidthBand => Constraint::with_space(
            AvailableSpace::Definite(parent_frame.width),
            AvailableSpace::MaxContent,
        ),
        NearbyConstraintKind::HeightBand => Constraint::with_space(
            AvailableSpace::MaxContent,
            AvailableSpace::Definite(parent_frame.height),
        ),
    }
}

fn nearby_origin_x(
    parent_frame: Frame,
    nearby_frame: Frame,
    slot: NearbySlot,
    align_x: AlignX,
) -> f32 {
    match slot {
        NearbySlot::BehindContent | NearbySlot::Above | NearbySlot::Below | NearbySlot::InFront => {
            aligned_x_in_slot(
                parent_frame.x,
                parent_frame.width,
                nearby_frame.width,
                align_x,
            )
        }
        NearbySlot::OnLeft => parent_frame.x - nearby_frame.width,
        NearbySlot::OnRight => parent_frame.x + parent_frame.width,
    }
}

fn nearby_origin_y(
    parent_frame: Frame,
    nearby_frame: Frame,
    slot: NearbySlot,
    align_y: AlignY,
) -> f32 {
    match slot {
        NearbySlot::Above => parent_frame.y - nearby_frame.height,
        NearbySlot::Below => parent_frame.y + parent_frame.height,
        NearbySlot::BehindContent
        | NearbySlot::OnLeft
        | NearbySlot::OnRight
        | NearbySlot::InFront => aligned_y_in_slot(
            parent_frame.y,
            parent_frame.height,
            nearby_frame.height,
            align_y,
        ),
    }
}

fn aligned_x_in_slot(slot_x: f32, slot_width: f32, nearby_width: f32, align_x: AlignX) -> f32 {
    match align_x {
        AlignX::Left => slot_x,
        AlignX::Center => slot_x + (slot_width - nearby_width) / 2.0,
        AlignX::Right => slot_x + slot_width - nearby_width,
    }
}

fn aligned_y_in_slot(slot_y: f32, slot_height: f32, nearby_height: f32, align_y: AlignY) -> f32 {
    match align_y {
        AlignY::Top => slot_y,
        AlignY::Center => slot_y + (slot_height - nearby_height) / 2.0,
        AlignY::Bottom => slot_y + slot_height - nearby_height,
    }
}

fn is_content_length(length: Option<&Length>) -> bool {
    match length {
        None | Some(Length::Content) => true,
        Some(Length::Min(left, right)) | Some(Length::Max(left, right)) => {
            is_content_length(Some(left)) || is_content_length(Some(right))
        }
        _ => false,
    }
}

fn length_allows_content_expansion(length: Option<&Length>) -> bool {
    match length {
        None | Some(Length::Content) => true,
        Some(Length::Min(left, right)) => {
            length_allows_content_expansion(Some(left))
                && length_allows_content_expansion(Some(right))
        }
        Some(Length::Max(left, right)) => {
            length_allows_content_expansion(Some(left))
                || length_allows_content_expansion(Some(right))
        }
        Some(Length::Px(_)) | Some(Length::Fill) | Some(Length::FillWeighted(_)) => false,
    }
}

/// Get the weight value for a fill-based length.
/// Returns 1.0 for Fill, the configured weight for FillWeighted, or 0.0 for non-fill.
fn get_fill_weight(length: Option<&Length>) -> f32 {
    match length {
        Some(Length::Fill) => 1.0,
        Some(Length::FillWeighted(weight)) => *weight as f32,
        Some(Length::Min(left, right)) => match (
            get_fill_weight_opt(Some(left)),
            get_fill_weight_opt(Some(right)),
        ) {
            (Some(left), Some(right)) => left.min(right),
            (Some(weight), None) | (None, Some(weight)) => weight,
            (None, None) => 0.0,
        },
        Some(Length::Max(left, right)) => match (
            get_fill_weight_opt(Some(left)),
            get_fill_weight_opt(Some(right)),
        ) {
            (Some(left), Some(right)) => left.max(right),
            (Some(weight), None) | (None, Some(weight)) => weight,
            (None, None) => 0.0,
        },
        _ => 0.0,
    }
}

fn get_fill_weight_opt(length: Option<&Length>) -> Option<f32> {
    let weight = get_fill_weight(length);
    (weight > 0.0).then_some(weight)
}

#[derive(Clone, Copy, Debug)]
struct ElChildrenOptions {
    parent_align_x: AlignX,
    parent_align_y: AlignY,
    scroll_x_enabled: bool,
    scroll_y_enabled: bool,
}

#[derive(Clone, Copy, Debug)]
struct RowChildrenOptions {
    spacing: f32,
    allow_fill_width: bool,
    space_evenly: bool,
}

#[derive(Clone, Copy, Debug)]
struct ColumnChildrenOptions {
    spacing: f32,
    allow_fill_height: bool,
    space_evenly: bool,
    is_scrollable: bool,
}

#[derive(Clone, Copy, Debug)]
struct WrappedRowChildrenOptions {
    spacing_x: f32,
    spacing_y: f32,
}

#[derive(Clone, Copy, Debug)]
struct TextFlowLayoutContext<'a> {
    content: ContentRect,
    spacing_x: f32,
    spacing_y: f32,
    inherited: &'a FontContext,
}

#[derive(Clone, Copy, Debug)]
struct ResolvePlacement<'a> {
    constraint: Constraint,
    x: f32,
    y: f32,
    inherited: &'a FontContext,
    use_resolve_cache: bool,
}

#[derive(Clone, Copy, Debug)]
struct RowResolveContext<'a> {
    content: ContentRect,
    options: RowChildrenOptions,
    inherited: &'a FontContext,
    use_resolve_cache: bool,
}

#[derive(Clone, Copy, Debug)]
struct ChildFrameSnapshot {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    content_width: f32,
    content_height: f32,
}

fn child_frame_snapshot(tree: &ElementTree, child_id: &NodeId) -> Option<ChildFrameSnapshot> {
    let frame = tree.get(child_id)?.layout.frame?;
    Some(ChildFrameSnapshot {
        x: frame.x,
        y: frame.y,
        width: frame.width,
        height: frame.height,
        content_width: frame.content_width,
        content_height: frame.content_height,
    })
}

fn resolve_child_with_placement<M: TextMeasurer>(
    tree: &mut ElementTree,
    child_id: &NodeId,
    placement: ResolvePlacement<'_>,
    measurer: &M,
) -> Option<ChildFrameSnapshot> {
    resolve_element(tree, child_id, placement, measurer);
    child_frame_snapshot(tree, child_id)
}

fn child_align_x(tree: &ElementTree, child_id: &NodeId) -> AlignX {
    tree.get(child_id)
        .map(|child| child.layout.effective.align_x.unwrap_or_default())
        .unwrap_or_default()
}

fn child_align_y(tree: &ElementTree, child_id: &NodeId) -> AlignY {
    tree.get(child_id)
        .map(|child| child.layout.effective.align_y.unwrap_or_default())
        .unwrap_or_default()
}

fn child_measured_width(tree: &ElementTree, child_id: &NodeId) -> f32 {
    tree.get(child_id)
        .and_then(|child| child.layout.measured_frame.or(child.layout.frame))
        .map(|frame| frame.width)
        .unwrap_or(0.0)
        .max(0.0)
}

fn child_measured_height(tree: &ElementTree, child_id: &NodeId) -> f32 {
    tree.get(child_id)
        .and_then(|child| child.layout.measured_frame.or(child.layout.frame))
        .map(|frame| frame.height)
        .unwrap_or(0.0)
        .max(0.0)
}

fn resolve_planned_length(length: Option<&Length>, intrinsic: f32, fill_unit: Option<f32>) -> f32 {
    match length {
        Some(Length::Px(px)) => *px as f32,
        Some(Length::Content) | None => intrinsic,
        Some(Length::Fill) => fill_unit.unwrap_or(intrinsic),
        Some(Length::FillWeighted(weight)) => fill_unit
            .map(|unit| unit * *weight as f32)
            .unwrap_or(intrinsic),
        Some(Length::Min(left, right)) => resolve_planned_length(Some(left), intrinsic, fill_unit)
            .min(resolve_planned_length(Some(right), intrinsic, fill_unit)),
        Some(Length::Max(left, right)) => resolve_planned_length(Some(left), intrinsic, fill_unit)
            .max(resolve_planned_length(Some(right), intrinsic, fill_unit)),
    }
}

fn force_child_width(tree: &mut ElementTree, child_id: &NodeId, width: f32) {
    if let Some(child) = tree.get_mut(child_id) {
        child.layout.effective.width = Some(Length::Px(width.max(0.0) as f64));
    }
}

fn slider_ratio(attrs: &Attrs) -> f32 {
    let (min, max) = slider_range(attrs);
    let value = normalize_slider_value(attrs, attrs.slider_value.unwrap_or(min));
    ((value - min) / (max - min)).clamp(0.0, 1.0) as f32
}

fn normalize_slider_value(attrs: &Attrs, value: f64) -> f64 {
    let (min, max) = slider_range(attrs);
    let clamped = if value.is_finite() {
        value.clamp(min, max)
    } else {
        min
    };
    let step = attrs.slider_step.unwrap_or(0.0);
    if !step.is_finite() || step <= 0.0 {
        return clamped;
    }

    let units = ((clamped - min) / step).round();
    (min + units * step).clamp(min, max)
}

fn slider_range(attrs: &Attrs) -> (f64, f64) {
    let min = attrs.slider_min.unwrap_or(0.0);
    let max = attrs.slider_max.unwrap_or(1.0);
    if min.is_finite() && max.is_finite() && max > min {
        (min, max)
    } else {
        (0.0, 1.0)
    }
}

// =============================================================================
// Child Resolution by Element Type
// =============================================================================

/// Resolve children for El (single child container with alignment).
/// Reads from pre-scaled attrs.
///   Returns (actual_content_width, actual_content_height).
///
/// Alignment follows elm-ui semantics:
/// - Parent's alignment (e.g., `el([centerX()], child)`) sets default for children
/// - Child can override with its own alignment attribute
fn resolve_el_children<M: TextMeasurer>(
    tree: &mut ElementTree,
    child_ids: &[NodeId],
    content: ContentRect,
    options: ElChildrenOptions,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) -> (f32, f32) {
    let mut max_child_width = 0.0_f32;
    let mut max_child_height = 0.0_f32;

    for child_id in child_ids {
        let (align_x, align_y) = {
            let Some(child) = tree.get(child_id) else {
                continue;
            };
            // Child can override parent alignment, otherwise use parent's
            let ax = child
                .layout
                .effective
                .align_x
                .unwrap_or(options.parent_align_x);
            let ay = child
                .layout
                .effective
                .align_y
                .unwrap_or(options.parent_align_y);
            (ax, ay)
        };

        let Some(frame) = resolve_child_with_placement(
            tree,
            child_id,
            ResolvePlacement {
                constraint: Constraint::new(content.width, content.height),
                x: 0.0,
                y: 0.0,
                inherited,
                use_resolve_cache,
            },
            measurer,
        ) else {
            continue;
        };

        // Track max child dimensions for content size
        let child_content_width = if options.scroll_x_enabled {
            frame.width
        } else {
            frame.content_width
        };
        let child_content_height = if options.scroll_y_enabled {
            frame.height
        } else {
            frame.content_height
        };
        max_child_width = max_child_width.max(child_content_width);
        max_child_height = max_child_height.max(child_content_height);

        let child_x = match align_x {
            AlignX::Left => content.x,
            AlignX::Center => content.x + (content.width - frame.width) / 2.0,
            AlignX::Right => content.x + content.width - frame.width,
        };

        let child_y = match align_y {
            AlignY::Top => content.y,
            AlignY::Center => content.y + (content.height - frame.height) / 2.0,
            AlignY::Bottom => content.y + content.height - frame.height,
        };

        let dx = child_x - frame.x;
        let dy = child_y - frame.y;
        shift_subtree(tree, child_id, dx, dy);
    }

    (max_child_width, max_child_height)
}

#[derive(Debug)]
struct RowLayoutPlan {
    children: Vec<(NodeId, f32)>,
    left_children: Vec<(NodeId, f32)>,
    center_children: Vec<(NodeId, f32)>,
    right_children: Vec<(NodeId, f32)>,
    total_left_width: f32,
    total_center_width: f32,
    total_right_width: f32,
    total_width: f32,
}

fn spacing_for_count(count: usize, spacing: f32) -> f32 {
    if count > 1 {
        spacing * (count - 1) as f32
    } else {
        0.0
    }
}

fn build_row_layout_plan(
    tree: &ElementTree,
    child_ids: &[NodeId],
    options: RowChildrenOptions,
    content_width: f32,
) -> RowLayoutPlan {
    let mut total_portions = 0.0_f32;
    let mut fixed_width = 0.0_f32;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else {
            continue;
        };
        let measured_width = child_measured_width(tree, child_id);
        let portion = if options.allow_fill_width {
            get_fill_weight(child.layout.effective.width.as_ref())
        } else {
            0.0
        };
        if portion > 0.0 {
            total_portions += portion;
        } else if layout_rotate_degrees(&child.layout.effective).is_some() {
            fixed_width += measured_width;
        } else {
            fixed_width +=
                resolve_planned_length(child.layout.effective.width.as_ref(), measured_width, None);
        }
    }

    // Calculate width per portion.
    let effective_spacing = if options.space_evenly {
        0.0
    } else {
        options.spacing
    };
    let total_spacing = effective_spacing * (child_ids.len().saturating_sub(1)) as f32;
    let remaining = (content_width - fixed_width - total_spacing).max(0.0);
    let width_per_portion = if total_portions > 0.0 {
        remaining / total_portions
    } else {
        0.0
    };

    // Partition children by horizontal alignment and calculate widths.
    let mut children: Vec<(NodeId, f32)> = Vec::new();
    let mut left_children: Vec<(NodeId, f32)> = Vec::new();
    let mut center_children: Vec<(NodeId, f32)> = Vec::new();
    let mut right_children: Vec<(NodeId, f32)> = Vec::new();
    let mut total_left_width = 0.0_f32;
    let mut total_center_width = 0.0_f32;
    let mut total_right_width = 0.0_f32;
    let mut total_width = 0.0_f32;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else {
            continue;
        };
        let measured_width = child_measured_width(tree, child_id);
        let portion = if options.allow_fill_width {
            get_fill_weight(child.layout.effective.width.as_ref())
        } else {
            0.0
        };
        let width = if portion > 0.0 {
            resolve_planned_length(
                child.layout.effective.width.as_ref(),
                measured_width,
                Some(width_per_portion),
            )
        } else if layout_rotate_degrees(&child.layout.effective).is_some() {
            measured_width
        } else {
            resolve_planned_length(child.layout.effective.width.as_ref(), measured_width, None)
        };
        children.push((*child_id, width));
        total_width += width;

        match child.layout.effective.align_x.unwrap_or_default() {
            AlignX::Left => {
                left_children.push((*child_id, width));
                total_left_width += width;
            }
            AlignX::Center => {
                center_children.push((*child_id, width));
                total_center_width += width;
            }
            AlignX::Right => {
                right_children.push((*child_id, width));
                total_right_width += width;
            }
        }
    }

    RowLayoutPlan {
        children,
        left_children,
        center_children,
        right_children,
        total_left_width,
        total_center_width,
        total_right_width,
        total_width,
    }
}

fn build_row_layout_plan_from_widths(tree: &ElementTree, line: &[(NodeId, f32)]) -> RowLayoutPlan {
    let mut children: Vec<(NodeId, f32)> = Vec::new();
    let mut left_children: Vec<(NodeId, f32)> = Vec::new();
    let mut center_children: Vec<(NodeId, f32)> = Vec::new();
    let mut right_children: Vec<(NodeId, f32)> = Vec::new();
    let mut total_left_width = 0.0_f32;
    let mut total_center_width = 0.0_f32;
    let mut total_right_width = 0.0_f32;
    let mut total_width = 0.0_f32;

    for (child_id, width) in line {
        let Some(child) = tree.get(child_id) else {
            continue;
        };

        children.push((*child_id, *width));
        total_width += *width;

        match child.layout.effective.align_x.unwrap_or_default() {
            AlignX::Left => {
                left_children.push((*child_id, *width));
                total_left_width += *width;
            }
            AlignX::Center => {
                center_children.push((*child_id, *width));
                total_center_width += *width;
            }
            AlignX::Right => {
                right_children.push((*child_id, *width));
                total_right_width += *width;
            }
        }
    }

    RowLayoutPlan {
        children,
        left_children,
        center_children,
        right_children,
        total_left_width,
        total_center_width,
        total_right_width,
        total_width,
    }
}

fn resolve_grouped_row_line<M: TextMeasurer>(
    tree: &mut ElementTree,
    content: ContentRect,
    spacing: f32,
    plan: &RowLayoutPlan,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) -> f32 {
    let left_spacing = spacing_for_count(plan.left_children.len(), spacing);
    let center_spacing = spacing_for_count(plan.center_children.len(), spacing);
    let right_spacing = spacing_for_count(plan.right_children.len(), spacing);

    let total_left_width = plan.total_left_width + left_spacing;
    let total_center_width = plan.total_center_width + center_spacing;
    let total_right_width = plan.total_right_width + right_spacing;

    let mut max_child_height = 0.0_f32;
    let mut current_x = content.x;

    for (child_id, child_width) in &plan.left_children {
        if let Some(frame) = resolve_child_with_placement(
            tree,
            child_id,
            ResolvePlacement {
                constraint: Constraint::new(*child_width, content.height),
                x: current_x,
                y: content.y,
                inherited,
                use_resolve_cache,
            },
            measurer,
        ) {
            max_child_height = max_child_height.max(frame.content_height);
        }

        current_x += *child_width + spacing;
    }

    let mut right_x = content.x + content.width;
    for (child_id, child_width) in plan.right_children.iter().rev() {
        right_x -= *child_width;
        if let Some(frame) = resolve_child_with_placement(
            tree,
            child_id,
            ResolvePlacement {
                constraint: Constraint::new(*child_width, content.height),
                x: right_x,
                y: content.y,
                inherited,
                use_resolve_cache,
            },
            measurer,
        ) {
            max_child_height = max_child_height.max(frame.content_height);
        }

        right_x -= spacing;
    }

    if !plan.center_children.is_empty() {
        let left_end = content.x + total_left_width;
        let right_start = content.x + content.width - total_right_width;
        let available_center = (right_start - left_end).max(0.0);
        let center_start = left_end + (available_center - total_center_width) / 2.0;

        let mut center_x = center_start.max(left_end);
        for (child_id, child_width) in &plan.center_children {
            if let Some(frame) = resolve_child_with_placement(
                tree,
                child_id,
                ResolvePlacement {
                    constraint: Constraint::new(*child_width, content.height),
                    x: center_x,
                    y: content.y,
                    inherited,
                    use_resolve_cache,
                },
                measurer,
            ) {
                max_child_height = max_child_height.max(frame.content_height);
            }

            center_x += *child_width + spacing;
        }
    }

    max_child_height
}

fn resolve_row_space_evenly<M: TextMeasurer>(
    tree: &mut ElementTree,
    children: &[(NodeId, f32)],
    content: ContentRect,
    total_child_width: f32,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) -> (f32, f32) {
    let mut max_child_height = 0.0_f32;
    let mut current_x = content.x;
    let gap_count = children.len().saturating_sub(1) as f32;
    let gap = if gap_count > 0.0 {
        (content.width - total_child_width).max(0.0) / gap_count
    } else {
        0.0
    };

    for (child_id, child_width) in children {
        let align_y = child_align_y(tree, child_id);

        if let Some(frame) = resolve_child_with_placement(
            tree,
            child_id,
            ResolvePlacement {
                constraint: Constraint::new(*child_width, content.height),
                x: current_x,
                y: content.y,
                inherited,
                use_resolve_cache,
            },
            measurer,
        ) {
            max_child_height = max_child_height.max(frame.content_height);
            apply_vertical_alignment(tree, child_id, content.y, content.height, align_y);
        }

        current_x += *child_width + gap;
    }

    let actual_content_width = if gap_count > 0.0 {
        total_child_width + gap * gap_count
    } else {
        total_child_width
    };

    (actual_content_width, max_child_height)
}

fn resolve_row_grouped<M: TextMeasurer>(
    tree: &mut ElementTree,
    child_ids: &[NodeId],
    plan: &RowLayoutPlan,
    context: RowResolveContext<'_>,
    measurer: &M,
) -> (f32, f32) {
    let RowResolveContext {
        content,
        options,
        inherited,
        use_resolve_cache,
    } = context;

    let left_spacing = spacing_for_count(plan.left_children.len(), options.spacing);
    let center_spacing = spacing_for_count(plan.center_children.len(), options.spacing);
    let right_spacing = spacing_for_count(plan.right_children.len(), options.spacing);

    let total_left_width = plan.total_left_width + left_spacing;
    let total_center_width = plan.total_center_width + center_spacing;
    let total_right_width = plan.total_right_width + right_spacing;

    // Position left-aligned children from left edge.
    let mut current_x = content.x;
    let mut max_child_height = 0.0_f32;

    for (child_id, child_width) in &plan.left_children {
        let align_y = child_align_y(tree, child_id);

        if let Some(frame) = resolve_child_with_placement(
            tree,
            child_id,
            ResolvePlacement {
                constraint: Constraint::new(*child_width, content.height),
                x: current_x,
                y: content.y,
                inherited,
                use_resolve_cache,
            },
            measurer,
        ) {
            max_child_height = max_child_height.max(frame.content_height);
            apply_vertical_alignment(tree, child_id, content.y, content.height, align_y);
        }

        current_x += *child_width + options.spacing;
    }

    // Position right-aligned children from right edge.
    let mut right_x = content.x + content.width;
    for (child_id, child_width) in plan.right_children.iter().rev() {
        let align_y = child_align_y(tree, child_id);

        right_x -= *child_width;
        if let Some(frame) = resolve_child_with_placement(
            tree,
            child_id,
            ResolvePlacement {
                constraint: Constraint::new(*child_width, content.height),
                x: right_x,
                y: content.y,
                inherited,
                use_resolve_cache,
            },
            measurer,
        ) {
            max_child_height = max_child_height.max(frame.content_height);
            apply_vertical_alignment(tree, child_id, content.y, content.height, align_y);
        }

        right_x -= options.spacing;
    }

    // Position center-aligned children in the middle of remaining space.
    if !plan.center_children.is_empty() {
        let left_end = content.x + total_left_width;
        let right_start = content.x + content.width - total_right_width;
        let available_center = (right_start - left_end).max(0.0);
        let center_start = left_end + (available_center - total_center_width) / 2.0;

        let mut center_x = center_start.max(left_end);
        for (child_id, child_width) in &plan.center_children {
            let align_y = child_align_y(tree, child_id);

            if let Some(frame) = resolve_child_with_placement(
                tree,
                child_id,
                ResolvePlacement {
                    constraint: Constraint::new(*child_width, content.height),
                    x: center_x,
                    y: content.y,
                    inherited,
                    use_resolve_cache,
                },
                measurer,
            ) {
                max_child_height = max_child_height.max(frame.content_height);
                apply_vertical_alignment(tree, child_id, content.y, content.height, align_y);
            }

            center_x += *child_width + options.spacing;
        }
    }

    let total_spacing_used = spacing_for_count(child_ids.len(), options.spacing);
    let actual_content_width = plan.total_width + total_spacing_used;

    (actual_content_width, max_child_height)
}

/// Resolve children for Row with fill distribution and self-alignment.
/// Children with align_x position themselves within the row:
/// - Left (default): laid out left-to-right from start
/// - Right: positioned at right edge
/// - Center: centered in remaining space
///   Returns (actual_content_width, actual_content_height).
fn resolve_row_children<M: TextMeasurer>(
    tree: &mut ElementTree,
    child_ids: &[NodeId],
    content: ContentRect,
    options: RowChildrenOptions,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) -> (f32, f32) {
    if child_ids.is_empty() {
        return (0.0, 0.0);
    }

    let plan = build_row_layout_plan(tree, child_ids, options, content.width);

    if options.space_evenly {
        resolve_row_space_evenly(
            tree,
            &plan.children,
            content,
            plan.total_width,
            inherited,
            measurer,
            use_resolve_cache,
        )
    } else {
        resolve_row_grouped(
            tree,
            child_ids,
            &plan,
            RowResolveContext {
                content,
                options,
                inherited,
                use_resolve_cache,
            },
            measurer,
        )
    }
}

/// Apply vertical alignment to a child element.
fn apply_vertical_alignment(
    tree: &mut ElementTree,
    child_id: &NodeId,
    content_y: f32,
    content_height: f32,
    align_y: AlignY,
) {
    if let Some(child) = tree.get(child_id)
        && let Some(frame) = &child.layout.frame
    {
        let aligned_y = match align_y {
            AlignY::Top => content_y,
            AlignY::Center => content_y + (content_height - frame.height) / 2.0,
            AlignY::Bottom => content_y + content_height - frame.height,
        };
        let dy = aligned_y - frame.y;
        if dy != 0.0 {
            shift_subtree(tree, child_id, 0.0, dy);
        }
    }
}

#[derive(Debug)]
struct ColumnLayoutPlan {
    children: Vec<(NodeId, f32)>,
    top_children: Vec<(NodeId, f32)>,
    center_children: Vec<(NodeId, f32)>,
    bottom_children: Vec<(NodeId, f32)>,
    total_center_height: f32,
    total_height: f32,
}

#[derive(Debug)]
struct ColumnPlanSeed {
    id: NodeId,
    height: Option<Length>,
    measured_height: f32,
    fill_portion: f32,
    rotated: bool,
    align_y: AlignY,
}

fn build_column_layout_plan<M: TextMeasurer>(
    tree: &mut ElementTree,
    child_ids: &[NodeId],
    content: ContentRect,
    options: ColumnChildrenOptions,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) -> ColumnLayoutPlan {
    let seeds: Vec<_> = child_ids
        .iter()
        .filter_map(|child_id| {
            let child = tree.get(child_id)?;
            let height = child.layout.effective.height.clone();

            Some(ColumnPlanSeed {
                id: *child_id,
                measured_height: child_measured_height(tree, child_id),
                fill_portion: if options.allow_fill_height {
                    get_fill_weight(height.as_ref())
                } else {
                    0.0
                },
                rotated: layout_rotate_degrees(&child.layout.effective).is_some(),
                align_y: child.layout.effective.align_y.unwrap_or_default(),
                height,
            })
        })
        .collect();

    let total_portions = seeds.iter().map(|seed| seed.fill_portion).sum::<f32>();
    let resolve_fixed_children = options.allow_fill_height && total_portions > 0.0;
    let resolved_seeds: Vec<_> = seeds
        .into_iter()
        .map(|seed| {
            let planned_height = if seed.rotated {
                seed.measured_height
            } else {
                resolve_planned_length(seed.height.as_ref(), seed.measured_height, None)
            };
            let height_can_reflow =
                seed.rotated || length_allows_content_expansion(seed.height.as_ref());
            let resolved_height =
                if resolve_fixed_children && seed.fill_portion == 0.0 && height_can_reflow {
                    resolve_child_with_placement(
                        tree,
                        &seed.id,
                        ResolvePlacement {
                            constraint: Constraint::new(content.width, planned_height),
                            x: content.x,
                            y: content.y,
                            inherited,
                            use_resolve_cache,
                        },
                        measurer,
                    )
                    .map(|frame| frame.height)
                    .unwrap_or(planned_height)
                } else {
                    planned_height
                };

            (seed, resolved_height)
        })
        .collect();
    let fixed_height = resolved_seeds
        .iter()
        .filter(|(seed, _)| seed.fill_portion == 0.0)
        .map(|(_, height)| *height)
        .sum::<f32>();

    // Calculate height per portion after width-dependent fixed children have
    // resolved against the column's final content width. Their measured heights
    // can be stale when a paragraph or wrapped row reflows.
    let effective_spacing = if options.space_evenly {
        0.0
    } else {
        options.spacing
    };
    let total_spacing = effective_spacing * (child_ids.len().saturating_sub(1)) as f32;
    let remaining = (content.height - fixed_height - total_spacing).max(0.0);
    let height_per_portion = if total_portions > 0.0 {
        remaining / total_portions
    } else {
        0.0
    };

    // Partition children by vertical alignment and calculate heights.
    let mut children: Vec<(NodeId, f32)> = Vec::new();
    let mut top_children: Vec<(NodeId, f32)> = Vec::new();
    let mut center_children: Vec<(NodeId, f32)> = Vec::new();
    let mut bottom_children: Vec<(NodeId, f32)> = Vec::new();
    let mut total_center_height = 0.0_f32;
    let mut total_height = 0.0_f32;

    for (seed, resolved_height) in resolved_seeds {
        let height = if seed.fill_portion > 0.0 {
            resolve_planned_length(
                seed.height.as_ref(),
                seed.measured_height,
                Some(height_per_portion),
            )
        } else {
            resolved_height
        };
        children.push((seed.id, height));
        total_height += height;

        match seed.align_y {
            AlignY::Top => top_children.push((seed.id, height)),
            AlignY::Center => {
                center_children.push((seed.id, height));
                total_center_height += height;
            }
            AlignY::Bottom => bottom_children.push((seed.id, height)),
        }
    }

    ColumnLayoutPlan {
        children,
        top_children,
        center_children,
        bottom_children,
        total_center_height,
        total_height,
    }
}

fn resolve_column_space_evenly<M: TextMeasurer>(
    tree: &mut ElementTree,
    children: &[(NodeId, f32)],
    content: ContentRect,
    total_child_height: f32,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) -> f32 {
    let mut current_y = content.y;
    let gap_count = children.len().saturating_sub(1) as f32;
    let gap = if gap_count > 0.0 {
        (content.height - total_child_height).max(0.0) / gap_count
    } else {
        0.0
    };
    let mut total_height = 0.0_f32;

    for (child_id, child_height) in children {
        let align_x = child_align_x(tree, child_id);

        let frame = resolve_child_with_placement(
            tree,
            child_id,
            ResolvePlacement {
                constraint: Constraint::new(content.width, *child_height),
                x: content.x,
                y: current_y,
                inherited,
                use_resolve_cache,
            },
            measurer,
        );
        let actual_height = frame
            .map(|snapshot| snapshot.height)
            .unwrap_or(*child_height);

        apply_horizontal_alignment(tree, child_id, content.x, content.width, align_x);

        total_height += actual_height;
        current_y += actual_height + gap;
    }

    if gap_count > 0.0 {
        total_height += gap * gap_count;
    }

    total_height.max(0.0)
}

fn resolve_column_grouped<M: TextMeasurer>(
    tree: &mut ElementTree,
    content: ContentRect,
    options: ColumnChildrenOptions,
    plan: &ColumnLayoutPlan,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) -> f32 {
    let top_spacing = spacing_for_count(plan.top_children.len(), options.spacing);
    let center_spacing = spacing_for_count(plan.center_children.len(), options.spacing);
    let bottom_spacing = spacing_for_count(plan.bottom_children.len(), options.spacing);
    let total_center_height = plan.total_center_height + center_spacing;

    // Position top-aligned children from top edge.
    let mut current_y = content.y;
    let mut actual_top_height = 0.0_f32;

    for (child_id, child_height) in &plan.top_children {
        let align_x = child_align_x(tree, child_id);

        let frame = resolve_child_with_placement(
            tree,
            child_id,
            ResolvePlacement {
                constraint: Constraint::new(content.width, *child_height),
                x: content.x,
                y: current_y,
                inherited,
                use_resolve_cache,
            },
            measurer,
        );
        let actual_height = frame
            .map(|snapshot| snapshot.height)
            .unwrap_or(*child_height);

        apply_horizontal_alignment(tree, child_id, content.x, content.width, align_x);

        actual_top_height += actual_height;
        current_y += actual_height + options.spacing;
    }
    if !plan.top_children.is_empty() {
        actual_top_height += top_spacing;
    }

    // Position bottom-aligned children.
    let mut actual_bottom_height = 0.0_f32;

    if options.is_scrollable {
        let mut current_bottom_y = content.y + actual_top_height;
        for (child_id, child_height) in &plan.bottom_children {
            let align_x = child_align_x(tree, child_id);

            let frame = resolve_child_with_placement(
                tree,
                child_id,
                ResolvePlacement {
                    constraint: Constraint::new(content.width, *child_height),
                    x: content.x,
                    y: current_bottom_y,
                    inherited,
                    use_resolve_cache,
                },
                measurer,
            );
            let actual_height = frame
                .map(|snapshot| snapshot.height)
                .unwrap_or(*child_height);

            apply_horizontal_alignment(tree, child_id, content.x, content.width, align_x);

            actual_bottom_height += actual_height;
            current_bottom_y += actual_height + options.spacing;
        }
        if !plan.bottom_children.is_empty() {
            actual_bottom_height += bottom_spacing;
        }
    } else {
        let mut bottom_y = content.y + content.height;
        for (child_id, child_height) in plan.bottom_children.iter().rev() {
            let align_x = child_align_x(tree, child_id);

            bottom_y -= *child_height;
            let frame = resolve_child_with_placement(
                tree,
                child_id,
                ResolvePlacement {
                    constraint: Constraint::new(content.width, *child_height),
                    x: content.x,
                    y: bottom_y,
                    inherited,
                    use_resolve_cache,
                },
                measurer,
            );
            let actual_height = frame
                .map(|snapshot| snapshot.height)
                .unwrap_or(*child_height);

            let height_diff = actual_height - *child_height;
            if height_diff != 0.0 {
                bottom_y -= height_diff;
                shift_subtree(tree, child_id, 0.0, -height_diff);
            }

            apply_horizontal_alignment(tree, child_id, content.x, content.width, align_x);

            actual_bottom_height += actual_height;
            bottom_y -= options.spacing;
        }
        if !plan.bottom_children.is_empty() {
            actual_bottom_height += bottom_spacing;
        }
    }

    // Position center-aligned children in the middle of remaining space.
    let mut actual_center_height = 0.0_f32;
    if !plan.center_children.is_empty() {
        let top_end = content.y + actual_top_height;
        let bottom_start = if options.is_scrollable {
            content.y + actual_top_height
        } else {
            content.y + content.height - actual_bottom_height
        };
        let available_center = (bottom_start - top_end).max(0.0);
        let center_start = top_end + (available_center - total_center_height) / 2.0;

        let mut center_y = center_start.max(top_end);
        for (child_id, child_height) in &plan.center_children {
            let align_x = child_align_x(tree, child_id);

            let frame = resolve_child_with_placement(
                tree,
                child_id,
                ResolvePlacement {
                    constraint: Constraint::new(content.width, *child_height),
                    x: content.x,
                    y: center_y,
                    inherited,
                    use_resolve_cache,
                },
                measurer,
            );
            let actual_height = frame
                .map(|snapshot| snapshot.height)
                .unwrap_or(*child_height);

            apply_horizontal_alignment(tree, child_id, content.x, content.width, align_x);

            actual_center_height += actual_height;
            center_y += actual_height + options.spacing;
        }
        actual_center_height += center_spacing;
    }

    let mut non_empty_zones = 0_usize;
    if !plan.top_children.is_empty() {
        non_empty_zones += 1;
    }
    if !plan.center_children.is_empty() {
        non_empty_zones += 1;
    }
    if !plan.bottom_children.is_empty() {
        non_empty_zones += 1;
    }
    let inter_zone_spacing = spacing_for_count(non_empty_zones, options.spacing);

    actual_top_height + actual_center_height + actual_bottom_height + inter_zone_spacing
}

/// Resolve children for Column with fill distribution and vertical self-alignment.
/// Children are partitioned by align_y into top/center/bottom zones.
/// For scrollable columns, bottom-aligned children are positioned after top content.
/// For non-scrollable columns, bottom-aligned children are at the container bottom.
/// Returns the actual content height after resolution.
fn resolve_column_children<M: TextMeasurer>(
    tree: &mut ElementTree,
    child_ids: &[NodeId],
    content: ContentRect,
    options: ColumnChildrenOptions,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) -> f32 {
    if child_ids.is_empty() {
        return 0.0;
    }

    let plan = build_column_layout_plan(
        tree,
        child_ids,
        content,
        options,
        inherited,
        measurer,
        use_resolve_cache,
    );

    if options.space_evenly {
        resolve_column_space_evenly(
            tree,
            &plan.children,
            content,
            plan.total_height,
            inherited,
            measurer,
            use_resolve_cache,
        )
    } else {
        resolve_column_grouped(
            tree,
            content,
            options,
            &plan,
            inherited,
            measurer,
            use_resolve_cache,
        )
    }
}

#[derive(Clone, Copy, Debug)]
struct FlowFloat {
    side: AlignX,
    x: f32,
    width: f32,
    top: f32,
    bottom: f32,
}

#[derive(Clone, Copy, Debug)]
struct FlowPlacementContext<'a> {
    content_x: f32,
    content_width: f32,
    spacing_x: f32,
    inherited: &'a FontContext,
    active_floats: &'a [FlowFloat],
}

fn prune_flow_floats(active_floats: &mut Vec<FlowFloat>, y: f32) {
    active_floats.retain(|flow_float| flow_float.bottom > y + 0.001);
}

fn max_flow_float_bottom(active_floats: &[FlowFloat]) -> Option<f32> {
    active_floats
        .iter()
        .map(|flow_float| flow_float.bottom)
        .max_by(|a, b| a.total_cmp(b))
}

fn max_flow_float_bottom_for_side(active_floats: &[FlowFloat], side: AlignX) -> Option<f32> {
    active_floats
        .iter()
        .filter(|flow_float| flow_float.side == side)
        .map(|flow_float| flow_float.bottom)
        .max_by(|a, b| a.total_cmp(b))
}

fn next_flow_float_bottom(active_floats: &[FlowFloat], y: f32) -> Option<f32> {
    active_floats
        .iter()
        .filter(|flow_float| flow_float.bottom > y + 0.001)
        .map(|flow_float| flow_float.bottom)
        .min_by(|a, b| a.total_cmp(b))
}

fn flow_line_bounds(
    content_x: f32,
    content_width: f32,
    line_y: f32,
    line_height: f32,
    spacing_x: f32,
    active_floats: &[FlowFloat],
) -> (f32, f32) {
    let mut left = content_x;
    let mut right = content_x + content_width;
    let line_bottom = line_y + line_height.max(1.0);

    for flow_float in active_floats {
        let overlaps_line = flow_float.bottom > line_y && flow_float.top < line_bottom;
        if !overlaps_line {
            continue;
        }

        match flow_float.side {
            AlignX::Left => {
                let candidate =
                    (flow_float.x + flow_float.width + spacing_x).min(content_x + content_width);
                left = left.max(candidate);
            }
            AlignX::Right => {
                let candidate = (flow_float.x - spacing_x).max(content_x);
                right = right.min(candidate);
            }
            AlignX::Center => {}
        }
    }

    (left, right.max(left))
}

fn place_flow_float<M: TextMeasurer>(
    tree: &mut ElementTree,
    child_id: &NodeId,
    side: AlignX,
    desired_y: f32,
    context: FlowPlacementContext<'_>,
    measurer: &M,
    use_resolve_cache: bool,
) -> Option<FlowFloat> {
    let (desired_width, desired_height) = {
        let child = tree.get(child_id)?;
        let intrinsic_frame = child.layout.measured_frame.or(child.layout.frame);
        let intrinsic_width = intrinsic_frame.map(|frame| frame.width).unwrap_or(0.0);
        let intrinsic_height = intrinsic_frame.map(|frame| frame.height).unwrap_or(0.0);

        let width =
            resolve_intrinsic_length(child.layout.effective.width.as_ref(), intrinsic_width)
                .max(0.0)
                .min(context.content_width);
        let height =
            resolve_intrinsic_length(child.layout.effective.height.as_ref(), intrinsic_height)
                .max(0.0);
        (width, height)
    };

    let mut float_y = desired_y;
    if let Some(side_bottom) = max_flow_float_bottom_for_side(context.active_floats, side) {
        float_y = float_y.max(side_bottom);
    }

    let (line_left, line_right, float_y) = loop {
        let (line_left, line_right) = flow_line_bounds(
            context.content_x,
            context.content_width,
            float_y,
            desired_height.max(1.0),
            context.spacing_x,
            context.active_floats,
        );

        let available_width = (line_right - line_left).max(0.0);
        if desired_width <= available_width + 0.001 {
            break (line_left, line_right, float_y);
        }

        let Some(next_y) = next_flow_float_bottom(context.active_floats, float_y) else {
            break (line_left, line_right, float_y);
        };

        if next_y <= float_y + 0.001 {
            break (line_left, line_right, float_y);
        }

        float_y = next_y;
    };

    let float_x = match side {
        AlignX::Left => line_left,
        AlignX::Right => (line_right - desired_width).max(line_left),
        AlignX::Center => line_left,
    };

    let child_constraint = Constraint::new(desired_width.max(0.0), desired_height.max(0.0));
    resolve_element(
        tree,
        child_id,
        ResolvePlacement {
            constraint: child_constraint,
            x: float_x,
            y: float_y,
            inherited: context.inherited,
            use_resolve_cache,
        },
        measurer,
    );

    let mut frame = tree.get(child_id).and_then(|child| child.layout.frame)?;

    if matches!(side, AlignX::Left | AlignX::Right) {
        let (left, right) = flow_line_bounds(
            context.content_x,
            context.content_width,
            frame.y,
            frame.height.max(1.0),
            context.spacing_x,
            context.active_floats,
        );

        let target_x = match side {
            AlignX::Left => left,
            AlignX::Right => (right - frame.width).max(left),
            AlignX::Center => frame.x,
        };

        let dx = target_x - frame.x;
        if dx != 0.0 {
            shift_subtree(tree, child_id, dx, 0.0);
            frame.x += dx;
        }
    }

    Some(FlowFloat {
        side,
        x: frame.x,
        width: frame.width,
        top: frame.y,
        bottom: frame.y + frame.height,
    })
}

fn resolve_paragraph_with_flow<M: TextMeasurer>(
    tree: &mut ElementTree,
    child_id: &NodeId,
    layout: TextFlowLayoutContext<'_>,
    y: f32,
    measurer: &M,
    active_floats: &mut Vec<FlowFloat>,
    use_resolve_cache: bool,
) {
    let child_constraint = Constraint::new(layout.content.width, f32::MAX);
    resolve_element(
        tree,
        child_id,
        ResolvePlacement {
            constraint: child_constraint,
            x: layout.content.x,
            y,
            inherited: layout.inherited,
            use_resolve_cache: false,
        },
        measurer,
    );

    let (child_ids, attrs, frame) = {
        let Some(child) = tree.get(child_id) else {
            return;
        };
        (
            tree.child_ids(child_id),
            child.layout.effective.clone(),
            child.layout.frame,
        )
    };
    let Some(frame) = frame else {
        return;
    };

    let insets = LayoutInsets::from_attrs(&attrs);
    let (content_x, content_y, content_width, content_height) =
        insets.content_rect(frame.x, frame.y, frame.width, frame.height);
    let spacing_x = spacing_x(&attrs);
    let spacing_y = spacing_y(&attrs);
    let is_scrollable = attrs.scrollbar_x.unwrap_or(false) || attrs.scrollbar_y.unwrap_or(false);
    let element_context = layout.inherited.merge_with_attrs(&attrs);

    let (fragments, actual_content_height) = resolve_paragraph_children(
        tree,
        &child_ids,
        TextFlowLayoutContext {
            content: ContentRect {
                x: content_x,
                y: content_y,
                width: content_width,
                height: content_height,
            },
            spacing_x,
            spacing_y,
            inherited: &element_context,
        },
        measurer,
        active_floats,
        use_resolve_cache,
    );

    if let Some(element) = tree.get_mut(child_id) {
        element.layout.paragraph_fragments = Some(fragments);
    }

    if actual_content_height > content_height && !is_scrollable {
        expand_frame_height_to_content(tree, child_id, actual_content_height, insets);
    } else {
        set_frame_content_height(tree, child_id, actual_content_height, insets);
    }
}

fn resolve_text_column_children<M: TextMeasurer>(
    tree: &mut ElementTree,
    child_ids: &[NodeId],
    layout: TextFlowLayoutContext<'_>,
    measurer: &M,
    use_resolve_cache: bool,
) -> f32 {
    if child_ids.is_empty() {
        return 0.0;
    }

    let content_x = layout.content.x;
    let content_y = layout.content.y;
    let content_width = layout.content.width;
    let spacing_x = layout.spacing_x;
    let spacing_y = layout.spacing_y;
    let inherited = layout.inherited;

    let mut active_floats: Vec<FlowFloat> = Vec::new();
    let mut next_flow_y = content_y;
    let mut max_bottom = content_y;
    let mut has_prior_child = false;

    for child_id in child_ids {
        if has_prior_child {
            next_flow_y += spacing_y;
        }
        has_prior_child = true;

        prune_flow_floats(&mut active_floats, next_flow_y);

        let (kind, child_align_x) = {
            let Some(child) = tree.get(child_id) else {
                continue;
            };
            (child.spec.kind, child.layout.effective.align_x)
        };

        if let Some(side) = child_align_x
            && matches!(side, AlignX::Left | AlignX::Right)
        {
            if let Some(flow_float) = place_flow_float(
                tree,
                child_id,
                side,
                next_flow_y,
                FlowPlacementContext {
                    content_x,
                    content_width,
                    spacing_x,
                    inherited,
                    active_floats: &active_floats,
                },
                measurer,
                use_resolve_cache,
            ) {
                max_bottom = max_bottom.max(flow_float.bottom);
                active_floats.push(flow_float);
            }
            continue;
        }

        let mut child_y = next_flow_y;
        if kind != ElementKind::Paragraph
            && let Some(float_bottom) = max_flow_float_bottom(&active_floats)
        {
            child_y = child_y.max(float_bottom);
            prune_flow_floats(&mut active_floats, child_y);
        }

        if kind == ElementKind::Paragraph {
            resolve_paragraph_with_flow(
                tree,
                child_id,
                layout,
                child_y,
                measurer,
                &mut active_floats,
                use_resolve_cache,
            );
        } else {
            let child_constraint = Constraint::new(content_width, f32::MAX);
            resolve_element(
                tree,
                child_id,
                ResolvePlacement {
                    constraint: child_constraint,
                    x: content_x,
                    y: child_y,
                    inherited,
                    use_resolve_cache,
                },
                measurer,
            );
        }

        let align_x = tree
            .get(child_id)
            .map(|child| child.layout.effective.align_x.unwrap_or_default())
            .unwrap_or_default();
        apply_horizontal_alignment(tree, child_id, content_x, content_width, align_x);

        let child_bottom = tree
            .get(child_id)
            .and_then(|child| child.layout.frame.as_ref())
            .map(|frame| frame.y + frame.height)
            .unwrap_or(child_y);

        next_flow_y = child_bottom;
        max_bottom = max_bottom.max(child_bottom);
        if let Some(float_bottom) = max_flow_float_bottom(&active_floats) {
            max_bottom = max_bottom.max(float_bottom);
        }
    }

    (max_bottom - content_y).max(0.0)
}

/// Apply horizontal alignment to a child element.
fn apply_horizontal_alignment(
    tree: &mut ElementTree,
    child_id: &NodeId,
    content_x: f32,
    content_width: f32,
    align_x: AlignX,
) {
    if let Some(child) = tree.get(child_id)
        && let Some(frame) = &child.layout.frame
    {
        let aligned_x = match align_x {
            AlignX::Left => content_x,
            AlignX::Center => content_x + (content_width - frame.width) / 2.0,
            AlignX::Right => content_x + content_width - frame.width,
        };
        let dx = aligned_x - frame.x;
        if dx != 0.0 {
            shift_subtree(tree, child_id, dx, 0.0);
        }
    }
}

/// Resolve children for WrappedRow.
/// Reads from pre-scaled attrs.
/// Returns the actual content height after wrapping.
fn resolve_wrapped_row_children<M: TextMeasurer>(
    tree: &mut ElementTree,
    child_ids: &[NodeId],
    content: ContentRect,
    options: WrappedRowChildrenOptions,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) -> f32 {
    if child_ids.is_empty() {
        return 0.0;
    }

    // Build lines by wrapping (attrs are pre-scaled).
    // Width determines line membership; actual heights are measured after each child
    // is resolved against its final line width.
    let mut lines: Vec<Vec<(NodeId, f32)>> = Vec::new(); // (id, width)
    let mut current_line: Vec<(NodeId, f32)> = Vec::new();
    let mut current_line_width = 0.0;

    for child_id in child_ids {
        let Some(_child) = tree.get(child_id) else {
            continue;
        };
        let Some(child) = tree.get(child_id) else {
            continue;
        };
        let intrinsic_width = child_measured_width(tree, child_id);
        let child_width = if get_fill_weight(child.layout.effective.width.as_ref()) > 0.0 {
            resolve_length(
                child.layout.effective.width.as_ref(),
                intrinsic_width,
                content.width,
            )
        } else if layout_rotate_degrees(&child.layout.effective).is_some() {
            intrinsic_width
        } else {
            resolve_length(
                child.layout.effective.width.as_ref(),
                intrinsic_width,
                intrinsic_width,
            )
        };

        // Check if we need to wrap
        let would_exceed = !current_line.is_empty()
            && current_line_width + options.spacing_x + child_width > content.width;

        if would_exceed {
            lines.push(std::mem::take(&mut current_line));
            current_line_width = 0.0;
        }

        if !current_line.is_empty() {
            current_line_width += options.spacing_x;
        }
        current_line_width += child_width;
        current_line.push((*child_id, child_width));
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    // Layout each line and track total height
    let mut current_y = content.y;
    let num_lines = lines.len();

    for line in lines {
        let line_children: Vec<(NodeId, AlignY)> = line
            .iter()
            .map(|(child_id, _)| (*child_id, child_align_y(tree, child_id)))
            .collect();
        let plan = build_row_layout_plan_from_widths(tree, &line);
        let line_height = resolve_grouped_row_line(
            tree,
            ContentRect {
                x: content.x,
                y: current_y,
                width: content.width,
                height: content.height,
            },
            options.spacing_x,
            &plan,
            inherited,
            measurer,
            use_resolve_cache,
        );

        for (child_id, align_y) in &line_children {
            apply_vertical_alignment(tree, child_id, current_y, line_height, *align_y);
        }

        current_y += line_height + options.spacing_y;
    }

    // Return total content height (subtract trailing spacing)
    let total_height = current_y - content.y;
    if num_lines > 0 {
        total_height - options.spacing_y // Remove trailing spacing
    } else {
        0.0
    }
}

// =============================================================================
// Paragraph Resolution
// =============================================================================

/// Extract inline text content and font context from a child element.
/// Returns (text_content, font_context) or None if child is not a text source.
fn extract_inline_text(
    tree: &ElementTree,
    child_id: &NodeId,
    inherited: &FontContext,
) -> Option<(String, FontContext)> {
    let child = tree.get(child_id)?;

    match child.spec.kind {
        ElementKind::Text => {
            let content = child.layout.effective.content.as_deref()?.to_string();
            let font_ctx = inherited.merge_with_attrs(&child.layout.effective);
            Some((content, font_ctx))
        }
        ElementKind::El => {
            // Look for the first text child of this el wrapper
            let el_context = inherited.merge_with_attrs(&child.layout.effective);
            for grandchild_id in tree.child_ids(&child.id) {
                let grandchild = tree.get(&grandchild_id)?;
                if grandchild.spec.kind == ElementKind::Text {
                    let content = grandchild.layout.effective.content.as_deref()?.to_string();
                    let font_ctx = el_context.merge_with_attrs(&grandchild.layout.effective);
                    return Some((content, font_ctx));
                }
            }
            None
        }
        _ => None,
    }
}

/// Resolve paragraph children by word-wrapping text content.
/// Returns (fragments, total_content_height).
fn resolve_paragraph_children<M: TextMeasurer>(
    tree: &mut ElementTree,
    child_ids: &[NodeId],
    layout: TextFlowLayoutContext<'_>,
    measurer: &M,
    active_floats: &mut Vec<FlowFloat>,
    use_resolve_cache: bool,
) -> (Vec<TextFragment>, f32) {
    let content_x = layout.content.x;
    let content_y = layout.content.y;
    let content_width = layout.content.width;
    let spacing_x = layout.spacing_x;
    let spacing_y = layout.spacing_y;
    let inherited = layout.inherited;

    let incoming_float_count = active_floats.len();
    let mut fragments = Vec::new();
    let mut cursor_y = content_y;
    let mut local_float_bottom = content_y;
    let mut line_height: f32 = 0.0;

    prune_flow_floats(active_floats, cursor_y);
    let (mut line_left, _) = flow_line_bounds(
        content_x,
        content_width,
        cursor_y,
        1.0,
        spacing_x,
        active_floats,
    );
    let mut cursor_x = line_left;

    for child_id in child_ids {
        let float_side = tree
            .get(child_id)
            .and_then(|child| child.layout.effective.align_x)
            .filter(|side| matches!(side, AlignX::Left | AlignX::Right));

        if let Some(side) = float_side {
            if line_height > 0.0 && cursor_x > line_left + 0.001 {
                cursor_y += line_height + spacing_y;
                line_height = 0.0;
                prune_flow_floats(active_floats, cursor_y);
                let (next_line_left, _) = flow_line_bounds(
                    content_x,
                    content_width,
                    cursor_y,
                    1.0,
                    spacing_x,
                    active_floats,
                );
                line_left = next_line_left;
                cursor_x = line_left;
            }

            if let Some(flow_float) = place_flow_float(
                tree,
                child_id,
                side,
                cursor_y,
                FlowPlacementContext {
                    content_x,
                    content_width,
                    spacing_x,
                    inherited,
                    active_floats,
                },
                measurer,
                use_resolve_cache,
            ) {
                local_float_bottom = local_float_bottom.max(flow_float.bottom);
                active_floats.push(flow_float);
            }

            let (next_line_left, _) = flow_line_bounds(
                content_x,
                content_width,
                cursor_y,
                line_height.max(1.0),
                spacing_x,
                active_floats,
            );
            line_left = next_line_left;
            if line_height == 0.0 || cursor_x < line_left {
                cursor_x = line_left;
            }

            continue;
        }

        let Some((content, font_ctx)) = extract_inline_text(tree, child_id, inherited) else {
            continue;
        };

        if content.is_empty() {
            continue;
        }

        let font_size = font_ctx.font_size.unwrap_or(16.0);
        let family = font_ctx
            .font_family
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let weight = font_ctx.font_weight.unwrap_or(400);
        let italic = font_ctx.font_italic.unwrap_or(false);
        let color = font_ctx.font_color.unwrap_or(DEFAULT_TEXT_COLOR);
        let underline = font_ctx.font_underline.unwrap_or(false);
        let strike = font_ctx.font_strike.unwrap_or(false);

        let (_, text_height) = measurer.measure_with_font("Hg", font_size, &family, weight, italic);
        let (ascent, _descent) = measurer.font_metrics(font_size, &family, weight, italic);

        let space_width =
            measurer.measure_visual_width_with_font(" ", font_size, &family, weight, italic);

        // Split content into words
        let words: Vec<&str> = content.split_whitespace().collect();
        let starts_with_space = content.starts_with(char::is_whitespace);
        let ends_with_space = content.ends_with(char::is_whitespace);

        // Add leading space if content starts with whitespace
        if starts_with_space && !words.is_empty() {
            let (next_line_left, line_right) = flow_line_bounds(
                content_x,
                content_width,
                cursor_y,
                line_height.max(text_height).max(1.0),
                spacing_x,
                active_floats,
            );
            line_left = next_line_left;
            if cursor_x < line_left {
                cursor_x = line_left;
            }
            if cursor_x > line_left + 0.001 && cursor_x + space_width > line_right {
                cursor_y += line_height + spacing_y;
                line_height = 0.0;
                prune_flow_floats(active_floats, cursor_y);
                let (next_line_left, _) = flow_line_bounds(
                    content_x,
                    content_width,
                    cursor_y,
                    1.0,
                    spacing_x,
                    active_floats,
                );
                line_left = next_line_left;
                cursor_x = line_left;
            }
            cursor_x += space_width;
        }

        for (i, word) in words.iter().enumerate() {
            let word_width =
                measurer.measure_visual_width_with_font(word, font_size, &family, weight, italic);

            loop {
                prune_flow_floats(active_floats, cursor_y);
                let (next_line_left, line_right) = flow_line_bounds(
                    content_x,
                    content_width,
                    cursor_y,
                    line_height.max(text_height).max(1.0),
                    spacing_x,
                    active_floats,
                );
                line_left = next_line_left;

                if cursor_x < line_left {
                    cursor_x = line_left;
                }

                let available_width = (line_right - line_left).max(0.0);
                if available_width <= 0.001
                    && let Some(next_y) = next_flow_float_bottom(active_floats, cursor_y)
                    && next_y > cursor_y + 0.001
                {
                    cursor_y = next_y;
                    line_height = 0.0;
                    cursor_x = content_x;
                    continue;
                }

                // Wrap if word doesn't fit and we're not at line start
                if cursor_x > line_left + 0.001 && cursor_x + word_width > line_right {
                    cursor_y += line_height + spacing_y;
                    line_height = 0.0;
                    cursor_x = content_x;
                    continue;
                }

                break;
            }

            fragments.push(TextFragment {
                x: cursor_x,
                y: cursor_y,
                text: word.to_string(),
                font_size,
                color,
                family: family.clone(),
                weight,
                italic,
                underline,
                strike,
                ascent,
            });

            cursor_x += word_width;
            line_height = line_height.max(text_height);

            // Add space after word (unless last word)
            if i < words.len() - 1 {
                let (next_line_left, line_right) = flow_line_bounds(
                    content_x,
                    content_width,
                    cursor_y,
                    line_height.max(1.0),
                    spacing_x,
                    active_floats,
                );
                line_left = next_line_left;

                if cursor_x < line_left {
                    cursor_x = line_left;
                }

                if cursor_x > line_left + 0.001 && cursor_x + space_width > line_right {
                    cursor_y += line_height + spacing_y;
                    line_height = 0.0;
                    prune_flow_floats(active_floats, cursor_y);
                    let (next_line_left, _) = flow_line_bounds(
                        content_x,
                        content_width,
                        cursor_y,
                        1.0,
                        spacing_x,
                        active_floats,
                    );
                    line_left = next_line_left;
                    cursor_x = line_left;
                } else {
                    cursor_x += space_width;
                }
            }
        }

        // Add trailing space if content ends with whitespace
        if ends_with_space && !words.is_empty() {
            let (next_line_left, line_right) = flow_line_bounds(
                content_x,
                content_width,
                cursor_y,
                line_height.max(1.0),
                spacing_x,
                active_floats,
            );
            line_left = next_line_left;

            if cursor_x < line_left {
                cursor_x = line_left;
            }
            if cursor_x > line_left + 0.001 && cursor_x + space_width > line_right {
                cursor_y += line_height + spacing_y;
                line_height = 0.0;
                prune_flow_floats(active_floats, cursor_y);
                let (next_line_left, _) = flow_line_bounds(
                    content_x,
                    content_width,
                    cursor_y,
                    1.0,
                    spacing_x,
                    active_floats,
                );
                line_left = next_line_left;
                cursor_x = line_left;
            }
            cursor_x += space_width;
        }
    }

    if active_floats.len() > incoming_float_count {
        for flow_float in active_floats.iter().skip(incoming_float_count) {
            local_float_bottom = local_float_bottom.max(flow_float.bottom);
        }
    }

    let text_bottom = if line_height > 0.0 {
        cursor_y + line_height
    } else {
        content_y
    };
    let total_height = (text_bottom.max(local_float_bottom) - content_y).max(0.0);

    (fragments, total_height)
}

// =============================================================================
// Helpers
// =============================================================================

/// Resolved padding values.
#[derive(Clone, Copy, Debug, Default)]
struct ResolvedPadding {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct LayoutInsets {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

impl LayoutInsets {
    fn from_attrs(attrs: &Attrs) -> Self {
        let padding = get_padding(attrs.padding.as_ref());
        let border = get_border_inset(attrs.border_width.as_ref());
        Self {
            top: padding.top + border.top,
            right: padding.right + border.right,
            bottom: padding.bottom + border.bottom,
            left: padding.left + border.left,
        }
    }

    fn horizontal(self) -> f32 {
        self.left + self.right
    }

    fn vertical(self) -> f32 {
        self.top + self.bottom
    }

    fn outer_width(self, content_width: f32) -> f32 {
        content_width + self.horizontal()
    }

    fn outer_height(self, content_height: f32) -> f32 {
        content_height + self.vertical()
    }

    fn content_rect(self, x: f32, y: f32, width: f32, height: f32) -> (f32, f32, f32, f32) {
        (
            x + self.left,
            y + self.top,
            (width - self.horizontal()).max(0.0),
            (height - self.vertical()).max(0.0),
        )
    }
}

/// Get padding as resolved values.
fn get_padding(padding: Option<&Padding>) -> ResolvedPadding {
    match padding {
        Some(Padding::Uniform(p)) => {
            let p = *p as f32;
            ResolvedPadding {
                top: p,
                right: p,
                bottom: p,
                left: p,
            }
        }
        Some(Padding::Sides {
            top,
            right,
            bottom,
            left,
        }) => ResolvedPadding {
            top: *top as f32,
            right: *right as f32,
            bottom: *bottom as f32,
            left: *left as f32,
        },
        None => ResolvedPadding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        },
    }
}

/// Get border width as resolved inset values (same shape as padding).
fn get_border_inset(border_width: Option<&BorderWidth>) -> ResolvedPadding {
    match border_width {
        Some(BorderWidth::Uniform(w)) => {
            let w = *w as f32;
            ResolvedPadding {
                top: w,
                right: w,
                bottom: w,
                left: w,
            }
        }
        Some(BorderWidth::Sides {
            top,
            right,
            bottom,
            left,
        }) => ResolvedPadding {
            top: *top as f32,
            right: *right as f32,
            bottom: *bottom as f32,
            left: *left as f32,
        },
        None => ResolvedPadding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        },
    }
}

fn spacing_x(attrs: &Attrs) -> f32 {
    attrs.spacing_x.or(attrs.spacing).unwrap_or(0.0) as f32
}

fn spacing_y(attrs: &Attrs) -> f32 {
    attrs.spacing_y.or(attrs.spacing).unwrap_or(0.0) as f32
}

fn set_frame_content_width(
    tree: &mut ElementTree,
    id: &NodeId,
    actual_content_width: f32,
    insets: LayoutInsets,
) {
    let before_geometry = registry_geometry_snapshot(tree, id);
    if let Some(element) = tree.get_mut(id)
        && let Some(ref mut frame) = element.layout.frame
    {
        frame.content_width = insets.outer_width(actual_content_width);
    }
    mark_registry_dirty_if_geometry_changed(tree, id, before_geometry);
}

fn set_frame_content_height(
    tree: &mut ElementTree,
    id: &NodeId,
    actual_content_height: f32,
    insets: LayoutInsets,
) {
    let before_geometry = registry_geometry_snapshot(tree, id);
    if let Some(element) = tree.get_mut(id)
        && let Some(ref mut frame) = element.layout.frame
    {
        frame.content_height = insets.outer_height(actual_content_height);
    }
    mark_registry_dirty_if_geometry_changed(tree, id, before_geometry);
}

fn set_frame_content_size(
    tree: &mut ElementTree,
    id: &NodeId,
    actual_content_width: f32,
    actual_content_height: f32,
    insets: LayoutInsets,
) {
    let before_geometry = registry_geometry_snapshot(tree, id);
    if let Some(element) = tree.get_mut(id)
        && let Some(ref mut frame) = element.layout.frame
    {
        frame.content_width = insets.outer_width(actual_content_width);
        frame.content_height = insets.outer_height(actual_content_height);
    }
    mark_registry_dirty_if_geometry_changed(tree, id, before_geometry);
}

fn expand_frame_height_to_content(
    tree: &mut ElementTree,
    id: &NodeId,
    actual_content_height: f32,
    insets: LayoutInsets,
) {
    let new_height = insets.outer_height(actual_content_height);
    let before_geometry = registry_geometry_snapshot(tree, id);
    if let Some(element) = tree.get_mut(id)
        && let Some(ref mut frame) = element.layout.frame
    {
        frame.height = new_height;
        frame.content_height = new_height;
    }
    mark_registry_dirty_if_geometry_changed(tree, id, before_geometry);
}

fn update_scroll_state(tree: &mut ElementTree, id: &NodeId) {
    let before_geometry = registry_geometry_snapshot(tree, id);
    let scroll_cache_context_active = {
        let Some(element) = tree.get_mut(id) else {
            return;
        };

        let scroll_x_enabled = effective_scrollbar_x(&element.layout.effective);
        let scroll_y_enabled = effective_scrollbar_y(&element.layout.effective);

        if !scroll_x_enabled {
            element.layout.scroll_x = 0.0;
            element.layout.scroll_x_max = 0.0;
        }
        if !scroll_y_enabled {
            element.layout.scroll_y = 0.0;
            element.layout.scroll_y_max = 0.0;
        }

        if (scroll_x_enabled || scroll_y_enabled)
            && let Some(frame) = element.layout.frame
        {
            let max_x = (frame.content_width - frame.width).max(0.0);
            let max_y = (frame.content_height - frame.height).max(0.0);
            let prev_max_x = if element.layout.scroll_x_max == 0.0 {
                max_x
            } else {
                element.layout.scroll_x_max
            };
            let prev_max_y = if element.layout.scroll_y_max == 0.0 {
                max_y
            } else {
                element.layout.scroll_y_max
            };
            let prev_scroll_x = element.layout.scroll_x;
            let prev_scroll_y = element.layout.scroll_y;

            if scroll_x_enabled {
                let delta_x = max_x - prev_max_x;
                let at_end_x = prev_max_x > 0.0 && (prev_scroll_x - prev_max_x).abs() < 0.5;
                let next_scroll_x = if max_x < prev_max_x {
                    prev_scroll_x.min(max_x)
                } else if at_end_x {
                    prev_scroll_x + delta_x
                } else {
                    prev_scroll_x
                }
                .clamp(0.0, max_x);
                element.layout.scroll_x = next_scroll_x;
                element.layout.scroll_x_max = max_x;
            }

            if scroll_y_enabled {
                let delta_y = max_y - prev_max_y;
                let at_end_y = prev_max_y > 0.0 && (prev_scroll_y - prev_max_y).abs() < 0.5;
                let next_scroll_y = if max_y < prev_max_y {
                    prev_scroll_y.min(max_y)
                } else if at_end_y {
                    prev_scroll_y + delta_y
                } else {
                    prev_scroll_y
                }
                .clamp(0.0, max_y);
                element.layout.scroll_y = next_scroll_y;
                element.layout.scroll_y_max = max_y;
            }
        }

        (element.layout.scroll_x > f32::EPSILON || element.layout.scroll_y > f32::EPSILON)
            && (element.layout.scroll_x_max > f32::EPSILON
                || element.layout.scroll_y_max > f32::EPSILON)
    };
    if scroll_cache_context_active {
        tree.mark_scroll_cache_context_active();
    }
    mark_registry_dirty_if_geometry_changed(tree, id, before_geometry);
}

fn shift_subtree(tree: &mut ElementTree, id: &NodeId, dx: f32, dy: f32) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }

    let before_geometry = registry_geometry_snapshot(tree, id);
    let child_ids = {
        let Some(element) = tree.get_mut(id) else {
            return;
        };
        if let Some(frame) = &mut element.layout.frame {
            frame.x += dx;
            frame.y += dy;
        }
        if let Some(frame) = &mut element.layout.render_frame {
            frame.x += dx;
            frame.y += dy;
        }
        if let Some(fragments) = &mut element.layout.paragraph_fragments {
            for frag in fragments.iter_mut() {
                frag.x += dx;
                frag.y += dy;
            }
        }

        let mut child_ids = tree.child_ids(id);
        child_ids.extend(tree.nearby_mounts_for(id).into_iter().map(|mount| mount.id));
        child_ids
    };
    mark_registry_dirty_if_geometry_changed(tree, id, before_geometry);

    for child_id in child_ids {
        shift_subtree(tree, &child_id, dx, dy);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RegistryGeometrySnapshot {
    frame: Option<Rect>,
    render_frame: Option<Rect>,
    scroll_x: f32,
    scroll_y: f32,
    scroll_x_max: f32,
    scroll_y_max: f32,
}

fn registry_geometry_snapshot(tree: &ElementTree, id: &NodeId) -> Option<RegistryGeometrySnapshot> {
    tree.get(id).map(|element| RegistryGeometrySnapshot {
        frame: element.layout.frame.map(rect_from_frame_geometry),
        render_frame: element.layout.render_frame.map(rect_from_frame_geometry),
        scroll_x: element.layout.scroll_x,
        scroll_y: element.layout.scroll_y,
        scroll_x_max: element.layout.scroll_x_max,
        scroll_y_max: element.layout.scroll_y_max,
    })
}

fn rect_from_frame_geometry(frame: Frame) -> Rect {
    Rect {
        x: frame.x,
        y: frame.y,
        width: frame.width,
        height: frame.height,
    }
}

fn mark_registry_dirty_if_geometry_changed(
    tree: &mut ElementTree,
    id: &NodeId,
    before: Option<RegistryGeometrySnapshot>,
) {
    let Some(before) = before else {
        return;
    };
    let Some(after) = registry_geometry_snapshot(tree, id) else {
        return;
    };
    if before != after && tree.cached_subtree_affects_registry(id) {
        tree.mark_registry_refresh_dirty(id);
    }
}

fn capture_registry_geometry_snapshots(
    tree: &ElementTree,
) -> HashMap<NodeId, RegistryGeometrySnapshot> {
    tree.iter_nodes()
        .filter(|element| tree.cached_subtree_affects_registry(&element.id))
        .filter_map(|element| {
            registry_geometry_snapshot(tree, &element.id).map(|snapshot| (element.id, snapshot))
        })
        .collect()
}

fn registry_geometry_changed_since(
    tree: &ElementTree,
    before: &HashMap<NodeId, RegistryGeometrySnapshot>,
) -> bool {
    before.iter().any(|(id, snapshot)| {
        registry_geometry_snapshot(tree, id).is_none_or(|after| after != *snapshot)
    })
}

// =============================================================================
// Layout Output (combined render + event registry)
// =============================================================================

use super::render::{
    RefreshBuildOutput, RefreshRegistryMode, RefreshRegistryOutput,
    build_refresh_output_with_scroll_layers,
};
use crate::events::{RegistryRebuildPayload, TextInputState};
use crate::render_scene::RenderScene;

/// Output of layout refresh: both render commands and event registry.
pub struct LayoutOutput {
    pub scene: RenderScene,
    pub event_rebuild: RegistryRebuildPayload,
    pub event_rebuild_changed: bool,
    pub ime_enabled: bool,
    pub ime_cursor_area: Option<(f32, f32, f32, f32)>,
    pub ime_text_state: Option<TextInputState>,
    pub animations_active: bool,
}

pub struct LayoutUpdateOutput {
    pub output: LayoutOutput,
    pub layout_performed: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LayoutUpdateTiming {
    pub layout: Duration,
    pub refresh: Duration,
    pub refresh_traversal: Duration,
    pub refresh_registry_post: Duration,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RefreshTiming {
    pub traversal: Duration,
    pub registry_post: Duration,
}

#[derive(Clone, Copy)]
enum RegistryRefreshPlan<'a> {
    Rebuild,
    ReuseClean(&'a RegistryRebuildPayload),
}

fn registry_refresh_plan<'a>(
    tree: &ElementTree,
    cached_rebuild: Option<&'a RegistryRebuildPayload>,
) -> RegistryRefreshPlan<'a> {
    if let Some(cached_rebuild) = cached_rebuild
        && !tree.has_registry_refresh_damage()
    {
        RegistryRefreshPlan::ReuseClean(cached_rebuild)
    } else {
        RegistryRefreshPlan::Rebuild
    }
}

/// After DOM/scroll changes, produce new outputs without re-running layout.
/// Use this when only scroll positions changed (not structure).
pub fn refresh(tree: &mut ElementTree) -> LayoutOutput {
    refresh_with_registry_plan(tree, RegistryRefreshPlan::Rebuild)
}

pub fn refresh_default_with_frame_attrs(
    tree: &mut ElementTree,
    scale: f32,
    runtime: Option<&AnimationRuntime>,
    sample_time: Option<Instant>,
) -> LayoutOutput {
    let preparation = prepare_frame_attrs_for_update(tree, scale, runtime, sample_time);
    refresh_prepared_default(tree, preparation).output
}

#[cfg(any(test, feature = "bench-diagnostics"))]
#[doc(hidden)]
pub fn refresh_render_scene_for_benchmark(tree: &mut ElementTree) -> RenderScene {
    let render_output =
        build_refresh_output_with_scroll_layers(tree, RefreshRegistryMode::ReuseClean);
    tree.clear_render_refresh_dirty();
    render_output.scene
}

#[cfg(any(test, feature = "bench-diagnostics"))]
#[doc(hidden)]
pub fn refresh_reusing_clean_registry_for_benchmark(
    tree: &mut ElementTree,
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> LayoutOutput {
    refresh_reusing_clean_registry(tree, cached_rebuild)
}

pub(crate) fn refresh_reusing_clean_registry(
    tree: &mut ElementTree,
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> LayoutOutput {
    refresh_with_registry_plan(tree, registry_refresh_plan(tree, cached_rebuild))
}

pub(crate) fn refresh_reusing_clean_registry_timed(
    tree: &mut ElementTree,
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> (LayoutOutput, RefreshTiming) {
    refresh_with_registry_plan_timed(tree, registry_refresh_plan(tree, cached_rebuild))
}

fn refresh_with_registry_plan(
    tree: &mut ElementTree,
    plan: RegistryRefreshPlan<'_>,
) -> LayoutOutput {
    let render_output = build_refresh_output_for_plan(tree, plan);
    finish_refresh_build_output(tree, render_output, plan)
}

fn refresh_with_registry_plan_timed(
    tree: &mut ElementTree,
    plan: RegistryRefreshPlan<'_>,
) -> (LayoutOutput, RefreshTiming) {
    let traversal_started_at = Instant::now();
    let render_output = build_refresh_output_for_plan(tree, plan);
    let traversal = traversal_started_at.elapsed();

    let registry_post_started_at = Instant::now();
    let output = finish_refresh_build_output(tree, render_output, plan);
    let registry_post = registry_post_started_at.elapsed();

    (
        output,
        RefreshTiming {
            traversal,
            registry_post,
        },
    )
}

fn build_refresh_output_for_plan(
    tree: &ElementTree,
    plan: RegistryRefreshPlan<'_>,
) -> RefreshBuildOutput {
    match plan {
        RegistryRefreshPlan::Rebuild => {
            build_refresh_output_with_scroll_layers(tree, RefreshRegistryMode::Rebuild)
        }
        RegistryRefreshPlan::ReuseClean(_) => {
            build_refresh_output_with_scroll_layers(tree, RefreshRegistryMode::ReuseClean)
        }
    }
}

fn finish_refresh_build_output(
    tree: &mut ElementTree,
    render_output: RefreshBuildOutput,
    plan: RegistryRefreshPlan<'_>,
) -> LayoutOutput {
    match (render_output.registry, plan) {
        (RefreshRegistryOutput::Rebuilt(event_rebuild), _) => {
            let ime_text_state = ime_text_state_from_rebuild(&event_rebuild);
            tree.clear_refresh_dirty();
            LayoutOutput {
                scene: render_output.scene,
                event_rebuild,
                event_rebuild_changed: true,
                ime_enabled: render_output.text_input_focused,
                ime_cursor_area: render_output.text_input_cursor_area,
                ime_text_state,
                animations_active: false,
            }
        }
        (RefreshRegistryOutput::ReusedClean, RegistryRefreshPlan::ReuseClean(cached_rebuild)) => {
            let refreshed_rebuild =
                crate::events::registry_builder::refresh_runtime_state_in_cached_rebuild(
                    tree,
                    cached_rebuild,
                );
            let rebuild_for_ime = refreshed_rebuild.as_ref().unwrap_or(cached_rebuild);
            let ime_text_state = ime_text_state_from_rebuild(rebuild_for_ime);
            let event_rebuild_changed = refreshed_rebuild.is_some();
            tree.clear_render_refresh_dirty();
            LayoutOutput {
                scene: render_output.scene,
                event_rebuild: refreshed_rebuild.unwrap_or_default(),
                event_rebuild_changed,
                ime_enabled: render_output.text_input_focused,
                ime_cursor_area: render_output.text_input_cursor_area,
                ime_text_state,
                animations_active: false,
            }
        }
        (RefreshRegistryOutput::ReusedClean, RegistryRefreshPlan::Rebuild) => {
            let event_rebuild = crate::events::registry_builder::build_registry_rebuild(tree);
            let ime_text_state = ime_text_state_from_rebuild(&event_rebuild);
            tree.clear_refresh_dirty();
            LayoutOutput {
                scene: render_output.scene,
                event_rebuild,
                event_rebuild_changed: true,
                ime_enabled: render_output.text_input_focused,
                ime_cursor_area: render_output.text_input_cursor_area,
                ime_text_state,
                animations_active: false,
            }
        }
    }
}

fn ime_text_state_from_rebuild(rebuild: &RegistryRebuildPayload) -> Option<TextInputState> {
    rebuild
        .focused_id
        .as_ref()
        .and_then(|focused_id| rebuild.text_inputs.get(focused_id).cloned())
}

/// Full layout with default Skia text measurer, followed by refresh.
pub fn layout_and_refresh_default(
    tree: &mut ElementTree,
    constraint: Constraint,
    scale: f32,
) -> LayoutOutput {
    let animations_active = layout_tree_default_with_animation(
        tree,
        constraint,
        scale,
        &AnimationRuntime::default(),
        Instant::now(),
    );
    let mut output = refresh(tree);
    output.animations_active = animations_active;
    output
}

pub fn layout_and_refresh_default_with_animation(
    tree: &mut ElementTree,
    constraint: Constraint,
    scale: f32,
    runtime: &AnimationRuntime,
    sample_time: Instant,
) -> LayoutOutput {
    let preparation = prepare_frame_attrs_for_update(tree, scale, Some(runtime), Some(sample_time));
    layout_and_refresh_prepared_default(tree, constraint, preparation).output
}

pub fn layout_or_refresh_default_with_animation(
    tree: &mut ElementTree,
    constraint: Constraint,
    scale: f32,
    runtime: &AnimationRuntime,
    sample_time: Instant,
) -> LayoutUpdateOutput {
    let mut invalidation = TreeInvalidation::None;
    let preparation = prepare_layout_or_refresh_default_frame_attrs(
        tree,
        scale,
        runtime,
        sample_time,
        &mut invalidation,
        None,
    );
    finish_layout_or_refresh_prepared_default(tree, constraint, preparation, invalidation, None)
}

fn prepare_layout_or_refresh_default_frame_attrs(
    tree: &mut ElementTree,
    scale: f32,
    runtime: &AnimationRuntime,
    sample_time: Instant,
    invalidation: &mut TreeInvalidation,
    dirty_ids: Option<&[NodeId]>,
) -> FrameAttrsPreparation {
    let preparation = if let Some(dirty_ids) = dirty_ids {
        if invalidation.can_refresh_only() && !runtime.has_transient_entries() {
            prepare_dirty_frame_attrs_for_update(
                tree,
                scale,
                (!runtime.is_empty()).then_some(runtime),
                Some(sample_time),
                dirty_ids,
            )
        } else {
            prepare_frame_attrs_for_update(tree, scale, Some(runtime), Some(sample_time))
        }
    } else if invalidation.is_none()
        && !runtime.is_empty()
        && tree
            .root_id()
            .and_then(|root_id| tree.get(&root_id).and_then(|element| element.layout.frame))
            .is_some()
    {
        prepare_animation_frame_attrs_for_update(tree, scale, runtime, Some(sample_time))
    } else {
        prepare_frame_attrs_for_update(tree, scale, Some(runtime), Some(sample_time))
    };
    invalidation.add(preparation.animation_result.invalidation);
    preparation
}

fn finish_layout_or_refresh_prepared_default(
    tree: &mut ElementTree,
    constraint: Constraint,
    preparation: FrameAttrsPreparation,
    invalidation: TreeInvalidation,
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> LayoutUpdateOutput {
    if invalidation.can_refresh_only() && prepared_root_has_frame(tree, &preparation) {
        refresh_prepared_default_reusing_clean_registry(tree, preparation, cached_rebuild)
    } else {
        layout_and_refresh_prepared_default_reusing_clean_registry(
            tree,
            constraint,
            preparation,
            cached_rebuild,
        )
    }
}

#[cfg(any(test, feature = "bench-diagnostics"))]
#[doc(hidden)]
pub fn layout_or_refresh_default_with_animation_reusing_clean_registry_for_benchmark(
    tree: &mut ElementTree,
    constraint: Constraint,
    scale: f32,
    runtime: &AnimationRuntime,
    sample_time: Instant,
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> LayoutUpdateOutput {
    let mut invalidation = TreeInvalidation::None;
    let preparation = prepare_layout_or_refresh_default_frame_attrs(
        tree,
        scale,
        runtime,
        sample_time,
        &mut invalidation,
        None,
    );
    finish_layout_or_refresh_prepared_default(
        tree,
        constraint,
        preparation,
        invalidation,
        cached_rebuild,
    )
}

#[cfg(any(test, feature = "bench-diagnostics"))]
#[doc(hidden)]
pub fn layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
    tree: &mut ElementTree,
    constraint: Constraint,
    scale: f32,
    runtime: &AnimationRuntime,
    sample_time: Instant,
    mut invalidation: TreeInvalidation,
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> LayoutUpdateOutput {
    let preparation = prepare_layout_or_refresh_default_frame_attrs(
        tree,
        scale,
        runtime,
        sample_time,
        &mut invalidation,
        None,
    );
    finish_layout_or_refresh_prepared_default(
        tree,
        constraint,
        preparation,
        invalidation,
        cached_rebuild,
    )
}

#[cfg(any(test, feature = "bench-diagnostics"))]
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn layout_or_refresh_default_with_animation_and_dirty_ids_reusing_clean_registry_for_benchmark(
    tree: &mut ElementTree,
    constraint: Constraint,
    scale: f32,
    runtime: &AnimationRuntime,
    sample_time: Instant,
    mut invalidation: TreeInvalidation,
    dirty_ids: &[NodeId],
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> LayoutUpdateOutput {
    let preparation = prepare_layout_or_refresh_default_frame_attrs(
        tree,
        scale,
        runtime,
        sample_time,
        &mut invalidation,
        Some(dirty_ids),
    );
    finish_layout_or_refresh_prepared_default(
        tree,
        constraint,
        preparation,
        invalidation,
        cached_rebuild,
    )
}

#[cfg(any(test, feature = "bench-diagnostics"))]
#[derive(Clone, Copy, Debug, Default)]
#[doc(hidden)]
pub struct LayoutBenchmarkProfile {
    pub prepare: Duration,
    pub layout: Duration,
    pub refresh: Duration,
    pub refresh_traversal: Duration,
    pub refresh_registry_post: Duration,
    pub pre_layout_registry_damage: bool,
    pub registry_damage_nodes: usize,
    pub layout_performed: bool,
    pub scene_nodes: usize,
    pub render_visits: u64,
    pub culled_subtrees: u64,
    pub registry_visits: u64,
    pub registry_cache_hits: u64,
    pub registry_cache_stores: u64,
    pub registry_cache_damaged: u64,
    pub registry_cache_ineligible: u64,
    pub registry_cache_misses: u64,
    pub registry_damage: bool,
}

#[cfg(any(test, feature = "bench-diagnostics"))]
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn layout_or_refresh_default_with_animation_and_invalidation_profile_for_benchmark(
    tree: &mut ElementTree,
    constraint: Constraint,
    scale: f32,
    runtime: &AnimationRuntime,
    sample_time: Instant,
    mut invalidation: TreeInvalidation,
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> (LayoutUpdateOutput, LayoutBenchmarkProfile) {
    let prepare_started_at = Instant::now();
    let preparation = prepare_layout_or_refresh_default_frame_attrs(
        tree,
        scale,
        runtime,
        sample_time,
        &mut invalidation,
        None,
    );
    let prepare = prepare_started_at.elapsed();
    let pre_layout_registry_damage = tree.has_registry_refresh_damage();

    let can_refresh_without_layout =
        invalidation.can_refresh_only() && prepared_root_has_frame(tree, &preparation);

    if can_refresh_without_layout {
        let refresh_started_at = Instant::now();
        let registry_damage = tree.has_registry_refresh_damage();
        let registry_damage_nodes = tree.registry_refresh_damage_count();
        let plan = registry_refresh_plan(tree, cached_rebuild);
        reset_render_traversal_diagnostics_for_benchmark();
        reset_registry_build_diagnostics_for_benchmark();
        let traversal_started_at = Instant::now();
        let render_output = build_refresh_output_for_plan(tree, plan);
        let render_diagnostics = take_render_traversal_diagnostics_for_benchmark();
        let traversal = traversal_started_at.elapsed();
        let registry_post_started_at = Instant::now();
        let mut output = finish_refresh_build_output(tree, render_output, plan);
        output.animations_active = preparation.animation_result.active;
        let registry_diagnostics = take_registry_build_diagnostics_for_benchmark();
        let registry_post = registry_post_started_at.elapsed();
        let update = LayoutUpdateOutput {
            output,
            layout_performed: false,
        };
        let profile = LayoutBenchmarkProfile {
            prepare,
            refresh: refresh_started_at.elapsed(),
            refresh_traversal: traversal,
            refresh_registry_post: registry_post,
            pre_layout_registry_damage,
            registry_damage_nodes,
            layout_performed: false,
            scene_nodes: update.output.scene.nodes.len(),
            render_visits: render_diagnostics.element_visits,
            culled_subtrees: render_diagnostics.culled_subtrees,
            registry_visits: registry_diagnostics.visits,
            registry_cache_hits: registry_diagnostics.cache_hits,
            registry_cache_stores: registry_diagnostics.cache_stores,
            registry_cache_damaged: registry_diagnostics.cache_damaged,
            registry_cache_ineligible: registry_diagnostics.cache_ineligible,
            registry_cache_misses: registry_diagnostics.cache_misses,
            registry_damage,
            ..LayoutBenchmarkProfile::default()
        };
        return (update, profile);
    }

    let layout_started_at = Instant::now();
    let layout_performed = run_prepared_default_layout(tree, constraint, &preparation);
    let layout = layout_started_at.elapsed();

    let refresh_started_at = Instant::now();
    let registry_damage = tree.has_registry_refresh_damage();
    let registry_damage_nodes = tree.registry_refresh_damage_count();
    let plan = registry_refresh_plan(tree, cached_rebuild);
    reset_render_traversal_diagnostics_for_benchmark();
    reset_registry_build_diagnostics_for_benchmark();
    let traversal_started_at = Instant::now();
    let render_output = build_refresh_output_for_plan(tree, plan);
    let render_diagnostics = take_render_traversal_diagnostics_for_benchmark();
    let traversal = traversal_started_at.elapsed();
    let registry_post_started_at = Instant::now();
    let mut output = finish_refresh_build_output(tree, render_output, plan);
    let registry_diagnostics = take_registry_build_diagnostics_for_benchmark();
    let registry_post = registry_post_started_at.elapsed();
    output.animations_active = preparation.animation_result.active;
    let refresh = refresh_started_at.elapsed();

    let update = LayoutUpdateOutput {
        output,
        layout_performed,
    };
    let profile = LayoutBenchmarkProfile {
        prepare,
        layout,
        refresh,
        refresh_traversal: traversal,
        refresh_registry_post: registry_post,
        pre_layout_registry_damage,
        registry_damage_nodes,
        layout_performed,
        scene_nodes: update.output.scene.nodes.len(),
        render_visits: render_diagnostics.element_visits,
        culled_subtrees: render_diagnostics.culled_subtrees,
        registry_visits: registry_diagnostics.visits,
        registry_cache_hits: registry_diagnostics.cache_hits,
        registry_cache_stores: registry_diagnostics.cache_stores,
        registry_cache_damaged: registry_diagnostics.cache_damaged,
        registry_cache_ineligible: registry_diagnostics.cache_ineligible,
        registry_cache_misses: registry_diagnostics.cache_misses,
        registry_damage,
    };

    (update, profile)
}

fn run_prepared_default_layout(
    tree: &mut ElementTree,
    constraint: Constraint,
    preparation: &FrameAttrsPreparation,
) -> bool {
    let Some(root_id) = preparation.root_id else {
        return false;
    };

    run_layout_passes(
        tree,
        &root_id,
        constraint,
        &SkiaTextMeasurer,
        &FontContext::default(),
        &preparation.animation_result,
    );
    true
}

pub(crate) fn layout_and_refresh_prepared_default(
    tree: &mut ElementTree,
    constraint: Constraint,
    preparation: FrameAttrsPreparation,
) -> LayoutUpdateOutput {
    let layout_performed = run_prepared_default_layout(tree, constraint, &preparation);

    let mut output = refresh(tree);
    output.animations_active = preparation.animation_result.active;

    LayoutUpdateOutput {
        output,
        layout_performed,
    }
}

pub(crate) fn layout_and_refresh_prepared_default_reusing_clean_registry(
    tree: &mut ElementTree,
    constraint: Constraint,
    preparation: FrameAttrsPreparation,
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> LayoutUpdateOutput {
    let layout_performed = run_prepared_default_layout(tree, constraint, &preparation);

    let mut output = refresh_reusing_clean_registry(tree, cached_rebuild);
    output.animations_active = preparation.animation_result.active;

    LayoutUpdateOutput {
        output,
        layout_performed,
    }
}

pub(crate) fn layout_and_refresh_prepared_default_reusing_clean_registry_timed(
    tree: &mut ElementTree,
    constraint: Constraint,
    preparation: FrameAttrsPreparation,
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> (LayoutUpdateOutput, LayoutUpdateTiming) {
    let layout_started_at = Instant::now();
    let layout_performed = run_prepared_default_layout(tree, constraint, &preparation);
    let layout = layout_started_at.elapsed();

    let refresh_started_at = Instant::now();
    let (mut output, refresh_timing) = refresh_reusing_clean_registry_timed(tree, cached_rebuild);
    output.animations_active = preparation.animation_result.active;
    let refresh = refresh_started_at.elapsed();

    (
        LayoutUpdateOutput {
            output,
            layout_performed,
        },
        LayoutUpdateTiming {
            layout,
            refresh,
            refresh_traversal: refresh_timing.traversal,
            refresh_registry_post: refresh_timing.registry_post,
        },
    )
}

pub(crate) fn refresh_prepared_default(
    tree: &mut ElementTree,
    preparation: FrameAttrsPreparation,
) -> LayoutUpdateOutput {
    let mut output = refresh(tree);
    output.animations_active = preparation.animation_result.active;

    LayoutUpdateOutput {
        output,
        layout_performed: false,
    }
}

pub(crate) fn refresh_prepared_default_reusing_clean_registry(
    tree: &mut ElementTree,
    preparation: FrameAttrsPreparation,
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> LayoutUpdateOutput {
    let mut output = refresh_reusing_clean_registry(tree, cached_rebuild);
    output.animations_active = preparation.animation_result.active;

    LayoutUpdateOutput {
        output,
        layout_performed: false,
    }
}

#[cfg(test)]
mod tests;
