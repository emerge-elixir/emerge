//! Render an ElementTree into DrawCmds.
//!
//! Reads from pre-scaled attrs (scaling is applied in the layout pass).

use super::attrs::{Background, BorderRadius, Color, Padding, TextAlign};
use super::deserialize::decode_tree;
use super::element::{ElementId, ElementKind, ElementTree, Frame};
use super::layout::{layout_tree, Constraint, SkiaTextMeasurer};
use crate::renderer::DrawCmd;

const SCROLLBAR_THICKNESS: f32 = 6.0;
const SCROLLBAR_MIN_LENGTH: f32 = 24.0;
const SCROLLBAR_COLOR: u32 = 0xD0D5DC99;

/// Render the tree to draw commands.
/// Reads from pre-scaled attrs (layout pass must run first).
pub fn render_tree(tree: &ElementTree) -> Vec<DrawCmd> {
    let Some(root) = tree.root.as_ref() else {
        return Vec::new();
    };

    let mut commands = Vec::new();
    render_element(tree, root, &mut commands);
    commands
}

fn render_element(tree: &ElementTree, id: &ElementId, commands: &mut Vec<DrawCmd>) {
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

    let transform_state = push_element_transform(commands, frame, attrs);

    // Render "behind" elements first (underlay)
    if let Some(behind_bytes) = &attrs.behind {
        render_nearby_element(behind_bytes, frame, NearbyPosition::Behind, commands);
    }

    if let Some(background) = &attrs.background {
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
                ));
            }
        }
    }

    if let (Some(border_width), Some(border_color)) = (attrs.border_width, &attrs.border_color)
        && border_width > 0.0
    {
        push_border_rect(
            commands,
            frame,
            radius,
            border_width as f32,
            color_to_u32(border_color),
        );
    }

    let clip_rect = overflow_clip_rect(frame, attrs);
    if let Some((x, y, w, h)) = clip_rect {
        commands.push(DrawCmd::PushClip(x, y, w, h));
    }

    if element.kind == ElementKind::Text
        && let Some(content) = attrs.content.as_deref()
    {
        let font_size = attrs.font_size.unwrap_or(16.0) as f32;
        let color = attrs.font_color.as_ref().map(color_to_u32).unwrap_or(0xFFFFFFFF);
        let (padding_left, padding_top) = text_padding(attrs.padding.as_ref());
        let (padding_right, _padding_bottom) = match attrs.padding.as_ref() {
            Some(Padding::Uniform(v)) => (*v as f32, *v as f32),
            Some(Padding::Sides { right, bottom, .. }) => (*right as f32, *bottom as f32),
            None => (0.0, 0.0),
        };
        let (ascent, _) = text_metrics(font_size);
        let text_width = measure_text_width(content, font_size);
        let content_width = frame.width - padding_left - padding_right;
        let text_align = attrs.text_align.unwrap_or_default();
        let text_x = match text_align {
            TextAlign::Left => frame.x + padding_left,
            TextAlign::Center => frame.x + padding_left + (content_width - text_width) / 2.0,
            TextAlign::Right => frame.x + frame.width - padding_right - text_width,
        };
        let baseline_y = frame.y + padding_top + ascent;
        commands.push(DrawCmd::Text(text_x, baseline_y, content.to_string(), font_size, color));
    }

    for child_id in &element.children {
        render_element(tree, child_id, commands);
    }

    if clip_rect.is_some() {
        commands.push(DrawCmd::PopClip);
    }

    push_scrollbar_thumbs(commands, frame, attrs);

    // Render nearby positioned elements (above, below, on_left, on_right)
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

    // Render "in_front" elements last (overlay)
    if let Some(in_front_bytes) = &attrs.in_front {
        render_nearby_element(in_front_bytes, frame, NearbyPosition::InFront, commands);
    }

    pop_element_transform(commands, transform_state);
}

fn color_to_u32(color: &Color) -> u32 {
    match color {
        Color::Rgb { r, g, b } => ((*r as u32) << 24) | ((*g as u32) << 16) | ((*b as u32) << 8) | 0xFF,
        Color::Rgba { r, g, b, a } => ((*r as u32) << 24) | ((*g as u32) << 16) | ((*b as u32) << 8) | (*a as u32),
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

fn overflow_clip_rect(frame: Frame, attrs: &super::attrs::Attrs) -> Option<(f32, f32, f32, f32)> {
    let clip_all = attrs.clip.unwrap_or(false);
    let clip_x = attrs.clip_x.unwrap_or(false) || attrs.scrollbar_x.unwrap_or(false);
    let clip_y = attrs.clip_y.unwrap_or(false) || attrs.scrollbar_y.unwrap_or(false);

    if !(clip_all || clip_x || clip_y) {
        return None;
    }

    let (padding_left, padding_top, padding_right, padding_bottom) = match attrs.padding.as_ref() {
        Some(Padding::Uniform(value)) => (*value as f32, *value as f32, *value as f32, *value as f32),
        Some(Padding::Sides {
            top,
            right,
            bottom,
            left,
        }) => (*left as f32, *top as f32, *right as f32, *bottom as f32),
        None => (0.0, 0.0, 0.0, 0.0),
    };

    let content_x = frame.x + padding_left;
    let content_y = frame.y + padding_top;
    let content_width = frame.width - padding_left - padding_right;
    let content_height = frame.height - padding_top - padding_bottom;

    let (x, width) = if clip_all || clip_x {
        (content_x, content_width)
    } else {
        (frame.x, frame.width)
    };
    let (y, height) = if clip_all || clip_y {
        (content_y, content_height)
    } else {
        (frame.y, frame.height)
    };

    Some((x, y, width.max(0.0), height.max(0.0)))
}

fn push_scrollbar_thumbs(commands: &mut Vec<DrawCmd>, frame: Frame, attrs: &super::attrs::Attrs) {
    let thickness = SCROLLBAR_THICKNESS;
    let radius = thickness / 2.0;

    if attrs.scrollbar_y.unwrap_or(false) {
        let viewport = frame.height;
        let content = frame.content_height;
        if content > viewport && viewport > 0.0 {
            let thumb_len = (viewport * viewport / content)
                .max(SCROLLBAR_MIN_LENGTH)
                .min(viewport);
            let scroll_offset = attrs.scroll_y.unwrap_or(0.0) as f32;
            let scroll_range = (content - viewport).max(0.0);
            let ratio = if scroll_range > 0.0 {
                (scroll_offset / scroll_range).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let track_len = (viewport - thumb_len).max(0.0);
            let thumb_y = frame.y + ratio * track_len;
            let x = frame.x + frame.width - thickness;
            commands.push(DrawCmd::RoundedRect(
                x,
                thumb_y,
                thickness,
                thumb_len,
                radius,
                SCROLLBAR_COLOR,
            ));
        }
    }

    if attrs.scrollbar_x.unwrap_or(false) {
        let viewport = frame.width;
        let content = frame.content_width;
        if content > viewport && viewport > 0.0 {
            let thumb_len = (viewport * viewport / content)
                .max(SCROLLBAR_MIN_LENGTH)
                .min(viewport);
            let scroll_offset = attrs.scroll_x.unwrap_or(0.0) as f32;
            let scroll_range = (content - viewport).max(0.0);
            let ratio = if scroll_range > 0.0 {
                (scroll_offset / scroll_range).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let track_len = (viewport - thumb_len).max(0.0);
            let thumb_x = frame.x + ratio * track_len;
            let y = frame.y + frame.height - thickness;
            commands.push(DrawCmd::RoundedRect(
                thumb_x,
                y,
                thumb_len,
                thickness,
                radius,
                SCROLLBAR_COLOR,
            ));
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct TransformState {
    active: bool,
    has_alpha_layer: bool,
}

fn push_element_transform(commands: &mut Vec<DrawCmd>, frame: Frame, attrs: &super::attrs::Attrs) -> TransformState {
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
            ));
        }
    }
}

/// Get text padding from pre-scaled attrs.
fn text_padding(padding: Option<&Padding>) -> (f32, f32) {
    match padding {
        Some(Padding::Uniform(value)) => (*value as f32, *value as f32),
        Some(Padding::Sides { top, left, .. }) => (*left as f32, *top as f32),
        None => (0.0, 0.0),
    }
}

fn text_metrics(font_size: f32) -> (f32, f32) {
    use crate::renderer::get_default_typeface;
    use skia_safe::Font;

    let typeface = get_default_typeface();
    let font = Font::new(typeface, font_size);
    let (_, metrics) = font.metrics();
    (metrics.ascent.abs(), metrics.descent)
}

fn measure_text_width(text: &str, font_size: f32) -> f32 {
    use crate::renderer::get_default_typeface;
    use skia_safe::Font;

    let typeface = get_default_typeface();
    let font = Font::new(typeface, font_size);
    let (width, _bounds) = font.measure_str(text, None);
    width
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
    render_tree_recursive(&nearby_tree, &root_id, commands);
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

/// Recursively render a tree (used for nearby elements).
fn render_tree_recursive(tree: &ElementTree, id: &ElementId, commands: &mut Vec<DrawCmd>) {
    let Some(element) = tree.get(id) else {
        return;
    };

    let frame = match element.frame {
        Some(frame) => frame,
        None => return,
    };

    let attrs = &element.attrs;
    let radius = attrs.border_radius.as_ref();

    let transform_state = push_element_transform(commands, frame, attrs);

    // Render behind first
    if let Some(behind_bytes) = &attrs.behind {
        render_nearby_element(behind_bytes, frame, NearbyPosition::Behind, commands);
    }

    if let Some(background) = &attrs.background {
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
                ));
            }
        }
    }

    if let (Some(border_width), Some(border_color)) = (attrs.border_width, &attrs.border_color)
        && border_width > 0.0
    {
        push_border_rect(
            commands,
            frame,
            radius,
            border_width as f32,
            color_to_u32(border_color),
        );
    }

    let clip_rect = overflow_clip_rect(frame, attrs);
    if let Some((x, y, w, h)) = clip_rect {
        commands.push(DrawCmd::PushClip(x, y, w, h));
    }

    if element.kind == ElementKind::Text
        && let Some(content) = attrs.content.as_deref()
    {
        let font_size = attrs.font_size.unwrap_or(16.0) as f32;
        let color = attrs
            .font_color
            .as_ref()
            .map(color_to_u32)
            .unwrap_or(0xFFFFFFFF);
        let (padding_left, padding_top) = text_padding(attrs.padding.as_ref());
        let (padding_right, _padding_bottom) = match attrs.padding.as_ref() {
            Some(Padding::Uniform(v)) => (*v as f32, *v as f32),
            Some(Padding::Sides { right, bottom, .. }) => (*right as f32, *bottom as f32),
            None => (0.0, 0.0),
        };
        let (ascent, _) = text_metrics(font_size);
        let text_width = measure_text_width(content, font_size);
        let content_width = frame.width - padding_left - padding_right;
        let text_align = attrs.text_align.unwrap_or_default();
        let text_x = match text_align {
            TextAlign::Left => frame.x + padding_left,
            TextAlign::Center => frame.x + padding_left + (content_width - text_width) / 2.0,
            TextAlign::Right => frame.x + frame.width - padding_right - text_width,
        };
        let baseline_y = frame.y + padding_top + ascent;
        commands.push(DrawCmd::Text(
            text_x,
            baseline_y,
            content.to_string(),
            font_size,
            color,
        ));
    }

    for child_id in &element.children {
        render_tree_recursive(tree, child_id, commands);
    }

    if clip_rect.is_some() {
        commands.push(DrawCmd::PopClip);
    }

    push_scrollbar_thumbs(commands, frame, attrs);

    // Render nearby positioned elements
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

    // Render in_front last
    if let Some(in_front_bytes) = &attrs.in_front {
        render_nearby_element(in_front_bytes, frame, NearbyPosition::InFront, commands);
    }

    pop_element_transform(commands, transform_state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::Attrs;
    use crate::tree::element::Element;

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
        let rgb = Color::Rgb { r: 255, g: 128, b: 64 };
        assert_eq!(color_to_u32(&rgb), 0xFF8040FF);

        // Test RGBA color
        let rgba = Color::Rgba { r: 255, g: 128, b: 64, a: 200 };
        assert_eq!(color_to_u32(&rgba), 0xFF8040C8);

        // Test named color
        let named = Color::Named("cyan".to_string());
        assert_eq!(color_to_u32(&named), 0x00FFFFFF);
    }

    #[test]
    fn test_nearby_position_calculations() {
        // Test Above: should be centered horizontally, positioned above
        let parent = Frame { x: 100.0, y: 100.0, width: 200.0, height: 50.0, content_width: 200.0, content_height: 50.0 };
        let nearby = Frame { x: 0.0, y: 0.0, width: 50.0, height: 20.0, content_width: 50.0, content_height: 20.0 };

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

        assert_eq!(commands, vec![DrawCmd::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)]);
    }

    #[test]
    fn test_render_scrollbar_y_thumb() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(50.0);
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
                DrawCmd::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF),
                DrawCmd::PushClip(0.0, 0.0, 100.0, 50.0),
                DrawCmd::PopClip,
                DrawCmd::RoundedRect(94.0, 13.0, 6.0, 24.0, 3.0, SCROLLBAR_COLOR),
            ]
        );
    }

    #[test]
    fn test_render_scrollbar_x_thumb() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scroll_x = Some(30.0);
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
                DrawCmd::Rect(0.0, 0.0, 80.0, 40.0, 0x000000FF),
                DrawCmd::PushClip(0.0, 0.0, 80.0, 40.0),
                DrawCmd::PopClip,
                DrawCmd::RoundedRect(15.0, 34.0, 40.0, 6.0, 3.0, SCROLLBAR_COLOR),
            ]
        );
    }

    #[test]
    fn test_clip_uses_padded_content_box() {
        let mut attrs = Attrs::default();
        attrs.clip = Some(true);
        attrs.padding = Some(Padding::Uniform(10.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        assert_eq!(overflow_clip_rect(frame, &attrs), Some((10.0, 10.0, 80.0, 30.0)));
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

        assert_eq!(overflow_clip_rect(frame, &attrs), Some((10.0, 0.0, 80.0, 50.0)));
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

        assert_eq!(overflow_clip_rect(frame, &attrs), Some((10.0, 0.0, 80.0, 50.0)));
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

        assert_eq!(overflow_clip_rect(frame, &attrs), None);
    }
}
