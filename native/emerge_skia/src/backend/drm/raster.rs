//! CPU-only KMS presenter. No GBM, EGL, OpenGL, or Vulkan objects.
//!
//! Two mapped dumb buffers alternate behind blocking atomic commits. A successful
//! commit is the reuse boundary: the previous front is never drawn into early.
use std::{
    collections::HashMap,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use drm::{
    Device as _, DriverCapability,
    buffer::{Buffer, DrmFourcc},
    control::{
        self, AtomicCommitFlags, Device as _, atomic, connector, crtc, dumbbuffer::DumbBuffer,
        framebuffer, plane, property,
    },
};
use skia_safe::{AlphaType, ColorType, ImageInfo, Paint, Rect, surfaces};

use super::{
    DrmBackendStartupInfo, DrmRunConfig, DrmRunContext,
    core::{self, Card, prop_handle},
    cursor_theme::{CursorVisual, DrmCursorTheme},
};
use crate::{
    actors::{EventMsg, RenderMsg, TreeMsg},
    cursor::CursorState,
    events::CursorIcon,
    input::InputEvent,
    linux_wait::poll_fds,
    renderer::{RenderFrame, RenderState, RenderTimings, SceneRenderer, text_surface_props},
};

struct DumbFramebuffer {
    card: Arc<Card>,
    buffer: DumbBuffer,
    fb: framebuffer::Handle,
}

impl DumbFramebuffer {
    fn new(card: Arc<Card>, dimensions: (u32, u32)) -> Result<Self, String> {
        let buffer = card
            .create_dumb_buffer(dimensions, DrmFourcc::Xrgb8888, 32)
            .map_err(|e| format!("creating DRM dumb buffer failed: {e}"))?;
        let fb = match card.add_framebuffer(&buffer, 24, 32) {
            Ok(fb) => fb,
            Err(error) => {
                let _ = card.destroy_dumb_buffer(buffer);
                return Err(format!("creating dumb framebuffer failed: {error}"));
            }
        };
        Ok(Self { card, buffer, fb })
    }
}

impl Drop for DumbFramebuffer {
    fn drop(&mut self) {
        let _ = self.card.destroy_framebuffer(self.fb);
        let _ = self.card.destroy_dumb_buffer(self.buffer);
    }
}

type Properties = HashMap<String, property::Info>;

struct Kms {
    card: Arc<Card>,
    connector: connector::Handle,
    crtc: crtc::Handle,
    plane: plane::Handle,
    mode: control::Mode,
    connector_props: Properties,
    crtc_props: Properties,
    plane_props: Properties,
    mode_blob: u64,
    touched: bool,
}

impl Kms {
    fn new(context: &DrmRunContext, config: &DrmRunConfig) -> Result<Self, String> {
        if !cfg!(target_endian = "little") {
            return Err("DRM raster XRGB8888 currently requires a little-endian target".into());
        }
        let card = Arc::new(if let Some(presenter) = &context.presenter {
            core::duplicate_card(&presenter.card)?
        } else {
            super::session::open_output(
                config.card_path.as_deref(),
                config.output.as_deref(),
                config.mode.as_deref(),
                config.requested_size,
                false,
                Some(Arc::clone(&context.lifecycle)),
            )?
        });
        if card
            .get_driver_capability(DriverCapability::DumbBuffer)
            .map_err(|e| format!("querying DRM dumb-buffer capability failed: {e}"))?
            == 0
        {
            return Err("DRM driver does not support dumb buffers".into());
        }
        let resources = card.resource_handles().map_err(|e| e.to_string())?;
        let (connector, mode, crtc, _) =
            core::first_connected_connector(&card, &resources, config.requested_size)?;
        let plane = core::find_primary_plane(&card, &resources, crtc)?;
        if !card
            .get_plane(plane)
            .map_err(|e| e.to_string())?
            .formats()
            .contains(&(DrmFourcc::Xrgb8888 as u32))
        {
            return Err("DRM primary plane does not support XRGB8888".into());
        }
        let connector_props = card
            .get_properties(connector)
            .and_then(|props| props.as_hashmap(card.as_ref()))
            .map_err(|e| e.to_string())?;
        let crtc_props = card
            .get_properties(crtc)
            .and_then(|props| props.as_hashmap(card.as_ref()))
            .map_err(|e| e.to_string())?;
        let plane_props = card
            .get_properties(plane)
            .and_then(|props| props.as_hashmap(card.as_ref()))
            .map_err(|e| e.to_string())?;
        let property::Value::Blob(mode_blob) = card
            .create_property_blob(&mode)
            .map_err(|e| format!("creating DRM mode blob failed: {e}"))?
        else {
            return Err("DRM mode property is not a blob".into());
        };
        Ok(Self {
            card,
            connector,
            crtc,
            plane,
            mode,
            connector_props,
            crtc_props,
            plane_props,
            mode_blob,
            touched: false,
        })
    }

    fn dimensions(&self) -> (u32, u32) {
        let (width, height) = self.mode.size();
        (u32::from(width), u32::from(height))
    }

    fn present(&mut self, fb: framebuffer::Handle) -> Result<(), String> {
        let mut req = atomic::AtomicModeReq::new();
        req.add_property(
            self.plane,
            prop_handle(&self.plane_props, "FB_ID")?,
            property::Value::Framebuffer(Some(fb)),
        );
        if !self.touched {
            req.add_property(
                self.connector,
                prop_handle(&self.connector_props, "CRTC_ID")?,
                property::Value::CRTC(Some(self.crtc)),
            );
            req.add_property(
                self.crtc,
                prop_handle(&self.crtc_props, "MODE_ID")?,
                property::Value::Blob(self.mode_blob),
            );
            req.add_property(
                self.crtc,
                prop_handle(&self.crtc_props, "ACTIVE")?,
                property::Value::Boolean(true),
            );
            req.add_property(
                self.plane,
                prop_handle(&self.plane_props, "CRTC_ID")?,
                property::Value::CRTC(Some(self.crtc)),
            );
            let (width, height) = self.dimensions();
            for (name, value) in [
                ("SRC_X", 0),
                ("SRC_Y", 0),
                ("SRC_W", u64::from(width) << 16),
                ("SRC_H", u64::from(height) << 16),
                ("CRTC_X", 0),
                ("CRTC_Y", 0),
                ("CRTC_W", u64::from(width)),
                ("CRTC_H", u64::from(height)),
            ] {
                req.add_property(
                    self.plane,
                    prop_handle(&self.plane_props, name)?,
                    property::Value::UnsignedRange(value),
                );
            }
        }
        let flags = if self.touched {
            AtomicCommitFlags::empty()
        } else {
            AtomicCommitFlags::ALLOW_MODESET
        };
        // Even a failed ioctl may leave uncertain ownership; teardown must run.
        self.touched = true;
        self.card
            .atomic_commit(flags, req)
            .map_err(|e| format!("DRM raster atomic commit failed: {e}"))
    }

    fn detach(&mut self) -> Result<(), String> {
        if !self.touched {
            return Ok(());
        }
        let mut req = atomic::AtomicModeReq::new();
        req.add_property(
            self.plane,
            prop_handle(&self.plane_props, "FB_ID")?,
            property::Value::Framebuffer(None),
        );
        req.add_property(
            self.plane,
            prop_handle(&self.plane_props, "CRTC_ID")?,
            property::Value::CRTC(None),
        );
        req.add_property(
            self.connector,
            prop_handle(&self.connector_props, "CRTC_ID")?,
            property::Value::CRTC(None),
        );
        req.add_property(
            self.crtc,
            prop_handle(&self.crtc_props, "ACTIVE")?,
            property::Value::Boolean(false),
        );
        req.add_property(
            self.crtc,
            prop_handle(&self.crtc_props, "MODE_ID")?,
            property::Value::Blob(0),
        );
        self.card
            .atomic_commit(AtomicCommitFlags::ALLOW_MODESET, req)
            .map_err(|e| format!("DRM raster detach failed: {e}"))?;
        self.touched = false;
        Ok(())
    }

    fn output_available(&self) -> Result<bool, String> {
        let info = self
            .card
            .get_connector(self.connector, true)
            .map_err(|e| e.to_string())?;
        Ok(info.state() == connector::State::Connected
            && info
                .modes()
                .iter()
                .any(|mode| core::mode_id(mode) == core::mode_id(&self.mode)))
    }
}

impl Drop for Kms {
    fn drop(&mut self) {
        let _ = self.card.destroy_property_blob(self.mode_blob);
    }
}

struct Session {
    buffers: Vec<DumbFramebuffer>,
    kms: Kms,
    lifecycle: Arc<crate::runtime::lifecycle::Lifecycle>,
}

impl Session {
    fn new(context: &DrmRunContext, config: &DrmRunConfig) -> Result<Self, String> {
        let kms = Kms::new(context, config)?;
        let buffers = (0..2)
            .map(|_| DumbFramebuffer::new(Arc::clone(&kms.card), kms.dimensions()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            buffers,
            kms,
            lifecycle: Arc::clone(&context.lifecycle),
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Also protect scanout during unwinding, before any buffer owners drop.
        if let Err(error) = self.kms.detach() {
            self.lifecycle.quarantine(error);
            // Each retained buffer holds the card/lease. The kernel retains an
            // active mode blob even when our userspace blob handle is destroyed.
            std::mem::forget(std::mem::take(&mut self.buffers));
        }
    }
}

/// Borrowed only during CPU drawing; Skia cannot retain the mapped buffer.
struct MappedFrame<'a> {
    dimensions: (u32, u32),
    pitch: usize,
    pixels: &'a mut [u8],
}

impl MappedFrame<'_> {
    fn render(
        &mut self,
        renderer: &mut SceneRenderer,
        state: &RenderState,
        cursor: Option<(&CursorVisual, CursorState)>,
        capture: bool,
        profile: bool,
    ) -> Result<(RenderTimings, Option<Vec<u8>>), String> {
        let (width, height) = self.dimensions;
        let row_bytes = (width as usize)
            .checked_mul(4)
            .ok_or("DRM raster row size overflow")?;
        let required = self
            .pitch
            .checked_mul(height as usize)
            .ok_or("DRM raster buffer size overflow")?;
        if width == 0 || height == 0 || self.pitch < row_bytes || self.pixels.len() < required {
            return Err("invalid DRM dumb-buffer dimensions, pitch or mapping length".into());
        }
        let size = (
            i32::try_from(width).map_err(|_| "DRM width is too large")?,
            i32::try_from(height).map_err(|_| "DRM height is too large")?,
        );
        let info = ImageInfo::new(size, ColorType::BGRA8888, AlphaType::Premul, None);
        let mut surface =
            surfaces::wrap_pixels(&info, self.pixels, self.pitch, Some(&text_surface_props()))
                .ok_or("failed to wrap DRM dumb buffer in Skia")?;
        // The destination alternates; an unchanged scene must still paint the other
        // buffer, and repainting also erases a software cursor's previous position.
        renderer.invalidate_visible_frame_fingerprint();
        let mut draw_cursor = |surface: &mut skia_safe::Surface| {
            if let Some((visual, cursor)) = cursor.filter(|(_, cursor)| cursor.visible) {
                let (width, height) = visual.size();
                let (hot_x, hot_y) = visual.hotspot();
                let rect = Rect::from_xywh(
                    cursor.pos.0 - hot_x,
                    cursor.pos.1 - hot_y,
                    width as f32,
                    height as f32,
                );
                surface
                    .canvas()
                    .draw_image_rect(visual.image(), None, rect, &Paint::default());
            }
        };
        let timings = if profile {
            renderer.render_profiled_with_before_flush(
                &mut RenderFrame::new(&mut surface, None),
                state,
                &mut draw_cursor,
            )
        } else {
            renderer.render_with_before_flush(
                &mut RenderFrame::new(&mut surface, None),
                state,
                &mut draw_cursor,
            )
        };
        let pixels = if capture {
            let info = ImageInfo::new(size, ColorType::RGBA8888, AlphaType::Premul, None);
            let mut pixels = vec![
                0;
                row_bytes
                    .checked_mul(height as usize)
                    .ok_or("DRM screenshot size overflow")?
            ];
            if !surface.read_pixels(&info, &mut pixels, row_bytes, (0, 0)) {
                return Err("DRM raster screenshot readback failed".into());
            }
            Some(pixels)
        } else {
            None
        };
        Ok((timings, pixels))
    }
}

fn wait(context: &DrmRunContext, timeout: Option<Duration>) -> Result<(), String> {
    poll_fds(
        &mut [libc::pollfd {
            fd: context.presenter_wake.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }],
        timeout,
    )
    .map(|_| ())
    .map_err(|e| format!("DRM raster wait failed: {e}"))
}

pub(super) fn run(context: DrmRunContext, config: DrmRunConfig) {
    let mut startup = Some(context.startup_tx.clone());
    let result = run_inner(&context, &config, &mut startup);
    if let Err(error) = result {
        if let Some(tx) = startup.take() {
            let _ = tx.send(Err(error.clone()));
        }
        context.native_log.error("drm_raster", error);
    }
    context.stop.store(true, Ordering::Release);
    context.running_flag.store(false, Ordering::Release);
    let _ = context.input_wake.signal();
}

fn run_inner(
    context: &DrmRunContext,
    config: &DrmRunConfig,
    startup: &mut Option<std::sync::mpsc::Sender<Result<DrmBackendStartupInfo, String>>>,
) -> Result<(), String> {
    context.video_registry.set_prime_video_available(false)?;
    let theme = DrmCursorTheme::load(&config.asset_config, &config.cursor_overrides)?;
    let mut retries = config.startup_retries;
    let session = loop {
        if context.stop.load(Ordering::Acquire) {
            return Err("DRM raster startup aborted".into());
        }
        match Session::new(context, config) {
            Ok(session) => break session,
            Err(error) if retries > 0 => {
                retries -= 1;
                context.native_log.warning("drm_raster", error);
                let _ = context.presenter_wake.drain();
                wait(
                    context,
                    Some(Duration::from_millis(u64::from(config.retry_interval_ms))),
                )?;
            }
            Err(error) => return Err(error),
        }
    };
    with_retained_scanout(
        session,
        |session| run_session(context, config, &theme, session, startup),
        |session| {
            if let Some(presenter) = &context.presenter {
                presenter.before_detach();
            }
            session.kms.detach().inspect_err(|error| {
                context.lifecycle.quarantine(error.clone());
            })
        },
    )
}

// Keep scanout ownership outside the draw unwind scope. Never drop framebuffer
// owners until suppression and blocking detach complete, even after a draw panic.
fn with_retained_scanout<T>(
    mut session: T,
    draw: impl FnOnce(&mut T) -> Result<(), String>,
    retire: impl FnOnce(&mut T) -> Result<(), String>,
) -> Result<(), String> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| draw(&mut session)))
        .unwrap_or_else(|_| Err("DRM raster presenter panicked".into()));
    if let Err(error) = retire(&mut session) {
        // Retain the buffers, card and lease on uncertain ownership.
        std::mem::forget(session);
        return Err(error);
    }
    result
}

fn run_session(
    context: &DrmRunContext,
    config: &DrmRunConfig,
    theme: &DrmCursorTheme,
    session: &mut Session,
    startup: &mut Option<std::sync::mpsc::Sender<Result<DrmBackendStartupInfo, String>>>,
) -> Result<(), String> {
    let dimensions = session.kms.dimensions();
    let interval = core::mode_frame_interval(&session.kms.mode);
    let [first, second] = session.buffers.as_mut_slice() else {
        return Err("DRM raster requires exactly two scanout buffers".into());
    };
    let pitches = [
        first.buffer.pitch() as usize,
        second.buffer.pitch() as usize,
    ];
    let framebuffers = [first.fb, second.fb];
    let mut mappings = [
        session
            .kms
            .card
            .map_dumb_buffer(&mut first.buffer)
            .map_err(|e| e.to_string())?,
        session
            .kms
            .card
            .map_dumb_buffer(&mut second.buffer)
            .map_err(|e| e.to_string())?,
    ];
    let _ = context.screen_tx.try_send(dimensions);
    let _ = context.input_wake.signal();
    let _ = context
        .event_tx
        .try_send(EventMsg::InputEvent(InputEvent::Resized {
            width: dimensions.0,
            height: dimensions.1,
            scale_factor: 1.0,
        }));
    context.native_log.info(
        "drm_raster",
        "CPU dumb-buffer scanout with software cursor; no GPU driver",
    );
    // Resource readiness is independent of visibility: return the handle so the
    // caller can upload its first tree before KMS is activated.
    if let Some(tx) = startup.take() {
        tx.send(Ok(DrmBackendStartupInfo::raster()))
            .map_err(|_| "DRM raster startup receiver closed".to_string())?;
    }
    let required_frame = context
        .presenter
        .as_ref()
        .map(|p| p.request_frame(&context.tree_tx, dimensions, &context.stop))
        .transpose()?;
    let mut first_present = true;
    let mut application_frame_ready = false;
    let mut renderer = SceneRenderer::with_cache_config(config.renderer_cache_config);
    let mut state = RenderState::default();
    let mut cursor = context.cursor_state.snapshot();
    let mut icon = CursorIcon::Default;
    let mut dirty = true;
    let mut next_buffer = 0;
    let mut next_hotplug = config.hotplug_interval.map(|i| Instant::now() + i);
    let mut animation_pulse = None;
    let mut next_profile = Instant::now();
    loop {
        let _ = context.presenter_wake.drain();
        if context.stop.load(Ordering::Acquire) {
            return Ok(());
        }
        while let Ok(message) = context.render_rx.try_recv() {
            match message {
                RenderMsg::Stop => return Ok(()),
                RenderMsg::Scene {
                    application_frame,
                    frame_request,
                    scene,
                    version,
                    animate,
                    pipeline_submitted_at,
                    pipeline_render_queued_at,
                    ..
                } => {
                    application_frame_ready = application_frame
                        && (!first_present
                            || required_frame.is_none_or(|request| frame_request == request));
                    state.set_scene(*scene);
                    state.render_version = version;
                    state.animate = animate;
                    state.pipeline_submitted_at = pipeline_submitted_at;
                    state.pipeline_render_queued_at = pipeline_render_queued_at;
                    context
                        .video_registry
                        .set_active_targets(&state.video_target_ids)?;
                    if let Some(stats) = context.stats.as_deref() {
                        stats.record_pipeline_draw_started(
                            pipeline_render_queued_at,
                            Instant::now(),
                        );
                        stats.record_drm_scene_selected_for_draw();
                    }
                    if !animate {
                        animation_pulse = None;
                    }
                    dirty = true;
                }
            }
        }
        while let Ok(new_icon) = context.cursor_icon_rx.try_recv() {
            dirty |= icon != new_icon;
            icon = new_icon;
        }
        let new_cursor = context.cursor_state.snapshot();
        dirty |= cursor.version != new_cursor.version;
        cursor = new_cursor;
        dirty |= renderer.sync_cpu_video_frames(&context.video_registry)?;
        let capture_generation = context.latest_frame.pending_capture_generation();
        dirty |= capture_generation.is_some();
        if next_hotplug.is_some_and(|t| Instant::now() >= t) {
            if !session.kms.output_available()? {
                return Err("DRM raster output disconnected or selected mode changed".into());
            }
            next_hotplug = config.hotplug_interval.map(|i| Instant::now() + i);
        }
        if dirty && application_frame_ready {
            let started = Instant::now();
            let profile = config.renderer_stats_log && started >= next_profile;
            if profile {
                next_profile = started + Duration::from_secs(1);
            }
            let (timings, capture) = MappedFrame {
                dimensions,
                pitch: pitches[next_buffer],
                pixels: mappings[next_buffer].as_mut(),
            }
            .render(
                &mut renderer,
                &state,
                Some((theme.cursor(icon), cursor.state)),
                capture_generation.is_some(),
                profile,
            )?;
            if profile && crate::stats::render_frame_has_slow_stage(&timings) {
                context.native_log.info(
                    "renderer_slow_frame",
                    crate::stats::format_slow_render_frame_log(
                        "drm_raster",
                        &timings,
                        state.scene.summary(),
                    ),
                );
            }
            // Blocking commit waits for flip completion, so the former front is
            // safe to reuse only after this succeeds. Never rotate on an error.
            let render_duration = started.elapsed();
            let commit_started = Instant::now();
            session.kms.present(framebuffers[next_buffer])?;
            first_present = false;
            if let Some(presenter) = &context.presenter {
                presenter.presented();
            }
            next_buffer ^= 1;
            dirty = false;
            let presented = Instant::now();
            if config.render_log {
                context.native_log.info(
                    "drm_raster",
                    format!(
                        "presented version={} latest={} render_us={} commit_us={}",
                        state.render_version,
                        context.render_counter.load(Ordering::Relaxed),
                        render_duration.as_micros(),
                        commit_started.elapsed().as_micros(),
                    ),
                );
            }
            if let Some(stats) = context.stats.as_deref() {
                stats.record_render_timings(render_duration, &timings);
                stats.record_drm_atomic_commit_ioctl(commit_started.elapsed());
                stats.record_frame_present();
                stats.record_display_interval(interval);
                if let Some(submitted) = state.pipeline_submitted_at {
                    stats.record_pipeline(submitted, presented);
                }
            }
            if let (Some(generation), Some(pixels)) = (capture_generation, capture) {
                context.latest_frame.publish_requested_capture(
                    generation,
                    dimensions.0,
                    dimensions.1,
                    1.0,
                    pixels,
                );
            }
            let _ = context.event_tx.try_send(EventMsg::PresentTiming {
                presented_at: presented,
                predicted_next_present_at: presented + interval,
            });
            if state.animate {
                animation_pulse = Some((presented, presented + interval));
            }
        }
        if let Some((presented_at, predicted_next_present_at)) = animation_pulse {
            match context.tree_tx.try_send(TreeMsg::AnimationPulse {
                presented_at,
                predicted_next_present_at,
                trace: None,
            }) {
                Ok(()) => animation_pulse = None,
                Err(crossbeam_channel::TrySendError::Disconnected(_)) => return Ok(()),
                Err(crossbeam_channel::TrySendError::Full(_)) => {}
            }
        }
        let timeout = if animation_pulse.is_some() {
            Some(Duration::from_millis(5))
        } else {
            next_hotplug.map(|t| t.saturating_duration_since(Instant::now()))
        };
        wait(context, timeout)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{render_scene::RenderScene, renderer::RendererCacheConfig};
    use skia_safe::Color;

    #[test]
    fn mapped_scanout_honors_pitch_and_captures_rgba() {
        let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
            enabled: false,
            ..RendererCacheConfig::default()
        });
        let state = RenderState::new(
            RenderScene::default(),
            Color::from_rgb(10, 20, 30),
            1,
            false,
        );
        let mut data = [0xAA; 24];
        let (_, capture) = MappedFrame {
            dimensions: (2, 2),
            pitch: 12,
            pixels: &mut data,
        }
        .render(&mut renderer, &state, None, true, true)
        .unwrap();
        assert_eq!(capture.unwrap(), [10, 20, 30, 255].repeat(4));
        assert_eq!(&data[..8], [30, 20, 10, 255].repeat(2));
        assert_eq!(&data[12..20], [30, 20, 10, 255].repeat(2));
        assert_eq!(&data[8..12], &[0xAA; 4]);
        // Same scene on a different buffer must not take the unchanged-frame skip.
        let mut other = [0xBB; 24];
        MappedFrame {
            dimensions: (2, 2),
            pitch: 12,
            pixels: &mut other,
        }
        .render(&mut renderer, &state, None, false, false)
        .unwrap();
        assert_eq!(&other[..8], &data[..8]);
        assert_eq!(&other[12..20], &data[12..20]);
    }

    #[test]
    fn software_cursor_is_erased_when_hidden() {
        let theme = DrmCursorTheme::load(&crate::assets::AssetConfig::default(), &[]).unwrap();
        let mut renderer = SceneRenderer::new();
        let state = RenderState::new(
            RenderScene::default(),
            Color::from_rgb(10, 20, 30),
            1,
            false,
        );
        let mut pixels = vec![0; 80 * 80 * 4];
        let background = [30, 20, 10, 255].repeat(80 * 80);
        let mut frame = MappedFrame {
            dimensions: (80, 80),
            pitch: 80 * 4,
            pixels: &mut pixels,
        };
        frame
            .render(
                &mut renderer,
                &state,
                Some((
                    theme.cursor(CursorIcon::Default),
                    CursorState {
                        pos: (10.0, 10.0),
                        visible: true,
                    },
                )),
                false,
                false,
            )
            .unwrap();
        assert_ne!(frame.pixels, background);
        frame
            .render(
                &mut renderer,
                &state,
                Some((
                    theme.cursor(CursorIcon::Default),
                    CursorState {
                        pos: (10.0, 10.0),
                        visible: false,
                    },
                )),
                false,
                false,
            )
            .unwrap();
        assert_eq!(frame.pixels, background);
    }

    #[test]
    fn scanout_survives_draw_unwind_and_is_retained_if_detach_fails() {
        struct Owner(Arc<std::sync::Mutex<Vec<&'static str>>>);
        impl Drop for Owner {
            fn drop(&mut self) {
                self.0.lock().unwrap().push("drop");
            }
        }
        for (panic_draw, fail_detach) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let trace = Arc::new(std::sync::Mutex::new(Vec::new()));
            let result = with_retained_scanout(
                Owner(trace.clone()),
                |owner| {
                    owner.0.lock().unwrap().push("draw");
                    assert!(!panic_draw, "injected draw panic");
                    Ok(())
                },
                |owner| {
                    owner.0.lock().unwrap().extend(["dark", "detach"]);
                    if fail_detach {
                        Err("detach failed".into())
                    } else {
                        Ok(())
                    }
                },
            );
            assert_eq!(result.is_err(), panic_draw || fail_detach);
            let trace = trace.lock().unwrap();
            assert_eq!(&trace[..3], &["draw", "dark", "detach"]);
            assert_eq!(trace.contains(&"drop"), !fail_detach);
        }
    }

    #[test]
    fn malformed_mapping_is_rejected_before_drawing() {
        let mut renderer = SceneRenderer::new();
        for (dimensions, pitch, len) in [
            ((0, 2), 8, 16),
            ((2, 0), 8, 16),
            ((2, 2), 7, 16),
            ((2, 2), 8, 15),
            ((2, 2), usize::MAX, 16),
        ] {
            let mut pixels = vec![0xAA; len];
            assert!(
                MappedFrame {
                    dimensions,
                    pitch,
                    pixels: &mut pixels
                }
                .render(&mut renderer, &RenderState::default(), None, false, false)
                .is_err()
            );
            assert!(pixels.iter().all(|byte| *byte == 0xAA));
        }
    }
}
