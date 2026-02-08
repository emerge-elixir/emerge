use rustler::LocalPid;

use crate::events::EventNode;
use crate::input::InputEvent;
use crate::renderer::DrawCmd;
use crate::tree::element::ElementId;

#[derive(Debug)]
pub enum TreeMsg {
    UploadTree {
        bytes: Vec<u8>,
        width: f32,
        height: f32,
        scale: f32,
    },
    PatchTree {
        bytes: Vec<u8>,
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
    Stop,
}

pub enum EventMsg {
    InputEvent(InputEvent),
    RegistryUpdate { registry: Vec<EventNode> },
    SetInputMask(u32),
    SetInputTarget(Option<LocalPid>),
    Stop,
}

#[derive(Debug)]
pub enum RenderMsg {
    Commands {
        commands: Vec<DrawCmd>,
        version: u64,
    },
    CursorUpdate {
        pos: (f32, f32),
        visible: bool,
    },
    Stop,
}
