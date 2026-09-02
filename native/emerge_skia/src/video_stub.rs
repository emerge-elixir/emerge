use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use crossbeam_channel::{Sender, unbounded};
use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo, images};

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

#[derive(Clone)]
pub struct CpuVideoFrame {
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoSubmitResult {
    Queued,
    DroppedInactive,
}

#[derive(Default)]
struct RegistryState {
    open: bool,
    generation: u64,
    active: HashSet<String>,
    frames: HashMap<String, CpuVideoFrame>,
}

pub struct VideoRegistry {
    state: Mutex<RegistryState>,
}

impl VideoRegistry {
    pub(crate) fn new(
        _release_tx: Sender<()>,
        _cleanup_dispatcher: CleanupDispatcher,
        _stats: Option<Arc<RendererStatsCollector>>,
    ) -> Self {
        Self {
            state: Mutex::new(RegistryState {
                open: true,
                ..RegistryState::default()
            }),
        }
    }

    pub fn set_active_targets(&self, active: &HashSet<String>) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned".to_string())?;
        state.active = active.clone();
        state.frames.retain(|id, _frame| active.contains(id));
        Ok(())
    }

    pub fn target_is_active(&self, id: &str) -> Result<bool, String> {
        self.state
            .lock()
            .map(|state| state.open && state.active.contains(id))
            .map_err(|_| "video registry lock poisoned".to_string())
    }

    pub fn submit_cpu_frame(
        &self,
        id: &str,
        mut frame: CpuVideoFrame,
    ) -> Result<VideoSubmitResult, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned".to_string())?;
        if !state.open {
            return Err("video registry is closed".to_string());
        }
        if !state.active.contains(id) {
            return Ok(VideoSubmitResult::DroppedInactive);
        }
        state.generation = state.generation.saturating_add(1);
        frame.generation = state.generation;
        state.frames.insert(id.to_string(), frame);
        Ok(VideoSubmitResult::Queued)
    }

    pub fn cpu_frame_snapshot(&self) -> Result<HashMap<String, CpuVideoFrame>, String> {
        self.state
            .lock()
            .map(|state| state.frames.clone())
            .map_err(|_| "video registry lock poisoned".to_string())
    }

    pub fn close_admission(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.open = false;
        }
    }

    pub fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.open = false;
            state.frames.clear();
        }
    }
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

struct RenderedCpuFrame {
    generation: u64,
    image: Image,
    width: u32,
    height: u32,
}

#[derive(Default)]
pub struct RendererVideoState {
    frames: HashMap<String, RenderedCpuFrame>,
}

impl RendererVideoState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sync_cpu(&mut self, registry: &Arc<VideoRegistry>) -> Result<bool, String> {
        let snapshot = registry.cpu_frame_snapshot()?;
        let before = self.frames.len();
        self.frames.retain(|id, _frame| snapshot.contains_key(id));
        let mut changed = self.frames.len() != before;

        for (id, frame) in snapshot {
            if self
                .frames
                .get(&id)
                .is_some_and(|current| current.generation == frame.generation)
            {
                continue;
            }
            let info = ImageInfo::new(
                (frame.width as i32, frame.height as i32),
                ColorType::RGBA8888,
                AlphaType::Premul,
                None,
            );
            let image = images::raster_from_data(
                &info,
                Data::new_copy(frame.rgba.as_ref()),
                frame.width as usize * 4,
            )
            .ok_or_else(|| format!("failed to create binary video image for target {id}"))?;
            self.frames.insert(
                id,
                RenderedCpuFrame {
                    generation: frame.generation,
                    image,
                    width: frame.width,
                    height: frame.height,
                },
            );
            changed = true;
        }
        Ok(changed)
    }

    pub fn image(&self, id: &str) -> Option<(RenderedVideoFrame<'_>, u32, u32)> {
        self.frames.get(id).map(|frame| {
            (
                RenderedVideoFrame::Image(&frame.image),
                frame.width,
                frame.height,
            )
        })
    }
}
