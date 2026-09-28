//! Frame authority. Tokens bind a preparation to a tree and a live owner epoch;
//! changed inputs invalidate them before any presentation write is applied.
use super::*;
use crate::tree::layout::Constraint;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug)]
pub(super) struct RuntimeIdentity(AtomicBool);
impl Default for RuntimeIdentity {
    fn default() -> Self {
        Self(AtomicBool::new(true))
    }
}
impl RuntimeIdentity {
    pub(super) fn invalidate(&self) {
        self.0.store(false, Ordering::Release);
    }
    fn valid(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Owning runtime handle invalidates outstanding capabilities when replaced or dropped.
#[derive(Debug, Default)]
pub(super) struct RuntimeAuthority(pub(super) Arc<RuntimeIdentity>);
impl std::ops::Deref for RuntimeAuthority {
    type Target = Arc<RuntimeIdentity>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for RuntimeAuthority {
    fn drop(&mut self) {
        self.0.invalidate();
    }
}

/// Cloning model/presentation storage never clones application authority.
#[derive(Debug, Default)]
pub(crate) struct TreeAuthority(Arc<()>, bool);
impl TreeAuthority {
    pub(crate) fn deferred(&self) -> bool {
        self.1
    }
}
impl Clone for TreeAuthority {
    fn clone(&self) -> Self {
        Self::default()
    }
}

/// A cached layout from another mount/clone is not a publication of this tree.
#[derive(Debug, Default)]
pub(crate) struct Publication(pub(crate) Option<PublishedLayout>);
#[derive(Clone, Copy, Debug)]
pub(crate) struct PublishedLayout {
    pub revision: u64,
    pub model: u64,
    pub structure: u64,
    pub root: Option<(NodeId, u64)>,
}
impl Publication {
    pub(crate) fn record(
        &mut self,
        revision: u64,
        model: u64,
        structure: u64,
        root: Option<(NodeId, u64)>,
    ) {
        self.0 = Some(PublishedLayout {
            revision,
            model,
            structure,
            root,
        });
    }
}
impl Clone for Publication {
    fn clone(&self) -> Self {
        Self::default()
    }
}

#[derive(Debug, Default)]
pub(crate) struct AppliedState(pub(crate) Option<Box<FrameToken>>);
impl Clone for AppliedState {
    fn clone(&self) -> Self {
        Self::default()
    }
}

#[derive(Debug)]
pub(crate) struct FrameToken {
    pub(super) ghosts: Vec<groups::OwnerKey>,
    tree: Arc<()>,
    attempt: Arc<()>,
    owner: Option<Arc<RuntimeIdentity>>,
    generation: Option<u64>,
    admissions: Option<[admission::Version; 6]>,
    synced_time: Option<Instant>,
    revision: u64,
    model: u64,
    root: Option<(NodeId, u64)>,
    constraint: Option<Constraint>,
    scale: f32,
}
impl FrameToken {
    pub(crate) fn prepare(tree: &mut ElementTree, runtime: Option<&AnimationRuntime>) -> Self {
        let attempt = Arc::new(());
        tree.prepared_attempt = Some(Arc::clone(&attempt));
        tree.applied_animation_frame.0 = None;
        Self {
            ghosts: runtime
                .map(|runtime| runtime.ghost_retirements(tree))
                .unwrap_or_default(),
            tree: Arc::clone(&tree.animation_authority.0),
            attempt,
            owner: runtime.and_then(|r| r.identity.as_ref().map(|id| Arc::clone(id))),
            generation: runtime.map(|r| r.last_generation),
            admissions: runtime.map(AnimationRuntime::admission_versions),
            synced_time: runtime.and_then(|r| r.synced_sample_time),
            revision: tree.revision(),
            model: tree.layout_model_epoch,
            root: tree.root_id().and_then(|id| {
                tree.get(&id)
                    .map(|node| (id, node.lifecycle.mounted_at_revision))
            }),
            constraint: tree.animation_constraint,
            scale: tree.current_scale(),
        }
    }
    pub(crate) fn validate(&self, tree: &ElementTree) -> Result<(), ProjectionError> {
        if !Arc::ptr_eq(&self.tree, &tree.animation_authority.0)
            || tree
                .prepared_attempt
                .as_ref()
                .is_none_or(|id| !Arc::ptr_eq(id, &self.attempt))
            || self.owner.as_ref().is_some_and(|owner| !owner.valid())
            || self.revision != tree.revision()
            || self.model != tree.layout_model_epoch
            || self.constraint != tree.animation_constraint
            || self.scale != tree.current_scale()
            || self.root
                != tree.root_id().and_then(|id| {
                    tree.get(&id)
                        .map(|node| (id, node.lifecycle.mounted_at_revision))
                })
        {
            return Err(ProjectionError::StalePreparation);
        }
        Ok(())
    }
    pub(crate) fn validate_owner(
        &self,
        runtime: Option<&AnimationRuntime>,
    ) -> Result<(), ProjectionError> {
        if self.admissions != runtime.map(AnimationRuntime::admission_versions)
            || self.generation != runtime.map(|r| r.last_generation)
            || self.synced_time != runtime.and_then(|r| r.synced_sample_time)
        {
            return Err(ProjectionError::StalePreparation);
        }
        let current = runtime.and_then(|r| r.identity.as_ref());
        match (self.owner.as_ref(), current) {
            (Some(old), Some(current)) if Arc::ptr_eq(old, current) => Ok(()),
            (None, None) => Ok(()),
            _ => Err(ProjectionError::StalePreparation),
        }
    }
    pub(crate) fn applied(mut self, tree: &mut ElementTree) {
        tree.prepared_attempt = Some(Arc::clone(&self.attempt));
        self.scale = tree.current_scale();
        tree.animation_authority.1 = self.owner.is_some();
        tree.applied_animation_frame.0 = Some(Box::new(self));
    }
}
