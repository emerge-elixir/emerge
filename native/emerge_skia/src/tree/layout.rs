//! Layout engine for Emerge element trees.
//!
//! Three-pass algorithm:
//! 0. Scale: Apply scale factor to all attributes
//! 1. Measurement (bottom-up): Compute intrinsic sizes
//! 2. Resolution (top-down): Assign frames with constraints

use super::animation::{
    AnimationOverlayResult, AnimationRuntime, apply_animation_overlays, scale_animation_spec,
};
use super::attrs::{
    AlignX, AlignY, Attrs, BorderWidth, Color, Font, Length, MouseOverAttrs, Padding, TextAlign,
    TextFragment, effective_scrollbar_x, effective_scrollbar_y,
};
use super::element::{
    Element, ElementKind, ElementTree, Frame, InheritedMeasureFontKey, IntrinsicMeasureCache,
    IntrinsicMeasureCacheKey, NearbyConstraintKind, NearbyMount, NearbySlot, NodeId, ResolveAttrs,
    ResolveAvailableSpaceKey, ResolveCache, ResolveCacheKey, ResolveConstraintKey, ResolveExtent,
    SubtreeMeasureAttrs, SubtreeMeasureCache, SubtreeMeasureCacheKey, TopologyDependencyKey,
};
use super::render::DEFAULT_TEXT_COLOR;
use super::text_layout::{TextLayoutStyle, layout_text_lines};
use crate::assets;
use std::collections::HashMap;
use std::time::Instant;

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

    // Pass 1: Measure (bottom-up) - uses pre-scaled attrs
    measure_element(tree, root_id, measurer, inherited, true);

    // Pass 2: Resolve (top-down) - uses pre-scaled attrs
    resolve_element(
        tree, root_id, constraint, 0.0, 0.0, inherited, measurer, true,
    );
}

fn mark_animation_refresh_effects_dirty(
    tree: &mut ElementTree,
    animation_result: &AnimationOverlayResult,
) {
    for effect in &animation_result.effects {
        tree.mark_refresh_dirty_for_invalidation(&effect.id, effect.invalidation);

        if effect.registry_refresh {
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
            tree.mark_measure_dirty_for_invalidation(&effect.id, effect.invalidation)
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
    for element in tree.iter_nodes_mut() {
        let scale_factor = match element.lifecycle.ghost_capture_scale {
            Some(capture_scale) => scale / capture_scale.max(f32::EPSILON),
            None => scale,
        };
        element.layout.effective = scale_attrs(&element.spec.declared, scale_factor);
        element.normalize_extracted_state();
    }

    apply_animation_overlays(tree, animation_runtime, sample_time, scale)
}

/// Scale all pixel-based attributes in an Attrs struct.
fn scale_attrs(attrs: &Attrs, scale: f32) -> Attrs {
    let scale_f64 = scale as f64;
    Attrs {
        width: attrs.width.as_ref().map(|l| scale_length(l, scale)),
        height: attrs.height.as_ref().map(|l| scale_length(l, scale)),
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

/// Scale pixel values within a Length, recursively handling Minimum/Maximum.
fn scale_length(length: &Length, scale: f32) -> Length {
    let scale_f64 = scale as f64;
    match length {
        Length::Px(val) => Length::Px(*val * scale_f64),
        Length::Minimum(min_px, inner) => {
            Length::Minimum(*min_px * scale_f64, Box::new(scale_length(inner, scale)))
        }
        Length::Maximum(max_px, inner) => {
            Length::Maximum(*max_px * scale_f64, Box::new(scale_length(inner, scale)))
        }
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
    let topology_key = tree.topology_dependency_key_for(id);
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
            measure_element(
                tree,
                child_id,
                measurer,
                &element_context,
                use_subtree_cache,
            )
        })
        .collect();

    for nearby_id in nearby_mounts.iter().map(|mount| mount.id) {
        let _ = measure_element(
            tree,
            &nearby_id,
            measurer,
            &element_context,
            use_subtree_cache,
        );
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

    if let Some(key) = cache_key.as_ref() {
        if let Some(intrinsic) = try_reuse_intrinsic_measure_cache(tree, id, key) {
            return intrinsic;
        }
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
            let text_width = measure_text_width_with_spacing(
                measurer,
                content,
                font_size,
                &family,
                weight,
                italic,
                (letter_spacing, word_spacing),
            );
            let (_width, text_height) =
                measurer.measure_with_font(content, font_size, &family, weight, italic);
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

        ElementKind::El | ElementKind::None => {
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
    let measured_frame = Frame {
        x: 0.0,
        y: 0.0,
        width: intrinsic.width,
        height: intrinsic.height,
        content_width: intrinsic.width,
        content_height: intrinsic.height,
    };
    let intrinsic_measure_cache = cache_key.map(|key| {
        tree.record_layout_cache_stats(|stats| stats.record_intrinsic_measure_store());
        IntrinsicMeasureCache {
            key,
            frame: measured_frame,
        }
    });
    let subtree_measure_cache = if use_subtree_cache {
        subtree_cache_key.map(|key| {
            tree.record_layout_cache_stats(|stats| stats.record_subtree_measure_store());
            SubtreeMeasureCache {
                key,
                frame: measured_frame,
            }
        })
    } else {
        None
    };

    if let Some(element) = tree.get_mut(id) {
        element.layout.measured_frame = Some(measured_frame);
        element.layout.intrinsic_measure_cache = intrinsic_measure_cache;
        if use_subtree_cache {
            element.layout.subtree_measure_cache = subtree_measure_cache;
            element.layout.measure_dirty = false;
            element.layout.measure_descendant_dirty = false;
        }
    }

    intrinsic
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
        .map(|cache| cache.frame);

    let Some(frame) = frame else {
        tree.record_layout_cache_stats(|stats| stats.record_subtree_measure_miss());
        return None;
    };

    tree.record_layout_cache_stats(|stats| stats.record_subtree_measure_hit());

    if let Some(element) = tree.get_mut(id) {
        element.layout.measured_frame = Some(frame);
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
        .map(|cache| cache.frame);

    let Some(frame) = frame else {
        tree.record_layout_cache_stats(|stats| stats.record_intrinsic_measure_miss());
        return None;
    };

    tree.record_layout_cache_stats(|stats| stats.record_intrinsic_measure_hit());

    if let Some(element) = tree.get_mut(id) {
        element.layout.measured_frame = Some(frame);
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
        Some(Length::Minimum(min_px, inner)) => {
            let inner_size = resolve_intrinsic_length(Some(inner), intrinsic);
            inner_size.max(*min_px as f32)
        }
        Some(Length::Maximum(max_px, inner)) => {
            let inner_size = resolve_intrinsic_length(Some(inner), intrinsic);
            inner_size.min(*max_px as f32)
        }
    }
}

fn resolve_outer_intrinsic_length(length: Option<&Length>, content_size: f32, insets: f32) -> f32 {
    resolve_intrinsic_length(length, content_size + insets)
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
    let available_width = if text_should_fill_width || prefer_fill_width {
        // Text with alignment should fill available width.
        constraint.width
    } else if kind == ElementKind::Paragraph && is_content_length(attrs.width.as_ref()) {
        // Paragraphs wrap text within parent's available width (like <p> in HTML).
        constraint.width
    } else if is_content_length(attrs.width.as_ref()) {
        match attrs.width.as_ref() {
            Some(Length::Minimum(_, inner)) if is_content_length(Some(inner)) => {
                AvailableSpace::MinContent
            }
            _ => AvailableSpace::MaxContent,
        }
    } else {
        constraint.width
    };

    let available_height = if prefer_fill_height {
        constraint.height
    } else if is_content_length(attrs.height.as_ref()) {
        match attrs.height.as_ref() {
            Some(Length::Minimum(_, inner)) if is_content_length(Some(inner)) => {
                AvailableSpace::MinContent
            }
            _ => AvailableSpace::MaxContent,
        }
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
        Some(Length::Minimum(_, inner)) | Some(Length::Maximum(_, inner)) => {
            length_requests_fill(Some(inner))
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

    if actual_ch > params.content.height && !params.is_scrollable {
        expand_frame_height_to_content(tree, params.id, actual_ch, params.insets);
        set_frame_content_width(tree, params.id, actual_cw, params.insets);
    } else {
        set_frame_content_size(tree, params.id, actual_cw, actual_ch, params.insets);
    }
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
        && is_content_length(params.attrs.height.as_ref())
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

    if actual_content_height > params.content.height && !params.is_scrollable {
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

    if actual_content_height > params.content.height && !params.is_scrollable {
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

    if actual_content_height > params.content.height && !params.is_scrollable {
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

    if actual_content_height > params.content.height && !params.is_scrollable {
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
        && is_content_length(params.attrs.height.as_ref())
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
    constraint: Constraint,
    x: f32,
    y: f32,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) {
    let Some(element) = tree.get(id) else {
        return;
    };

    // Read from pre-scaled attrs
    let attrs = element.layout.effective.clone();
    let kind = element.spec.kind;
    let measured_frame = element.layout.measured_frame;
    let resolve_dirty = element.layout.resolve_dirty;
    let intrinsic = element
        .layout
        .measured_frame
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

    if !use_resolve_cache || !resolve_kind_eligible || resolve_dirty {
        tree.record_layout_cache_stats(|stats| stats.record_resolve_miss());
    } else if cache_eligible {
        let key = resolve_cache_key(
            kind,
            &attrs,
            inherited,
            measured_frame,
            constraint,
            topology_key,
        );

        if try_reuse_resolve_cache(tree, id, &key, x, y) {
            return;
        }
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

    update_paint_children(tree, id, kind);
    update_scroll_state(tree, id);
    resolve_nearby_mounts(tree, id, &element_context, measurer, use_resolve_cache);

    if use_resolve_cache {
        if can_store_resolve_cache(tree, kind, &child_ids, &nearby_mounts)
            && let Some(frame) = tree.get(id).and_then(|element| element.layout.frame)
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
                    extent: resolve_extent(frame),
                });
                element.layout.resolve_dirty = false;
            }
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

fn resolve_extent(frame: Frame) -> ResolveExtent {
    ResolveExtent {
        width: frame.width,
        height: frame.height,
        content_width: frame.content_width,
        content_height: frame.content_height,
    }
}

fn frame_from_resolve_extent(extent: ResolveExtent, x: f32, y: f32) -> Frame {
    Frame {
        x,
        y,
        width: extent.width,
        height: extent.height,
        content_width: extent.content_width,
        content_height: extent.content_height,
    }
}

fn resolve_attrs(attrs: &Attrs) -> ResolveAttrs {
    ResolveAttrs {
        width: attrs.width.clone(),
        height: attrs.height.clone(),
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
            frame_from_resolve_extent(cache.extent, x, y),
            element.layout.frame?,
        ))
    });

    let Some((target_frame, current_frame)) = cached_frames else {
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

    if let Some(element) = tree.get_mut(id) {
        element.layout.frame = Some(target_frame);
        element.layout.resolve_dirty = false;
    }

    true
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
                .map(|frame| (index, child_id.clone(), frame.x, frame.y))
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
        Some(Length::Minimum(min_px, inner)) => {
            let inner_size = resolve_length(Some(inner), intrinsic, constraint);
            inner_size.max(*min_px as f32)
        }
        Some(Length::Maximum(max_px, inner)) => {
            let inner_size = resolve_length(Some(inner), intrinsic, constraint);
            inner_size.min(*max_px as f32)
        }
    }
}

fn resolve_nearby_mounts<M: TextMeasurer>(
    tree: &mut ElementTree,
    host_id: &NodeId,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) {
    let Some(host_frame) = tree.get(host_id).and_then(|element| element.layout.frame) else {
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
            constraint,
            host_frame.x,
            host_frame.y,
            inherited,
            measurer,
            use_resolve_cache,
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
        Some(Length::Minimum(_, inner)) | Some(Length::Maximum(_, inner)) => {
            is_content_length(Some(inner))
        }
        _ => false,
    }
}

/// Get the weight value for a fill-based length.
/// Returns 1.0 for Fill, the configured weight for FillWeighted, or 0.0 for non-fill.
fn get_fill_weight(length: Option<&Length>) -> f32 {
    match length {
        Some(Length::Fill) => 1.0,
        Some(Length::FillWeighted(weight)) => *weight as f32,
        Some(Length::Minimum(_, inner)) | Some(Length::Maximum(_, inner)) => {
            get_fill_weight(Some(inner))
        }
        _ => 0.0,
    }
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
struct ChildPlacement<'a> {
    constraint: Constraint,
    x: f32,
    y: f32,
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
    placement: ChildPlacement<'_>,
    measurer: &M,
) -> Option<ChildFrameSnapshot> {
    resolve_element(
        tree,
        child_id,
        placement.constraint,
        placement.x,
        placement.y,
        placement.inherited,
        measurer,
        placement.use_resolve_cache,
    );
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

fn planned_row_child_width(
    child: &Element,
    measured_width: f32,
    allow_fill_width: bool,
    width_per_portion: f32,
) -> f32 {
    let portion = if allow_fill_width {
        get_fill_weight(child.layout.effective.width.as_ref())
    } else {
        0.0
    };

    if portion > 0.0 {
        resolve_length(
            child.layout.effective.width.as_ref(),
            measured_width,
            width_per_portion * portion,
        )
    } else {
        resolve_length(
            child.layout.effective.width.as_ref(),
            measured_width,
            measured_width,
        )
    }
}

fn planned_column_child_height(
    child: &Element,
    measured_height: f32,
    allow_fill_height: bool,
    height_per_portion: f32,
) -> f32 {
    let portion = if allow_fill_height {
        get_fill_weight(child.layout.effective.height.as_ref())
    } else {
        0.0
    };

    if portion > 0.0 {
        resolve_length(
            child.layout.effective.height.as_ref(),
            measured_height,
            height_per_portion * portion,
        )
    } else {
        resolve_length(
            child.layout.effective.height.as_ref(),
            measured_height,
            measured_height,
        )
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
            ChildPlacement {
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
    child_widths: HashMap<NodeId, f32>,
    left_children: Vec<NodeId>,
    center_children: Vec<NodeId>,
    right_children: Vec<NodeId>,
    total_left_width: f32,
    total_center_width: f32,
    total_right_width: f32,
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
    // First pass: calculate weighted fill distribution.
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
        } else {
            fixed_width += planned_row_child_width(child, measured_width, false, 0.0);
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
    let mut left_children: Vec<NodeId> = Vec::new();
    let mut center_children: Vec<NodeId> = Vec::new();
    let mut right_children: Vec<NodeId> = Vec::new();

    let mut child_widths: HashMap<NodeId, f32> = HashMap::new();
    let mut total_left_width = 0.0_f32;
    let mut total_center_width = 0.0_f32;
    let mut total_right_width = 0.0_f32;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else {
            continue;
        };
        let measured_width = child_measured_width(tree, child_id);
        let width = planned_row_child_width(
            child,
            measured_width,
            options.allow_fill_width,
            width_per_portion,
        );
        child_widths.insert(child_id.clone(), width);

        match child.layout.effective.align_x.unwrap_or_default() {
            AlignX::Left => {
                left_children.push(child_id.clone());
                total_left_width += width;
            }
            AlignX::Center => {
                center_children.push(child_id.clone());
                total_center_width += width;
            }
            AlignX::Right => {
                right_children.push(child_id.clone());
                total_right_width += width;
            }
        }
    }

    RowLayoutPlan {
        child_widths,
        left_children,
        center_children,
        right_children,
        total_left_width,
        total_center_width,
        total_right_width,
    }
}

fn build_row_layout_plan_from_widths(tree: &ElementTree, line: &[(NodeId, f32)]) -> RowLayoutPlan {
    let mut left_children: Vec<NodeId> = Vec::new();
    let mut center_children: Vec<NodeId> = Vec::new();
    let mut right_children: Vec<NodeId> = Vec::new();

    let mut child_widths: HashMap<NodeId, f32> = HashMap::new();
    let mut total_left_width = 0.0_f32;
    let mut total_center_width = 0.0_f32;
    let mut total_right_width = 0.0_f32;

    for (child_id, width) in line {
        let Some(child) = tree.get(child_id) else {
            continue;
        };

        child_widths.insert(child_id.clone(), *width);

        match child.layout.effective.align_x.unwrap_or_default() {
            AlignX::Left => {
                left_children.push(child_id.clone());
                total_left_width += *width;
            }
            AlignX::Center => {
                center_children.push(child_id.clone());
                total_center_width += *width;
            }
            AlignX::Right => {
                right_children.push(child_id.clone());
                total_right_width += *width;
            }
        }
    }

    RowLayoutPlan {
        child_widths,
        left_children,
        center_children,
        right_children,
        total_left_width,
        total_center_width,
        total_right_width,
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

    for child_id in &plan.left_children {
        let child_width = *plan.child_widths.get(child_id).unwrap_or(&0.0);

        if let Some(frame) = resolve_child_with_placement(
            tree,
            child_id,
            ChildPlacement {
                constraint: Constraint::new(child_width, content.height),
                x: current_x,
                y: content.y,
                inherited,
                use_resolve_cache,
            },
            measurer,
        ) {
            max_child_height = max_child_height.max(frame.content_height);
        }

        current_x += child_width + spacing;
    }

    let mut right_x = content.x + content.width;
    for child_id in plan.right_children.iter().rev() {
        let child_width = *plan.child_widths.get(child_id).unwrap_or(&0.0);

        right_x -= child_width;
        if let Some(frame) = resolve_child_with_placement(
            tree,
            child_id,
            ChildPlacement {
                constraint: Constraint::new(child_width, content.height),
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
        for child_id in &plan.center_children {
            let child_width = *plan.child_widths.get(child_id).unwrap_or(&0.0);

            if let Some(frame) = resolve_child_with_placement(
                tree,
                child_id,
                ChildPlacement {
                    constraint: Constraint::new(child_width, content.height),
                    x: center_x,
                    y: content.y,
                    inherited,
                    use_resolve_cache,
                },
                measurer,
            ) {
                max_child_height = max_child_height.max(frame.content_height);
            }

            center_x += child_width + spacing;
        }
    }

    max_child_height
}

fn resolve_row_space_evenly<M: TextMeasurer>(
    tree: &mut ElementTree,
    child_ids: &[NodeId],
    content: ContentRect,
    child_widths: &HashMap<NodeId, f32>,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) -> (f32, f32) {
    let mut max_child_height = 0.0_f32;
    let mut current_x = content.x;
    let gap_count = child_ids.len().saturating_sub(1) as f32;
    let total_child_width: f32 = child_widths.values().sum();
    let gap = if gap_count > 0.0 {
        (content.width - total_child_width).max(0.0) / gap_count
    } else {
        0.0
    };

    for child_id in child_ids {
        let child_width = *child_widths.get(child_id).unwrap_or(&0.0);
        let align_y = child_align_y(tree, child_id);

        if let Some(frame) = resolve_child_with_placement(
            tree,
            child_id,
            ChildPlacement {
                constraint: Constraint::new(child_width, content.height),
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

        current_x += child_width + gap;
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
    content: ContentRect,
    options: RowChildrenOptions,
    plan: &RowLayoutPlan,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) -> (f32, f32) {
    let left_spacing = spacing_for_count(plan.left_children.len(), options.spacing);
    let center_spacing = spacing_for_count(plan.center_children.len(), options.spacing);
    let right_spacing = spacing_for_count(plan.right_children.len(), options.spacing);

    let total_left_width = plan.total_left_width + left_spacing;
    let total_center_width = plan.total_center_width + center_spacing;
    let total_right_width = plan.total_right_width + right_spacing;

    // Position left-aligned children from left edge.
    let mut current_x = content.x;
    let mut max_child_height = 0.0_f32;

    for child_id in &plan.left_children {
        let child_width = *plan.child_widths.get(child_id).unwrap_or(&0.0);
        let align_y = child_align_y(tree, child_id);

        if let Some(frame) = resolve_child_with_placement(
            tree,
            child_id,
            ChildPlacement {
                constraint: Constraint::new(child_width, content.height),
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

        current_x += child_width + options.spacing;
    }

    // Position right-aligned children from right edge.
    let mut right_x = content.x + content.width;
    for child_id in plan.right_children.iter().rev() {
        let child_width = *plan.child_widths.get(child_id).unwrap_or(&0.0);
        let align_y = child_align_y(tree, child_id);

        right_x -= child_width;
        if let Some(frame) = resolve_child_with_placement(
            tree,
            child_id,
            ChildPlacement {
                constraint: Constraint::new(child_width, content.height),
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
        for child_id in &plan.center_children {
            let child_width = *plan.child_widths.get(child_id).unwrap_or(&0.0);
            let align_y = child_align_y(tree, child_id);

            if let Some(frame) = resolve_child_with_placement(
                tree,
                child_id,
                ChildPlacement {
                    constraint: Constraint::new(child_width, content.height),
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

            center_x += child_width + options.spacing;
        }
    }

    let total_child_width: f32 = plan.child_widths.values().sum();
    let total_spacing_used = spacing_for_count(child_ids.len(), options.spacing);
    let actual_content_width = total_child_width + total_spacing_used;

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
            child_ids,
            content,
            &plan.child_widths,
            inherited,
            measurer,
            use_resolve_cache,
        )
    } else {
        resolve_row_grouped(
            tree,
            child_ids,
            content,
            options,
            &plan,
            inherited,
            measurer,
            use_resolve_cache,
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
    child_heights: HashMap<NodeId, f32>,
    top_children: Vec<NodeId>,
    center_children: Vec<NodeId>,
    bottom_children: Vec<NodeId>,
    total_center_height: f32,
}

fn build_column_layout_plan(
    tree: &ElementTree,
    child_ids: &[NodeId],
    options: ColumnChildrenOptions,
    content_height: f32,
) -> ColumnLayoutPlan {
    // First pass: calculate weighted fill distribution.
    let mut total_portions = 0.0_f32;
    let mut fixed_height = 0.0_f32;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else {
            continue;
        };
        let measured_height = child_measured_height(tree, child_id);
        let portion = if options.allow_fill_height {
            get_fill_weight(child.layout.effective.height.as_ref())
        } else {
            0.0
        };
        if portion > 0.0 {
            total_portions += portion;
        } else {
            fixed_height += planned_column_child_height(child, measured_height, false, 0.0);
        }
    }

    // Calculate height per portion.
    let effective_spacing = if options.space_evenly {
        0.0
    } else {
        options.spacing
    };
    let total_spacing = effective_spacing * (child_ids.len().saturating_sub(1)) as f32;
    let remaining = (content_height - fixed_height - total_spacing).max(0.0);
    let height_per_portion = if total_portions > 0.0 {
        remaining / total_portions
    } else {
        0.0
    };

    // Partition children by vertical alignment and calculate heights.
    let mut top_children: Vec<NodeId> = Vec::new();
    let mut center_children: Vec<NodeId> = Vec::new();
    let mut bottom_children: Vec<NodeId> = Vec::new();
    let mut child_heights: HashMap<NodeId, f32> = HashMap::new();
    let mut total_center_height = 0.0_f32;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else {
            continue;
        };
        let measured_height = child_measured_height(tree, child_id);
        let height = planned_column_child_height(
            child,
            measured_height,
            options.allow_fill_height,
            height_per_portion,
        );
        child_heights.insert(child_id.clone(), height);

        match child.layout.effective.align_y.unwrap_or_default() {
            AlignY::Top => top_children.push(child_id.clone()),
            AlignY::Center => {
                center_children.push(child_id.clone());
                total_center_height += height;
            }
            AlignY::Bottom => bottom_children.push(child_id.clone()),
        }
    }

    ColumnLayoutPlan {
        child_heights,
        top_children,
        center_children,
        bottom_children,
        total_center_height,
    }
}

fn resolve_column_space_evenly<M: TextMeasurer>(
    tree: &mut ElementTree,
    child_ids: &[NodeId],
    content: ContentRect,
    child_heights: &HashMap<NodeId, f32>,
    inherited: &FontContext,
    measurer: &M,
    use_resolve_cache: bool,
) -> f32 {
    let mut current_y = content.y;
    let gap_count = child_ids.len().saturating_sub(1) as f32;
    let total_child_height: f32 = child_heights.values().sum();
    let gap = if gap_count > 0.0 {
        (content.height - total_child_height).max(0.0) / gap_count
    } else {
        0.0
    };
    let mut total_height = 0.0_f32;

    for child_id in child_ids {
        let child_height = *child_heights.get(child_id).unwrap_or(&0.0);
        let align_x = child_align_x(tree, child_id);

        let frame = resolve_child_with_placement(
            tree,
            child_id,
            ChildPlacement {
                constraint: Constraint::new(content.width, child_height),
                x: content.x,
                y: current_y,
                inherited,
                use_resolve_cache,
            },
            measurer,
        );
        let actual_height = frame
            .map(|snapshot| snapshot.height)
            .unwrap_or(child_height);

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

    for child_id in &plan.top_children {
        let child_height = *plan.child_heights.get(child_id).unwrap_or(&0.0);
        let align_x = child_align_x(tree, child_id);

        let frame = resolve_child_with_placement(
            tree,
            child_id,
            ChildPlacement {
                constraint: Constraint::new(content.width, child_height),
                x: content.x,
                y: current_y,
                inherited,
                use_resolve_cache,
            },
            measurer,
        );
        let actual_height = frame
            .map(|snapshot| snapshot.height)
            .unwrap_or(child_height);

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
        for child_id in &plan.bottom_children {
            let child_height = *plan.child_heights.get(child_id).unwrap_or(&0.0);
            let align_x = child_align_x(tree, child_id);

            let frame = resolve_child_with_placement(
                tree,
                child_id,
                ChildPlacement {
                    constraint: Constraint::new(content.width, child_height),
                    x: content.x,
                    y: current_bottom_y,
                    inherited,
                    use_resolve_cache,
                },
                measurer,
            );
            let actual_height = frame
                .map(|snapshot| snapshot.height)
                .unwrap_or(child_height);

            apply_horizontal_alignment(tree, child_id, content.x, content.width, align_x);

            actual_bottom_height += actual_height;
            current_bottom_y += actual_height + options.spacing;
        }
        if !plan.bottom_children.is_empty() {
            actual_bottom_height += bottom_spacing;
        }
    } else {
        let mut bottom_y = content.y + content.height;
        for child_id in plan.bottom_children.iter().rev() {
            let child_height = *plan.child_heights.get(child_id).unwrap_or(&0.0);
            let align_x = child_align_x(tree, child_id);

            bottom_y -= child_height;
            let frame = resolve_child_with_placement(
                tree,
                child_id,
                ChildPlacement {
                    constraint: Constraint::new(content.width, child_height),
                    x: content.x,
                    y: bottom_y,
                    inherited,
                    use_resolve_cache,
                },
                measurer,
            );
            let actual_height = frame
                .map(|snapshot| snapshot.height)
                .unwrap_or(child_height);

            let height_diff = actual_height - child_height;
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
        for child_id in &plan.center_children {
            let child_height = *plan.child_heights.get(child_id).unwrap_or(&0.0);
            let align_x = child_align_x(tree, child_id);

            let frame = resolve_child_with_placement(
                tree,
                child_id,
                ChildPlacement {
                    constraint: Constraint::new(content.width, child_height),
                    x: content.x,
                    y: center_y,
                    inherited,
                    use_resolve_cache,
                },
                measurer,
            );
            let actual_height = frame
                .map(|snapshot| snapshot.height)
                .unwrap_or(child_height);

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

    let plan = build_column_layout_plan(tree, child_ids, options, content.height);

    if options.space_evenly {
        resolve_column_space_evenly(
            tree,
            child_ids,
            content,
            &plan.child_heights,
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
        child_constraint,
        float_x,
        float_y,
        context.inherited,
        measurer,
        use_resolve_cache,
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
        child_constraint,
        layout.content.x,
        y,
        layout.inherited,
        measurer,
        false,
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
                child_constraint,
                content_x,
                child_y,
                inherited,
                measurer,
                use_resolve_cache,
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
        current_line.push((child_id.clone(), child_width));
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
            .map(|(child_id, _)| (child_id.clone(), child_align_y(tree, child_id)))
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
    if let Some(element) = tree.get_mut(id)
        && let Some(ref mut frame) = element.layout.frame
    {
        frame.content_width = insets.outer_width(actual_content_width);
    }
}

fn set_frame_content_height(
    tree: &mut ElementTree,
    id: &NodeId,
    actual_content_height: f32,
    insets: LayoutInsets,
) {
    if let Some(element) = tree.get_mut(id)
        && let Some(ref mut frame) = element.layout.frame
    {
        frame.content_height = insets.outer_height(actual_content_height);
    }
}

fn set_frame_content_size(
    tree: &mut ElementTree,
    id: &NodeId,
    actual_content_width: f32,
    actual_content_height: f32,
    insets: LayoutInsets,
) {
    if let Some(element) = tree.get_mut(id)
        && let Some(ref mut frame) = element.layout.frame
    {
        frame.content_width = insets.outer_width(actual_content_width);
        frame.content_height = insets.outer_height(actual_content_height);
    }
}

fn expand_frame_height_to_content(
    tree: &mut ElementTree,
    id: &NodeId,
    actual_content_height: f32,
    insets: LayoutInsets,
) {
    let new_height = insets.outer_height(actual_content_height);
    if let Some(element) = tree.get_mut(id)
        && let Some(ref mut frame) = element.layout.frame
    {
        frame.height = new_height;
        frame.content_height = new_height;
    }
}

fn update_scroll_state(tree: &mut ElementTree, id: &NodeId) {
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

    if !(scroll_x_enabled || scroll_y_enabled) {
        return;
    }

    let Some(frame) = element.layout.frame else {
        return;
    };

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

fn shift_subtree(tree: &mut ElementTree, id: &NodeId, dx: f32, dy: f32) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }

    let child_ids = {
        let Some(element) = tree.get_mut(id) else {
            return;
        };
        if let Some(frame) = &mut element.layout.frame {
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

    for child_id in child_ids {
        shift_subtree(tree, &child_id, dx, dy);
    }
}

// =============================================================================
// Layout Output (combined render + event registry)
// =============================================================================

use super::render::{render_tree_scene, render_tree_scene_cached};
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

/// After DOM/scroll changes, produce new outputs without re-running layout.
/// Use this when only scroll positions changed (not structure).
pub fn refresh(tree: &mut ElementTree) -> LayoutOutput {
    let render_output = render_tree_scene_cached(tree);
    refresh_from_render_output(tree, render_output)
}

#[doc(hidden)]
pub fn refresh_uncached_for_benchmark(tree: &mut ElementTree) -> LayoutOutput {
    let render_output = render_tree_scene(tree);
    refresh_from_render_output(tree, render_output)
}

fn refresh_from_render_output(
    tree: &mut ElementTree,
    render_output: super::render::RenderSceneOutput,
) -> LayoutOutput {
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

#[doc(hidden)]
pub fn refresh_reusing_clean_registry_for_benchmark(
    tree: &mut ElementTree,
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> LayoutOutput {
    refresh_reusing_clean_registry(tree, cached_rebuild)
}

#[doc(hidden)]
pub fn refresh_uncached_reusing_clean_registry_for_benchmark(
    tree: &mut ElementTree,
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> LayoutOutput {
    let can_reuse_registry = cached_rebuild.is_some() && !tree.has_registry_refresh_damage();

    if !can_reuse_registry {
        return refresh_uncached_for_benchmark(tree);
    }

    let render_output = render_tree_scene(tree);
    let event_rebuild = cached_rebuild
        .cloned()
        .expect("cached rebuild should be present when registry can be reused");
    let ime_text_state = ime_text_state_from_rebuild(&event_rebuild);

    tree.clear_render_refresh_dirty();

    LayoutOutput {
        scene: render_output.scene,
        event_rebuild,
        event_rebuild_changed: false,
        ime_enabled: render_output.text_input_focused,
        ime_cursor_area: render_output.text_input_cursor_area,
        ime_text_state,
        animations_active: false,
    }
}

pub(crate) fn refresh_reusing_clean_registry(
    tree: &mut ElementTree,
    cached_rebuild: Option<&RegistryRebuildPayload>,
) -> LayoutOutput {
    let can_reuse_registry = cached_rebuild.is_some() && !tree.has_registry_refresh_damage();

    if !can_reuse_registry {
        return refresh(tree);
    }

    let render_output = render_tree_scene_cached(tree);
    let event_rebuild = cached_rebuild
        .cloned()
        .expect("cached rebuild should be present when registry can be reused");
    let ime_text_state = ime_text_state_from_rebuild(&event_rebuild);

    tree.clear_render_refresh_dirty();

    LayoutOutput {
        scene: render_output.scene,
        event_rebuild,
        event_rebuild_changed: false,
        ime_enabled: render_output.text_input_focused,
        ime_cursor_area: render_output.text_input_cursor_area,
        ime_text_state,
        animations_active: false,
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

#[doc(hidden)]
pub fn layout_and_refresh_default_uncached_for_benchmark(
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
    let mut output = refresh_uncached_for_benchmark(tree);
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
    let preparation = prepare_frame_attrs_for_update(tree, scale, Some(runtime), Some(sample_time));
    let can_refresh_without_layout = preparation.animation_result.invalidation.can_refresh_only()
        && prepared_root_has_frame(tree, &preparation);

    if can_refresh_without_layout {
        refresh_prepared_default(tree, preparation)
    } else {
        layout_and_refresh_prepared_default(tree, constraint, preparation)
    }
}

#[doc(hidden)]
pub fn layout_or_refresh_default_with_animation_uncached_for_benchmark(
    tree: &mut ElementTree,
    constraint: Constraint,
    scale: f32,
    runtime: &AnimationRuntime,
    sample_time: Instant,
) -> LayoutUpdateOutput {
    let preparation = prepare_frame_attrs_for_update(tree, scale, Some(runtime), Some(sample_time));
    let can_refresh_without_layout = preparation.animation_result.invalidation.can_refresh_only()
        && prepared_root_has_frame(tree, &preparation);

    if can_refresh_without_layout {
        refresh_prepared_default_uncached_for_benchmark(tree, preparation)
    } else {
        layout_and_refresh_prepared_default_uncached_for_benchmark(tree, constraint, preparation)
    }
}

fn layout_and_refresh_prepared_default_uncached_for_benchmark(
    tree: &mut ElementTree,
    constraint: Constraint,
    preparation: FrameAttrsPreparation,
) -> LayoutUpdateOutput {
    let layout_performed = if let Some(root_id) = preparation.root_id {
        run_layout_passes(
            tree,
            &root_id,
            constraint,
            &SkiaTextMeasurer,
            &FontContext::default(),
            &preparation.animation_result,
        );
        true
    } else {
        false
    };

    let mut output = refresh_uncached_for_benchmark(tree);
    output.animations_active = preparation.animation_result.active;

    LayoutUpdateOutput {
        output,
        layout_performed,
    }
}

pub(crate) fn layout_and_refresh_prepared_default(
    tree: &mut ElementTree,
    constraint: Constraint,
    preparation: FrameAttrsPreparation,
) -> LayoutUpdateOutput {
    let layout_performed = if let Some(root_id) = preparation.root_id {
        run_layout_passes(
            tree,
            &root_id,
            constraint,
            &SkiaTextMeasurer,
            &FontContext::default(),
            &preparation.animation_result,
        );
        true
    } else {
        false
    };

    let mut output = refresh(tree);
    output.animations_active = preparation.animation_result.active;

    LayoutUpdateOutput {
        output,
        layout_performed,
    }
}

fn refresh_prepared_default_uncached_for_benchmark(
    tree: &mut ElementTree,
    preparation: FrameAttrsPreparation,
) -> LayoutUpdateOutput {
    let mut output = refresh_uncached_for_benchmark(tree);
    output.animations_active = preparation.animation_result.active;

    LayoutUpdateOutput {
        output,
        layout_performed: false,
    }
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
