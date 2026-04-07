use crate::tree::attrs::{BorderStyle, ImageFit};
use crate::tree::geometry::ClipShape;
use crate::tree::transform::Affine2;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderScene {
    pub nodes: Vec<RenderNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenderNode {
    ShadowPass {
        children: Vec<RenderNode>,
    },
    Clip {
        clips: Vec<ClipShape>,
        children: Vec<RenderNode>,
    },
    RelaxedClip {
        clips: Vec<ClipShape>,
        children: Vec<RenderNode>,
    },
    Transform {
        transform: Affine2,
        children: Vec<RenderNode>,
    },
    Alpha {
        alpha: f32,
        children: Vec<RenderNode>,
    },
    Primitive(DrawPrimitive),
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawPrimitive {
    Rect(f32, f32, f32, f32, u32),
    RoundedRect(f32, f32, f32, f32, f32, u32),
    Border(f32, f32, f32, f32, f32, f32, u32, BorderStyle),
    BorderCorners(
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        u32,
        BorderStyle,
    ),
    BorderEdges(
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        u32,
        BorderStyle,
    ),
    Shadow(f32, f32, f32, f32, f32, f32, f32, f32, f32, u32),
    InsetShadow(f32, f32, f32, f32, f32, f32, f32, f32, f32, u32),
    TextWithFont(f32, f32, String, f32, u32, String, u16, bool),
    Gradient(f32, f32, f32, f32, u32, u32, f32),
    Image(f32, f32, f32, f32, String, ImageFit, Option<u32>),
    Video(f32, f32, f32, f32, String, ImageFit),
    ImageLoading(f32, f32, f32, f32),
    ImageFailed(f32, f32, f32, f32),
}
