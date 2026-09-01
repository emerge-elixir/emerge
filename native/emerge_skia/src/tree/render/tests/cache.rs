use super::common::{build_tree_with_child_frame, mount_nearby, solid_fill_attrs};
use super::*;
use crate::render_scene::{
    DrawPrimitive, PaintLayerPolicy, PaintLayerReason, RenderNode, RenderPaintLayer,
    RenderPaintLayerContentNode,
};
use crate::renderer::{RendererCacheConfig, SceneRenderer};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
struct LayerDescriptor {
    stable_id: u64,
    parent: Option<(u64, PaintLayerReason)>,
    reason: PaintLayerReason,
    policy: PaintLayerPolicy,
    own_colors: BTreeSet<u32>,
    own_run_colors: Vec<BTreeSet<u32>>,
    owns_video: bool,
}

struct CameraFixture {
    tree: ElementTree,
    root_id: NodeId,
    video_id: NodeId,
    top_id: NodeId,
    bottom_id: NodeId,
    slider_ids: Vec<NodeId>,
    track_ids: Vec<NodeId>,
    fill_ids: Vec<NodeId>,
    track_colors: Vec<u32>,
    fill_colors: Vec<u32>,
    thumb_colors: Vec<u32>,
    static_colors: [u32; 3],
}

#[test]
fn camera_like_semantic_topology_is_exact_and_generation_local() {
    let mut fixture = camera_fixture();

    let dirty = super::super::render_tree_scene_with_scroll_layers(&fixture.tree).scene;
    let dirty_descriptor = describe_scene(&dirty.nodes);
    assert_camera_descriptor(&fixture, &dirty_descriptor);
    let dirty_generations = layer_generations(&dirty.nodes);

    fixture.tree.clear_refresh_dirty();
    let clean = super::super::render_tree_scene_with_scroll_layers(&fixture.tree).scene;
    assert_eq!(describe_scene(&clean.nodes), dirty_descriptor);
    assert_eq!(layer_generations(&clean.nodes), dirty_generations);

    let changed_fill = fixture.fill_ids[2];
    fixture
        .tree
        .get_mut(&changed_fill)
        .and_then(|fill| fill.layout.frame.as_mut())
        .expect("selected slider fill frame")
        .width += 7.0;
    fixture
        .tree
        .mark_render_and_registry_refresh_dirty(&changed_fill);
    let slider_changed = super::super::render_tree_scene_with_scroll_layers(&fixture.tree).scene;
    assert_eq!(describe_scene(&slider_changed.nodes), dirty_descriptor);
    let changed_generations = layer_generations(&slider_changed.nodes);
    let changed_keys = changed_generations
        .iter()
        .filter_map(|(key, generation)| {
            (dirty_generations.get(key) != Some(generation)).then_some(*key)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        changed_keys,
        vec![(
            fixture.slider_ids[2].to_wire_u64(),
            PaintLayerReason::SliderValue,
        )]
    );

    fixture.tree.clear_refresh_dirty();
    fixture
        .tree
        .mark_render_and_registry_refresh_dirty(&fixture.video_id);
    let camera_only = super::super::render_tree_scene_with_scroll_layers(&fixture.tree).scene;
    assert_eq!(describe_scene(&camera_only.nodes), dirty_descriptor);
    assert_eq!(layer_generations(&camera_only.nodes), changed_generations);
}

#[test]
fn warm_camera_like_scene_reuses_every_static_run_while_video_stays_direct() {
    let mut fixture = camera_fixture();
    fixture.tree.clear_refresh_dirty();
    let scene = super::super::render_tree_scene_with_scroll_layers(&fixture.tree).scene;
    let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
        enabled: true,
        max_new_payloads_per_frame: 64,
        ..RendererCacheConfig::default()
    });

    let (_, cold_timings) = super::pipeline::render_scene_with_renderer_to_pixels_and_timings(
        &mut renderer,
        scene.clone(),
        800,
        600,
    );
    let (_, warm_timings) = super::pipeline::render_scene_with_renderer_to_pixels_and_timings(
        &mut renderer,
        scene,
        800,
        600,
    );
    let cold = cold_timings
        .renderer_cache
        .expect("cold Camera cache stats");
    let warm = warm_timings
        .renderer_cache
        .expect("warm Camera cache stats");
    assert_eq!(cold.paint_layer.stores, 22);
    assert_eq!(warm.paint_layer.hits, 22);
    assert_eq!(warm.paint_layer.stores, 0);

    fixture
        .tree
        .mark_render_and_registry_refresh_dirty(&fixture.video_id);
    let camera_only = super::super::render_tree_scene_with_scroll_layers(&fixture.tree).scene;
    let (_, video_timings) = super::pipeline::render_scene_with_renderer_to_pixels_and_timings(
        &mut renderer,
        camera_only,
        800,
        600,
    );
    let video = video_timings
        .renderer_cache
        .expect("camera-only redraw cache stats");
    assert_eq!(video.paint_layer.hits, 22);
    assert_eq!(video.paint_layer.stores, 0);

    let changed_track = fixture.track_ids[2];
    fixture
        .tree
        .get_mut(&changed_track)
        .and_then(|track| track.layout.frame.as_mut())
        .expect("selected slider track frame")
        .width -= 5.0;
    fixture
        .tree
        .mark_render_and_registry_refresh_dirty(&changed_track);
    let changed_fill = fixture.fill_ids[2];
    fixture
        .tree
        .get_mut(&changed_fill)
        .and_then(|fill| fill.layout.frame.as_mut())
        .expect("selected slider fill frame")
        .width += 7.0;
    fixture
        .tree
        .mark_render_and_registry_refresh_dirty(&changed_fill);
    let slider_changed = super::super::render_tree_scene_with_scroll_layers(&fixture.tree).scene;
    let (_, slider_timings) = super::pipeline::render_scene_with_renderer_to_pixels_and_timings(
        &mut renderer,
        slider_changed,
        800,
        600,
    );
    let slider = slider_timings
        .renderer_cache
        .expect("changed slider cache stats");
    assert_eq!(slider.paint_layer.hits, 20);
    assert_eq!(slider.paint_layer.misses, 2);
    assert_eq!(slider.paint_layer.stores, 2);
}

#[test]
fn root_and_nearby_boundaries_do_not_depend_on_dirty_state() {
    let mut fixture = camera_fixture();
    let dirty = super::super::render_tree_scene_with_scroll_layers(&fixture.tree).scene;
    fixture.tree.clear_refresh_dirty();
    let clean = super::super::render_tree_scene_with_scroll_layers(&fixture.tree).scene;

    assert_eq!(describe_scene(&dirty.nodes), describe_scene(&clean.nodes));
    let descriptor = describe_scene(&dirty.nodes);
    assert_eq!(descriptor[0].reason, PaintLayerReason::Root);
    assert_eq!(descriptor[0].policy, PaintLayerPolicy::DirectOnly);
    assert_eq!(
        descriptor
            .iter()
            .filter(|layer| layer.reason == PaintLayerReason::Nearby)
            .count(),
        2
    );
}

#[test]
fn video_under_cacheable_nearby_becomes_local_direct_media() {
    let root_id = NodeId::from_u64(50_000);
    let nearby_id = NodeId::from_u64(50_001);
    let video_id = NodeId::from_u64(50_002);
    let mut root = element(
        root_id,
        ElementKind::El,
        solid_fill_attrs((5, 5, 5)),
        frame(0.0, 0.0, 200.0, 120.0),
    );
    root.nearby.push(NearbySlot::InFront, nearby_id);
    let mut nearby = element(
        nearby_id,
        ElementKind::El,
        Attrs::default(),
        frame(0.0, 0.0, 200.0, 120.0),
    );
    nearby.children = vec![video_id];
    let video = element(
        video_id,
        ElementKind::Video,
        Attrs {
            video_target: Some("overlay-video".to_string()),
            ..Attrs::default()
        },
        frame(10.0, 10.0, 100.0, 80.0),
    );
    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(nearby);
    tree.insert(video);

    let scene = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    let descriptor = describe_scene(&scene.nodes);
    assert_eq!(
        descriptor
            .iter()
            .map(|layer| layer.reason)
            .collect::<Vec<_>>(),
        vec![
            PaintLayerReason::Root,
            PaintLayerReason::Nearby,
            PaintLayerReason::DirectMedia,
        ]
    );
    assert_eq!(descriptor[2].policy, PaintLayerPolicy::DirectOnly);
    assert!(descriptor[2].owns_video);
    assert!(!descriptor[1].owns_video);
}

#[test]
fn animated_video_stays_direct_below_its_cacheable_animation_boundary() {
    let root_id = NodeId::from_u64(55_000);
    let video_id = NodeId::from_u64(55_001);
    let mut root = element(
        root_id,
        ElementKind::El,
        Attrs::default(),
        frame(0.0, 0.0, 200.0, 120.0),
    );
    root.children = vec![video_id];
    let animation = crate::tree::animation::AnimationSpec {
        keyframes: vec![
            Attrs {
                move_x: Some(0.0),
                ..Attrs::default()
            },
            Attrs {
                move_x: Some(30.0),
                ..Attrs::default()
            },
        ],
        duration_ms: 100.0,
        curve: crate::tree::animation::AnimationCurve::Linear,
        repeat: crate::tree::animation::AnimationRepeat::Once,
    };
    let mut video = element(
        video_id,
        ElementKind::Video,
        Attrs {
            video_target: Some("animated-video".to_string()),
            ..Attrs::default()
        },
        frame(10.0, 10.0, 100.0, 80.0),
    );
    video.spec.declared.animate = Some(animation);
    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(video);

    let scene = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    let descriptor = describe_scene(&scene.nodes);
    assert_eq!(
        descriptor
            .iter()
            .map(|layer| layer.reason)
            .collect::<Vec<_>>(),
        vec![
            PaintLayerReason::Root,
            PaintLayerReason::Animation,
            PaintLayerReason::DirectMedia,
        ]
    );
    assert!(descriptor[2].owns_video);
    assert!(
        descriptor
            .iter()
            .filter(|layer| layer.policy == PaintLayerPolicy::Cacheable)
            .all(|layer| !layer.owns_video)
    );
}

#[test]
fn declared_scroll_and_animation_boundaries_are_structural() {
    let root_id = NodeId::from_u64(60_000);
    let scroll_id = NodeId::from_u64(60_001);
    let animated_id = NodeId::from_u64(60_002);
    let child_id = NodeId::from_u64(60_003);
    let mut root = element(
        root_id,
        ElementKind::El,
        Attrs::default(),
        frame(0.0, 0.0, 240.0, 160.0),
    );
    root.children = vec![scroll_id];
    let mut scroll = element(
        scroll_id,
        ElementKind::El,
        Attrs {
            scrollbar_y: Some(true),
            ..Attrs::default()
        },
        Frame {
            content_height: 240.0,
            ..frame(0.0, 0.0, 240.0, 160.0)
        },
    );
    scroll.children = vec![animated_id];
    scroll.layout.scroll_y_max = 80.0;
    let animation = crate::tree::animation::AnimationSpec {
        keyframes: vec![
            Attrs {
                move_x: Some(0.0),
                ..Attrs::default()
            },
            Attrs {
                move_x: Some(20.0),
                ..Attrs::default()
            },
        ],
        duration_ms: 100.0,
        curve: crate::tree::animation::AnimationCurve::Linear,
        repeat: crate::tree::animation::AnimationRepeat::Once,
    };
    let mut animated = element(
        animated_id,
        ElementKind::El,
        solid_fill_attrs((20, 40, 60)),
        frame(0.0, 0.0, 200.0, 80.0),
    );
    animated.spec.declared.animate = Some(animation);
    animated.children = vec![child_id];
    let child = element(
        child_id,
        ElementKind::El,
        solid_fill_attrs((80, 100, 120)),
        frame(0.0, 0.0, 100.0, 40.0),
    );
    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(scroll);
    tree.insert(animated);
    tree.insert(child);

    let dirty = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    tree.clear_refresh_dirty();
    let clean = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    let reasons = describe_scene(&dirty.nodes)
        .iter()
        .map(|layer| layer.reason)
        .collect::<Vec<_>>();
    assert_eq!(
        reasons,
        vec![
            PaintLayerReason::Root,
            PaintLayerReason::ScrollContent,
            PaintLayerReason::Animation,
        ]
    );
    assert_eq!(describe_scene(&dirty.nodes), describe_scene(&clean.nodes));
    let clean_generations = layer_generations(&clean.nodes);
    let clean_transform = transform_for_layer_reason(&clean.nodes, PaintLayerReason::Animation)
        .expect("declared animation placement");

    let animated = tree.get_mut(&animated_id).expect("animated element");
    animated.layout.effective.move_x = Some(15.0);
    animated.layout.effective.alpha = Some(0.6);
    tree.mark_render_and_registry_refresh_dirty(&animated_id);
    let sampled = super::super::render_tree_scene_with_scroll_layers(&tree).scene;

    assert_eq!(describe_scene(&sampled.nodes), describe_scene(&clean.nodes));
    let clean_animation = describe_layers(&clean.nodes)
        .into_iter()
        .find(|layer| layer.id.role == PaintLayerReason::Animation)
        .expect("clean animation layer");
    let sampled_animation = describe_layers(&sampled.nodes)
        .into_iter()
        .find(|layer| layer.id.role == PaintLayerReason::Animation)
        .expect("sampled animation layer");
    assert_eq!(
        clean_animation.content.own_payload_render_nodes(),
        sampled_animation.content.own_payload_render_nodes()
    );
    assert_eq!(layer_generations(&sampled.nodes), clean_generations);
    assert_eq!(clean_transform, crate::tree::transform::Affine2::identity());
    assert_eq!(
        transform_for_layer_reason(&sampled.nodes, PaintLayerReason::Animation),
        Some(crate::tree::transform::Affine2::translation(15.0, 0.0))
    );
}

#[test]
fn paint_animation_does_not_create_a_compositor_layer() {
    let root_id = NodeId::from_u64(65_000);
    let child_id = NodeId::from_u64(65_001);
    let mut root = element(
        root_id,
        ElementKind::El,
        Attrs::default(),
        frame(0.0, 0.0, 160.0, 100.0),
    );
    root.children = vec![child_id];
    let mut child = element(
        child_id,
        ElementKind::El,
        solid_fill_attrs((20, 40, 60)),
        frame(0.0, 0.0, 80.0, 40.0),
    );
    child.spec.declared.animate = Some(crate::tree::animation::AnimationSpec {
        keyframes: vec![
            Attrs {
                background: Some(crate::tree::attrs::Background::Color(
                    crate::tree::attrs::Color::Rgba {
                        r: 0x10,
                        g: 0x20,
                        b: 0x30,
                        a: 0xFF,
                    },
                )),
                ..Attrs::default()
            },
            Attrs {
                background: Some(crate::tree::attrs::Background::Color(
                    crate::tree::attrs::Color::Rgba {
                        r: 0x40,
                        g: 0x50,
                        b: 0x60,
                        a: 0xFF,
                    },
                )),
                ..Attrs::default()
            },
        ],
        duration_ms: 100.0,
        curve: crate::tree::animation::AnimationCurve::Linear,
        repeat: crate::tree::animation::AnimationRepeat::Once,
    });
    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);

    let scene = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    assert_eq!(
        describe_scene(&scene.nodes)
            .iter()
            .map(|layer| layer.reason)
            .collect::<Vec<_>>(),
        vec![PaintLayerReason::Root]
    );
}

#[test]
fn scroll_content_normalizes_offset_out_of_payload_generation() {
    let root_id = NodeId::from_u64(70_000);
    let child_id = NodeId::from_u64(70_001);
    let mut root = element(
        root_id,
        ElementKind::El,
        Attrs {
            scrollbar_y: Some(true),
            ..Attrs::default()
        },
        Frame {
            content_height: 200.0,
            ..frame(0.0, 0.0, 160.0, 100.0)
        },
    );
    root.children = vec![child_id];
    root.layout.scroll_y_max = 100.0;
    let child = element(
        child_id,
        ElementKind::El,
        solid_fill_attrs((40, 80, 120)),
        frame(0.0, 20.0, 140.0, 40.0),
    );
    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);
    tree.clear_refresh_dirty();

    let first = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    let first_layer = describe_layers(&first.nodes)
        .into_iter()
        .find(|layer| layer.id.role == PaintLayerReason::ScrollContent)
        .expect("declared scroll content layer");
    let first_generation = first_layer.content_generation;
    let first_transform = transform_for_layer_reason(&first.nodes, PaintLayerReason::ScrollContent)
        .expect("scroll content placement");

    assert!(tree.apply_scroll_y(&root_id, -10.0).is_dirty());
    let second = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    let second_layer = describe_layers(&second.nodes)
        .into_iter()
        .find(|layer| layer.id.role == PaintLayerReason::ScrollContent)
        .expect("stable scroll content layer");
    let second_transform =
        transform_for_layer_reason(&second.nodes, PaintLayerReason::ScrollContent)
            .expect("moved scroll content placement");

    assert_eq!(second_layer.content_generation, first_generation);
    assert_eq!(first_transform, crate::tree::transform::Affine2::identity());
    assert_eq!(
        second_transform,
        crate::tree::transform::Affine2::translation(0.0, -10.0)
    );
}

#[test]
fn nested_slider_generation_is_local_and_scroll_offset_independent() {
    let root_id = NodeId::from_u64(75_000);
    let slider_id = NodeId::from_u64(75_001);
    let track_id = NodeId::from_u64(75_002);
    let fill_id = NodeId::from_u64(75_003);
    let thumb_id = NodeId::from_u64(75_004);
    let mut root = element(
        root_id,
        ElementKind::El,
        Attrs {
            scrollbar_y: Some(true),
            ..Attrs::default()
        },
        Frame {
            content_height: 200.0,
            ..frame(0.0, 0.0, 160.0, 100.0)
        },
    );
    root.layout.scroll_y_max = 100.0;
    root.children = vec![slider_id];
    let mut slider = element(
        slider_id,
        ElementKind::Slider,
        Attrs::default(),
        frame(10.0, 20.0, 120.0, 24.0),
    );
    slider.children = vec![track_id, fill_id, thumb_id];
    let track = element(
        track_id,
        ElementKind::El,
        color_attrs(0x202020FF),
        frame(10.0, 28.0, 120.0, 8.0),
    );
    let fill = element(
        fill_id,
        ElementKind::El,
        color_attrs(0x40A0FFFF),
        frame(10.0, 28.0, 70.0, 8.0),
    );
    let thumb = element(
        thumb_id,
        ElementKind::El,
        color_attrs(0xFFFFFFFF),
        frame(70.0, 22.0, 20.0, 20.0),
    );
    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    for element in [root, slider, track, fill, thumb] {
        tree.insert(element);
    }
    tree.clear_refresh_dirty();

    let first = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    let first_generations = layer_generations(&first.nodes);
    assert!(tree.apply_scroll_y(&root_id, -10.0).is_dirty());
    let scrolled = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    assert_eq!(layer_generations(&scrolled.nodes), first_generations);

    tree.get_mut(&fill_id)
        .and_then(|fill| fill.layout.frame.as_mut())
        .expect("slider fill frame")
        .width += 5.0;
    tree.mark_render_and_registry_refresh_dirty(&fill_id);
    let changed = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    let changed_generations = layer_generations(&changed.nodes);
    assert_eq!(
        changed_generations[&(root_id.to_wire_u64(), PaintLayerReason::ScrollContent)],
        first_generations[&(root_id.to_wire_u64(), PaintLayerReason::ScrollContent)]
    );
    assert_ne!(
        changed_generations[&(slider_id.to_wire_u64(), PaintLayerReason::SliderValue)],
        first_generations[&(slider_id.to_wire_u64(), PaintLayerReason::SliderValue)]
    );
}

#[test]
fn clean_nearby_fragment_reuses_the_mounted_subtree_without_descending() {
    let host_id = NodeId::from_term_bytes(vec![5]);
    let nearby_id = NodeId::from_term_bytes(vec![42]);
    let mut tree = build_tree_with_child_frame(
        Attrs::default(),
        frame(0.0, 0.0, 360.0, 240.0),
        solid_fill_attrs((20, 24, 32)),
        frame(64.0, 72.0, 120.0, 48.0),
    );
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::InFront,
        ElementKind::El,
        solid_fill_attrs((248, 250, 252)),
        frame(64.0, 72.0, 220.0, 180.0),
        42,
    );

    let child_ids = (0u8..24)
        .map(|index| {
            let id = NodeId::from_term_bytes(vec![100 + index]);
            let child = element(
                id,
                ElementKind::El,
                solid_fill_attrs((80 + index, 120, 180)),
                frame(72.0, 80.0 + f32::from(index) * 5.0, 160.0, 4.0),
            );
            tree.insert(child);
            id
        })
        .collect();
    tree.get_mut(&nearby_id).expect("nearby root").children = child_ids;
    tree.clear_refresh_dirty();

    super::super::reset_render_traversal_diagnostics_for_benchmark();
    let first = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    let first_diagnostics = super::super::take_render_traversal_diagnostics_for_benchmark();
    assert!(
        describe_layers(&first.nodes)
            .iter()
            .any(|layer| layer.id.role == PaintLayerReason::Nearby)
    );

    super::super::reset_render_traversal_diagnostics_for_benchmark();
    let second = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    let second_diagnostics = super::super::take_render_traversal_diagnostics_for_benchmark();

    assert_eq!(describe_scene(&first.nodes), describe_scene(&second.nodes));
    assert!(
        first_diagnostics.element_visits >= 26,
        "{first_diagnostics:?}"
    );
    assert!(
        second_diagnostics.element_visits <= 3,
        "{second_diagnostics:?}"
    );
}

fn camera_fixture() -> CameraFixture {
    let root_id = NodeId::from_u64(10_000);
    let video_id = NodeId::from_u64(10_001);
    let top_id = NodeId::from_u64(10_002);
    let bottom_id = NodeId::from_u64(10_003);
    let mut root = element(
        root_id,
        ElementKind::El,
        solid_fill_attrs((8, 12, 18)),
        frame(0.0, 0.0, 800.0, 600.0),
    );
    root.children = vec![video_id];
    root.nearby.push(NearbySlot::InFront, top_id);
    root.nearby.push(NearbySlot::InFront, bottom_id);
    let video = element(
        video_id,
        ElementKind::Video,
        Attrs {
            video_target: Some("camera-preview".to_string()),
            ..Attrs::default()
        },
        frame(0.0, 0.0, 800.0, 600.0),
    );
    let top = element(
        top_id,
        ElementKind::El,
        solid_fill_attrs((16, 24, 34)),
        frame(0.0, 0.0, 800.0, 80.0),
    );
    let mut bottom = element(
        bottom_id,
        ElementKind::El,
        solid_fill_attrs((20, 28, 38)),
        frame(0.0, 360.0, 800.0, 240.0),
    );
    let header_id = NodeId::from_u64(10_004);
    let button_id = NodeId::from_u64(10_005);
    let footer_id = NodeId::from_u64(10_006);
    let static_colors = [0xA02020FF, 0x20A020FF, 0x2020A0FF];
    let header = element(
        header_id,
        ElementKind::El,
        color_attrs(static_colors[0]),
        frame(360.0, 372.0, 120.0, 18.0),
    );
    let button = element(
        button_id,
        ElementKind::El,
        color_attrs(static_colors[1]),
        frame(500.0, 440.0, 90.0, 28.0),
    );
    let footer = element(
        footer_id,
        ElementKind::El,
        color_attrs(static_colors[2]),
        frame(620.0, 570.0, 120.0, 18.0),
    );
    bottom.children.push(header_id);

    let mut elements = vec![root, video, top, header, button, footer];
    let mut slider_ids = Vec::new();
    let mut track_ids = Vec::new();
    let mut fill_ids = Vec::new();
    let mut track_colors = Vec::new();
    let mut fill_colors = Vec::new();
    let mut thumb_colors = Vec::new();
    for index in 0..6u64 {
        let slider_id = NodeId::from_u64(11_000 + index * 10);
        let track_id = NodeId::from_u64(11_001 + index * 10);
        let fill_id = NodeId::from_u64(11_002 + index * 10);
        let thumb_id = NodeId::from_u64(11_003 + index * 10);
        let y = 380.0 + index as f32 * 30.0;
        let track_color = 0x2000_00FFu32.wrapping_add((index as u32) << 16);
        let fill_color = 0x0020_00FFu32.wrapping_add((index as u32) << 8);
        let thumb_color = 0x0000_20FFu32.wrapping_add(index as u32);
        let mut slider = element(
            slider_id,
            ElementKind::Slider,
            Attrs::default(),
            frame(40.0, y, 300.0, 24.0),
        );
        slider.children = vec![track_id, fill_id, thumb_id];
        let track = element(
            track_id,
            ElementKind::El,
            color_attrs(track_color),
            frame(40.0, y + 8.0, 300.0, 8.0),
        );
        let fill = element(
            fill_id,
            ElementKind::El,
            color_attrs(fill_color),
            frame(40.0, y + 8.0, 180.0, 8.0),
        );
        let thumb = element(
            thumb_id,
            ElementKind::El,
            color_attrs(thumb_color),
            frame(210.0, y + 2.0, 20.0, 20.0),
        );
        bottom.children.push(slider_id);
        if index == 2 {
            bottom.children.push(button_id);
        }
        elements.extend([slider, track, fill, thumb]);
        slider_ids.push(slider_id);
        track_ids.push(track_id);
        fill_ids.push(fill_id);
        track_colors.push(track_color);
        fill_colors.push(fill_color);
        thumb_colors.push(thumb_color);
    }
    bottom.children.push(footer_id);
    elements.push(bottom);

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    elements
        .into_iter()
        .for_each(|element| tree.insert(element));
    CameraFixture {
        tree,
        root_id,
        video_id,
        top_id,
        bottom_id,
        slider_ids,
        track_ids,
        fill_ids,
        track_colors,
        fill_colors,
        thumb_colors,
        static_colors,
    }
}

fn assert_camera_descriptor(fixture: &CameraFixture, descriptor: &[LayerDescriptor]) {
    assert_eq!(descriptor.len(), 9, "{descriptor:#?}");
    assert_eq!(
        descriptor
            .iter()
            .map(|layer| layer.reason)
            .collect::<Vec<_>>(),
        vec![
            PaintLayerReason::Root,
            PaintLayerReason::Nearby,
            PaintLayerReason::Nearby,
            PaintLayerReason::SliderValue,
            PaintLayerReason::SliderValue,
            PaintLayerReason::SliderValue,
            PaintLayerReason::SliderValue,
            PaintLayerReason::SliderValue,
            PaintLayerReason::SliderValue,
        ]
    );
    assert_eq!(descriptor[0].stable_id, fixture.root_id.to_wire_u64());
    assert_eq!(descriptor[0].policy, PaintLayerPolicy::DirectOnly);
    assert!(descriptor[0].owns_video);
    assert_eq!(descriptor[1].stable_id, fixture.top_id.to_wire_u64());
    assert_eq!(descriptor[2].stable_id, fixture.bottom_id.to_wire_u64());
    assert_eq!(descriptor[1].policy, PaintLayerPolicy::Cacheable);
    assert_eq!(descriptor[2].policy, PaintLayerPolicy::Cacheable);
    assert_eq!(
        descriptor[2].own_run_colors.len(),
        9,
        "bottom runs: {:?}",
        descriptor[2].own_run_colors
    );
    assert!(descriptor[2].own_run_colors[0].contains(&fixture.static_colors[0]));
    assert!(descriptor[2].own_run_colors[4].contains(&fixture.static_colors[1]));
    assert!(descriptor[2].own_run_colors[8].contains(&fixture.static_colors[2]));
    assert_eq!(
        descriptor[1].parent,
        Some((fixture.root_id.to_wire_u64(), PaintLayerReason::Root))
    );
    assert_eq!(descriptor[2].parent, descriptor[1].parent);

    fixture
        .slider_ids
        .iter()
        .enumerate()
        .for_each(|(index, id)| {
            let layer = &descriptor[index + 3];
            assert_eq!(layer.stable_id, id.to_wire_u64());
            assert_eq!(layer.policy, PaintLayerPolicy::Cacheable);
            assert_eq!(
                layer.parent,
                Some((fixture.bottom_id.to_wire_u64(), PaintLayerReason::Nearby))
            );
            assert!(layer.own_colors.contains(&fixture.fill_colors[index]));
            assert!(layer.own_colors.contains(&fixture.thumb_colors[index]));
            assert!(!layer.own_colors.contains(&fixture.track_colors[index]));
            assert!(
                descriptor[2]
                    .own_colors
                    .contains(&fixture.track_colors[index])
            );
            let run_index = [1, 2, 3, 5, 6, 7][index];
            assert!(descriptor[2].own_run_colors[run_index].contains(&fixture.track_colors[index]));
        });
}

fn describe_scene(nodes: &[RenderNode]) -> Vec<LayerDescriptor> {
    let mut descriptors = Vec::new();
    visit_scene_nodes(nodes, None, &mut descriptors);
    descriptors
}

fn visit_scene_nodes(
    nodes: &[RenderNode],
    parent: Option<(u64, PaintLayerReason)>,
    descriptors: &mut Vec<LayerDescriptor>,
) {
    nodes.iter().for_each(|node| match node {
        RenderNode::ShadowPass { children }
        | RenderNode::Clip { children, .. }
        | RenderNode::RelaxedClip { children, .. }
        | RenderNode::Transform { children, .. }
        | RenderNode::Alpha { children, .. } => visit_scene_nodes(children, parent, descriptors),
        RenderNode::PaintLayer(layer) => visit_layer(layer, parent, descriptors),
        RenderNode::Primitive(_) => {}
    });
}

fn visit_layer(
    layer: &RenderPaintLayer,
    parent: Option<(u64, PaintLayerReason)>,
    descriptors: &mut Vec<LayerDescriptor>,
) {
    let own_nodes = layer.own_render_nodes();
    let descriptor = LayerDescriptor {
        stable_id: layer.id.node_id,
        parent,
        reason: layer.id.role,
        policy: layer.policy,
        own_colors: primitive_colors(&own_nodes),
        own_run_colors: layer
            .own_runs()
            .into_iter()
            .map(|run| primitive_colors(&run.nodes))
            .collect(),
        owns_video: nodes_contain_video(&own_nodes),
    };
    descriptors.push(descriptor);
    visit_layer_content(
        &layer.content.nodes,
        Some((layer.id.node_id, layer.id.role)),
        descriptors,
    );
}

fn visit_layer_content(
    content: &[RenderPaintLayerContentNode],
    parent: Option<(u64, PaintLayerReason)>,
    descriptors: &mut Vec<LayerDescriptor>,
) {
    content.iter().for_each(|node| match node {
        RenderPaintLayerContentNode::Own(_) => {}
        RenderPaintLayerContentNode::Child(layer) => visit_layer(layer, parent, descriptors),
        RenderPaintLayerContentNode::ShadowPass { children }
        | RenderPaintLayerContentNode::Clip { children, .. }
        | RenderPaintLayerContentNode::RelaxedClip { children, .. }
        | RenderPaintLayerContentNode::Transform { children, .. }
        | RenderPaintLayerContentNode::Alpha { children, .. } => {
            visit_layer_content(children, parent, descriptors)
        }
    });
}

fn layer_generations(nodes: &[RenderNode]) -> BTreeMap<(u64, PaintLayerReason), u64> {
    describe_layers(nodes)
        .into_iter()
        .map(|layer| ((layer.id.node_id, layer.id.role), layer.content_generation))
        .collect()
}

fn describe_layers(nodes: &[RenderNode]) -> Vec<&RenderPaintLayer> {
    nodes
        .iter()
        .flat_map(|node| match node {
            RenderNode::ShadowPass { children }
            | RenderNode::Clip { children, .. }
            | RenderNode::RelaxedClip { children, .. }
            | RenderNode::Transform { children, .. }
            | RenderNode::Alpha { children, .. } => describe_layers(children),
            RenderNode::PaintLayer(layer) => {
                let mut layers = vec![layer];
                layers.extend(layer.descendant_layers());
                layers
            }
            RenderNode::Primitive(_) => Vec::new(),
        })
        .collect()
}

fn transform_for_layer_reason(
    nodes: &[RenderNode],
    reason: PaintLayerReason,
) -> Option<crate::tree::transform::Affine2> {
    fn first_transform(
        nodes: &[RenderNode],
        transform: crate::tree::transform::Affine2,
    ) -> Option<crate::tree::transform::Affine2> {
        nodes.iter().find_map(|node| match node {
            RenderNode::Transform {
                transform: local, ..
            } => Some(transform.then(*local)),
            RenderNode::ShadowPass { children }
            | RenderNode::Clip { children, .. }
            | RenderNode::RelaxedClip { children, .. }
            | RenderNode::Alpha { children, .. } => first_transform(children, transform),
            RenderNode::PaintLayer(layer) => first_transform(&layer.content_nodes(), transform),
            RenderNode::Primitive(_) => None,
        })
    }

    fn visit(
        nodes: &[RenderNode],
        reason: PaintLayerReason,
        transform: crate::tree::transform::Affine2,
    ) -> Option<crate::tree::transform::Affine2> {
        nodes.iter().find_map(|node| match node {
            RenderNode::Transform {
                transform: local,
                children,
            } => visit(children, reason, transform.then(*local)),
            RenderNode::ShadowPass { children }
            | RenderNode::Clip { children, .. }
            | RenderNode::RelaxedClip { children, .. }
            | RenderNode::Alpha { children, .. } => visit(children, reason, transform),
            RenderNode::PaintLayer(layer) if layer.id.role == reason => {
                first_transform(&layer.content_nodes(), transform).or(Some(transform))
            }
            RenderNode::PaintLayer(layer) => visit(&layer.content_nodes(), reason, transform),
            RenderNode::Primitive(_) => None,
        })
    }

    visit(nodes, reason, crate::tree::transform::Affine2::identity())
}

fn primitive_colors(nodes: &[RenderNode]) -> BTreeSet<u32> {
    let mut colors = BTreeSet::new();
    visit_primitives(nodes, &mut |primitive| match primitive {
        DrawPrimitive::Rect(_, _, _, _, color)
        | DrawPrimitive::RoundedRect(_, _, _, _, _, color) => {
            colors.insert(*color);
        }
        _ => {}
    });
    colors
}

fn nodes_contain_video(nodes: &[RenderNode]) -> bool {
    let mut found = false;
    visit_primitives(nodes, &mut |primitive| {
        found |= matches!(primitive, DrawPrimitive::Video(..))
    });
    found
}

fn visit_primitives(nodes: &[RenderNode], visitor: &mut impl FnMut(&DrawPrimitive)) {
    nodes.iter().for_each(|node| match node {
        RenderNode::ShadowPass { children }
        | RenderNode::Clip { children, .. }
        | RenderNode::RelaxedClip { children, .. }
        | RenderNode::Transform { children, .. }
        | RenderNode::Alpha { children, .. } => visit_primitives(children, visitor),
        RenderNode::PaintLayer(_) => {}
        RenderNode::Primitive(primitive) => visitor(primitive),
    });
}

fn element(id: NodeId, kind: ElementKind, attrs: Attrs, frame: Frame) -> Element {
    let mut element = Element::with_attrs(id, kind, Vec::new(), attrs);
    element.layout.frame = Some(frame);
    element
}

fn frame(x: f32, y: f32, width: f32, height: f32) -> Frame {
    Frame {
        x,
        y,
        width,
        height,
        content_width: width,
        content_height: height,
    }
}

fn color_attrs(color: u32) -> Attrs {
    Attrs {
        background: Some(Background::Color(Color::Rgba {
            r: (color >> 24) as u8,
            g: (color >> 16) as u8,
            b: (color >> 8) as u8,
            a: color as u8,
        })),
        ..Attrs::default()
    }
}
