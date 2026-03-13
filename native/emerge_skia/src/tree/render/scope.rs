use crate::renderer::DrawCmd;
use crate::tree::geometry::ClipShape;

#[derive(Clone, Debug, Default)]
pub(crate) struct RenderScope {
    pub(crate) inherited_clip_mode: InheritedClipMode,
    pub(crate) local_clip: Option<ClipShape>,
    pub(crate) host_clip: Option<HostClipDescriptor>,
    pub(crate) transform: Option<ScopeTransform>,
    pub(crate) items: Vec<RenderItem>,
}

#[derive(Clone, Debug)]
pub(crate) enum RenderItem {
    Draw(DrawCmd),
    Scope(RenderScope),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ScopeTransform {
    pub(crate) move_x: f32,
    pub(crate) move_y: f32,
    pub(crate) rotate: f32,
    pub(crate) scale: f32,
    pub(crate) center_x: f32,
    pub(crate) center_y: f32,
    pub(crate) alpha: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HostClipDescriptor {
    pub(crate) clip: ClipShape,
    pub(crate) scroll_x: bool,
    pub(crate) scroll_y: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum InheritedClipMode {
    #[default]
    Normal,
    None,
    ShadowAxes,
}
