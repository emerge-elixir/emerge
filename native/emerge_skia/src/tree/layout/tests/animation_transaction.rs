use super::super::*;
use crate::tree::{attrs::Attrs, element::Element};

#[test]
fn prepared_font_environment_survives_load_until_next_frame_and_remeasures_then() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let id = NodeId(42);
    let mut tree = ElementTree::new();
    tree.insert(Element::with_attrs(
        id,
        ElementKind::Text,
        vec![],
        Attrs {
            content: Some("Frozen font environment 123".into()),
            font: Some(crate::tree::attrs::Font::String("transaction-font".into())),
            font_size: Some(24.0),
            ..Default::default()
        },
    ));
    tree.set_root_id(id);
    let constraint = Constraint::new(800.0, 600.0);
    let mut runtime = AnimationRuntime::default();
    let start = Instant::now();
    layout_and_refresh_default_with_animation(&mut tree, constraint, 1.0, &mut runtime, start)
        .unwrap();
    let old = tree.get(&id).unwrap().layout.frame.unwrap();
    let epoch = tree.frame_metrics_epoch;
    let prep = prepare_frame_attrs_for_update(&mut tree, 1.0, Some(&mut runtime), Some(start));
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../priv/test_assets/Lobster-Regular.ttf"),
    )
    .unwrap();
    crate::services::load_font_bytes(&assets, "transaction-font", 400, false, &bytes).unwrap();
    prep.publish(&mut tree, &mut runtime, constraint, true, None)
        .unwrap();
    assert_eq!(old, tree.get(&id).unwrap().layout.frame.unwrap());
    assert_eq!(epoch, tree.frame_metrics_epoch);
    let next =
        layout_or_refresh_default_with_animation(&mut tree, constraint, 1.0, &mut runtime, start)
            .unwrap();
    assert!(next.layout_performed);
    assert_ne!(epoch, tree.frame_metrics_epoch);
    let new = tree.get(&id).unwrap().layout.frame.unwrap();
    assert_ne!(old.width, new.width);
    let warm =
        layout_or_refresh_default_with_animation(&mut tree, constraint, 1.0, &mut runtime, start)
            .unwrap();
    assert_eq!(next.output.scene, warm.output.scene);
    assert_eq!(new, tree.get(&id).unwrap().layout.frame.unwrap());
}

#[test]
fn cancelling_last_resolved_run_does_not_freeze_stale_image_context_for_cleanup() {
    use crate::tree::{
        animation::{AnimationCurve, AnimationRepeat, AnimationSpec},
        attrs::Length,
    };
    let id = NodeId(73);
    let mut tree = ElementTree::new();
    tree.insert(Element::with_attrs(
        id,
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(Length::Px(40.0)),
            height: Some(Length::Px(40.0)),
            animate: Some(AnimationSpec {
                keyframes: vec![
                    Attrs {
                        width: Some(Length::Px(40.0)),
                        ..Default::default()
                    },
                    Attrs {
                        width: Some(Length::Fill),
                        ..Default::default()
                    },
                ],
                duration_ms: 1000.0,
                curve: AnimationCurve::Linear,
                repeat: AnimationRepeat::Once,
            }),
            ..Default::default()
        },
    ));
    tree.set_root_id(id);
    let mut runtime = AnimationRuntime::default();
    let now = Instant::now();
    let constraint = Constraint::new(600.0, 600.0);
    layout_and_refresh_default_with_animation(&mut tree, constraint, 1.0, &mut runtime, now)
        .unwrap();
    let next = prepare_frame_attrs_for_update(&mut tree, 1.0, Some(&mut runtime), Some(now));
    assert!(next.context.is_some());
    drop(next);
    tree.capture_animation_source(&id);
    tree.get_mut(&id).unwrap().spec.declared.animate = None;
    tree.bump_revision();
    tree.layout_model_epoch += 1;
    runtime.sync_with_tree(&tree, now);
    let cleanup = prepare_frame_attrs_for_update(&mut tree, 1.0, Some(&mut runtime), Some(now));
    assert!(cleanup.commit_required);
    assert!(
        cleanup.context.is_none(),
        "a cleanup has no new endpoint query to freeze"
    );
    cleanup
        .publish(&mut tree, &mut runtime, constraint, true, None)
        .unwrap();
    assert!(tree.length_runtime.is_none());
    assert!(tree.pending_patch_effects.sources.is_empty());
}

#[test]
fn equal_font_generations_in_distinct_renderers_do_not_reuse_native_metrics() {
    let first = crate::assets::AssetRuntime::new();
    let second = crate::assets::AssetRuntime::new();
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../priv/test_assets/Lobster-Regular.ttf"),
    )
    .unwrap();
    crate::services::load_font_bytes(&first, "scope-font", 400, false, &bytes).unwrap();
    crate::services::load_font_bytes(&second, "other-font", 400, false, &bytes).unwrap();
    let node = NodeId(901);
    let mut tree = ElementTree::new();
    tree.insert(Element::with_attrs(
        node,
        ElementKind::Text,
        vec![],
        Attrs {
            font: Some(crate::tree::attrs::Font::String("scope-font".into())),
            font_size: Some(24.0),
            content: Some("Independent metrics 123".into()),
            ..Default::default()
        },
    ));
    tree.set_root_id(node);
    let constraint = Constraint::new(800.0, 600.0);
    let now = Instant::now();
    let mut runtime = AnimationRuntime::default();
    let (before, epoch) = {
        let _first = first.enter();
        layout_and_refresh_default_with_animation(&mut tree, constraint, 1.0, &mut runtime, now)
            .unwrap();
        (
            tree.get(&node).unwrap().layout.frame.unwrap(),
            tree.frame_metrics_epoch,
        )
    };
    let _second = second.enter();
    let output =
        layout_or_refresh_default_with_animation(&mut tree, constraint, 1.0, &mut runtime, now)
            .unwrap();
    assert!(output.layout_performed);
    assert_eq!(epoch, tree.frame_metrics_epoch);
    assert_ne!(
        before.width,
        tree.get(&node).unwrap().layout.frame.unwrap().width
    );
}
