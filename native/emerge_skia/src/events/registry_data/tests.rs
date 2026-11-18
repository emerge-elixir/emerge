use super::*;
use crate::{actors::ListenerBarrier, events::registry_builder::FocusRevealScroll};

fn focus_data(mount: u64, dy: f32, initial: bool) -> RegistryRebuildPayload {
    let id = NodeId(7);
    let key = FocusKey {
        element_id: id,
        mounted_at_revision: mount,
    };
    RegistryRebuildPayload::new(CompiledRegistryData {
        mount_focus_targets: Some(Arc::new(HashMap::from([(
            id,
            FocusOnMountTarget {
                element_id: id,
                mounted_at_revision: mount,
                reveal_scrolls: vec![FocusRevealScroll {
                    element_id: NodeId(1),
                    dx: 0.0,
                    dy,
                }],
            },
        )]))),
        initial_mount_focus: initial.then_some(key),
        ..Default::default()
    })
}

#[test]
fn envelopes_share_all_metadata_and_fence_identity_is_not_compilation_identity() {
    for count in [0, 1, 64, 512, 2048] {
        let text_inputs = (0..count)
            .map(|index| {
                (
                    NodeId(index + 10),
                    TextInputState {
                        content: format!("input {index} 雪"),
                        preedit: Some("候補".into()),
                        font_family: "snapshot font".into(),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let payload = RegistryRebuildPayload::new(CompiledRegistryData {
            text_inputs,
            ..Default::default()
        });
        let first = ListenerBarrier::default();
        let second = ListenerBarrier::default();
        let mut a = payload.for_revision(1);
        a.listener_barrier = Some(first.clone());
        let mut b = a.for_revision(2);
        b.listener_barrier = Some(second.clone());
        for next in [payload.clone(), a.clone(), b.clone()] {
            assert!(payload.shares_compilation(&next));
            assert!(std::ptr::eq(&payload.text_inputs, &next.text_inputs));
            assert!(std::ptr::eq(&payload.scrolls, &next.scrolls));
            assert!(std::ptr::eq(&payload.sliders, &next.sliders));
            assert!(std::ptr::eq(&payload.scrollbars, &next.scrollbars));
            if count > 0 {
                assert_eq!(
                    payload.text_inputs[&NodeId(10)].content.as_ptr(),
                    next.text_inputs[&NodeId(10)].content.as_ptr()
                );
            }
        }
        assert!(a.listener_barrier.as_ref().unwrap().matches(&first));
        assert!(!a.listener_barrier.as_ref().unwrap().matches(&second));
        assert!(b.listener_barrier.as_ref().unwrap().matches(&second));
        let different_metadata = RegistryRebuildPayload::new(CompiledRegistryData {
            base_registry: payload.base_registry.clone(),
            focused_id: Some(NodeId(99)),
            ..Default::default()
        });
        assert!(
            !payload.shares_compilation(&different_metadata),
            "same listener storage does not prove equal metadata"
        );
    }
}

#[test]
fn carried_focus_is_a_key_resolved_in_latest_geometry_and_revision_resets_selection() {
    let old = focus_data(1, -6.0, true);
    let pending = old.focus_on_mount;
    let mut next = focus_data(1, -9.0, false);
    let compiled = Arc::clone(&next.compiled);
    next.carry_mount_focus(pending, None);
    assert_eq!(next.focus_on_mount, pending);
    assert!(Arc::ptr_eq(&compiled, &next.compiled));
    assert_eq!(
        next.focus_on_mount_target().unwrap().reveal_scrolls[0].dy,
        -9.0
    );
    assert_eq!(
        old.focus_on_mount_target().unwrap().reveal_scrolls[0].dy,
        -6.0
    );
    assert!(
        next.for_revision(1).focus_on_mount.is_none(),
        "carried selection is not a new compilation's initial choice"
    );
    assert!(old.for_revision(2).focus_on_mount.is_none());
    assert_eq!(old.for_revision(1).focus_on_mount, pending);
    let mut remounted = focus_data(2, -12.0, false);
    remounted.carry_mount_focus(pending, None);
    assert!(remounted.focus_on_mount.is_none());
    let mut absent = RegistryRebuildPayload::default();
    absent.carry_mount_focus(pending, None);
    assert!(absent.focus_on_mount.is_none());
}
