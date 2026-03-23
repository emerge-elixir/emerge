use std::collections::HashMap;
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::raw::c_void;
use std::ptr;
use std::sync::mpsc::Sender as StartupSender;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

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
use skia_safe::{Color, Paint, PaintStyle, gpu::gl::FramebufferInfo};

use crossbeam_channel::{Receiver, Sender, TrySendError};

use crate::actors::{EventMsg, RenderMsg, TreeMsg};
use crate::cursor::CursorState;
use crate::events::CursorIcon;
use crate::input::InputEvent;
use crate::renderer::{RenderState, Renderer};
use crate::video::{VideoImportContext, VideoRegistry};

const EGL_PLATFORM_GBM_KHR: EGLenum = 0x31D7;

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
    handle: plane::Handle,
    props: HashMap<String, property::Info>,
    fb: framebuffer::Handle,
    _bo: BufferObject<()>,
    size: (u32, u32),
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
        let _ = card.destroy_framebuffer(cursor_plane.fb);
    }

    destroy_framebuffers(card, framebuffer_cache);
    destroy_mode_blob(card, mode_blob_id);
}

fn teardown_drm_output(
    card: &Card,
    connector: connector::Handle,
    crtc_handle: crtc::Handle,
    plane: plane::Handle,
    con_props: &HashMap<String, property::Info>,
    crtc_props: &HashMap<String, property::Info>,
    plane_props: &HashMap<String, property::Info>,
    cursor_plane: Option<&CursorPlane>,
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
        plane,
        prop_handle(plane_props, "FB_ID")?,
        property::Value::Framebuffer(None),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "CRTC_ID")?,
        property::Value::CRTC(None),
    );
    req.add_property(
        connector,
        prop_handle(con_props, "CRTC_ID")?,
        property::Value::CRTC(None),
    );
    req.add_property(
        crtc_handle,
        prop_handle(crtc_props, "ACTIVE")?,
        property::Value::Boolean(false),
    );

    if let Ok(mode_handle) = prop_handle(crtc_props, "MODE_ID") {
        req.add_property(crtc_handle, mode_handle, property::Value::Blob(0));
    }

    card.atomic_commit(AtomicCommitFlags::ALLOW_MODESET, req)
        .map_err(|e| format!("tearing down DRM output failed: {e}"))
}

fn cleanup_active_session(
    card: &Card,
    connector: connector::Handle,
    crtc_handle: crtc::Handle,
    plane: plane::Handle,
    con_props: &HashMap<String, property::Info>,
    crtc_props: &HashMap<String, property::Info>,
    plane_props: &HashMap<String, property::Info>,
    cursor_plane: Option<CursorPlane>,
    framebuffer_cache: &mut HashMap<u32, framebuffer::Handle>,
    mode_blob_id: Option<u64>,
) {
    if let Err(err) = teardown_drm_output(
        card,
        connector,
        crtc_handle,
        plane,
        con_props,
        crtc_props,
        plane_props,
        cursor_plane.as_ref(),
    ) {
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

fn draw_cursor_bitmap(size: u32) -> Vec<u8> {
    let mut data = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let mut a = 0;
            let mut r = 0;
            let mut g = 0;
            let mut b = 0;
            let white = (x < 2 && y < 18) || (y < 2 && x < 18) || (x == y && x < 18);
            let outline = (x == 2 && y < 18) || (y == 2 && x < 18) || ((x == y) && x < 18 && x > 0);
            if white {
                a = 255;
                r = 255;
                g = 255;
                b = 255;
            }
            if outline {
                a = 255;
                r = 0;
                g = 0;
                b = 0;
            }

            let idx = ((y * size + x) * 4) as usize;
            data[idx] = b;
            data[idx + 1] = g;
            data[idx + 2] = r;
            data[idx + 3] = a;
        }
    }

    data
}

fn create_cursor_plane<T: AsFd>(
    card: &Card,
    gbm_device: &GbmDevice<T>,
    resources: &ResourceHandles,
    crtc_handle: crtc::Handle,
) -> Result<Option<CursorPlane>, String> {
    let Some(handle) = find_cursor_plane(card, resources, crtc_handle)? else {
        return Ok(None);
    };
    let props = card
        .get_properties(handle)
        .and_then(|props| props.as_hashmap(card))
        .map_err(|e| format!("failed to read cursor plane properties: {e}"))?;

    let size = (64, 64);
    let mut bo = gbm_device
        .create_buffer_object(
            size.0,
            size.1,
            GbmFormat::Argb8888,
            BufferObjectFlags::CURSOR | BufferObjectFlags::WRITE | BufferObjectFlags::LINEAR,
        )
        .map_err(|e| format!("failed to create cursor bo: {e}"))?;

    let data = draw_cursor_bitmap(size.0);
    bo.write(&data)
        .map_err(|e| format!("failed to write cursor bo: {e}"))?;

    let fb = card
        .add_framebuffer(&bo, 32, 32)
        .map_err(|e| format!("failed to create cursor fb: {e}"))?;

    Ok(Some(CursorPlane {
        handle,
        props,
        fb,
        _bo: bo,
        size,
    }))
}

fn update_cursor_plane(
    card: &Card,
    crtc_handle: crtc::Handle,
    plane: &CursorPlane,
    cursor: CursorState,
    screen_size: (u32, u32),
) -> Result<(), String> {
    let mut req = atomic::AtomicModeReq::new();
    if cursor.visible {
        let (screen_w, screen_h) = screen_size;
        let min_x = -(plane.size.0 as i64) + 1;
        let min_y = -(plane.size.1 as i64) + 1;
        let max_x = screen_w.saturating_sub(1) as i64;
        let max_y = screen_h.saturating_sub(1) as i64;
        let x = (cursor.pos.0.round() as i64).clamp(min_x, max_x);
        let y = (cursor.pos.1.round() as i64).clamp(min_y, max_y);
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

    card.atomic_commit(AtomicCommitFlags::NONBLOCK, req)
        .map_err(|e| format!("cursor plane commit failed: {e}"))
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

fn create_renderer(egl: &egl::Egl, dimensions: (u32, u32)) -> Result<Renderer, String> {
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

    Ok(Renderer::new_gl(dimensions, fb_info, gr_context, 0, 0))
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

fn draw_software_cursor(renderer: &mut Renderer, cursor_pos: (f32, f32), screen_size: (u32, u32)) {
    let (width, height) = screen_size;
    let x = cursor_pos.0.clamp(0.0, width.saturating_sub(1) as f32);
    let y = cursor_pos.1.clamp(0.0, height.saturating_sub(1) as f32);

    let canvas = renderer.surface_mut().canvas();
    let mut fill = Paint::default();
    fill.set_anti_alias(true);
    fill.set_color(Color::from_argb(240, 255, 255, 255));
    canvas.draw_circle((x, y), 4.0, &fill);

    let mut stroke = Paint::default();
    stroke.set_anti_alias(true);
    stroke.set_style(PaintStyle::Stroke);
    stroke.set_stroke_width(1.0);
    stroke.set_color(Color::from_argb(200, 0, 0, 0));
    canvas.draw_circle((x, y), 4.0, &stroke);

    renderer.flush();
}

#[derive(Clone)]
pub struct DrmRunConfig {
    pub requested_size: Option<(u32, u32)>,
    pub card_path: Option<String>,
    pub startup_retries: u32,
    pub retry_interval_ms: u32,
    pub hw_cursor: bool,
    pub render_log: bool,
}

pub struct DrmRunContext {
    pub startup_tx: StartupSender<Result<(), String>>,
    pub stop: Arc<AtomicBool>,
    pub running_flag: Arc<AtomicBool>,
    pub tree_tx: Sender<TreeMsg>,
    pub render_rx: Receiver<RenderMsg>,
    pub cursor_icon_rx: Receiver<CursorIcon>,
    pub cursor_pos_rx: Receiver<CursorState>,
    pub event_tx: Sender<EventMsg>,
    pub screen_tx: Sender<(u32, u32)>,
    pub render_counter: Arc<AtomicU64>,
    pub video_registry: Arc<VideoRegistry>,
}

pub fn run(context: DrmRunContext, config: DrmRunConfig) {
    let DrmRunContext {
        startup_tx,
        stop,
        running_flag,
        tree_tx,
        render_rx,
        cursor_icon_rx,
        cursor_pos_rx,
        event_tx,
        screen_tx,
        render_counter,
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
            .and_then(|props| props.as_hashmap(&card))
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
            .and_then(|props| props.as_hashmap(&card))
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
            .and_then(|props| props.as_hashmap(&card))
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
        if !logged_mode_info {
            println!(
                "DRM resources: connector={} encoder={} crtc={} plane={}",
                u32::from(connector),
                u32::from(encoder_handle),
                u32::from(crtc_handle),
                u32::from(plane)
            );
            println!(
                "DRM mode: {}x{} @ {}Hz",
                dimensions.0,
                dimensions.1,
                mode.vrefresh()
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
            match create_cursor_plane(&card, &gbm_device, &resources, crtc_handle) {
                Ok(plane) => plane,
                Err(e) => {
                    eprintln!("DRM cursor setup failed: {e}");
                    None
                }
            }
        } else {
            None
        };
        if !logged_cursor_info {
            if config.hw_cursor {
                if cursor_plane.is_some() {
                    println!("DRM cursor: hardware plane enabled");
                } else {
                    println!("DRM cursor: hardware unavailable, using software");
                }
            } else {
                println!("DRM cursor: hardware disabled, using software");
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

        let mut renderer = match create_renderer(&egl_state.egl, dimensions) {
            Ok(renderer) => renderer,
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
        renderer.render(&render_state);

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

        let mut current_bo = Some(bo);
        let mut pending_render = true;
        let mut cursor_pos = (0.0, 0.0);
        let mut cursor_visible = true;
        let mut _current_cursor_icon = CursorIcon::Default;
        let mut last_cursor_pos = cursor_pos;
        let mut last_cursor_visible = cursor_visible;

        let mut next_hotplug_check = Instant::now() + hotplug_interval;
        let mut last_video_generation = video_registry.generation();
        let mut stop_requested = false;

        loop {
            if stop.load(Ordering::Relaxed) {
                stop_requested = true;
                break;
            }

            if Instant::now() >= next_hotplug_check {
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
                    RenderMsg::Commands {
                        commands,
                        version,
                        animate,
                        ..
                    } => {
                        render_state.commands = commands;
                        render_state.render_version = version;
                        render_state.animate = animate;
                        pending_render = true;
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

            while let Ok(icon) = cursor_icon_rx.try_recv() {
                _current_cursor_icon = icon;
            }

            while let Ok(cursor) = cursor_pos_rx.try_recv() {
                cursor_pos = cursor.pos;
                cursor_visible = cursor.visible;
            }

            if cursor_plane.is_some()
                && (cursor_visible != last_cursor_visible || cursor_pos != last_cursor_pos)
            {
                let cursor = crate::cursor::CursorState {
                    pos: cursor_pos,
                    visible: cursor_visible,
                };
                let cursor_plane_error = cursor_plane.as_ref().and_then(|plane| {
                    update_cursor_plane(&card, crtc_handle, plane, cursor, dimensions).err()
                });
                if let Some(err) = cursor_plane_error
                    && !is_ebusy(&err)
                {
                    eprintln!("DRM cursor update failed: {err}");
                    cursor_plane = None;
                    pending_render = true;
                }
            }

            if cursor_plane.is_none() {
                if cursor_visible && cursor_pos != last_cursor_pos {
                    pending_render = true;
                }
                if cursor_visible != last_cursor_visible {
                    pending_render = true;
                }
            }

            let video_generation = video_registry.generation();
            if video_generation != last_video_generation {
                pending_render = true;
                last_video_generation = video_generation;
            }

            last_cursor_pos = cursor_pos;
            last_cursor_visible = cursor_visible;

            if pending_render {
                let frame_version = render_state.render_version;
                let mut video_needs_cleanup = false;
                match renderer.sync_video_frames(&video_registry, video_import.as_ref()) {
                    Ok(result) => video_needs_cleanup = result.needs_cleanup,
                    Err(err) => eprintln!("video sync failed: {err}"),
                }
                renderer.render(&render_state);
                if cursor_plane.is_none() && cursor_visible {
                    draw_software_cursor(&mut renderer, cursor_pos, dimensions);
                }

                if unsafe {
                    egl_state
                        .egl
                        .SwapBuffers(egl_state.display, egl_state.surface)
                } == egl::FALSE
                {
                    eprintln!("DRM backend unavailable: eglSwapBuffers failed");
                    break;
                }

                let next_bo = match unsafe { gbm_surface.lock_front_buffer() } {
                    Ok(bo) => bo,
                    Err(e) => {
                        eprintln!("DRM backend unavailable: {e}");
                        break;
                    }
                };

                let next_fb = match framebuffer_for_bo(&card, &mut framebuffer_cache, &next_bo) {
                    Ok(fb) => fb,
                    Err(e) => {
                        eprintln!("DRM backend unavailable: {e}");
                        break;
                    }
                };

                let mut flip_req = atomic::AtomicModeReq::new();
                if let Err(e) =
                    add_plane_properties(&mut flip_req, plane, &plane_props, crtc_handle, next_fb)
                {
                    eprintln!("DRM backend unavailable: {e}");
                    break;
                }

                if let Err(e) = card.atomic_commit(AtomicCommitFlags::empty(), flip_req) {
                    let err = e.to_string();
                    if is_ebusy(&err) {
                        if log_render {
                            eprintln!("drm flip EBUSY, retrying fresh frame");
                        }
                        drop(next_bo);
                        pending_render = true;
                        continue;
                    }
                    eprintln!("DRM backend unavailable: {err}");
                    break;
                }
                if log_render {
                    eprintln!("drm present version={frame_version}");
                }
                let presented_at = Instant::now();
                if render_state.animate {
                    let msg = TreeMsg::AnimationPulse {
                        presented_at,
                        predicted_next_present_at: presented_at + frame_interval,
                    };

                    match tree_tx.try_send(msg) {
                        Ok(()) => {}
                        Err(TrySendError::Full(msg)) => {
                            let _ = tree_tx.send(msg);
                        }
                        Err(TrySendError::Disconnected(_)) => {}
                    }
                }

                drop(current_bo.take());
                current_bo = Some(next_bo);
                pending_render = video_needs_cleanup;
            }
            std::thread::sleep(Duration::from_millis(4));
        }

        drop(current_bo.take());
        cleanup_active_session(
            &card,
            connector,
            crtc_handle,
            plane,
            &con_props,
            &crtc_props,
            &plane_props,
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
