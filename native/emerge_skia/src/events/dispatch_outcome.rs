use crate::tree::element::ElementId;

use super::{TextInputCommandRequest, TextInputEditRequest, TextInputPreeditRequest};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeKey(pub Vec<u8>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementEventKind {
    Click,
    Press,
    MouseDown,
    MouseUp,
    MouseEnter,
    MouseLeave,
    MouseMove,
    Focus,
    Blur,
    Change,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementEventOut {
    pub target: NodeKey,
    pub kind: ElementEventKind,
    pub payload: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Milli(pub i32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrollRequestOut {
    pub target: NodeKey,
    pub dx: Milli,
    pub dy: Milli,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextCommandReqOut {
    pub target: NodeKey,
    pub request: TextInputCommandRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEditReqOut {
    pub target: NodeKey,
    pub request: TextInputEditRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextPreeditReqOut {
    pub target: NodeKey,
    pub request: TextInputPreeditRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollbarAxisOut {
    X,
    Y,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrollbarThumbDragReqOut {
    pub target: NodeKey,
    pub axis: ScrollbarAxisOut,
    pub delta: Milli,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrollbarHoverReqOut {
    pub target: NodeKey,
    pub axis: ScrollbarAxisOut,
    pub hovered: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleRuntimeKind {
    MouseOver,
    MouseDown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleRuntimeReqOut {
    pub target: NodeKey,
    pub kind: StyleRuntimeKind,
    pub active: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DispatchOutcome {
    pub focus_change: Option<Option<NodeKey>>,
    pub element_events: Vec<ElementEventOut>,
    pub scroll_requests: Vec<ScrollRequestOut>,
    pub text_cursor: Vec<String>,
    pub text_command_requests: Vec<TextCommandReqOut>,
    pub text_edit_requests: Vec<TextEditReqOut>,
    pub text_preedit_requests: Vec<TextPreeditReqOut>,
    pub hover: Vec<String>,
    pub style_runtime_requests: Vec<StyleRuntimeReqOut>,
    pub scrollbar_thumb_drag_requests: Vec<ScrollbarThumbDragReqOut>,
    pub scrollbar_hover_requests: Vec<ScrollbarHoverReqOut>,
}

pub fn milli(v: f32) -> Milli {
    if v.abs() < 0.0005 {
        return Milli(0);
    }
    Milli((v * 1000.0).round() as i32)
}

pub fn node_key(id: &ElementId) -> NodeKey {
    NodeKey(id.0.clone())
}
