//! Selected fields of one regular specification. Admission clocks belong to runs,
//! never groups. One enter can expose fields at mount, paint completion and release;
//! consequently there are at most three selected runs, sharing one specification.
use super::*;
use fields::FieldMask;

#[derive(Clone, Debug)]
pub(super) struct SelectedFields {
    pub fields: FieldMask,
    pub clock: AnimationRuntimeEntry,
    pub generation: RunGeneration,
    pub source: Option<Arc<source::PresentationSource>>,
}
impl AnimationRuntime {
    fn regular_blocked(&self, tree: &ElementTree, id: NodeId, now: Instant) -> FieldMask {
        self.enter_entries
            .get(&id)
            .map(|entry| {
                if entry_is_active(&entry.spec, entry.started_at, now) {
                    tree.get(&id)
                        .and_then(|node| node.spec.declared.animate.as_ref())
                        .map(FieldMask::from_spec)
                        .unwrap_or_default()
                } else {
                    self.enter_phases(tree, id, entry, now).writes()
                }
            })
            .unwrap_or_default()
    }
    pub(super) fn regular_handoff_needs_generation(
        &self,
        node: &super::super::element::Element,
    ) -> bool {
        let Some(spec) = node.spec.declared.animate.as_ref() else {
            return false;
        };
        self.animate_entries.get(&node.id).is_none_or(|entry| {
            entry.mount != node.lifecycle.mounted_at_revision
                || entry.clock.spec_hash != spec_fingerprint(spec)
                || (!entry.pending.is_empty() && !entry.fields.is_empty())
        })
    }
    pub(super) fn sync_regular_handoffs(
        &mut self,
        tree: &ElementTree,
        now: Instant,
    ) -> Result<(), ProjectionError> {
        let ids: Vec<_> = self
            .animate_entries
            .iter()
            .filter_map(|(id, entry)| (!entry.pending.is_empty()).then_some(*id))
            .chain(self.enter_entries.iter().filter_map(|(id, entry)| {
                (!entry_is_active(&entry.spec, entry.started_at, now)).then_some(*id)
            }))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        for id in ids {
            self.sync_regular_node(tree, id, now)?;
        }
        Ok(())
    }
    pub(super) fn sync_regular_node(
        &mut self,
        tree: &ElementTree,
        id: NodeId,
        now: Instant,
    ) -> Result<(), ProjectionError> {
        let enter = self
            .enter_entries
            .get(&id)
            .map(|entry| Arc::clone(&entry.spec));
        self.sync_regular_node_from(tree, id, now, enter.as_deref())
    }
    pub(super) fn sync_regular_node_from(
        &mut self,
        tree: &ElementTree,
        id: NodeId,
        now: Instant,
        enter: Option<&AnimationSpec>,
    ) -> Result<(), ProjectionError> {
        let Some(node) = tree.get(&id).filter(|node| node.is_live()) else {
            return Ok(());
        };
        let Some(spec) = node.spec.declared.animate.as_ref() else {
            self.animate_entries.remove(&id);
            return Ok(());
        };
        let hash = self
            .animate_entries
            .get(&id)
            .filter(|entry| {
                self.synced
                    && self.last_seen_revision == tree.revision()
                    && entry.mount == node.lifecycle.mounted_at_revision
            })
            .map(|entry| entry.clock.spec_hash)
            .unwrap_or_else(|| spec_fingerprint(spec));
        let blocked = self.regular_blocked(tree, id, now);
        if let Some(base) = self.animate_entries.committed(&id).filter(|entry| {
            entry.mount == node.lifecycle.mounted_at_revision && entry.clock.spec_hash == hash
        }) && self
            .animate_entries
            .get(&id)
            .is_none_or(|entry| entry.generation != base.generation)
        {
            for (generation, started) in std::iter::once((base.generation, base.clock.started_at))
                .chain(
                    base.handoffs
                        .iter()
                        .map(|lane| (lane.generation, lane.clock.started_at)),
                )
            {
                self.groups.restore_owner(groups::OwnerKey {
                    node: id,
                    mount: base.mount,
                    kind: groups::Owner::Regular,
                    generation,
                    started,
                });
            }
            self.animate_entries.restore(&id);
        }
        if self.animate_entries.get(&id).is_none_or(|entry| {
            entry.clock.spec_hash != hash || entry.mount != node.lifecycle.mounted_at_revision
        }) {
            let all = FieldMask::from_spec(spec);
            let pending = all.blocked_by(blocked);
            if all.without(pending).is_empty() {
                return Ok(());
            }
            let generation = self.allocate_generation()?;
            self.animate_entries.insert(
                id,
                OwnedAnimationEntry {
                    spec: Arc::new(spec.clone()),
                    source: handoff_source(tree, id, all.without(pending), spec, enter),
                    fields: all.without(pending),
                    pending,
                    handoffs: Vec::new(),
                    mount: node.lifecycle.mounted_at_revision,
                    clock: AnimationRuntimeEntry {
                        spec_hash: hash,
                        started_at: now,
                    },
                    generation,
                },
            );
            return Ok(());
        }
        let entry = self
            .animate_entries
            .get(&id)
            .expect("selected regular entry");
        let available = entry.pending.without(entry.pending.blocked_by(blocked));
        if available.is_empty() {
            return Ok(());
        }
        let first = entry.fields.is_empty();
        let generation = if first {
            entry.generation
        } else {
            self.allocate_generation()?
        };
        let source = handoff_source(tree, id, available, spec, enter);
        let entry = self
            .animate_entries
            .get_mut(&id)
            .expect("selected regular entry");
        entry.pending = entry.pending.without(available);
        if first {
            entry.fields = available;
            entry.clock.started_at = now;
            entry.source = source;
        } else {
            entry.handoffs.push(Arc::new(SelectedFields {
                fields: available,
                clock: AnimationRuntimeEntry {
                    spec_hash: hash,
                    started_at: now,
                },
                generation,
                source,
            }));
        }
        debug_assert!(
            entry.handoffs.len() <= 2,
            "a single enter has only paint and geometry completion boundaries"
        );
        Ok(())
    }
    pub(super) fn compact_regular_sources(&mut self) {
        let Some(now) = self.synced_sample_time else {
            return;
        };
        let obsolete =
            |id, mount, spec: &AnimationSpec, clock: AnimationRuntimeEntry, generation| {
                let phase = timing::position(spec, Some(&clock), Some(now));
                phase.is_some_and(|p| {
                    p.start_ms > 0.0
                        || (!p.active
                            && self
                                .groups
                                .group(groups::OwnerKey {
                                    node: id,
                                    mount,
                                    kind: groups::Owner::Regular,
                                    generation,
                                    started: clock.started_at,
                                })
                                .is_none())
                })
            };
        let edits: Vec<_> = self
            .animate_entries
            .iter()
            .filter_map(|(id, entry)| {
                let first = entry.source.is_some()
                    && obsolete(*id, entry.mount, &entry.spec, entry.clock, entry.generation);
                let lanes: Vec<_> = entry
                    .handoffs
                    .iter()
                    .enumerate()
                    .filter_map(|(index, lane)| {
                        (lane.source.is_some()
                            && obsolete(*id, entry.mount, &entry.spec, lane.clock, lane.generation))
                        .then_some(index)
                    })
                    .collect();
                (first || !lanes.is_empty()).then_some((*id, first, lanes))
            })
            .collect();
        for (id, first, lanes) in edits {
            let entry = self.animate_entries.get_mut(&id).expect("retained regular");
            if first {
                entry.source = None;
            }
            for index in lanes {
                Arc::make_mut(&mut entry.handoffs[index]).source = None;
            }
        }
    }
    pub(super) fn regular_runs<'a>(
        &'a self,
        node: &super::super::element::Element,
        now: Instant,
    ) -> Vec<lengths::RunRef<'a>> {
        let Some(entry) = self
            .animate_entries
            .get(&node.id)
            .filter(|entry| entry.mount == node.lifecycle.mounted_at_revision && node.is_live())
        else {
            return Vec::new();
        };
        let blocked = self
            .enter_entries
            .get(&node.id)
            .map(|enter| {
                if entry_is_active(&enter.spec, enter.started_at, now) {
                    FieldMask::from_spec(&entry.spec)
                } else {
                    self.owner_phases(
                        groups::OwnerKey {
                            node: node.id,
                            mount: enter.mount,
                            kind: groups::Owner::Enter,
                            generation: enter.generation,
                            started: enter.started_at,
                        },
                        enter.fields,
                        false,
                        Some(now),
                        FieldMask::default(),
                    )
                    .writes()
                }
            })
            .unwrap_or_default();
        std::iter::once(lengths::RunRef {
            fields: entry.fields,
            spec: &entry.spec,
            started: entry.clock.started_at,
            owner: groups::Owner::Regular,
            revision: entry.clock.spec_hash,
            generation: entry.generation,
            source: entry.source.as_deref(),
        })
        .chain(entry.handoffs.iter().map(|lane| lengths::RunRef {
            fields: lane.fields,
            spec: &entry.spec,
            started: lane.clock.started_at,
            owner: groups::Owner::Regular,
            revision: lane.clock.spec_hash,
            generation: lane.generation,
            source: lane.source.as_deref(),
        }))
        .map(|mut run| {
            run.fields = run.fields.without(run.fields.blocked_by(blocked));
            run
        })
        .filter(|run| !run.fields.is_empty())
        .collect()
    }
}

fn handoff_source(
    tree: &ElementTree,
    id: NodeId,
    fields: FieldMask,
    spec: &AnimationSpec,
    enter: Option<&AnimationSpec>,
) -> Option<Arc<source::PresentationSource>> {
    let mut source = source::PresentationSource::capture(tree, &id)?;
    source.declared = fields.select(&source.declared);
    source.effective = fields.select(&source.effective);
    let first = spec.keyframes.first()?;
    for (i, field) in [change::Field::Width, change::Field::Height]
        .into_iter()
        .enumerate()
    {
        // Different explicit initial expressions are intentional new-run boundaries,
        // not implicit changes. Only matching expressions can reuse a footprint.
        if !fields.intersects(FieldMask::field(field))
            || enter
                .and_then(|spec| spec.keyframes.last())
                .is_none_or(|last| field.extract(first) != field.extract(last))
        {
            source.dimensions[i] = None;
            source.sampled[i] = false;
        }
    }
    Some(Arc::new(source))
}
