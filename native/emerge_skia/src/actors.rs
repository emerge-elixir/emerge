use rustler::LocalPid;
use std::time::Instant;

use crate::events::{RegistryRebuildPayload, TextInputState};
use crate::input::InputEvent;
use crate::render_scene::RenderScene;
use crate::tree::element::NodeId;

#[derive(Debug, Clone, Copy)]
pub struct AnimationPulseTrace {
    pub sequence: u64,
    pub sent_at: Instant,
}

#[derive(Debug, Clone, Copy)]
pub struct AnimationFrameTrace {
    pub sequence: Option<u64>,
    pub pulse_sent_at: Option<Instant>,
    pub tree_started_at: Instant,
    pub render_queued_at: Instant,
    pub presented_at: Option<Instant>,
    pub predicted_next_present_at: Option<Instant>,
    pub sample_time: Instant,
    pub previous_sample_time: Option<Instant>,
    pub animations_active: bool,
    pub pulse_requested_sample: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AnimationFrameTraceSeed {
    pub(crate) sequence: Option<u64>,
    pub(crate) pulse_sent_at: Option<Instant>,
    pub(crate) tree_started_at: Instant,
    pub(crate) presented_at: Option<Instant>,
    pub(crate) predicted_next_present_at: Option<Instant>,
    pub(crate) sample_time: Instant,
    pub(crate) previous_sample_time: Option<Instant>,
    pub(crate) animations_active: bool,
    pub(crate) pulse_requested_sample: bool,
}

impl AnimationFrameTraceSeed {
    pub(crate) fn queued_at(self, render_queued_at: Instant) -> AnimationFrameTrace {
        AnimationFrameTrace {
            sequence: self.sequence,
            pulse_sent_at: self.pulse_sent_at,
            tree_started_at: self.tree_started_at,
            render_queued_at,
            presented_at: self.presented_at,
            predicted_next_present_at: self.predicted_next_present_at,
            sample_time: self.sample_time,
            previous_sample_time: self.previous_sample_time,
            animations_active: self.animations_active,
            pulse_requested_sample: self.pulse_requested_sample,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TreeMsg {
    UploadTree {
        bytes: Vec<u8>,
        submitted_at: Option<Instant>,
    },
    PatchTree {
        bytes: Vec<u8>,
        submitted_at: Option<Instant>,
    },
    Resize {
        width: f32,
        height: f32,
        scale: f32,
    },
    ScrollRequest {
        element_id: NodeId,
        dx: f32,
        dy: f32,
    },
    ScrollbarThumbDragX {
        element_id: NodeId,
        dx: f32,
    },
    ScrollbarThumbDragY {
        element_id: NodeId,
        dy: f32,
    },
    SetScrollbarXHover {
        element_id: NodeId,
        hovered: bool,
    },
    SetScrollbarYHover {
        element_id: NodeId,
        hovered: bool,
    },
    SetMouseOverActive {
        element_id: NodeId,
        active: bool,
    },
    SetMouseDownActive {
        element_id: NodeId,
        active: bool,
    },
    SetFocusedActive {
        element_id: NodeId,
        active: bool,
    },
    SetTextInputContent {
        element_id: NodeId,
        content: String,
    },
    SetTextInputRuntime {
        element_id: NodeId,
        focused: bool,
        cursor: Option<u32>,
        selection_anchor: Option<u32>,
        preedit: Option<String>,
        preedit_cursor: Option<(u32, u32)>,
    },
    AnimationPulse {
        presented_at: Instant,
        predicted_next_present_at: Instant,
        trace: Option<AnimationPulseTrace>,
    },
    Batch(Vec<TreeMsg>),
    RebuildRegistry,
    AssetStateChanged,
    Stop,
}

pub enum EventMsg {
    InputEvent(InputEvent),
    PresentTiming {
        presented_at: Instant,
        predicted_next_present_at: Instant,
    },
    RegistryUpdate {
        rebuild: RegistryRebuildPayload,
    },
    SetInputMask(u32),
    SetInputTarget(Option<LocalPid>),
    Stop,
}

#[derive(Debug)]
pub enum RenderMsg {
    Scene {
        scene: Box<RenderScene>,
        version: u64,
        pipeline_submitted_at: Option<Instant>,
        pipeline_render_queued_at: Option<Instant>,
        animation_trace: Option<Box<AnimationFrameTrace>>,
        animate: bool,
        #[cfg_attr(not(all(feature = "wayland", target_os = "linux")), allow(dead_code))]
        ime_enabled: bool,
        #[cfg_attr(not(all(feature = "wayland", target_os = "linux")), allow(dead_code))]
        ime_cursor_area: Option<(f32, f32, f32, f32)>,
        #[cfg_attr(not(all(feature = "wayland", target_os = "linux")), allow(dead_code))]
        ime_text_state: Box<Option<TextInputState>>,
    },
    Stop,
}

pub(crate) fn earliest_pipeline_submitted_at(
    left: Option<Instant>,
    right: Option<Instant>,
) -> Option<Instant> {
    match (left, right) {
        (Some(left), Some(right)) => Some(if left <= right { left } else { right }),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

impl RenderMsg {
    pub(crate) fn absorb_pipeline_submitted_at(&mut self, dropped: &Self) {
        let (
            Self::Scene {
                pipeline_submitted_at,
                ..
            },
            Self::Scene {
                pipeline_submitted_at: dropped_submitted_at,
                ..
            },
        ) = (self, dropped)
        else {
            return;
        };

        *pipeline_submitted_at =
            earliest_pipeline_submitted_at(*pipeline_submitted_at, *dropped_submitted_at);
    }
}
