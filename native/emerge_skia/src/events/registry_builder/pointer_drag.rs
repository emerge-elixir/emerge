//! Explicit left-button capture for custom two-dimensional controls.
use super::*;
use crate::events::DragPhase;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PointerCaptureTracker {
    pub element_id: NodeId,
    pub matcher_kind: ListenerMatcherKind,
    clear_mouse_down: bool,
}

pub(super) fn start_actions(
    element: &Element,
    coordinates: PointerCoordinates,
    matcher_kind: ListenerMatcherKind,
) -> Vec<ListenerAction> {
    if !element.layout.effective.on_drag.unwrap_or(false) {
        return Vec::new();
    }
    vec![
        ListenerAction::RuntimeChange(RuntimeChange::StartPointerCapture {
            tracker: PointerCaptureTracker {
                element_id: element.id,
                matcher_kind,
                clear_mouse_down: element.layout.effective.mouse_down.is_some(),
            },
        }),
        sample(element.id, DragPhase::Start, coordinates),
    ]
}

fn sample(element_id: NodeId, phase: DragPhase, coordinates: PointerCoordinates) -> ListenerAction {
    ListenerAction::Semantic(SemanticAction::PointerDrag {
        element_id,
        phase,
        coordinates,
    })
}

fn source_coordinates(
    base: &Registry,
    tracker: &PointerCaptureTracker,
) -> Option<PointerCoordinates> {
    let source = base.view().find_precedence(|source| {
        source.element_id == Some(tracker.element_id)
            && source.matcher.kind() == tracker.matcher_kind
            && match &source.compute {
                ListenerCompute::Static { actions }
                | ListenerCompute::StaticWithLeftPressRuntimeAugment { actions, .. } => {
                    actions.iter().any(|action| {
                        matches!(
                            action,
                            ListenerAction::RuntimeChange(
                                RuntimeChange::StartPointerCapture { .. }
                            )
                        )
                    })
                }
                _ => false,
            }
    })?;
    match &source.matcher {
        ListenerMatcher::CursorButtonLeftPressInside { region } => Some(region.coordinates),
        _ => None,
    }
}

pub(crate) fn has_source(base: &Registry, tracker: &PointerCaptureTracker) -> bool {
    source_coordinates(base, tracker).is_some()
}

pub(super) fn overlay_listeners(
    base: &Registry,
    tracker: Option<&PointerCaptureTracker>,
) -> Vec<Listener> {
    let Some(tracker) = tracker else {
        return Vec::new();
    };
    // Resolve from the current registry, never the press-time rectangle: marker
    // patches, scrolling and inherited transforms can change during capture.
    let Some(coordinates) = source_coordinates(base, tracker) else {
        return Vec::new();
    };
    [
        (ListenerMatcher::CursorPosAnywhere, DragPhase::Move),
        (
            ListenerMatcher::CursorButtonLeftReleaseAnywhere,
            DragPhase::End,
        ),
        (ListenerMatcher::WindowBlurred, DragPhase::Cancel),
        (ListenerMatcher::WindowCursorLeft, DragPhase::Cancel),
    ]
    .into_iter()
    .map(|(matcher, phase)| {
        let ending = phase != DragPhase::Move;
        let actions = std::iter::once(sample(tracker.element_id, phase, coordinates))
            .chain(ending.then_some(ListenerAction::RuntimeChange(
                RuntimeChange::ClearPointerCapture,
            )))
            .chain(
                (ending && tracker.clear_mouse_down).then_some(ListenerAction::TreeMsg(
                    TreeMsg::SetMouseDownActive {
                        element_id: tracker.element_id,
                        active: false,
                    },
                )),
            )
            .collect();
        Listener {
            element_id: Some(tracker.element_id),
            matcher,
            compute: ListenerCompute::Static { actions },
        }
    })
    .collect()
}
