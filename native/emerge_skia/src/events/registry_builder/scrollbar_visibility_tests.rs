use super::*;
use crate::tree::attrs::Attrs;
use crate::tree::element::Frame;
use crate::tree::scene::resolve_node_state;

const PRESS_SLOTS: &[ElementSlotBuilder] = &[
    slot_scrollbar_thumb_press_y,
    slot_scrollbar_thumb_press_x,
    slot_scrollbar_track_press_y,
    slot_scrollbar_track_press_x,
];

fn scroller(frame: Frame) -> Element {
    let mut element = Element::with_attrs(
        NodeId(1),
        ElementKind::El,
        vec![],
        Attrs {
            scrollbar_x: Some(true),
            scrollbar_y: Some(true),
            on_press: Some(true),
            ..Default::default()
        },
    );
    element.layout.frame = Some(frame);
    element
}

fn frame(width: f32, height: f32) -> Frame {
    Frame {
        x: 0.0,
        y: 0.0,
        width,
        height,
        content_width: 400.0,
        content_height: 400.0,
    }
}

#[test]
fn scrollbar_press_slots_skip_fully_clipped_interaction_with_live_metrics() {
    let element = scroller(Frame {
        y: 200.0,
        ..frame(120.0, 60.0)
    });
    let state = resolve_node_state(
        &element,
        SceneContext {
            visible_clip: Some(ClipShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 120.0,
                    height: 80.0,
                },
                radii: None,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!state.visible);
    assert!(state.scrollbar_x.is_some() && state.scrollbar_y.is_some());
    for slot in PRESS_SLOTS {
        assert!(slot(&element, Some(&state)).is_none());
    }
}

#[test]
fn scrollbar_press_slots_skip_collapsed_cross_axis_with_live_metrics() {
    for (width, height) in [(120.0, 0.0), (0.0, 60.0)] {
        let element = scroller(frame(width, height));
        let state = resolve_node_state(&element, SceneContext::default()).unwrap();
        assert!(!state.visible);
        assert!(state.scrollbar_x.is_some() || state.scrollbar_y.is_some());
        for slot in PRESS_SLOTS {
            assert!(slot(&element, Some(&state)).is_none());
        }
    }
}

#[test]
fn scrollbar_press_slots_accept_missing_scene_state() {
    let element = scroller(frame(120.0, 60.0));
    for slot in PRESS_SLOTS {
        assert!(slot(&element, None).is_none());
    }
}

#[test]
fn scrollbar_press_slots_preserve_visible_hits_and_reject_singular_transforms() {
    for transform in [Affine2::identity(), Affine2::scale(0.0, 0.0)] {
        let element = scroller(frame(120.0, 60.0));
        let state = resolve_node_state(
            &element,
            SceneContext {
                interaction_transform: transform,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(state.visible);
        for slot in PRESS_SLOTS {
            let listener = slot(&element, Some(&state)).unwrap();
            let ListenerMatcher::CursorButtonLeftPressInside { region } = &listener.matcher else {
                panic!("expected a press region");
            };
            let bounds = region.local_shape.rect;
            let (x, y) = (
                bounds.x + bounds.width / 2.0,
                bounds.y + bounds.height / 2.0,
            );
            let input = ListenerInput::Raw(InputEvent::CursorButton {
                button: "left".into(),
                action: ACTION_PRESS,
                mods: 0,
                x,
                y,
            });
            assert_eq!(
                listener.matcher.matches_input(&input),
                state.interaction_inverse.is_some()
            );
            if state.interaction_inverse.is_some() {
                let actions = listener.compute_listener_input_actions(&input);
                assert!(actions.iter().any(|action| matches!(
                    action,
                    ListenerAction::RuntimeChange(RuntimeChange::StartScrollbarDrag { .. })
                )));
            }
        }
    }
}

#[test]
fn invisible_scrollbars_do_not_remove_focused_keyboard_actions() {
    let mut element = scroller(frame(120.0, 0.0));
    element.runtime.focused_active = true;
    let listeners = listeners_for_element(&element);
    assert!(listeners.iter().all(|listener| !matches!(
        listener.compute,
        ListenerCompute::ScrollbarPressToRuntime { .. }
    )));
    assert!(listeners.iter().any(|listener| {
        matches!(
            listener.matcher,
            ListenerMatcher::KeyEnterPressNoCtrlAltMeta
        ) && listener
            .compute_listener_input_actions(&ListenerInput::Raw(InputEvent::Key {
                key: CanonicalKey::Enter,
                action: ACTION_PRESS,
                mods: 0,
            }))
            .iter()
            .any(|action| {
                matches!(
                    action,
                    ListenerAction::ElixirEvent(ElixirEvent {
                        kind: ElementEventKind::Press,
                        ..
                    })
                )
            })
    }));
}

fn nested_scroll_tree() -> ElementTree {
    use crate::tree::attrs::Length;
    let nodes = [
        (1, ElementKind::Column, 120.0, 80.0, true, false),
        (2, ElementKind::El, 120.0, 160.0, false, false),
        (3, ElementKind::Column, 120.0, 60.0, true, true),
        (4, ElementKind::TextInput, 240.0, 30.0, false, false),
        (5, ElementKind::El, 240.0, 180.0, false, false),
    ];
    let mut tree = ElementTree::new();
    for (id, kind, width, height, vertical, horizontal) in nodes {
        let mut element = Element::with_attrs(
            NodeId(id),
            kind,
            vec![],
            Attrs {
                width: Some(Length::Px(width)),
                height: Some(Length::Px(height)),
                scrollbar_y: vertical.then_some(true),
                scrollbar_x: horizontal.then_some(true),
                content: (id == 4).then(|| "retained keyboard input".into()),
                on_change: (id == 4).then_some(true),
                ..Default::default()
            },
        );
        element.runtime.focused_active = id == 4;
        element.runtime.text_input_focused = id == 4;
        tree.insert(element);
    }
    tree.set_children(&NodeId(1), vec![NodeId(2), NodeId(3)])
        .unwrap();
    tree.set_children(&NodeId(3), vec![NodeId(4), NodeId(5)])
        .unwrap();
    tree.set_root_id(NodeId(1));
    tree
}

fn scrollbar_presses(registry: &Registry, id: NodeId) -> usize {
    registry
        .view()
        .iter_precedence()
        .filter(|listener| {
            listener.element_id == Some(id)
                && matches!(
                    listener.compute,
                    ListenerCompute::ScrollbarPressToRuntime { .. }
                )
        })
        .count()
}

#[test]
fn clipped_nested_scrollbars_survive_full_cached_and_render_rebuilds() {
    use crate::tree::layout::{Constraint, layout_and_refresh_default, refresh};
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for scale in [1.0, 1.6, 2.0] {
        let mut tree = nested_scroll_tree();
        let initial = layout_and_refresh_default(&mut tree, Constraint::new(500.0, 400.0), scale)
            .event_rebuild;
        assert!(initial.text_inputs.contains_key(&NodeId(4)));
        assert_eq!(initial.focused_id, Some(NodeId(4)));
        for axis in [ScrollbarAxis::X, ScrollbarAxis::Y] {
            assert!(initial.scrollbars.contains_key(&(NodeId(3), axis)));
        }
        assert_eq!(scrollbar_presses(&initial.base_registry, NodeId(3)), 0);
        for _ in 0..2 {
            let full = build_registry_rebuild(&tree);
            let cached = build_registry_rebuild_cached(&mut tree);
            assert_registry_rebuild_payloads_equivalent(&initial, &full);
            assert_registry_rebuild_payloads_equivalent(&full, &cached);
        }
        // Scroll the retained focused subtree into view, then out again. Do not
        // delete its keyboard/drag metadata just to avoid building pointer slots.
        for visible in [true, false, true] {
            let root = tree.get_mut(&NodeId(1)).unwrap();
            root.layout.scroll_y = if visible {
                root.layout.scroll_y_max
            } else {
                0.0
            };
            let rebuilt = refresh(&mut tree).event_rebuild;
            assert_eq!(
                scrollbar_presses(&rebuilt.base_registry, NodeId(3)),
                if visible { 4 } else { 0 }
            );
            assert!(rebuilt.text_inputs.contains_key(&NodeId(4)));
            assert_eq!(rebuilt.focused_id, Some(NodeId(4)));
            assert!(
                rebuilt
                    .scrollbars
                    .contains_key(&(NodeId(3), ScrollbarAxis::Y))
            );
        }
    }
}
