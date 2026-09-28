//! Real raster and hit-region checks, including retained renderer replay.
use super::support::{Driver, Event, Mode};
use super::*;
use crate::render_scene::RenderScene;
use crate::renderer::{RenderFrame, RenderState, SceneRenderer};
use crate::tree::animation::change::{ChangePolicy, Field};
use crate::tree::patch::{Patch, apply_patches};

pub(super) fn raster(
    renderer: &mut SceneRenderer,
    scene: RenderScene,
    width: u32,
    height: u32,
    version: u64,
) -> Vec<u8> {
    let info = skia_safe::ImageInfo::new(
        (width as i32, height as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut surface = skia_safe::surfaces::raster(&info, None, None).unwrap();
    renderer.render(
        &mut RenderFrame::new(&mut surface, None),
        &RenderState::new(scene, skia_safe::Color::TRANSPARENT, version, false),
    );
    let mut pixels = vec![0; (width * height * 4) as usize];
    assert!(surface.read_pixels(&info, &mut pixels[..], width as usize * 4, (0, 0)));
    pixels
}
fn axis_length(attrs: &mut Attrs, axis: Axis, length: Length) {
    match axis {
        Axis::Width => attrs.width = Some(length),
        Axis::Height => attrs.height = Some(length),
    }
}
fn visual_tree(a: Length, b: Length, axis: Axis, owner: u8) -> ElementTree {
    let mut tree = fixture(a.clone(), b.clone(), axis);
    let root = tree.get_mut(&id(1)).unwrap();
    axis_length(
        &mut root.spec.declared,
        if axis == Axis::Width {
            Axis::Height
        } else {
            Axis::Width
        },
        Length::Px(60.0),
    );
    let node = tree.get_mut(&id(2)).unwrap();
    node.spec.declared.background = Some(Background::Color(Color::Rgba {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    }));
    node.spec.declared.on_click = Some(true);
    axis_length(&mut node.spec.declared, axis, a.clone());
    axis_length(
        &mut node.spec.declared,
        if axis == Axis::Width {
            Axis::Height
        } else {
            Axis::Width
        },
        Length::Fill,
    );
    if owner == 1 {
        node.lifecycle.mounted_at_revision = 1;
        node.spec.declared.animate_enter = node.spec.declared.animate.take();
        axis_length(&mut node.spec.declared, axis, b);
    } else if owner == 2 {
        node.spec.declared.animate = None;
        node.spec.declared.animate_change = Some(Arc::new(vec![ChangePolicy {
            field: if axis == Axis::Width {
                Field::Width
            } else {
                Field::Height
            },
            duration_ms: 1000.0,
            curve: AnimationCurve::Linear,
        }]));
    } else if owner == 3 {
        node.spec.declared.animate_exit = node.spec.declared.animate.take();
    }
    let peer = tree.get_mut(&id(3)).unwrap();
    peer.spec.declared.background = Some(Background::Color(Color::Rgba {
        r: 0,
        g: 0,
        b: 255,
        a: 255,
    }));
    peer.spec.declared.on_click = Some(true);
    let mut content = Attrs {
        width: Some(Length::Px(20.0)),
        height: Some(Length::Px(20.0)),
        ..Default::default()
    };
    axis_length(&mut content, axis, Length::Px(80.0));
    tree.insert(Element::with_attrs(id(4), ElementKind::El, vec![], content));
    tree.set_children(&id(2), vec![id(4)]).unwrap();
    tree
}
#[test]
fn all_supported_length_pairs_and_owners_paint_real_geometry_and_replay_retained_scenes() {
    let lengths = [
        (Length::Px(40.0), 40.0),
        (Length::Content, 80.0),
        (Length::Fill, 300.0),
        (Length::FillWeighted(3.0), 450.0),
    ];
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for owner in 0..4 {
                for scale in [0.5, 1.0, 2.0] {
                    for (a, from) in &lengths {
                        for (b, to) in &lengths {
                            let primary = (600.0 * scale) as u32;
                            let cross = (60.0 * scale) as u32;
                            let (width, height) = if axis == Axis::Width {
                                (primary, cross)
                            } else {
                                (cross, primary)
                            };
                            let mut d = Driver::new(
                                visual_tree(a.clone(), b.clone(), axis, owner),
                                Instant::now(),
                                mode,
                            );
                            assert!(
                                d.step(
                                    0,
                                    vec![Event::Viewport {
                                        width: width as f32,
                                        height: height as f32,
                                        scale
                                    }]
                                )
                                .result
                                .is_ok()
                            );
                            if owner == 2 {
                                let mut attrs = d.attrs(id(2));
                                axis_length(&mut attrs, axis, b.clone());
                                assert!(
                                    d.step(0, vec![Event::Attrs(id(2), Box::new(attrs))])
                                        .result
                                        .is_ok()
                                );
                            }
                            if owner == 3 {
                                apply_patches(&mut d.tree, vec![Patch::Remove { id: id(2) }])
                                    .unwrap();
                            }
                            let mut retained = SceneRenderer::new();
                            let mut first = None;
                            for (frame, us) in [0, 250_000, 500_000, 750_000, 1_000_000]
                                .into_iter()
                                .enumerate()
                            {
                                let out = d.step(us, vec![]);
                                assert!(
                                    out.result.is_ok(),
                                    "axis={axis:?} owner={owner} {a:?}->{b:?} at {us}: {:?}",
                                    out.result
                                );
                                let output = out.published_after.unwrap().output;
                                let expected =
                                    (from + (to - from) * (us as f32 / 1_000_000.0)) * scale;
                                let pixels = raster(
                                    &mut retained,
                                    output.scene.clone(),
                                    width,
                                    height,
                                    frame as u64 + 1,
                                );
                                assert_eq!(
                                    pixels,
                                    raster(
                                        &mut SceneRenderer::new(),
                                        output.scene.clone(),
                                        width,
                                        height,
                                        1
                                    )
                                );
                                for n in 0..primary {
                                    if (n as f32 + 1.0) > expected && (n as f32) < expected {
                                        continue;
                                    }
                                    let color = if (n as f32) < expected {
                                        [255, 0, 0, 255]
                                    } else {
                                        [0, 0, 255, 255]
                                    };
                                    let (x, y) = if axis == Axis::Width {
                                        (n, cross / 2)
                                    } else {
                                        (cross / 2, n)
                                    };
                                    assert_eq!(
                                        pixels[((y * width + x) * 4) as usize..][..4],
                                        color,
                                        "axis={axis:?} owner={owner} {a:?}->{b:?} at {us}, pixel={n}, extent={expected}"
                                    );
                                }
                                for (position, target) in [
                                    (expected / 2.0, id(2)),
                                    ((expected + primary as f32) / 2.0, id(3)),
                                ] {
                                    let (x, y) = if axis == Axis::Width {
                                        (position, cross as f32 / 2.0)
                                    } else {
                                        (cross as f32 / 2.0, position)
                                    };
                                    let click = crate::input::InputEvent::CursorButton {
                                        button: "left".into(),
                                        action: 1,
                                        mods: 0,
                                        x,
                                        y,
                                    };
                                    let hit = output
                                        .event_rebuild
                                        .base_registry
                                        .view()
                                        .iter_precedence()
                                        .any(|listener| {
                                            listener.element_id == Some(target)
                                                && listener.matcher.matches(&click)
                                        });
                                    assert_eq!(hit, owner != 3 || target == id(3));
                                }
                                if frame == 0 {
                                    first = Some((output.scene.clone(), pixels));
                                }
                            }
                            let (scene, pixels) = first.unwrap();
                            assert_eq!(raster(&mut retained, scene, width, height, 6), pixels);
                            if owner == 3 {
                                assert!(d.step(1_000_001, vec![]).result.is_ok());
                            }
                            assert_eq!(
                                d.tree.length_runtime.is_some(),
                                owner == 2 && matches!(b, Length::Content),
                                "axis={axis:?} owner={owner} {a:?}->{b:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn animated_scroll_viewports_clip_pixels_and_hits_and_clamp_offsets_in_real_layout() {
    use crate::tree::attrs::{AlignX, AlignY};
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for scale in [0.5, 1.0, 2.0] {
                let mut tree = visual_tree(Length::Px(40.0), Length::Px(120.0), axis, 0);
                let node = tree.get_mut(&id(2)).unwrap();
                if axis == Axis::Width {
                    node.spec.declared.scrollbar_x = Some(true);
                } else {
                    node.spec.declared.scrollbar_y = Some(true);
                }
                let child = tree.get_mut(&id(4)).unwrap();
                child.spec.declared.background = Some(Background::Color(Color::Rgba {
                    r: 0,
                    g: 255,
                    b: 0,
                    a: 255,
                }));
                child.spec.declared.on_click = Some(true);
                child.spec.declared.align_x = Some(AlignX::Left);
                child.spec.declared.align_y = Some(AlignY::Top);
                let primary = (600.0 * scale) as u32;
                let cross = (60.0 * scale) as u32;
                let (width, height) = if axis == Axis::Width {
                    (primary, cross)
                } else {
                    (cross, primary)
                };
                let mut d = Driver::new(tree, Instant::now(), mode);
                assert!(
                    d.step(
                        0,
                        vec![Event::Viewport {
                            width: width as f32,
                            height: height as f32,
                            scale
                        }]
                    )
                    .result
                    .is_ok()
                );
                let invalidation = d.tree.apply_scroll(
                    &id(2),
                    if axis == Axis::Width {
                        -15.0 * scale
                    } else {
                        0.0
                    },
                    if axis == Axis::Height {
                        -15.0 * scale
                    } else {
                        0.0
                    },
                );
                d.tree.pending_patch_effects.invalidation.add(invalidation);
                let mut renderer = SceneRenderer::new();
                for (index, us) in [0, 250_000, 500_000, 750_000, 1_000_000]
                    .into_iter()
                    .enumerate()
                {
                    let out = d.step(us, vec![]);
                    assert!(out.result.is_ok(), "{:?}", out.result);
                    let viewport = (40.0 + 80.0 * us as f32 / 1_000_000.0) * scale;
                    let node = d.tree.get(&id(2)).unwrap();
                    let (offset, range) = if axis == Axis::Width {
                        (node.layout.scroll_x, node.layout.scroll_x_max)
                    } else {
                        (node.layout.scroll_y, node.layout.scroll_y_max)
                    };
                    close(range, (80.0 * scale - viewport).max(0.0));
                    close(offset, (15.0 * scale).min(range));
                    let green = (80.0 * scale - offset).min(viewport);
                    let output = out.published_after.unwrap().output;
                    let pixels = raster(
                        &mut renderer,
                        output.scene.clone(),
                        width,
                        height,
                        index as u64 + 1,
                    );
                    assert_eq!(
                        pixels,
                        raster(
                            &mut SceneRenderer::new(),
                            output.scene.clone(),
                            width,
                            height,
                            1
                        )
                    );
                    for n in 0..primary {
                        if [viewport, green]
                            .iter()
                            .any(|edge| n as f32 + 1.0 > *edge && (n as f32) < *edge)
                        {
                            continue;
                        }
                        let (x, y) = if axis == Axis::Width {
                            (n, (5.0 * scale) as u32)
                        } else {
                            ((5.0 * scale) as u32, n)
                        };
                        let color = if (n as f32) < green {
                            [0, 255, 0, 255]
                        } else if (n as f32) < viewport {
                            [255, 0, 0, 255]
                        } else {
                            [0, 0, 255, 255]
                        };
                        assert_eq!(
                            pixels[((y * width + x) * 4) as usize..][..4],
                            color,
                            "{axis:?} scale={scale} at={us} pixel={n}"
                        );
                        let click = crate::input::InputEvent::CursorButton {
                            button: "left".into(),
                            action: 1,
                            mods: 0,
                            x: x as f32 + 0.5,
                            y: y as f32 + 0.5,
                        };
                        let hit = output
                            .event_rebuild
                            .base_registry
                            .view()
                            .iter_precedence()
                            .any(|listener| {
                                listener.element_id == Some(id(4))
                                    && listener.matcher.matches(&click)
                            });
                        assert_eq!(
                            hit,
                            (n as f32) < green,
                            "clipped child hit {axis:?} {us} {n}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn font_only_and_combined_deadline_inputs_preserve_old_rasters_and_match_fresh_native_output() {
    use super::super::inspection::QueryKind;
    use super::support::Injection;
    use crate::tree::attrs::{Font, Padding};
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for combined in [false, true] {
            let assets = crate::assets::AssetRuntime::new();
            let _assets = assets.enter();
            let mut tree = visual_tree(Length::Fill, Length::Fill, Axis::Width, 0);
            tree.set_children(&id(2), vec![]).unwrap();
            tree.remove_node(&id(4));
            let node = tree.get_mut(&id(2)).unwrap();
            node.spec.kind = ElementKind::Text;
            node.spec.declared.font = Some(Font::String("animation-raster-font".into()));
            node.spec.declared.font_size = Some(24.0);
            node.spec.declared.content = Some("Wavy raster".into());
            node.spec.declared.font_color = Some(Color::Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            });
            let mut d = Driver::new(tree, Instant::now(), mode);
            assert!(
                d.step(
                    0,
                    vec![Event::Viewport {
                        width: 600.0,
                        height: 60.0,
                        scale: 1.0
                    }]
                )
                .result
                .is_ok()
            );
            let before = d.step(999_999, vec![]).published_after.unwrap();
            let mut renderer = SceneRenderer::new();
            let old_pixels = raster(&mut renderer, before.output.scene.clone(), 600, 60, 1);
            let bytes = std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../priv/test_assets/Lobster-Regular.ttf"),
            )
            .unwrap();
            crate::services::load_font_bytes(&assets, "animation-raster-font", 400, false, &bytes)
                .unwrap();
            let (events, width, height, scale, kind) = if combined {
                let mut root = d.attrs(id(1));
                root.width = Some(Length::Fill);
                root.padding = Some(Padding::Uniform(10.0));
                (
                    vec![
                        Event::Attrs(id(1), Box::new(root)),
                        Event::Viewport {
                            width: 800.0,
                            height: 120.0,
                            scale: 2.0,
                        },
                    ],
                    800,
                    120,
                    2.0,
                    QueryKind::PreviousModel,
                )
            } else {
                (vec![], 600, 60, 1.0, QueryKind::PreviousInputs)
            };
            let failed = d.attempt(1_000_000, events, Injection::Query(kind));
            assert!(failed.result.is_err());
            failed.published_after.unwrap().assert_same(&before);
            assert_eq!(
                raster(&mut renderer, before.output.scene.clone(), 600, 60, 2),
                old_pixels
            );
            let out = d.step(1_000_000, vec![]);
            assert!(out.result.is_ok(), "{:?}", out.result);
            let output = out.published_after.unwrap().output;
            let pixels = raster(&mut renderer, output.scene.clone(), width, height, 3);
            if !combined {
                assert_eq!(
                    d.tree.get(&id(2)).unwrap().layout.frame,
                    before.pose[&id(2)].frame
                );
                assert_ne!(
                    pixels, old_pixels,
                    "same-sized boxes must not reuse obsolete glyph rasters"
                );
            }
            // These native fixtures intentionally have no attrs_raw wire payload.
            // Build a fresh declared model, with no frames, samples or render caches.
            let mut fresh = ElementTree::new();
            for node in d.tree.iter_nodes() {
                let mut attrs = node.spec.declared.clone();
                attrs.animate = None;
                fresh.insert(Element::with_attrs(node.id, node.spec.kind, vec![], attrs));
            }
            for node in d.tree.iter_nodes() {
                fresh
                    .set_children(&node.id, d.tree.child_ids(&node.id))
                    .unwrap();
            }
            fresh.set_root_id(d.tree.root_id().unwrap());
            let fresh = crate::tree::layout::layout_and_refresh_default(
                &mut fresh,
                Constraint::new(width as f32, height as f32),
                scale,
            );
            assert!(
                pixels == raster(&mut SceneRenderer::new(), fresh.scene, width, height, 1),
                "fresh native raster mismatch: {mode:?}, combined={combined}"
            );
            assert_eq!(
                raster(&mut renderer, before.output.scene.clone(), 600, 60, 4),
                old_pixels
            );
            assert!(d.tree.length_runtime.is_none());
        }
    }
}

#[test]
fn equal_size_image_replacement_changes_rasters_without_restarting_dimension_clocks() {
    use crate::tree::attrs::ImageSource;
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let assets = crate::assets::AssetRuntime::new();
        let _assets = assets.enter();
        for (name, color) in [("animation-red", "red"), ("animation-green", "lime")] {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="20"><rect width="80" height="20" fill="{color}"/></svg>"#
            );
            crate::assets::register_vector_asset(
                name,
                resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).unwrap(),
            )
            .unwrap();
        }
        let mut tree = visual_tree(Length::Fill, Length::Fill, Axis::Width, 0);
        tree.set_children(&id(2), vec![]).unwrap();
        tree.remove_node(&id(4));
        let node = tree.get_mut(&id(2)).unwrap();
        node.spec.kind = ElementKind::Image;
        node.spec.declared.image_src = Some(ImageSource::Id("animation-red".into()));
        let mut d = Driver::new(tree, Instant::now(), mode);
        d.step(
            0,
            vec![Event::Viewport {
                width: 600.0,
                height: 60.0,
                scale: 1.0,
            }],
        );
        let before = d.step(250_000, vec![]).published_after.unwrap();
        let entry = d.runtime.animate_entries.get(&id(2)).unwrap();
        let identity = (entry.generation, entry.clock.started_at);
        let mut renderer = SceneRenderer::new();
        let old_pixels = raster(&mut renderer, before.output.scene.clone(), 600, 60, 1);
        let mut attrs = d.attrs(id(2));
        attrs.image_src = Some(ImageSource::Id("animation-green".into()));
        let out = d.step(500_000, vec![Event::Attrs(id(2), Box::new(attrs))]);
        assert!(out.result.is_ok(), "{:?}", out.result);
        let entry = d.runtime.animate_entries.get(&id(2)).unwrap();
        assert_eq!(identity, (entry.generation, entry.clock.started_at));
        assert_eq!(
            d.tree.get(&id(2)).unwrap().layout.frame,
            before.pose[&id(2)].frame
        );
        let scene = out.published_after.unwrap().output.scene.clone();
        let pixels = raster(&mut renderer, scene.clone(), 600, 60, 2);
        assert_eq!(&old_pixels[(30 * 600 + 150) * 4..][..4], &[255, 0, 0, 255]);
        assert_eq!(&pixels[(30 * 600 + 150) * 4..][..4], &[0, 255, 0, 255]);
        assert_eq!(pixels, raster(&mut SceneRenderer::new(), scene, 600, 60, 1));
        assert_eq!(
            old_pixels,
            raster(&mut renderer, before.output.scene.clone(), 600, 60, 3)
        );
        assert!(d.step(1_000_000, vec![]).result.is_ok());
        assert!(d.tree.length_runtime.is_none());
    }
}

#[test]
fn geometry_pulses_reuse_registry_eligibility_but_external_handler_edits_invalidate_it() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut d = Driver::new(
            visual_tree(Length::Px(40.0), Length::Fill, Axis::Width, 0),
            Instant::now(),
            mode,
        );
        assert!(d.step(0, vec![]).result.is_ok());
        let cold = d.tree.registry_affects_visit_count();
        assert_eq!(cold, d.tree.len() as u64);
        for us in [250_000, 500_000] {
            assert!(d.step(us, vec![]).result.is_ok());
            assert_eq!(d.tree.registry_affects_visit_count(), cold);
        }
        // Same-revision native mutation still invalidates derived eligibility.
        d.tree.get_mut(&id(2)).unwrap().spec.declared.on_click = Some(false);
        d.tree.mark_registry_refresh_dirty(&id(2));
        let out = d.step(750_000, vec![]);
        assert!(out.result.is_ok());
        let output = out.published_after.unwrap().output;
        assert!(output.event_rebuild_changed);
        assert!(
            !output
                .event_rebuild
                .base_registry
                .view()
                .iter_precedence()
                .any(|listener| listener.element_id == Some(id(2)))
        );
        let edited = d.tree.registry_affects_visit_count();
        assert!(edited > cold);
        let mut attrs = d.attrs(id(4));
        attrs.on_click = Some(true);
        let out = d.step(900_000, vec![Event::Attrs(id(4), Box::new(attrs))]);
        assert!(out.result.is_ok());
        let output = out.published_after.unwrap().output;
        assert!(output.event_rebuild_changed);
        assert!(
            output
                .event_rebuild
                .base_registry
                .view()
                .iter_precedence()
                .any(|listener| listener.element_id == Some(id(4)))
        );
        assert!(d.tree.registry_affects_visit_count() > edited);
    }
}

#[test]
fn registry_eligibility_cache_invalidates_runtime_and_nearby_topology_changes() {
    use crate::tree::element::{NearbyMount, NearbySlot};
    let mut tree = ElementTree::new();
    tree.insert(Element::with_attrs(
        id(1),
        ElementKind::El,
        vec![],
        Attrs::default(),
    ));
    tree.set_root_id(id(1));
    tree.refresh_registry_subtree_affects_cache();
    assert!(!tree.root_cached_subtree_affects_registry());
    let cold = tree.registry_affects_visit_count();
    tree.refresh_registry_subtree_affects_cache();
    assert_eq!(tree.registry_affects_visit_count(), cold);
    tree.get_mut(&id(1)).unwrap().runtime.focused_active = true;
    tree.refresh_registry_subtree_affects_cache();
    assert!(tree.root_cached_subtree_affects_registry());
    tree.get_mut(&id(1)).unwrap().runtime.focused_active = false;
    tree.refresh_registry_subtree_affects_cache();
    assert!(!tree.root_cached_subtree_affects_registry());
    tree.insert(Element::with_attrs(
        id(2),
        ElementKind::El,
        vec![],
        Attrs::default(),
    ));
    tree.set_nearby_mounts(
        &id(1),
        vec![NearbyMount {
            slot: NearbySlot::InFront,
            id: id(2),
        }],
    )
    .unwrap();
    tree.refresh_registry_subtree_affects_cache();
    assert!(tree.root_cached_subtree_affects_registry());
    tree.set_nearby_mounts(&id(1), vec![]).unwrap();
    tree.refresh_registry_subtree_affects_cache();
    assert!(!tree.root_cached_subtree_affects_registry());
}

#[test]
fn mixed_parent_reset_releases_all_owners_with_matching_pixels_and_hits() {
    use super::super::inspection::QueryKind;
    use super::support::Injection;
    use crate::input::InputEvent::CursorButton;
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for owner in 0..4 {
                for scale in [0.5, 1.0, 2.0] {
                    for independent in [false, true] {
                        let mut tree = visual_tree(Length::Px(40.0), Length::Fill, axis, owner);
                        let attrs = &mut tree.get_mut(&id(2)).unwrap().spec.declared;
                        match owner {
                            0 => attrs.animate.as_mut().unwrap().duration_ms = 2000.0,
                            1 => attrs.animate_enter.as_mut().unwrap().duration_ms = 2000.0,
                            2 => {
                                Arc::make_mut(attrs.animate_change.as_mut().unwrap())[0]
                                    .duration_ms = 2000.0
                            }
                            _ => attrs.animate_exit.as_mut().unwrap().duration_ms = 2000.0,
                        }
                        let mut parent = spec(Length::Px(200.0), Length::Fill, 2000.0, axis);
                        parent.repeat = AnimationRepeat::Loop;
                        tree.get_mut(&id(1)).unwrap().spec.declared.animate = Some(parent);
                        let (width, height) = if axis == Axis::Width {
                            ((600.0 * scale) as u32, (60.0 * scale) as u32)
                        } else {
                            ((60.0 * scale) as u32, (600.0 * scale) as u32)
                        };
                        if independent {
                            let bounds = Attrs {
                                width: Some(Length::Px(f64::from(width) / f64::from(scale))),
                                height: Some(Length::Px(f64::from(height) / f64::from(scale))),
                                ..Default::default()
                            };
                            let mut other = bounds.clone();
                            let mut clock = spec(Length::Px(100.0), Length::Fill, 3000.0, axis);
                            clock.repeat = AnimationRepeat::Loop;
                            other.animate = Some(clock);
                            tree.insert(Element::with_attrs(id(5), ElementKind::El, vec![], other));
                            tree.insert(Element::with_attrs(
                                id(10),
                                if axis == Axis::Width {
                                    ElementKind::Column
                                } else {
                                    ElementKind::Row
                                },
                                vec![],
                                bounds,
                            ));
                            tree.set_children(&id(10), vec![id(1), id(5)]).unwrap();
                            tree.set_root_id(id(10));
                        }
                        let mut d = Driver::new(tree, Instant::now(), mode);
                        assert!(
                            d.step(
                                0,
                                vec![Event::Viewport {
                                    width: width as f32,
                                    height: height as f32,
                                    scale
                                }]
                            )
                            .result
                            .is_ok()
                        );
                        if owner == 2 {
                            let mut attrs = d.attrs(id(2));
                            axis_length(&mut attrs, axis, Length::Fill);
                            assert!(
                                d.step(0, vec![Event::Attrs(id(2), Box::new(attrs))])
                                    .result
                                    .is_ok()
                            );
                        }
                        if owner == 3 {
                            apply_patches(&mut d.tree, vec![Patch::Remove { id: id(2) }]).unwrap();
                            assert!(d.step(0, vec![]).result.is_ok());
                        }
                        assert!(d.step(500_000, vec![]).result.is_ok());
                        let before = d.step(1_750_000, vec![]).published_after.unwrap();
                        let mut renderer = SceneRenderer::new();
                        let old =
                            raster(&mut renderer, before.output.scene.clone(), width, height, 1);
                        let rejected = d.attempt(
                            2_000_000,
                            vec![],
                            Injection::Query(if independent {
                                QueryKind::IndependentContext
                            } else {
                                QueryKind::ForeignTarget
                            }),
                        );
                        assert!(
                            rejected.result.is_err(),
                            "owner={owner}: {:?}",
                            rejected.result
                        );
                        rejected.published_after.unwrap().assert_same(&before);
                        assert_eq!(
                            old,
                            raster(&mut renderer, before.output.scene.clone(), width, height, 2)
                        );
                        let out = d.step(2_100_000, vec![]);
                        assert!(
                            out.result.is_ok(),
                            "{axis:?} {mode:?} owner={owner}: {:?}",
                            out.result
                        );
                        let output = out.published_after.unwrap().output;
                        let pixels = raster(&mut renderer, output.scene.clone(), width, height, 3);
                        assert_eq!(
                            pixels,
                            raster(
                                &mut SceneRenderer::new(),
                                output.scene.clone(),
                                width,
                                height,
                                1
                            )
                        );
                        let pixel = |buffer: &Vec<u8>, at: f32| {
                            let (x, y) = if axis == Axis::Width {
                                (at * scale, 10.0 * scale)
                            } else {
                                (10.0 * scale, at * scale)
                            };
                            let n = (y as usize * width as usize + x as usize) * 4;
                            buffer[n..n + 4].to_vec()
                        };
                        assert_eq!(pixel(&old, 150.0), vec![255, 0, 0, 255]);
                        assert_eq!(pixel(&pixels, 50.0), vec![255, 0, 0, 255]);
                        assert_eq!(pixel(&pixels, 150.0), vec![0, 0, 255, 255]);
                        assert_eq!(pixel(&pixels, 300.0), vec![0, 0, 0, 0]);
                        let (x, y) = if axis == Axis::Width {
                            (150.0 * scale, 10.0 * scale)
                        } else {
                            (10.0 * scale, 150.0 * scale)
                        };
                        let click = CursorButton {
                            button: "left".into(),
                            action: 1,
                            mods: 0,
                            x,
                            y,
                        };
                        assert!(output.event_rebuild_changed);
                        assert!(
                            output
                                .event_rebuild
                                .base_registry
                                .view()
                                .iter_precedence()
                                .any(|listener| listener.element_id == Some(id(3))
                                    && listener.matcher.matches(&click))
                        );
                        assert_eq!(
                            old,
                            raster(&mut renderer, before.output.scene.clone(), width, height, 4)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn coupled_peer_release_matches_pixels_and_hits_for_every_owner() {
    use super::super::inspection::QueryKind;
    use super::support::Injection;
    use crate::input::InputEvent::CursorButton;
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for owner in 0..4 {
                for scale in [0.5, 1.0, 2.0] {
                    let mut tree = visual_tree(Length::Px(40.0), Length::Fill, axis, owner);
                    let mut peer = spec(Length::Px(200.0), Length::Fill, 2000.0, axis);
                    peer.repeat = AnimationRepeat::Loop;
                    tree.get_mut(&id(3)).unwrap().spec.declared.animate = Some(peer);
                    let (width, height) = if axis == Axis::Width {
                        ((600.0 * scale) as u32, (60.0 * scale) as u32)
                    } else {
                        ((60.0 * scale) as u32, (600.0 * scale) as u32)
                    };
                    let mut d = Driver::new(tree, Instant::now(), mode);
                    assert!(
                        d.step(
                            0,
                            vec![Event::Viewport {
                                width: width as f32,
                                height: height as f32,
                                scale
                            }]
                        )
                        .result
                        .is_ok()
                    );
                    if owner == 2 {
                        let mut attrs = d.attrs(id(2));
                        axis_length(&mut attrs, axis, Length::Fill);
                        assert!(
                            d.step(0, vec![Event::Attrs(id(2), Box::new(attrs))])
                                .result
                                .is_ok()
                        );
                    }
                    if owner == 3 {
                        apply_patches(&mut d.tree, vec![Patch::Remove { id: id(2) }]).unwrap();
                        assert!(d.step(0, vec![]).result.is_ok());
                    }
                    let before = d.step(500_000, vec![]).published_after.unwrap();
                    let mut renderer = SceneRenderer::new();
                    let old = raster(&mut renderer, before.output.scene.clone(), width, height, 1);
                    let (key, previous) = d
                        .tree
                        .length_runtime
                        .as_ref()
                        .unwrap()
                        .tracks
                        .iter()
                        .find(|((node, lane), _)| *node != id(3) && *lane == axis)
                        .map(|(key, track)| (*key, track.clone()))
                        .unwrap();
                    let middle = d.step(750_000, vec![]);
                    assert!(middle.result.is_ok());
                    let track = &d.tree.length_runtime.as_ref().unwrap().tracks[&key];
                    assert_eq!(track.key, previous.key);
                    assert_eq!(track.anchor, previous.anchor);
                    assert!(track.from.release_matches(previous.from));
                    assert!(
                        previous
                            .from
                            .interpolate(
                                track.to,
                                ((0.75 - previous.anchor) / (1.0 - previous.anchor)) as f32
                            )
                            .unwrap()
                            .release_matches(capture_axis(&d.tree, &key.0, axis).unwrap())
                    );
                    let middle = middle.published_after.unwrap();
                    let failed = d.attempt(
                        1_000_000,
                        vec![],
                        Injection::Query(QueryKind::ForeignTarget),
                    );
                    assert!(failed.result.is_err());
                    failed.published_after.unwrap().assert_same(&middle);
                    let out = d.step(1_100_000, vec![]);
                    assert!(
                        out.result.is_ok(),
                        "{axis:?} {mode:?} {owner}: {:?}",
                        out.result
                    );
                    let output = out.published_after.unwrap().output;
                    let pixels = raster(&mut renderer, output.scene.clone(), width, height, 2);
                    assert_eq!(
                        pixels,
                        raster(
                            &mut SceneRenderer::new(),
                            output.scene.clone(),
                            width,
                            height,
                            1
                        )
                    );
                    let point = |at: f32| {
                        if axis == Axis::Width {
                            (at * scale, 10.0 * scale)
                        } else {
                            (10.0 * scale, at * scale)
                        }
                    };
                    let pixel = |buffer: &Vec<u8>, at| {
                        let (x, y) = point(at);
                        let n = (y as usize * width as usize + x as usize) * 4;
                        buffer[n..n + 4].to_vec()
                    };
                    assert_eq!(pixel(&old, 200.0), vec![0, 0, 255, 255]);
                    assert_eq!(pixel(&old, 250.0), vec![0, 0, 255, 255]);
                    assert_eq!(pixel(&pixels, 250.0), vec![255, 0, 0, 255]);
                    assert_eq!(pixel(&old, 575.0), vec![0, 0, 0, 0]);
                    assert_eq!(pixel(&pixels, 575.0), vec![0, 0, 255, 255]);
                    for (at, node) in [(250.0, (owner != 3).then_some(id(2))), (575.0, Some(id(3)))]
                    {
                        let (x, y) = point(at);
                        let click = CursorButton {
                            button: "left".into(),
                            action: 1,
                            mods: 0,
                            x,
                            y,
                        };
                        let hit = output
                            .event_rebuild
                            .base_registry
                            .view()
                            .iter_precedence()
                            .find(|listener| listener.matcher.matches(&click))
                            .and_then(|listener| listener.element_id);
                        assert_eq!(hit, node);
                    }
                    assert_eq!(
                        old,
                        raster(&mut renderer, before.output.scene.clone(), width, height, 3)
                    );
                }
            }
        }
    }
}
