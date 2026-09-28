//! First-write presentation preimages. This is pending frame work, not idle target history.
use super::super::layout::dimensions::{Axis, AxisFootprint, capture_axis};
use super::super::{
    attrs::Attrs,
    element::{ElementKind, ElementTree, NearbySlot, NodeId, ParentLink},
    invalidation::TreeInvalidation,
};
use std::{collections::HashMap, sync::Arc};
type AttachmentPath = Vec<(NodeId, u64, ElementKind, Option<NearbySlot>)>;

#[derive(Clone, Debug)]
pub struct PresentationSource {
    pub mounted_at: u64,
    pub model: u64,
    pub declared: Attrs,
    /// Existing effective coordinates, before patch or scale preparation.
    pub effective: Attrs,
    pub scale: f32,
    pub dimensions: [Option<AxisFootprint>; 2],
    pub sampled: [bool; 2],
    pub path: Arc<AttachmentPath>,
    transport_origin: Arc<TransportOrigin>,
}

#[derive(Clone, Debug)]
struct TransportOrigin {
    mounted_at: u64,
    model: u64,
    scale: f32,
    dimensions: [Option<AxisFootprint>; 2],
    path: Arc<AttachmentPath>,
}
impl PresentationSource {
    pub(crate) fn attachment_path(tree: &ElementTree, id: &NodeId) -> AttachmentPath {
        std::iter::successors(tree.ix_of(id), |ix| {
            if tree.id_of(*ix) == tree.root_id() {
                return None;
            }
            match tree.parent_link_of(*ix)? {
                ParentLink::Child { parent } => Some(parent),
                ParentLink::Nearby { host, .. } => Some(host),
            }
        })
        .take(tree.len())
        .filter_map(|ix| {
            let node = tree.get_ix(ix)?;
            let slot = match tree.parent_link_of(ix) {
                Some(ParentLink::Nearby { slot, .. }) if Some(node.id) != tree.root_id() => {
                    Some(slot)
                }
                _ => None,
            };
            Some((
                node.id,
                node.lifecycle.mounted_at_revision,
                node.spec.kind,
                slot,
            ))
        })
        .collect()
    }
    pub(crate) fn attachment_changed(&self, tree: &ElementTree, id: &NodeId, axis: Axis) -> bool {
        tree.get(id)
            .is_some_and(|node| node.lifecycle.mounted_at_revision == self.mounted_at)
            && (*self.path != Self::attachment_path(tree, id)
                || self.dimensions[if axis == Axis::Width { 0 } else { 1 }].is_some_and(|fp| {
                    fp.scope
                        != super::super::layout::dimensions::declared_allocation_scope(
                            tree, id, axis,
                        )
                }))
    }
    pub(crate) fn valid_transport_origin(&self) -> bool {
        self.mounted_at == self.transport_origin.mounted_at
            && self.model == self.transport_origin.model
            && self.scale == self.transport_origin.scale
            && self.dimensions == self.transport_origin.dimensions
            && Arc::ptr_eq(&self.path, &self.transport_origin.path)
    }
    pub fn capture(tree: &ElementTree, id: &NodeId) -> Option<Self> {
        let node = tree.get(id)?;
        // A mount that has never had layout has no presentation to interrupt.
        node.layout.frame?;
        let published = tree.publication.0?;
        if node.lifecycle.mounted_at_revision > published.revision {
            return None;
        }
        let dimensions = [Axis::Width, Axis::Height].map(|axis| capture_axis(tree, id, axis));
        let path = Arc::new(Self::attachment_path(tree, id));
        if path.last().map(|(id, mount, ..)| (*id, *mount)) != published.root {
            return None;
        }
        let transport_origin = Arc::new(TransportOrigin {
            mounted_at: node.lifecycle.mounted_at_revision,
            model: published.model,
            scale: node.layout.dimension_facts.scale,
            dimensions,
            path: Arc::clone(&path),
        });
        Some(Self {
            mounted_at: node.lifecycle.mounted_at_revision,
            model: published.model,
            declared: node.spec.declared.clone(),
            effective: node.layout.effective.clone(),
            scale: node.layout.dimension_facts.scale,
            path,
            dimensions,
            transport_origin,
            sampled: [Axis::Width, Axis::Height].map(|axis| {
                node.layout
                    .dimension_samples
                    .as_deref()
                    .is_some_and(|samples| samples.get(axis).is_some())
            }),
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct PatchEffects {
    pub invalidation: TreeInvalidation,
    pub model_changed: bool,
    pub sources: HashMap<NodeId, PresentationSource>,
}

impl ElementTree {
    /// Capture once before geometry/attrs are overwritten. A reused id with a new
    /// mount is a different source. The current declaration is the latest target;
    /// keeping a second target copy in this journal would be redundant history.
    pub(crate) fn capture_animation_source(&mut self, id: &NodeId) {
        if self
            .pending_patch_effects
            .sources
            .get(id)
            .is_some_and(|source| {
                self.get(id)
                    .is_some_and(|node| node.lifecycle.mounted_at_revision == source.mounted_at)
            })
        {
            return;
        }
        if let Some(source) = PresentationSource::capture(self, id) {
            self.invalidate_unapplied_frame();
            self.pending_patch_effects.sources.insert(*id, source);
        }
    }

    pub(crate) fn capture_animation_subtree(&mut self, root: &NodeId) {
        let mut pending = vec![*root];
        let ids: Vec<_> = std::iter::from_fn(|| {
            let id = pending.pop()?;
            pending.extend(self.child_ids(&id));
            pending.extend(
                self.nearby_mounts_for(&id)
                    .into_iter()
                    .map(|mount| mount.id),
            );
            Some(id)
        })
        .collect();
        for id in ids {
            self.capture_animation_source(&id);
        }
    }

    pub(crate) fn finish_patch_frame(&mut self) {
        self.pending_patch_effects = PatchEffects::default();
    }
}

#[cfg(test)]
mod tests;
