use super::support::{Driver, Event, Injection, Mode};
use super::*;
use crate::tree::patch::{Patch, apply_patches};

fn transport_tree(axis: Axis) -> ElementTree {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
    let kind = if axis == Axis::Width {
        ElementKind::Row
    } else {
        ElementKind::Column
    };
    tree.insert(Element::with_attrs(
        id(10),
        kind,
        vec![],
        Attrs {
            width: Some(Length::Px(400.0)),
            height: Some(Length::Px(400.0)),
            ..Default::default()
        },
    ));
    tree.insert(Element::with_attrs(
        id(11),
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            ..Default::default()
        },
    ));
    tree.set_children(&id(10), vec![id(11)]).unwrap();
    tree.insert(Element::with_attrs(
        id(20),
        if axis == Axis::Width {
            ElementKind::Column
        } else {
            ElementKind::Row
        },
        vec![],
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            ..Default::default()
        },
    ));
    tree.set_children(&id(20), vec![id(1), id(10)]).unwrap();
    tree.set_root_id(id(20));
    tree
}

fn reparent(d: &mut Driver) {
    apply_patches(
        &mut d.tree,
        vec![
            Patch::SetChildren {
                id: id(1),
                children: vec![id(3)],
            },
            Patch::SetChildren {
                id: id(10),
                children: vec![id(2), id(11)],
            },
        ],
    )
    .unwrap();
}

#[test]
fn same_mount_reparent_preserves_published_box_and_native_release() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            let mut d = Driver::new(transport_tree(axis), Instant::now(), mode);
            assert!(d.step(0, vec![]).result.is_ok());
            assert!(d.step(500_000, vec![]).result.is_ok());
            let before = capture_axis(&d.tree, &id(2), axis).unwrap();
            let key = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)].key;
            reparent(&mut d);
            let out = d.step(600_000, vec![]);
            assert!(out.result.is_ok(), "{axis:?} {mode:?}: {:?}", out.result);
            close(
                capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                before.visible,
            );
            assert_eq!(
                d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)].key,
                key
            );
            assert!(d.step(1_000_000, vec![]).result.is_ok());
            assert!(d.runtime.groups.is_empty());
            close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 200.0);
            close(capture_axis(&d.tree, &id(3), axis).unwrap().visible, 600.0);
        }
    }
}

#[test]
fn native_transport_maps_roles_and_units_without_lowering_own_channels() {
    use crate::tree::attrs::AlignX;
    use crate::tree::element::{NearbyMount, NearbySlot};
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for role in 0..5 {
                for curve in [
                    AnimationCurve::Linear,
                    AnimationCurve::EaseIn,
                    AnimationCurve::EaseOut,
                ] {
                    for (old_scale, new_scale) in [(1.0, 1.0), (0.5, 2.0), (2.0, 0.5)] {
                        let mut tree = transport_tree(axis);
                        tree.get_mut(&id(2))
                            .unwrap()
                            .spec
                            .declared
                            .animate
                            .as_mut()
                            .unwrap()
                            .curve = curve.clone();
                        tree.get_mut(&id(1)).unwrap().spec.declared.layout_scale = Some(old_scale);
                        let parent = tree.get_mut(&id(10)).unwrap();
                        parent.spec.declared.layout_scale = Some(new_scale);
                        parent.spec.kind = match role {
                            0 => parent.spec.kind,
                            1 => {
                                if axis == Axis::Width {
                                    ElementKind::Column
                                } else {
                                    ElementKind::Row
                                }
                            }
                            2 => ElementKind::WrappedRow,
                            3 => ElementKind::Paragraph,
                            _ => ElementKind::El,
                        };
                        if role == 3 {
                            tree.get_mut(&id(2)).unwrap().spec.declared.align_x =
                                Some(AlignX::Left);
                        }
                        let mut d = Driver::new(tree, Instant::now(), mode);
                        assert!(d.step(0, vec![]).result.is_ok());
                        assert!(d.step(500_000, vec![]).result.is_ok());
                        let before = capture_axis(&d.tree, &id(2), axis).unwrap();
                        let before_scale = d.tree.get(&id(2)).unwrap().layout.dimension_facts.scale;
                        if role == 4 {
                            apply_patches(
                                &mut d.tree,
                                vec![
                                    Patch::SetChildren {
                                        id: id(1),
                                        children: vec![id(3)],
                                    },
                                    Patch::SetNearbyMounts {
                                        host_id: id(10),
                                        mounts: vec![NearbyMount {
                                            id: id(2),
                                            slot: NearbySlot::InFront,
                                        }],
                                    },
                                ],
                            )
                            .unwrap();
                        } else {
                            reparent(&mut d);
                        }
                        let out = d.step(600_000, vec![]);
                        assert!(
                            out.result.is_ok(),
                            "{axis:?} role={role} {old_scale}->{new_scale}: {:?}",
                            out.result
                        );
                        let fp = capture_axis(&d.tree, &id(2), axis).unwrap();
                        let scale = d.tree.get(&id(2)).unwrap().layout.dimension_facts.scale;
                        close(fp.visible * scale, before.visible * before_scale);
                        close(fp.intrinsic * scale, before.intrinsic * before_scale);
                        close(fp.initial * scale, before.initial * before_scale);
                        assert_eq!(fp.policy, before.policy);
                        let out = d.step(1_000_000, vec![]);
                        assert!(
                            out.result.is_ok(),
                            "{axis:?} role={role} {old_scale}->{new_scale} release: {:?}",
                            out.result
                        );
                        assert!(d.runtime.groups.is_empty());
                        let expected = out
                            .inspection
                            .queries
                            .iter()
                            .find(|q| q.kind == super::super::inspection::QueryKind::Release)
                            .unwrap()
                            .result
                            .as_ref()
                            .unwrap()[&(id(2), axis)];
                        assert!(
                            expected.release_matches(capture_axis(&d.tree, &id(2), axis).unwrap())
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn transport_failure_second_move_and_unpublished_reversal_keep_first_source() {
    use super::super::inspection::QueryKind;
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for fault in [
            Injection::Query(QueryKind::Transport),
            Injection::Query(QueryKind::Target),
            Injection::BeforeLayout,
        ] {
            let mut d = Driver::new(transport_tree(Axis::Width), Instant::now(), mode);
            assert!(d.step(0, vec![]).result.is_ok());
            let before = d.step(500_000, vec![]).published_after.unwrap();
            let initial =
                d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].clone();
            reparent(&mut d);
            let out = d.attempt(600_000, vec![], fault);
            assert!(out.result.is_err());
            out.published_after.unwrap().assert_same(&before);
            apply_patches(
                &mut d.tree,
                vec![
                    Patch::SetChildren {
                        id: id(10),
                        children: vec![id(11)],
                    },
                    Patch::SetChildren {
                        id: id(1),
                        children: vec![id(2), id(3)],
                    },
                ],
            )
            .unwrap();
            let out = d.step(750_000, vec![]);
            assert!(out.result.is_ok(), "{:?}", out.result);
            let track = &d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)];
            assert_eq!(track.key, initial.key);
            assert_eq!(track.anchor, initial.anchor);
            assert!(track.from.release_matches(initial.from));
            close(
                capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
                235.0,
            );
            assert!(d.step(1_000_000, vec![]).result.is_ok());
        }
    }
}

#[test]
fn promotion_to_root_transports_the_same_mounted_owner() {
    for axis in [Axis::Width, Axis::Height] {
        let mut d = Driver::new(transport_tree(axis), Instant::now(), Mode::Full);
        assert!(d.step(0, vec![]).result.is_ok());
        assert!(d.step(500_000, vec![]).result.is_ok());
        let source = capture_axis(&d.tree, &id(2), axis).unwrap();
        d.tree.set_root_id(id(2));
        let out = d.step(600_000, vec![]);
        assert!(out.result.is_ok(), "{:?}", out.result);
        let fp = capture_axis(&d.tree, &id(2), axis).unwrap();
        assert!(fp.scope.is_none());
        close(fp.visible, source.visible);
        let out = d.step(1_000_000, vec![]);
        assert!(out.result.is_ok(), "{:?}", out.result);
        assert!(d.runtime.groups.is_empty());
        close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 600.0);
    }
}

#[test]
fn declared_float_handoff_and_due_move_are_native_and_transactional() {
    use super::super::inspection::QueryKind;
    use crate::tree::attrs::AlignX;
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for due in [false, true] {
                let mut tree = transport_tree(axis);
                if !due {
                    tree.get_mut(&id(1)).unwrap().spec.kind = ElementKind::TextColumn;
                }
                let node = tree.get_mut(&id(2)).unwrap();
                node.spec.declared.width = Some(Length::Px(40.0));
                node.spec.declared.height = Some(Length::Px(40.0));
                let mut d = Driver::new(tree, Instant::now(), mode);
                let out = d.step(0, vec![]);
                assert!(
                    out.result.is_ok(),
                    "initial {axis:?} due={due}: {:?}",
                    out.result
                );
                let published = d.step(500_000, vec![]).published_after.unwrap();
                let source = capture_axis(&d.tree, &id(2), axis).unwrap();
                if due {
                    reparent(&mut d);
                } else {
                    let mut attrs = d.tree.get(&id(2)).unwrap().spec.declared.clone();
                    attrs.align_x = Some(AlignX::Left);
                    let out = d.attempt(
                        600_000,
                        vec![Event::Attrs(id(2), Box::new(attrs))],
                        Injection::Query(QueryKind::Transport),
                    );
                    assert!(out.result.is_err());
                    out.published_after.unwrap().assert_same(&published);
                }
                let out = d.step(if due { 1_000_000 } else { 600_000 }, vec![]);
                assert!(out.result.is_ok(), "{axis:?} due={due}: {:?}", out.result);
                if !due {
                    close(
                        capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                        source.visible,
                    );
                    assert!(d.step(1_000_000, vec![]).result.is_ok());
                }
                assert!(d.runtime.groups.is_empty());
            }
        }
    }
}

#[test]
fn corrupt_structural_source_and_clock_receipts_cannot_be_relabelled() {
    for corruption in 0..4 {
        let mut d = Driver::new(transport_tree(Axis::Width), Instant::now(), Mode::Full);
        assert!(d.step(0, vec![]).result.is_ok());
        let published = d.step(500_000, vec![]).published_after.unwrap();
        reparent(&mut d);
        let source = d.tree.pending_patch_effects.sources[&id(2)].clone();
        let track = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].clone();
        match corruption {
            0 => {
                d.tree
                    .pending_patch_effects
                    .sources
                    .get_mut(&id(2))
                    .unwrap()
                    .dimensions[0]
                    .as_mut()
                    .unwrap()
                    .visible += 10.0
            }
            1 => {
                d.tree
                    .pending_patch_effects
                    .sources
                    .get_mut(&id(2))
                    .unwrap()
                    .scale *= 2.0
            }
            2 => {
                d.tree
                    .pending_patch_effects
                    .sources
                    .get_mut(&id(2))
                    .unwrap()
                    .model += 1
            }
            _ => {
                Arc::make_mut(
                    d.tree
                        .length_runtime
                        .as_mut()
                        .unwrap()
                        .tracks
                        .get_mut(&(id(2), Axis::Width))
                        .unwrap(),
                )
                .to
                .visible += 10.0
            }
        }
        let out = d.step(600_000, vec![]);
        assert!(out.result.is_err(), "corruption {corruption}");
        out.published_after.unwrap().assert_same(&published);
        d.tree.pending_patch_effects.sources.insert(id(2), source);
        d.tree
            .length_runtime
            .as_mut()
            .unwrap()
            .tracks
            .insert((id(2), Axis::Width), track);
        assert!(d.step(750_000, vec![]).result.is_ok());
        close(
            capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
            170.0,
        );
        assert!(d.step(1_000_000, vec![]).result.is_ok());
    }
}

#[test]
fn transported_coupled_forecasts_keep_new_scope_and_leaf_clock_evidence() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            let mut tree = transport_tree(axis);
            let mut animation = spec(Length::Px(200.0), Length::Fill, 2000.0, axis);
            animation.repeat = AnimationRepeat::Loop;
            tree.get_mut(&id(3)).unwrap().spec.declared.animate = Some(animation);
            let mut d = Driver::new(tree, Instant::now(), mode);
            assert!(d.step(0, vec![]).result.is_ok());
            assert!(d.step(500_000, vec![]).result.is_ok());
            apply_patches(
                &mut d.tree,
                vec![
                    Patch::SetChildren {
                        id: id(1),
                        children: vec![],
                    },
                    Patch::SetChildren {
                        id: id(10),
                        children: vec![id(2), id(3), id(11)],
                    },
                ],
            )
            .unwrap();
            let out = d.step(600_000, vec![]);
            assert!(out.result.is_ok(), "{:?}", out.result);
            let from = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)].clone();
            for at in [750_000, 900_000] {
                let out = d.step(at, vec![]);
                assert!(out.result.is_ok(), "{:?}", out.result);
                let state = d.tree.length_runtime.as_ref().unwrap();
                let current = &state.tracks[&(id(2), axis)];
                assert!(current.from.release_matches(from.from));
                assert_eq!(current.anchor, from.anchor);
                assert_eq!(current.key, from.key);
                assert!(state.tracks[&(id(3), axis)].forecast.is_none());
            }
            let out = d.step(1_000_000, vec![]);
            assert!(out.result.is_ok(), "{:?}", out.result);
            assert!(d.runtime.groups.is_empty());
        }
    }
}

#[test]
fn moving_a_held_owner_keeps_its_admitted_deadline() {
    for axis in [Axis::Width, Axis::Height] {
        let mut tree = transport_tree(axis);
        tree.get_mut(&id(2))
            .unwrap()
            .spec
            .declared
            .animate
            .as_mut()
            .unwrap()
            .duration_ms = 500.0;
        tree.get_mut(&id(3)).unwrap().spec.declared.animate =
            Some(spec(Length::Px(40.0), Length::Fill, 2000.0, axis));
        let mut d = Driver::new(tree, Instant::now(), Mode::Full);
        for t in [0, 500_000, 1_000_000] {
            assert!(d.step(t, vec![]).result.is_ok());
        }
        let key = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)].key;
        let hold = key.started + Duration::from_secs(2);
        reparent(&mut d);
        let out = d.step(1_100_000, vec![]);
        assert!(out.result.is_ok(), "{:?}", out.result);
        let track = d
            .tree
            .length_runtime
            .as_ref()
            .unwrap()
            .tracks
            .get(&(id(2), axis))
            .expect("move must not truncate admitted hold");
        assert_eq!(track.hold_interval.unwrap().1, hold);
        assert!(d.step(1_500_000, vec![]).result.is_ok());
        assert!(d.step(2_000_000, vec![]).result.is_ok());
        assert!(d.runtime.groups.is_empty());
    }
}

#[test]
fn transport_at_segment_repeat_boundaries_preserves_mount_and_run_clock() {
    for repeat in [AnimationRepeat::Times(2), AnimationRepeat::Loop] {
        for at in [1_000_000, 1_100_000] {
            let mut tree = transport_tree(Axis::Width);
            tree.get_mut(&id(2))
                .unwrap()
                .spec
                .declared
                .animate
                .as_mut()
                .unwrap()
                .repeat = repeat.clone();
            let mut d = Driver::new(tree, Instant::now(), Mode::Full);
            assert!(d.step(0, vec![]).result.is_ok());
            assert!(d.step(750_000, vec![]).result.is_ok());
            let key = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key;
            reparent(&mut d);
            let out = d.step(at, vec![]);
            assert!(out.result.is_ok(), "{:?}", out.result);
            close(
                capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
                235.0,
            );
            let next = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key;
            assert_eq!(next.started, key.started);
            assert_eq!(next.generation, key.generation);
            assert_eq!(next.mount, key.mount);
            assert!(d.step(1_500_000, vec![]).result.is_ok());
            let out = d.step(2_000_000, vec![]);
            assert!(out.result.is_ok(), "{:?}", out.result);
        }
    }
}

#[test]
fn rotated_transport_preserves_native_source_channels() {
    for axis in [Axis::Width, Axis::Height] {
        let mut tree = transport_tree(axis);
        let attrs = &mut tree.get_mut(&id(2)).unwrap().spec.declared;
        attrs.width = Some(Length::Px(40.0));
        attrs.height = Some(Length::Px(40.0));
        attrs.layout_rotate = Some(15.0);
        let mut d = Driver::new(tree, Instant::now(), Mode::Full);
        assert!(d.step(0, vec![]).result.is_ok());
        assert!(d.step(500_000, vec![]).result.is_ok());
        let before = capture_axis(&d.tree, &id(2), axis).unwrap();
        reparent(&mut d);
        let out = d.step(600_000, vec![]);
        assert!(out.result.is_ok(), "{:?}", out.result);
        let after = capture_axis(&d.tree, &id(2), axis).unwrap();
        close(after.visible, before.visible);
        close(after.initial, before.initial);
        close(after.intrinsic, before.intrinsic);
        assert!(d.step(1_000_000, vec![]).result.is_ok());
    }
}

#[test]
fn change_handoff_during_move_uses_first_published_native_source() {
    use crate::tree::animation::change::{ChangePolicy, Field};
    let mut d = Driver::new(transport_tree(Axis::Width), Instant::now(), Mode::Full);
    assert!(d.step(0, vec![]).result.is_ok());
    assert!(d.step(500_000, vec![]).result.is_ok());
    reparent(&mut d);
    let mut attrs = d.tree.get(&id(2)).unwrap().spec.declared.clone();
    attrs.animate = None;
    attrs.width = Some(Length::Px(100.0));
    attrs.animate_change = Some(Arc::new(vec![ChangePolicy {
        field: Field::Width,
        duration_ms: 1000.0,
        curve: AnimationCurve::Linear,
    }]));
    let out = d.step(600_000, vec![Event::Attrs(id(2), Box::new(attrs))]);
    assert!(out.result.is_ok(), "{:?}", out.result);
    close(
        capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
        170.0,
    );
    assert!(d.step(1_100_000, vec![]).result.is_ok());
    close(
        capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
        135.0,
    );
    assert!(d.step(1_600_000, vec![]).result.is_ok());
    assert!(d.runtime.groups.is_empty());
}

#[test]
fn root_replacement_and_remount_do_not_inherit_old_clocks_or_sources() {
    for root in [false, true] {
        let mut d = Driver::new(transport_tree(Axis::Width), Instant::now(), Mode::Full);
        assert!(d.step(0, vec![]).result.is_ok());
        assert!(d.step(500_000, vec![]).result.is_ok());
        let old = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key;
        let mut subtree = ElementTree::new();
        subtree.insert(Element::with_attrs(
            id(2),
            ElementKind::El,
            vec![],
            Attrs {
                animate: Some(spec(Length::Px(80.0), Length::Fill, 1000.0, Axis::Width)),
                ..Default::default()
            },
        ));
        subtree.set_root_id(id(2));
        apply_patches(
            &mut d.tree,
            vec![
                Patch::Remove { id: id(2) },
                Patch::InsertSubtree {
                    parent_id: (!root).then_some(id(10)),
                    index: 0,
                    subtree,
                },
            ],
        )
        .unwrap();
        let out = d.step(600_000, vec![]);
        assert!(out.result.is_ok(), "{:?}", out.result);
        let next = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key;
        assert_ne!(next.mount, old.mount);
        assert_ne!(next.generation, old.generation);
        close(
            capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
            80.0,
        );
        assert!(d.step(1_600_000, vec![]).result.is_ok());
        assert!(d.runtime.groups.is_empty());
    }
}

#[test]
fn transport_replays_joint_asset_runtime_scroll_viewport_and_scale_inputs() {
    use super::super::inspection::QueryKind;
    use crate::tree::attrs::{Font, ImageSource};
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../priv/test_assets/Lobster-Regular.ttf"),
    )
    .unwrap();
    let image = |width| {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="20"><rect width="{width}" height="20" fill="red"/></svg>"#
        );
        crate::assets::register_vector_asset(
            "transport-media",
            resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).unwrap(),
        )
        .unwrap();
    };
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for media in [false, true] {
                for next_scale in [0.5, 2.0] {
                    let assets = crate::assets::AssetRuntime::new();
                    let _assets = assets.enter();
                    image(80);
                    let mut tree = transport_tree(axis);
                    tree.get_mut(&id(1)).unwrap().spec.declared.layout_scale = Some(0.75);
                    tree.get_mut(&id(10)).unwrap().spec.declared.layout_scale = Some(1.25);
                    let node = tree.get_mut(&id(2)).unwrap();
                    node.spec.kind = if media {
                        ElementKind::Image
                    } else {
                        ElementKind::Text
                    };
                    node.spec.declared.content = Some("Wavy historical inputs".into());
                    node.spec.declared.font = Some(Font::String("transport-font".into()));
                    node.spec.declared.font_size = Some(24.0);
                    node.spec.declared.image_src =
                        media.then(|| ImageSource::Id("transport-media".into()));
                    node.spec.declared.scrollbar_x = Some(true);
                    let mut d = Driver::new(tree, Instant::now(), mode);
                    assert!(d.step(0, vec![]).result.is_ok());
                    let before = d.step(500_000, vec![]).published_after.unwrap();
                    let prior =
                        d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)].clone();
                    let source = capture_axis(&d.tree, &id(2), axis).unwrap();
                    let old_scale = d.tree.get(&id(2)).unwrap().layout.dimension_facts.scale;
                    let mut renderer = crate::renderer::SceneRenderer::new();
                    let old_pixels = super::rendered::raster(
                        &mut renderer,
                        before.output.scene.clone(),
                        600,
                        600,
                        1,
                    );
                    reparent(&mut d);
                    d.tree.set_mouse_over_active(&id(2), true);
                    d.tree.get_mut(&id(2)).unwrap().layout.scroll_x = 25.0;
                    image(160);
                    crate::services::load_font_bytes(&assets, "transport-font", 400, false, &bytes)
                        .unwrap();
                    let out = d.attempt(
                        600_000,
                        vec![Event::Viewport {
                            width: 800.0,
                            height: 800.0,
                            scale: next_scale,
                        }],
                        Injection::Query(QueryKind::Transport),
                    );
                    assert!(out.result.is_err());
                    out.published_after.unwrap().assert_same(&before);
                    assert_eq!(
                        old_pixels,
                        super::rendered::raster(
                            &mut renderer,
                            before.output.scene.clone(),
                            600,
                            600,
                            2
                        )
                    );
                    let out = d.step(750_000, vec![]);
                    assert!(
                        out.result.is_ok(),
                        "{axis:?} media={media}: {:?}",
                        out.result
                    );
                    let fp = capture_axis(&d.tree, &id(2), axis).unwrap();
                    let scale = d.tree.get(&id(2)).unwrap().layout.dimension_facts.scale;
                    close(fp.visible * scale, source.visible * old_scale);
                    let track =
                        d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)].clone();
                    assert_eq!(track.key, prior.key);
                    assert!(
                        prior
                            .evidence
                            .native(
                                prior.model,
                                &prior.projection,
                                &prior.context,
                                (id(2), axis),
                                prior.key.mount
                            )
                            .unwrap()
                            .release_matches(prior.to)
                    );
                    // A publication may clamp the requested scroll offset. Sample
                    // those current seeds before isolating the foreign renderer.
                    assert!(d.step(800_000, vec![]).result.is_ok());
                    let stable_context = d.tree.length_runtime.as_ref().unwrap().tracks
                        [&(id(2), axis)]
                        .context
                        .clone();
                    let other = crate::assets::AssetRuntime::new();
                    {
                        let _other = other.enter();
                        image(900);
                        crate::services::load_font_bytes(
                            &other,
                            "transport-font",
                            400,
                            false,
                            &bytes,
                        )
                        .unwrap();
                    }
                    assert!(d.step(850_000, vec![]).result.is_ok());
                    assert!(
                        Arc::ptr_eq(
                            &stable_context,
                            &d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)].context
                        ),
                        "another renderer must not invalidate these inputs"
                    );
                    let out = d.step(1_000_000, vec![]);
                    assert!(out.result.is_ok(), "{:?}", out.result);
                    assert!(d.runtime.groups.is_empty());
                    assert_eq!(
                        old_pixels,
                        super::rendered::raster(
                            &mut renderer,
                            before.output.scene.clone(),
                            600,
                            600,
                            3
                        )
                    );
                }
            }
        }
    }
}

#[test]
fn replacing_parent_kind_captures_child_provenance_before_overwrite() {
    for axis in [Axis::Width, Axis::Height] {
        let mut d = Driver::new(transport_tree(axis), Instant::now(), Mode::Full);
        assert!(d.step(0, vec![]).result.is_ok());
        assert!(d.step(500_000, vec![]).result.is_ok());
        let before = capture_axis(&d.tree, &id(2), axis).unwrap();
        let mut parent = d.tree.get(&id(1)).unwrap().clone();
        parent.spec.kind = if axis == Axis::Width {
            ElementKind::Column
        } else {
            ElementKind::Row
        };
        d.tree.insert(parent);
        let out = d.step(600_000, vec![]);
        assert!(out.result.is_ok(), "{:?}", out.result);
        close(
            capture_axis(&d.tree, &id(2), axis).unwrap().visible,
            before.visible,
        );
        assert!(d.step(1_000_000, vec![]).result.is_ok());
        close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 600.0);
    }
}

#[test]
fn unpublished_held_detour_restores_committed_group_membership() {
    let mut tree = transport_tree(Axis::Width);
    tree.get_mut(&id(2))
        .unwrap()
        .spec
        .declared
        .animate
        .as_mut()
        .unwrap()
        .duration_ms = 500.0;
    tree.get_mut(&id(3)).unwrap().spec.declared.animate =
        Some(spec(Length::Px(40.0), Length::Fill, 2000.0, Axis::Width));
    let mut d = Driver::new(tree, Instant::now(), Mode::Full);
    for at in [0, 500_000, 1_000_000] {
        assert!(d.step(at, vec![]).result.is_ok());
    }
    let groups = d.runtime.groups.observation();
    reparent(&mut d);
    assert!(
        d.attempt(1_100_000, vec![], Injection::BeforeLayout)
            .result
            .is_err()
    );
    apply_patches(
        &mut d.tree,
        vec![
            Patch::SetChildren {
                id: id(10),
                children: vec![id(11)],
            },
            Patch::SetChildren {
                id: id(1),
                children: vec![id(2), id(3)],
            },
        ],
    )
    .unwrap();
    assert!(d.step(1_200_000, vec![]).result.is_ok());
    assert_eq!(d.runtime.groups.observation(), groups);
    assert!(d.step(2_000_000, vec![]).result.is_ok());
    assert!(d.runtime.groups.is_empty());
}

#[test]
fn ghost_after_transport_uses_new_native_baseline_and_retries_cleanup() {
    for axis in [Axis::Width, Axis::Height] {
        let mut tree = transport_tree(axis);
        tree.get_mut(&id(2)).unwrap().spec.declared.animate_exit =
            Some(spec(Length::Px(1.0), Length::Px(0.0), 400.0, axis));
        let mut d = Driver::new(tree, Instant::now(), Mode::Full);
        assert!(d.step(0, vec![]).result.is_ok());
        assert!(d.step(500_000, vec![]).result.is_ok());
        reparent(&mut d);
        let before = d.step(600_000, vec![]).published_after.unwrap();
        let source = capture_axis(&d.tree, &id(2), axis).unwrap();
        apply_patches(&mut d.tree, vec![Patch::Remove { id: id(2) }]).unwrap();
        let failed = d.attempt(700_000, vec![], Injection::BeforeLayout);
        assert!(failed.result.is_err());
        failed.published_after.unwrap().assert_same(&before);
        let out = d.step(700_000, vec![]);
        assert!(out.result.is_ok(), "{:?}", out.result);
        let ghost = d.tree.iter_nodes().find(|n| n.is_ghost_root()).unwrap().id;
        close(
            capture_axis(&d.tree, &ghost, axis).unwrap().visible,
            source.visible,
        );
        let out = d.step(900_000, vec![]);
        assert!(out.result.is_ok());
        let before = out.published_after.unwrap();
        let failed = d.attempt(1_100_000, vec![], Injection::BeforeLayout);
        assert!(failed.result.is_err());
        failed.published_after.unwrap().assert_same(&before);
        assert!(d.step(1_100_000, vec![]).result.is_ok());
        assert!(d.step(1_100_001, vec![]).result.is_ok());
        assert!(d.tree.get(&ghost).is_none());
        assert!(d.runtime.groups.is_empty());
    }
}

#[test]
fn numeric_moves_enter_native_transport_without_changing_the_normal_pixel_fast_path() {
    for axis in [Axis::Width, Axis::Height] {
        let mut tree = transport_tree(axis);
        tree.get_mut(&id(2)).unwrap().spec.declared.animate =
            Some(spec(Length::Px(40.0), Length::Px(80.0), 1000.0, axis));
        tree.get_mut(&id(1)).unwrap().spec.declared.layout_scale = Some(0.5);
        tree.get_mut(&id(10)).unwrap().spec.declared.layout_scale = Some(2.0);
        let mut d = Driver::new(tree, Instant::now(), Mode::Full);
        assert!(d.step(0, vec![]).result.is_ok());
        assert!(d.step(500_000, vec![]).result.is_ok());
        assert!(d.tree.length_runtime.is_none());
        reparent(&mut d);
        let out = d.step(600_000, vec![]);
        assert!(out.result.is_ok(), "{:?}", out.result);
        close(extent(&d.tree, 2, axis), 30.0);
        assert!(d.tree.length_runtime.is_some());
        assert!(d.step(800_000, vec![]).result.is_ok());
        close(extent(&d.tree, 2, axis), 95.0);
        let out = d.step(1_000_000, vec![]);
        assert!(out.result.is_ok(), "{:?}", out.result);
        close(extent(&d.tree, 2, axis), 160.0);
        assert!(d.tree.length_runtime.is_none());
    }
}

#[test]
fn both_axes_transport_through_one_joint_native_source_and_release() {
    let mut tree = transport_tree(Axis::Width);
    tree.get_mut(&id(10)).unwrap().spec.kind = ElementKind::Column;
    let spec = tree
        .get_mut(&id(2))
        .unwrap()
        .spec
        .declared
        .animate
        .as_mut()
        .unwrap();
    spec.keyframes[0].height = Some(Length::Px(40.0));
    spec.keyframes[1].height = Some(Length::Fill);
    let mut d = Driver::new(tree, Instant::now(), Mode::Full);
    assert!(d.step(0, vec![]).result.is_ok());
    assert!(d.step(500_000, vec![]).result.is_ok());
    let before =
        [Axis::Width, Axis::Height].map(|axis| capture_axis(&d.tree, &id(2), axis).unwrap());
    reparent(&mut d);
    let out = d.step(600_000, vec![]);
    assert!(out.result.is_ok(), "{:?}", out.result);
    assert_eq!(
        out.inspection
            .queries
            .iter()
            .filter(|query| query.kind == super::super::inspection::QueryKind::Transport)
            .count(),
        1
    );
    for (axis, fp) in [Axis::Width, Axis::Height].into_iter().zip(before) {
        close(
            capture_axis(&d.tree, &id(2), axis).unwrap().visible,
            fp.visible,
        );
    }
    let out = d.step(1_000_000, vec![]);
    assert!(out.result.is_ok(), "{:?}", out.result);
    assert!(d.runtime.groups.is_empty());
    close(extent(&d.tree, 2, Axis::Width), 400.0);
    close(extent(&d.tree, 2, Axis::Height), 200.0);
}

#[test]
fn multi_edit_orphan_root_wrappers_preserve_first_sources_across_clock_boundaries() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for attach_first in [false, true] {
                for root_first in [false, true] {
                    for at in [600_000, 1_000_000, 2_100_000] {
                        for cross in [false, true] {
                            let mut tree = transport_tree(axis);
                            tree.get_mut(&id(2))
                                .unwrap()
                                .spec
                                .declared
                                .animate
                                .as_mut()
                                .unwrap()
                                .repeat = AnimationRepeat::Times(3);
                            if cross {
                                tree.get_mut(&id(10)).unwrap().spec.kind = if axis == Axis::Width {
                                    ElementKind::Column
                                } else {
                                    ElementKind::Row
                                };
                            }
                            tree.insert(Element::with_attrs(
                                id(30),
                                ElementKind::El,
                                vec![],
                                Attrs {
                                    width: Some(Length::Px(300.0)),
                                    height: Some(Length::Px(300.0)),
                                    ..Default::default()
                                },
                            ));
                            tree.insert(Element::with_attrs(
                                id(40),
                                ElementKind::El,
                                vec![],
                                Attrs {
                                    width: Some(Length::Fill),
                                    height: Some(Length::Fill),
                                    ..Default::default()
                                },
                            ));
                            let mut d = Driver::new(tree, Instant::now(), mode);
                            assert!(d.step(0, vec![]).result.is_ok());
                            let published = d.step(500_000, vec![]).published_after.unwrap();
                            let source = capture_axis(&d.tree, &id(2), axis).unwrap();
                            let key =
                                d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)].key;
                            let root = |d: &mut Driver| {
                                d.tree.set_children(&id(40), vec![id(20)]).unwrap();
                                d.tree.set_root_id(id(40));
                            };
                            if root_first {
                                root(&mut d);
                            }
                            let move_to = |d: &mut Driver, from, to, from_children, to_children| {
                                let remove = Patch::SetChildren {
                                    id: from,
                                    children: from_children,
                                };
                                let add = Patch::SetChildren {
                                    id: to,
                                    children: to_children,
                                };
                                apply_patches(
                                    &mut d.tree,
                                    if attach_first {
                                        vec![add, remove]
                                    } else {
                                        vec![remove, add]
                                    },
                                )
                                .unwrap();
                            };
                            // The intermediate parent is an orphan, never a published attachment.
                            move_to(&mut d, id(1), id(30), vec![id(3)], vec![id(2)]);
                            move_to(&mut d, id(30), id(10), vec![], vec![id(11), id(2)]);
                            if !root_first {
                                root(&mut d);
                            }
                            let failed = d.attempt(at, vec![], Injection::BeforeLayout);
                            assert!(failed.result.is_err());
                            failed.published_after.unwrap().assert_same(&published);
                            let out = d.step(at + 1, vec![]);
                            assert!(
                                out.result.is_ok(),
                                "{axis:?} {mode:?} attach={attach_first} root={root_first} at={at} cross={cross}: {:?}",
                                out.result
                            );
                            close(
                                capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                                source.visible,
                            );
                            let next =
                                d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)].key;
                            assert_eq!(
                                (next.started, next.generation, next.mount),
                                (key.started, key.generation, key.mount)
                            );
                            assert!(d.step(at + 100_000, vec![]).result.is_ok());
                            let out = d.step(3_000_000, vec![]);
                            assert!(out.result.is_ok(), "{:?}", out.result);
                            assert!(d.runtime.groups.is_empty());
                            let expected = out
                                .inspection
                                .queries
                                .iter()
                                .find(|q| q.kind == super::super::inspection::QueryKind::Release)
                                .unwrap()
                                .result
                                .as_ref()
                                .unwrap()[&(id(2), axis)];
                            assert!(
                                expected
                                    .release_matches(capture_axis(&d.tree, &id(2), axis).unwrap())
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn actual_root_ignores_stale_nearby_parent_roles_without_interrupting_its_curve() {
    use crate::tree::element::{NearbyMount, NearbySlot};
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut tree = transport_tree(Axis::Width);
        tree.set_root_id(id(2));
        let mut d = Driver::new(tree, Instant::now(), mode);
        assert!(d.step(0, vec![]).result.is_ok());
        assert!(d.step(500_000, vec![]).result.is_ok());
        d.tree
            .set_nearby_mounts(
                &id(10),
                vec![NearbyMount {
                    id: id(2),
                    slot: NearbySlot::InFront,
                }],
            )
            .unwrap();
        let out = d.step(750_000, vec![]);
        assert!(out.result.is_ok(), "{:?}", out.result);
        close(
            capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
            460.0,
        );
        assert!(
            !out.inspection
                .queries
                .iter()
                .any(|q| q.kind == super::super::inspection::QueryKind::Transport)
        );
        assert!(d.step(1_000_000, vec![]).result.is_ok());
    }
}

#[test]
fn complex_ghost_transport_and_remount_survive_interrupted_terminal_cleanup() {
    use crate::tree::element::{NearbyMount, NearbySlot};
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for scale in [0.5, 2.0] {
                for floated in [false, true] {
                    let assets = crate::assets::AssetRuntime::new();
                    let _assets = assets.enter();
                    let svg=resvg::usvg::Tree::from_str("<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'><rect width='40' height='20' fill='red'/></svg>",&resvg::usvg::Options::default()).unwrap();
                    crate::assets::register_vector_asset("ghost-media", svg).unwrap();
                    let mut tree = transport_tree(axis);
                    let owner = tree.get_mut(&id(2)).unwrap();
                    owner.spec.kind = if floated {
                        ElementKind::TextColumn
                    } else {
                        ElementKind::WrappedRow
                    };
                    owner.spec.declared.animate_exit =
                        Some(spec(Length::Px(1.0), Length::Px(0.0), 400.0, axis));
                    owner.spec.declared.layout_scale = Some(scale);
                    owner.spec.declared.layout_rotate = Some(15.0);
                    for n in 50..54 {
                        tree.insert(Element::with_attrs(
                            id(n),
                            if n == 50 {
                                ElementKind::Image
                            } else {
                                ElementKind::Text
                            },
                            vec![],
                            Attrs {
                                width: Some(Length::Px(40.0)),
                                height: Some(Length::Px(20.0)),
                                content: (n != 50).then(|| "ghost baseline".into()),
                                image_src: (n == 50).then(|| {
                                    crate::tree::attrs::ImageSource::Id("ghost-media".into())
                                }),
                                align_x: (floated && n == 50)
                                    .then_some(crate::tree::attrs::AlignX::Left),
                                on_mouse_move: Some(true),
                                ..Default::default()
                            },
                        ));
                    }
                    tree.set_children(&id(2), vec![id(50), id(51), id(52)])
                        .unwrap();
                    tree.set_nearby_mounts(
                        &id(2),
                        vec![NearbyMount {
                            id: id(53),
                            slot: NearbySlot::InFront,
                        }],
                    )
                    .unwrap();
                    let mut d = Driver::new(tree, Instant::now(), mode);
                    assert!(d.step(0, vec![]).result.is_ok());
                    assert!(d.step(500_000, vec![]).result.is_ok());
                    reparent(&mut d);
                    let before = d.step(600_000, vec![]);
                    assert!(before.result.is_ok(), "{:?}", before.result);
                    let before = before.published_after.unwrap();
                    let source = capture_axis(&d.tree, &id(2), axis).unwrap();
                    let mount = d.tree.get(&id(2)).unwrap().lifecycle.mounted_at_revision;
                    let mut renderer = crate::renderer::SceneRenderer::new();
                    let old_pixels = super::rendered::raster(
                        &mut renderer,
                        before.output.scene.clone(),
                        600,
                        600,
                        1,
                    );
                    apply_patches(&mut d.tree, vec![Patch::Remove { id: id(2) }]).unwrap();
                    assert!(
                        d.attempt(700_000, vec![], Injection::BeforeLayout)
                            .result
                            .is_err()
                    );
                    let out = d.step(700_000, vec![]);
                    assert!(
                        out.result.is_ok(),
                        "{axis:?} {mode:?} {scale} {floated}: {:?}",
                        out.result
                    );
                    let ghost = d.tree.iter_nodes().find(|n| n.is_ghost_root()).unwrap().id;
                    let ghost_ids = d
                        .tree
                        .iter_nodes()
                        .filter(|n| n.is_ghost())
                        .map(|n| n.id)
                        .collect::<HashSet<_>>();
                    assert_eq!(ghost_ids.len(), 5);
                    close(
                        capture_axis(&d.tree, &ghost, axis).unwrap().visible,
                        source.visible * scale as f32
                            / d.tree.get(&ghost).unwrap().layout.dimension_facts.scale,
                    );
                    assert!(
                        !out.published_after
                            .unwrap()
                            .output
                            .event_rebuild
                            .base_registry
                            .view()
                            .iter_precedence()
                            .any(|listener| listener
                                .element_id
                                .is_some_and(|id| ghost_ids.contains(&id)))
                    );
                    assert!(d.step(900_000, vec![]).result.is_ok());
                    let replacement=resvg::usvg::Tree::from_str("<svg xmlns='http://www.w3.org/2000/svg' width='80' height='20'><rect width='80' height='20' fill='lime'/></svg>",&resvg::usvg::Options::default()).unwrap();
                    crate::assets::register_vector_asset("ghost-media", replacement).unwrap();
                    let terminal = d.step(1_100_000, vec![]);
                    assert!(terminal.result.is_ok());
                    let terminal = terminal.published_after.unwrap();
                    let mut subtree = ElementTree::new();
                    subtree.insert(Element::with_attrs(
                        id(2),
                        ElementKind::El,
                        vec![],
                        Attrs {
                            animate: Some(spec(Length::Px(80.0), Length::Fill, 1000.0, axis)),
                            ..Default::default()
                        },
                    ));
                    subtree.set_root_id(id(2));
                    apply_patches(
                        &mut d.tree,
                        vec![Patch::InsertSubtree {
                            parent_id: Some(id(10)),
                            index: 0,
                            subtree,
                        }],
                    )
                    .unwrap();
                    let failed = d.attempt(1_100_001, vec![], Injection::BeforeLayout);
                    assert!(failed.result.is_err());
                    failed.published_after.unwrap().assert_same(&terminal);
                    let out = d.step(1_200_000, vec![]);
                    assert!(out.result.is_ok(), "cleanup: {:?}", out.result);
                    assert!(d.tree.get(&ghost).is_none());
                    assert!(!d.tree.iter_nodes().any(|n| n.is_ghost()));
                    assert_ne!(
                        d.tree.get(&id(2)).unwrap().lifecycle.mounted_at_revision,
                        mount
                    );
                    assert_eq!(
                        old_pixels,
                        super::rendered::raster(
                            &mut renderer,
                            before.output.scene.clone(),
                            600,
                            600,
                            2
                        )
                    );
                    assert!(d.step(2_300_000, vec![]).result.is_ok());
                    assert!(d.runtime.groups.is_empty());
                }
            }
        }
    }
}

#[test]
fn arrivals_and_cancellations_at_migrated_hold_barriers_keep_native_joint_release() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for arrival in [1_250_000, 2_000_000] {
                for cancel in [false, true] {
                    let mut tree = transport_tree(axis);
                    tree.get_mut(&id(2))
                        .unwrap()
                        .spec
                        .declared
                        .animate
                        .as_mut()
                        .unwrap()
                        .duration_ms = 500.0;
                    tree.get_mut(&id(3)).unwrap().spec.declared.animate =
                        Some(spec(Length::Px(40.0), Length::Fill, 2000.0, axis));
                    let mut d = Driver::new(tree, Instant::now(), mode);
                    for at in [0, 500_000, 1_000_000] {
                        assert!(d.step(at, vec![]).result.is_ok());
                    }
                    let key = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)].key;
                    reparent(&mut d);
                    let before = d.step(1_100_000, vec![]).published_after.unwrap();
                    let mut attrs = d.attrs(id(11));
                    attrs.animate = Some(spec(Length::Px(40.0), Length::Fill, 2000.0, axis));
                    let failed = d.attempt(
                        arrival,
                        vec![Event::Attrs(id(11), Box::new(attrs))],
                        Injection::BeforeLayout,
                    );
                    assert!(failed.result.is_err());
                    failed.published_after.unwrap().assert_same(&before);
                    let out = d.step(arrival + 1, vec![]);
                    assert!(
                        out.result.is_ok(),
                        "{axis:?} {mode:?} {arrival}: {:?}",
                        out.result
                    );
                    assert_eq!(
                        d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)].key,
                        key
                    );
                    let end = if cancel {
                        let mut attrs = d.attrs(id(11));
                        attrs.animate = None;
                        let at = arrival + 500_000;
                        let out = d.step(at, vec![Event::Attrs(id(11), Box::new(attrs))]);
                        assert!(
                            out.result.is_ok(),
                            "cancel {axis:?} {mode:?} {arrival}: {:?}",
                            out.result
                        );
                        at.max(2_000_000)
                    } else {
                        arrival + 2_000_000
                    };
                    let out = d.step(end, vec![]);
                    assert!(
                        out.result.is_ok(),
                        "release {axis:?} {mode:?} {arrival} cancel={cancel}: {:?}",
                        out.result
                    );
                    assert!(d.runtime.groups.is_empty());
                    close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 200.0);
                    close(capture_axis(&d.tree, &id(11), axis).unwrap().visible, 200.0);
                }
            }
        }
    }
}

#[test]
fn clearing_an_orphan_root_before_reattachment_keeps_its_published_source() {
    use crate::tree::element::{NearbyMount, NearbySlot};
    let mut tree = transport_tree(Axis::Width);
    tree.set_root_id(id(2));
    tree.set_nearby_mounts(
        &id(10),
        vec![NearbyMount {
            id: id(2),
            slot: NearbySlot::InFront,
        }],
    )
    .unwrap();
    tree.insert(Element::with_attrs(
        id(40),
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(Length::Px(400.0)),
            height: Some(Length::Px(400.0)),
            ..Default::default()
        },
    ));
    let mut d = Driver::new(tree, Instant::now(), Mode::Active);
    assert!(d.step(0, vec![]).result.is_ok());
    assert!(d.step(500_000, vec![]).result.is_ok());
    let source = capture_axis(&d.tree, &id(2), Axis::Width).unwrap();
    d.tree.clear_root();
    d.tree.set_children(&id(40), vec![id(2)]).unwrap();
    d.tree.set_nearby_mounts(&id(10), vec![]).unwrap();
    d.tree.set_root_id(id(40));
    let out = d.step(750_000, vec![]);
    assert!(out.result.is_ok(), "{:?}", out.result);
    close(
        capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
        source.visible,
    );
    assert!(d.step(1_000_000, vec![]).result.is_ok());
    close(
        capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
        400.0,
    );
}

#[test]
fn rebased_inline_decorations_survive_dimension_transport_alignment_and_ghost_cleanup() {
    use super::support::Publication;
    use crate::renderer::SceneRenderer;
    use crate::tree::attrs::{BorderRadius, BorderWidth, BoxShadow, Padding, TextAlign};
    fn check(d: &Driver, p: &Publication, renderer: &mut SceneRenderer, version: u64, size: u32) {
        for node in d
            .tree
            .iter_nodes()
            .filter(|n| n.spec.kind == ElementKind::Paragraph)
        {
            let fragments = node.layout.paragraph_fragments.as_ref().unwrap();
            assert!(!node.layout.paragraph_boxes.is_empty());
            for b in &node.layout.paragraph_boxes {
                assert!(
                    b.text_range.start < b.text_range.end && b.text_range.end <= fragments.len()
                );
                let owner = d
                    .tree
                    .get(&b.owner)
                    .expect("inline owner must survive remapping");
                assert_eq!(owner.is_ghost(), node.is_ghost());
                assert!(b.frame.width > 0.0 && b.frame.height > 0.0);
            }
        }
        let pixels = super::rendered::raster(renderer, p.output.scene.clone(), size, size, version);
        let fresh = super::rendered::raster(
            &mut SceneRenderer::new(),
            p.output.scene.clone(),
            size,
            size,
            version,
        );
        assert_eq!(
            pixels, fresh,
            "cached inline paint must match fresh drawing"
        );
    }
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for scale in [0.5, 2.0] {
            for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
                let assets = crate::assets::AssetRuntime::new();
                let _assets = assets.enter();
                let mut tree = transport_tree(Axis::Width);
                let paragraph = tree.get_mut(&id(2)).unwrap();
                paragraph.spec.kind = ElementKind::Paragraph;
                paragraph.spec.declared = Attrs {
                    width: Some(Length::Px(180.0)),
                    height: Some(Length::Content),
                    padding: Some(Padding::Uniform(10.0)),
                    font_size: Some(20.0),
                    on_click: Some(true),
                    animate: Some(spec(Length::Px(180.0), Length::Fill, 1000.0, Axis::Width)),
                    animate_exit: Some(spec(Length::Fill, Length::Px(80.0), 250.0, Axis::Width)),
                    ..Default::default()
                };
                tree.get_mut(&id(20)).unwrap().spec.declared.text_align = Some(align);
                tree.insert(Element::with_attrs(
                    id(4),
                    ElementKind::El,
                    vec![],
                    Attrs {
                        padding: Some(Padding::Uniform(3.0)),
                        border_width: Some(BorderWidth::Uniform(2.0)),
                        border_color: Some(Color::Named("blue".into())),
                        background: Some(Background::Color(Color::Named("white".into()))),
                        border_radius: Some(BorderRadius::Corners {
                            tl: 9.0,
                            tr: 1.0,
                            br: 5.0,
                            bl: 0.0,
                        }),
                        box_shadows: Some(vec![BoxShadow {
                            offset_x: 3.0,
                            offset_y: 2.0,
                            blur: 4.0,
                            size: 2.0,
                            color: Color::Named("red".into()),
                            inset: false,
                        }]),
                        ..Default::default()
                    },
                ));
                tree.insert(Element::with_attrs(
                    id(5),
                    ElementKind::Text,
                    vec![],
                    Attrs {
                        content: Some("AA BB CC DD EE FF GG HH II JJ KK LL".into()),
                        ..Default::default()
                    },
                ));
                tree.set_children(&id(2), vec![id(4)]).unwrap();
                tree.set_children(&id(4), vec![id(5)]).unwrap();
                let mut d = Driver::new(tree, Instant::now(), mode);
                let mut renderer = SceneRenderer::new();
                let size = (600.0 * scale) as u32;
                let first = d.step(
                    0,
                    vec![Event::Viewport {
                        width: size as f32,
                        height: size as f32,
                        scale,
                    }],
                );
                assert!(first.result.is_ok(), "{:?}", first.result);
                let first = first.published_after.unwrap();
                let old = super::rendered::raster(
                    &mut renderer,
                    first.output.scene.clone(),
                    size,
                    size,
                    1,
                );
                for us in [250_000, 500_000] {
                    let out = d.step(us, vec![]);
                    assert!(out.result.is_ok(), "{:?}", out.result);
                    check(&d, &out.published_after.unwrap(), &mut renderer, us, size);
                }
                let source = capture_axis(&d.tree, &id(2), Axis::Width).unwrap();
                let key = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key;
                reparent(&mut d);
                let mut attrs = d.tree.get(&id(20)).unwrap().spec.declared.clone();
                attrs.text_align = Some(TextAlign::Right);
                let failure = d.attempt(
                    600_000,
                    vec![Event::Attrs(id(20), Box::new(attrs))],
                    Injection::BeforeLayout,
                );
                assert!(failure.result.is_err());
                failure
                    .published_before
                    .unwrap()
                    .assert_same(&failure.published_after.unwrap());
                let moved = d.step(650_000, vec![]);
                assert!(
                    moved.result.is_ok(),
                    "{mode:?} {scale} {align:?}: {:?}",
                    moved.result
                );
                close(
                    capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
                    source.visible,
                );
                assert_eq!(
                    d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key,
                    key
                );
                check(
                    &d,
                    &moved.published_after.unwrap(),
                    &mut renderer,
                    650_000,
                    size,
                );
                for us in [750_000, 1_000_000] {
                    let out = d.step(us, vec![]);
                    assert!(out.result.is_ok(), "{:?}", out.result);
                    check(&d, &out.published_after.unwrap(), &mut renderer, us, size);
                }
                assert!(d.runtime.groups.is_empty());
                apply_patches(&mut d.tree, vec![Patch::Remove { id: id(2) }]).unwrap();
                // Before any reflow, copied decoration owners must already refer
                // to the cloned ghost, not the now-removed live wrapper.
                let ghost = d.tree.iter_nodes().find(|n| n.is_ghost_root()).unwrap();
                assert!(!ghost.layout.paragraph_boxes.is_empty());
                assert!(
                    ghost.layout.paragraph_boxes.iter().all(|b| d
                        .tree
                        .get(&b.owner)
                        .unwrap()
                        .is_ghost())
                );
                for us in [1_100_000, 1_200_000, 1_350_000] {
                    let out = d.step(us, vec![]);
                    assert!(
                        out.result.is_ok(),
                        "ghost {mode:?} {scale}: {:?}",
                        out.result
                    );
                    let publication = out.published_after.unwrap();
                    check(&d, &publication, &mut renderer, us, size);
                    assert!(
                        !publication
                            .output
                            .event_rebuild
                            .base_registry
                            .view()
                            .iter_precedence()
                            .any(|l| l
                                .element_id
                                .is_some_and(|id| d.tree.get(&id).is_some_and(|n| n.is_ghost())))
                    );
                }
                assert!(
                    d.attempt(1_350_001, vec![], Injection::BeforeLayout)
                        .result
                        .is_err()
                );
                let cleanup = d.step(1_350_002, vec![]);
                assert!(cleanup.result.is_ok(), "{:?}", cleanup.result);
                assert!(!d.tree.iter_nodes().any(|n| n.is_ghost()));
                check(
                    &d,
                    &cleanup.published_after.unwrap(),
                    &mut renderer,
                    1_350_002,
                    size,
                );
                assert_eq!(
                    old,
                    super::rendered::raster(
                        &mut renderer,
                        first.output.scene.clone(),
                        size,
                        size,
                        1_400_000
                    )
                );
            }
        }
    }
}
