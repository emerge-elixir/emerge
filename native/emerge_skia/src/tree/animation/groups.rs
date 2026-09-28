//! Parent-scoped finite layout ownership. This ledger contains no layout tree,
//! sampled geometry, animation specs, or independent clocks.
use super::*;
use crate::tree::element::{NearbySlot, ParentLink};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Mount {
    id: NodeId,
    revision: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum GroupKey {
    Flow(Mount),
    Root(Mount),
    Nearby(Mount, NearbySlot),
    Ghost(Mount),
}
impl GroupKey {
    pub(crate) fn for_node(tree: &ElementTree, id: NodeId) -> Option<Self> {
        let node = tree.get(&id)?;
        let own = Mount {
            id,
            revision: node.lifecycle.mounted_at_revision,
        };
        if node.is_ghost_root() {
            return Some(Self::Ghost(own));
        }
        if tree.root_id() == Some(id) {
            return Some(Self::Root(own));
        }
        let mount = |ix| {
            tree.get_ix(ix).map(|n| Mount {
                id: n.id,
                revision: n.lifecycle.mounted_at_revision,
            })
        };
        match tree.parent_link_of(tree.ix_of(&id)?) {
            Some(ParentLink::Child { parent }) => mount(parent).map(Self::Flow),
            Some(ParentLink::Nearby { host, slot }) => {
                mount(host).map(|host| Self::Nearby(host, slot))
            }
            None => Some(Self::Root(own)),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Owner {
    Regular,
    Enter,
    Exit,
    Change(change::Field),
}
/// Renderer-local identity assigned to an accepted run, never inferred from time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RunGeneration(pub(super) u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SegmentStamp {
    pub index: usize,
    pub start_ms_bits: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct OwnerKey {
    pub node: NodeId,
    pub mount: u64,
    pub kind: Owner,
    pub generation: RunGeneration,
    pub started: Instant,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct MemberInput {
    pub owner: OwnerKey,
    pub group: GroupKey,
    pub deadline: Instant,
    pub active: bool,
    pub resolved: bool,
    pub segment: SegmentStamp,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Member {
    segment: SegmentStamp,
    /// First admitted hold barrier, carried with the owner across placement changes.
    held_until: Option<Instant>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Group {
    members: HashMap<OwnerKey, Member>,
    barrier: Instant,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct GroupLedger {
    groups: AdmissionMap<GroupKey, Arc<Group>>,
    work: GroupWork,
    owners: AdmissionMap<OwnerKey, GroupKey>,
}
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GroupObservation {
    pub members: HashMap<OwnerKey, SegmentStamp>,
    pub barrier: Instant,
}
/// Immutable group versions. An unchanged pulse reuses the Arc; membership,
/// segment/cycle or barrier changes create a different version without hashing specs.
#[derive(Debug)]
pub(super) struct GroupTicket {
    groups: HashMap<GroupKey, Arc<Group>>,
}
impl GroupTicket {
    pub(super) fn valid_for(&self, ledger: &GroupLedger) -> bool {
        self.groups.iter().all(|(key, group)| {
            ledger
                .groups
                .get(key)
                .is_some_and(|current| Arc::ptr_eq(current, group))
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct GroupWork {
    pub input_visits: u64,
    pub member_copies: u64,
    pub group_copies: u64,
}
impl GroupLedger {
    pub(super) fn versions(&self) -> [super::admission::Version; 2] {
        [self.groups.version(), self.owners.version()]
    }

    fn edit_group(&mut self, key: GroupKey, edit: impl FnOnce(&mut Group)) -> bool {
        let Some(record) = self.groups.get_mut(&key) else {
            return false;
        };
        if Arc::strong_count(record) > 1 {
            self.work.group_copies += 1;
            self.work.member_copies += record.members.len() as u64;
        }
        edit(Arc::make_mut(record));
        true
    }
    /// Refresh from selected owners, not from a tree traversal. Expired owners may
    /// stay only if already retained; completed regular declarations cannot rejoin.
    pub(super) fn refresh(&mut self, inputs: impl IntoIterator<Item = MemberInput>, _now: Instant) {
        let visits = &mut self.work.input_visits;
        let inputs: Vec<_> = inputs
            .into_iter()
            .inspect(|_| *visits += 1)
            .filter(|input| input.active || self.owners.contains_key(&input.owner))
            .collect();
        self.work.input_visits += 2 * inputs.len() as u64;
        let eligible: HashSet<_> = inputs
            .iter()
            .filter(|input| input.resolved)
            .map(|input| input.group)
            .chain(self.groups.keys().copied())
            .collect();
        let inputs: Vec<_> = inputs
            .into_iter()
            .filter(|input| {
                eligible.contains(&input.group) || self.owners.contains_key(&input.owner)
            })
            .collect();
        self.work.input_visits += 2 * inputs.len() as u64;
        let owners: HashSet<_> = inputs.iter().map(|input| input.owner).collect();
        let removed: Vec<_> = self
            .owners
            .iter()
            .filter(|(owner, _)| !owners.contains(owner))
            .map(|(owner, group)| (*owner, *group))
            .collect();
        let mut changed = HashSet::new();
        for (owner, group) in removed {
            self.owners.remove(&owner);
            if self.edit_group(group, |record| {
                record.members.remove(&owner);
            }) {
                changed.insert(group);
            }
        }
        let barriers = inputs.into_iter().fold(
            HashMap::<GroupKey, Instant>::new(),
            |mut barriers, input| {
                let previous = self
                    .owners
                    .get(&input.owner)
                    .and_then(|key| self.groups.get(key));
                let held_until = previous.and_then(|group| {
                    group
                        .members
                        .get(&input.owner)
                        .and_then(|member| member.held_until)
                        .or_else(|| {
                            (!input.active && self.owners.get(&input.owner) != Some(&input.group))
                                .then_some(group.barrier)
                        })
                });
                let member = Member {
                    segment: input.segment,
                    held_until,
                };
                let deadline = held_until.map_or(input.deadline, |held| held.max(input.deadline));
                if self.owners.get(&input.owner) != Some(&input.group) {
                    if let Some(old) = self.owners.get(&input.owner).copied()
                        && self.edit_group(old, |record| {
                            record.members.remove(&input.owner);
                        })
                    {
                        changed.insert(old);
                    }
                    self.owners.insert(input.owner, input.group);
                }
                if !self.groups.contains_key(&input.group) {
                    self.groups.insert(
                        input.group,
                        Arc::new(Group {
                            members: HashMap::new(),
                            barrier: deadline,
                        }),
                    );
                    changed.insert(input.group);
                }
                if self
                    .groups
                    .get(&input.group)
                    .is_some_and(|group| group.members.get(&input.owner) != Some(&member))
                    && self.edit_group(input.group, |group| {
                        group.members.insert(input.owner, member);
                    })
                {
                    changed.insert(input.group);
                }
                barriers
                    .entry(input.group)
                    .and_modify(|value| *value = (*value).max(deadline))
                    .or_insert(deadline);
                barriers
            },
        );
        for (key, barrier) in barriers {
            if self
                .groups
                .get(&key)
                .is_some_and(|group| group.barrier != barrier)
                && self.edit_group(key, |group| group.barrier = barrier)
            {
                changed.insert(key);
            }
        }
        for key in changed {
            if self
                .groups
                .get(&key)
                .is_some_and(|group| group.members.is_empty())
            {
                self.groups.remove(&key);
            } else if self
                .groups
                .get(&key)
                .zip(self.groups.committed(&key))
                .is_some_and(|(next, old)| next == old)
            {
                self.groups.restore(&key);
            }
        }
        self.owners.restore_unchanged(|next, old| next == old);
    }
    /// A ready barrier is an intent, not permission to discard its owners.
    pub(super) fn ready(&self, now: Instant) -> HashSet<GroupKey> {
        self.groups
            .iter()
            .filter_map(|(key, group)| (group.barrier <= now).then_some(*key))
            .collect()
    }
    pub(super) fn ticket(
        &self,
        keys: &HashSet<GroupKey>,
    ) -> Result<GroupTicket, super::super::layout::projection::ProjectionError> {
        let groups = keys
            .iter()
            .map(|key| {
                self.groups
                    .get(key)
                    .cloned()
                    .map(|group| (*key, group))
                    .ok_or(super::super::layout::projection::ProjectionError::StalePreparation)
            })
            .collect::<Result<_, _>>()?;
        Ok(GroupTicket { groups })
    }
    pub(super) fn commit(&mut self, released: &HashSet<GroupKey>) {
        self.groups.retain(|key, _| !released.contains(key));
        self.owners.retain(|_, key| !released.contains(key));
    }
    /// A restored committed run may already be past its nominal deadline. Make
    /// its original retention visible before refresh, rather than treating it as
    /// an idle declaration trying to create a new group.
    pub(super) fn restore_owner(&mut self, owner: OwnerKey) {
        self.owners.restore(&owner);
    }
    pub(super) fn commit_admissions(&mut self) {
        self.groups.commit();
        self.owners.commit();
    }
    pub(super) fn has_staged(&self) -> bool {
        self.groups.has_staged() || self.owners.has_staged()
    }
    pub(super) fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
    pub(super) fn group(&self, owner: OwnerKey) -> Option<GroupKey> {
        self.owners.get(&owner).copied()
    }
    pub(super) fn held_until(&self, owner: OwnerKey) -> Option<Instant> {
        let group = self.groups.get(self.owners.get(&owner)?)?;
        group.members.get(&owner)?.held_until
    }
    pub(super) fn barrier(&self, key: GroupKey) -> Option<Instant> {
        self.groups.get(&key).map(|g| g.barrier)
    }
    #[cfg(test)]
    pub(super) fn observation(&self) -> HashMap<GroupKey, GroupObservation> {
        self.groups
            .iter()
            .map(|(key, group)| {
                (
                    *key,
                    GroupObservation {
                        members: group
                            .members
                            .iter()
                            .map(|(owner, member)| (*owner, member.segment))
                            .collect(),
                        barrier: group.barrier,
                    },
                )
            })
            .collect()
    }
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.groups.len()
    }
}

pub(super) fn has_layout_fields(a: &Attrs) -> bool {
    a.width.is_some()
        || a.height.is_some()
        || a.padding.is_some()
        || a.border_width.is_some()
        || a.spacing.is_some()
        || a.spacing_x.is_some()
        || a.spacing_y.is_some()
        || a.layout_scale.is_some()
        || a.layout_rotate.is_some()
        || a.font_size.is_some()
        || a.font_letter_spacing.is_some()
        || a.font_word_spacing.is_some()
}

impl AnimationRuntime {
    pub(super) fn sync_groups(&mut self, tree: &ElementTree, now: Instant) {
        let runtime = &*self;
        let inputs: Vec<_> = self
            .active_node_ids()
            .into_iter()
            .filter_map(|id| tree.get(&id))
            .flat_map(|node| {
                let group = GroupKey::for_node(tree, node.id);
                super::runs_for_element_including_finished(node, Some(self), Some(now))
                    .into_iter()
                    .filter_map(move |run| {
                        if run.fields.layout().is_empty() {
                            return None;
                        }
                        let deadline = timing::finite_deadline(run.spec, run.started)?;
                        let deadline = tree
                            .length_runtime
                            .as_ref()
                            .and_then(|lengths| lengths.hold_deadline(run.owner_key(node), now))
                            .map_or(deadline, |held| deadline.max(held));
                        let entry = AnimationRuntimeEntry {
                            spec_hash: run.revision,
                            started_at: run.started,
                        };
                        let p = timing::position(run.spec, Some(&entry), Some(now))?;
                        let phase = runtime.owner_phases(
                            run.owner_key(node),
                            run.fields,
                            p.active,
                            Some(now),
                            super::fields::FieldMask::default(),
                        );
                        if !p.active && phase.writes().is_empty() {
                            return None;
                        }
                        let a = run.fields.select(run.spec.keyframes.get(p.index)?);
                        let b = run.fields.select(run.spec.keyframes.get(p.index + 1)?);
                        if !has_layout_fields(&a) && !has_layout_fields(&b) {
                            return None;
                        }
                        let resolved = a
                            .width
                            .as_ref()
                            .zip(b.width.as_ref())
                            .is_some_and(|(a, b)| !lengths::direct_numeric(a, b))
                            || a.height
                                .as_ref()
                                .zip(b.height.as_ref())
                                .is_some_and(|(a, b)| !lengths::direct_numeric(a, b))
                            || run.source.is_some_and(|s| s.sampled.iter().any(|v| *v));
                        let axes = |test: fn(&Length) -> bool| {
                            u8::from(b.width.as_ref().is_some_and(test))
                                | (u8::from(b.height.as_ref().is_some_and(test)) << 1)
                        };
                        Some((
                            MemberInput {
                                owner: run.owner_key(node),
                                group: group?,
                                deadline,
                                active: p.active,
                                resolved,
                                segment: SegmentStamp {
                                    index: p.index,
                                    start_ms_bits: p.start_ms.to_bits(),
                                },
                            },
                            if lengths::layout_attrs(&b)
                                == (Attrs {
                                    width: b.width.clone(),
                                    height: b.height.clone(),
                                    ..Default::default()
                                })
                            {
                                axes(|length| {
                                    matches!(
                                        length,
                                        Length::Content | Length::Fill | Length::FillWeighted(_)
                                    )
                                })
                            } else {
                                3
                            },
                            if lengths::layout_attrs(&b)
                                == (Attrs {
                                    width: b.width.clone(),
                                    height: b.height.clone(),
                                    ..Default::default()
                                })
                                && matches!(
                                    b.width.as_ref().or(node.spec.declared.width.as_ref()),
                                    Some(Length::Px(_))
                                )
                                && matches!(
                                    b.height.as_ref().or(node.spec.declared.height.as_ref()),
                                    Some(Length::Px(_))
                                )
                            {
                                axes(|_| true)
                            } else {
                                3
                            },
                            axes(|_| true),
                        ))
                    })
            })
            .collect();
        // A content/fill animated parent depends on the child's intrinsic
        // endpoint, including channels invisible in a fixed viewport. Share its target
        // and barrier; fill can otherwise create circular allocation feedback.
        // These are upward links in the retained mount tree, not a future-time
        // dependency graph or a new animation/group specification.
        let (content, dependent) = inputs.iter().fold(
            (
                HashMap::<NodeId, (GroupKey, u8, u8)>::new(),
                HashMap::<GroupKey, u8>::new(),
            ),
            |(mut content, mut dependent), (input, content_axes, dependent_axes, owned_axes)| {
                content
                    .entry(input.owner.node)
                    .and_modify(|(_, axes, owned)| {
                        *axes |= *content_axes;
                        *owned |= *owned_axes;
                    })
                    .or_insert((input.group, *content_axes, *owned_axes));
                if *dependent_axes != 0 {
                    *dependent.entry(input.group).or_default() |= *dependent_axes;
                }
                (content, dependent)
            },
        );
        self.groups.work.input_visits += (inputs.len() + dependent.len()) as u64;
        let links: HashMap<_, _> = dependent
            .into_iter()
            .filter_map(|(group, axes)| {
                let GroupKey::Flow(parent) = group else {
                    return None;
                };
                let mut current = parent.id;
                let mut axes = axes;
                for _ in 0..tree.len() {
                    self.groups.work.input_visits += 1;
                    let node = tree.get(&current)?;
                    if matches!(
                        node.spec.kind,
                        crate::tree::element::ElementKind::WrappedRow
                            | crate::tree::element::ElementKind::Paragraph
                    ) || node
                        .spec
                        .declared
                        .layout_rotate
                        .is_some_and(|angle| angle != 0.0)
                    {
                        axes = 3;
                    }
                    if let Some((parent_group, parent_axes, owned_axes)) = content.get(&current) {
                        if axes & parent_axes != 0 {
                            return Some((group, *parent_group));
                        }
                        axes &= !owned_axes;
                    }
                    if matches!(node.spec.declared.width, Some(Length::Px(_))) {
                        axes &= !1;
                    }
                    if matches!(node.spec.declared.height, Some(Length::Px(_))) {
                        axes &= !2;
                    }
                    if axes == 0 {
                        return None;
                    }
                    // Nearby children do not contribute to their host's intrinsic box.
                    current = match tree.parent_link_of(tree.ix_of(&current)?)? {
                        ParentLink::Child { parent } => tree.get_ix(parent)?.id,
                        ParentLink::Nearby { .. } => return None,
                    };
                }
                None
            })
            .collect();
        // Path compression bounds ancestry work even for deep dependent scopes.
        let canonical = links.keys().fold(HashMap::new(), |mut canonical, group| {
            let path = std::iter::successors(Some(*group), |key| {
                if canonical.contains_key(key) {
                    None
                } else {
                    links.get(key).copied()
                }
            })
            .take(links.len() + 1)
            .collect::<Vec<_>>();
            self.groups.work.input_visits += path.len() as u64;
            let last = path.last().copied().unwrap_or(*group);
            let root = canonical.get(&last).copied().unwrap_or(last);
            canonical.extend(path.into_iter().map(|key| (key, root)));
            canonical
        });
        let inputs = inputs
            .into_iter()
            .map(|(mut input, _, _, _)| {
                self.groups.work.input_visits += 1;
                input.group = canonical.get(&input.group).copied().unwrap_or(input.group);
                input
            })
            .collect::<Vec<_>>();
        self.groups.refresh(inputs, now);
    }
}

#[cfg(test)]
mod tests;
