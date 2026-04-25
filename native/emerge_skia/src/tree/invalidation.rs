use super::attrs::{Attrs, MouseOverAttrs};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum TreeInvalidation {
    #[default]
    None,
    Registry,
    Paint,
    Resolve,
    Measure,
    Structure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshDecision {
    Skip,
    UseCachedRebuild,
    RefreshOnly,
    Recompute,
}

impl TreeInvalidation {
    pub fn add(&mut self, other: Self) {
        *self = self.join(other);
    }

    pub fn join(self, other: Self) -> Self {
        self.max(other)
    }

    pub fn when_changed(changed: bool, invalidation: Self) -> Self {
        if changed { invalidation } else { Self::None }
    }

    pub fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    pub fn is_dirty(self) -> bool {
        !self.is_none()
    }

    pub fn can_refresh_only(self) -> bool {
        matches!(self, Self::Registry | Self::Paint)
    }

    pub fn requires_recompute(self) -> bool {
        matches!(self, Self::Resolve | Self::Measure | Self::Structure)
    }

    pub fn requires_measure(self) -> bool {
        matches!(self, Self::Measure | Self::Structure)
    }
}

pub fn decide_refresh_action(
    invalidation: TreeInvalidation,
    registry_requested: bool,
    has_cached_rebuild: bool,
    has_active_animations: bool,
) -> RefreshDecision {
    if invalidation.requires_recompute()
        || (invalidation.can_refresh_only() && has_active_animations)
    {
        RefreshDecision::Recompute
    } else if invalidation.can_refresh_only() {
        RefreshDecision::RefreshOnly
    } else if registry_requested && has_cached_rebuild {
        RefreshDecision::UseCachedRebuild
    } else if registry_requested {
        RefreshDecision::Recompute
    } else {
        RefreshDecision::Skip
    }
}

pub fn classify_attrs_change(before: &Attrs, after: &Attrs) -> TreeInvalidation {
    let mut invalidation = TreeInvalidation::None;

    if before.on_click != after.on_click
        || before.on_mouse_down != after.on_mouse_down
        || before.on_mouse_up != after.on_mouse_up
        || before.on_mouse_enter != after.on_mouse_enter
        || before.on_mouse_leave != after.on_mouse_leave
        || before.on_mouse_move != after.on_mouse_move
        || before.on_press != after.on_press
        || before.on_swipe_up != after.on_swipe_up
        || before.on_swipe_down != after.on_swipe_down
        || before.on_swipe_left != after.on_swipe_left
        || before.on_swipe_right != after.on_swipe_right
        || before.on_change != after.on_change
        || before.on_focus != after.on_focus
        || before.on_blur != after.on_blur
        || before.focus_on_mount != after.focus_on_mount
        || before.on_key_down != after.on_key_down
        || before.on_key_up != after.on_key_up
        || before.on_key_press != after.on_key_press
        || before.virtual_key != after.virtual_key
    {
        invalidation.add(TreeInvalidation::Registry);
    }

    if before.background != after.background
        || before.border_radius != after.border_radius
        || before.border_style != after.border_style
        || before.border_color != after.border_color
        || before.box_shadows != after.box_shadows
        || before.font_color != after.font_color
        || before.svg_color != after.svg_color
        || before.font_underline != after.font_underline
        || before.font_strike != after.font_strike
        || before.video_target != after.video_target
        || before.move_x != after.move_x
        || before.move_y != after.move_y
        || before.rotate != after.rotate
        || before.scale != after.scale
        || before.alpha != after.alpha
    {
        invalidation.add(TreeInvalidation::Paint);
    }

    if before.align_x != after.align_x || before.align_y != after.align_y {
        invalidation.add(TreeInvalidation::Resolve);
    }

    if before.width != after.width
        || before.height != after.height
        || before.padding != after.padding
        || before.spacing != after.spacing
        || before.spacing_x != after.spacing_x
        || before.spacing_y != after.spacing_y
        || before.scrollbar_y != after.scrollbar_y
        || before.scrollbar_x != after.scrollbar_x
        || before.ghost_scrollbar_y != after.ghost_scrollbar_y
        || before.ghost_scrollbar_x != after.ghost_scrollbar_x
        || before.scroll_x != after.scroll_x
        || before.scroll_y != after.scroll_y
        || before.clip_nearby != after.clip_nearby
        || before.border_width != after.border_width
        || before.font_size != after.font_size
        || before.font != after.font
        || before.font_weight != after.font_weight
        || before.font_style != after.font_style
        || before.font_letter_spacing != after.font_letter_spacing
        || before.font_word_spacing != after.font_word_spacing
        || before.image_src != after.image_src
        || before.image_fit != after.image_fit
        || before.image_size != after.image_size
        || before.text_align != after.text_align
        || before.content != after.content
        || before.snap_layout != after.snap_layout
        || before.snap_text_metrics != after.snap_text_metrics
        || before.space_evenly != after.space_evenly
        || before.animate.is_some()
        || after.animate.is_some()
        || before.animate_enter.is_some()
        || after.animate_enter.is_some()
        || before.animate_exit.is_some()
        || after.animate_exit.is_some()
    {
        invalidation.add(TreeInvalidation::Measure);
    }

    if before.mouse_over != after.mouse_over {
        invalidation.add(classify_optional_interaction_style_change(
            before.mouse_over.as_ref(),
            after.mouse_over.as_ref(),
        ));
    }
    if before.focused != after.focused {
        invalidation.add(classify_optional_interaction_style_change(
            before.focused.as_ref(),
            after.focused.as_ref(),
        ));
    }
    if before.mouse_down != after.mouse_down {
        invalidation.add(classify_optional_interaction_style_change(
            before.mouse_down.as_ref(),
            after.mouse_down.as_ref(),
        ));
    }

    invalidation
}

pub fn classify_interaction_style(style: Option<&MouseOverAttrs>) -> TreeInvalidation {
    let Some(style) = style else {
        return TreeInvalidation::Registry;
    };

    if interaction_style_affects_measure(style) {
        TreeInvalidation::Measure
    } else if interaction_style_affects_paint(style) {
        TreeInvalidation::Paint
    } else {
        TreeInvalidation::Registry
    }
}

fn classify_optional_interaction_style_change(
    before: Option<&MouseOverAttrs>,
    after: Option<&MouseOverAttrs>,
) -> TreeInvalidation {
    classify_interaction_style(before)
        .join(classify_interaction_style(after))
        .join(TreeInvalidation::Registry)
}

fn interaction_style_affects_measure(style: &MouseOverAttrs) -> bool {
    style.border_width.is_some()
        || style.font.is_some()
        || style.font_weight.is_some()
        || style.font_style.is_some()
        || style.font_size.is_some()
        || style.font_letter_spacing.is_some()
        || style.font_word_spacing.is_some()
        || style.text_align.is_some()
}

fn interaction_style_affects_paint(style: &MouseOverAttrs) -> bool {
    style.background.is_some()
        || style.border_radius.is_some()
        || style.border_style.is_some()
        || style.border_color.is_some()
        || style.box_shadows.is_some()
        || style.font_color.is_some()
        || style.svg_color.is_some()
        || style.font_underline.is_some()
        || style.font_strike.is_some()
        || style.move_x.is_some()
        || style.move_y.is_some()
        || style.rotate.is_some()
        || style.scale.is_some()
        || style.alpha.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::{Background, BorderWidth, Color};

    #[test]
    fn classify_visual_attrs_as_paint() {
        let before = Attrs::default();
        let mut after = Attrs::default();
        after.background = Some(Background::Color(Color::Rgba {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        }));

        assert_eq!(
            classify_attrs_change(&before, &after),
            TreeInvalidation::Paint
        );
    }

    #[test]
    fn classify_layout_attrs_as_measure() {
        let before = Attrs::default();
        let mut after = Attrs::default();
        after.border_width = Some(BorderWidth::Uniform(2.0));

        assert_eq!(
            classify_attrs_change(&before, &after),
            TreeInvalidation::Measure
        );
    }

    #[test]
    fn classify_event_attrs_as_registry() {
        let before = Attrs::default();
        let mut after = Attrs::default();
        after.on_click = Some(true);

        assert_eq!(
            classify_attrs_change(&before, &after),
            TreeInvalidation::Registry
        );
    }

    #[test]
    fn paint_invalidation_uses_refresh_only_without_active_animations() {
        assert_eq!(
            decide_refresh_action(TreeInvalidation::Paint, false, false, false),
            RefreshDecision::RefreshOnly
        );
    }

    #[test]
    fn paint_invalidation_recomputes_with_active_animations() {
        assert_eq!(
            decide_refresh_action(TreeInvalidation::Paint, false, false, true),
            RefreshDecision::Recompute
        );
    }

    #[test]
    fn registry_request_reuses_cached_rebuild_without_invalidation() {
        assert_eq!(
            decide_refresh_action(TreeInvalidation::None, true, true, false),
            RefreshDecision::UseCachedRebuild
        );
    }

    #[test]
    fn registry_request_recomputes_without_cached_rebuild() {
        assert_eq!(
            decide_refresh_action(TreeInvalidation::None, true, false, false),
            RefreshDecision::Recompute
        );
    }
}
