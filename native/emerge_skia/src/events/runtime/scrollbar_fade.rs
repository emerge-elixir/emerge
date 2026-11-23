//! Scrollbars that show while a container scrolls (`scrollbars: :while_scrolling`): scroll activity
//! sets a container's thumb alpha to 1, it holds for `SCROLLBAR_HOLD`, then fades out over
//! `SCROLLBAR_FADE`. Timers run only while a thumb is visible.
use super::*;

/// Time a thumb stays fully visible after the last scroll movement.
const SCROLLBAR_HOLD: Duration = Duration::from_millis(800);
/// Fade-out duration after the hold.
const SCROLLBAR_FADE: Duration = Duration::from_millis(250);
/// Interval of fade steps when no present timing arrives.
const SCROLLBAR_FADE_STEP: Duration = Duration::from_millis(16);
/// Alpha steps sent to the tree; finer steps are not visible on a 5 px thumb.
const SCROLLBAR_ALPHA_STEPS: f32 = 32.0;

/// When the renderer draws the scrollbar thumbs of a scroll container.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, rustler::NifUnitEnum)]
pub(crate) enum ScrollbarVisibility {
    /// Thumbs are visible whenever the content overflows.
    #[default]
    Always,
    /// Thumbs are visible while the container scrolls, then fade out.
    WhileScrolling,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ScrollbarFade {
    pub(super) last_activity: Instant,
    pub(super) sent: f32,
}

impl ScrollbarFade {
    fn alpha_at(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.last_activity);
        let fading = elapsed.saturating_sub(SCROLLBAR_HOLD).as_secs_f32();
        let alpha = (1.0 - fading / SCROLLBAR_FADE.as_secs_f32()).clamp(0.0, 1.0);
        (alpha * SCROLLBAR_ALPHA_STEPS).round() / SCROLLBAR_ALPHA_STEPS
    }

    fn next_deadline(&self, now: Instant) -> Instant {
        let fade_start = self.last_activity + SCROLLBAR_HOLD;
        if now < fade_start {
            fade_start
        } else {
            now + SCROLLBAR_FADE_STEP
        }
    }
}

impl DirectEventRuntime {
    pub(super) fn set_scrollbar_visibility(&mut self, visibility: ScrollbarVisibility) {
        self.scrollbar_visibility = visibility;
        if visibility == ScrollbarVisibility::Always {
            self.scrollbar_fades.clear();
        }
    }

    /// Shows the thumbs of `element_id` after scroll movement; the tree message when it was hidden.
    pub(super) fn show_scrollbars(&mut self, element_id: NodeId, now: Instant) -> Option<TreeMsg> {
        if self.scrollbar_visibility != ScrollbarVisibility::WhileScrolling {
            return None;
        }
        let fade = self
            .scrollbar_fades
            .entry(element_id)
            .or_insert(ScrollbarFade {
                last_activity: now,
                sent: 0.0,
            });
        fade.last_activity = now;
        (fade.sent != 1.0).then(|| {
            fade.sent = 1.0;
            TreeMsg::SetScrollbarAlpha {
                element_id,
                alpha: 1.0,
            }
        })
    }

    pub(super) fn next_scrollbar_fade_deadline(&self) -> Option<Instant> {
        let now = Instant::now();
        self.scrollbar_fades
            .values()
            .map(|fade| fade.next_deadline(now))
            .min()
    }

    /// Advances every fade to `now`; drops thumbs that reached zero.
    pub(super) fn step_scrollbar_fades(&mut self, now: Instant, tree_tx: &dyn TreeSink, log: bool) {
        let messages: Vec<_> = self
            .scrollbar_fades
            .iter_mut()
            .filter_map(|(element_id, fade)| {
                let alpha = fade.alpha_at(now);
                (alpha != fade.sent).then(|| {
                    fade.sent = alpha;
                    TreeMsg::SetScrollbarAlpha {
                        element_id: *element_id,
                        alpha,
                    }
                })
            })
            .collect();
        self.scrollbar_fades.retain(|_, fade| fade.sent > 0.0);
        let fading = self
            .scrollbar_fades
            .values()
            .any(|fade| now >= fade.last_activity + SCROLLBAR_HOLD);
        send_tree_messages(tree_tx, messages, log);
        if fading {
            self.backend_wake.request_redraw();
        }
    }
}
