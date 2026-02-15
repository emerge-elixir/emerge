//! Render an ElementTree into DrawCmds.
//!
//! Reads from pre-scaled attrs (scaling is applied in the layout pass).

use super::attrs::{
    Attrs, Background, BorderRadius, BorderStyle, BorderWidth, Color, Font, FontStyle, FontWeight,
    ImageFit, ImageSource, Padding, TextAlign,
};
use super::deserialize::decode_tree;
use super::element::{Element, ElementId, ElementKind, ElementTree, Frame};
use super::layout::{
    font_info_with_inheritance, layout_tree, Constraint, FontContext, SkiaTextMeasurer,
};
use super::scrollbar;
use crate::assets::{self, AssetStatus};
use crate::renderer::{make_font_with_style, DrawCmd};

const SCROLLBAR_COLOR: u32 = 0xD0D5DC99;

/// Render the tree to draw commands.
/// Reads from pre-scaled attrs (layout pass must run first).
pub fn render_tree(tree: &ElementTree) -> Vec<DrawCmd> {
    let Some(root) = tree.root.as_ref() else {
        return Vec::new();
    };

    let mut commands = Vec::new();
    render_element(tree, root, &mut commands, &FontContext::default());
    commands
}

fn render_element(
    tree: &ElementTree,
    id: &ElementId,
    commands: &mut Vec<DrawCmd>,
    inherited: &FontContext,
) {
    let Some(element) = tree.get(id) else {
        return;
    };

    let frame = match element.frame {
        Some(frame) => frame,
        None => return,
    };

    // Read from pre-scaled attrs
    let attrs = &element.attrs;
    let radius = attrs.border_radius.as_ref();

    // Merge inherited font context with this element's attrs
    let element_context = inherited.merge_with_attrs(attrs);

    let transform_state = push_element_transform(commands, frame, attrs);

    render_pre_layers(commands, frame, attrs, radius);

    let clip_state = begin_overflow_clipping(commands, frame, attrs);

    match element.kind {
        ElementKind::Text => render_text_content(commands, frame, attrs, inherited),
        ElementKind::Image => render_image_content(commands, frame, attrs),
        _ => {}
    }

    if element.kind == ElementKind::Paragraph {
        render_paragraph_content(tree, element, commands, &element_context);
    } else {
        render_children_content(tree, element, commands, &element_context, attrs);
    }

    finish_overflow_clipping(commands, frame, attrs, clip_state);

    render_nearby_overlays(commands, frame, attrs);

    pop_element_transform(commands, transform_state);
}

#[derive(Clone, Copy, Debug, Default)]
struct ClipState {
    inner_active: bool,
    compositing: bool,
}

fn render_pre_layers(
    commands: &mut Vec<DrawCmd>,
    frame: Frame,
    attrs: &Attrs,
    radius: Option<&BorderRadius>,
) {
    render_nearby_behind(commands, frame, attrs);
    push_box_shadows(commands, frame, attrs, radius, false);
    push_background(commands, frame, attrs, radius);
    push_box_shadows(commands, frame, attrs, radius, true);
}

fn render_nearby_behind(commands: &mut Vec<DrawCmd>, frame: Frame, attrs: &Attrs) {
    if let Some(behind_bytes) = &attrs.behind {
        render_nearby_element(behind_bytes, frame, NearbyPosition::Behind, commands);
    }
}

fn render_nearby_overlays(commands: &mut Vec<DrawCmd>, frame: Frame, attrs: &Attrs) {
    if let Some(above_bytes) = &attrs.above {
        render_nearby_element(above_bytes, frame, NearbyPosition::Above, commands);
    }
    if let Some(below_bytes) = &attrs.below {
        render_nearby_element(below_bytes, frame, NearbyPosition::Below, commands);
    }
    if let Some(on_left_bytes) = &attrs.on_left {
        render_nearby_element(on_left_bytes, frame, NearbyPosition::OnLeft, commands);
    }
    if let Some(on_right_bytes) = &attrs.on_right {
        render_nearby_element(on_right_bytes, frame, NearbyPosition::OnRight, commands);
    }
    if let Some(in_front_bytes) = &attrs.in_front {
        render_nearby_element(in_front_bytes, frame, NearbyPosition::InFront, commands);
    }
}

fn push_box_shadows(
    commands: &mut Vec<DrawCmd>,
    frame: Frame,
    attrs: &Attrs,
    radius: Option<&BorderRadius>,
    inset: bool,
) {
    let Some(shadows) = &attrs.box_shadows else {
        return;
    };

    for shadow in shadows {
        if shadow.inset != inset {
            continue;
        }

        if inset {
            commands.push(DrawCmd::InsetShadow(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                shadow.offset_x as f32,
                shadow.offset_y as f32,
                shadow.blur as f32,
                shadow.size as f32,
                border_radius_uniform(radius),
                color_to_u32(&shadow.color),
            ));
        } else {
            commands.push(DrawCmd::Shadow(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                shadow.offset_x as f32,
                shadow.offset_y as f32,
                shadow.blur as f32,
                shadow.size as f32,
                border_radius_uniform(radius),
                color_to_u32(&shadow.color),
            ));
        }
    }
}

fn push_background(
    commands: &mut Vec<DrawCmd>,
    frame: Frame,
    attrs: &Attrs,
    radius: Option<&BorderRadius>,
) {
    let Some(background) = &attrs.background else {
        return;
    };

    match background {
        Background::Color(color) => {
            let fill = color_to_u32(color);
            push_background_rect(commands, frame, radius, fill);
        }
        Background::Gradient { from, to, angle } => {
            commands.push(DrawCmd::Gradient(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                color_to_u32(from),
                color_to_u32(to),
                *angle as f32,
                border_radius_uniform(radius),
            ));
        }
        Background::Image { source, fit } => {
            let clipped = push_border_clip(commands, frame, attrs);
            push_image_for_source(
                commands,
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                source,
                *fit,
            );
            if clipped {
                commands.push(DrawCmd::PopClip);
            }
        }
    }
}

fn begin_overflow_clipping(commands: &mut Vec<DrawCmd>, frame: Frame, attrs: &Attrs) -> ClipState {
    // When border_radius + border_width + overflow clip are all present, use
    // nested clips: outer clip at element bounds (outer radius) shapes the
    // element, inner clip at content bounds (inner radius) clips children.
    // The border is drawn between inner pop and outer pop, covering the inner
    // clip's AA fringe.  No SaveLayerAlpha needed.
    let clip = overflow_clip(frame, attrs);
    let compositing = clip.is_some() && needs_compositing_clip(attrs);

    if compositing {
        push_border_clip(commands, frame, attrs);
    }

    let mode = if compositing {
        ClipMode::Hard
    } else {
        ClipMode::AntiAlias
    };
    push_overflow_clip(commands, clip.as_ref(), mode);

    ClipState {
        inner_active: clip.is_some(),
        compositing,
    }
}

fn finish_overflow_clipping(
    commands: &mut Vec<DrawCmd>,
    frame: Frame,
    attrs: &Attrs,
    clip_state: ClipState,
) {
    if clip_state.inner_active {
        commands.push(DrawCmd::PopClip);
    }

    // Border between inner and outer clip to cover AA fringe
    push_border_commands(commands, frame, attrs);

    if clip_state.compositing {
        push_scrollbar_thumbs(commands, frame, attrs);
        commands.push(DrawCmd::PopClip); // pop outer compositing clip
    } else if push_border_clip(commands, frame, attrs) {
        push_scrollbar_thumbs(commands, frame, attrs);
        commands.push(DrawCmd::PopClip);
    } else {
        push_scrollbar_thumbs(commands, frame, attrs);
    }
}

fn render_text_content(
    commands: &mut Vec<DrawCmd>,
    frame: Frame,
    attrs: &Attrs,
    inherited: &FontContext,
) {
    let Some(content) = attrs.content.as_deref() else {
        return;
    };

    // Use inherited font context for missing values
    let font_size = attrs
        .font_size
        .map(|s| s as f32)
        .or(inherited.font_size)
        .unwrap_or(16.0);
    let color = attrs
        .font_color
        .as_ref()
        .map(color_to_u32)
        .or(inherited.font_color)
        .unwrap_or(0xFFFFFFFF);
    let underline = attrs
        .font_underline
        .or(inherited.font_underline)
        .unwrap_or(false);
    let strike = attrs.font_strike.or(inherited.font_strike).unwrap_or(false);
    let letter_spacing = attrs
        .font_letter_spacing
        .map(|v| v as f32)
        .or(inherited.font_letter_spacing)
        .unwrap_or(0.0);
    let word_spacing = attrs
        .font_word_spacing
        .map(|v| v as f32)
        .or(inherited.font_word_spacing)
        .unwrap_or(0.0);
    let (family, weight, italic) = font_info_with_inheritance(attrs, inherited);
    let insets = content_insets(attrs);
    let inset_left = insets.left;
    let inset_top = insets.top;
    let inset_right = insets.right;
    let (ascent, _) = text_metrics_with_font(font_size, &family, weight, italic);
    let text_width = measure_text_width_with_font(
        content,
        font_size,
        &family,
        weight,
        italic,
        letter_spacing,
        word_spacing,
    );
    let content_width = frame.width - inset_left - inset_right;
    let text_align = attrs
        .text_align
        .or(inherited.text_align)
        .unwrap_or_default();
    let text_x = match text_align {
        TextAlign::Left => frame.x + inset_left,
        TextAlign::Center => frame.x + inset_left + (content_width - text_width) / 2.0,
        TextAlign::Right => frame.x + frame.width - inset_right - text_width,
    };
    let baseline_y = frame.y + inset_top + ascent;

    if letter_spacing == 0.0 && word_spacing == 0.0 {
        commands.push(DrawCmd::TextWithFont(
            text_x,
            baseline_y,
            content.to_string(),
            font_size,
            color,
            family.clone(),
            weight,
            italic,
        ));
    } else {
        let measure_font = make_font_with_style(&family, weight, italic, font_size);
        let mut cursor_x = text_x;
        let mut chars = content.chars().peekable();

        while let Some(ch) = chars.next() {
            let glyph = ch.to_string();
            commands.push(DrawCmd::TextWithFont(
                cursor_x,
                baseline_y,
                glyph.clone(),
                font_size,
                color,
                family.clone(),
                weight,
                italic,
            ));

            let (glyph_width, _bounds) = measure_font.measure_str(&glyph, None);
            cursor_x += glyph_width;

            if chars.peek().is_some() {
                cursor_x += letter_spacing;
                if ch.is_whitespace() {
                    cursor_x += word_spacing;
                }
            }
        }
    }

    push_text_decorations(
        commands, text_x, baseline_y, text_width, font_size, color, underline, strike,
    );
}

fn render_image_content(commands: &mut Vec<DrawCmd>, frame: Frame, attrs: &Attrs) {
    let Some(source) = attrs.image_src.as_ref() else {
        return;
    };

    let fit = attrs.image_fit.unwrap_or(ImageFit::Contain);
    let (draw_x, draw_y, draw_w, draw_h) = content_rect(frame, attrs);

    push_image_for_source(commands, draw_x, draw_y, draw_w, draw_h, source, fit);
}

fn render_paragraph_content(
    tree: &ElementTree,
    element: &Element,
    commands: &mut Vec<DrawCmd>,
    element_context: &FontContext,
) {
    let attrs = &element.attrs;

    // Render floating children before paragraph fragments.
    for child_id in &element.children {
        let should_render_float_child = tree.get(child_id).is_some_and(|child| {
            matches!(
                child.attrs.align_x,
                Some(super::attrs::AlignX::Left | super::attrs::AlignX::Right)
            )
        });

        if should_render_float_child {
            render_element(tree, child_id, commands, element_context);
        }
    }

    if let Some(fragments) = &attrs.paragraph_fragments {
        for frag in fragments {
            let baseline_y = frag.y + frag.ascent;
            commands.push(DrawCmd::TextWithFont(
                frag.x,
                baseline_y,
                frag.text.clone(),
                frag.font_size,
                frag.color,
                frag.family.clone(),
                frag.weight,
                frag.italic,
            ));

            if frag.underline || frag.strike {
                let font =
                    make_font_with_style(&frag.family, frag.weight, frag.italic, frag.font_size);
                let (word_width, _) = font.measure_str(&frag.text, None);
                push_text_decorations(
                    commands,
                    frag.x,
                    baseline_y,
                    word_width,
                    frag.font_size,
                    frag.color,
                    frag.underline,
                    frag.strike,
                );
            }
        }
    }
}

fn render_children_content(
    tree: &ElementTree,
    element: &Element,
    commands: &mut Vec<DrawCmd>,
    element_context: &FontContext,
    attrs: &Attrs,
) {
    let scrollable = attrs.scrollbar_x.unwrap_or(false) || attrs.scrollbar_y.unwrap_or(false);
    let scroll_x = attrs.scroll_x.unwrap_or(0.0) as f32;
    let scroll_y = attrs.scroll_y.unwrap_or(0.0) as f32;
    let has_children = !element.children.is_empty();

    if has_children && scrollable && (scroll_x != 0.0 || scroll_y != 0.0) {
        commands.push(DrawCmd::Save);
        commands.push(DrawCmd::Translate(-scroll_x, -scroll_y));
    }

    for child_id in &element.children {
        render_element(tree, child_id, commands, element_context);
    }

    if has_children && scrollable && (scroll_x != 0.0 || scroll_y != 0.0) {
        commands.push(DrawCmd::Restore);
    }
}

fn push_image_for_source(
    commands: &mut Vec<DrawCmd>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    source: &ImageSource,
    fit: ImageFit,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }

    assets::ensure_source(source);

    match assets::source_status(source) {
        Some(AssetStatus::Ready(asset)) => {
            commands.push(DrawCmd::Image(x, y, w, h, asset.id, fit));
        }
        Some(AssetStatus::Failed) => {
            commands.push(DrawCmd::ImageFailed(x, y, w, h));
        }
        _ => {
            commands.push(DrawCmd::ImageLoading(x, y, w, h));
        }
    }
}

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
        "gray" => 0x808080FF,
        "grey" => 0x808080FF,
        "navy" => 0x000080FF,
        "teal" => 0x008080FF,
        _ => 0xFFFFFFFF,
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ResolvedInsets {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

fn resolved_padding(padding: Option<&Padding>) -> ResolvedInsets {
    match padding {
        Some(Padding::Uniform(value)) => {
            let value = *value as f32;
            ResolvedInsets {
                top: value,
                right: value,
                bottom: value,
                left: value,
            }
        }
        Some(Padding::Sides {
            top,
            right,
            bottom,
            left,
        }) => ResolvedInsets {
            top: *top as f32,
            right: *right as f32,
            bottom: *bottom as f32,
            left: *left as f32,
        },
        None => ResolvedInsets::default(),
    }
}

fn resolved_border_width(border_width: Option<&BorderWidth>) -> ResolvedInsets {
    match border_width {
        Some(BorderWidth::Uniform(value)) => {
            let value = *value as f32;
            ResolvedInsets {
                top: value,
                right: value,
                bottom: value,
                left: value,
            }
        }
        Some(BorderWidth::Sides {
            top,
            right,
            bottom,
            left,
        }) => ResolvedInsets {
            top: *top as f32,
            right: *right as f32,
            bottom: *bottom as f32,
            left: *left as f32,
        },
        None => ResolvedInsets::default(),
    }
}

fn content_insets(attrs: &Attrs) -> ResolvedInsets {
    let padding = resolved_padding(attrs.padding.as_ref());
    let border = resolved_border_width(attrs.border_width.as_ref());
    ResolvedInsets {
        top: padding.top + border.top,
        right: padding.right + border.right,
        bottom: padding.bottom + border.bottom,
        left: padding.left + border.left,
    }
}

fn content_rect(frame: Frame, attrs: &Attrs) -> (f32, f32, f32, f32) {
    let insets = content_insets(attrs);
    let x = frame.x + insets.left;
    let y = frame.y + insets.top;
    let w = (frame.width - insets.left - insets.right).max(0.0);
    let h = (frame.height - insets.top - insets.bottom).max(0.0);
    (x, y, w, h)
}

/// Describes the clip operation mode to apply.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ClipMode {
    AntiAlias,
    Hard,
}

/// Describes the type of overflow clip to apply.
#[derive(Clone, Debug, PartialEq)]
enum OverflowClip {
    Rect(f32, f32, f32, f32),
    Rounded(f32, f32, f32, f32, f32),
    RoundedCorners(f32, f32, f32, f32, f32, f32, f32, f32),
}

fn overflow_clip(frame: Frame, attrs: &super::attrs::Attrs) -> Option<OverflowClip> {
    let clip_x = attrs.clip_x.unwrap_or(false) || attrs.scrollbar_x.unwrap_or(false);
    let clip_y = attrs.clip_y.unwrap_or(false) || attrs.scrollbar_y.unwrap_or(false);

    if !(clip_x || clip_y) {
        return None;
    }

    let border = resolved_border_width(attrs.border_width.as_ref());
    let (content_x, content_y, content_width, content_height) = content_rect(frame, attrs);

    let (x, width) = if clip_x {
        (content_x, content_width)
    } else {
        (frame.x, frame.width)
    };
    let (y, height) = if clip_y {
        (content_y, content_height)
    } else {
        (frame.y, frame.height)
    };

    let w = width.max(0.0);
    let h = height.max(0.0);

    // When border-radius is set, use a rounded clip with inner radius reduced by border width
    let max_border = border
        .left
        .max(border.top)
        .max(border.right)
        .max(border.bottom);
    match attrs.border_radius.as_ref() {
        Some(BorderRadius::Uniform(r)) if *r > 0.0 => {
            let inner_r = (*r as f32 - max_border).max(0.0);
            if inner_r > 0.0 {
                Some(OverflowClip::Rounded(x, y, w, h, inner_r))
            } else {
                Some(OverflowClip::Rect(x, y, w, h))
            }
        }
        Some(BorderRadius::Corners { tl, tr, br, bl }) => {
            let inner_tl = (*tl as f32 - max_border).max(0.0);
            let inner_tr = (*tr as f32 - max_border).max(0.0);
            let inner_br = (*br as f32 - max_border).max(0.0);
            let inner_bl = (*bl as f32 - max_border).max(0.0);
            if inner_tl > 0.0 || inner_tr > 0.0 || inner_br > 0.0 || inner_bl > 0.0 {
                Some(OverflowClip::RoundedCorners(
                    x, y, w, h, inner_tl, inner_tr, inner_br, inner_bl,
                ))
            } else {
                Some(OverflowClip::Rect(x, y, w, h))
            }
        }
        _ => Some(OverflowClip::Rect(x, y, w, h)),
    }
}

/// Returns true when the element needs an outer compositing clip to eliminate
/// the hairline gap between the inner overflow clip and the border stroke.
/// This happens when both border_radius and border_width are present — the two
/// independent AA boundaries at the same geometric position create a seam.
fn needs_compositing_clip(attrs: &super::attrs::Attrs) -> bool {
    let has_border_radius = match attrs.border_radius.as_ref() {
        Some(BorderRadius::Uniform(r)) => *r > 0.0,
        Some(BorderRadius::Corners { tl, tr, br, bl }) => {
            *tl > 0.0 || *tr > 0.0 || *br > 0.0 || *bl > 0.0
        }
        None => false,
    };
    let has_border_width = match attrs.border_width.as_ref() {
        Some(BorderWidth::Uniform(w)) => *w > 0.0,
        Some(BorderWidth::Sides {
            top,
            right,
            bottom,
            left,
        }) => *top > 0.0 || *right > 0.0 || *bottom > 0.0 || *left > 0.0,
        None => false,
    };
    has_border_radius && has_border_width
}

fn push_border_clip(
    commands: &mut Vec<DrawCmd>,
    frame: Frame,
    attrs: &super::attrs::Attrs,
) -> bool {
    match attrs.border_radius.as_ref() {
        Some(BorderRadius::Uniform(value)) if *value > 0.0 => {
            commands.push(DrawCmd::PushClipRounded(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                *value as f32,
            ));
            true
        }
        Some(BorderRadius::Corners { tl, tr, br, bl }) => {
            commands.push(DrawCmd::PushClipRoundedCorners(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                *tl as f32,
                *tr as f32,
                *br as f32,
                *bl as f32,
            ));
            true
        }
        _ => false,
    }
}

fn push_overflow_clip(commands: &mut Vec<DrawCmd>, clip: Option<&OverflowClip>, mode: ClipMode) {
    match clip {
        Some(OverflowClip::Rect(x, y, w, h)) => {
            if mode == ClipMode::Hard {
                commands.push(DrawCmd::PushClipHard(*x, *y, *w, *h));
            } else {
                commands.push(DrawCmd::PushClip(*x, *y, *w, *h));
            }
        }
        Some(OverflowClip::Rounded(x, y, w, h, r)) => {
            if mode == ClipMode::Hard {
                commands.push(DrawCmd::PushClipRoundedHard(*x, *y, *w, *h, *r));
            } else {
                commands.push(DrawCmd::PushClipRounded(*x, *y, *w, *h, *r));
            }
        }
        Some(OverflowClip::RoundedCorners(x, y, w, h, tl, tr, br, bl)) => {
            if mode == ClipMode::Hard {
                commands.push(DrawCmd::PushClipRoundedCornersHard(
                    *x, *y, *w, *h, *tl, *tr, *br, *bl,
                ));
            } else {
                commands.push(DrawCmd::PushClipRoundedCorners(
                    *x, *y, *w, *h, *tl, *tr, *br, *bl,
                ));
            }
        }
        None => {}
    }
}

fn push_scrollbar_thumbs(commands: &mut Vec<DrawCmd>, frame: Frame, attrs: &super::attrs::Attrs) {
    if let Some(metrics) = scrollbar::vertical_metrics(frame, attrs) {
        commands.push(DrawCmd::RoundedRect(
            metrics.thumb_x,
            metrics.thumb_y,
            metrics.thumb_width,
            metrics.thumb_height,
            metrics.thumb_width / 2.0,
            SCROLLBAR_COLOR,
        ));
    }

    if let Some(metrics) = scrollbar::horizontal_metrics(frame, attrs) {
        commands.push(DrawCmd::RoundedRect(
            metrics.thumb_x,
            metrics.thumb_y,
            metrics.thumb_width,
            metrics.thumb_height,
            metrics.thumb_height / 2.0,
            SCROLLBAR_COLOR,
        ));
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct TransformState {
    active: bool,
    has_alpha_layer: bool,
}

fn push_element_transform(
    commands: &mut Vec<DrawCmd>,
    frame: Frame,
    attrs: &super::attrs::Attrs,
) -> TransformState {
    let move_x = attrs.move_x.unwrap_or(0.0) as f32;
    let move_y = attrs.move_y.unwrap_or(0.0) as f32;
    let rotate = attrs.rotate.unwrap_or(0.0) as f32;
    let scale = attrs.scale.unwrap_or(1.0) as f32;
    let alpha = attrs.alpha.unwrap_or(1.0) as f32;

    let has_translation = move_x != 0.0 || move_y != 0.0;
    let has_rotation = rotate != 0.0;
    let has_scale = (scale - 1.0).abs() > f32::EPSILON;
    let has_alpha = alpha < 1.0;

    if !(has_translation || has_rotation || has_scale || has_alpha) {
        return TransformState::default();
    }

    commands.push(DrawCmd::Save);

    if has_translation {
        commands.push(DrawCmd::Translate(move_x, move_y));
    }

    if has_rotation || has_scale {
        let center_x = frame.x + frame.width / 2.0;
        let center_y = frame.y + frame.height / 2.0;
        commands.push(DrawCmd::Translate(center_x, center_y));
        if has_rotation {
            commands.push(DrawCmd::Rotate(rotate));
        }
        if has_scale {
            commands.push(DrawCmd::Scale(scale, scale));
        }
        commands.push(DrawCmd::Translate(-center_x, -center_y));
    }

    if has_alpha {
        commands.push(DrawCmd::SaveLayerAlpha(alpha));
    }

    TransformState {
        active: true,
        has_alpha_layer: has_alpha,
    }
}

fn pop_element_transform(commands: &mut Vec<DrawCmd>, state: TransformState) {
    if !state.active {
        return;
    }

    if state.has_alpha_layer {
        commands.push(DrawCmd::Restore);
    }
    commands.push(DrawCmd::Restore);
}

fn push_background_rect(
    commands: &mut Vec<DrawCmd>,
    frame: Frame,
    radius: Option<&BorderRadius>,
    fill: u32,
) {
    match radius {
        Some(BorderRadius::Uniform(value)) if *value > 0.0 => {
            commands.push(DrawCmd::RoundedRect(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                *value as f32,
                fill,
            ));
        }
        Some(BorderRadius::Corners { tl, tr, br, bl }) => {
            commands.push(DrawCmd::RoundedRectCorners(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                *tl as f32,
                *tr as f32,
                *br as f32,
                *bl as f32,
                fill,
            ));
        }
        _ => {
            commands.push(DrawCmd::Rect(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                fill,
            ));
        }
    }
}

fn push_border_rect(
    commands: &mut Vec<DrawCmd>,
    frame: Frame,
    radius: Option<&BorderRadius>,
    border_width: f32,
    color: u32,
    style: BorderStyle,
) {
    match radius {
        Some(BorderRadius::Uniform(value)) if *value > 0.0 => {
            commands.push(DrawCmd::Border(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                *value as f32,
                border_width,
                color,
                style,
            ));
        }
        Some(BorderRadius::Corners { tl, tr, br, bl }) => {
            commands.push(DrawCmd::BorderCorners(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                *tl as f32,
                *tr as f32,
                *br as f32,
                *bl as f32,
                border_width,
                color,
                style,
            ));
        }
        _ => {
            commands.push(DrawCmd::Border(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                0.0,
                border_width,
                color,
                style,
            ));
        }
    }
}

/// Push border draw commands for an element.
fn push_border_commands(commands: &mut Vec<DrawCmd>, frame: Frame, attrs: &super::attrs::Attrs) {
    let radius = attrs.border_radius.as_ref();
    if let (Some(border_width), Some(border_color)) =
        (attrs.border_width.as_ref(), &attrs.border_color)
    {
        let color = color_to_u32(border_color);
        let style = attrs.border_style.unwrap_or(BorderStyle::Solid);
        match border_width {
            BorderWidth::Uniform(w) if *w > 0.0 => {
                push_border_rect(commands, frame, radius, *w as f32, color, style);
            }
            BorderWidth::Sides {
                top,
                right,
                bottom,
                left,
            } => {
                commands.push(DrawCmd::BorderEdges(
                    frame.x,
                    frame.y,
                    frame.width,
                    frame.height,
                    border_radius_uniform(radius),
                    *top as f32,
                    *right as f32,
                    *bottom as f32,
                    *left as f32,
                    color,
                    style,
                ));
            }
            _ => {}
        }
    }
}

fn text_metrics_with_font(font_size: f32, family: &str, weight: u16, italic: bool) -> (f32, f32) {
    use skia_safe::Font;

    let font = make_font_with_style(family, weight, italic, font_size);
    let (_, metrics) = font.metrics();
    (metrics.ascent.abs(), metrics.descent)
}

fn push_text_decorations(
    commands: &mut Vec<DrawCmd>,
    x: f32,
    baseline_y: f32,
    width: f32,
    font_size: f32,
    color: u32,
    underline: bool,
    strike: bool,
) {
    if width <= 0.0 || (!underline && !strike) {
        return;
    }

    let thickness = (font_size * 0.06).max(1.0);
    if underline {
        let y = baseline_y + font_size * 0.08 - thickness / 2.0;
        commands.push(DrawCmd::Rect(x, y, width, thickness, color));
    }
    if strike {
        let y = baseline_y - font_size * 0.3 - thickness / 2.0;
        commands.push(DrawCmd::Rect(x, y, width, thickness, color));
    }
}

fn measure_text_width_with_font(
    text: &str,
    font_size: f32,
    family: &str,
    weight: u16,
    italic: bool,
    letter_spacing: f32,
    word_spacing: f32,
) -> f32 {
    let font = make_font_with_style(family, weight, italic, font_size);

    if text.is_empty() {
        return 0.0;
    }

    if letter_spacing == 0.0 && word_spacing == 0.0 {
        let (width, _bounds) = font.measure_str(text, None);
        return width;
    }

    let mut total = 0.0;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        let glyph = ch.to_string();
        let (glyph_width, _bounds) = font.measure_str(&glyph, None);
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

// =============================================================================
// Nearby Element Rendering
// =============================================================================

/// Position for nearby elements relative to the parent.
#[derive(Clone, Copy, Debug)]
enum NearbyPosition {
    Above,
    Below,
    OnLeft,
    OnRight,
    InFront,
    Behind,
}

/// Decode, layout, and render a nearby element.
fn render_nearby_element(
    data: &[u8],
    parent_frame: Frame,
    position: NearbyPosition,
    commands: &mut Vec<DrawCmd>,
) {
    // Decode the nearby element tree from EMRG bytes
    let mut nearby_tree = match decode_tree(data) {
        Ok(tree) => tree,
        Err(_) => return,
    };

    if nearby_tree.is_empty() {
        return;
    }

    // Layout the nearby element with a large constraint (it will size to content)
    let constraint = Constraint::new(10000.0, 10000.0);
    layout_tree(&mut nearby_tree, constraint, 1.0, &SkiaTextMeasurer);

    // Get the nearby element's computed size
    let Some(root_id) = nearby_tree.root.clone() else {
        return;
    };
    let nearby_frame = {
        let Some(root) = nearby_tree.get(&root_id) else {
            return;
        };
        let Some(frame) = root.frame else {
            return;
        };
        frame
    };

    // Calculate the offset based on position
    let (offset_x, offset_y) = match position {
        NearbyPosition::Above => {
            // Centered horizontally, positioned above
            let x = parent_frame.x + (parent_frame.width - nearby_frame.width) / 2.0;
            let y = parent_frame.y - nearby_frame.height;
            (x, y)
        }
        NearbyPosition::Below => {
            // Centered horizontally, positioned below
            let x = parent_frame.x + (parent_frame.width - nearby_frame.width) / 2.0;
            let y = parent_frame.y + parent_frame.height;
            (x, y)
        }
        NearbyPosition::OnLeft => {
            // Positioned to the left, centered vertically
            let x = parent_frame.x - nearby_frame.width;
            let y = parent_frame.y + (parent_frame.height - nearby_frame.height) / 2.0;
            (x, y)
        }
        NearbyPosition::OnRight => {
            // Positioned to the right, centered vertically
            let x = parent_frame.x + parent_frame.width;
            let y = parent_frame.y + (parent_frame.height - nearby_frame.height) / 2.0;
            (x, y)
        }
        NearbyPosition::InFront | NearbyPosition::Behind => {
            // Centered on the parent
            let x = parent_frame.x + (parent_frame.width - nearby_frame.width) / 2.0;
            let y = parent_frame.y + (parent_frame.height - nearby_frame.height) / 2.0;
            (x, y)
        }
    };

    // Shift the entire nearby tree to the calculated position
    shift_nearby_tree(&mut nearby_tree, offset_x, offset_y);

    // Render the nearby tree
    render_element(&nearby_tree, &root_id, commands, &FontContext::default());
}

/// Shift all frames in a nearby tree by the given offset.
fn shift_nearby_tree(tree: &mut ElementTree, offset_x: f32, offset_y: f32) {
    for element in tree.nodes.values_mut() {
        if let Some(ref mut frame) = element.frame {
            frame.x += offset_x;
            frame.y += offset_y;
        }
    }
}

/// Extract a uniform radius value from a BorderRadius, or 0.0 if per-corner.
fn border_radius_uniform(radius: Option<&BorderRadius>) -> f32 {
    match radius {
        Some(BorderRadius::Uniform(value)) => *value as f32,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::Attrs;
    use crate::tree::element::Element;
    use crate::tree::serialize::encode_tree;

    #[test]
    fn test_named_colors() {
        // Test basic colors
        assert_eq!(named_color("white"), 0xFFFFFFFF);
        assert_eq!(named_color("black"), 0x000000FF);
        assert_eq!(named_color("red"), 0xFF0000FF);
        assert_eq!(named_color("green"), 0x00FF00FF);
        assert_eq!(named_color("blue"), 0x0000FFFF);

        // Test newly added colors
        assert_eq!(named_color("cyan"), 0x00FFFFFF);
        assert_eq!(named_color("magenta"), 0xFF00FFFF);
        assert_eq!(named_color("yellow"), 0xFFFF00FF);
        assert_eq!(named_color("orange"), 0xFFA500FF);
        assert_eq!(named_color("purple"), 0x800080FF);
        assert_eq!(named_color("pink"), 0xFFC0CBFF);
        assert_eq!(named_color("navy"), 0x000080FF);
        assert_eq!(named_color("teal"), 0x008080FF);

        // Test both spellings of gray
        assert_eq!(named_color("gray"), 0x808080FF);
        assert_eq!(named_color("grey"), 0x808080FF);

        // Test unknown color defaults to white
        assert_eq!(named_color("unknown"), 0xFFFFFFFF);
    }

    #[test]
    fn test_color_to_u32() {
        // Test RGB color
        let rgb = Color::Rgb {
            r: 255,
            g: 128,
            b: 64,
        };
        assert_eq!(color_to_u32(&rgb), 0xFF8040FF);

        // Test RGBA color
        let rgba = Color::Rgba {
            r: 255,
            g: 128,
            b: 64,
            a: 200,
        };
        assert_eq!(color_to_u32(&rgba), 0xFF8040C8);

        // Test named color
        let named = Color::Named("cyan".to_string());
        assert_eq!(color_to_u32(&named), 0x00FFFFFF);
    }

    #[test]
    fn test_render_image_source_pending_emits_loading_placeholder() {
        let id = ElementId::from_term_bytes(vec![9]);
        let mut attrs = Attrs::default();
        attrs.image_src = Some(ImageSource::Logical("images/photo.jpg".to_string()));
        attrs.image_fit = Some(ImageFit::Contain);

        let mut element = Element::with_attrs(id.clone(), ElementKind::Image, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 90.0,
            content_width: 120.0,
            content_height: 90.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id);
        tree.insert(element);

        let commands = render_tree(&tree);

        assert!(commands
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::ImageLoading(_, _, _, _))));
    }

    #[test]
    fn test_render_text_with_underline_and_strike_emits_decoration_rects() {
        let mut attrs = Attrs::default();
        attrs.content = Some("Decorated".to_string());
        attrs.font_size = Some(18.0);
        attrs.font_color = Some(Color::Rgb { r: 1, g: 2, b: 3 });
        attrs.font_underline = Some(true);
        attrs.font_strike = Some(true);

        let tree = build_text_tree_with_frame(
            attrs,
            Frame {
                x: 10.0,
                y: 20.0,
                width: 220.0,
                height: 40.0,
                content_width: 220.0,
                content_height: 40.0,
            },
        );

        let commands = render_tree(&tree);

        assert!(commands.iter().any(|cmd| {
            matches!(
                cmd,
                DrawCmd::TextWithFont(_, _, content, _, _, _, _, _) if content == "Decorated"
            )
        }));

        let decoration_rects: Vec<(f32, f32, f32, f32)> = commands
            .iter()
            .filter_map(|cmd| match cmd {
                DrawCmd::Rect(x, y, w, h, color) if *color == 0x010203FF => Some((*x, *y, *w, *h)),
                _ => None,
            })
            .collect();

        assert_eq!(decoration_rects.len(), 2);
        assert!(decoration_rects
            .iter()
            .all(|(_, _, width, height)| *width > 0.0 && *height >= 1.0));
    }

    #[test]
    fn test_render_text_with_spacing_emits_per_glyph_commands() {
        let mut attrs = Attrs::default();
        attrs.content = Some("A A".to_string());
        attrs.font_size = Some(16.0);
        attrs.font_letter_spacing = Some(4.0);
        attrs.font_word_spacing = Some(6.0);

        let tree = build_text_tree_with_frame(
            attrs,
            Frame {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 40.0,
                content_width: 200.0,
                content_height: 40.0,
            },
        );

        let commands = render_tree(&tree);

        let text_cmds: Vec<(f32, String)> = commands
            .iter()
            .filter_map(|cmd| match cmd {
                DrawCmd::TextWithFont(x, _y, text, _size, _fill, _family, _weight, _italic) => {
                    Some((*x, text.clone()))
                }
                _ => None,
            })
            .collect();

        assert_eq!(text_cmds.len(), 3);
        assert_eq!(text_cmds[0].1, "A");
        assert_eq!(text_cmds[1].1, " ");
        assert_eq!(text_cmds[2].1, "A");
        assert!(text_cmds[0].0 < text_cmds[1].0);
        assert!(text_cmds[1].0 < text_cmds[2].0);
    }

    #[test]
    fn test_render_text_insets_by_padding_and_border() {
        let mut attrs = Attrs::default();
        attrs.content = Some("Inset".to_string());
        attrs.font_size = Some(16.0);
        attrs.padding = Some(Padding::Uniform(4.0));
        attrs.border_width = Some(BorderWidth::Uniform(3.0));

        let tree = build_text_tree_with_frame(
            attrs,
            Frame {
                x: 10.0,
                y: 20.0,
                width: 200.0,
                height: 60.0,
                content_width: 200.0,
                content_height: 60.0,
            },
        );

        let commands = render_tree(&tree);
        let (ascent, _) = text_metrics_with_font(16.0, "default", 400, false);

        let text_cmd = commands
            .iter()
            .find(|cmd| matches!(cmd, DrawCmd::TextWithFont(..)))
            .expect("text command should exist");

        match text_cmd {
            DrawCmd::TextWithFont(x, y, content, _, _, _, _, _) => {
                assert_eq!(*x, 17.0, "x should include 4px padding + 3px border");
                assert_eq!(*y, 27.0 + ascent, "baseline should include top insets");
                assert_eq!(content, "Inset");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_nearby_position_calculations() {
        // Test Above: should be centered horizontally, positioned above
        let parent = Frame {
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 50.0,
            content_width: 200.0,
            content_height: 50.0,
        };
        let nearby = Frame {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 20.0,
            content_width: 50.0,
            content_height: 20.0,
        };

        // Above: x = 100 + (200 - 50) / 2 = 175, y = 100 - 20 = 80
        let (x, y) = match NearbyPosition::Above {
            NearbyPosition::Above => {
                let x = parent.x + (parent.width - nearby.width) / 2.0;
                let y = parent.y - nearby.height;
                (x, y)
            }
            _ => unreachable!(),
        };
        assert_eq!(x, 175.0);
        assert_eq!(y, 80.0);

        // Below: x = 175, y = 100 + 50 = 150
        let (x, y) = match NearbyPosition::Below {
            NearbyPosition::Below => {
                let x = parent.x + (parent.width - nearby.width) / 2.0;
                let y = parent.y + parent.height;
                (x, y)
            }
            _ => unreachable!(),
        };
        assert_eq!(x, 175.0);
        assert_eq!(y, 150.0);

        // OnLeft: x = 100 - 50 = 50, y = 100 + (50 - 20) / 2 = 115
        let (x, y) = match NearbyPosition::OnLeft {
            NearbyPosition::OnLeft => {
                let x = parent.x - nearby.width;
                let y = parent.y + (parent.height - nearby.height) / 2.0;
                (x, y)
            }
            _ => unreachable!(),
        };
        assert_eq!(x, 50.0);
        assert_eq!(y, 115.0);

        // OnRight: x = 100 + 200 = 300, y = 115
        let (x, y) = match NearbyPosition::OnRight {
            NearbyPosition::OnRight => {
                let x = parent.x + parent.width;
                let y = parent.y + (parent.height - nearby.height) / 2.0;
                (x, y)
            }
            _ => unreachable!(),
        };
        assert_eq!(x, 300.0);
        assert_eq!(y, 115.0);

        // InFront: centered on parent x = 175, y = 115
        let (x, y) = match NearbyPosition::InFront {
            NearbyPosition::InFront => {
                let x = parent.x + (parent.width - nearby.width) / 2.0;
                let y = parent.y + (parent.height - nearby.height) / 2.0;
                (x, y)
            }
            _ => unreachable!(),
        };
        assert_eq!(x, 175.0);
        assert_eq!(y, 115.0);
    }

    fn build_tree_with_attrs(mut attrs: Attrs) -> ElementTree {
        if attrs.background.is_none() {
            attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
        }

        let id = ElementId::from_term_bytes(vec![1]);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id);
        tree.insert(element);
        tree
    }

    fn build_tree_with_frame(mut attrs: Attrs, frame: Frame) -> ElementTree {
        if attrs.background.is_none() {
            attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
        }

        let id = ElementId::from_term_bytes(vec![1]);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(frame);

        let mut tree = ElementTree::new();
        tree.root = Some(id);
        tree.insert(element);
        tree
    }

    fn build_text_tree_with_frame(attrs: Attrs, frame: Frame) -> ElementTree {
        let id = ElementId::from_term_bytes(vec![2]);
        let mut element = Element::with_attrs(id.clone(), ElementKind::Text, Vec::new(), attrs);
        element.frame = Some(frame);

        let mut tree = ElementTree::new();
        tree.root = Some(id);
        tree.insert(element);
        tree
    }

    fn encode_nearby_el(width: f64, height: f64, rgb: (u8, u8, u8)) -> Vec<u8> {
        fn attrs_raw(width: f64, height: f64, rgb: (u8, u8, u8)) -> Vec<u8> {
            let mut raw = Vec::new();
            raw.extend_from_slice(&3_u16.to_be_bytes());

            raw.push(1);
            raw.push(2);
            raw.extend_from_slice(&width.to_be_bytes());

            raw.push(2);
            raw.push(2);
            raw.extend_from_slice(&height.to_be_bytes());

            raw.push(12);
            raw.push(0);
            raw.push(0);
            raw.push(rgb.0);
            raw.push(rgb.1);
            raw.push(rgb.2);

            raw
        }

        let id = ElementId::from_term_bytes(vec![77]);
        let element = Element::with_attrs(
            id.clone(),
            ElementKind::El,
            attrs_raw(width, height, rgb),
            Attrs::default(),
        );
        let mut tree = ElementTree::new();
        tree.root = Some(id);
        tree.insert(element);
        encode_tree(&tree)
    }

    #[test]
    fn test_render_emits_translate_for_move() {
        let mut attrs = Attrs::default();
        attrs.move_x = Some(10.0);
        attrs.move_y = Some(5.0);
        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        assert_eq!(
            commands,
            vec![
                DrawCmd::Save,
                DrawCmd::Translate(10.0, 5.0),
                DrawCmd::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF),
                DrawCmd::Restore,
            ]
        );
    }

    #[test]
    fn test_render_emits_rotate_for_rotation() {
        let mut attrs = Attrs::default();
        attrs.rotate = Some(45.0);
        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        assert_eq!(
            commands,
            vec![
                DrawCmd::Save,
                DrawCmd::Translate(50.0, 25.0),
                DrawCmd::Rotate(45.0),
                DrawCmd::Translate(-50.0, -25.0),
                DrawCmd::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF),
                DrawCmd::Restore,
            ]
        );
    }

    #[test]
    fn test_render_emits_scale_for_scale() {
        let mut attrs = Attrs::default();
        attrs.scale = Some(1.1);
        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        assert_eq!(
            commands,
            vec![
                DrawCmd::Save,
                DrawCmd::Translate(50.0, 25.0),
                DrawCmd::Scale(1.1, 1.1),
                DrawCmd::Translate(-50.0, -25.0),
                DrawCmd::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF),
                DrawCmd::Restore,
            ]
        );
    }

    #[test]
    fn test_render_emits_alpha_layer() {
        let mut attrs = Attrs::default();
        attrs.alpha = Some(0.5);
        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        assert_eq!(
            commands,
            vec![
                DrawCmd::Save,
                DrawCmd::SaveLayerAlpha(0.5),
                DrawCmd::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF),
                DrawCmd::Restore,
                DrawCmd::Restore,
            ]
        );
    }

    #[test]
    fn test_render_skips_transform_when_default() {
        let attrs = Attrs::default();
        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        assert_eq!(
            commands,
            vec![DrawCmd::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)]
        );
    }

    #[test]
    fn test_render_scrollbar_y_thumb() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(50.0);
        attrs.border_radius = Some(BorderRadius::Uniform(8.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 150.0,
        };
        let tree = build_tree_with_frame(attrs, frame);
        let commands = render_tree(&tree);

        assert_eq!(
            commands,
            vec![
                DrawCmd::RoundedRect(0.0, 0.0, 100.0, 50.0, 8.0, 0x000000FF),
                DrawCmd::PushClipRounded(0.0, 0.0, 100.0, 50.0, 8.0),
                DrawCmd::PopClip,
                DrawCmd::PushClipRounded(0.0, 0.0, 100.0, 50.0, 8.0),
                DrawCmd::RoundedRect(95.0, 13.0, 5.0, 24.0, 2.5, SCROLLBAR_COLOR),
                DrawCmd::PopClip,
            ]
        );
    }

    #[test]
    fn test_render_scrollbar_x_thumb() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scroll_x = Some(30.0);
        attrs.border_radius = Some(BorderRadius::Corners {
            tl: 4.0,
            tr: 6.0,
            br: 12.0,
            bl: 8.0,
        });
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 40.0,
            content_width: 160.0,
            content_height: 40.0,
        };
        let tree = build_tree_with_frame(attrs, frame);
        let commands = render_tree(&tree);

        assert_eq!(
            commands,
            vec![
                DrawCmd::RoundedRectCorners(0.0, 0.0, 80.0, 40.0, 4.0, 6.0, 12.0, 8.0, 0x000000FF),
                DrawCmd::PushClipRoundedCorners(0.0, 0.0, 80.0, 40.0, 4.0, 6.0, 12.0, 8.0),
                DrawCmd::PopClip,
                DrawCmd::PushClipRoundedCorners(0.0, 0.0, 80.0, 40.0, 4.0, 6.0, 12.0, 8.0),
                DrawCmd::RoundedRect(15.0, 35.0, 40.0, 5.0, 2.5, SCROLLBAR_COLOR),
                DrawCmd::PopClip,
            ]
        );
    }

    #[test]
    fn test_render_scrollbar_hover_uses_wider_thumb() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(50.0);
        attrs.scrollbar_hover_axis = Some(crate::tree::attrs::ScrollbarHoverAxis::Y);
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 150.0,
        };
        let tree = build_tree_with_frame(attrs, frame);
        let commands = render_tree(&tree);

        assert!(commands.contains(&DrawCmd::RoundedRect(
            93.0,
            13.0,
            7.0,
            24.0,
            3.5,
            SCROLLBAR_COLOR,
        )));
    }

    #[test]
    fn test_render_nearby_behind_and_in_front_order() {
        let behind = encode_nearby_el(20.0, 10.0, (255, 0, 0));
        let in_front = encode_nearby_el(20.0, 10.0, (0, 0, 255));

        let mut attrs = Attrs::default();
        attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
        attrs.behind = Some(behind);
        attrs.in_front = Some(in_front);

        let tree = build_tree_with_frame(
            attrs,
            Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
                content_width: 100.0,
                content_height: 50.0,
            },
        );
        let commands = render_tree(&tree);

        assert_eq!(
            commands,
            vec![
                DrawCmd::Rect(40.0, 20.0, 20.0, 10.0, 0xFF0000FF),
                DrawCmd::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF),
                DrawCmd::Rect(40.0, 20.0, 20.0, 10.0, 0x0000FFFF),
            ]
        );
    }

    #[test]
    fn test_render_nearby_above_below_order_after_parent() {
        let above = encode_nearby_el(20.0, 10.0, (0, 255, 0));
        let below = encode_nearby_el(20.0, 10.0, (255, 255, 0));

        let mut attrs = Attrs::default();
        attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
        attrs.above = Some(above);
        attrs.below = Some(below);

        let tree = build_tree_with_frame(
            attrs,
            Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
                content_width: 100.0,
                content_height: 50.0,
            },
        );
        let commands = render_tree(&tree);

        assert_eq!(
            commands,
            vec![
                DrawCmd::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF),
                DrawCmd::Rect(40.0, -10.0, 20.0, 10.0, 0x00FF00FF),
                DrawCmd::Rect(40.0, 50.0, 20.0, 10.0, 0xFFFF00FF),
            ]
        );
    }

    #[test]
    fn test_clip_uses_padded_content_box() {
        let mut attrs = Attrs::default();
        attrs.clip_x = Some(true);
        attrs.clip_y = Some(true);
        attrs.padding = Some(Padding::Uniform(10.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        assert_eq!(
            overflow_clip(frame, &attrs),
            Some(OverflowClip::Rect(10.0, 10.0, 80.0, 30.0))
        );
    }

    #[test]
    fn test_clip_x_uses_padded_x_only() {
        let mut attrs = Attrs::default();
        attrs.clip_x = Some(true);
        attrs.padding = Some(Padding::Uniform(10.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        assert_eq!(
            overflow_clip(frame, &attrs),
            Some(OverflowClip::Rect(10.0, 0.0, 80.0, 50.0))
        );
    }

    #[test]
    fn test_scrollbar_x_clips_x() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.padding = Some(Padding::Uniform(10.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        assert_eq!(
            overflow_clip(frame, &attrs),
            Some(OverflowClip::Rect(10.0, 0.0, 80.0, 50.0))
        );
    }

    #[test]
    fn test_no_clip_no_pushclip() {
        let attrs = Attrs::default();
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        assert_eq!(overflow_clip(frame, &attrs), None);
    }

    #[test]
    fn test_clip_with_border_radius_uses_rounded_clip() {
        let mut attrs = Attrs::default();
        attrs.clip_x = Some(true);
        attrs.clip_y = Some(true);
        attrs.border_radius = Some(BorderRadius::Uniform(10.0));
        attrs.border_width = Some(BorderWidth::Uniform(2.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        // Inner radius = 10 - 2 = 8, clip rect inset by border (2 each side)
        assert_eq!(
            overflow_clip(frame, &attrs),
            Some(OverflowClip::Rounded(2.0, 2.0, 96.0, 46.0, 8.0))
        );
    }

    #[test]
    fn test_clip_with_border_radius_corners_uses_rounded_corners_clip() {
        let mut attrs = Attrs::default();
        attrs.clip_x = Some(true);
        attrs.clip_y = Some(true);
        attrs.border_radius = Some(BorderRadius::Corners {
            tl: 12.0,
            tr: 8.0,
            br: 4.0,
            bl: 16.0,
        });
        attrs.border_width = Some(BorderWidth::Uniform(3.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        // Inner radii = outer - 3 (border), clamped to 0
        assert_eq!(
            overflow_clip(frame, &attrs),
            Some(OverflowClip::RoundedCorners(
                3.0, 3.0, 94.0, 44.0, 9.0, 5.0, 1.0, 13.0
            ))
        );
    }

    #[test]
    fn test_clip_with_border_radius_falls_back_to_rect_when_radius_consumed() {
        let mut attrs = Attrs::default();
        attrs.clip_x = Some(true);
        attrs.clip_y = Some(true);
        attrs.border_radius = Some(BorderRadius::Uniform(3.0));
        attrs.border_width = Some(BorderWidth::Uniform(5.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        // Inner radius = 3 - 5 = 0 (clamped), so falls back to rect
        assert_eq!(
            overflow_clip(frame, &attrs),
            Some(OverflowClip::Rect(5.0, 5.0, 90.0, 40.0))
        );
    }

    #[test]
    fn test_clip_with_border_radius_no_border_width() {
        let mut attrs = Attrs::default();
        attrs.clip_x = Some(true);
        attrs.clip_y = Some(true);
        attrs.border_radius = Some(BorderRadius::Uniform(8.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        // No border width, so inner radius = outer radius = 8
        assert_eq!(
            overflow_clip(frame, &attrs),
            Some(OverflowClip::Rounded(0.0, 0.0, 100.0, 50.0, 8.0))
        );
    }

    // =========================================================================
    // Paragraph Render Tests
    // =========================================================================

    fn build_paragraph_tree(mut attrs: Attrs, frame: Frame) -> ElementTree {
        let id = ElementId::from_term_bytes(vec![10]);
        attrs.background = attrs.background.take();
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::Paragraph, Vec::new(), attrs);
        element.frame = Some(frame);

        let mut tree = ElementTree::new();
        tree.root = Some(id);
        tree.insert(element);
        tree
    }

    // =========================================================================
    // Border Feature Tests
    // =========================================================================

    #[test]
    fn test_render_border_uniform_emits_border_cmd() {
        let mut attrs = Attrs::default();
        attrs.border_width = Some(BorderWidth::Uniform(2.0));
        attrs.border_color = Some(Color::Named("red".to_string()));
        attrs.border_radius = Some(BorderRadius::Uniform(4.0));

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        assert!(commands.iter().any(|cmd| matches!(
            cmd,
            DrawCmd::Border(_, _, _, _, 4.0, 2.0, 0xFF0000FF, BorderStyle::Solid)
        )));
    }

    #[test]
    fn test_render_border_edges_emits_border_edges_cmd() {
        let mut attrs = Attrs::default();
        attrs.border_width = Some(BorderWidth::Sides {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 4.0,
        });
        attrs.border_color = Some(Color::Named("red".to_string()));

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        assert!(commands.iter().any(|cmd| matches!(
            cmd,
            DrawCmd::BorderEdges(
                _,
                _,
                _,
                _,
                _,
                1.0,
                2.0,
                3.0,
                4.0,
                0xFF0000FF,
                BorderStyle::Solid
            )
        )));
    }

    #[test]
    fn test_render_border_dashed_passes_style() {
        let mut attrs = Attrs::default();
        attrs.border_width = Some(BorderWidth::Uniform(2.0));
        attrs.border_style = Some(BorderStyle::Dashed);
        attrs.border_color = Some(Color::Named("white".to_string()));

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        assert!(commands.iter().any(|cmd| matches!(
            cmd,
            DrawCmd::Border(_, _, _, _, _, 2.0, 0xFFFFFFFF, BorderStyle::Dashed)
        )));
    }

    #[test]
    fn test_render_shadow_emits_before_background() {
        let mut attrs = Attrs::default();
        attrs.background = Some(Background::Color(Color::Named("white".to_string())));
        attrs.box_shadows = Some(vec![super::super::attrs::BoxShadow {
            offset_x: 2.0,
            offset_y: 2.0,
            blur: 8.0,
            size: 4.0,
            color: Color::Named("black".to_string()),
            inset: false,
        }]);

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        let shadow_idx = commands
            .iter()
            .position(|cmd| matches!(cmd, DrawCmd::Shadow(..)))
            .expect("shadow should exist");
        let bg_idx = commands
            .iter()
            .position(|cmd| matches!(cmd, DrawCmd::Rect(..) | DrawCmd::RoundedRect(..)))
            .expect("background should exist");

        assert!(
            shadow_idx < bg_idx,
            "shadow should render before background"
        );
    }

    #[test]
    fn test_render_inset_shadow_emits_after_background() {
        let mut attrs = Attrs::default();
        attrs.background = Some(Background::Color(Color::Named("white".to_string())));
        attrs.box_shadows = Some(vec![super::super::attrs::BoxShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 10.0,
            size: 0.0,
            color: Color::Named("black".to_string()),
            inset: true,
        }]);

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        let bg_idx = commands
            .iter()
            .position(|cmd| matches!(cmd, DrawCmd::Rect(..) | DrawCmd::RoundedRect(..)))
            .expect("background should exist");
        let inset_idx = commands
            .iter()
            .position(|cmd| matches!(cmd, DrawCmd::InsetShadow(..)))
            .expect("inset shadow should exist");

        assert!(
            inset_idx > bg_idx,
            "inset shadow should render after background"
        );
    }

    #[test]
    fn test_render_no_border_without_color() {
        let mut attrs = Attrs::default();
        attrs.border_width = Some(BorderWidth::Uniform(2.0));
        // No border_color set

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        assert!(!commands
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::Border(..) | DrawCmd::BorderEdges(..))));
    }

    #[test]
    fn test_render_paragraph_emits_text_commands() {
        use crate::tree::attrs::TextFragment;

        let mut attrs = Attrs::default();
        attrs.paragraph_fragments = Some(vec![
            TextFragment {
                x: 10.0,
                y: 5.0,
                text: "Hello".to_string(),
                font_size: 16.0,
                color: 0xFFFFFFFF,
                family: "default".to_string(),
                weight: 400,
                italic: false,
                underline: false,
                strike: false,
                ascent: 12.0,
            },
            TextFragment {
                x: 60.0,
                y: 5.0,
                text: "World".to_string(),
                font_size: 16.0,
                color: 0xFF0000FF,
                family: "default".to_string(),
                weight: 700,
                italic: false,
                underline: false,
                strike: false,
                ascent: 12.0,
            },
        ]);

        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 30.0,
            content_width: 200.0,
            content_height: 30.0,
        };

        let tree = build_paragraph_tree(attrs, frame);
        let commands = render_tree(&tree);

        // Should produce TextWithFont commands for each fragment
        let text_cmds: Vec<(f32, f32, String, u32, u16)> = commands
            .iter()
            .filter_map(|cmd| match cmd {
                DrawCmd::TextWithFont(x, y, text, _size, color, _family, weight, _italic) => {
                    Some((*x, *y, text.clone(), *color, *weight))
                }
                _ => None,
            })
            .collect();

        assert_eq!(text_cmds.len(), 2);
        // First fragment: x=10, baseline_y = 5 + 12 = 17
        assert_eq!(text_cmds[0].0, 10.0);
        assert_eq!(text_cmds[0].1, 17.0);
        assert_eq!(text_cmds[0].2, "Hello");
        assert_eq!(text_cmds[0].3, 0xFFFFFFFF);
        assert_eq!(text_cmds[0].4, 400);

        // Second fragment: x=60, baseline_y = 5 + 12 = 17
        assert_eq!(text_cmds[1].0, 60.0);
        assert_eq!(text_cmds[1].1, 17.0);
        assert_eq!(text_cmds[1].2, "World");
        assert_eq!(text_cmds[1].3, 0xFF0000FF);
        assert_eq!(text_cmds[1].4, 700);
    }

    #[test]
    fn test_render_paragraph_renders_float_child_and_fragments() {
        use crate::tree::attrs::{AlignX, TextFragment};

        let para_id = ElementId::from_term_bytes(vec![10]);
        let float_id = ElementId::from_term_bytes(vec![11]);

        let mut para_attrs = Attrs::default();
        para_attrs.paragraph_fragments = Some(vec![TextFragment {
            x: 24.0,
            y: 8.0,
            text: "AA".to_string(),
            font_size: 16.0,
            color: 0xFFFFFFFF,
            family: "default".to_string(),
            weight: 400,
            italic: false,
            underline: false,
            strike: false,
            ascent: 12.0,
        }]);

        let mut paragraph = Element::with_attrs(
            para_id.clone(),
            ElementKind::Paragraph,
            Vec::new(),
            para_attrs,
        );
        paragraph.children = vec![float_id.clone()];
        paragraph.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 40.0,
            content_width: 120.0,
            content_height: 40.0,
        });

        let mut float_attrs = Attrs::default();
        float_attrs.align_x = Some(AlignX::Left);
        float_attrs.background = Some(Background::Color(Color::Rgb { r: 255, g: 0, b: 0 }));
        let mut float_el =
            Element::with_attrs(float_id.clone(), ElementKind::El, Vec::new(), float_attrs);
        float_el.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 20.0,
            content_width: 20.0,
            content_height: 20.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(para_id.clone());
        tree.insert(paragraph);
        tree.insert(float_el);

        let commands = render_tree(&tree);

        assert!(commands.iter().any(|cmd| {
            matches!(cmd, DrawCmd::Rect(x, y, w, h, color) if *x == 0.0 && *y == 0.0 && *w == 20.0 && *h == 20.0 && *color == 0xFF0000FF)
        }));
        assert!(commands.iter().any(|cmd| {
            matches!(cmd, DrawCmd::TextWithFont(x, y, text, _, _, _, _, _) if *x == 24.0 && *y == 20.0 && text == "AA")
        }));
    }

    #[test]
    fn test_render_paragraph_underline_and_strike() {
        use crate::tree::attrs::TextFragment;

        let mut attrs = Attrs::default();
        attrs.paragraph_fragments = Some(vec![TextFragment {
            x: 10.0,
            y: 5.0,
            text: "Decorated".to_string(),
            font_size: 18.0,
            color: 0x010203FF,
            family: "default".to_string(),
            weight: 400,
            italic: false,
            underline: true,
            strike: true,
            ascent: 14.0,
        }]);

        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 30.0,
            content_width: 200.0,
            content_height: 30.0,
        };

        let tree = build_paragraph_tree(attrs, frame);
        let commands = render_tree(&tree);

        // Should have text command + 2 decoration rects (underline + strike)
        let text_count = commands
            .iter()
            .filter(|cmd| matches!(cmd, DrawCmd::TextWithFont(..)))
            .count();
        assert_eq!(text_count, 1);

        let decoration_rects: Vec<_> = commands
            .iter()
            .filter(|cmd| matches!(cmd, DrawCmd::Rect(_, _, _, _, color) if *color == 0x010203FF))
            .collect();
        assert_eq!(decoration_rects.len(), 2);
    }

    #[test]
    fn test_render_paragraph_no_fragments() {
        let mut attrs = Attrs::default();
        attrs.paragraph_fragments = Some(vec![]);

        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 30.0,
            content_width: 200.0,
            content_height: 30.0,
        };

        let tree = build_paragraph_tree(attrs, frame);
        let commands = render_tree(&tree);

        // No text or rect commands should be emitted
        let text_count = commands
            .iter()
            .filter(|cmd| matches!(cmd, DrawCmd::TextWithFont(..)))
            .count();
        assert_eq!(text_count, 0);
    }

    #[test]
    fn test_render_paragraph_with_background() {
        use crate::tree::attrs::TextFragment;

        let mut attrs = Attrs::default();
        attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 128 }));
        attrs.paragraph_fragments = Some(vec![TextFragment {
            x: 0.0,
            y: 0.0,
            text: "Hi".to_string(),
            font_size: 16.0,
            color: 0xFFFFFFFF,
            family: "default".to_string(),
            weight: 400,
            italic: false,
            underline: false,
            strike: false,
            ascent: 12.0,
        }]);

        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 20.0,
            content_width: 100.0,
            content_height: 20.0,
        };

        let tree = build_paragraph_tree(attrs, frame);
        let commands = render_tree(&tree);

        // Background should be rendered before text
        let bg_idx = commands
            .iter()
            .position(|cmd| matches!(cmd, DrawCmd::Rect(_, _, 100.0, 20.0, 0x000080FF)))
            .expect("background rect should exist");
        let text_idx = commands
            .iter()
            .position(|cmd| matches!(cmd, DrawCmd::TextWithFont(..)))
            .expect("text command should exist");

        assert!(bg_idx < text_idx, "background should render before text");
    }

    #[test]
    fn test_render_gradient_with_rounded_corners_emits_radius() {
        let mut attrs = Attrs::default();
        attrs.background = Some(Background::Gradient {
            from: Color::Rgb {
                r: 67,
                g: 97,
                b: 238,
            },
            to: Color::Rgb {
                r: 114,
                g: 9,
                b: 183,
            },
            angle: 135.0,
        });
        attrs.border_radius = Some(BorderRadius::Uniform(10.0));

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        let gradient = commands
            .iter()
            .find(|cmd| matches!(cmd, DrawCmd::Gradient(..)))
            .expect("gradient command should exist");

        match gradient {
            DrawCmd::Gradient(_, _, _, _, _, _, _, radius) => {
                assert_eq!(*radius, 10.0, "gradient should carry the border radius");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_render_gradient_without_radius_emits_zero() {
        let mut attrs = Attrs::default();
        attrs.background = Some(Background::Gradient {
            from: Color::Rgb { r: 0, g: 0, b: 0 },
            to: Color::Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            angle: 90.0,
        });
        // No border_radius set

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        let gradient = commands
            .iter()
            .find(|cmd| matches!(cmd, DrawCmd::Gradient(..)))
            .expect("gradient command should exist");

        match gradient {
            DrawCmd::Gradient(_, _, _, _, _, _, _, radius) => {
                assert_eq!(
                    *radius, 0.0,
                    "gradient without border_radius should have radius 0"
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_render_gradient_with_per_corner_radius_emits_zero() {
        let mut attrs = Attrs::default();
        attrs.background = Some(Background::Gradient {
            from: Color::Rgb { r: 0, g: 0, b: 0 },
            to: Color::Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            angle: 0.0,
        });
        attrs.border_radius = Some(BorderRadius::Corners {
            tl: 10.0,
            tr: 5.0,
            br: 10.0,
            bl: 5.0,
        });

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        let gradient = commands
            .iter()
            .find(|cmd| matches!(cmd, DrawCmd::Gradient(..)))
            .expect("gradient command should exist");

        // Per-corner radius falls back to 0 via border_radius_uniform
        match gradient {
            DrawCmd::Gradient(_, _, _, _, _, _, _, radius) => {
                assert_eq!(
                    *radius, 0.0,
                    "per-corner radius should fall back to 0 for gradient"
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_render_border_edges_asymmetric_widths() {
        // Regression: thick top/bottom, thin sides should emit correct per-edge widths
        let mut attrs = Attrs::default();
        attrs.border_width = Some(BorderWidth::Sides {
            top: 4.0,
            right: 1.0,
            bottom: 4.0,
            left: 1.0,
        });
        attrs.border_color = Some(Color::Rgb {
            r: 120,
            g: 200,
            b: 160,
        });
        attrs.border_radius = Some(BorderRadius::Uniform(8.0));

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        let edges_cmd = commands
            .iter()
            .find(|cmd| matches!(cmd, DrawCmd::BorderEdges(..)))
            .expect("BorderEdges command should exist");

        match edges_cmd {
            DrawCmd::BorderEdges(_, _, _, _, radius, top, right, bottom, left, _, _) => {
                assert_eq!(*top, 4.0);
                assert_eq!(*right, 1.0);
                assert_eq!(*bottom, 4.0);
                assert_eq!(*left, 1.0);
                assert_eq!(*radius, 8.0, "border radius should be passed through");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_render_border_edges_bottom_only() {
        // Regression: bottom-only border should emit BorderEdges with zero for other sides
        let mut attrs = Attrs::default();
        attrs.border_width = Some(BorderWidth::Sides {
            top: 0.0,
            right: 0.0,
            bottom: 3.0,
            left: 0.0,
        });
        attrs.border_color = Some(Color::Rgb {
            r: 200,
            g: 180,
            b: 100,
        });

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        let edges_cmd = commands
            .iter()
            .find(|cmd| matches!(cmd, DrawCmd::BorderEdges(..)))
            .expect("BorderEdges command should exist for bottom-only border");

        match edges_cmd {
            DrawCmd::BorderEdges(_, _, _, _, radius, top, right, bottom, left, _, _) => {
                assert_eq!(*top, 0.0);
                assert_eq!(*right, 0.0);
                assert_eq!(*bottom, 3.0);
                assert_eq!(*left, 0.0);
                assert_eq!(*radius, 0.0, "no border radius set");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_render_border_edges_with_style() {
        // Per-edge borders should forward the border style
        let mut attrs = Attrs::default();
        attrs.border_width = Some(BorderWidth::Sides {
            top: 2.0,
            right: 2.0,
            bottom: 2.0,
            left: 2.0,
        });
        attrs.border_color = Some(Color::Named("white".to_string()));
        attrs.border_style = Some(BorderStyle::Dashed);

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        assert!(commands.iter().any(|cmd| matches!(
            cmd,
            DrawCmd::BorderEdges(
                _,
                _,
                _,
                _,
                _,
                2.0,
                2.0,
                2.0,
                2.0,
                0xFFFFFFFF,
                BorderStyle::Dashed
            )
        )));
    }

    #[test]
    fn test_border_renders_between_inner_and_outer_clip() {
        // When border_radius + border_width + overflow clip are all present,
        // the command sequence should be:
        //   outer PushClipRounded → inner PushClipRoundedHard → PopClip(inner) → Border → PopClip(outer)
        let mut attrs = Attrs::default();
        attrs.clip_x = Some(true);
        attrs.clip_y = Some(true);
        attrs.border_width = Some(BorderWidth::Uniform(2.0));
        attrs.border_color = Some(Color::Named("red".to_string()));
        attrs.border_radius = Some(BorderRadius::Uniform(8.0));

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        // Find all relevant command indices
        let clip_pushes: Vec<usize> = commands
            .iter()
            .enumerate()
            .filter(|(_, cmd)| {
                matches!(
                    cmd,
                    DrawCmd::PushClipRounded(..)
                        | DrawCmd::PushClipRoundedCorners(..)
                        | DrawCmd::PushClipRoundedHard(..)
                        | DrawCmd::PushClipRoundedCornersHard(..)
                )
            })
            .map(|(i, _)| i)
            .collect();
        let clip_pops: Vec<usize> = commands
            .iter()
            .enumerate()
            .filter(|(_, cmd)| matches!(cmd, DrawCmd::PopClip))
            .map(|(i, _)| i)
            .collect();
        let border_idx = commands
            .iter()
            .position(|cmd| matches!(cmd, DrawCmd::Border(..)))
            .expect("Border should exist");

        // Should have exactly 2 clip pushes (outer + inner) and 2 pops
        assert_eq!(clip_pushes.len(), 2, "expected outer + inner clip pushes");
        assert_eq!(clip_pops.len(), 2, "expected inner + outer clip pops");

        // Outer clip is the element bounds with outer radius (AA on)
        match &commands[clip_pushes[0]] {
            DrawCmd::PushClipRounded(x, y, w, h, r) => {
                assert_eq!((*x, *y, *w, *h, *r), (0.0, 0.0, 100.0, 50.0, 8.0));
            }
            _ => panic!("outer clip should be PushClipRounded"),
        }

        // Inner clip is content bounds with inner radius (8 - 2 = 6), hard clipped
        match &commands[clip_pushes[1]] {
            DrawCmd::PushClipRoundedHard(x, y, w, h, r) => {
                assert_eq!((*x, *y, *w, *h, *r), (2.0, 2.0, 96.0, 46.0, 6.0));
            }
            _ => panic!("inner clip should be PushClipRoundedHard"),
        }

        // Border must be between inner pop and outer pop
        assert!(
            border_idx > clip_pops[0],
            "border ({border_idx}) must render after inner clip pop ({})",
            clip_pops[0]
        );
        assert!(
            border_idx < clip_pops[1],
            "border ({border_idx}) must render before outer clip pop ({})",
            clip_pops[1]
        );
    }

    #[test]
    fn test_no_compositing_clip_without_border_width() {
        // Element with border_radius but no border_width → needs_compositing_clip is false
        let mut attrs = Attrs::default();
        attrs.clip_x = Some(true);
        attrs.clip_y = Some(true);
        attrs.border_radius = Some(BorderRadius::Uniform(8.0));
        // No border_width

        assert!(
            !needs_compositing_clip(&attrs),
            "should not need compositing clip without border_width"
        );

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        // First rounded clip should be the inner clip (at element bounds since no border inset),
        // NOT an outer compositing clip followed by an inner clip.
        let first_clip = commands
            .iter()
            .find(|cmd| {
                matches!(
                    cmd,
                    DrawCmd::PushClipRounded(..) | DrawCmd::PushClipRoundedCorners(..)
                )
            })
            .expect("should have at least one rounded clip");

        match first_clip {
            DrawCmd::PushClipRounded(x, y, w, h, r) => {
                // Inner clip with no border inset = element bounds, radius 8
                assert_eq!((*x, *y, *w, *h, *r), (0.0, 0.0, 100.0, 50.0, 8.0));
            }
            _ => panic!("expected PushClipRounded"),
        }
    }

    #[test]
    fn test_no_compositing_clip_without_border_radius() {
        // Element with border_width but no border_radius → no outer clip pushed
        let mut attrs = Attrs::default();
        attrs.clip_x = Some(true);
        attrs.clip_y = Some(true);
        attrs.border_width = Some(BorderWidth::Uniform(2.0));
        attrs.border_color = Some(Color::Named("red".to_string()));
        // No border_radius

        let tree = build_tree_with_attrs(attrs);
        let commands = render_tree(&tree);

        let clip_push_count = commands
            .iter()
            .filter(|cmd| {
                matches!(
                    cmd,
                    DrawCmd::PushClip(..)
                        | DrawCmd::PushClipRounded(..)
                        | DrawCmd::PushClipRoundedCorners(..)
                )
            })
            .count();

        // Only the inner clip (Rect type since no radius), no outer compositing clip
        assert_eq!(
            clip_push_count, 1,
            "should have only inner clip, no outer compositing clip"
        );
    }
}
