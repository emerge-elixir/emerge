//! Proof of a foreign resolved-dimension clock, not a numeric lowering.
use super::*;

pub(super) fn footprint_context_continues(
    tree: &ElementTree,
    old: &Track,
    next: &Projection,
    positions: &[(NodeId, u64, RunRef<'_>, timing::SegmentPosition)],
    now: Instant,
    tracks: &HashMap<(NodeId, Axis), Arc<Track>>,
    boundaries: Option<&HashMap<(NodeId, Axis), Arc<Track>>>,
) -> Option<Arc<ClockContinuation>> {
    if old.projection.nodes.len() != next.nodes.len() {
        return None;
    }
    // Compose field witnesses, not whole-projection alternatives. A node may
    // have independent owners on its axes, and numeric and resolved drivers may
    // coexist in one query. All unchanged attributes still have to match exactly.
    let nodes = old
        .projection
        .nodes
        .iter()
        .filter_map(|(id, before)| {
            let result = next.nodes.get(id).and_then(|after| {
                node_clock_witness(
                    tree,
                    old,
                    (*id, before, after),
                    (positions, now),
                    tracks,
                    boundaries,
                )
            });
            result.ok_or(()).transpose()
        })
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let projection = if nodes
        .iter()
        .any(|(id, node, _, _)| *node != old.projection.nodes[id])
    {
        let mut value = (*old.projection).clone();
        value
            .nodes
            .extend(nodes.iter().map(|(id, node, _, _)| (*id, node.clone())));
        Arc::new(value)
    } else {
        Arc::clone(&old.projection)
    };
    let next_pixels = nodes
        .iter()
        .flat_map(|(_, _, _, witnesses)| {
            witnesses
                .iter()
                .filter_map(|(key, _, fp, _)| fp.map(|fp| (*key, fp)))
        })
        .collect::<Vec<_>>();
    let next = (!next_pixels.is_empty()).then(|| {
        let mut value = next.clone();
        value
            .nodes
            .extend(nodes.iter().map(|(id, _, node, _)| (*id, node.clone())));
        (Arc::new(value), next_pixels)
    });
    Some(Arc::new(ClockContinuation {
        independent: None,
        own_axis: None,
        projection,
        next,
        pixels: nodes
            .iter()
            .flat_map(|(_, _, _, witnesses)| {
                witnesses
                    .iter()
                    .filter_map(|(key, fp, _, _)| fp.map(|fp| (*key, fp)))
            })
            .collect(),
        foreign: nodes
            .into_iter()
            .flat_map(|(_, _, _, witnesses)| {
                witnesses.into_iter().flat_map(|(key, _, _, clocks)| {
                    clocks.into_iter().map(move |clock| (key, clock))
                })
            })
            .collect(),
    }))
}

type FieldWitness = (
    (NodeId, Axis),
    Option<AxisFootprint>,
    Option<AxisFootprint>,
    Vec<Arc<ClockInput>>,
);
type NodeClockWitness = (NodeId, NodeProjection, NodeProjection, Vec<FieldWitness>);
fn node_clock_witness(
    tree: &ElementTree,
    old: &Track,
    (node, before, after): (NodeId, &NodeProjection, &NodeProjection),
    (positions, now): (
        &[(NodeId, u64, RunRef<'_>, timing::SegmentPosition)],
        Instant,
    ),
    tracks: &HashMap<(NodeId, Axis), Arc<Track>>,
    boundaries: Option<&HashMap<(NodeId, Axis), Arc<Track>>>,
) -> Option<Option<NodeClockWitness>> {
    let id = &node;
    if before == after {
        return Some(None);
    }
    if before.model != after.model {
        return None;
    }
    let mut a = before.clone();
    let mut b = before.clone();
    let mut native_before = before.clone();
    let mut native_after = after.clone();
    let witnesses = [Axis::Width, Axis::Height]
        .into_iter()
        .map(|axis| {
            let fp = |node: &NodeProjection| {
                if axis == Axis::Width {
                    node.width
                } else {
                    node.height
                }
            };
            if fp(before) == fp(after) && length(&before.attrs, axis) == length(&after.attrs, axis)
            {
                return Some(None);
            }
            positions
                .iter()
                .filter(|(node, _, _, _)| node == id)
                .find_map(|(_, mount, run, p)| {
                    let clock = AnimationRuntimeEntry {
                        spec_hash: run.revision,
                        started_at: run.started,
                    };
                    if run.started > old.evaluated_at {
                        return None;
                    }
                    let previous =
                        timing::position(run.spec, Some(&clock), Some(old.evaluated_at))?;
                    let changed = previous.index != p.index || previous.start_ms != p.start_ms;
                    if changed && boundaries.is_none() {
                        return None;
                    }
                    let selected =
                        |index| layout_attrs(&run.fields.select(&run.spec.keyframes[index]));
                    let endpoints =
                        [previous.index, previous.index + 1, p.index, p.index + 1].map(selected);
                    if endpoints.iter().any(|attrs| {
                        *attrs
                            != Attrs {
                                width: attrs.width.clone(),
                                height: attrs.height.clone(),
                                ..Default::default()
                            }
                    }) {
                        return None;
                    }
                    let lengths = endpoints
                        .iter()
                        .map(|attrs| length(attrs, axis))
                        .collect::<Option<Vec<_>>>()?;
                    let sample = |time| {
                        layout_attrs(&run.fields.select(
                            &sample_animation_spec(run.spec, Some(&clock), Some(time)).attrs,
                        ))
                    };
                    let from_attrs = sample(old.evaluated_at);
                    let to_attrs = sample(now);
                    let before_fp = fp(before);
                    let after_fp = fp(after);
                    let numeric = lengths.iter().all(|length| matches!(length, Length::Px(_)))
                        && !(changed && tracks.contains_key(&(*id, axis)));
                    let (prior, next_witness, foreign, old_pixel, new_pixel) = if numeric {
                        if run.owner != groups::Owner::Regular
                            || changed
                                && (tracks.contains_key(&(*id, axis))
                                    || before_fp.is_some()
                                    || after_fp.is_some()
                                    || run.source.is_some_and(|source| {
                                        source.sampled.into_iter().any(|sample| sample)
                                    }))
                        {
                            return None;
                        }
                        let (Some(Length::Px(from)), Some(Length::Px(to))) =
                            (length(&from_attrs, axis), length(&to_attrs, axis))
                        else {
                            return None;
                        };
                        if let Some(prototype) = before_fp.or(after_fp) {
                            let a = prototype.pixel_candidate(tree, *id, *from as f32)?;
                            let b = prototype.pixel_candidate(tree, *id, *to as f32)?;
                            if before_fp.is_some_and(|fp| !fp.release_matches(a))
                                || after_fp.is_some_and(|fp| !fp.release_matches(b))
                            {
                                return None;
                            }
                            (
                                before_fp.map(|_| a),
                                after_fp.map(|_| b),
                                Vec::new(),
                                before_fp.map(|_| *from),
                                after_fp.map(|_| *to),
                            )
                        } else {
                            (None, None, Vec::new(), None, None)
                        }
                    } else {
                        let current = &tracks.get(&(*id, axis))?.input;
                        let track = before_fp
                            .and_then(|_| old.forecast.as_ref()?.get(&(*id, axis)))
                            .unwrap_or(current);
                        if !track.same_run(*run, *mount)
                            || track.key.segment != previous.index
                            || track.key.cycle != previous.start_ms.to_bits()
                        {
                            return None;
                        }
                        let next_track = if changed {
                            &boundaries?.get(&(*id, axis))?.input
                        } else {
                            current
                        };
                        if !next_track.same_run(*run, *mount)
                            || next_track.key.segment != p.index
                            || next_track.key.cycle != p.start_ms.to_bits()
                            || next_track.model != old.model
                            || !Arc::ptr_eq(&next_track.context, &old.context)
                        {
                            return None;
                        }
                        let from = track.sample(*run, previous, old.evaluated_at).ok()?;
                        let to = next_track.sample(*run, *p, now).ok()?;
                        if before_fp.is_none()
                            && crate::tree::layout::dimensions::capture_axis(tree, id, axis)
                                .is_none_or(|published| !published.release_matches(from))
                        {
                            return None;
                        }
                        if before_fp.is_some_and(|fp| !fp.release_matches(from))
                            || after_fp.is_some_and(|fp| !fp.release_matches(to))
                            || after_fp.is_none() && p.active
                        {
                            return None;
                        }
                        let clocks = [track, current, next_track];
                        let foreign = clocks
                            .iter()
                            .enumerate()
                            .filter(|(i, value)| {
                                !clocks[..*i].iter().any(|prior| Arc::ptr_eq(prior, value))
                            })
                            .map(|(_, value)| Arc::clone(value))
                            .collect();
                        (
                            before_fp.map(|_| from),
                            after_fp.is_none().then_some(to),
                            foreign,
                            None,
                            None,
                        )
                    };
                    let apply =
                        |node: &mut NodeProjection, attrs: &Attrs, fp: Option<AxisFootprint>| {
                            match axis {
                                Axis::Width => {
                                    node.attrs.width = if fp.is_some() {
                                        None
                                    } else {
                                        attrs.width.clone()
                                    };
                                    node.width = fp;
                                }
                                Axis::Height => {
                                    node.attrs.height = if fp.is_some() {
                                        None
                                    } else {
                                        attrs.height.clone()
                                    };
                                    node.height = fp;
                                }
                            }
                        };
                    apply(&mut a, &from_attrs, before_fp);
                    apply(&mut b, &to_attrs, after_fp);
                    if old_pixel.is_some() {
                        apply(&mut native_before, &from_attrs, None);
                    }
                    if new_pixel.is_some() {
                        apply(&mut native_after, &to_attrs, None);
                    }
                    Some(Some(((*id, axis), prior, next_witness, foreign)))
                })
        })
        .collect::<Option<Vec<_>>>()?;
    (a == *before && b == *after).then(|| {
        Some((
            *id,
            native_before,
            native_after,
            witnesses.into_iter().flatten().collect::<Vec<_>>(),
        ))
    })
}

/// Retained ancestry only; no future animation dependency graph.
pub(super) fn is_ancestor(tree: &ElementTree, ancestor: NodeId, node: NodeId) -> bool {
    std::iter::successors(tree.ix_of(&node), |ix| {
        #[cfg(any(test, feature = "bench-diagnostics"))]
        {
            let visits = &tree.animation_query_retirement.continuation_ancestry_visits;
            visits.set(visits.get() + 1);
        }
        match tree.parent_link_of(*ix)? {
            crate::tree::element::ParentLink::Child { parent } => Some(parent),
            crate::tree::element::ParentLink::Nearby { host, .. } => Some(host),
        }
    })
    .take(tree.len())
    .skip(1)
    .any(|ix| tree.get_ix(ix).is_some_and(|node| node.id == ancestor))
}

impl ClockContinuation {
    /// Candidate feedback drivers for finite motion/holds/release, never general
    /// permission for upward clocks. Native witnesses must pass and the loops
    /// must apply the frozen presentation. Release additionally checks the joint target.
    pub(super) fn finite_feedback(
        &self,
        tree: &ElementTree,
        owner: (NodeId, Axis),
        positions: &[(NodeId, u64, RunRef<'_>, timing::SegmentPosition)],
    ) -> Option<HashSet<(NodeId, Axis)>> {
        if self.independent.is_some() {
            return None;
        }
        let drivers = self
            .foreign
            .iter()
            .map(|(key, track)| {
                if is_ancestor(tree, key.0, owner.0) {
                    return Some(None);
                }
                (*key != owner
                    && positions.iter().any(|(node, mount, run, _)| {
                        *node == key.0
                            && track.same_run(*run, *mount)
                            && matches!(run.spec.repeat, AnimationRepeat::Loop)
                    }))
                .then_some(Some(*key))
            })
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<HashSet<_>>();
        (!drivers.is_empty()).then_some(drivers)
    }

    pub(super) fn permits_owner(&self, tree: &ElementTree, owner: (NodeId, Axis)) -> bool {
        // Native evidence can be shared, but mixed-clock eligibility is relative
        // to each consumer. Never cache the first owner's ancestry decision.
        // Raw pixels (or fully witnessed pixel equivalents) do not depend on a
        // resolved foreign destination and can also drive ancestor intrinsic size.
        if let Some((replaced, _)) = &self.independent
            && replaced.iter().any(|id| {
                (*id == owner.0 && self.own_axis != Some(owner))
                    || is_ancestor(tree, *id, owner.0)
                    || is_ancestor(tree, owner.0, *id)
            })
        {
            return false;
        }
        self.foreign
            .iter()
            .all(|(driver, _)| is_ancestor(tree, driver.0, owner.0))
    }
}

/// Cache only verified-common input classifications. Each caller recomputes its
/// replacement set and checks causal ancestry; one owner's eligibility cannot leak.
#[derive(Default)]
pub(super) struct IndependenceCache {
    proofs: HashMap<IndependenceKey, Option<Arc<ClockContinuation>>>,
}
type IndependenceKey = (
    usize,
    usize,
    Instant,
    bool,
    Vec<NodeId>,
    Option<(NodeId, Axis)>,
);

impl IndependenceCache {
    /// Candidate only: native original and hybrid targets must both be verified.
    /// Siblings are not assumed independent just because they lack an ancestry edge.
    pub(super) fn continuation(
        &mut self,
        tree: &ElementTree,
        (owner, old, releasing): ((NodeId, Axis), &Track, &HashSet<(NodeId, Axis)>),
        next: &Arc<Projection>,
        (positions, now): (
            &[(NodeId, u64, RunRef<'_>, timing::SegmentPosition)],
            Instant,
        ),
        tracks: &HashMap<(NodeId, Axis), Arc<Track>>,
        boundaries: Option<&HashMap<(NodeId, Axis), Arc<Track>>>,
    ) -> Option<Arc<ClockContinuation>> {
        if old.projection.nodes.len() != next.nodes.len() {
            return None;
        }
        let releases = |node| {
            releasing.contains(&(node, Axis::Width)) || releasing.contains(&(node, Axis::Height))
        };
        let consumer_releases = releases(owner.0);
        let mut replaced = old
            .projection
            .nodes
            .iter()
            .filter_map(|(id, before)| {
                let after = next.nodes.get(id)?;
                if before == after || before.model != after.model {
                    return None;
                }
                if *id == owner.0 {
                    let mut other = after.clone();
                    match owner.1 {
                        Axis::Width => {
                            other.height = before.height;
                            other.attrs.height = before.attrs.height.clone();
                        }
                        Axis::Height => {
                            other.width = before.width;
                            other.attrs.width = before.attrs.width.clone();
                        }
                    }
                    return (other == *before).then_some(*id);
                }
                // Co-releasing owners belong to the joint target contract, not
                // external context that a releasing consumer may replace.
                let co_releasing = consumer_releases && releases(*id);
                (!co_releasing
                    && !is_ancestor(tree, *id, owner.0)
                    && !is_ancestor(tree, owner.0, *id))
                .then_some(*id)
            })
            .collect::<Vec<_>>();
        if replaced.is_empty() {
            return None;
        }
        replaced.sort_unstable_by_key(|id| id.0);
        let own_axis = replaced.contains(&owner.0).then_some(owner);
        let key = (
            Arc::as_ptr(&old.projection) as usize,
            Arc::as_ptr(next) as usize,
            old.evaluated_at,
            boundaries.is_some(),
            replaced.clone(),
            own_axis,
        );
        self.proofs
            .entry(key)
            .or_insert_with(|| {
                let mut projection = (*old.projection).clone();
                projection
                    .nodes
                    .extend(replaced.iter().map(|id| (*id, next.nodes[id].clone())));
                let candidate = Track {
                    input: Arc::new(ClockInput {
                        projection: Arc::new(projection),
                        ..(**old).clone()
                    }),
                    forecast: old.forecast.clone(),
                };
                let proof = numeric_context_continues(
                    tree,
                    &candidate,
                    next,
                    positions,
                    now,
                    boundaries.is_some(),
                    tracks,
                )
                .or_else(|| {
                    footprint_context_continues(
                        tree, &candidate, next, positions, now, tracks, boundaries,
                    )
                })?;
                Some(Arc::new(ClockContinuation {
                    independent: Some((replaced, Arc::clone(&old.projection))),
                    own_axis,
                    ..(*proof).clone()
                }))
            })
            .as_ref()
            .filter(|proof| proof.permits_owner(tree, owner))
            .cloned()
    }
}
