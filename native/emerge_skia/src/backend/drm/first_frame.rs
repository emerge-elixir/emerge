//! The startup acknowledgement means resources are prepared, not visible.
//! A replay barrier prevents a recreated presenter from selecting a queued scene
//! from its previous layout/session; only an explicit application upload is valid.
use crate::{actors::RenderMsg, renderer::RenderState};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub(super) fn wait_for_frame(
    render_rx: &Receiver<RenderMsg>,
    stop: &AtomicBool,
    request: u64,
) -> Result<RenderState, String> {
    loop {
        if stop.load(Ordering::Acquire) {
            return Err("DRM stopped before its first application frame".into());
        }
        match render_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(RenderMsg::Stop) | Err(RecvTimeoutError::Disconnected) => {
                return Err("DRM render channel stopped before its first application frame".into());
            }
            Ok(message) => {
                if let Some(state) = application_frame(message, request) {
                    return Ok(state);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn application_frame(message: RenderMsg, request: u64) -> Option<RenderState> {
    match message {
        RenderMsg::Scene {
            application_frame: true,
            frame_request,
            scene,
            version,
            animate,
            pipeline_submitted_at,
            pipeline_render_queued_at,
            ..
        } if frame_request == request => {
            let mut state = RenderState::default();
            state.set_scene(*scene);
            state.render_version = version;
            state.animate = animate;
            state.pipeline_submitted_at = pipeline_submitted_at;
            state.pipeline_render_queued_at = pipeline_render_queued_at;
            Some(state)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_scene::RenderScene;
    fn scene(application_frame: bool, frame_request: u64) -> RenderMsg {
        RenderMsg::Scene {
            application_frame,
            frame_request,
            scene: Box::new(RenderScene::default()),
            version: 50,
            animate: false,
            pipeline_submitted_at: None,
            pipeline_render_queued_at: None,
            animation_trace: None,
            ime_enabled: false,
            ime_cursor_area: None,
            ime_text_state: Box::new(None),
        }
    }
    #[test]
    fn resize_and_stale_layouts_cannot_activate_even_with_a_nonzero_version() {
        assert!(application_frame(scene(false, 2), 2).is_none());
        assert!(application_frame(scene(true, 1), 2).is_none());
        assert_eq!(
            application_frame(scene(true, 2), 2).unwrap().render_version,
            50
        );
    }
    #[test]
    fn stop_and_disconnect_cancel_wait_without_an_upload() {
        let (tx, rx) = crossbeam_channel::bounded(2);
        tx.send(scene(false, 1)).unwrap();
        tx.send(RenderMsg::Stop).unwrap();
        assert!(wait_for_frame(&rx, &AtomicBool::new(false), 1).is_err());
        drop(tx);
        assert!(wait_for_frame(&rx, &AtomicBool::new(false), 1).is_err());
        assert!(wait_for_frame(&rx, &AtomicBool::new(true), 1).is_err());
    }
    #[test]
    fn valid_empty_application_frame_after_barrier_is_accepted() {
        let (tx, rx) = crossbeam_channel::bounded(3);
        tx.send(scene(true, 1)).unwrap();
        tx.send(scene(false, 2)).unwrap();
        tx.send(scene(true, 2)).unwrap();
        assert_eq!(
            wait_for_frame(&rx, &AtomicBool::new(false), 2)
                .unwrap()
                .render_version,
            50
        );
    }
}
