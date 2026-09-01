use std::sync::Arc;

use crossbeam_channel::{Sender, unbounded};
use skia_safe::Image;

use crate::{CleanupDispatcher, backend::wake::BackendWakeHandle, stats::RendererStatsCollector};

pub fn spawn_release_worker() -> std::io::Result<Sender<()>> {
    let (sender, _receiver) = unbounded();
    Ok(sender)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VideoStreamIdentity {
    pub renderer_epoch: u64,
    pub target_id: String,
    pub target_incarnation: u64,
    pub stream_id: u64,
}

pub struct VideoRegistry;

impl VideoRegistry {
    pub(crate) fn new(
        _release_tx: Sender<()>,
        _cleanup_dispatcher: CleanupDispatcher,
        _stats: Option<Arc<RendererStatsCollector>>,
    ) -> Self {
        Self
    }

    pub fn close_admission(&self) {}

    pub fn close(&self) {}
}

#[derive(Clone)]
pub struct VideoWake(BackendWakeHandle);

impl VideoWake {
    pub fn noop() -> Self {
        Self(BackendWakeHandle::noop())
    }

    pub fn notify(&self) {
        self.0.notify_video_frame();
    }
}

#[allow(dead_code)]
pub(crate) enum RenderedVideoFrame<'a> {
    Image(&'a Image),
}

#[derive(Default)]
pub struct RendererVideoState;

impl RendererVideoState {
    pub fn new() -> Self {
        Self
    }

    pub fn image(&self, _id: &str) -> Option<(RenderedVideoFrame<'_>, u32, u32)> {
        None
    }
}
