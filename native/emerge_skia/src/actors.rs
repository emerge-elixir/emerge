use rustler::LocalPid;

use crate::events::EventNode;
use crate::input::InputEvent;
use crate::renderer::DrawCmd;
use crate::tree::element::ElementId;

#[derive(Debug, Clone)]
pub enum ElementEvent {
    Change { value: String },
}

#[derive(Debug)]
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
        element_id: ElementId,
        dx: f32,
        dy: f32,
    },
    ScrollbarThumbDragX {
        element_id: ElementId,
        dx: f32,
    },
    ScrollbarThumbDragY {
        element_id: ElementId,
        dy: f32,
    },
    SetScrollbarXHover {
        element_id: ElementId,
        hovered: bool,
    },
    SetScrollbarYHover {
        element_id: ElementId,
        hovered: bool,
    },
    SetMouseOverActive {
        element_id: ElementId,
        active: bool,
    },
    SetTextInputFocus {
        element_id: Option<ElementId>,
    },
    TextInputMoveLeft {
        element_id: ElementId,
        extend_selection: bool,
    },
    TextInputMoveRight {
        element_id: ElementId,
        extend_selection: bool,
    },
    TextInputMoveHome {
        element_id: ElementId,
        extend_selection: bool,
    },
    TextInputMoveEnd {
        element_id: ElementId,
        extend_selection: bool,
    },
    TextInputBackspace {
        element_id: ElementId,
    },
    TextInputDelete {
        element_id: ElementId,
    },
    TextInputInsert {
        element_id: ElementId,
        text: String,
    },
    SetTextInputCursorFromPoint {
        element_id: ElementId,
        x: f32,
        extend_selection: bool,
    },
    TextInputSelectAll {
        element_id: ElementId,
    },
    TextInputCopy {
        element_id: ElementId,
    },
    TextInputCut {
        element_id: ElementId,
    },
    TextInputPaste {
        element_id: ElementId,
    },
    SetTextInputPreedit {
        element_id: ElementId,
        text: String,
        cursor: Option<(u32, u32)>,
    },
    ClearTextInputPreedit {
        element_id: ElementId,
    },
    AssetStateChanged,
    Stop,
}

pub enum EventMsg {
    InputEvent(InputEvent),
    RegistryUpdate {
        registry: Vec<EventNode>,
    },
    SetInputMask(u32),
    SetInputTarget(Option<LocalPid>),
    ElementEvent {
        element_id: ElementId,
        event: ElementEvent,
    },
    Stop,
}

#[derive(Debug)]
pub enum RenderMsg {
    Commands {
        commands: Vec<DrawCmd>,
        version: u64,
        animate: bool,
        ime_enabled: bool,
        ime_cursor_area: Option<(f32, f32, f32, f32)>,
    },
    CursorUpdate {
        pos: (f32, f32),
        visible: bool,
    },
    Stop,
}
