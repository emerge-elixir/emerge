use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::time::Instant;

use super::attrs::{
    Attrs, Background, BorderRadius, BorderWidth, BoxShadow, Color, Length, Padding,
};
use super::element::{ElementId, ElementTree};

#[derive(Clone, Debug)]
pub enum AnimationCurve {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

#[derive(Clone, Debug)]
pub enum AnimationRepeat {
    Once,
    Times(u32),
    Loop,
}

#[derive(Clone, Debug)]
pub struct AnimationSpec {
    pub keyframes: Vec<Attrs>,
    pub duration_ms: f64,
    pub curve: AnimationCurve,
    pub repeat: AnimationRepeat,
}

#[derive(Clone, Copy, Debug)]
pub struct AnimationRuntimeEntry {
    pub spec_hash: u64,
    pub started_at: Instant,
}

#[derive(Clone, Debug)]
pub struct EnterAnimationRuntimeEntry {
    pub spec: AnimationSpec,
    pub started_at: Instant,
}

#[derive(Clone, Debug, Default)]
pub struct AnimationRuntime {
    animate_entries: HashMap<ElementId, AnimationRuntimeEntry>,
    enter_entries: HashMap<ElementId, EnterAnimationRuntimeEntry>,
    last_seen_revision: u64,
}

#[derive(Clone, Debug, Default)]
pub struct AnimationSample {
    pub attrs: Attrs,
    pub active: bool,
}

impl AnimationRuntime {
    pub fn sync_with_tree(&mut self, tree: &ElementTree, started_at: Instant) {
        self.animate_entries.retain(|id, _| {
            tree.get(id)
                .is_some_and(|element| element.base_attrs.animate.is_some())
        });
        self.enter_entries.retain(|id, _| tree.get(id).is_some());

        for (id, element) in &tree.nodes {
            if tree.was_mounted_after(id, self.last_seen_revision) {
                self.animate_entries.remove(id);

                if let Some(spec) = element.base_attrs.animate_enter.as_ref() {
                    self.enter_entries.insert(
                        id.clone(),
                        EnterAnimationRuntimeEntry {
                            spec: spec.clone(),
                            started_at,
                        },
                    );
                } else {
                    self.enter_entries.remove(id);
                }
            }

            let enter_active = self.enter_entries.get(id).cloned().is_some_and(|entry| {
                let sample = sample_enter_animation_spec(&entry, Some(started_at), 1.0);
                if sample.active {
                    self.animate_entries.remove(id);
                    true
                } else {
                    self.enter_entries.remove(id);
                    false
                }
            });

            if enter_active {
                continue;
            }

            let Some(spec) = element.base_attrs.animate.as_ref() else {
                self.animate_entries.remove(id);
                continue;
            };
            let spec_hash = spec_fingerprint(spec);

            match self.animate_entries.get(id) {
                Some(entry) if entry.spec_hash == spec_hash => {}
                _ => {
                    self.animate_entries.insert(
                        id.clone(),
                        AnimationRuntimeEntry {
                            spec_hash,
                            started_at,
                        },
                    );
                }
            }
        }

        self.last_seen_revision = tree.revision();
    }

    pub fn is_empty(&self) -> bool {
        self.animate_entries.is_empty() && self.enter_entries.is_empty()
    }

    pub fn animate_entry(&self, id: &ElementId) -> Option<&AnimationRuntimeEntry> {
        self.animate_entries.get(id)
    }

    pub fn enter_entry(&self, id: &ElementId) -> Option<&EnterAnimationRuntimeEntry> {
        self.enter_entries.get(id)
    }
}

pub fn spec_fingerprint(spec: &AnimationSpec) -> u64 {
    let mut hasher = DefaultHasher::new();
    format!("{spec:?}").hash(&mut hasher);
    hasher.finish()
}

pub fn scale_animation_spec(spec: &AnimationSpec, scale: f64) -> AnimationSpec {
    AnimationSpec {
        keyframes: spec
            .keyframes
            .iter()
            .map(|keyframe| scale_animation_keyframe(keyframe, scale))
            .collect(),
        duration_ms: spec.duration_ms,
        curve: spec.curve.clone(),
        repeat: spec.repeat.clone(),
    }
}

pub fn apply_animation_overlays(
    tree: &mut ElementTree,
    runtime: Option<&AnimationRuntime>,
    sample_time: Option<Instant>,
    scale: f32,
) -> bool {
    tree.nodes.values_mut().fold(false, |active_any, element| {
        if let Some(sample) = runtime
            .and_then(|state| state.enter_entry(&element.id))
            .map(|entry| sample_enter_animation_spec(entry, sample_time, scale as f64))
            .filter(|sample| sample.active)
        {
            apply_sample_attrs(&mut element.attrs, &sample.attrs);
            return active_any || sample.active;
        }

        let Some(spec) = element.attrs.animate.as_ref() else {
            return active_any;
        };

        let sample = sample_animation_spec(
            spec,
            runtime.and_then(|state| state.animate_entry(&element.id)),
            sample_time,
        );
        apply_sample_attrs(&mut element.attrs, &sample.attrs);
        active_any || sample.active
    })
}

fn sample_enter_animation_spec(
    entry: &EnterAnimationRuntimeEntry,
    sample_time: Option<Instant>,
    scale: f64,
) -> AnimationSample {
    let scaled_spec = scale_animation_spec(&entry.spec, scale);
    let runtime_entry = AnimationRuntimeEntry {
        spec_hash: 0,
        started_at: entry.started_at,
    };

    sample_animation_spec(&scaled_spec, Some(&runtime_entry), sample_time)
}

pub fn sample_animation_spec(
    spec: &AnimationSpec,
    entry: Option<&AnimationRuntimeEntry>,
    sample_time: Option<Instant>,
) -> AnimationSample {
    if spec.keyframes.is_empty() {
        return AnimationSample::default();
    }

    if spec.keyframes.len() == 1 {
        return AnimationSample {
            attrs: spec.keyframes[0].clone(),
            active: false,
        };
    }

    let elapsed_ms = entry
        .zip(sample_time)
        .map(|(entry, sample_time)| {
            if sample_time > entry.started_at {
                sample_time.duration_since(entry.started_at).as_secs_f64() * 1000.0
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);

    let duration_ms = spec.duration_ms.max(f64::EPSILON);

    let (local_ms, active) = match spec.repeat {
        AnimationRepeat::Once => {
            if elapsed_ms >= duration_ms {
                (duration_ms, false)
            } else {
                (elapsed_ms, true)
            }
        }
        AnimationRepeat::Times(count) => {
            let total_ms = duration_ms * count.max(1) as f64;
            if elapsed_ms >= total_ms {
                (duration_ms, false)
            } else {
                (elapsed_ms % duration_ms, true)
            }
        }
        AnimationRepeat::Loop => (elapsed_ms % duration_ms, true),
    };

    if !active && local_ms >= duration_ms {
        return AnimationSample {
            attrs: spec.keyframes.last().cloned().unwrap_or_default(),
            active,
        };
    }

    let segments = spec.keyframes.len() - 1;
    let normalized = (local_ms / duration_ms).clamp(0.0, 1.0);
    let segment_position = normalized * segments as f64;
    let mut segment_index = segment_position.floor() as usize;
    let mut segment_t = segment_position - segment_index as f64;

    if segment_index >= segments {
        segment_index = segments - 1;
        segment_t = 1.0;
    }

    let eased_t = apply_curve(&spec.curve, segment_t);
    let attrs = interpolate_attrs(
        &spec.keyframes[segment_index],
        &spec.keyframes[segment_index + 1],
        eased_t,
    );

    AnimationSample { attrs, active }
}

fn apply_curve(curve: &AnimationCurve, t: f64) -> f64 {
    match curve {
        AnimationCurve::Linear => t,
        AnimationCurve::EaseIn => t * t * t,
        AnimationCurve::EaseOut => 1.0 - (1.0 - t).powi(3),
        AnimationCurve::EaseInOut => {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
            }
        }
    }
}

fn scale_animation_keyframe(attrs: &Attrs, scale: f64) -> Attrs {
    Attrs {
        width: attrs.width.as_ref().map(|value| scale_length(value, scale)),
        height: attrs
            .height
            .as_ref()
            .map(|value| scale_length(value, scale)),
        padding: attrs
            .padding
            .as_ref()
            .map(|value| scale_padding(value, scale)),
        spacing: attrs.spacing.map(|value| value * scale),
        spacing_x: attrs.spacing_x.map(|value| value * scale),
        spacing_y: attrs.spacing_y.map(|value| value * scale),
        background: attrs.background.clone(),
        border_radius: attrs
            .border_radius
            .as_ref()
            .map(|value| scale_border_radius(value, scale)),
        border_width: attrs
            .border_width
            .as_ref()
            .map(|value| scale_border_width(value, scale)),
        border_color: attrs.border_color.clone(),
        box_shadows: attrs.box_shadows.as_ref().map(|shadows| {
            shadows
                .iter()
                .map(|shadow| BoxShadow {
                    offset_x: shadow.offset_x * scale,
                    offset_y: shadow.offset_y * scale,
                    blur: shadow.blur * scale,
                    size: shadow.size * scale,
                    color: shadow.color.clone(),
                    inset: shadow.inset,
                })
                .collect()
        }),
        font_size: attrs.font_size.map(|value| value * scale),
        font_color: attrs.font_color.clone(),
        font_letter_spacing: attrs.font_letter_spacing.map(|value| value * scale),
        font_word_spacing: attrs.font_word_spacing.map(|value| value * scale),
        svg_color: attrs.svg_color.clone(),
        move_x: attrs.move_x.map(|value| value * scale),
        move_y: attrs.move_y.map(|value| value * scale),
        rotate: attrs.rotate,
        scale: attrs.scale,
        alpha: attrs.alpha,
        ..Attrs::default()
    }
}

fn interpolate_attrs(from: &Attrs, to: &Attrs, t: f64) -> Attrs {
    Attrs {
        width: interpolate_opt_ref(
            from.width.as_ref(),
            to.width.as_ref(),
            t,
            interpolate_length,
        ),
        height: interpolate_opt_ref(
            from.height.as_ref(),
            to.height.as_ref(),
            t,
            interpolate_length,
        ),
        padding: interpolate_opt_ref(
            from.padding.as_ref(),
            to.padding.as_ref(),
            t,
            interpolate_padding,
        ),
        spacing: interpolate_opt_copy(from.spacing, to.spacing, t, lerp_f64),
        spacing_x: interpolate_opt_copy(from.spacing_x, to.spacing_x, t, lerp_f64),
        spacing_y: interpolate_opt_copy(from.spacing_y, to.spacing_y, t, lerp_f64),
        background: interpolate_opt_ref(
            from.background.as_ref(),
            to.background.as_ref(),
            t,
            interpolate_background,
        ),
        border_radius: interpolate_opt_ref(
            from.border_radius.as_ref(),
            to.border_radius.as_ref(),
            t,
            interpolate_border_radius,
        ),
        border_width: interpolate_opt_ref(
            from.border_width.as_ref(),
            to.border_width.as_ref(),
            t,
            interpolate_border_width,
        ),
        border_color: interpolate_opt_ref(
            from.border_color.as_ref(),
            to.border_color.as_ref(),
            t,
            interpolate_color,
        ),
        box_shadows: interpolate_opt_ref(
            from.box_shadows.as_ref(),
            to.box_shadows.as_ref(),
            t,
            interpolate_box_shadows,
        ),
        font_size: interpolate_opt_copy(from.font_size, to.font_size, t, lerp_f64),
        font_color: interpolate_opt_ref(
            from.font_color.as_ref(),
            to.font_color.as_ref(),
            t,
            interpolate_color,
        ),
        font_letter_spacing: interpolate_opt_copy(
            from.font_letter_spacing,
            to.font_letter_spacing,
            t,
            lerp_f64,
        ),
        font_word_spacing: interpolate_opt_copy(
            from.font_word_spacing,
            to.font_word_spacing,
            t,
            lerp_f64,
        ),
        svg_color: interpolate_opt_ref(
            from.svg_color.as_ref(),
            to.svg_color.as_ref(),
            t,
            interpolate_color,
        ),
        move_x: interpolate_opt_copy(from.move_x, to.move_x, t, lerp_f64),
        move_y: interpolate_opt_copy(from.move_y, to.move_y, t, lerp_f64),
        rotate: interpolate_opt_copy(from.rotate, to.rotate, t, lerp_f64),
        scale: interpolate_opt_copy(from.scale, to.scale, t, lerp_f64),
        alpha: interpolate_opt_copy(from.alpha, to.alpha, t, lerp_f64),
        ..Attrs::default()
    }
}

fn apply_sample_attrs(attrs: &mut Attrs, sample: &Attrs) {
    if let Some(value) = sample.width.clone() {
        attrs.width = Some(value);
    }
    if let Some(value) = sample.height.clone() {
        attrs.height = Some(value);
    }
    if let Some(value) = sample.padding.clone() {
        attrs.padding = Some(value);
    }
    if let Some(value) = sample.spacing {
        attrs.spacing = Some(value);
    }
    if let Some(value) = sample.spacing_x {
        attrs.spacing_x = Some(value);
    }
    if let Some(value) = sample.spacing_y {
        attrs.spacing_y = Some(value);
    }
    if let Some(value) = sample.background.clone() {
        attrs.background = Some(value);
    }
    if let Some(value) = sample.border_radius.clone() {
        attrs.border_radius = Some(value);
    }
    if let Some(value) = sample.border_width.clone() {
        attrs.border_width = Some(value);
    }
    if let Some(value) = sample.border_color.clone() {
        attrs.border_color = Some(value);
    }
    if let Some(value) = sample.box_shadows.clone() {
        attrs.box_shadows = Some(value);
    }
    if let Some(value) = sample.font_size {
        attrs.font_size = Some(value);
    }
    if let Some(value) = sample.font_color.clone() {
        attrs.font_color = Some(value);
    }
    if let Some(value) = sample.font_letter_spacing {
        attrs.font_letter_spacing = Some(value);
    }
    if let Some(value) = sample.font_word_spacing {
        attrs.font_word_spacing = Some(value);
    }
    if let Some(value) = sample.svg_color.clone() {
        attrs.svg_color = Some(value);
    }
    if let Some(value) = sample.move_x {
        attrs.move_x = Some(value);
    }
    if let Some(value) = sample.move_y {
        attrs.move_y = Some(value);
    }
    if let Some(value) = sample.rotate {
        attrs.rotate = Some(value);
    }
    if let Some(value) = sample.scale {
        attrs.scale = Some(value);
    }
    if let Some(value) = sample.alpha {
        attrs.alpha = Some(value);
    }
}

fn interpolate_opt_copy<T: Copy, U, F>(
    from: Option<T>,
    to: Option<T>,
    t: f64,
    interpolate: F,
) -> Option<U>
where
    F: Fn(T, T, f64) -> U,
{
    match (from, to) {
        (Some(from), Some(to)) => Some(interpolate(from, to, t)),
        _ => None,
    }
}

fn interpolate_opt_ref<T, U, F>(
    from: Option<&T>,
    to: Option<&T>,
    t: f64,
    interpolate: F,
) -> Option<U>
where
    F: Fn(&T, &T, f64) -> U,
{
    match (from, to) {
        (Some(from), Some(to)) => Some(interpolate(from, to, t)),
        _ => None,
    }
}

fn lerp_f64(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

fn interpolate_length(from: &Length, to: &Length, t: f64) -> Length {
    match (from, to) {
        (Length::Fill, Length::Fill) => Length::Fill,
        (Length::Content, Length::Content) => Length::Content,
        (Length::Px(from), Length::Px(to)) => Length::Px(lerp_f64(*from, *to, t)),
        (Length::FillWeighted(from), Length::FillWeighted(to)) => {
            Length::FillWeighted(lerp_f64(*from, *to, t))
        }
        (Length::Minimum(from_min, from_inner), Length::Minimum(to_min, to_inner)) => {
            Length::Minimum(
                lerp_f64(*from_min, *to_min, t),
                Box::new(interpolate_length(from_inner, to_inner, t)),
            )
        }
        (Length::Maximum(from_max, from_inner), Length::Maximum(to_max, to_inner)) => {
            Length::Maximum(
                lerp_f64(*from_max, *to_max, t),
                Box::new(interpolate_length(from_inner, to_inner, t)),
            )
        }
        _ => from.clone(),
    }
}

fn interpolate_padding(from: &Padding, to: &Padding, t: f64) -> Padding {
    match (from, to) {
        (Padding::Uniform(from), Padding::Uniform(to)) => Padding::Uniform(lerp_f64(*from, *to, t)),
        (
            Padding::Sides {
                top: from_top,
                right: from_right,
                bottom: from_bottom,
                left: from_left,
            },
            Padding::Sides {
                top: to_top,
                right: to_right,
                bottom: to_bottom,
                left: to_left,
            },
        ) => Padding::Sides {
            top: lerp_f64(*from_top, *to_top, t),
            right: lerp_f64(*from_right, *to_right, t),
            bottom: lerp_f64(*from_bottom, *to_bottom, t),
            left: lerp_f64(*from_left, *to_left, t),
        },
        _ => from.clone(),
    }
}

fn interpolate_border_radius(from: &BorderRadius, to: &BorderRadius, t: f64) -> BorderRadius {
    match (from, to) {
        (BorderRadius::Uniform(from), BorderRadius::Uniform(to)) => {
            BorderRadius::Uniform(lerp_f64(*from, *to, t))
        }
        (
            BorderRadius::Corners {
                tl: from_tl,
                tr: from_tr,
                br: from_br,
                bl: from_bl,
            },
            BorderRadius::Corners {
                tl: to_tl,
                tr: to_tr,
                br: to_br,
                bl: to_bl,
            },
        ) => BorderRadius::Corners {
            tl: lerp_f64(*from_tl, *to_tl, t),
            tr: lerp_f64(*from_tr, *to_tr, t),
            br: lerp_f64(*from_br, *to_br, t),
            bl: lerp_f64(*from_bl, *to_bl, t),
        },
        _ => from.clone(),
    }
}

fn interpolate_border_width(from: &BorderWidth, to: &BorderWidth, t: f64) -> BorderWidth {
    match (from, to) {
        (BorderWidth::Uniform(from), BorderWidth::Uniform(to)) => {
            BorderWidth::Uniform(lerp_f64(*from, *to, t))
        }
        (
            BorderWidth::Sides {
                top: from_top,
                right: from_right,
                bottom: from_bottom,
                left: from_left,
            },
            BorderWidth::Sides {
                top: to_top,
                right: to_right,
                bottom: to_bottom,
                left: to_left,
            },
        ) => BorderWidth::Sides {
            top: lerp_f64(*from_top, *to_top, t),
            right: lerp_f64(*from_right, *to_right, t),
            bottom: lerp_f64(*from_bottom, *to_bottom, t),
            left: lerp_f64(*from_left, *to_left, t),
        },
        _ => from.clone(),
    }
}

fn interpolate_background(from: &Background, to: &Background, t: f64) -> Background {
    match (from, to) {
        (Background::Color(from), Background::Color(to)) => {
            Background::Color(interpolate_color(from, to, t))
        }
        (
            Background::Gradient {
                from: from_start,
                to: from_end,
                angle: from_angle,
            },
            Background::Gradient {
                from: to_start,
                to: to_end,
                angle: to_angle,
            },
        ) => Background::Gradient {
            from: interpolate_color(from_start, to_start, t),
            to: interpolate_color(from_end, to_end, t),
            angle: lerp_f64(*from_angle, *to_angle, t),
        },
        (
            Background::Image {
                source: from_source,
                fit: from_fit,
            },
            Background::Image {
                source: to_source,
                fit: to_fit,
            },
        ) if from_source == to_source && from_fit == to_fit => from.clone(),
        _ => from.clone(),
    }
}

fn interpolate_box_shadows(from: &Vec<BoxShadow>, to: &Vec<BoxShadow>, t: f64) -> Vec<BoxShadow> {
    from.iter()
        .zip(to.iter())
        .map(|(from, to)| BoxShadow {
            offset_x: lerp_f64(from.offset_x, to.offset_x, t),
            offset_y: lerp_f64(from.offset_y, to.offset_y, t),
            blur: lerp_f64(from.blur, to.blur, t),
            size: lerp_f64(from.size, to.size, t),
            color: interpolate_color(&from.color, &to.color, t),
            inset: from.inset,
        })
        .collect()
}

fn interpolate_color(from: &Color, to: &Color, t: f64) -> Color {
    let (from_r, from_g, from_b, from_a) = color_to_rgba(from);
    let (to_r, to_g, to_b, to_a) = color_to_rgba(to);

    Color::Rgba {
        r: lerp_channel(from_r, to_r, t),
        g: lerp_channel(from_g, to_g, t),
        b: lerp_channel(from_b, to_b, t),
        a: lerp_channel(from_a, to_a, t),
    }
}

fn color_to_rgba(color: &Color) -> (u8, u8, u8, u8) {
    match color {
        Color::Rgb { r, g, b } => (*r, *g, *b, 255),
        Color::Rgba { r, g, b, a } => (*r, *g, *b, *a),
        Color::Named(name) => named_color_rgba(name),
    }
}

fn named_color_rgba(name: &str) -> (u8, u8, u8, u8) {
    match name {
        "white" => (255, 255, 255, 255),
        "black" => (0, 0, 0, 255),
        "red" => (255, 0, 0, 255),
        "green" => (0, 255, 0, 255),
        "blue" => (0, 0, 255, 255),
        "cyan" => (0, 255, 255, 255),
        "magenta" => (255, 0, 255, 255),
        "yellow" => (255, 255, 0, 255),
        "orange" => (255, 165, 0, 255),
        "purple" => (128, 0, 128, 255),
        "pink" => (255, 192, 203, 255),
        "gray" | "grey" => (128, 128, 128, 255),
        "navy" => (0, 0, 128, 255),
        "teal" => (0, 128, 128, 255),
        _ => (255, 255, 255, 255),
    }
}

fn lerp_channel(from: u8, to: u8, t: f64) -> u8 {
    lerp_f64(from as f64, to as f64, t)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn scale_length(value: &Length, scale: f64) -> Length {
    match value {
        Length::Fill => Length::Fill,
        Length::Content => Length::Content,
        Length::Px(value) => Length::Px(value * scale),
        Length::FillWeighted(value) => Length::FillWeighted(*value),
        Length::Minimum(min, inner) => {
            Length::Minimum(min * scale, Box::new(scale_length(inner, scale)))
        }
        Length::Maximum(max, inner) => {
            Length::Maximum(max * scale, Box::new(scale_length(inner, scale)))
        }
    }
}

fn scale_padding(value: &Padding, scale: f64) -> Padding {
    match value {
        Padding::Uniform(value) => Padding::Uniform(value * scale),
        Padding::Sides {
            top,
            right,
            bottom,
            left,
        } => Padding::Sides {
            top: top * scale,
            right: right * scale,
            bottom: bottom * scale,
            left: left * scale,
        },
    }
}

fn scale_border_radius(value: &BorderRadius, scale: f64) -> BorderRadius {
    match value {
        BorderRadius::Uniform(value) => BorderRadius::Uniform(value * scale),
        BorderRadius::Corners { tl, tr, br, bl } => BorderRadius::Corners {
            tl: tl * scale,
            tr: tr * scale,
            br: br * scale,
            bl: bl * scale,
        },
    }
}

fn scale_border_width(value: &BorderWidth, scale: f64) -> BorderWidth {
    match value {
        BorderWidth::Uniform(value) => BorderWidth::Uniform(value * scale),
        BorderWidth::Sides {
            top,
            right,
            bottom,
            left,
        } => BorderWidth::Sides {
            top: top * scale,
            right: right * scale,
            bottom: bottom * scale,
            left: left * scale,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::element::{Element, ElementKind};

    fn move_x_spec(
        from_x: f64,
        to_x: f64,
        duration_ms: f64,
        repeat: AnimationRepeat,
    ) -> AnimationSpec {
        let mut from = Attrs::default();
        from.move_x = Some(from_x);

        let mut to = Attrs::default();
        to.move_x = Some(to_x);

        AnimationSpec {
            keyframes: vec![from, to],
            duration_ms,
            curve: AnimationCurve::Linear,
            repeat,
        }
    }

    fn alpha_spec(from_alpha: f64, to_alpha: f64, duration_ms: f64) -> AnimationSpec {
        let mut from = Attrs::default();
        from.alpha = Some(from_alpha);

        let mut to = Attrs::default();
        to.alpha = Some(to_alpha);

        AnimationSpec {
            keyframes: vec![from, to],
            duration_ms,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        }
    }

    fn tree_with_element(
        attrs: Attrs,
        tree_revision: u64,
        mounted_at_revision: u64,
    ) -> (ElementTree, ElementId) {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.mounted_at_revision = mounted_at_revision;

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);
        tree.set_revision(tree_revision);
        (tree, id)
    }

    #[test]
    fn sample_animation_spec_loops_with_time() {
        let mut from = Attrs::default();
        from.move_x = Some(0.0);

        let mut to = Attrs::default();
        to.move_x = Some(10.0);

        let spec = AnimationSpec {
            keyframes: vec![from, to],
            duration_ms: 100.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Loop,
        };
        let start = Instant::now();
        let entry = AnimationRuntimeEntry {
            spec_hash: spec_fingerprint(&spec),
            started_at: start,
        };

        let sample = sample_animation_spec(
            &spec,
            Some(&entry),
            Some(start + std::time::Duration::from_millis(150)),
        );

        assert_eq!(sample.attrs.move_x, Some(5.0));
        assert!(sample.active);
    }

    #[test]
    fn sample_animation_spec_clamps_once_to_last_keyframe() {
        let mut from = Attrs::default();
        from.alpha = Some(0.0);

        let mut to = Attrs::default();
        to.alpha = Some(1.0);

        let spec = AnimationSpec {
            keyframes: vec![from, to],
            duration_ms: 100.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        };
        let start = Instant::now();
        let entry = AnimationRuntimeEntry {
            spec_hash: spec_fingerprint(&spec),
            started_at: start,
        };

        let sample = sample_animation_spec(
            &spec,
            Some(&entry),
            Some(start + std::time::Duration::from_millis(250)),
        );

        assert_eq!(sample.attrs.alpha, Some(1.0));
        assert!(!sample.active);
    }

    #[test]
    fn sync_with_tree_starts_enter_animation_for_newly_mounted_nodes() {
        let mut attrs = Attrs::default();
        attrs.animate_enter = Some(alpha_spec(0.0, 1.0, 100.0));
        let (tree, id) = tree_with_element(attrs, 1, 1);
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, start);

        assert!(runtime.enter_entry(&id).is_some());
        assert!(runtime.animate_entry(&id).is_none());
        assert_eq!(runtime.last_seen_revision, 1);
    }

    #[test]
    fn sync_with_tree_does_not_start_enter_when_attr_is_added_later() {
        let (mut tree, id) = tree_with_element(Attrs::default(), 1, 1);
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, start);

        tree.set_revision(2);
        let element = tree.get_mut(&id).expect("element should exist");
        element.base_attrs.animate_enter = Some(alpha_spec(0.0, 1.0, 100.0));
        element.attrs.animate_enter = element.base_attrs.animate_enter.clone();

        runtime.sync_with_tree(&tree, start + std::time::Duration::from_millis(16));

        assert!(runtime.enter_entry(&id).is_none());
    }

    #[test]
    fn enter_animation_captures_spec_at_mount_time() {
        let mut attrs = Attrs::default();
        attrs.animate_enter = Some(move_x_spec(0.0, 100.0, 100.0, AnimationRepeat::Once));
        let (mut tree, id) = tree_with_element(attrs, 1, 1);
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, start);

        let element = tree.get_mut(&id).expect("element should exist");
        element.base_attrs.animate_enter =
            Some(move_x_spec(0.0, 200.0, 100.0, AnimationRepeat::Once));
        element.attrs.animate_enter = element.base_attrs.animate_enter.clone();

        let sample = sample_enter_animation_spec(
            runtime.enter_entry(&id).expect("enter entry should exist"),
            Some(start + std::time::Duration::from_millis(50)),
            1.0,
        );

        assert_eq!(sample.attrs.move_x, Some(50.0));
    }

    #[test]
    fn completed_enter_hands_off_to_base_attrs_when_no_animate_is_present() {
        let mut attrs = Attrs::default();
        attrs.animate_enter = Some(move_x_spec(0.0, 100.0, 100.0, AnimationRepeat::Once));
        let (mut tree, id) = tree_with_element(attrs, 1, 1);
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, start);
        runtime.sync_with_tree(&tree, start + std::time::Duration::from_millis(150));

        assert!(runtime.enter_entry(&id).is_none());
        assert!(runtime.animate_entry(&id).is_none());

        let active = apply_animation_overlays(
            &mut tree,
            Some(&runtime),
            Some(start + std::time::Duration::from_millis(150)),
            1.0,
        );

        assert!(!active);
        assert_eq!(tree.get(&id).unwrap().attrs.move_x, None);
    }

    #[test]
    fn completed_enter_starts_regular_animation_from_zero_progress() {
        let mut attrs = Attrs::default();
        attrs.animate_enter = Some(alpha_spec(0.0, 1.0, 100.0));
        attrs.animate = Some(move_x_spec(10.0, 30.0, 100.0, AnimationRepeat::Loop));
        let (mut tree, id) = tree_with_element(attrs, 1, 1);
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, start);
        runtime.sync_with_tree(&tree, start + std::time::Duration::from_millis(150));

        assert!(runtime.enter_entry(&id).is_none());
        assert!(runtime.animate_entry(&id).is_some());

        let active = apply_animation_overlays(
            &mut tree,
            Some(&runtime),
            Some(start + std::time::Duration::from_millis(150)),
            1.0,
        );

        assert!(active);
        assert_eq!(tree.get(&id).unwrap().attrs.move_x, Some(10.0));
    }
}
