use super::*;
use crate::render_scene::{DrawPrimitive, RenderNode, RenderScene};
use crate::renderer::{RenderState, Renderer};
use crate::tree::geometry::{ClipShape, CornerRadii};
use crate::tree::transform::Affine2;
use skia_safe::Color as SkColor;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DebugRenderCmd {
    Rect(f32, f32, f32, f32, u32),
    RoundedRect(f32, f32, f32, f32, f32, u32),
    RoundedRectCorners(f32, f32, f32, f32, f32, f32, f32, f32, u32),
    Border(f32, f32, f32, f32, f32, f32, u32, BorderStyle),
    BorderCorners(
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        u32,
        BorderStyle,
    ),
    BorderEdges(
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        u32,
        BorderStyle,
    ),
    Shadow(f32, f32, f32, f32, f32, f32, f32, f32, f32, u32),
    InsetShadow(f32, f32, f32, f32, f32, f32, f32, f32, f32, u32),
    TextWithFont(f32, f32, String, f32, u32, String, u16, bool),
    Gradient(f32, f32, f32, f32, u32, u32, f32, f32),
    Image(f32, f32, f32, f32, String, ImageFit, Option<u32>),
    Video(f32, f32, f32, f32, String, ImageFit),
    ImageLoading(f32, f32, f32, f32),
    ImageFailed(f32, f32, f32, f32),
    PushClip(f32, f32, f32, f32),
    PushClipRounded(f32, f32, f32, f32, f32),
    PushClipRoundedCorners(f32, f32, f32, f32, f32, f32, f32, f32),
    PopClip,
    PushTransform(Affine2),
    PopTransform,
    PushAlpha(f32),
    PopAlpha,
}

pub(super) fn render_output(tree: &ElementTree) -> super::super::RenderOutput {
    super::super::render_tree(tree)
}

pub(super) fn render_tree(tree: &ElementTree) -> Vec<DebugRenderCmd> {
    flatten_scene(&render_output(tree).scene)
}

pub(super) fn flatten_scene(scene: &RenderScene) -> Vec<DebugRenderCmd> {
    let mut commands = Vec::new();
    flatten_nodes(&scene.nodes, &mut commands);
    commands
}

pub(super) fn render_scene_to_pixels(width: u32, height: u32, scene: RenderScene) -> Vec<u8> {
    let info = skia_safe::ImageInfo::new(
        (width as i32, height as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let surface = skia_safe::surfaces::raster(&info, None, None)
        .expect("raster surface should be created for render test");

    let mut renderer = Renderer::from_surface(surface);
    let state = RenderState {
        scene,
        clear_color: SkColor::TRANSPARENT,
        render_version: 1,
        animate: false,
    };
    renderer.render(&state);

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    renderer
        .surface_mut()
        .read_pixels(&info, pixels.as_mut_slice(), (width * 4) as usize, (0, 0));
    pixels
}

pub(super) fn rgba_at(pixels: &[u8], width: u32, x: u32, y: u32) -> (u8, u8, u8, u8) {
    let idx = ((y * width + x) * 4) as usize;
    (
        pixels[idx],
        pixels[idx + 1],
        pixels[idx + 2],
        pixels[idx + 3],
    )
}

fn flatten_nodes(nodes: &[RenderNode], commands: &mut Vec<DebugRenderCmd>) {
    for node in nodes {
        match node {
            RenderNode::Clip { clips, children } => {
                let clip_count = push_debug_clips(clips, commands);
                flatten_nodes(children, commands);
                for _ in 0..clip_count {
                    commands.push(DebugRenderCmd::PopClip);
                }
            }
            RenderNode::Transform {
                transform,
                children,
            } => {
                commands.push(DebugRenderCmd::PushTransform(*transform));
                flatten_nodes(children, commands);
                commands.push(DebugRenderCmd::PopTransform);
            }
            RenderNode::Alpha { alpha, children } => {
                commands.push(DebugRenderCmd::PushAlpha(*alpha));
                flatten_nodes(children, commands);
                commands.push(DebugRenderCmd::PopAlpha);
            }
            RenderNode::Primitive(primitive) => {
                commands.push(debug_cmd_for_primitive(primitive));
            }
        }
    }
}

fn push_debug_clips(clips: &[ClipShape], commands: &mut Vec<DebugRenderCmd>) -> usize {
    for clip in clips {
        match clip.radii {
            None => commands.push(DebugRenderCmd::PushClip(
                clip.rect.x,
                clip.rect.y,
                clip.rect.width,
                clip.rect.height,
            )),
            Some(CornerRadii { tl, tr, br, bl }) if tl == tr && tr == br && br == bl => {
                commands.push(DebugRenderCmd::PushClipRounded(
                    clip.rect.x,
                    clip.rect.y,
                    clip.rect.width,
                    clip.rect.height,
                    tl,
                ));
            }
            Some(CornerRadii { tl, tr, br, bl }) => {
                commands.push(DebugRenderCmd::PushClipRoundedCorners(
                    clip.rect.x,
                    clip.rect.y,
                    clip.rect.width,
                    clip.rect.height,
                    tl,
                    tr,
                    br,
                    bl,
                ));
            }
        }
    }

    clips.len()
}

fn debug_cmd_for_primitive(primitive: &DrawPrimitive) -> DebugRenderCmd {
    match primitive {
        DrawPrimitive::Rect(x, y, w, h, color) => DebugRenderCmd::Rect(*x, *y, *w, *h, *color),
        DrawPrimitive::RoundedRect(x, y, w, h, radius, color) => {
            DebugRenderCmd::RoundedRect(*x, *y, *w, *h, *radius, *color)
        }
        DrawPrimitive::RoundedRectCorners(x, y, w, h, tl, tr, br, bl, color) => {
            DebugRenderCmd::RoundedRectCorners(*x, *y, *w, *h, *tl, *tr, *br, *bl, *color)
        }
        DrawPrimitive::Border(x, y, w, h, radius, width, color, style) => {
            DebugRenderCmd::Border(*x, *y, *w, *h, *radius, *width, *color, *style)
        }
        DrawPrimitive::BorderCorners(x, y, w, h, tl, tr, br, bl, width, color, style) => {
            DebugRenderCmd::BorderCorners(
                *x, *y, *w, *h, *tl, *tr, *br, *bl, *width, *color, *style,
            )
        }
        DrawPrimitive::BorderEdges(x, y, w, h, radius, top, right, bottom, left, color, style) => {
            DebugRenderCmd::BorderEdges(
                *x, *y, *w, *h, *radius, *top, *right, *bottom, *left, *color, *style,
            )
        }
        DrawPrimitive::Shadow(x, y, w, h, ox, oy, blur, size, radius, color) => {
            DebugRenderCmd::Shadow(*x, *y, *w, *h, *ox, *oy, *blur, *size, *radius, *color)
        }
        DrawPrimitive::InsetShadow(x, y, w, h, ox, oy, blur, size, radius, color) => {
            DebugRenderCmd::InsetShadow(*x, *y, *w, *h, *ox, *oy, *blur, *size, *radius, *color)
        }
        DrawPrimitive::TextWithFont(x, y, text, size, color, family, weight, italic) => {
            DebugRenderCmd::TextWithFont(
                *x,
                *y,
                text.clone(),
                *size,
                *color,
                family.clone(),
                *weight,
                *italic,
            )
        }
        DrawPrimitive::Gradient(x, y, w, h, from, to, angle, radius) => {
            DebugRenderCmd::Gradient(*x, *y, *w, *h, *from, *to, *angle, *radius)
        }
        DrawPrimitive::Image(x, y, w, h, image_id, fit, svg_tint) => {
            DebugRenderCmd::Image(*x, *y, *w, *h, image_id.clone(), *fit, *svg_tint)
        }
        DrawPrimitive::Video(x, y, w, h, target_id, fit) => {
            DebugRenderCmd::Video(*x, *y, *w, *h, target_id.clone(), *fit)
        }
        DrawPrimitive::ImageLoading(x, y, w, h) => DebugRenderCmd::ImageLoading(*x, *y, *w, *h),
        DrawPrimitive::ImageFailed(x, y, w, h) => DebugRenderCmd::ImageFailed(*x, *y, *w, *h),
    }
}

pub(super) fn build_tree_with_attrs(mut attrs: Attrs) -> ElementTree {
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

pub(super) fn build_tree_with_frame(mut attrs: Attrs, frame: Frame) -> ElementTree {
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

pub(super) fn build_text_tree_with_frame(attrs: Attrs, frame: Frame) -> ElementTree {
    let id = ElementId::from_term_bytes(vec![2]);
    let mut element = Element::with_attrs(id.clone(), ElementKind::Text, Vec::new(), attrs);
    element.frame = Some(frame);

    let mut tree = ElementTree::new();
    tree.root = Some(id);
    tree.insert(element);
    tree
}

pub(super) fn build_text_input_tree_with_frame(attrs: Attrs, frame: Frame) -> ElementTree {
    let id = ElementId::from_term_bytes(vec![3]);
    let mut element = Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
    element.frame = Some(frame);

    let mut tree = ElementTree::new();
    tree.root = Some(id);
    tree.insert(element);
    tree
}

pub(super) fn build_tree_with_child_frame(
    mut parent_attrs: Attrs,
    parent_frame: Frame,
    mut child_attrs: Attrs,
    child_frame: Frame,
) -> ElementTree {
    if parent_attrs.background.is_none() {
        parent_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    }

    if child_attrs.background.is_none() {
        child_attrs.background = Some(Background::Color(Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        }));
    }

    let parent_id = ElementId::from_term_bytes(vec![4]);
    let child_id = ElementId::from_term_bytes(vec![5]);

    let mut parent =
        Element::with_attrs(parent_id.clone(), ElementKind::El, Vec::new(), parent_attrs);
    parent.children = vec![child_id.clone()];
    parent.frame = Some(parent_frame);

    let mut child = Element::with_attrs(child_id.clone(), ElementKind::El, Vec::new(), child_attrs);
    child.frame = Some(child_frame);

    let mut tree = ElementTree::new();
    tree.root = Some(parent_id);
    tree.insert(parent);
    tree.insert(child);
    tree
}

pub(super) fn mount_nearby(
    tree: &mut ElementTree,
    host_id: &ElementId,
    slot: NearbySlot,
    kind: ElementKind,
    attrs: Attrs,
    frame: Frame,
    id_byte: u8,
) {
    let nearby_id = ElementId::from_term_bytes(vec![id_byte]);
    let mut nearby = Element::with_attrs(nearby_id.clone(), kind, Vec::new(), attrs);
    nearby.frame = Some(frame);
    tree.insert(nearby);
    tree.get_mut(host_id)
        .expect("host should exist")
        .nearby
        .set(slot, Some(nearby_id));
}

pub(super) fn solid_fill_attrs(rgb: (u8, u8, u8)) -> Attrs {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgb {
        r: rgb.0,
        g: rgb.1,
        b: rgb.2,
    }));
    attrs
}

pub(super) fn nearby_origin(
    parent_frame: Frame,
    nearby_frame: Frame,
    slot: NearbySlot,
    align_x: AlignX,
    align_y: AlignY,
) -> (f32, f32) {
    let x = match slot {
        NearbySlot::BehindContent | NearbySlot::Above | NearbySlot::Below | NearbySlot::InFront => {
            match align_x {
                AlignX::Left => parent_frame.x,
                AlignX::Center => parent_frame.x + (parent_frame.width - nearby_frame.width) / 2.0,
                AlignX::Right => parent_frame.x + parent_frame.width - nearby_frame.width,
            }
        }
        NearbySlot::OnLeft => parent_frame.x - nearby_frame.width,
        NearbySlot::OnRight => parent_frame.x + parent_frame.width,
    };

    let y = match slot {
        NearbySlot::Above => parent_frame.y - nearby_frame.height,
        NearbySlot::Below => parent_frame.y + parent_frame.height,
        NearbySlot::BehindContent
        | NearbySlot::OnLeft
        | NearbySlot::OnRight
        | NearbySlot::InFront => match align_y {
            AlignY::Top => parent_frame.y,
            AlignY::Center => parent_frame.y + (parent_frame.height - nearby_frame.height) / 2.0,
            AlignY::Bottom => parent_frame.y + parent_frame.height - nearby_frame.height,
        },
    };

    (x, y)
}

pub(super) fn build_paragraph_tree(mut attrs: Attrs, frame: Frame) -> ElementTree {
    let id = ElementId::from_term_bytes(vec![10]);
    attrs.background = attrs.background.take();
    let mut element = Element::with_attrs(id.clone(), ElementKind::Paragraph, Vec::new(), attrs);
    element.frame = Some(frame);

    let mut tree = ElementTree::new();
    tree.root = Some(id);
    tree.insert(element);
    tree
}
