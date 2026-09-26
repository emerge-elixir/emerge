//! Accelerated clock/lifecycle coverage for the Goat title slide's looping SVG.
//! This does not simulate hours of GPU allocations or KMS buffer recycling.
use super::*;
use crate::{
    render_scene::RenderScene,
    renderer::{RenderFrame, RenderState, SceneRenderer},
    tree::{
        animation::{AnimationCurve, AnimationRepeat, AnimationSpec},
        attrs::{Attrs, Background, Color, ImageSource, Length, SolidColor},
        element::{Element, ElementKind, NearbyMount, NearbySlot},
    },
};

const ROOT: NodeId = NodeId(1);
const TITLE: NodeId = NodeId(2);
const SVG: NodeId = NodeId(3);
const OVERLAY: NodeId = NodeId(4);
const OTHER_ANIMATION: NodeId = NodeId(5);
const PERIOD_MS: u64 = 18_000;

fn title_gradient() -> AnimationSpec {
    let frames = 180;
    AnimationSpec {
        keyframes: (-1..=1)
            .flat_map(|sign| {
                (0..=frames).map(move |position| Attrs {
                    svg_color: Some(Color::Gradient {
                        colors: (0..=frames)
                            .map(|stop| SolidColor::Rgba {
                                r: 255,
                                g: 255,
                                b: 255,
                                a: match position - stop - 1 {
                                    -2 | 0 => 230,
                                    -1 => 255,
                                    1 => 179,
                                    2 => 128,
                                    3 => 77,
                                    4 => 51,
                                    5 => 26,
                                    _ => 0,
                                },
                            })
                            .collect(),
                        angle: 60.0 * sign as f64 * (1.0 - position as f64 / 120.0).max(0.0),
                    }),
                    ..Attrs::default()
                })
            })
            .collect(),
        duration_ms: PERIOD_MS as f64,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    }
}

fn tree(with_title: bool) -> ElementTree {
    let size = || Attrs {
        width: Some(Length::Px(64.0)),
        height: Some(Length::Px(32.0)),
        ..Attrs::default()
    };
    let image = || Attrs {
        image_src: Some(ImageSource::Id("long-running-title".into())),
        ..size()
    };
    let mut tree = ElementTree::new();
    tree.insert(Element::with_attrs(
        ROOT,
        ElementKind::Column,
        vec![],
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
            ..Attrs::default()
        },
    ));
    tree.insert(Element::with_attrs(
        OTHER_ANIMATION,
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(Length::Px(8.0)),
            height: Some(Length::Px(8.0)),
            background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 255 })),
            animate: Some(AnimationSpec {
                keyframes: [0.0, 40.0, 0.0]
                    .map(|x| Attrs {
                        move_x: Some(x),
                        ..Attrs::default()
                    })
                    .to_vec(),
                duration_ms: PERIOD_MS as f64,
                curve: AnimationCurve::Linear,
                repeat: AnimationRepeat::Loop,
            }),
            ..Attrs::default()
        },
    ));
    if with_title {
        tree.insert(Element::with_attrs(TITLE, ElementKind::El, vec![], size()));
        tree.insert(Element::with_attrs(
            SVG,
            ElementKind::Image,
            vec![],
            image(),
        ));
        tree.insert(Element::with_attrs(
            OVERLAY,
            ElementKind::Image,
            vec![],
            Attrs {
                animate: Some(title_gradient()),
                ..image()
            },
        ));
        tree.set_children(&TITLE, vec![SVG]).unwrap();
        tree.set_nearby_mounts(
            &TITLE,
            vec![NearbyMount {
                id: OVERLAY,
                slot: NearbySlot::InFront,
            }],
        )
        .unwrap();
    }
    tree.set_children(
        &ROOT,
        if with_title {
            vec![TITLE, OTHER_ANIMATION]
        } else {
            vec![OTHER_ANIMATION]
        },
    )
    .unwrap();
    tree.set_root_id(ROOT);
    let revision = tree.bump_revision();
    tree.stamp_all_mounted_at_revision(revision);
    tree
}

fn pulse(now: Instant) -> TreeMsg {
    TreeMsg::AnimationPulse {
        presented_at: now,
        predicted_next_present_at: now,
        trace: None,
    }
}

fn update(engine: &mut TreeUpdateEngine, messages: Vec<TreeMsg>) -> RenderScene {
    let effect = engine
        .process_messages(
            messages,
            TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
        )
        .unwrap();
    let TreeUpdateEffect::Layout { output, .. } = effect else {
        panic!("expected animated layout")
    };
    assert!(output.animations_active);
    output.scene
}

fn pixels(renderer: &mut SceneRenderer, scene: RenderScene) -> Vec<u8> {
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    renderer.render(
        &mut RenderFrame::new(&mut surface, None),
        &RenderState::new(scene, skia_safe::Color::BLACK, 1, true),
    );
    let info = skia_safe::ImageInfo::new(
        (64, 64),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut pixels = vec![0; 64 * 64 * 4];
    assert!(surface.read_pixels(&info, &mut pixels, 64 * 4, (0, 0)));
    pixels
}

#[test]
fn looping_svg_is_periodic_after_days_and_cannot_reappear_after_removal() {
    let assets = assets::AssetRuntime::new();
    let _context = assets.enter();
    let svg = resvg::usvg::Tree::from_str(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="32"><path d="M0 0H64L32 32Z" fill="#804020"/></svg>"##,
        &resvg::usvg::Options::default(),
    ).unwrap();
    crate::renderer::insert_vector_asset("long-running-title", svg).unwrap();
    let start = Instant::now();
    let mut engine = TreeUpdateEngine::new(tree(true), 64, 64);
    let mut renderer = SceneRenderer::new();
    renderer.enable_paint_layer_cache_for_benchmark();
    let offsets = [0, 16, 1_000, 6_000, 12_000, PERIOD_MS - 1];
    let reference: Vec<_> = offsets
        .iter()
        .map(|ms| {
            let scene = update(&mut engine, vec![pulse(start + Duration::from_millis(*ms))]);
            assert_eq!(scene.summary().images, 2);
            pixels(&mut renderer, scene)
        })
        .collect();
    assert!(reference.windows(2).any(|pair| pair[0] != pair[1]));

    for hours in [12, 24, 72] {
        for (ms, expected) in offsets.iter().zip(&reference) {
            let now = start + Duration::from_secs(hours * 3600) + Duration::from_millis(*ms);
            let scene = update(&mut engine, vec![pulse(now)]);
            assert_eq!(scene.summary().images, 2);
            assert_eq!(
                pixels(&mut renderer, scene),
                *expected,
                "hours={hours}, offset={ms}"
            );
            assert_eq!(engine.animation_runtime.active_node_ids().len(), 2);
        }
    }

    // Remove the title host through the real patch decoder, including its nearby SVG.
    let mut bytes = vec![4];
    bytes.extend_from_slice(&TITLE.to_wire_u64().to_be_bytes());
    let end = start + Duration::from_secs(72 * 3600) + Duration::from_millis(PERIOD_MS);
    let removed = update(
        &mut engine,
        vec![
            TreeMsg::PatchTree {
                bytes,
                submitted_at: None,
            },
            pulse(end),
        ],
    );
    assert_eq!(removed.summary().images, 0);
    pixels(&mut renderer, removed);
    for id in [TITLE, SVG, OVERLAY] {
        assert!(engine.tree().get(&id).is_none());
    }
    assert_eq!(
        engine.animation_runtime.active_node_ids(),
        vec![OTHER_ANIMATION]
    );

    // Keep the same renderer/cache. Subsequent animation frames must match a fresh
    // scene that has never contained the title, not just omit SVGs from the tree.
    let mut clean = TreeUpdateEngine::new(tree(false), 64, 64);
    for ms in (0..2 * PERIOD_MS).step_by(300) {
        let now = end + Duration::from_millis(ms);
        let actual = update(&mut engine, vec![pulse(now)]);
        let expected = update(&mut clean, vec![pulse(now)]);
        assert_eq!(actual.summary().images, 0);
        assert_eq!(
            pixels(&mut renderer, actual),
            pixels(&mut SceneRenderer::new(), expected)
        );
    }
}
