use super::geometry::{ClipShape, CornerRadii, Rect};
use super::render::{
    HostClipDescriptor, InheritedClipMode, RenderItem, RenderScope, ScopeTransform,
};
use crate::renderer::DrawCmd;

pub fn lower_render_scope(root: &RenderScope, scene_bounds: Rect) -> Vec<DrawCmd> {
    let mut commands = Vec::new();
    let mut ctx = LowerContext {
        scene_bounds,
        ..LowerContext::default()
    };
    lower_scope(root, &mut commands, &mut ctx);
    commands
}

#[derive(Clone, Debug, Default)]
struct LowerContext {
    active_host_clips: Vec<HostClipDescriptor>,
    scene_bounds: Rect,
}

fn lower_scope(scope: &RenderScope, commands: &mut Vec<DrawCmd>, ctx: &mut LowerContext) {
    let transform_state = push_scope_transform(commands, scope.transform.as_ref());
    let suspended = suspend_inherited_clips(scope.inherited_clip_mode, commands, ctx);

    let pushed_local_clip = if let Some(local_clip) = scope.local_clip {
        push_clip_shape(commands, &local_clip);
        true
    } else {
        false
    };

    let pushed_host_clip = if let Some(host_clip) = scope.host_clip {
        push_clip_shape(commands, &host_clip.clip);
        ctx.active_host_clips.push(host_clip);
        true
    } else {
        false
    };

    for item in &scope.items {
        match item {
            RenderItem::Draw(cmd) => commands.push(cmd.clone()),
            RenderItem::Scope(child_scope) => lower_scope(child_scope, commands, ctx),
        }
    }

    if pushed_host_clip {
        let _ = ctx.active_host_clips.pop();
        commands.push(DrawCmd::PopClip);
    }

    if pushed_local_clip {
        commands.push(DrawCmd::PopClip);
    }

    restore_inherited_clips(suspended, commands, ctx);
    pop_scope_transform(commands, transform_state);
}

#[derive(Clone, Debug, Default)]
struct LowerTransformState {
    active: bool,
    has_alpha_layer: bool,
}

fn push_scope_transform(
    commands: &mut Vec<DrawCmd>,
    transform: Option<&ScopeTransform>,
) -> LowerTransformState {
    let Some(transform) = transform else {
        return LowerTransformState::default();
    };

    let has_translation = transform.move_x != 0.0 || transform.move_y != 0.0;
    let has_rotation = transform.rotate != 0.0;
    let has_scale = (transform.scale - 1.0).abs() > f32::EPSILON;
    let has_alpha = transform.alpha < 1.0;

    if !(has_translation || has_rotation || has_scale || has_alpha) {
        return LowerTransformState::default();
    }

    commands.push(DrawCmd::Save);

    if has_translation {
        commands.push(DrawCmd::Translate(transform.move_x, transform.move_y));
    }

    if has_rotation || has_scale {
        commands.push(DrawCmd::Translate(transform.center_x, transform.center_y));
        if has_rotation {
            commands.push(DrawCmd::Rotate(transform.rotate));
        }
        if has_scale {
            commands.push(DrawCmd::Scale(transform.scale, transform.scale));
        }
        commands.push(DrawCmd::Translate(-transform.center_x, -transform.center_y));
    }

    if has_alpha {
        commands.push(DrawCmd::SaveLayerAlpha(transform.alpha));
    }

    LowerTransformState {
        active: true,
        has_alpha_layer: has_alpha,
    }
}

fn pop_scope_transform(commands: &mut Vec<DrawCmd>, state: LowerTransformState) {
    if !state.active {
        return;
    }

    if state.has_alpha_layer {
        commands.push(DrawCmd::Restore);
    }
    commands.push(DrawCmd::Restore);
}

#[derive(Clone, Debug, Default)]
struct SuspendedClips {
    host_clips: Vec<HostClipDescriptor>,
    shadow_clip_count: usize,
}

fn suspend_inherited_clips(
    mode: InheritedClipMode,
    commands: &mut Vec<DrawCmd>,
    ctx: &mut LowerContext,
) -> SuspendedClips {
    match mode {
        InheritedClipMode::Normal => SuspendedClips::default(),
        InheritedClipMode::None | InheritedClipMode::ShadowAxes => {
            let host_clips = std::mem::take(&mut ctx.active_host_clips);
            for _ in &host_clips {
                commands.push(DrawCmd::PopClip);
            }

            let shadow_clip_count = if matches!(mode, InheritedClipMode::ShadowAxes) {
                push_shadow_axis_clips(commands, &host_clips, ctx.scene_bounds)
            } else {
                0
            };

            SuspendedClips {
                host_clips,
                shadow_clip_count,
            }
        }
    }
}

fn restore_inherited_clips(
    suspended: SuspendedClips,
    commands: &mut Vec<DrawCmd>,
    ctx: &mut LowerContext,
) {
    for _ in 0..suspended.shadow_clip_count {
        commands.push(DrawCmd::PopClip);
    }

    for clip in &suspended.host_clips {
        push_clip_shape(commands, &clip.clip);
    }

    if !suspended.host_clips.is_empty() {
        ctx.active_host_clips = suspended.host_clips;
    }
}

fn push_shadow_axis_clips(
    commands: &mut Vec<DrawCmd>,
    clips: &[HostClipDescriptor],
    scene_bounds: Rect,
) -> usize {
    let mut count = 0;

    for clip in clips {
        match (clip.scroll_x, clip.scroll_y) {
            (false, false) => {}
            (true, true) => {
                push_clip_shape(commands, &clip.clip);
                count += 1;
            }
            (true, false) => {
                commands.push(DrawCmd::PushClip(
                    clip.clip.rect.x,
                    scene_bounds.y,
                    clip.clip.rect.width,
                    scene_bounds.height,
                ));
                count += 1;
            }
            (false, true) => {
                commands.push(DrawCmd::PushClip(
                    scene_bounds.x,
                    clip.clip.rect.y,
                    scene_bounds.width,
                    clip.clip.rect.height,
                ));
                count += 1;
            }
        }
    }

    count
}

fn push_clip_shape(commands: &mut Vec<DrawCmd>, clip: &ClipShape) {
    match clip.radii {
        None => commands.push(DrawCmd::PushClip(
            clip.rect.x,
            clip.rect.y,
            clip.rect.width,
            clip.rect.height,
        )),
        Some(CornerRadii { tl, tr, br, bl })
            if tl > 0.0
                && (tl - tr).abs() < f32::EPSILON
                && (tl - br).abs() < f32::EPSILON
                && (tl - bl).abs() < f32::EPSILON =>
        {
            commands.push(DrawCmd::PushClipRounded(
                clip.rect.x,
                clip.rect.y,
                clip.rect.width,
                clip.rect.height,
                tl,
            ));
        }
        Some(CornerRadii { tl, tr, br, bl }) => {
            commands.push(DrawCmd::PushClipRoundedCorners(
                clip.rect.x,
                clip.rect.y,
                clip.rect.width,
                clip.rect.height,
                tl,
                tr,
                br,
                bl,
            ));
        }
    }
}
