use std::fmt;

use crate::tree::attrs::{BorderStyle, ImageFit};
use crate::tree::geometry::{ClipShape, Rect};
use crate::tree::transform::Affine2;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderScene {
    pub nodes: Vec<RenderNode>,
}

impl RenderScene {
    pub fn summary(&self) -> RenderSceneSummary {
        let mut summary = RenderSceneSummary::default();
        summary.record_nodes(&self.nodes);
        summary
    }

    pub fn has_cache_candidates(&self) -> bool {
        nodes_have_cache_candidates(&self.nodes)
    }
}

fn nodes_have_cache_candidates(nodes: &[RenderNode]) -> bool {
    nodes.iter().any(|node| match node {
        RenderNode::ShadowPass { children }
        | RenderNode::Clip { children, .. }
        | RenderNode::RelaxedClip { children, .. }
        | RenderNode::Transform { children, .. }
        | RenderNode::Alpha { children, .. } => nodes_have_cache_candidates(children),
        RenderNode::CacheCandidate(_) => true,
        RenderNode::Primitive(_) => false,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderSceneSummary {
    pub nodes: usize,
    pub shadow_passes: usize,
    pub clips: usize,
    pub relaxed_clips: usize,
    pub clip_shapes: usize,
    pub transforms: usize,
    pub alphas: usize,
    pub primitives: usize,
    pub rects: usize,
    pub rounded_rects: usize,
    pub borders: usize,
    pub border_corners: usize,
    pub border_edges: usize,
    pub shadows: usize,
    pub inset_shadows: usize,
    pub texts: usize,
    pub text_bytes: usize,
    pub gradients: usize,
    pub images: usize,
    pub videos: usize,
    pub image_loading: usize,
    pub image_failed: usize,
}

impl RenderSceneSummary {
    fn record_nodes(&mut self, nodes: &[RenderNode]) {
        for node in nodes {
            self.nodes += 1;

            match node {
                RenderNode::ShadowPass { children } => {
                    self.shadow_passes += 1;
                    self.record_nodes(children);
                }
                RenderNode::Clip { clips, children } => {
                    self.clips += 1;
                    self.clip_shapes += clips.len();
                    self.record_nodes(children);
                }
                RenderNode::RelaxedClip { clips, children } => {
                    self.relaxed_clips += 1;
                    self.clip_shapes += clips.len();
                    self.record_nodes(children);
                }
                RenderNode::Transform { children, .. } => {
                    self.transforms += 1;
                    self.record_nodes(children);
                }
                RenderNode::Alpha { children, .. } => {
                    self.alphas += 1;
                    self.record_nodes(children);
                }
                RenderNode::CacheCandidate(candidate) => {
                    self.record_nodes(&candidate.children);
                }
                RenderNode::Primitive(primitive) => self.record_primitive(primitive),
            }
        }
    }

    fn record_primitive(&mut self, primitive: &DrawPrimitive) {
        self.primitives += 1;

        match primitive {
            DrawPrimitive::Rect(..) => self.rects += 1,
            DrawPrimitive::RoundedRect(..) => self.rounded_rects += 1,
            DrawPrimitive::Border(..) => self.borders += 1,
            DrawPrimitive::BorderCorners(..) => self.border_corners += 1,
            DrawPrimitive::BorderEdges(..) => self.border_edges += 1,
            DrawPrimitive::Shadow(..) => self.shadows += 1,
            DrawPrimitive::InsetShadow(..) => self.inset_shadows += 1,
            DrawPrimitive::TextWithFont(_, _, text, ..) => {
                self.texts += 1;
                self.text_bytes += text.len();
            }
            DrawPrimitive::Gradient(..) => self.gradients += 1,
            DrawPrimitive::Image(..) => self.images += 1,
            DrawPrimitive::Video(..) => self.videos += 1,
            DrawPrimitive::ImageLoading(..) => self.image_loading += 1,
            DrawPrimitive::ImageFailed(..) => self.image_failed += 1,
        }
    }
}

impl fmt::Display for RenderSceneSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            concat!(
                "nodes={} primitives={} scopes={{shadow_passes={} clips={} relaxed_clips={} ",
                "clip_shapes={} transforms={} alphas={}}} draws={{rects={} rounded_rects={} ",
                "borders={} border_corners={} border_edges={} shadows={} inset_shadows={} ",
                "texts={} text_bytes={} gradients={} images={} videos={} image_loading={} ",
                "image_failed={}}}"
            ),
            self.nodes,
            self.primitives,
            self.shadow_passes,
            self.clips,
            self.relaxed_clips,
            self.clip_shapes,
            self.transforms,
            self.alphas,
            self.rects,
            self.rounded_rects,
            self.borders,
            self.border_corners,
            self.border_edges,
            self.shadows,
            self.inset_shadows,
            self.texts,
            self.text_bytes,
            self.gradients,
            self.images,
            self.videos,
            self.image_loading,
            self.image_failed
        )
    }
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
    CacheCandidate(RenderCacheCandidate),
    Primitive(DrawPrimitive),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderCacheCandidate {
    pub kind: RenderCacheCandidateKind,
    pub stable_id: u64,
    pub content_generation: u64,
    pub bounds: Rect,
    /// Local subtree content used for direct fallback and future payload
    /// preparation. Candidates must not hide shadow-escape semantics from a
    /// parent clip; tree rendering should only emit clean local subtrees here.
    pub children: Vec<RenderNode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderCacheCandidateKind {
    CleanSubtree,
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
