use super::*;
use crate::tree::{
    attrs::{Attrs, Length},
    element::{Element, ElementKind, NodeId},
};

fn tree(source: ImageSource) -> ElementTree {
    let mut tree = ElementTree::new();
    tree.insert(Element::with_attrs(
        NodeId(1),
        ElementKind::Image,
        vec![],
        Attrs {
            image_src: Some(source),
            width: Some(Length::Px(16.0)),
            height: Some(Length::Px(16.0)),
            ..Default::default()
        },
    ));
    tree.set_root_id(NodeId(1));
    tree
}
fn raster(id: &str, width: u32, height: u32) {
    crate::renderer::insert_test_raster_asset_rgba(
        id,
        width,
        height,
        &[255, 0, 0, 255].repeat((width * height) as usize),
    )
    .unwrap();
}

#[test]
fn direct_capture_registers_without_pruning_or_status_only_binding_reuse() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    raster("kept", 1, 1);
    raster("unrelated", 1, 1);
    let unrelated = ImageSource::Id("unrelated".into());
    ensure_source(&unrelated);
    let mut tree = tree(ImageSource::Id("kept".into()));
    reset_source_index_diagnostics_for_benchmark();
    let old = tree.capture_frame_assets();
    let work = take_source_index_diagnostics_for_benchmark();
    assert_eq!(
        (
            work.retained_list_requests,
            work.reconcile_passes,
            work.registration_passes,
            work.frame_asset_captures
        ),
        (1, 0, 1, 1)
    );
    assert!(source_status(&unrelated).is_some());
    raster("kept", 2, 3);
    let new = tree.capture_frame_assets();
    assert_eq!(old.images.dimensions("kept"), Some((1, 1)));
    assert_eq!(new.images.dimensions("kept"), Some((2, 3)));
    assert!(source_status(&unrelated).is_some());
}

#[test]
fn reconciliation_moves_removed_ids_borrows_retained_aliases_and_keeps_old_pins() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    raster("shared", 1, 1);
    let retained = ImageSource::Logical("retained".into());
    let removed = ImageSource::Logical("removed".into());
    {
        let context = current_context();
        let mut state = context.state.lock().unwrap();
        for source in [&retained, &removed] {
            state.set_source_status(
                source.clone(),
                AssetStatus::Ready(ResolvedAsset {
                    id: "shared".into(),
                    width: 1,
                    height: 1,
                }),
            );
        }
    }
    let mut tree = tree(retained.clone());
    let old = tree.capture_frame_assets();
    reset_source_index_diagnostics_for_benchmark();
    let sources = prepare_tree_sources(&tree);
    let preparation = crate::tree::layout::prepare_frame_attrs_with_sources_for_update(
        &mut tree, 1.0, None, None, &sources, None,
    );
    let work = take_source_index_diagnostics_for_benchmark();
    assert_eq!(
        (
            work.retained_list_requests,
            work.reconcile_passes,
            work.registration_passes,
            work.frame_asset_captures
        ),
        (1, 1, 1, 1)
    );
    assert!(source_status(&removed).is_none());
    assert!(source_status(&retained).is_some());
    assert!(
        current_context()
            .state
            .lock()
            .unwrap()
            .records
            .contains_key("shared")
    );
    drop(preparation);
    tree.get_mut(&NodeId(1)).unwrap().spec.declared.image_src = None;
    ensure_tree_sources(&tree);
    assert!(source_status(&retained).is_none());
    assert!(
        !current_context()
            .state
            .lock()
            .unwrap()
            .records
            .contains_key("shared")
    );
    assert_eq!(old.images.dimensions("shared"), Some((1, 1)));
}
