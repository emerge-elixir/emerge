use super::element::{Element, Frame, RetainedPaintPhase};
use super::geometry::{ClipShape, ShapeBounds, visible_bounds};
use super::scrollbar as tree_scrollbar;
use super::scrollbar::ScrollbarMetrics;
use super::transform::{Affine2, InteractionClip, element_transform};

#[derive(Clone, Copy, Debug)]
struct LocalSceneGeometry {
    self_shape: ShapeBounds,
    host_clip: ClipShape,
    scrollbar_x: Option<ScrollbarMetrics>,
    scrollbar_y: Option<ScrollbarMetrics>,
}

#[derive(Clone, Debug, Default)]
pub struct SceneContext {
    pub scroll_dx: f32,
    pub scroll_dy: f32,
    pub visible_clip: Option<ClipShape>,
    pub nearby_visible_clip: Option<ClipShape>,
    pub front_nearby_subtree: bool,
    pub front_nearby_root: bool,
    pub interaction_transform: Affine2,
    pub interaction_clips: Vec<InteractionClip>,
    pub nearby_interaction_clips: Vec<InteractionClip>,
}

#[derive(Clone, Debug)]
pub struct ResolvedNodeState {
    pub frame: Frame,
    pub adjusted_frame: Frame,
    pub self_shape: ShapeBounds,
    pub host_clip: ClipShape,
    pub visible: bool,
    pub child_visible_clip: ClipShape,
    pub nearby_visible_clip: Option<ClipShape>,
    pub scrollbar_x: Option<ScrollbarMetrics>,
    pub scrollbar_y: Option<ScrollbarMetrics>,
    pub local_scroll_x: f32,
    pub local_scroll_y: f32,
    pub clip_nearby: bool,
    pub front_nearby_subtree: bool,
    pub front_nearby_root: bool,
    pub interaction_transform: Affine2,
    pub interaction_inverse: Option<Affine2>,
    pub interaction_clips: Vec<InteractionClip>,
    pub nearby_interaction_clips: Vec<InteractionClip>,
}

impl ResolvedNodeState {
    fn accumulated_scroll(&self) -> (f32, f32) {
        (
            self.frame.x - self.adjusted_frame.x,
            self.frame.y - self.adjusted_frame.y,
        )
    }
}

pub fn resolve_node_state(element: &Element, ctx: SceneContext) -> Option<ResolvedNodeState> {
    let frame = element.frame?;
    let scene = local_scene_geometry(frame, &element.attrs);

    let adjusted_frame = Frame {
        x: frame.x - ctx.scroll_dx,
        y: frame.y - ctx.scroll_dy,
        ..frame
    };
    let self_shape = scene.self_shape.offset(ctx.scroll_dx, ctx.scroll_dy);
    let clip_nearby = element.attrs.clip_nearby.unwrap_or(false);
    let inherited_clip = if ctx.front_nearby_subtree {
        ctx.nearby_visible_clip
    } else {
        ctx.visible_clip
    };
    let visible_bounds = visible_bounds(self_shape, inherited_clip);
    let visible = visible_bounds.width > 0.0 && visible_bounds.height > 0.0;
    let host_clip = scene.host_clip.offset(ctx.scroll_dx, ctx.scroll_dy);
    let child_visible_clip = inherited_clip
        .map(|clip| super::geometry::intersect_clip(Some(clip), host_clip))
        .unwrap_or(host_clip);
    let nearby_visible_clip = if clip_nearby {
        Some(super::geometry::intersect_clip(
            ctx.nearby_visible_clip,
            host_clip,
        ))
    } else {
        ctx.nearby_visible_clip
    };

    let scrollbar_x = scene
        .scrollbar_x
        .map(|metrics| offset_scrollbar_metrics(metrics, &ctx));
    let scrollbar_y = scene
        .scrollbar_y
        .map(|metrics| offset_scrollbar_metrics(metrics, &ctx));
    let local_transform = element_transform(adjusted_frame, &element.attrs);
    let interaction_transform = ctx.interaction_transform.then(local_transform);
    let interaction_inverse = interaction_transform.inverse();

    Some(ResolvedNodeState {
        frame,
        adjusted_frame,
        self_shape,
        host_clip,
        visible,
        child_visible_clip,
        nearby_visible_clip,
        scrollbar_x,
        scrollbar_y,
        local_scroll_x: element.attrs.scroll_x.unwrap_or(0.0) as f32,
        local_scroll_y: element.attrs.scroll_y.unwrap_or(0.0) as f32,
        clip_nearby,
        front_nearby_subtree: ctx.front_nearby_subtree,
        front_nearby_root: ctx.front_nearby_root,
        interaction_transform,
        interaction_inverse,
        interaction_clips: ctx.interaction_clips,
        nearby_interaction_clips: ctx.nearby_interaction_clips,
    })
}

fn local_scene_geometry(frame: Frame, attrs: &super::attrs::Attrs) -> LocalSceneGeometry {
    LocalSceneGeometry {
        self_shape: super::geometry::self_shape(frame, attrs),
        host_clip: super::geometry::host_clip_shape(frame, attrs),
        scrollbar_x: tree_scrollbar::horizontal_metrics(frame, attrs),
        scrollbar_y: tree_scrollbar::vertical_metrics(frame, attrs),
    }
}

pub fn child_context(state: ResolvedNodeState, phase: RetainedPaintPhase) -> SceneContext {
    let (scroll_dx, scroll_dy) = state.accumulated_scroll();
    let mut interaction_clips = state.interaction_clips.clone();
    let mut nearby_interaction_clips = state.nearby_interaction_clips.clone();

    if !matches!(phase, RetainedPaintPhase::Overlay(_)) {
        interaction_clips.push(InteractionClip::new(
            state.host_clip,
            state.interaction_transform,
        ));
    }

    if state.clip_nearby {
        nearby_interaction_clips.push(InteractionClip::new(
            state.host_clip,
            state.interaction_transform,
        ));
    }

    match phase {
        RetainedPaintPhase::Children => SceneContext {
            scroll_dx: scroll_dx + state.local_scroll_x,
            scroll_dy: scroll_dy + state.local_scroll_y,
            visible_clip: Some(state.child_visible_clip),
            nearby_visible_clip: state.nearby_visible_clip,
            front_nearby_subtree: state.front_nearby_subtree,
            front_nearby_root: false,
            interaction_transform: state.interaction_transform,
            interaction_clips,
            nearby_interaction_clips,
        },
        RetainedPaintPhase::BehindContent => SceneContext {
            scroll_dx,
            scroll_dy,
            visible_clip: Some(state.child_visible_clip),
            nearby_visible_clip: state.nearby_visible_clip,
            front_nearby_subtree: state.front_nearby_subtree,
            front_nearby_root: false,
            interaction_transform: state.interaction_transform,
            interaction_clips,
            nearby_interaction_clips,
        },
        RetainedPaintPhase::Overlay(_) => SceneContext {
            scroll_dx,
            scroll_dy,
            visible_clip: state.nearby_visible_clip,
            nearby_visible_clip: state.nearby_visible_clip,
            front_nearby_subtree: true,
            front_nearby_root: true,
            interaction_transform: state.interaction_transform,
            interaction_clips: nearby_interaction_clips.clone(),
            nearby_interaction_clips,
        },
    }
}

fn offset_scrollbar_metrics(metrics: ScrollbarMetrics, ctx: &SceneContext) -> ScrollbarMetrics {
    ScrollbarMetrics {
        track_x: metrics.track_x - ctx.scroll_dx,
        track_y: metrics.track_y - ctx.scroll_dy,
        thumb_x: metrics.thumb_x - ctx.scroll_dx,
        thumb_y: metrics.thumb_y - ctx.scroll_dy,
        track_start: metrics.track_start
            - match metrics.axis {
                super::scrollbar::ScrollbarAxis::X => ctx.scroll_dx,
                super::scrollbar::ScrollbarAxis::Y => ctx.scroll_dy,
            },
        thumb_start: metrics.thumb_start
            - match metrics.axis {
                super::scrollbar::ScrollbarAxis::X => ctx.scroll_dx,
                super::scrollbar::ScrollbarAxis::Y => ctx.scroll_dy,
            },
        ..metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::Attrs;
    use crate::tree::element::{Element, ElementKind, NearbySlot, NodeId};
    use crate::tree::geometry::Rect;

    fn make_element(id: u8, attrs: Attrs, frame: Frame) -> Element {
        let mut element = Element::with_attrs(
            NodeId::from_term_bytes(vec![id]),
            ElementKind::El,
            Vec::new(),
            attrs,
        );
        element.frame = Some(frame);
        element
    }

    #[test]
    fn child_context_always_applies_host_clip_for_children() {
        let parent = make_element(
            1,
            Attrs::default(),
            Frame {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 50.0,
                content_width: 100.0,
                content_height: 50.0,
            },
        );

        let state = resolve_node_state(
            &parent,
            SceneContext {
                visible_clip: Some(ClipShape {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 400.0,
                        height: 300.0,
                    },
                    radii: None,
                }),
                ..SceneContext::default()
            },
        )
        .expect("state should resolve");

        let child_visible_clip = state.child_visible_clip;
        let child_ctx = child_context(state, RetainedPaintPhase::Children);
        assert_eq!(
            child_ctx.visible_clip,
            Some(child_visible_clip),
            "all hosts should tighten child visibility to the host clip"
        );
    }

    #[test]
    fn child_context_always_applies_host_clip_for_behind_content() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);
        let parent = make_element(
            2,
            attrs,
            Frame {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 50.0,
                content_width: 100.0,
                content_height: 50.0,
            },
        );

        let state = resolve_node_state(
            &parent,
            SceneContext {
                visible_clip: Some(ClipShape {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 400.0,
                        height: 300.0,
                    },
                    radii: None,
                }),
                ..SceneContext::default()
            },
        )
        .expect("state should resolve");

        let child_visible_clip = state.child_visible_clip;
        let child_ctx = child_context(state, RetainedPaintPhase::BehindContent);
        assert_eq!(
            child_ctx.visible_clip,
            Some(child_visible_clip),
            "behind content should render inside the host clip"
        );
    }

    #[test]
    fn resolve_node_state_keeps_scrolled_inherited_clip_in_screen_space() {
        let mut parent_attrs = Attrs::default();
        parent_attrs.scrollbar_y = Some(true);
        parent_attrs.scroll_y = Some(20.0);
        let parent = make_element(
            3,
            parent_attrs,
            Frame {
                x: 0.0,
                y: 100.0,
                width: 120.0,
                height: 50.0,
                content_width: 120.0,
                content_height: 150.0,
            },
        );
        let child = make_element(
            4,
            Attrs::default(),
            Frame {
                x: 0.0,
                y: 130.0,
                width: 120.0,
                height: 20.0,
                content_width: 120.0,
                content_height: 20.0,
            },
        );

        let parent_state = resolve_node_state(&parent, SceneContext::default())
            .expect("parent state should resolve");
        let child_ctx = child_context(parent_state.clone(), RetainedPaintPhase::Children);
        let child_state =
            resolve_node_state(&child, child_ctx.clone()).expect("child state should resolve");

        assert_eq!(child_state.adjusted_frame.y, 110.0);
        assert_eq!(
            child_ctx.visible_clip,
            Some(parent_state.child_visible_clip),
            "child inherited clip should stay in screen space after parent scroll"
        );
    }

    #[test]
    fn resolve_node_state_uses_current_frame_geometry() {
        let frame = Frame {
            x: 40.0,
            y: 60.0,
            width: 120.0,
            height: 50.0,
            content_width: 120.0,
            content_height: 140.0,
        };
        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);
        let mut element = make_element(8, attrs, frame);
        element.frame = Some(frame);

        let state =
            resolve_node_state(&element, SceneContext::default()).expect("state should resolve");

        assert_eq!(state.self_shape.rect.x, 40.0);
        assert_eq!(state.self_shape.rect.y, 60.0);
        assert_eq!(state.host_clip.rect.x, 40.0);
        assert_eq!(state.host_clip.rect.y, 60.0);
        assert_eq!(state.scrollbar_y.expect("scrollbar").track_y, 60.0);
    }

    #[test]
    fn child_context_accumulates_ancestor_and_local_scroll_for_grandchildren() {
        let mut root_attrs = Attrs::default();
        root_attrs.scrollbar_y = Some(true);
        root_attrs.scroll_y = Some(20.0);
        let root = make_element(
            5,
            root_attrs,
            Frame {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 80.0,
                content_width: 120.0,
                content_height: 200.0,
            },
        );

        let mut child_attrs = Attrs::default();
        child_attrs.scrollbar_y = Some(true);
        child_attrs.scroll_y = Some(5.0);
        let child = make_element(
            6,
            child_attrs,
            Frame {
                x: 0.0,
                y: 30.0,
                width: 120.0,
                height: 60.0,
                content_width: 120.0,
                content_height: 120.0,
            },
        );

        let grandchild = make_element(
            7,
            Attrs::default(),
            Frame {
                x: 0.0,
                y: 50.0,
                width: 120.0,
                height: 20.0,
                content_width: 120.0,
                content_height: 20.0,
            },
        );

        let root_state =
            resolve_node_state(&root, SceneContext::default()).expect("root state should resolve");
        let child_state = resolve_node_state(
            &child,
            child_context(root_state, RetainedPaintPhase::Children),
        )
        .expect("child state should resolve");
        let grandchild_ctx = child_context(child_state, RetainedPaintPhase::Children);
        let grandchild_state = resolve_node_state(&grandchild, grandchild_ctx.clone())
            .expect("grandchild should resolve");

        assert_eq!(grandchild_ctx.scroll_dy, 25.0);
        assert_eq!(grandchild_state.adjusted_frame.y, 25.0);
    }

    #[test]
    fn overlay_child_context_marks_only_overlay_root_as_front_nearby_root() {
        let host = make_element(
            9,
            Attrs::default(),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 80.0,
                content_width: 160.0,
                content_height: 80.0,
            },
        );
        let overlay = make_element(
            10,
            Attrs::default(),
            Frame {
                x: 20.0,
                y: 10.0,
                width: 120.0,
                height: 60.0,
                content_width: 120.0,
                content_height: 60.0,
            },
        );
        let descendant = make_element(
            11,
            Attrs::default(),
            Frame {
                x: 30.0,
                y: 20.0,
                width: 80.0,
                height: 20.0,
                content_width: 80.0,
                content_height: 20.0,
            },
        );

        let host_state =
            resolve_node_state(&host, SceneContext::default()).expect("host state should resolve");
        let overlay_ctx =
            child_context(host_state, RetainedPaintPhase::Overlay(NearbySlot::InFront));
        assert!(overlay_ctx.front_nearby_subtree);
        assert!(overlay_ctx.front_nearby_root);

        let overlay_state =
            resolve_node_state(&overlay, overlay_ctx).expect("overlay should resolve");
        assert!(overlay_state.front_nearby_subtree);
        assert!(overlay_state.front_nearby_root);

        let descendant_ctx = child_context(overlay_state, RetainedPaintPhase::Children);
        assert!(descendant_ctx.front_nearby_subtree);
        assert!(!descendant_ctx.front_nearby_root);

        let descendant_state =
            resolve_node_state(&descendant, descendant_ctx).expect("descendant should resolve");
        assert!(descendant_state.front_nearby_subtree);
        assert!(!descendant_state.front_nearby_root);
    }
}
