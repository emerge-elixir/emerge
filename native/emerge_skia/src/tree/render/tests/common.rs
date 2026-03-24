use super::*;
use crate::render_scene::{DrawPrimitive, RenderNode, RenderScene};
use crate::renderer::{RenderState, Renderer};
use crate::tree::geometry::ClipShape;
use crate::tree::transform::Affine2;
use skia_safe::Color as SkColor;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AppliedClip {
    pub id: usize,
    pub shape: ClipShape,
    pub transform_at_application: Affine2,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AlphaScope {
    pub id: usize,
    pub alpha: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ResolvedDraw {
    pub primitive: DrawPrimitive,
    pub paint_order: usize,
    pub cumulative_transform: Affine2,
    pub clips: Vec<AppliedClip>,
    pub alpha_scopes: Vec<AlphaScope>,
}

#[derive(Clone, Debug, Default)]
struct ObserveContext {
    cumulative_transform: Affine2,
    clips: Vec<AppliedClip>,
    alpha_scopes: Vec<AlphaScope>,
}

#[derive(Debug, Default)]
struct ObserveState {
    next_clip_id: usize,
    next_alpha_id: usize,
    next_paint_order: usize,
}

pub(super) fn render_output(tree: &ElementTree) -> super::super::RenderOutput {
    super::super::render_tree(tree)
}

pub(super) fn observe_output(
    tree: &ElementTree,
) -> (super::super::RenderOutput, Vec<ResolvedDraw>) {
    let output = render_output(tree);
    let draws = observe_scene(&output.scene);
    (output, draws)
}

pub(super) fn observe_tree(tree: &ElementTree) -> Vec<ResolvedDraw> {
    observe_output(tree).1
}

pub(super) fn observe_scene(scene: &RenderScene) -> Vec<ResolvedDraw> {
    let mut draws = Vec::new();
    let mut state = ObserveState::default();
    observe_nodes(
        &scene.nodes,
        &ObserveContext::default(),
        &mut state,
        &mut draws,
    );
    draws
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

pub(super) fn only_draw<'a, F>(draws: &'a [ResolvedDraw], pred: F) -> &'a ResolvedDraw
where
    F: Fn(&ResolvedDraw) -> bool,
{
    let matches = matching_draws(draws, pred);
    assert_eq!(matches.len(), 1, "expected exactly one matching draw");
    matches[0]
}

pub(super) fn matching_draws<'a, F>(draws: &'a [ResolvedDraw], pred: F) -> Vec<&'a ResolvedDraw>
where
    F: Fn(&ResolvedDraw) -> bool,
{
    draws.iter().filter(|draw| pred(draw)).collect()
}

pub(super) fn paints_before(a: &ResolvedDraw, b: &ResolvedDraw) -> bool {
    a.paint_order < b.paint_order
}

pub(super) fn shares_alpha_scope(a: &ResolvedDraw, b: &ResolvedDraw) -> bool {
    a.alpha_scopes
        .iter()
        .map(|scope| scope.id)
        .eq(b.alpha_scopes.iter().map(|scope| scope.id))
}

pub(super) fn unique_clip_scope_count<F>(draws: &[ResolvedDraw], pred: F) -> usize
where
    F: Fn(&AppliedClip) -> bool,
{
    let mut clip_ids = HashSet::new();
    for draw in draws {
        for clip in &draw.clips {
            if pred(clip) {
                clip_ids.insert(clip.id);
            }
        }
    }

    clip_ids.len()
}

fn observe_nodes(
    nodes: &[RenderNode],
    context: &ObserveContext,
    state: &mut ObserveState,
    draws: &mut Vec<ResolvedDraw>,
) {
    for node in nodes {
        match node {
            RenderNode::Clip { clips, children } => {
                let mut next_context = context.clone();
                for clip in clips {
                    next_context.clips.push(AppliedClip {
                        id: state.next_clip_id,
                        shape: *clip,
                        transform_at_application: context.cumulative_transform,
                    });
                    state.next_clip_id += 1;
                }
                observe_nodes(children, &next_context, state, draws);
            }
            RenderNode::Transform {
                transform,
                children,
            } => {
                let mut next_context = context.clone();
                next_context.cumulative_transform = context.cumulative_transform.mul(*transform);
                observe_nodes(children, &next_context, state, draws);
            }
            RenderNode::Alpha { alpha, children } => {
                let mut next_context = context.clone();
                next_context.alpha_scopes.push(AlphaScope {
                    id: state.next_alpha_id,
                    alpha: *alpha,
                });
                state.next_alpha_id += 1;
                observe_nodes(children, &next_context, state, draws);
            }
            RenderNode::Primitive(primitive) => {
                draws.push(ResolvedDraw {
                    primitive: primitive.clone(),
                    paint_order: state.next_paint_order,
                    cumulative_transform: context.cumulative_transform,
                    clips: context.clips.clone(),
                    alpha_scopes: context.alpha_scopes.clone(),
                });
                state.next_paint_order += 1;
            }
        }
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
