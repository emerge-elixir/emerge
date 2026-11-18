//! Immutable compiled input data and the small, independently fenced delivery
//! envelope. Runtime editing/scroll/slider sessions never mutate this snapshot.
use super::*;
use std::{ops::Deref, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusKey {
    pub element_id: NodeId,
    pub mounted_at_revision: u64,
}
impl From<&FocusOnMountTarget> for FocusKey {
    fn from(target: &FocusOnMountTarget) -> Self {
        Self {
            element_id: target.element_id,
            mounted_at_revision: target.mounted_at_revision,
        }
    }
}

// Deliberately not Clone: input handling cannot accidentally shift a full-map
// copy to Arc::make_mut. Only construction owns mutable maps.
#[derive(Debug, Default)]
pub struct CompiledRegistryData {
    pub mount_focus_targets: Option<Arc<HashMap<NodeId, FocusOnMountTarget>>>,
    pub base_registry: registry_builder::Registry,
    pub scrolls: HashMap<NodeId, scroll::ScrollNode>,
    pub text_inputs: HashMap<NodeId, TextInputState>,
    pub sliders: HashMap<NodeId, SliderState>,
    pub scrollbars: HashMap<(NodeId, ScrollbarAxis), ScrollbarNode>,
    pub focused_id: Option<NodeId>,
    pub(crate) initial_mount_focus: Option<FocusKey>,
}

impl CompiledRegistryData {
    pub(crate) fn focus_target(&self, key: Option<FocusKey>) -> Option<&FocusOnMountTarget> {
        let key = key?;
        self.mount_focus_targets
            .as_ref()?
            .get(&key.element_id)
            .filter(|target| target.mounted_at_revision == key.mounted_at_revision)
    }
}

#[derive(Clone, Debug, Default)]
pub struct RegistryRebuildPayload {
    pub(crate) compiled: Arc<CompiledRegistryData>,
    pub listener_barrier: Option<crate::actors::ListenerBarrier>,
    pub focus_on_mount: Option<FocusKey>,
}
impl Deref for RegistryRebuildPayload {
    type Target = CompiledRegistryData;
    fn deref(&self) -> &Self::Target {
        &self.compiled
    }
}
impl RegistryRebuildPayload {
    pub(crate) fn new(data: CompiledRegistryData) -> Self {
        let focus_on_mount = data.initial_mount_focus;
        let payload = Self {
            compiled: Arc::new(data),
            listener_barrier: None,
            focus_on_mount,
        };
        debug_assert!(focus_on_mount.is_none() || payload.focus_on_mount_target().is_some());
        payload
    }
    pub(crate) fn for_revision(&self, revision: u64) -> Self {
        Self {
            compiled: Arc::clone(&self.compiled),
            listener_barrier: self.listener_barrier.clone(),
            focus_on_mount: self
                .compiled
                .initial_mount_focus
                .filter(|key| key.mounted_at_revision == revision),
        }
    }
    pub(crate) fn shares_compilation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.compiled, &other.compiled)
    }
    pub(crate) fn focus_on_mount_target(&self) -> Option<&FocusOnMountTarget> {
        self.compiled.focus_target(self.focus_on_mount)
    }
    pub(crate) fn carry_mount_focus(
        &mut self,
        pending: Option<FocusKey>,
        previous_focused: Option<NodeId>,
    ) {
        if self.focus_on_mount.is_none()
            && (self.focused_id.is_none() || self.focused_id == previous_focused)
        {
            self.focus_on_mount = pending.filter(|key| {
                self.mount_focus_targets
                    .as_ref()
                    .and_then(|targets| targets.get(&key.element_id))
                    .is_some_and(|target| target.mounted_at_revision == key.mounted_at_revision)
            });
        }
    }
}

// Fixture construction only. Tests may construct new complete data from owned
// maps without granting production input code a mutable compiled-data API.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct RebuildFixture {
    pub listener_barrier: Option<crate::actors::ListenerBarrier>,
    pub mount_focus_targets: Option<Arc<HashMap<NodeId, FocusOnMountTarget>>>,
    pub base_registry: registry_builder::Registry,
    pub scrolls: HashMap<NodeId, scroll::ScrollNode>,
    pub text_inputs: HashMap<NodeId, TextInputState>,
    pub sliders: HashMap<NodeId, SliderState>,
    pub scrollbars: HashMap<(NodeId, ScrollbarAxis), ScrollbarNode>,
    pub focused_id: Option<NodeId>,
    pub focus_on_mount: Option<FocusOnMountTarget>,
}
#[cfg(test)]
impl From<RegistryRebuildPayload> for RebuildFixture {
    fn from(payload: RegistryRebuildPayload) -> Self {
        Self {
            listener_barrier: payload.listener_barrier.clone(),
            mount_focus_targets: payload.mount_focus_targets.clone(),
            base_registry: payload.base_registry.clone(),
            scrolls: payload.scrolls.clone(),
            text_inputs: payload.text_inputs.clone(),
            sliders: payload.sliders.clone(),
            scrollbars: payload.scrollbars.clone(),
            focused_id: payload.focused_id,
            focus_on_mount: payload.focus_on_mount_target().cloned(),
        }
    }
}
#[cfg(test)]
impl RebuildFixture {
    pub(crate) fn finish(mut self) -> RegistryRebuildPayload {
        let key = self.focus_on_mount.as_ref().map(FocusKey::from);
        if let Some(target) = self.focus_on_mount {
            let targets = self
                .mount_focus_targets
                .get_or_insert_with(|| Arc::new(HashMap::new()));
            Arc::make_mut(targets).insert(target.element_id, target);
        }
        let mut payload = RegistryRebuildPayload::new(CompiledRegistryData {
            mount_focus_targets: self.mount_focus_targets,
            base_registry: self.base_registry,
            scrolls: self.scrolls,
            text_inputs: self.text_inputs,
            sliders: self.sliders,
            scrollbars: self.scrollbars,
            focused_id: self.focused_id,
            initial_mount_focus: key,
        });
        payload.listener_barrier = self.listener_barrier;
        payload
    }
}
#[cfg(test)]
macro_rules! rebuild_payload {
    (@value $name:ident, $value:expr) => { $value };
    (@value $name:ident) => { $name };
    ($($name:ident $(: $value:expr)?,)* ..$base:expr $(,)?) => {
        $crate::events::RebuildFixture {
            $($name: $crate::events::rebuild_payload!(@value $name $(, $value)?),)*
            ..$crate::events::RebuildFixture::from($base)
        }.finish()
    };
    ($($name:ident $(: $value:expr)?),* $(,)?) => {
        $crate::events::RebuildFixture {
            $($name: $crate::events::rebuild_payload!(@value $name $(, $value)?),)*
        }.finish()
    };
}
#[cfg(test)]
pub(crate) use rebuild_payload;

#[cfg(test)]
mod tests;
