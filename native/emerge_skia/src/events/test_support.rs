use std::time::{Duration, Instant};

use super::registry_builder::{self, Listener, ListenerAction, Registry};
use super::{ElementEventKind, RegistryRebuildPayload};
use crate::tree::animation::{AnimationCurve, AnimationRepeat, AnimationRuntime, AnimationSpec};
use crate::tree::attrs::{AlignX, AlignY, Attrs, Length, MouseOverAttrs};
use crate::tree::element::{Element, ElementKind, ElementTree, NearbySlot, NodeId};
use crate::tree::layout::{
    Constraint, layout_and_refresh_default_with_animation, layout_tree_default_with_animation,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpectedHitWinner {
    None,
    Underlying,
    Target,
}

#[derive(Clone, Debug)]
pub struct HitProbe {
    pub label: &'static str,
    pub point: (f32, f32),
    pub expected_by_sample: Vec<(u64, ExpectedHitWinner)>,
}

impl HitProbe {
    pub fn expected_at(&self, sample_ms: u64) -> ExpectedHitWinner {
        self.expected_by_sample
            .iter()
            .find(|(sample, _)| *sample == sample_ms)
            .map(|(_, winner)| *winner)
            .unwrap_or(ExpectedHitWinner::None)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum SampledRegistrySource {
    LayoutOnly,
    RenderRebuild,
}

#[derive(Clone, Debug)]
pub struct AnimatedNearbyHitCase {
    pub host_id: NodeId,
    pub underlying_id: NodeId,
    pub target_id: NodeId,
    pub constraint: Constraint,
    pub sample_times_ms: Vec<u64>,
    pub probes: Vec<HitProbe>,
}

impl AnimatedNearbyHitCase {
    pub fn width_move_in_front() -> Self {
        Self {
            host_id: NodeId::from_term_bytes(vec![210]),
            underlying_id: NodeId::from_term_bytes(vec![211]),
            target_id: NodeId::from_term_bytes(vec![212]),
            constraint: Constraint::new(128.0, 82.0),
            sample_times_ms: vec![0, 500, 1000],
            probes: vec![
                HitProbe {
                    label: "stable_inside",
                    point: (48.0, 41.0),
                    expected_by_sample: vec![
                        (0, ExpectedHitWinner::Target),
                        (500, ExpectedHitWinner::Target),
                        (1000, ExpectedHitWinner::Target),
                    ],
                },
                HitProbe {
                    label: "newly_occupied_inside_host",
                    point: (110.0, 41.0),
                    expected_by_sample: vec![
                        (0, ExpectedHitWinner::Underlying),
                        (500, ExpectedHitWinner::Target),
                        (1000, ExpectedHitWinner::Target),
                    ],
                },
                HitProbe {
                    label: "newly_occupied_outside_host",
                    point: (130.0, 41.0),
                    expected_by_sample: vec![
                        (0, ExpectedHitWinner::None),
                        (500, ExpectedHitWinner::Target),
                        (1000, ExpectedHitWinner::Target),
                    ],
                },
                HitProbe {
                    label: "stable_outside",
                    point: (175.0, 41.0),
                    expected_by_sample: vec![
                        (0, ExpectedHitWinner::None),
                        (500, ExpectedHitWinner::None),
                        (1000, ExpectedHitWinner::None),
                    ],
                },
            ],
        }
    }

    pub fn registry_at(&self, source: SampledRegistrySource, sample_ms: u64) -> Registry {
        match source {
            SampledRegistrySource::LayoutOnly => {
                let tree = self.tree_at(sample_ms, false);
                let elements: Vec<_> = tree.iter_nodes().cloned().collect();
                registry_builder::registry_for_elements(&elements)
            }
            SampledRegistrySource::RenderRebuild => self.rebuild_at(sample_ms, false).base_registry,
        }
    }

    pub fn rebuild_at(&self, sample_ms: u64, hover_active: bool) -> RegistryRebuildPayload {
        sampled_rebuild_for_case(self, sample_ms, hover_active)
    }

    pub fn tree_at(&self, sample_ms: u64, hover_active: bool) -> ElementTree {
        sampled_tree_for_case(self, sample_ms, hover_active)
    }

    pub fn source_tree(&self, hover_active: bool) -> ElementTree {
        source_tree_for_case(self, hover_active)
    }

    pub fn probe(&self, label: &str) -> &HitProbe {
        self.probes
            .iter()
            .find(|probe| probe.label == label)
            .expect("probe should exist")
    }

    pub fn first_target_sample_ms(&self, label: &str) -> Option<u64> {
        self.probe(label)
            .expected_by_sample
            .iter()
            .find_map(|(sample_ms, winner)| {
                (*winner == ExpectedHitWinner::Target).then_some(*sample_ms)
            })
    }
}

pub fn assert_registry_probe_matrix(case: &AnimatedNearbyHitCase, source: SampledRegistrySource) {
    for sample_ms in &case.sample_times_ms {
        let registry = case.registry_at(source, *sample_ms);
        for probe in &case.probes {
            let actions = first_matching_actions(
                &registry,
                &crate::input::InputEvent::CursorPos {
                    x: probe.point.0,
                    y: probe.point.1,
                },
            );
            let actual = winner_from_actions(&actions, &case.target_id, &case.underlying_id);
            assert_eq!(
                actual,
                probe.expected_at(*sample_ms),
                "probe '{}' expected {:?} at {}ms, got {:?}",
                probe.label,
                probe.expected_at(*sample_ms),
                sample_ms,
                actual
            );
        }
    }
}

fn first_matching_actions(
    registry: &Registry,
    input: &crate::input::InputEvent,
) -> Vec<ListenerAction> {
    registry
        .view()
        .find_precedence(|listener: &Listener| listener.matcher.matches(input))
        .map(|listener| listener.compute_actions(input))
        .unwrap_or_default()
}

pub(crate) fn winner_from_actions(
    actions: &[ListenerAction],
    target_id: &NodeId,
    underlying_id: &NodeId,
) -> ExpectedHitWinner {
    for action in actions {
        match action {
            ListenerAction::ElixirEvent(event) if event.kind == ElementEventKind::MouseMove => {
                if &event.element_id == target_id {
                    return ExpectedHitWinner::Target;
                }
                if &event.element_id == underlying_id {
                    return ExpectedHitWinner::Underlying;
                }
            }
            _ => {}
        }
    }

    ExpectedHitWinner::None
}

fn sampled_tree_for_case(
    case: &AnimatedNearbyHitCase,
    sample_ms: u64,
    hover_active: bool,
) -> ElementTree {
    let mut tree = source_tree_for_case(case, hover_active);

    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);
    let _ = layout_tree_default_with_animation(
        &mut tree,
        case.constraint,
        1.0,
        &runtime,
        start + Duration::from_millis(sample_ms),
    );

    tree
}

fn source_tree_for_case(case: &AnimatedNearbyHitCase, hover_active: bool) -> ElementTree {
    let mut tree = ElementTree::new();

    let mut host_attrs = Attrs::default();
    host_attrs.width = Some(Length::Px(128.0));
    host_attrs.height = Some(Length::Px(82.0));
    let mut host = Element::with_attrs(case.host_id, ElementKind::El, Vec::new(), host_attrs);
    host.nearby.set(NearbySlot::InFront, Some(case.target_id));
    host.children = vec![case.underlying_id];

    let mut underlying_attrs = Attrs::default();
    underlying_attrs.width = Some(Length::Px(128.0));
    underlying_attrs.height = Some(Length::Px(82.0));
    underlying_attrs.on_mouse_move = Some(true);
    let underlying = Element::with_attrs(
        case.underlying_id,
        ElementKind::El,
        Vec::new(),
        underlying_attrs,
    );

    let mut from = Attrs::default();
    from.width = Some(Length::Px(96.0));
    from.move_x = Some(-16.0);

    let mut to = Attrs::default();
    to.width = Some(Length::Px(156.0));
    to.move_x = Some(26.0);

    let mut target_attrs = Attrs::default();
    target_attrs.width = Some(Length::Px(128.0));
    target_attrs.height = Some(Length::Px(82.0));
    target_attrs.align_x = Some(AlignX::Center);
    target_attrs.align_y = Some(AlignY::Center);
    target_attrs.on_mouse_move = Some(true);
    target_attrs.mouse_over = Some(MouseOverAttrs::default());
    target_attrs.mouse_over_active = Some(hover_active);
    target_attrs.animate = Some(AnimationSpec {
        keyframes: vec![from, to],
        duration_ms: 1000.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Once,
    });
    let target = Element::with_attrs(case.target_id, ElementKind::El, Vec::new(), target_attrs);

    tree.insert(host);
    tree.insert(underlying);
    tree.insert(target);
    tree.set_root_id(case.host_id);

    tree
}

fn sampled_rebuild_for_case(
    case: &AnimatedNearbyHitCase,
    sample_ms: u64,
    hover_active: bool,
) -> RegistryRebuildPayload {
    let mut tree = sampled_tree_for_case(case, sample_ms, hover_active);
    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);
    layout_and_refresh_default_with_animation(
        &mut tree,
        case.constraint,
        1.0,
        &runtime,
        start + Duration::from_millis(sample_ms),
    )
    .event_rebuild
}
