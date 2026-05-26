use std::time::{Duration, Instant, SystemTime};

use smithay_client_toolkit::shell::{WaylandSurface, xdg::window::Window};
use wayland_client::QueueHandle;

use crate::backend::present::{FrameIntervalEstimator, plausible_frame_interval};

use super::runtime::WaylandApp;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum FrameCallbackState {
    #[default]
    None,
    Requested,
    Received,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DrawKind {
    Normal,
    LateReplacement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DrawDecision {
    Skip,
    Draw(DrawKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrameCallbackRequest {
    pub(super) sequence: u64,
    pub(super) requested_at: Instant,
    pub(super) wall_requested_at: SystemTime,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PresentSnapshot {
    pub(super) configured: bool,
    pub(super) redraw_requested: bool,
    pub(super) frame_callback_state: FrameCallbackState,
    pub(super) requested_frame_callback_sequence: Option<u64>,
    pub(super) requested_frame_callback_age: Option<Duration>,
    pub(super) requested_frame_callback_wall_age: Option<Duration>,
    pub(super) latest_received_render_version: Option<u64>,
    pub(super) latest_received_from_patch: bool,
    pub(super) latest_received_animation_active: bool,
    pub(super) last_submitted_render_version: Option<u64>,
    pub(super) late_replacement_used: bool,
    pub(super) has_newer_received_scene: bool,
    pub(super) can_late_replace: bool,
    pub(super) ready_frame_callback_buffered: bool,
    pub(super) estimated_frame_interval: Duration,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PresentState {
    pub(super) configured: bool,
    redraw_requested: bool,
    frame_callback_state: FrameCallbackState,
    next_frame_callback_request_sequence: u64,
    requested_frame_callback: Option<FrameCallbackRequest>,
    last_frame_callback_at: Option<Instant>,
    last_frame_callback_time_ms: Option<u32>,
    ready_frame_callback_at: Option<Instant>,
    frame_interval: FrameIntervalEstimator,
    latest_received_render_version: Option<u64>,
    latest_received_from_patch: bool,
    latest_received_animation_active: bool,
    last_submitted_render_version: Option<u64>,
    late_replacement_used: bool,
}

impl Default for PresentState {
    fn default() -> Self {
        Self {
            configured: false,
            redraw_requested: false,
            frame_callback_state: FrameCallbackState::None,
            next_frame_callback_request_sequence: 0,
            requested_frame_callback: None,
            last_frame_callback_at: None,
            last_frame_callback_time_ms: None,
            ready_frame_callback_at: None,
            frame_interval: FrameIntervalEstimator::default(),
            latest_received_render_version: None,
            latest_received_from_patch: false,
            latest_received_animation_active: false,
            last_submitted_render_version: None,
            late_replacement_used: false,
        }
    }
}

impl PresentState {
    #[cfg(test)]
    pub(super) fn configured_for_test() -> Self {
        Self {
            configured: true,
            ..Self::default()
        }
    }

    pub(super) fn queue_redraw(&mut self) {
        self.redraw_requested = true;
    }

    pub(super) fn draw_decision(&self, exit: bool, allow_late_replacement: bool) -> DrawDecision {
        if exit || !self.configured || !self.redraw_requested {
            return DrawDecision::Skip;
        }

        if self.frame_callback_state != FrameCallbackState::Requested {
            return DrawDecision::Draw(DrawKind::Normal);
        }

        if allow_late_replacement && self.can_late_replace() {
            DrawDecision::Draw(DrawKind::LateReplacement)
        } else {
            DrawDecision::Skip
        }
    }

    pub(super) fn note_scene_received(
        &mut self,
        version: u64,
        from_patch: bool,
        animation_active: bool,
    ) {
        self.latest_received_render_version = Some(version);
        self.latest_received_from_patch = from_patch;
        self.latest_received_animation_active = animation_active;
    }

    pub(super) fn prepare_draw(
        &mut self,
        kind: DrawKind,
        window: &Window,
        qh: &QueueHandle<WaylandApp>,
    ) -> Option<FrameCallbackRequest> {
        if kind == DrawKind::Normal {
            self.request_frame_callback(window, qh)
        } else {
            None
        }
    }

    fn request_frame_callback(
        &mut self,
        window: &Window,
        qh: &QueueHandle<WaylandApp>,
    ) -> Option<FrameCallbackRequest> {
        match self.frame_callback_state {
            FrameCallbackState::None | FrameCallbackState::Received => {
                window.wl_surface().frame(qh, window.wl_surface().clone());
                let request = FrameCallbackRequest {
                    sequence: self.next_frame_callback_request_sequence,
                    requested_at: Instant::now(),
                    wall_requested_at: SystemTime::now(),
                };
                self.next_frame_callback_request_sequence =
                    self.next_frame_callback_request_sequence.wrapping_add(1);
                self.requested_frame_callback = Some(request);
                self.frame_callback_state = FrameCallbackState::Requested;
                self.late_replacement_used = false;
                Some(request)
            }
            FrameCallbackState::Requested => None,
        }
    }

    pub(super) fn frame_callback_received(&mut self, received_at: Instant, callback_time_ms: u32) {
        let observed_from_callback =
            self.last_frame_callback_time_ms
                .map(|last_callback_time_ms| {
                    Duration::from_millis(u64::from(
                        callback_time_ms.wrapping_sub(last_callback_time_ms),
                    ))
                });
        let observed_from_arrival = self
            .last_frame_callback_at
            .map(|last_callback_at| received_at.saturating_duration_since(last_callback_at));

        if let Some(observed) = observed_from_callback
            .filter(|interval| plausible_frame_interval(*interval))
            .or(observed_from_arrival)
            .filter(|interval| plausible_frame_interval(*interval))
        {
            self.frame_interval.observe_interval(observed);
        }

        self.last_frame_callback_at = Some(received_at);
        self.last_frame_callback_time_ms = Some(callback_time_ms);
        self.ready_frame_callback_at = Some(received_at);
        self.frame_callback_state = FrameCallbackState::Received;
        self.requested_frame_callback = None;
        self.late_replacement_used = false;
    }

    pub(super) fn finish_present(
        &mut self,
        render_version: u64,
        kind: DrawKind,
        video_needs_cleanup: bool,
    ) {
        self.last_submitted_render_version = Some(render_version);
        self.late_replacement_used = kind == DrawKind::LateReplacement;
        self.redraw_requested = video_needs_cleanup;
    }

    pub(super) fn finish_noop_present(&mut self, render_version: u64) {
        self.last_submitted_render_version = Some(render_version);
        self.late_replacement_used = false;
        self.redraw_requested = false;
    }

    pub(super) fn present_timing_for_normal_draw(
        &mut self,
        fallback_presented_at: Instant,
    ) -> (Instant, Instant) {
        let presented_at = self
            .ready_frame_callback_at
            .take()
            .unwrap_or(fallback_presented_at);
        (
            presented_at,
            self.frame_interval.predict_next_present_after(presented_at),
        )
    }

    pub(super) fn estimated_frame_interval(&self) -> Duration {
        self.frame_interval.estimated_frame_interval()
    }

    pub(super) fn clear_ready_frame_callback_timing_if_idle(&mut self) {
        if !self.redraw_requested && self.frame_callback_state == FrameCallbackState::Received {
            self.ready_frame_callback_at = None;
            self.last_frame_callback_at = None;
            self.last_frame_callback_time_ms = None;
        }
    }

    pub(super) fn snapshot(&self, now: Instant, wall_now: SystemTime) -> PresentSnapshot {
        let requested_frame_callback_age = self
            .requested_frame_callback
            .map(|request| now.saturating_duration_since(request.requested_at));
        let requested_frame_callback_wall_age = self
            .requested_frame_callback
            .and_then(|request| wall_now.duration_since(request.wall_requested_at).ok());

        PresentSnapshot {
            configured: self.configured,
            redraw_requested: self.redraw_requested,
            frame_callback_state: self.frame_callback_state,
            requested_frame_callback_sequence: self
                .requested_frame_callback
                .map(|request| request.sequence),
            requested_frame_callback_age,
            requested_frame_callback_wall_age,
            latest_received_render_version: self.latest_received_render_version,
            latest_received_from_patch: self.latest_received_from_patch,
            latest_received_animation_active: self.latest_received_animation_active,
            last_submitted_render_version: self.last_submitted_render_version,
            late_replacement_used: self.late_replacement_used,
            has_newer_received_scene: self.has_newer_received_scene(),
            can_late_replace: self.can_late_replace(),
            ready_frame_callback_buffered: self.ready_frame_callback_at.is_some(),
            estimated_frame_interval: self.estimated_frame_interval(),
        }
    }

    fn can_late_replace(&self) -> bool {
        self.latest_received_from_patch
            && !self.latest_received_animation_active
            && !self.late_replacement_used
            && self.has_newer_received_scene()
    }

    fn has_newer_received_scene(&self) -> bool {
        match (
            self.latest_received_render_version,
            self.last_submitted_render_version,
        ) {
            (Some(latest), Some(submitted)) => latest > submitted,
            (Some(_), None) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DrawDecision, DrawKind, FrameCallbackState, PresentState};
    use std::time::{Duration, Instant};

    #[test]
    fn pending_frame_waits_when_no_new_patch_scene_arrived() {
        let mut present = PresentState {
            configured: true,
            ..PresentState::default()
        };
        present.queue_redraw();
        present.frame_callback_state = FrameCallbackState::Requested;
        present.last_submitted_render_version = Some(1);

        assert_eq!(present.draw_decision(false, true), DrawDecision::Skip);
    }

    #[test]
    fn allows_one_patch_late_replacement_when_swap_is_nonblocking() {
        let mut present = PresentState {
            configured: true,
            ..PresentState::default()
        };
        present.queue_redraw();
        present.frame_callback_state = FrameCallbackState::Requested;
        present.last_submitted_render_version = Some(1);
        present.note_scene_received(2, true, false);

        assert_eq!(
            present.draw_decision(false, true),
            DrawDecision::Draw(DrawKind::LateReplacement)
        );

        present.finish_present(2, DrawKind::LateReplacement, false);
        present.queue_redraw();
        present.note_scene_received(3, true, false);

        assert_eq!(present.draw_decision(false, true), DrawDecision::Skip);
    }

    #[test]
    fn skips_late_replacement_when_swap_may_block() {
        let mut present = PresentState {
            configured: true,
            ..PresentState::default()
        };
        present.queue_redraw();
        present.frame_callback_state = FrameCallbackState::Requested;
        present.last_submitted_render_version = Some(1);
        present.note_scene_received(2, true, false);

        assert_eq!(present.draw_decision(false, false), DrawDecision::Skip);
    }

    #[test]
    fn ignores_animation_only_scene_updates() {
        let mut present = PresentState {
            configured: true,
            ..PresentState::default()
        };
        present.queue_redraw();
        present.frame_callback_state = FrameCallbackState::Requested;
        present.last_submitted_render_version = Some(1);
        present.note_scene_received(2, false, true);

        assert_eq!(present.draw_decision(false, true), DrawDecision::Skip);
    }

    #[test]
    fn skips_late_replacement_for_animation_active_patch_scene() {
        let mut present = PresentState {
            configured: true,
            ..PresentState::default()
        };
        present.queue_redraw();
        present.frame_callback_state = FrameCallbackState::Requested;
        present.last_submitted_render_version = Some(1);
        present.note_scene_received(2, true, true);

        assert_eq!(present.draw_decision(false, true), DrawDecision::Skip);
    }

    #[test]
    fn normal_draw_requests_after_callback_even_when_replacement_was_used() {
        let mut present = PresentState {
            configured: true,
            ..PresentState::default()
        };
        present.queue_redraw();
        present.frame_callback_state = FrameCallbackState::Requested;
        present.last_submitted_render_version = Some(1);
        present.note_scene_received(2, true, false);

        assert_eq!(
            present.draw_decision(false, true),
            DrawDecision::Draw(DrawKind::LateReplacement)
        );
        present.finish_present(2, DrawKind::LateReplacement, false);

        present.frame_callback_received(Instant::now(), 1_000);
        present.queue_redraw();
        present.note_scene_received(3, true, false);

        assert_eq!(
            present.draw_decision(false, true),
            DrawDecision::Draw(DrawKind::Normal)
        );
    }

    #[test]
    fn frame_callbacks_update_reasonable_display_interval() {
        let mut present = PresentState::default();
        let first = Instant::now();

        present.frame_callback_received(first, 1_000);
        let (_, predicted) =
            present.present_timing_for_normal_draw(first + Duration::from_millis(2));
        assert_eq!(predicted, first + Duration::from_millis(16));

        let second = first + Duration::from_millis(12);
        present.frame_callback_received(second, 1_012);
        let (presented, predicted) =
            present.present_timing_for_normal_draw(second + Duration::from_millis(2));
        assert_eq!(presented, second);
        assert_eq!(predicted, second + Duration::from_millis(12));
    }

    #[test]
    fn frame_callbacks_prefer_compositor_timestamp_over_arrival_jitter() {
        let mut present = PresentState::default();
        let first = Instant::now();
        let second = first + Duration::from_millis(30);

        present.frame_callback_received(first, 1_000);
        present.present_timing_for_normal_draw(first + Duration::from_millis(2));

        present.frame_callback_received(second, 1_016);
        let (_, predicted) = present.present_timing_for_normal_draw(second);

        assert_eq!(predicted, second + Duration::from_millis(16));
    }

    #[test]
    fn idle_skip_discards_stale_frame_callback_timing() {
        let mut present = PresentState::default();
        let callback_at = Instant::now();

        present.frame_callback_received(callback_at, 1_000);
        present.clear_ready_frame_callback_timing_if_idle();

        let fallback = callback_at + Duration::from_millis(80);
        let (presented, predicted) = present.present_timing_for_normal_draw(fallback);
        assert_eq!(presented, fallback);
        assert_eq!(predicted, fallback + Duration::from_millis(16));
    }

    #[test]
    fn idle_skip_discards_stale_interval_anchor_but_keeps_estimate() {
        let mut present = PresentState::default();
        let first = Instant::now();
        let second = first + Duration::from_millis(17);
        let after_idle = second + Duration::from_millis(67);

        present.frame_callback_received(first, 1_000);
        present.present_timing_for_normal_draw(first);
        present.frame_callback_received(second, 1_017);
        let (_, predicted) = present.present_timing_for_normal_draw(second);
        assert_eq!(predicted, second + Duration::from_millis(17));

        present.clear_ready_frame_callback_timing_if_idle();
        present.frame_callback_received(after_idle, 1_084);
        let (_, predicted_after_idle) = present.present_timing_for_normal_draw(after_idle);

        assert_eq!(predicted_after_idle, after_idle + Duration::from_millis(17));
    }
}
