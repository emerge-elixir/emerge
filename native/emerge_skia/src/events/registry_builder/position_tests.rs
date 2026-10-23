use super::*;
use crate::events::PointerPosition;
use crate::tree::attrs::{Attrs, BorderWidth, Length, Padding};
use crate::tree::element::{Frame, NearbySlot};
use crate::tree::layout::{Constraint, layout_and_refresh_default};
use crate::tree::scene::resolve_node_state;

fn element(id: u64, attrs: Attrs) -> Element {
    Element::with_attrs(NodeId(id), ElementKind::El, vec![], attrs)
}

fn attrs() -> Attrs {
    Attrs {
        width: Some(Length::Px(100.0)),
        height: Some(Length::Px(60.0)),
        on_mouse_down: Some(true),
        on_mouse_up: Some(true),
        on_mouse_move: Some(true),
        on_mouse_enter: Some(true),
        on_mouse_leave: Some(true),
        on_click: Some(true),
        on_press: Some(true),
        ..Default::default()
    }
}

fn events(actions: Vec<ListenerAction>) -> Vec<ElixirEvent> {
    actions
        .into_iter()
        .filter_map(|action| match action {
            ListenerAction::ElixirEvent(event) => Some(event),
            _ => None,
        })
        .collect()
}

fn position(event: &ElixirEvent, kind: ElementEventKind, x: f64, y: f64) {
    assert_eq!(event.kind, kind);
    let Some(ElixirEventPayload::Position(actual)) = event.payload else {
        panic!("missing position: {event:?}");
    };
    assert!((actual.x - x).abs() < 0.0001, "{event:?}, expected x={x}");
    assert!((actual.y - y).abs() < 0.0001, "{event:?}, expected y={y}");
}

fn button(action: u8, x: f32, y: f32) -> ListenerInput {
    ListenerInput::Raw(InputEvent::CursorButton {
        button: "left".into(),
        action,
        x,
        y,
        mods: 0,
    })
}

#[test]
fn all_six_events_use_logical_positions_and_press_stays_payload_free() {
    for scale in [1.0, 1.25, 1.6, 2.0] {
        let mut tree = ElementTree::new();
        tree.insert(element(1, attrs()));
        tree.set_root_id(NodeId(1));
        let rebuild = layout_and_refresh_default(&mut tree, Constraint::new(400.0, 300.0), scale)
            .event_rebuild;
        let (x, y) = (35.5 * scale, 20.25 * scale);
        for (input, kind) in [
            (button(ACTION_PRESS, x, y), ElementEventKind::MouseDown),
            (button(ACTION_RELEASE, x, y), ElementEventKind::MouseUp),
            (
                ListenerInput::Raw(InputEvent::CursorPos { x, y }),
                ElementEventKind::MouseMove,
            ),
            (
                ListenerInput::PointerEnter { x, y },
                ElementEventKind::MouseEnter,
            ),
        ] {
            let emitted = events(rebuild.base_registry.view().first_match(
                &input,
                &[],
                &mut NoopListenerComputeCtx,
            ));
            position(
                emitted.iter().find(|event| event.kind == kind).unwrap(),
                kind,
                35.5,
                20.25,
            );
        }
        let source = rebuild
            .base_registry
            .view()
            .iter_precedence()
            .find(|listener| {
                matches!(
                    listener.matcher,
                    ListenerMatcher::CursorButtonLeftPressInside { .. }
                )
            })
            .unwrap();
        let click = runtime_click_press_release_listener(
            &rebuild.base_registry,
            &ClickPressTracker {
                element_id: NodeId(1),
                matcher_kind: source.matcher.kind(),
                emit_click: true,
                emit_press_pointer: true,
                clear_mouse_down: false,
            },
        )
        .unwrap();
        let emitted = events(click.compute_listener_input_actions(&button(ACTION_RELEASE, x, y)));
        position(&emitted[0], ElementEventKind::Click, 35.5, 20.25);
        assert_eq!(emitted[1].kind, ElementEventKind::Press);
        assert_eq!(emitted[1].payload, None);

        let state =
            resolve_node_state(tree.get(&NodeId(1)).unwrap(), SceneContext::default()).unwrap();
        let tracker =
            hover_tracker_for_element(tree.get(&NodeId(1)).unwrap(), Some(&state)).unwrap();
        let input = ListenerInput::PointerLeave {
            x: -2.5 * scale,
            y: 73.25 * scale,
            window_left: false,
        };
        let emitted = events(resolve_listener_actions(
            hover_leave_actions(&input, &[tracker]),
            &mut NoopListenerComputeCtx,
            input.pointer_position(),
        ));
        position(&emitted[0], ElementEventKind::MouseLeave, -2.5, 73.25);
    }
}

#[test]
fn coordinates_follow_inverse_transforms_scroll_render_origin_and_not_insets() {
    let mut node = element(
        1,
        Attrs {
            move_x: Some(17.0),
            move_y: Some(-9.0),
            rotate: Some(30.0),
            scale: Some(1.5),
            border_width: Some(BorderWidth::Uniform(4.0)),
            padding: Some(Padding::Uniform(12.0)),
            ..attrs()
        },
    );
    node.layout.frame = Some(Frame {
        x: 80.0,
        y: 90.0,
        width: 200.0,
        height: 120.0,
        ..Default::default()
    });
    // A layout-rotation AABB and the painted border box need not share an origin.
    node.layout.render_frame = Some(Frame {
        x: 100.0,
        y: 110.0,
        width: 200.0,
        height: 120.0,
        ..Default::default()
    });
    node.layout.dimension_facts.scale = 2.0;
    let state = resolve_node_state(
        &node,
        SceneContext {
            scroll_dx: 23.0,
            scroll_dy: 31.0,
            interaction_transform: Affine2::translation(43.0, 29.0).then(Affine2::scale(2.0, 2.0)),
            ..Default::default()
        },
    )
    .unwrap();
    let surface = state.interaction_transform.map_point(Point {
        x: state.adjusted_render_frame.x + 71.0,
        y: state.adjusted_render_frame.y + 40.5,
    });
    let actual = PointerCoordinates::for_state(&state)
        .position(surface.x, surface.y)
        .unwrap();
    assert!((actual.x - 35.5).abs() < 0.0001, "{actual:?}");
    assert!((actual.y - 20.25).abs() < 0.0001, "{actual:?}");
    let subregion = PointerRegion::for_subregion(
        &state,
        Rect {
            x: 200.0,
            y: 200.0,
            width: 10.0,
            height: 10.0,
        },
        None,
    );
    assert_eq!(
        subregion.coordinates.position(surface.x, surface.y),
        Some(actual)
    );
}

#[test]
fn nested_layout_scales_and_nearby_use_the_prepared_scale() {
    for nearby in [false, true] {
        let mut tree = ElementTree::new();
        let mut parent = element(
            1,
            Attrs {
                layout_scale: Some(1.5),
                ..attrs()
            },
        );
        parent.spec.kind = ElementKind::Column;
        if nearby {
            parent.nearby.push(NearbySlot::InFront, NodeId(2));
        } else {
            parent.children = vec![NodeId(2)];
        }
        tree.insert(parent);
        tree.insert(element(
            2,
            Attrs {
                layout_scale: Some(2.0),
                ..attrs()
            },
        ));
        tree.set_root_id(NodeId(1));
        let rebuild = layout_and_refresh_default(&mut tree, Constraint::new(1000.0, 1000.0), 1.25)
            .event_rebuild;
        let child = tree.get(&NodeId(2)).unwrap();
        assert_eq!(child.layout.dimension_facts.scale, 3.75);
        let region = rebuild
            .base_registry
            .view()
            .iter_precedence()
            .find_map(|listener| {
                if listener.element_id == Some(NodeId(2)) {
                    if let ListenerMatcher::CursorButtonLeftPressInside { region } =
                        &listener.matcher
                    {
                        return Some(region);
                    }
                }
                None
            })
            .unwrap();
        let origin = region.coordinates.origin;
        assert_eq!(
            region
                .coordinates
                .position(origin.x + 133.125, origin.y + 75.9375),
            Some(PointerPosition { x: 35.5, y: 20.25 })
        );
    }
}

#[test]
fn unchanged_fill_frame_with_scale_change_refreshes_pointer_metadata() {
    let mut tree = ElementTree::new();
    tree.insert(element(
        1,
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            ..attrs()
        },
    ));
    tree.set_root_id(NodeId(1));
    for (scale, expected) in [(1.0, 71.0), (2.0, 35.5)] {
        let rebuild = layout_and_refresh_default(&mut tree, Constraint::new(200.0, 120.0), scale)
            .event_rebuild;
        assert_eq!(
            tree.get(&NodeId(1)).unwrap().layout.frame.unwrap().width,
            200.0
        );
        let uncached = build_registry_rebuild(&tree);
        let cached = build_registry_rebuild_cached(&mut tree);
        assert_registry_rebuild_payloads_equivalent(&uncached, &cached);
        assert_registry_rebuild_payloads_equivalent(
            &cached,
            &build_registry_rebuild_cached(&mut tree),
        );
        let emitted = events(rebuild.base_registry.view().first_match(
            &button(ACTION_PRESS, 71.0, 40.0),
            &[],
            &mut NoopListenerComputeCtx,
        ));
        position(
            &emitted[0],
            ElementEventKind::MouseDown,
            expected,
            40.0 / f64::from(scale),
        );
    }
}

#[test]
fn each_hover_tracker_uses_its_own_origin_and_scale() {
    let mut tree = ElementTree::new();
    let mut parent = element(1, attrs());
    parent.spec.kind = ElementKind::Column;
    parent.children = vec![NodeId(2)];
    tree.insert(parent);
    tree.insert(element(
        2,
        Attrs {
            layout_scale: Some(0.5),
            move_x: Some(10.0),
            ..attrs()
        },
    ));
    tree.set_root_id(NodeId(1));
    let rebuild =
        layout_and_refresh_default(&mut tree, Constraint::new(400.0, 300.0), 1.0).event_rebuild;
    let emitted = events(rebuild.base_registry.view().first_match(
        &ListenerInput::PointerEnter { x: 20.0, y: 10.0 },
        &[],
        &mut NoopListenerComputeCtx,
    ));
    position(
        emitted
            .iter()
            .find(|event| event.element_id == NodeId(1))
            .unwrap(),
        ElementEventKind::MouseEnter,
        20.0,
        10.0,
    );
    position(
        emitted
            .iter()
            .find(|event| event.element_id == NodeId(2))
            .unwrap(),
        ElementEventKind::MouseEnter,
        30.0,
        20.0,
    );
}

#[test]
fn singular_and_non_finite_coordinates_are_not_emitted() {
    for coordinates in [
        PointerCoordinates {
            screen_to_local: None,
            origin: Point::default(),
            scale: 1.0,
        },
        PointerCoordinates {
            screen_to_local: Some(Affine2::identity()),
            origin: Point::default(),
            scale: 0.0,
        },
        PointerCoordinates {
            screen_to_local: Some(Affine2::identity()),
            origin: Point::default(),
            scale: f32::NAN,
        },
        PointerCoordinates {
            screen_to_local: Some(Affine2::identity()),
            origin: Point::default(),
            scale: f32::INFINITY,
        },
    ] {
        assert_eq!(coordinates.position(71.0, 40.0), None);
    }
}

#[test]
fn release_up_and_click_keep_their_distinct_target_coordinate_spaces() {
    struct BaseContext<'a>(&'a Registry);
    impl ListenerComputeCtx for BaseContext<'_> {
        fn dispatch_base(&mut self, input: &ListenerInput) -> Vec<ListenerAction> {
            self.0.view().first_match(input, &[], self)
        }
    }
    let mut tree = ElementTree::new();
    let mut parent = element(1, attrs());
    parent.children = vec![NodeId(2)];
    tree.insert(parent);
    tree.insert(element(
        2,
        Attrs {
            width: Some(Length::Px(100.0)),
            height: Some(Length::Px(60.0)),
            layout_scale: Some(0.5),
            move_x: Some(100.0),
            on_mouse_up: Some(true),
            ..Default::default()
        },
    ));
    tree.set_root_id(NodeId(1));
    let base = layout_and_refresh_default(&mut tree, Constraint::new(400.0, 300.0), 1.0)
        .event_rebuild
        .base_registry;
    let click = runtime_click_press_release_listener(
        &base,
        &ClickPressTracker {
            element_id: NodeId(1),
            matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
            emit_click: true,
            emit_press_pointer: false,
            clear_mouse_down: false,
        },
    )
    .unwrap();
    let emitted = events(click.compute_listener_input_with_ctx(
        &button(ACTION_RELEASE, 60.0, 10.0),
        &mut BaseContext(&base),
    ));
    assert_eq!(emitted.len(), 2);
    assert_eq!(emitted[0].element_id, NodeId(2));
    position(&emitted[0], ElementEventKind::MouseUp, 20.0, 20.0);
    assert_eq!(emitted[1].element_id, NodeId(1));
    position(&emitted[1], ElementEventKind::Click, 60.0, 10.0);
}

#[test]
fn animated_layout_scale_uses_the_sampled_not_declared_value() {
    use crate::tree::animation::{
        AnimationCurve, AnimationRepeat, AnimationRuntime, AnimationSpec,
    };
    use crate::tree::layout::layout_and_refresh_default_with_animation;
    use std::time::{Duration, Instant};
    let mut tree = ElementTree::new();
    tree.insert(element(
        1,
        Attrs {
            layout_scale: Some(1.0),
            animate: Some(AnimationSpec {
                keyframes: vec![
                    Attrs {
                        layout_scale: Some(1.0),
                        ..Default::default()
                    },
                    Attrs {
                        layout_scale: Some(2.0),
                        ..Default::default()
                    },
                ],
                duration_ms: 1000.0,
                curve: AnimationCurve::Linear,
                repeat: AnimationRepeat::Once,
            }),
            ..attrs()
        },
    ));
    tree.set_root_id(NodeId(1));
    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);
    let rebuild = layout_and_refresh_default_with_animation(
        &mut tree,
        Constraint::new(400.0, 300.0),
        1.0,
        &mut runtime,
        start + Duration::from_millis(500),
    )
    .unwrap()
    .event_rebuild;
    assert_eq!(
        tree.get(&NodeId(1)).unwrap().layout.dimension_facts.scale,
        1.5
    );
    let emitted = events(rebuild.base_registry.view().first_match(
        &button(ACTION_PRESS, 53.25, 30.375),
        &[],
        &mut NoopListenerComputeCtx,
    ));
    position(&emitted[0], ElementEventKind::MouseDown, 35.5, 20.25);
}
