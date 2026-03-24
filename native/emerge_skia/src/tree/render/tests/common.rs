use super::*;
use crate::render_scene::{DrawPrimitive, RenderNode, RenderScene};
use crate::renderer::{RenderState, Renderer};
use crate::tree::geometry::ClipShape;
use crate::tree::transform::Affine2;
use skia_safe::Color as SkColor;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ScopeKind {
    Clip { clips: Vec<ClipShape> },
    Transform { transform: Affine2 },
    Alpha { alpha: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScopeRecord {
    pub id: usize,
    pub parent_id: Option<usize>,
    pub kind: ScopeKind,
    pub entry_transform: Affine2,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AppliedClip {
    pub scope_id: usize,
    pub shape: ClipShape,
    pub transform_at_application: Affine2,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AlphaScope {
    pub scope_id: usize,
    pub alpha: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ResolvedDraw {
    pub primitive: DrawPrimitive,
    pub paint_order: usize,
    pub cumulative_transform: Affine2,
    pub clips: Vec<AppliedClip>,
    pub alpha_scopes: Vec<AlphaScope>,
    pub scope_path: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SceneTrace {
    pub scopes: Vec<ScopeRecord>,
    pub draws: Vec<ResolvedDraw>,
}

#[derive(Clone, Debug, Default)]
struct ObserveContext {
    cumulative_transform: Affine2,
    clips: Vec<AppliedClip>,
    alpha_scopes: Vec<AlphaScope>,
    scope_path: Vec<usize>,
}

#[derive(Debug, Default)]
struct ObserveState {
    next_scope_id: usize,
    next_paint_order: usize,
}

pub(super) fn render_output(tree: &ElementTree) -> super::super::RenderOutput {
    super::super::render_tree(tree)
}

pub(super) fn trace_output(tree: &ElementTree) -> (super::super::RenderOutput, SceneTrace) {
    let output = render_output(tree);
    let trace = trace_scene(&output.scene);
    (output, trace)
}

pub(super) fn trace_tree(tree: &ElementTree) -> SceneTrace {
    trace_output(tree).1
}

pub(super) fn observe_output(
    tree: &ElementTree,
) -> (super::super::RenderOutput, Vec<ResolvedDraw>) {
    let (output, trace) = trace_output(tree);
    (output, trace.draws)
}

pub(super) fn observe_tree(tree: &ElementTree) -> Vec<ResolvedDraw> {
    trace_tree(tree).draws
}

pub(super) fn trace_scene(scene: &RenderScene) -> SceneTrace {
    let mut draws = Vec::new();
    let mut scopes = Vec::new();
    let mut state = ObserveState::default();
    trace_nodes(
        &scene.nodes,
        &ObserveContext::default(),
        &mut state,
        &mut scopes,
        &mut draws,
    );
    SceneTrace { scopes, draws }
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

pub(super) fn render_tree_to_pixels(
    width: u32,
    height: u32,
    tree: &ElementTree,
) -> (super::super::RenderOutput, Vec<u8>) {
    let output = render_output(tree);
    let pixels = render_scene_to_pixels(width, height, output.scene.clone());
    (output, pixels)
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
        .map(|scope| scope.scope_id)
        .eq(b.alpha_scopes.iter().map(|scope| scope.scope_id))
}

pub(super) fn scope<'a>(trace: &'a SceneTrace, scope_id: usize) -> &'a ScopeRecord {
    trace
        .scopes
        .iter()
        .find(|scope| scope.id == scope_id)
        .expect("scope id should exist in scene trace")
}

pub(super) fn scope_chain<'a>(trace: &'a SceneTrace, draw: &ResolvedDraw) -> Vec<&'a ScopeRecord> {
    draw.scope_path
        .iter()
        .map(|scope_id| scope(trace, *scope_id))
        .collect()
}

pub(super) fn matching_scopes<'a, F>(
    trace: &'a SceneTrace,
    draw: &ResolvedDraw,
    pred: F,
) -> Vec<&'a ScopeRecord>
where
    F: Fn(&ScopeRecord) -> bool,
{
    scope_chain(trace, draw)
        .into_iter()
        .filter(|scope| pred(scope))
        .collect()
}

pub(super) fn clip_scope_chain<'a>(
    trace: &'a SceneTrace,
    draw: &ResolvedDraw,
) -> Vec<&'a ScopeRecord> {
    matching_scopes(trace, draw, |scope| {
        matches!(scope.kind, ScopeKind::Clip { .. })
    })
}

pub(super) fn alpha_scope_chain<'a>(
    trace: &'a SceneTrace,
    draw: &ResolvedDraw,
) -> Vec<&'a ScopeRecord> {
    matching_scopes(trace, draw, |scope| {
        matches!(scope.kind, ScopeKind::Alpha { .. })
    })
}

pub(super) fn clip_scope_shapes(scope: &ScopeRecord) -> Option<&[ClipShape]> {
    match &scope.kind {
        ScopeKind::Clip { clips } => Some(clips.as_slice()),
        _ => None,
    }
}

pub(super) fn alpha_scope_value(scope: &ScopeRecord) -> Option<f32> {
    match scope.kind {
        ScopeKind::Alpha { alpha } => Some(alpha),
        _ => None,
    }
}

pub(super) fn same_immediate_clip_scope(
    trace: &SceneTrace,
    a: &ResolvedDraw,
    b: &ResolvedDraw,
) -> bool {
    immediate_clip_scope(trace, a).map(|scope| scope.id)
        == immediate_clip_scope(trace, b).map(|scope| scope.id)
}

pub(super) fn immediate_clip_scope<'a>(
    trace: &'a SceneTrace,
    draw: &ResolvedDraw,
) -> Option<&'a ScopeRecord> {
    draw.scope_path.iter().rev().find_map(|scope_id| {
        let scope = scope(trace, *scope_id);
        matches!(scope.kind, ScopeKind::Clip { .. }).then_some(scope)
    })
}

pub(super) fn clip_scope_usage<F>(trace: &SceneTrace, pred: F) -> usize
where
    F: Fn(&ScopeRecord) -> bool,
{
    trace.scopes.iter().filter(|scope| pred(scope)).count()
}

fn trace_nodes(
    nodes: &[RenderNode],
    context: &ObserveContext,
    state: &mut ObserveState,
    scopes: &mut Vec<ScopeRecord>,
    draws: &mut Vec<ResolvedDraw>,
) {
    for node in nodes {
        match node {
            RenderNode::Clip { clips, children } => {
                let scope_id = state.next_scope_id;
                state.next_scope_id += 1;

                scopes.push(ScopeRecord {
                    id: scope_id,
                    parent_id: context.scope_path.last().copied(),
                    kind: ScopeKind::Clip {
                        clips: clips.clone(),
                    },
                    entry_transform: context.cumulative_transform,
                });

                let mut next_context = context.clone();
                next_context.scope_path.push(scope_id);
                for clip in clips {
                    next_context.clips.push(AppliedClip {
                        scope_id,
                        shape: *clip,
                        transform_at_application: context.cumulative_transform,
                    });
                }
                trace_nodes(children, &next_context, state, scopes, draws);
            }
            RenderNode::Transform {
                transform,
                children,
            } => {
                let scope_id = state.next_scope_id;
                state.next_scope_id += 1;

                scopes.push(ScopeRecord {
                    id: scope_id,
                    parent_id: context.scope_path.last().copied(),
                    kind: ScopeKind::Transform {
                        transform: *transform,
                    },
                    entry_transform: context.cumulative_transform,
                });

                let mut next_context = context.clone();
                next_context.scope_path.push(scope_id);
                next_context.cumulative_transform = context.cumulative_transform.mul(*transform);
                trace_nodes(children, &next_context, state, scopes, draws);
            }
            RenderNode::Alpha { alpha, children } => {
                let scope_id = state.next_scope_id;
                state.next_scope_id += 1;

                scopes.push(ScopeRecord {
                    id: scope_id,
                    parent_id: context.scope_path.last().copied(),
                    kind: ScopeKind::Alpha { alpha: *alpha },
                    entry_transform: context.cumulative_transform,
                });

                let mut next_context = context.clone();
                next_context.scope_path.push(scope_id);
                next_context.alpha_scopes.push(AlphaScope {
                    scope_id,
                    alpha: *alpha,
                });
                trace_nodes(children, &next_context, state, scopes, draws);
            }
            RenderNode::Primitive(primitive) => {
                draws.push(ResolvedDraw {
                    primitive: primitive.clone(),
                    paint_order: state.next_paint_order,
                    cumulative_transform: context.cumulative_transform,
                    clips: context.clips.clone(),
                    alpha_scopes: context.alpha_scopes.clone(),
                    scope_path: context.scope_path.clone(),
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
