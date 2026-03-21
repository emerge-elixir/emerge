use std::time::{Duration, Instant};

use smithay_client_toolkit::shell::{WaylandSurface, xdg::window::Window};
use wayland_client::QueueHandle;

use super::runtime::WaylandApp;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum FrameCallbackState {
    #[default]
    None,
    Requested,
    Received,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PresentState {
    pub(super) configured: bool,
    redraw_requested: bool,
    frame_callback_state: FrameCallbackState,
    last_present_at: Option<Instant>,
    estimated_frame_interval: Duration,
}

impl Default for PresentState {
    fn default() -> Self {
        Self {
            configured: false,
            redraw_requested: false,
            frame_callback_state: FrameCallbackState::None,
            last_present_at: None,
            estimated_frame_interval: Duration::from_millis(16),
        }
    }
}

impl PresentState {
    pub(super) fn queue_redraw(&mut self) {
        self.redraw_requested = true;
    }

    pub(super) fn can_draw(&self, exit: bool) -> bool {
        !exit
            && self.configured
            && self.redraw_requested
            && self.frame_callback_state != FrameCallbackState::Requested
    }

    pub(super) fn request_frame_callback(&mut self, window: &Window, qh: &QueueHandle<WaylandApp>) {
        match self.frame_callback_state {
            FrameCallbackState::None | FrameCallbackState::Received => {
                window.wl_surface().frame(qh, window.wl_surface().clone());
                self.frame_callback_state = FrameCallbackState::Requested;
            }
            FrameCallbackState::Requested => {}
        }
    }

    pub(super) fn frame_callback_received(&mut self) {
        self.frame_callback_state = FrameCallbackState::Received;
    }

    pub(super) fn finish_present(&mut self, video_needs_cleanup: bool) {
        self.redraw_requested = video_needs_cleanup;
    }

    pub(super) fn observe_present(&mut self, presented_at: Instant) -> Instant {
        if let Some(last_present_at) = self.last_present_at {
            let observed = presented_at.saturating_duration_since(last_present_at);
            if observed >= Duration::from_millis(4) && observed <= Duration::from_millis(100) {
                self.estimated_frame_interval = observed;
            }
        }

        self.last_present_at = Some(presented_at);
        presented_at + self.estimated_frame_interval
    }
}
