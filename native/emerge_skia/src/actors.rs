use rustler::LocalPid;
use std::time::Instant;

use crate::events::{RegistryRebuildPayload, TextInputState};
use crate::input::InputEvent;
use crate::render_scene::RenderScene;
use crate::tree::element::NodeId;

#[derive(Debug, Clone)]
pub enum TreeMsg {
    UploadTree {
        bytes: Vec<u8>,
    },
    PatchTree {
        bytes: Vec<u8>,
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
