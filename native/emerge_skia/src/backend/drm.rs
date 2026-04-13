use std::collections::HashMap;
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::raw::c_void;
use std::ptr;
use std::sync::mpsc::Sender as StartupSender;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

mod cursor_theme;

use drm::ClientCapability;
use drm::Device as BasicDevice;
use drm::control::{
    self, AtomicCommitFlags, Device as ControlDevice, PlaneType, ResourceHandles, atomic,
    connector, crtc, encoder, framebuffer, plane, property,
};
use gbm::{
    AsRaw, BufferObject, BufferObjectFlags, Device as GbmDevice, Format as GbmFormat, Surface,
};
use glutin_egl_sys::egl;
use glutin_egl_sys::egl::types::{EGLConfig, EGLContext, EGLDisplay, EGLSurface, EGLenum, EGLint};
use libloading::Library;
use skia_safe::{Paint, Rect, gpu::gl::FramebufferInfo};

use crossbeam_channel::{Receiver, Sender, TrySendError};

use crate::DrmCursorOverrideConfig;
use crate::actors::{EventMsg, RenderMsg, TreeMsg};
use crate::assets::AssetConfig;
use crate::backend::skia_gpu::GlFrameSurface;
use crate::backend::wake::BackendWake;
use crate::cursor::{CursorState, SharedCursorState};
use crate::events::CursorIcon;
use crate::input::InputEvent;
use crate::linux_wait::{EventFd, poll_fds};
use crate::native_log::NativeLogRelay;
use crate::renderer::{RenderState, SceneRenderer};
use crate::stats::RendererStatsCollector;
use crate::video::{VideoImportContext, VideoRegistry};

use self::cursor_theme::{CURSOR_PLANE_SIZE, CursorVisual, DrmCursorTheme};

const EGL_PLATFORM_GBM_KHR: EGLenum = 0x31D7;

#[derive(Clone)]
pub struct DrmBackendWake {
    presenter_wake: EventFd,
    input_wake: EventFd,
}

impl DrmBackendWake {
    pub fn new(presenter_wake: EventFd, input_wake: EventFd) -> Self {
        Self {
            presenter_wake,
            input_wake,
        }
    }
}

impl BackendWake for DrmBackendWake {
    fn request_stop(&self) {
        let _ = self.presenter_wake.signal();
        let _ = self.input_wake.signal();
    }

    fn request_redraw(&self) {
        let _ = self.presenter_wake.signal();
    }

    fn notify_video_frame(&self) {
        let _ = self.presenter_wake.signal();
    }
}

struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl AsRawFd for Card {
    fn as_raw_fd(&self) -> i32 {
        self.0.as_raw_fd()
    }
}

impl BasicDevice for Card {}
impl ControlDevice for Card {}

struct EglState {
    egl: egl::Egl,
    _egl_lib: Library,
    display: EGLDisplay,
    _context: EGLContext,
    surface: EGLSurface,
}

struct CursorPlane {
    commit: CursorPlaneCommit,
    bo: BufferObject<()>,
}

struct PreparedPrimaryFrame {
    generation: u64,
    render_version: u64,
    bo: BufferObject<()>,
    fb: framebuffer::Handle,
    video_needs_cleanup: bool,
}

#[derive(Clone, Copy)]
struct DrmOutputConfig<'a> {
    connector: connector::Handle,
    crtc_handle: crtc::Handle,
    plane: plane::Handle,
    con_props: &'a HashMap<String, property::Info>,
    crtc_props: &'a HashMap<String, property::Info>,
    plane_props: &'a HashMap<String, property::Info>,
}

struct PrimaryFrameCursorContext<'a> {
    pos: (f32, f32),
    visible: bool,
    hw_cursor_enabled: bool,
    icon: CursorIcon,
    theme: &'a DrmCursorTheme,
}

struct PrimaryFrameContext<'a> {
    renderer: &'a mut SceneRenderer,
    frame_surface: &'a mut GlFrameSurface,
    render_state: &'a RenderState,
    cursor: PrimaryFrameCursorContext<'a>,
    video_registry: &'a Arc<VideoRegistry>,
    video_import: Option<&'a VideoImportContext>,
    egl_state: &'a EglState,
    gbm_surface: &'a Surface<()>,
    card: &'a Card,
    framebuffer_cache: &'a mut HashMap<u32, framebuffer::Handle>,
}

struct CurrentPrimaryFrame {
    generation: u64,
    render_version: u64,
    bo: BufferObject<()>,
    fb: framebuffer::Handle,
}

struct SubmittedCursorState {
    version: Option<u64>,
    visible: bool,
    icon: CursorIcon,
}

struct InFlightCommit {
    primary: Option<PreparedPrimaryFrame>,
    cursor: Option<SubmittedCursorState>,
    emit_animation_pulse: bool,
}

const FOLLOW_UP_PRIMARY_WINDOW: Duration = Duration::from_millis(4);

struct DrmPresentState {
    last_present_at: Option<Instant>,
    estimated_frame_interval: Duration,
}

impl DrmPresentState {
    fn new(initial_frame_interval: Duration) -> Self {
        Self {
            last_present_at: None,
            estimated_frame_interval: initial_frame_interval,
        }
    }

    fn observe_present(&mut self, presented_at: Instant) -> Instant {
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

#[derive(Clone)]
struct CursorPlaneCommit {
    handle: plane::Handle,
    props: Arc<HashMap<String, property::Info>>,
    fb: framebuffer::Handle,
    size: (u32, u32),
}

impl CursorPlane {
    fn commit(&self) -> &CursorPlaneCommit {
        &self.commit
    }

    fn write_visual(&mut self, visual: &CursorVisual) -> Result<(), String> {
        self.bo
            .write(visual.plane_bgra())
            .map_err(|err| format!("failed to write cursor bo: {err}"))
    }
}

fn open_card(card_path: Option<&str>) -> Result<Card, String> {
    let card_path = card_path.unwrap_or("/dev/dri/card0");

    let fd = OpenOptions::new()
        .read(true)
        .write(true)
        .open(card_path)
        .map_err(|e| format!("failed to open {card_path}: {e}"))?;

    Ok(Card(fd))
}

fn sleep_with_stop(stop: &Arc<AtomicBool>, duration: Duration) {
    let deadline = Instant::now() + duration;

    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now >= deadline {
            break;
        }

        std::thread::sleep((deadline - now).min(Duration::from_millis(25)));
    }
}

fn release_master_lock(card: &Card) {
    if let Err(err) = card.release_master_lock() {
        eprintln!("DRM master release failed: {err}");
    }
}

fn handle_startup_failure_with_card(
    card: &Card,
    startup_tx: &mut Option<StartupSender<Result<(), String>>>,
    running_flag: &Arc<AtomicBool>,
    stop: &Arc<AtomicBool>,
    retries_remaining: &mut u32,
    retry_interval: Duration,
    message: String,
) -> bool {
    release_master_lock(card);

    handle_startup_failure(
        startup_tx,
        running_flag,
        stop,
        retries_remaining,
        retry_interval,
        message,
    )
}

fn handle_startup_failure(
    startup_tx: &mut Option<StartupSender<Result<(), String>>>,
    running_flag: &Arc<AtomicBool>,
    stop: &Arc<AtomicBool>,
    retries_remaining: &mut u32,
    retry_interval: Duration,
    message: String,
) -> bool {
    if startup_tx.is_none() {
        eprintln!("DRM backend unavailable: {message}");
        sleep_with_stop(stop, retry_interval);

        if stop.load(Ordering::Relaxed) {
            running_flag.store(false, Ordering::Relaxed);
            return true;
        }

        return false;
    }

    if *retries_remaining == 0 {
        let final_message = format!("DRM backend unavailable: {message}");
        eprintln!("{final_message}");

        if let Some(startup_tx) = startup_tx.take() {
            let _ = startup_tx.send(Err(final_message));
        }

        running_flag.store(false, Ordering::Relaxed);
        return true;
    }

    *retries_remaining -= 1;
    eprintln!(
        "DRM backend unavailable: {message} (retrying, {} attempts left)",
        *retries_remaining
    );
    sleep_with_stop(stop, retry_interval);

    if stop.load(Ordering::Relaxed) {
        if let Some(startup_tx) = startup_tx.take() {
            let _ = startup_tx.send(Err("DRM startup aborted".to_string()));
        }

        running_flag.store(false, Ordering::Relaxed);
        return true;
    }

    false
}

fn mode_blob_id(mode_blob: &property::Value<'static>) -> Option<u64> {
    match mode_blob {
        property::Value::Blob(blob) if *blob != 0 => Some(*blob),
        _ => None,
    }
}

fn destroy_mode_blob(card: &Card, blob_id: Option<u64>) {
    if let Some(blob_id) = blob_id {
        let _ = card.destroy_property_blob(blob_id);
    }
}

fn destroy_framebuffers(card: &Card, framebuffer_cache: &mut HashMap<u32, framebuffer::Handle>) {
    for (_, framebuffer) in framebuffer_cache.drain() {
        let _ = card.destroy_framebuffer(framebuffer);
    }
}

fn destroy_session_resources(
    card: &Card,
    cursor_plane: Option<CursorPlane>,
    framebuffer_cache: &mut HashMap<u32, framebuffer::Handle>,
    mode_blob_id: Option<u64>,
) {
    if let Some(cursor_plane) = cursor_plane {
        let _ = card.destroy_framebuffer(cursor_plane.commit.fb);
    }

    destroy_framebuffers(card, framebuffer_cache);
    destroy_mode_blob(card, mode_blob_id);
}

fn teardown_drm_output(
    card: &Card,
    output: DrmOutputConfig<'_>,
    cursor_plane: Option<&CursorPlaneCommit>,
) -> Result<(), String> {
    let mut req = atomic::AtomicModeReq::new();

    if let Some(cursor_plane) = cursor_plane {
        if let Ok(fb_handle) = prop_handle(&cursor_plane.props, "FB_ID") {
            req.add_property(
                cursor_plane.handle,
                fb_handle,
                property::Value::Framebuffer(None),
            );
        }

        if let Ok(crtc_prop) = prop_handle(&cursor_plane.props, "CRTC_ID") {
            req.add_property(cursor_plane.handle, crtc_prop, property::Value::CRTC(None));
        }
    }

    req.add_property(
        output.plane,
        prop_handle(output.plane_props, "FB_ID")?,
        property::Value::Framebuffer(None),
    );
    req.add_property(
        output.plane,
        prop_handle(output.plane_props, "CRTC_ID")?,
        property::Value::CRTC(None),
    );
    req.add_property(
        output.connector,
        prop_handle(output.con_props, "CRTC_ID")?,
        property::Value::CRTC(None),
    );
    req.add_property(
        output.crtc_handle,
        prop_handle(output.crtc_props, "ACTIVE")?,
        property::Value::Boolean(false),
    );

    if let Ok(mode_handle) = prop_handle(output.crtc_props, "MODE_ID") {
        req.add_property(output.crtc_handle, mode_handle, property::Value::Blob(0));
    }

    card.atomic_commit(AtomicCommitFlags::ALLOW_MODESET, req)
        .map_err(|e| format!("tearing down DRM output failed: {e}"))
}

fn cleanup_active_session(
    card: &Card,
    output: DrmOutputConfig<'_>,
    cursor_plane: Option<CursorPlane>,
    framebuffer_cache: &mut HashMap<u32, framebuffer::Handle>,
    mode_blob_id: Option<u64>,
) {
    if let Err(err) =
        teardown_drm_output(card, output, cursor_plane.as_ref().map(CursorPlane::commit))
    {
        eprintln!("DRM teardown failed: {err}");
    }

    destroy_session_resources(card, cursor_plane, framebuffer_cache, mode_blob_id);
    release_master_lock(card);
}

fn mode_distance(mode: &control::Mode, requested: (u32, u32)) -> i64 {
    let (width, height) = mode.size();
    let dx = width as i64 - requested.0 as i64;
    let dy = height as i64 - requested.1 as i64;
    dx * dx + dy * dy
}

fn mode_area(mode: &control::Mode) -> i64 {
    let (width, height) = mode.size();
    width as i64 * height as i64
}

fn mode_is_preferred(mode: &control::Mode) -> bool {
    mode.mode_type().contains(control::ModeTypeFlags::PREFERRED)
}

fn preferred_size(modes: &[control::Mode]) -> Option<(u32, u32)> {
    modes
        .iter()
        .find(|mode| mode_is_preferred(mode))
        .map(|mode| {
            let (width, height) = mode.size();
            (width as u32, height as u32)
        })
}

fn choose_mode(
    modes: &[control::Mode],
    requested: Option<(u32, u32)>,
) -> Result<control::Mode, String> {
    let first = modes
        .first()
        .cloned()
        .ok_or_else(|| "connector has no modes".to_string())?;

    let target_size = requested.or_else(|| preferred_size(modes));
    let mut best = first;
    let mut best_score = score_mode(&best, target_size);

    for mode in modes.iter().skip(1) {
        let score = score_mode(mode, target_size);
        if score < best_score {
            best = *mode;
            best_score = score;
        }
    }

    Ok(best)
}

fn score_mode(mode: &control::Mode, target_size: Option<(u32, u32)>) -> (i64, i32, i32, i64) {
    let distance = target_size
        .map(|size| mode_distance(mode, size))
        .unwrap_or(0);
    let refresh = -(mode.vrefresh() as i32);
    let preferred = if mode_is_preferred(mode) { 0 } else { 1 };
    let area = -mode_area(mode);
    (distance, refresh, preferred, area)
}

fn first_connected_connector(
    card: &Card,
    resources: &ResourceHandles,
    requested: Option<(u32, u32)>,
) -> Result<
    (
        connector::Handle,
        control::Mode,
        crtc::Handle,
        encoder::Handle,
    ),
    String,
> {
    let mut last_error = None;

    for handle in resources.connectors() {
        let info = card
            .get_connector(*handle, false)
            .map_err(|e| format!("failed to read connector {handle:?}: {e}"))?;

        if info.state() != connector::State::Connected {
            continue;
        }

        let mode = match choose_mode(info.modes(), requested) {
            Ok(mode) => mode,
            Err(err) => {
                last_error = Some(format!("connector {handle:?} {err}"));
                continue;
            }
        };

        match pick_encoder_and_crtc(card, resources, &info) {
            Ok((encoder, crtc)) => return Ok((*handle, mode, crtc, encoder)),
            Err(err) => last_error = Some(err),
        }
    }

    if let Some(err) = last_error {
        Err(err)
    } else {
        Err("no connected DRM connectors found".into())
    }
}

fn pick_encoder_and_crtc(
    card: &Card,
    resources: &ResourceHandles,
    connector_info: &connector::Info,
) -> Result<(encoder::Handle, crtc::Handle), String> {
    let mut encoder_handles = Vec::new();

    if let Some(current_encoder) = connector_info.current_encoder() {
        encoder_handles.push(current_encoder);
    }

    for encoder_handle in connector_info.encoders() {
        if !encoder_handles.contains(encoder_handle) {
            encoder_handles.push(*encoder_handle);
        }
    }

    for encoder_handle in encoder_handles {
        let encoder_info = card
            .get_encoder(encoder_handle)
            .map_err(|e| format!("failed to read encoder {encoder_handle:?}: {e}"))?;

        if let Some(crtc_handle) = encoder_info.crtc() {
            return Ok((encoder_handle, crtc_handle));
        }

        if let Some(crtc_handle) = resources
            .filter_crtcs(encoder_info.possible_crtcs())
            .first()
            .copied()
        {
            return Ok((encoder_handle, crtc_handle));
        }
    }

    Err(format!(
        "connector {:?} has no usable encoder/CRTC pair",
        connector_info.handle()
    ))
}

fn is_primary_plane(card: &Card, plane: plane::Handle) -> Result<bool, String> {
    let props = card
        .get_properties(plane)
        .map_err(|e| format!("failed to get plane properties: {e}"))?;
    for (&id, &val) in props.iter() {
        let info = card
            .get_property(id)
            .map_err(|e| format!("failed to read property info: {e}"))?;
        if info
            .name()
            .to_str()
            .map(|name| name == "type")
            .unwrap_or(false)
        {
            return Ok(val == u64::from(PlaneType::Primary as u32));
        }
    }
    Ok(false)
}

fn is_cursor_plane(card: &Card, plane: plane::Handle) -> Result<bool, String> {
    let props = card
        .get_properties(plane)
        .map_err(|e| format!("failed to get plane properties: {e}"))?;
    for (&id, &val) in props.iter() {
        let info = card
            .get_property(id)
            .map_err(|e| format!("failed to read property info: {e}"))?;
        if info
            .name()
            .to_str()
            .map(|name| name == "type")
            .unwrap_or(false)
        {
            return Ok(val == u64::from(PlaneType::Cursor as u32));
        }
    }
    Ok(false)
}

fn find_primary_plane(
    card: &Card,
    resources: &ResourceHandles,
    crtc_handle: crtc::Handle,
) -> Result<plane::Handle, String> {
    let planes = card
        .plane_handles()
        .map_err(|e| format!("could not list planes: {e}"))?;
    let mut compatible = Vec::new();
    let mut primary = Vec::new();

    for plane in planes {
        let info = card
            .get_plane(plane)
            .map_err(|e| format!("failed to read plane info: {e}"))?;
        let compatible_crtcs = resources.filter_crtcs(info.possible_crtcs());
        if !compatible_crtcs.contains(&crtc_handle) {
            continue;
        }
        compatible.push(plane);
        if is_primary_plane(card, plane)? {
            primary.push(plane);
        }
    }

    primary
        .first()
        .copied()
        .or_else(|| compatible.first().copied())
        .ok_or_else(|| "no compatible planes found".to_string())
}

fn find_cursor_plane(
    card: &Card,
    resources: &ResourceHandles,
    crtc_handle: crtc::Handle,
) -> Result<Option<plane::Handle>, String> {
    let planes = card
        .plane_handles()
        .map_err(|e| format!("could not list planes: {e}"))?;
    let mut compatible = Vec::new();

    for plane in planes {
        let info = card
            .get_plane(plane)
            .map_err(|e| format!("failed to read plane info: {e}"))?;
        let compatible_crtcs = resources.filter_crtcs(info.possible_crtcs());
        if !compatible_crtcs.contains(&crtc_handle) {
            continue;
        }
        if is_cursor_plane(card, plane)? {
            compatible.push(plane);
        }
    }

    Ok(compatible.first().copied())
}

fn prop_handle(
    props: &HashMap<String, property::Info>,
    name: &str,
) -> Result<property::Handle, String> {
    props
        .get(name)
        .map(|info| info.handle())
        .ok_or_else(|| format!("missing property {name}"))
}

fn create_cursor_plane<T: AsFd>(
    card: &Card,
    gbm_device: &GbmDevice<T>,
    resources: &ResourceHandles,
    crtc_handle: crtc::Handle,
    theme: &DrmCursorTheme,
) -> Result<Option<CursorPlane>, String> {
    let Some(handle) = find_cursor_plane(card, resources, crtc_handle)? else {
        return Ok(None);
    };
    let props = card
        .get_properties(handle)
        .and_then(|props| props.as_hashmap(card))
        .map_err(|e| format!("failed to read cursor plane properties: {e}"))?;
    let props = Arc::new(props);

    let size = CURSOR_PLANE_SIZE;
    let mut bo = gbm_device
        .create_buffer_object(
            size.0,
            size.1,
            GbmFormat::Argb8888,
            BufferObjectFlags::CURSOR | BufferObjectFlags::WRITE | BufferObjectFlags::LINEAR,
        )
        .map_err(|e| format!("failed to create cursor bo: {e}"))?;

    bo.write(theme.cursor(CursorIcon::Default).plane_bgra())
        .map_err(|e| format!("failed to write cursor bo: {e}"))?;

    let fb = card
        .add_framebuffer(&bo, 32, 32)
        .map_err(|e| format!("failed to create cursor fb: {e}"))?;

    Ok(Some(CursorPlane {
        commit: CursorPlaneCommit {
            handle,
            props,
            fb,
            size,
        },
        bo,
    }))
}

fn cursor_plane_position(
    cursor: CursorState,
    plane_size: (u32, u32),
    hotspot: (f32, f32),
    screen_size: (u32, u32),
) -> Option<(i64, i64)> {
    if !cursor.visible {
        return None;
    }

    let (screen_w, screen_h) = screen_size;
    let min_x = -(plane_size.0 as i64) + 1;
    let min_y = -(plane_size.1 as i64) + 1;
    let max_x = screen_w.saturating_sub(1) as i64;
    let max_y = screen_h.saturating_sub(1) as i64;
    let x = (cursor.pos.0 - hotspot.0).round() as i64;
    let y = (cursor.pos.1 - hotspot.1).round() as i64;
    let x = x.clamp(min_x, max_x);
    let y = y.clamp(min_y, max_y);
    Some((x, y))
}

fn add_cursor_plane_properties(
    req: &mut atomic::AtomicModeReq,
    crtc_handle: crtc::Handle,
    plane: &CursorPlaneCommit,
    cursor: CursorState,
    visual: &CursorVisual,
    screen_size: (u32, u32),
) -> Result<(), String> {
    if let Some((x, y)) = cursor_plane_position(cursor, plane.size, visual.hotspot(), screen_size) {
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "FB_ID")?,
            property::Value::Framebuffer(Some(plane.fb)),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "CRTC_ID")?,
            property::Value::CRTC(Some(crtc_handle)),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "CRTC_X")?,
            property::Value::SignedRange(x),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "CRTC_Y")?,
            property::Value::SignedRange(y),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "CRTC_W")?,
            property::Value::UnsignedRange(plane.size.0 as u64),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "CRTC_H")?,
            property::Value::UnsignedRange(plane.size.1 as u64),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "SRC_X")?,
            property::Value::UnsignedRange(0),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "SRC_Y")?,
            property::Value::UnsignedRange(0),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "SRC_W")?,
            property::Value::UnsignedRange((plane.size.0 as u64) << 16),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "SRC_H")?,
            property::Value::UnsignedRange((plane.size.1 as u64) << 16),
        );
    } else {
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "FB_ID")?,
            property::Value::Framebuffer(None),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "CRTC_ID")?,
            property::Value::CRTC(None),
        );
    }

    Ok(())
}

fn send_animation_pulse(
    tree_tx: &Sender<TreeMsg>,
    presented_at: Instant,
    predicted_next_present_at: Instant,
    log_render: bool,
) -> bool {
    let msg = TreeMsg::AnimationPulse {
        presented_at,
        predicted_next_present_at,
    };

    match tree_tx.try_send(msg) {
        Ok(()) => true,
        Err(TrySendError::Full(msg)) => {
            if log_render {
                eprintln!("tree channel full, blocking send");
            }
            tree_tx.send(msg).is_ok()
        }
        Err(TrySendError::Disconnected(_)) => false,
    }
}

fn send_present_timing(
    event_tx: &Sender<EventMsg>,
    presented_at: Instant,
    predicted_next_present_at: Instant,
) {
    let msg = EventMsg::PresentTiming {
        presented_at,
        predicted_next_present_at,
    };

    match event_tx.try_send(msg) {
        Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
    }
}

fn should_defer_cursor_only_commit(
    submit_primary: bool,
    submit_cursor: bool,
    follow_up_primary_until: Option<Instant>,
    now: Instant,
) -> bool {
    submit_cursor
        && !submit_primary
        && follow_up_primary_until
            .map(|deadline| now < deadline)
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drm_present_state_updates_estimated_interval_from_observed_presents() {
        let start = Instant::now();
        let mut present = DrmPresentState::new(Duration::from_millis(16));

        let first_predicted = present.observe_present(start);
        assert_eq!(first_predicted, start + Duration::from_millis(16));

        let second_presented = start + Duration::from_millis(20);
        let second_predicted = present.observe_present(second_presented);
        assert_eq!(
            second_predicted,
            second_presented + Duration::from_millis(20)
        );
    }

    #[test]
    fn defer_cursor_only_commit_requires_active_follow_up_window() {
        let now = Instant::now();
        assert!(should_defer_cursor_only_commit(
            false,
            true,
            Some(now + FOLLOW_UP_PRIMARY_WINDOW),
            now,
        ));
    }

    #[test]
    fn defer_cursor_only_commit_never_blocks_primary_work() {
        let now = Instant::now();
        assert!(!should_defer_cursor_only_commit(
            true,
            true,
            Some(now + FOLLOW_UP_PRIMARY_WINDOW),
            now,
        ));
    }

    #[test]
    fn defer_cursor_only_commit_expires_with_deadline() {
        let now = Instant::now();
        assert!(!should_defer_cursor_only_commit(
            false,
            true,
            Some(now),
            now,
        ));
        assert!(!should_defer_cursor_only_commit(false, true, None, now));
    }

    #[test]
    fn cursor_plane_position_clamps_visible_cursor_to_screen_bounds() {
        let position = cursor_plane_position(
            CursorState {
                pos: (-20.0, 200.0),
                visible: true,
            },
            (64, 64),
            (7.0, 2.0),
            (128, 128),
        );

        assert_eq!(position, Some((-27, 127)));
    }

    #[test]
    fn cursor_plane_position_accounts_for_hotspot_offset() {
        let position = cursor_plane_position(
            CursorState {
                pos: (40.0, 24.0),
                visible: true,
            },
            (64, 64),
            (7.0, 2.0),
            (128, 128),
        );

        assert_eq!(position, Some((33, 22)));
    }

    #[test]
    fn cursor_plane_position_returns_none_when_hidden() {
        let position = cursor_plane_position(
            CursorState {
                pos: (10.0, 20.0),
                visible: false,
            },
            (64, 64),
            (11.5, 11.5),
            (128, 128),
        );

        assert_eq!(position, None);
    }
}

fn add_plane_properties(
    req: &mut atomic::AtomicModeReq,
    plane: plane::Handle,
    plane_props: &HashMap<String, property::Info>,
    crtc_handle: crtc::Handle,
    fb: framebuffer::Handle,
) -> Result<(), String> {
    req.add_property(
        plane,
        prop_handle(plane_props, "FB_ID")?,
        property::Value::Framebuffer(Some(fb)),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "CRTC_ID")?,
        property::Value::CRTC(Some(crtc_handle)),
    );
    Ok(())
}

fn add_plane_geometry(
    req: &mut atomic::AtomicModeReq,
    plane: plane::Handle,
    plane_props: &HashMap<String, property::Info>,
    mode: &control::Mode,
) -> Result<(), String> {
    let (width, height) = mode.size();
    req.add_property(
        plane,
        prop_handle(plane_props, "SRC_X")?,
        property::Value::UnsignedRange(0),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "SRC_Y")?,
        property::Value::UnsignedRange(0),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "SRC_W")?,
        property::Value::UnsignedRange((width as u64) << 16),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "SRC_H")?,
        property::Value::UnsignedRange((height as u64) << 16),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "CRTC_X")?,
        property::Value::SignedRange(0),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "CRTC_Y")?,
        property::Value::SignedRange(0),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "CRTC_W")?,
        property::Value::UnsignedRange(width as u64),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "CRTC_H")?,
        property::Value::UnsignedRange(height as u64),
    );
    Ok(())
}

fn is_ebusy(err: &str) -> bool {
    err.contains("Device or resource busy") || err.contains("EBUSY")
}

fn load_egl() -> Result<(Library, egl::Egl), String> {
    let lib = unsafe { Library::new("libEGL.so.1") }
        .map_err(|e| format!("failed to load libEGL: {e}"))?;
    let get_proc = unsafe {
        lib.get::<unsafe extern "system" fn(*const std::ffi::c_char) -> *const c_void>(
            b"eglGetProcAddress\0",
        )
        .map_err(|e| format!("failed to load eglGetProcAddress: {e}"))?
    };

    let egl = egl::Egl::load_with(|name| unsafe {
        let symbol = CString::new(name).expect("egl symbol");
        let ptr = get_proc(symbol.as_ptr());
        if !ptr.is_null() {
            return ptr;
        }
        let raw = format!("{name}\0");
        lib.get::<*const c_void>(raw.as_bytes())
            .map(|s| *s)
            .unwrap_or(ptr::null())
    });

    Ok((lib, egl))
}

fn egl_get_platform_display(egl: &egl::Egl, display_ptr: *mut c_void) -> EGLDisplay {
    if egl.GetPlatformDisplayEXT.is_loaded() {
        unsafe { egl.GetPlatformDisplayEXT(EGL_PLATFORM_GBM_KHR, display_ptr, ptr::null()) }
    } else if egl.GetPlatformDisplay.is_loaded() {
        unsafe { egl.GetPlatformDisplay(EGL_PLATFORM_GBM_KHR, display_ptr, ptr::null()) }
    } else {
        unsafe { egl.GetDisplay(display_ptr as egl::EGLNativeDisplayType) }
    }
}

fn init_egl(
    egl: &egl::Egl,
    gbm_device_ptr: *mut c_void,
    gbm_surface_ptr: *mut c_void,
) -> Result<(EGLDisplay, EGLContext, EGLSurface), String> {
    let display = egl_get_platform_display(egl, gbm_device_ptr);
    if display == egl::NO_DISPLAY {
        return Err("failed to get EGL display".to_string());
    }

    let mut major: EGLint = 0;
    let mut minor: EGLint = 0;
    if unsafe { egl.Initialize(display, &mut major, &mut minor) } == egl::FALSE {
        return Err("failed to initialize EGL".to_string());
    }

    if unsafe { egl.BindAPI(egl::OPENGL_ES_API) } == egl::FALSE {
        return Err("failed to bind EGL OpenGL ES API".to_string());
    }

    let config_attribs: [EGLint; 13] = [
        egl::SURFACE_TYPE as EGLint,
        egl::WINDOW_BIT as EGLint,
        egl::RENDERABLE_TYPE as EGLint,
        egl::OPENGL_ES2_BIT as EGLint,
        egl::RED_SIZE as EGLint,
        8,
        egl::GREEN_SIZE as EGLint,
        8,
        egl::BLUE_SIZE as EGLint,
        8,
        egl::ALPHA_SIZE as EGLint,
        8,
        egl::NONE as EGLint,
    ];

    let mut config: EGLConfig = ptr::null();
    let mut num_configs: EGLint = 0;
    if unsafe {
        egl.ChooseConfig(
            display,
            config_attribs.as_ptr(),
            &mut config,
            1,
            &mut num_configs,
        )
    } == egl::FALSE
        || num_configs == 0
    {
        return Err("failed to choose EGL config".to_string());
    }

    let context_attribs: [EGLint; 3] = [
        egl::CONTEXT_CLIENT_VERSION as EGLint,
        2,
        egl::NONE as EGLint,
    ];
    let context =
        unsafe { egl.CreateContext(display, config, egl::NO_CONTEXT, context_attribs.as_ptr()) };
    if context == egl::NO_CONTEXT {
        return Err("failed to create EGL context".to_string());
    }

    let surface = unsafe {
        egl.CreateWindowSurface(
            display,
            config,
            gbm_surface_ptr as egl::EGLNativeWindowType,
            ptr::null(),
        )
    };
    if surface == egl::NO_SURFACE {
        return Err("failed to create EGL surface".to_string());
    }

    if unsafe { egl.MakeCurrent(display, surface, surface, context) } == egl::FALSE {
        return Err("failed to make EGL context current".to_string());
    }

    unsafe {
        egl.SwapInterval(display, 1);
    }

    Ok((display, context, surface))
}

fn create_frame_surface(egl: &egl::Egl, dimensions: (u32, u32)) -> Result<GlFrameSurface, String> {
    gl::load_with(|s| unsafe {
        let symbol = CString::new(s).expect("gl symbol");
        egl.GetProcAddress(symbol.as_ptr()) as *const _
    });

    let interface = skia_safe::gpu::gl::Interface::new_load_with(|name| unsafe {
        if name == "eglGetCurrentDisplay" {
            return ptr::null();
        }
        let symbol = CString::new(name).expect("egl symbol");
        egl.GetProcAddress(symbol.as_ptr()) as *const _
    })
    .ok_or_else(|| "could not create Skia GL interface".to_string())?;

    let gr_context = skia_safe::gpu::direct_contexts::make_gl(interface, None)
        .ok_or_else(|| "make_gl failed: could not create Skia direct context".to_string())?;

    let fb_info = {
        let mut fboid: i32 = 0;
        unsafe { gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut fboid) };

        FramebufferInfo {
            fboid: fboid as u32,
            format: skia_safe::gpu::gl::Format::RGBA8.into(),
            ..Default::default()
        }
    };

    Ok(GlFrameSurface::new(dimensions, fb_info, gr_context, 0, 0))
}

fn framebuffer_for_bo(
    card: &Card,
    cache: &mut HashMap<u32, framebuffer::Handle>,
    bo: &BufferObject<()>,
) -> Result<framebuffer::Handle, String> {
    let handle = unsafe { bo.handle().u32_ };
    if let Some(existing) = cache.get(&handle).copied() {
        return Ok(existing);
    }

    let framebuffer = card
        .add_framebuffer(bo, 24, 32)
        .map_err(|e| format!("failed to create framebuffer: {e}"))?;
    cache.insert(handle, framebuffer);
    Ok(framebuffer)
}

fn prepare_primary_frame(
    generation: u64,
    context: PrimaryFrameContext<'_>,
) -> Result<PreparedPrimaryFrame, String> {
    let PrimaryFrameContext {
        renderer,
        frame_surface,
        render_state,
        cursor,
        video_registry,
        video_import,
        egl_state,
        gbm_surface,
        card,
        framebuffer_cache,
    } = context;

    let mut video_needs_cleanup = false;
    {
        let mut frame = frame_surface.frame();
        match renderer.sync_video_frames(&mut frame, video_registry, video_import) {
            Ok(result) => video_needs_cleanup = result.needs_cleanup,
            Err(err) => eprintln!("video sync failed: {err}"),
        }
        renderer.render(&mut frame, render_state);
        if !cursor.hw_cursor_enabled && cursor.visible {
            draw_software_cursor(
                renderer,
                &mut frame,
                cursor.theme.cursor(cursor.icon),
                cursor.pos,
            );
        }
    }

    if unsafe {
        egl_state
            .egl
            .SwapBuffers(egl_state.display, egl_state.surface)
    } == egl::FALSE
    {
        return Err("eglSwapBuffers failed".to_string());
    }

    let bo = unsafe { gbm_surface.lock_front_buffer() }
        .map_err(|e| format!("locking GBM buffer failed: {e}"))?;
    let fb = framebuffer_for_bo(card, framebuffer_cache, &bo)?;

    Ok(PreparedPrimaryFrame {
        generation,
        render_version: render_state.render_version,
        bo,
        fb,
        video_needs_cleanup,
    })
}

fn draw_software_cursor(
    renderer: &mut SceneRenderer,
    frame: &mut crate::renderer::RenderFrame<'_>,
    visual: &CursorVisual,
    cursor_pos: (f32, f32),
) {
    let (cursor_width, cursor_height) = visual.size();
    let hotspot = visual.hotspot();
    let x = cursor_pos.0 - hotspot.0;
    let y = cursor_pos.1 - hotspot.1;
    let canvas = frame.surface_mut().canvas();
    let sampling =
        skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::None);
    let paint = Paint::default();
    let dst = Rect::from_xywh(x, y, cursor_width as f32, cursor_height as f32);
    canvas.draw_image_rect_with_sampling_options(visual.image(), None, dst, sampling, &paint);

    renderer.flush(frame);
}

#[derive(Clone)]
pub struct DrmRunConfig {
    pub requested_size: Option<(u32, u32)>,
    pub card_path: Option<String>,
    pub asset_config: AssetConfig,
    pub startup_retries: u32,
    pub cursor_overrides: Vec<DrmCursorOverrideConfig>,
    pub retry_interval_ms: u32,
    pub hw_cursor: bool,
    pub render_log: bool,
}

pub struct DrmRunContext {
    pub startup_tx: StartupSender<Result<(), String>>,
    pub stop: Arc<AtomicBool>,
    pub running_flag: Arc<AtomicBool>,
    pub presenter_wake: EventFd,
    pub input_wake: EventFd,
    pub tree_tx: Sender<TreeMsg>,
    pub render_rx: Receiver<RenderMsg>,
    pub cursor_icon_rx: Receiver<CursorIcon>,
    pub cursor_state: Arc<SharedCursorState>,
    pub event_tx: Sender<EventMsg>,
    pub screen_tx: Sender<(u32, u32)>,
    pub render_counter: Arc<AtomicU64>,
    pub native_log: Arc<NativeLogRelay>,
    pub stats: Option<Arc<RendererStatsCollector>>,
    pub video_registry: Arc<VideoRegistry>,
}

pub fn run(context: DrmRunContext, config: DrmRunConfig) {
    let DrmRunContext {
        startup_tx,
        stop,
        running_flag,
        presenter_wake,
        input_wake,
        tree_tx,
        render_rx,
        cursor_icon_rx,
        cursor_state,
        event_tx,
        screen_tx,
        render_counter,
        native_log,
        stats,
        video_registry,
    } = context;

    let log_render = config.render_log;
    let mut startup_tx = Some(startup_tx);
    let retry_interval = Duration::from_millis(config.retry_interval_ms as u64);
    let mut startup_retries_remaining = config.startup_retries;
    let mut last_dimensions: Option<(u32, u32)> = None;
    let hotplug_interval = Duration::from_millis(750);
    let mut logged_cursor_info = false;
    let mut logged_mode_info = false;
    let cursor_theme = match DrmCursorTheme::load(&config.asset_config, &config.cursor_overrides) {
        Ok(theme) => theme,
        Err(err) => {
            if let Some(startup_tx) = startup_tx.take() {
                let _ = startup_tx.send(Err(format!("DRM cursor setup failed: {err}")));
            }
            running_flag.store(false, Ordering::Relaxed);
            return;
        }
    };

    loop {
        if stop.load(Ordering::Relaxed) {
            if let Some(startup_tx) = startup_tx.take() {
                let _ = startup_tx.send(Err("DRM startup aborted".to_string()));
            }
            running_flag.store(false, Ordering::Relaxed);
            break;
        }

        let card = match open_card(config.card_path.as_deref()) {
            Ok(card) => card,
            Err(err) => {
                if handle_startup_failure(
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    err,
                ) {
                    break;
                }

                continue;
            }
        };

        let card = Arc::new(card);

        if let Err(err) = card.acquire_master_lock() {
            if handle_startup_failure(
                &mut startup_tx,
                &running_flag,
                &stop,
                &mut startup_retries_remaining,
                retry_interval,
                format!("acquiring DRM master failed: {err}"),
            ) {
                break;
            }

            continue;
        }

        if let Err(err) = card.set_client_capability(ClientCapability::UniversalPlanes, true) {
            if handle_startup_failure_with_card(
                &card,
                &mut startup_tx,
                &running_flag,
                &stop,
                &mut startup_retries_remaining,
                retry_interval,
                format!("enabling universal planes failed: {err}"),
            ) {
                break;
            }

            continue;
        }

        if let Err(err) = card.set_client_capability(ClientCapability::Atomic, true) {
            if handle_startup_failure_with_card(
                &card,
                &mut startup_tx,
                &running_flag,
                &stop,
                &mut startup_retries_remaining,
                retry_interval,
                format!("enabling atomic modesetting failed: {err}"),
            ) {
                break;
            }

            continue;
        }

        let gbm_device = match GbmDevice::new(card.as_fd()) {
            Ok(device) => device,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("creating GBM device failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let resources = match card.resource_handles() {
            Ok(handles) => handles,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("querying DRM resources failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let (connector, mode, crtc_handle, encoder_handle) =
            match first_connected_connector(&card, &resources, config.requested_size) {
                Ok(values) => values,
                Err(err) => {
                    if handle_startup_failure_with_card(
                        &card,
                        &mut startup_tx,
                        &running_flag,
                        &stop,
                        &mut startup_retries_remaining,
                        retry_interval,
                        format!("selecting connector failed: {err}"),
                    ) {
                        break;
                    }

                    continue;
                }
            };

        let plane = match find_primary_plane(&card, &resources, crtc_handle) {
            Ok(handle) => handle,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("selecting primary plane failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let con_props = match card
            .get_properties(connector)
            .and_then(|props| props.as_hashmap(card.as_ref()))
        {
            Ok(props) => props,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("reading connector properties failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };
        let crtc_props = match card
            .get_properties(crtc_handle)
            .and_then(|props| props.as_hashmap(card.as_ref()))
        {
            Ok(props) => props,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("reading CRTC properties failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };
        let plane_props = match card
            .get_properties(plane)
            .and_then(|props| props.as_hashmap(card.as_ref()))
        {
            Ok(props) => props,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("reading plane properties failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let (width, height) = mode.size();
        let dimensions = (width as u32, height as u32);
        let refresh_hz = mode.vrefresh().max(1) as f64;
        let frame_interval = Duration::from_secs_f64(1.0 / refresh_hz);
        let _ = screen_tx.send(dimensions);
        let _ = input_wake.signal();
        if !logged_mode_info {
            native_log.info(
                "drm",
                format!(
                    "DRM resources: connector={} encoder={} crtc={} plane={}",
                    u32::from(connector),
                    u32::from(encoder_handle),
                    u32::from(crtc_handle),
                    u32::from(plane)
                ),
            );
            native_log.info(
                "drm",
                format!(
                    "DRM mode: {}x{} @ {}Hz",
                    dimensions.0,
                    dimensions.1,
                    mode.vrefresh()
                ),
            );
            logged_mode_info = true;
        }
        if last_dimensions != Some(dimensions) {
            let _ = event_tx.send(EventMsg::InputEvent(InputEvent::Resized {
                width: dimensions.0,
                height: dimensions.1,
                scale_factor: 1.0,
            }));
            last_dimensions = Some(dimensions);
        }

        let mut cursor_plane = if config.hw_cursor {
            match create_cursor_plane(&card, &gbm_device, &resources, crtc_handle, &cursor_theme) {
                Ok(plane) => plane,
                Err(e) => {
                    native_log.warning("drm", format!("DRM cursor setup failed: {e}"));
                    None
                }
            }
        } else {
            None
        };
        if !logged_cursor_info {
            if config.hw_cursor {
                if cursor_plane.is_some() {
                    native_log.info("drm", "DRM cursor: hardware plane enabled");
                } else {
                    native_log.info("drm", "DRM cursor: hardware unavailable, using software");
                }
            } else {
                native_log.info("drm", "DRM cursor: hardware disabled, using software");
            }
            logged_cursor_info = true;
        }

        let gbm_surface: Surface<()> = match gbm_device.create_surface(
            dimensions.0,
            dimensions.1,
            GbmFormat::Xrgb8888,
            BufferObjectFlags::SCANOUT | BufferObjectFlags::RENDERING,
        ) {
            Ok(surface) => surface,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("creating GBM surface failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let (egl_lib, egl_api) = match load_egl() {
            Ok(values) => values,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("loading EGL failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let (display, context, surface) = match init_egl(
            &egl_api,
            gbm_device.as_raw() as *mut c_void,
            gbm_surface.as_raw() as *mut c_void,
        ) {
            Ok(values) => values,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("initializing EGL failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let egl_state = EglState {
            egl: egl_api,
            _egl_lib: egl_lib,
            display,
            _context: context,
            surface,
        };

        let mut frame_surface = match create_frame_surface(&egl_state.egl, dimensions) {
            Ok(frame_surface) => frame_surface,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("creating renderer failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };
        let mut renderer = SceneRenderer::new();
        let video_import = match VideoImportContext::new_current() {
            Ok(ctx) => Some(ctx),
            Err(err) => {
                eprintln!("prime video import unavailable: {err}");
                None
            }
        };

        let mode_blob = match card.create_property_blob(&mode) {
            Ok(blob) => blob,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("creating mode blob failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };
        let mode_blob_id = mode_blob_id(&mode_blob);

        let mut framebuffer_cache: HashMap<u32, framebuffer::Handle> = HashMap::new();

        let mut render_state = RenderState::default();
        {
            let mut frame = frame_surface.frame();
            renderer.render(&mut frame, &render_state);
        }

        if unsafe {
            egl_state
                .egl
                .SwapBuffers(egl_state.display, egl_state.surface)
        } == egl::FALSE
        {
            destroy_session_resources(
                &card,
                cursor_plane.take(),
                &mut framebuffer_cache,
                mode_blob_id,
            );

            if handle_startup_failure_with_card(
                &card,
                &mut startup_tx,
                &running_flag,
                &stop,
                &mut startup_retries_remaining,
                retry_interval,
                "eglSwapBuffers failed".to_string(),
            ) {
                break;
            }

            continue;
        }

        let bo = match unsafe { gbm_surface.lock_front_buffer() } {
            Ok(bo) => bo,
            Err(err) => {
                destroy_session_resources(
                    &card,
                    cursor_plane.take(),
                    &mut framebuffer_cache,
                    mode_blob_id,
                );

                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("locking first GBM buffer failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let fb = match framebuffer_for_bo(&card, &mut framebuffer_cache, &bo) {
            Ok(fb) => fb,
            Err(err) => {
                destroy_session_resources(
                    &card,
                    cursor_plane.take(),
                    &mut framebuffer_cache,
                    mode_blob_id,
                );

                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("creating framebuffer failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let mut atomic_req = atomic::AtomicModeReq::new();
        if let Err(e) = (|| -> Result<(), String> {
            atomic_req.add_property(
                connector,
                prop_handle(&con_props, "CRTC_ID")?,
                property::Value::CRTC(Some(crtc_handle)),
            );
            atomic_req.add_property(crtc_handle, prop_handle(&crtc_props, "MODE_ID")?, mode_blob);
            atomic_req.add_property(
                crtc_handle,
                prop_handle(&crtc_props, "ACTIVE")?,
                property::Value::Boolean(true),
            );
            add_plane_properties(&mut atomic_req, plane, &plane_props, crtc_handle, fb)?;
            add_plane_geometry(&mut atomic_req, plane, &plane_props, &mode)
        })() {
            drop(bo);
            destroy_session_resources(
                &card,
                cursor_plane.take(),
                &mut framebuffer_cache,
                mode_blob_id,
            );

            if handle_startup_failure_with_card(
                &card,
                &mut startup_tx,
                &running_flag,
                &stop,
                &mut startup_retries_remaining,
                retry_interval,
                format!("preparing initial atomic commit failed: {e}"),
            ) {
                break;
            }

            continue;
        }

        if let Err(err) = card.atomic_commit(AtomicCommitFlags::ALLOW_MODESET, atomic_req) {
            drop(bo);
            destroy_session_resources(
                &card,
                cursor_plane.take(),
                &mut framebuffer_cache,
                mode_blob_id,
            );

            if handle_startup_failure_with_card(
                &card,
                &mut startup_tx,
                &running_flag,
                &stop,
                &mut startup_retries_remaining,
                retry_interval,
                format!("initial atomic commit failed: {err}"),
            ) {
                break;
            }

            continue;
        }

        if let Some(startup_tx) = startup_tx.take() {
            let _ = startup_tx.send(Ok(()));
        }

        if log_render {
            eprintln!("drm present version={}", render_state.render_version);
        }

        let mut current_primary = CurrentPrimaryFrame {
            generation: 0,
            render_version: render_state.render_version,
            bo,
            fb,
        };
        let mut prepared_primary: Option<PreparedPrimaryFrame> = None;
        let mut in_flight: Option<InFlightCommit> = None;
        let mut desired_primary_generation = 1u64;
        let mut committed_primary_generation = 0u64;
        let mut cursor_snapshot = cursor_state.snapshot();
        let mut cursor_pos = cursor_snapshot.state.pos;
        let mut cursor_visible = cursor_snapshot.state.visible;
        let mut current_cursor_icon = CursorIcon::Default;
        let mut last_cursor_pos = cursor_pos;
        let mut last_cursor_visible = cursor_visible;
        let mut last_cursor_icon = current_cursor_icon;
        let mut hw_cursor_enabled = cursor_plane.is_some();
        let mut committed_cursor_version: Option<u64> = None;
        let mut committed_cursor_visible = false;
        let mut committed_cursor_icon: Option<CursorIcon> = None;
        let mut present_state = DrmPresentState::new(frame_interval);
        let mut follow_up_primary_until: Option<Instant> = None;
        let mut retry_commit_at: Option<Instant> = None;
        let mut drm_ready = false;
        let mut presenter_wake_ready = false;

        let mut next_hotplug_check = Instant::now() + hotplug_interval;
        let mut last_video_generation = video_registry.generation();
        let mut stop_requested = false;

        loop {
            if presenter_wake_ready {
                let _ = presenter_wake.drain();
                presenter_wake_ready = false;
            }

            if drm_ready {
                drm_ready = false;
                match card.receive_events() {
                    Ok(events) => {
                        for event in events {
                            if let control::Event::PageFlip(page_flip) = event {
                                if page_flip.crtc != crtc_handle {
                                    continue;
                                }

                                if let Some(submitted) = in_flight.take() {
                                    if let Some(frame) = submitted.primary {
                                        let old_primary = std::mem::replace(
                                            &mut current_primary,
                                            CurrentPrimaryFrame {
                                                generation: frame.generation,
                                                render_version: frame.render_version,
                                                bo: frame.bo,
                                                fb: frame.fb,
                                            },
                                        );
                                        drop(old_primary.bo);
                                        committed_primary_generation = current_primary.generation;
                                        if frame.video_needs_cleanup {
                                            desired_primary_generation =
                                                desired_primary_generation.wrapping_add(1);
                                        }
                                        if log_render {
                                            eprintln!(
                                                "drm present version={}",
                                                current_primary.render_version
                                            );
                                        }

                                        if let Some(stats) = stats.as_ref() {
                                            stats.record_frame_present();
                                        }

                                        let presented_at = Instant::now();
                                        let predicted_next_present_at =
                                            present_state.observe_present(presented_at);

                                        if let Some(stats) = stats.as_ref() {
                                            stats.record_display_interval(
                                                predicted_next_present_at
                                                    .saturating_duration_since(presented_at),
                                            );
                                        }

                                        send_present_timing(
                                            &event_tx,
                                            presented_at,
                                            predicted_next_present_at,
                                        );

                                        if submitted.emit_animation_pulse {
                                            if !send_animation_pulse(
                                                &tree_tx,
                                                presented_at,
                                                predicted_next_present_at,
                                                log_render,
                                            ) {
                                                stop_requested = true;
                                                break;
                                            }
                                            follow_up_primary_until =
                                                Some(presented_at + FOLLOW_UP_PRIMARY_WINDOW);
                                        }
                                    }

                                    if let Some(cursor) = submitted.cursor {
                                        committed_cursor_version = cursor.version;
                                        committed_cursor_visible = cursor.visible;
                                        committed_cursor_icon = Some(cursor.icon);
                                    }
                                }
                            }
                        }
                    }
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                    Err(err) => {
                        eprintln!(
                            "DRM backend unavailable: failed to receive page flip events: {err}"
                        );
                        break;
                    }
                }
            }

            if stop.load(Ordering::Relaxed) {
                stop_requested = true;
                break;
            }

            let now = Instant::now();
            if now >= next_hotplug_check {
                let resources = match card.resource_handles() {
                    Ok(handles) => handles,
                    Err(_) => break,
                };
                let next = first_connected_connector(&card, &resources, config.requested_size);
                match next {
                    Ok((next_connector, next_mode, next_crtc, _next_encoder)) => {
                        let next_dimensions = next_mode.size();
                        let next_dimensions = (next_dimensions.0 as u32, next_dimensions.1 as u32);
                        if next_connector != connector
                            || next_crtc != crtc_handle
                            || next_dimensions != dimensions
                        {
                            break;
                        }
                    }
                    Err(_) => break,
                }
                next_hotplug_check = Instant::now() + hotplug_interval;
            }

            while let Ok(msg) = render_rx.try_recv() {
                match msg {
                    RenderMsg::Scene {
                        scene,
                        version,
                        animate,
                        ..
                    } => {
                        render_state.scene = *scene;
                        render_state.render_version = version;
                        render_state.animate = animate;
                        desired_primary_generation = desired_primary_generation.wrapping_add(1);
                        follow_up_primary_until = None;
                        if log_render {
                            let latest = render_counter.load(Ordering::Relaxed);
                            let delta = latest.saturating_sub(version);
                            eprintln!("drm render version={version} latest={latest} delta={delta}");
                        }
                    }
                    RenderMsg::Stop => {
                        stop_requested = true;
                        break;
                    }
                }
            }

            if stop_requested {
                break;
            }

            if let Some(retry_at) = retry_commit_at
                && Instant::now() >= retry_at
            {
                retry_commit_at = None;
            }
            if let Some(deadline) = follow_up_primary_until
                && Instant::now() >= deadline
            {
                follow_up_primary_until = None;
            }

            while let Ok(icon) = cursor_icon_rx.try_recv() {
                current_cursor_icon = icon;
            }

            cursor_snapshot = cursor_state.snapshot();
            cursor_pos = cursor_snapshot.state.pos;
            cursor_visible = cursor_snapshot.state.visible;

            if !hw_cursor_enabled {
                if cursor_visible && cursor_pos != last_cursor_pos {
                    desired_primary_generation = desired_primary_generation.wrapping_add(1);
                }
                if cursor_visible != last_cursor_visible {
                    desired_primary_generation = desired_primary_generation.wrapping_add(1);
                }
                if cursor_visible && current_cursor_icon != last_cursor_icon {
                    desired_primary_generation = desired_primary_generation.wrapping_add(1);
                }
            }

            let video_generation = video_registry.generation();
            if video_generation != last_video_generation {
                desired_primary_generation = desired_primary_generation.wrapping_add(1);
                last_video_generation = video_generation;
            }

            last_cursor_pos = cursor_pos;
            last_cursor_visible = cursor_visible;
            last_cursor_icon = current_cursor_icon;

            let primary_dirty = desired_primary_generation != committed_primary_generation;
            if in_flight.is_none()
                && primary_dirty
                && prepared_primary.as_ref().map(|frame| frame.generation)
                    != Some(desired_primary_generation)
            {
                match prepare_primary_frame(
                    desired_primary_generation,
                    PrimaryFrameContext {
                        renderer: &mut renderer,
                        frame_surface: &mut frame_surface,
                        render_state: &render_state,
                        cursor: PrimaryFrameCursorContext {
                            pos: cursor_pos,
                            visible: cursor_visible,
                            hw_cursor_enabled,
                            icon: current_cursor_icon,
                            theme: &cursor_theme,
                        },
                        video_registry: &video_registry,
                        video_import: video_import.as_ref(),
                        egl_state: &egl_state,
                        gbm_surface: &gbm_surface,
                        card: &card,
                        framebuffer_cache: &mut framebuffer_cache,
                    },
                ) {
                    Ok(frame) => prepared_primary = Some(frame),
                    Err(err) => {
                        eprintln!("DRM backend unavailable: {err}");
                        break;
                    }
                }
            }

            let submit_primary = in_flight.is_none()
                && prepared_primary
                    .as_ref()
                    .map(|frame| {
                        frame.generation == desired_primary_generation
                            && frame.generation != committed_primary_generation
                    })
                    .unwrap_or(false);
            let submit_cursor = cursor_plane.is_some()
                && in_flight.is_none()
                && ((hw_cursor_enabled
                    && (committed_cursor_version != Some(cursor_snapshot.version)
                        || committed_cursor_icon != Some(current_cursor_icon)))
                    || (!hw_cursor_enabled && committed_cursor_visible));
            let now = Instant::now();
            let defer_cursor_only = should_defer_cursor_only_commit(
                submit_primary,
                submit_cursor,
                follow_up_primary_until,
                now,
            );
            if defer_cursor_only && log_render {
                eprintln!("drm defer cursor-only commit waiting for follow-up primary");
            }

            if submit_primary || (submit_cursor && !defer_cursor_only) {
                let mut commit_req = atomic::AtomicModeReq::new();
                let primary_fb = prepared_primary
                    .as_ref()
                    .filter(|_| submit_primary)
                    .map(|frame| frame.fb)
                    .unwrap_or(current_primary.fb);
                if let Err(err) = add_plane_properties(
                    &mut commit_req,
                    plane,
                    &plane_props,
                    crtc_handle,
                    primary_fb,
                ) {
                    eprintln!("DRM backend unavailable: {err}");
                    break;
                }

                let cursor_visual = cursor_theme.cursor(current_cursor_icon);

                if submit_cursor
                    && hw_cursor_enabled
                    && let Some(cursor_plane) = cursor_plane.as_mut()
                    && let Err(err) = cursor_plane.write_visual(cursor_visual)
                {
                    native_log.error(
                        "drm",
                        format!("DRM cursor setup failed during cursor upload: {err}"),
                    );
                    hw_cursor_enabled = false;
                    committed_cursor_visible = false;
                    committed_cursor_icon = None;
                    desired_primary_generation = desired_primary_generation.wrapping_add(1);
                    continue;
                }

                if submit_cursor && let Some(plane) = cursor_plane.as_ref().map(CursorPlane::commit)
                {
                    let cursor_for_commit = if hw_cursor_enabled {
                        cursor_snapshot.state
                    } else {
                        CursorState {
                            pos: cursor_pos,
                            visible: false,
                        }
                    };

                    if let Err(err) = add_cursor_plane_properties(
                        &mut commit_req,
                        crtc_handle,
                        plane,
                        cursor_for_commit,
                        cursor_visual,
                        dimensions,
                    ) {
                        if hw_cursor_enabled {
                            native_log.error(
                                "drm",
                                format!("DRM cursor setup failed during commit build: {err}"),
                            );
                            hw_cursor_enabled = false;
                            committed_cursor_visible = false;
                            committed_cursor_icon = None;
                            desired_primary_generation = desired_primary_generation.wrapping_add(1);
                            continue;
                        }
                        eprintln!("DRM backend unavailable: {err}");
                        break;
                    }
                }

                match card.atomic_commit(
                    AtomicCommitFlags::NONBLOCK | AtomicCommitFlags::PAGE_FLIP_EVENT,
                    commit_req,
                ) {
                    Ok(()) => {
                        retry_commit_at = None;
                        in_flight = Some(InFlightCommit {
                            primary: if submit_primary {
                                prepared_primary.take()
                            } else {
                                None
                            },
                            cursor: if submit_cursor {
                                Some(SubmittedCursorState {
                                    version: if hw_cursor_enabled {
                                        Some(cursor_snapshot.version)
                                    } else {
                                        None
                                    },
                                    visible: hw_cursor_enabled && cursor_snapshot.state.visible,
                                    icon: current_cursor_icon,
                                })
                            } else {
                                None
                            },
                            emit_animation_pulse: submit_primary && render_state.animate,
                        });
                    }
                    Err(err) => {
                        let err = err.to_string();
                        if is_ebusy(&err) {
                            if log_render {
                                eprintln!("drm atomic commit EBUSY, retrying staged state");
                            }
                            retry_commit_at = Some(Instant::now() + Duration::from_millis(1));
                            continue;
                        }

                        if submit_cursor && hw_cursor_enabled {
                            native_log.error(
                                "drm",
                                format!(
                                    "DRM cursor commit failed: {err}; falling back to software cursor"
                                ),
                            );

                            if let Some(cursor_plane_commit) =
                                cursor_plane.as_ref().map(CursorPlane::commit)
                            {
                                let mut hide_req = atomic::AtomicModeReq::new();
                                let _ = add_cursor_plane_properties(
                                    &mut hide_req,
                                    crtc_handle,
                                    cursor_plane_commit,
                                    CursorState {
                                        pos: cursor_pos,
                                        visible: false,
                                    },
                                    cursor_theme.cursor(current_cursor_icon),
                                    dimensions,
                                );
                                let _ = add_plane_properties(
                                    &mut hide_req,
                                    plane,
                                    &plane_props,
                                    crtc_handle,
                                    current_primary.fb,
                                );
                                let _ = card.atomic_commit(AtomicCommitFlags::empty(), hide_req);
                            }

                            hw_cursor_enabled = false;
                            committed_cursor_visible = false;
                            committed_cursor_icon = None;
                            desired_primary_generation = desired_primary_generation.wrapping_add(1);
                            continue;
                        }

                        eprintln!("DRM backend unavailable: {err}");
                        break;
                    }
                }
            }

            let mut next_deadline = Some(next_hotplug_check);
            if let Some(retry_at) = retry_commit_at {
                next_deadline = Some(
                    next_deadline
                        .map(|deadline| deadline.min(retry_at))
                        .unwrap_or(retry_at),
                );
            }
            if let Some(deadline) = follow_up_primary_until {
                next_deadline = Some(
                    next_deadline
                        .map(|current_deadline| current_deadline.min(deadline))
                        .unwrap_or(deadline),
                );
            }

            if in_flight.is_none()
                && (submit_primary || (submit_cursor && !defer_cursor_only))
                && retry_commit_at.is_none()
            {
                continue;
            }

            let timeout =
                next_deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
            let mut pollfds = [
                libc::pollfd {
                    fd: card.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: presenter_wake.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];

            match poll_fds(&mut pollfds, timeout) {
                Ok(_) => {
                    drm_ready = (pollfds[0].revents & (libc::POLLIN | libc::POLLPRI)) != 0;
                    presenter_wake_ready = (pollfds[1].revents & libc::POLLIN) != 0;
                    if (pollfds[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0
                    {
                        eprintln!("DRM backend unavailable: poll reported DRM fd error");
                        break;
                    }
                }
                Err(err) => {
                    eprintln!("DRM backend unavailable: poll failed: {err}");
                    break;
                }
            }
        }

        // Keep prepared and in-flight GBM buffers alive until teardown completes.
        // A blocking ALLOW_MODESET teardown is our final barrier before those
        // buffers can be released safely.
        cleanup_active_session(
            &card,
            DrmOutputConfig {
                connector,
                crtc_handle,
                plane,
                con_props: &con_props,
                crtc_props: &crtc_props,
                plane_props: &plane_props,
            },
            cursor_plane.take(),
            &mut framebuffer_cache,
            mode_blob_id,
        );

        if stop_requested {
            running_flag.store(false, Ordering::Relaxed);
            break;
        }
    }
}
