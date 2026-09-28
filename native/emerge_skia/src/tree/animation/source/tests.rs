use super::*;
use crate::tree::{
    attrs::{Length, MouseOverAttrs},
    element::{Element, ElementKind},
    layout::{Constraint, layout_tree_default},
    patch::{Patch, apply_patches},
};

fn id(n: u64) -> NodeId {
    NodeId::from_wire_u64(n)
}
fn node(n: u64, kind: ElementKind, attrs: Attrs) -> Element {
    Element::with_attrs(id(n), kind, Vec::new(), attrs)
}
fn text_raw(text: &str) -> Vec<u8> {
    [
        vec![0, 1, 21],
        (text.len() as u16).to_be_bytes().to_vec(),
        text.as_bytes().to_vec(),
    ]
    .concat()
}
fn width_raw(width: f64) -> Vec<u8> {
    [vec![0, 1, 1, 2], width.to_be_bytes().to_vec()].concat()
}
fn text_tree() -> ElementTree {
    let mut tree = ElementTree::new();
    tree.set_root_id(id(1));
    tree.insert(node(
        1,
        ElementKind::Column,
        Attrs {
            font_size: Some(18.0),
            ..Attrs::default()
        },
    ));
    tree.insert(node(2, ElementKind::El, Attrs::default()));
    tree.insert(node(
        3,
        ElementKind::Text,
        Attrs {
            content: Some("a".into()),
            ..Attrs::default()
        },
    ));
    tree.set_children(&id(1), vec![id(2)]).unwrap();
    tree.set_children(&id(2), vec![id(3)]).unwrap();
    tree
}
fn layout(tree: &mut ElementTree, scale: f32) {
    layout_tree_default(tree, Constraint::new(600.0, 200.0), scale);
}

#[test]
fn first_write_survives_coalescing_and_failed_prefix_then_releases_on_frame() {
    let mut tree = text_tree();
    layout(&mut tree, 2.0);
    let before: Vec<_> = (1..=3)
        .map(|n| PresentationSource::capture(&tree, &id(n)).unwrap())
        .collect();
    apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: id(3),
            attrs_raw: text_raw("a longer label"),
        }],
    )
    .unwrap();
    assert_eq!(tree.pending_patch_effects.sources.len(), 3);
    let result = apply_patches(
        &mut tree,
        vec![
            Patch::SetAttrs {
                id: id(3),
                attrs_raw: text_raw("latest successful target"),
            },
            Patch::SetAttrs {
                id: id(3),
                attrs_raw: vec![255],
            },
            Patch::SetAttrs {
                id: id(3),
                attrs_raw: text_raw("not reached"),
            },
        ],
    );
    assert!(result.is_err());
    assert_eq!(
        tree.get(&id(3)).unwrap().spec.declared.content.as_deref(),
        Some("latest successful target")
    );
    for (index, source) in before.iter().enumerate() {
        let captured = &tree.pending_patch_effects.sources[&id(index as u64 + 1)];
        assert_eq!(captured.dimensions, source.dimensions);
        assert_eq!(captured.effective, source.effective);
        assert_eq!(captured.declared, source.declared);
        assert_eq!(captured.scale, 2.0);
    }
    assert_eq!(
        tree.pending_patch_effects.invalidation,
        TreeInvalidation::Paint
    );
    assert!(tree.pending_patch_effects.model_changed);
    layout(&mut tree, 2.0);
    assert!(tree.pending_patch_effects.sources.is_empty());
    assert_eq!(
        tree.pending_patch_effects.invalidation,
        TreeInvalidation::None
    );
}

#[test]
fn manual_text_width_facts_match_fresh_layout_at_each_scale() {
    for scale in [0.5, 1.0, 2.0] {
        let mut tree = text_tree();
        layout(&mut tree, scale);
        for label in ["a wide label", "b", "a wider label again"] {
            assert_eq!(
                apply_patches(
                    &mut tree,
                    vec![Patch::SetAttrs {
                        id: id(3),
                        attrs_raw: text_raw(label)
                    }]
                )
                .unwrap(),
                TreeInvalidation::Paint
            );
            let mut fresh = tree.layout_query_snapshot();
            layout(&mut fresh, scale);
            for n in 1..=3 {
                for axis in [Axis::Width, Axis::Height] {
                    assert_eq!(
                        capture_axis(&tree, &id(n), axis),
                        capture_axis(&fresh, &id(n), axis),
                        "node {n} axis {axis:?} scale {scale}"
                    );
                }
                assert_eq!(
                    tree.get(&id(n)).unwrap().layout.frame,
                    fresh.get(&id(n)).unwrap().layout.frame
                );
            }
        }
    }
}

#[test]
fn same_declaration_does_not_replace_scaled_presentation_or_create_history() {
    let mut tree = ElementTree::new();
    tree.set_root_id(id(1));
    tree.insert(node(
        1,
        ElementKind::El,
        Attrs {
            width: Some(Length::Px(40.0)),
            ..Attrs::default()
        },
    ));
    layout(&mut tree, 2.0);
    assert_eq!(
        apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id: id(1),
                attrs_raw: width_raw(40.0)
            }]
        )
        .unwrap(),
        TreeInvalidation::None
    );
    assert_eq!(
        tree.get(&id(1)).unwrap().layout.effective.width,
        Some(Length::Px(80.0))
    );
    assert!(tree.pending_patch_effects.sources.is_empty());
}

#[test]
fn captured_mixed_footprints_do_not_become_compatible_pixel_sources() {
    use crate::tree::layout::dimensions::set_axis_sample;
    let mut tree = ElementTree::new();
    tree.set_root_id(id(1));
    tree.insert(node(
        1,
        ElementKind::Row,
        Attrs {
            width: Some(Length::Px(600.0)),
            ..Attrs::default()
        },
    ));
    tree.insert(node(
        2,
        ElementKind::El,
        Attrs {
            width: Some(Length::Min(
                Box::new(Length::Px(50.0)),
                Box::new(Length::Fill),
            )),
            ..Attrs::default()
        },
    ));
    tree.insert(node(
        3,
        ElementKind::El,
        Attrs {
            width: Some(Length::Fill),
            ..Attrs::default()
        },
    ));
    tree.set_children(&id(1), vec![id(2), id(3)]).unwrap();
    layout(&mut tree, 2.0);
    let mut sample = capture_axis(&tree, &id(2), Axis::Width).unwrap();
    sample.visible = 45.0;
    sample.initial = 45.0;
    sample.charge = 170.0;
    sample.parent_extent = 45.0;
    set_axis_sample(&mut tree, &id(2), Axis::Width, Some(sample)).unwrap();
    layout(&mut tree, 2.0);
    let whole = capture_axis(&tree, &id(2), Axis::Width);
    apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: id(2),
            attrs_raw: width_raw(100.0),
        }],
    )
    .unwrap();
    let source = &tree.pending_patch_effects.sources[&id(2)];
    assert_eq!(source.dimensions[0], whole);
    assert!(source.sampled[0]);
    assert_eq!(source.dimensions[0].unwrap().charge, 170.0);
}

#[test]
fn reparent_captures_old_scope_before_parent_links_change() {
    let mut tree = text_tree();
    tree.get_mut(&id(1)).unwrap().spec.kind = ElementKind::Row;
    tree.insert(node(4, ElementKind::Row, Attrs::default()));
    layout(&mut tree, 1.0);
    let old = capture_axis(&tree, &id(2), Axis::Width);
    assert!(old.unwrap().scope.is_some());
    apply_patches(
        &mut tree,
        vec![
            Patch::SetChildren {
                id: id(1),
                children: vec![id(4)],
            },
            Patch::SetChildren {
                id: id(4),
                children: vec![id(2)],
            },
        ],
    )
    .unwrap();
    assert_eq!(
        tree.pending_patch_effects.sources[&id(2)].dimensions[0],
        old
    );
    assert_ne!(
        capture_axis(&tree, &id(2), Axis::Width).unwrap().scope,
        old.unwrap().scope
    );
    assert!(tree.pending_patch_effects.model_changed);
}

#[test]
fn model_effects_exclude_animation_policy_and_paint_only_edits() {
    use crate::tree::animation::{AnimationCurve, AnimationRepeat, AnimationSpec};
    use crate::tree::invalidation::layout_model_attrs_changed;
    let before = Attrs {
        animate: Some(AnimationSpec {
            keyframes: vec![Attrs::default(); 2],
            duration_ms: 100.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        }),
        mouse_over: Some(MouseOverAttrs {
            font_size: Some(24.0),
            ..MouseOverAttrs::default()
        }),
        ..Attrs::default()
    };
    let mut after = before.clone();
    after.animate.as_mut().unwrap().duration_ms = 200.0;
    after.alpha = Some(0.5);
    after.mouse_over.as_mut().unwrap().font_underline = Some(true);
    assert!(!layout_model_attrs_changed(&before, &after));
    after.mouse_over.as_mut().unwrap().font_size = Some(25.0);
    assert!(layout_model_attrs_changed(&before, &after));
}

#[test]
fn content_column_in_row_uses_layout_instead_of_leaving_sibling_positions_stale() {
    let mut tree = text_tree();
    tree.insert(node(4, ElementKind::Row, Attrs::default()));
    tree.insert(node(
        5,
        ElementKind::El,
        Attrs {
            width: Some(Length::Px(20.0)),
            ..Attrs::default()
        },
    ));
    tree.set_children(&id(4), vec![id(1), id(5)]).unwrap();
    tree.set_root_id(id(4));
    layout(&mut tree, 1.0);
    let old_x = tree.get(&id(5)).unwrap().layout.frame.unwrap().x;
    assert_eq!(
        apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id: id(3),
                attrs_raw: text_raw("a much longer label")
            }]
        )
        .unwrap(),
        TreeInvalidation::Measure
    );
    layout(&mut tree, 1.0);
    assert!(tree.get(&id(5)).unwrap().layout.frame.unwrap().x > old_x);
}

#[test]
fn fixed_column_overflow_and_intrinsic_width_remain_distinct() {
    let mut tree = text_tree();
    tree.get_mut(&id(1)).unwrap().spec.declared.width = Some(Length::Px(20.0));
    layout(&mut tree, 1.0);
    assert_eq!(
        apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id: id(3),
                attrs_raw: text_raw("a very wide label")
            }]
        )
        .unwrap(),
        TreeInvalidation::Paint
    );
    let mut fresh = tree.layout_query_snapshot();
    layout(&mut fresh, 1.0);
    assert_eq!(
        tree.get(&id(1)).unwrap().layout.frame,
        fresh.get(&id(1)).unwrap().layout.frame
    );
    assert_eq!(
        capture_axis(&tree, &id(1), Axis::Width),
        capture_axis(&fresh, &id(1), Axis::Width)
    );
}

#[test]
fn remount_and_empty_tree_frames_release_pending_sources() {
    let mut tree = text_tree();
    layout(&mut tree, 1.0);
    apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: id(3),
            attrs_raw: text_raw("changed"),
        }],
    )
    .unwrap();
    assert!(!tree.pending_patch_effects.sources.is_empty());
    let uploaded = tree.clone();
    tree.replace_with_uploaded(uploaded);
    assert!(tree.pending_patch_effects.sources.is_empty());
    assert!(
        PresentationSource::capture(&tree, &id(3)).is_none(),
        "cached frames are not a publication of a new upload"
    );
    layout(&mut tree, 1.0);
    assert!(PresentationSource::capture(&tree, &id(3)).is_some());
    apply_patches(&mut tree, vec![Patch::Remove { id: id(1) }]).unwrap();
    assert!(!tree.pending_patch_effects.sources.is_empty());
    layout(&mut tree, 1.0);
    assert!(tree.pending_patch_effects.sources.is_empty());
}

#[test]
fn dimension_owned_text_does_not_take_the_manual_resize_shortcut() {
    use crate::tree::layout::dimensions::set_axis_sample;
    let mut tree = text_tree();
    layout(&mut tree, 1.0);
    let source = capture_axis(&tree, &id(3), Axis::Width).unwrap();
    set_axis_sample(&mut tree, &id(3), Axis::Width, Some(source)).unwrap();
    layout(&mut tree, 1.0);
    assert_eq!(
        apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id: id(3),
                attrs_raw: text_raw("changed")
            }]
        )
        .unwrap(),
        TreeInvalidation::Measure
    );
    assert_eq!(
        tree.pending_patch_effects.sources[&id(3)].dimensions[0],
        Some(source)
    );
}
