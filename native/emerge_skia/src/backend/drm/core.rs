//! Presenter-neutral DRM/KMS discovery and atomic error classification.
#![cfg_attr(not(feature = "drm"), allow(dead_code))]
//!
//! This module contains no EGL, OpenGL, Vulkan, or Wayland objects. Concrete render owners use
//! the selected connector/mode/plane handles while KMS page-flip events remain presentation
//! authority.

use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io,
    os::fd::{AsFd, AsRawFd, BorrowedFd},
    time::Duration,
};

#[cfg(feature = "drm-vulkan")]
use drm::ClientCapability;
use drm::{
    Device as BasicDevice,
    control::{
        self, Device as ControlDevice, PlaneType, ResourceHandles, connector, crtc, encoder, plane,
        property,
    },
};

#[cfg(feature = "drm-vulkan")]
use crate::backend::vulkan::capabilities::{DrmMatchField, DrmNodeId, SelectionNode};
#[cfg(feature = "drm-vulkan")]
use drm::node::NodeType;
#[cfg(feature = "drm-vulkan")]
use std::{os::unix::fs::FileTypeExt, path::Path};

// The file closes before the lease guard releases its reservation. Duplicates retain the guard.
pub(super) struct Card(
    pub(super) File,
    pub(super) Option<std::sync::Arc<super::session::Lease>>,
);

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

#[cfg(feature = "drm-vulkan")]
pub(super) fn duplicate_card(card: &Card) -> Result<Card, String> {
    card.0
        .try_clone()
        .map(|file| Card(file, card.1.clone()))
        .map_err(|error| format!("failed to duplicate KMS fd for GBM: {error}"))
}

/// Opens the compatibility KMS path. Deliberately retains the historical GL default and error
/// behavior; the explicit Vulkan preflight applies stricter primary-node validation separately.
pub(super) fn open_card(card_path: Option<&str>) -> Result<Card, String> {
    let card_path = card_path.unwrap_or("/dev/dri/card0");
    let fd = OpenOptions::new()
        .read(true)
        .write(true)
        .open(card_path)
        .map_err(|error| format!("failed to open {card_path}: {error}"))?;
    Ok(Card(fd, None))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AtomicCommitErrorKind {
    Busy,
    Invalid,
    Permission,
    Other(Option<i32>),
}

/// Preserve the kernel errno returned by the DRM ioctl. In particular, retry policy must never
/// depend on localized/display-formatted error strings.
pub(super) fn classify_atomic_commit_error(error: &io::Error) -> AtomicCommitErrorKind {
    match error.raw_os_error() {
        Some(libc::EBUSY) => AtomicCommitErrorKind::Busy,
        Some(libc::EINVAL) => AtomicCommitErrorKind::Invalid,
        Some(libc::EACCES) | Some(libc::EPERM) => AtomicCommitErrorKind::Permission,
        errno => AtomicCommitErrorKind::Other(errno),
    }
}

#[cfg(feature = "drm-vulkan")]
pub(super) struct VulkanSelectionNode {
    _file: File,
    pub(super) path: String,
    pub(super) selection: SelectionNode,
}

#[cfg(feature = "drm-vulkan")]
pub(super) fn open_vulkan_selection_node(path: &str) -> Result<VulkanSelectionNode, String> {
    if !Path::new(path).is_absolute() {
        return Err(format!(
            "configured Vulkan DRM selection node must be an absolute path, got {path:?}"
        ));
    }
    let file = open_character_device(path, "Vulkan DRM selection node")?;
    let node = drm::node::DrmNode::from_file(&file).map_err(|error| {
        format!("configured Vulkan DRM selection node {path:?} is not a DRM node: {error}")
    })?;
    let field = match node.ty() {
        NodeType::Primary => DrmMatchField::Primary,
        NodeType::Render => DrmMatchField::Render,
        NodeType::Control => {
            return Err(format!(
                "configured Vulkan DRM selection node {path:?} is a control node; a primary or render node is required"
            ));
        }
    };
    Ok(VulkanSelectionNode {
        _file: file,
        path: path.to_string(),
        selection: SelectionNode {
            node: DrmNodeId {
                major: node.major(),
                minor: node.minor(),
            },
            field,
        },
    })
}

#[cfg(feature = "drm-vulkan")]
pub(super) struct KmsOutputProbe {
    pub(super) card: Card,
    #[cfg(feature = "drm-vulkan")]
    pub(super) resources: ResourceHandles,
    pub(super) connector: connector::Handle,
    pub(super) mode: control::Mode,
    pub(super) crtc: crtc::Handle,
    pub(super) encoder: encoder::Handle,
    pub(super) primary: plane::Handle,
    pub(super) dimensions: (u32, u32),
    pub(super) primary_supports_xrgb8888: bool,
    pub(super) primary_has_in_fence_fd: bool,
}

/// Opens and validates the KMS card independently from the Vulkan selection node. Vulkan enables
/// atomic/universal-plane discovery and requires an actual primary plane; the compatibility GL
/// presenter deliberately retains its historical fallback selector.
#[cfg(feature = "drm-vulkan")]
pub(super) fn probe_kms_output(
    card_path: Option<&str>,
    requested_size: Option<(u32, u32)>,
) -> Result<KmsOutputProbe, String> {
    let path = card_path.unwrap_or("/dev/dri/card0");
    let card = Card(open_character_device(path, "KMS DRM card")?, None);
    let node = drm::node::DrmNode::from_file(&card.0)
        .map_err(|error| format!("configured KMS DRM card {path:?} is not a DRM node: {error}"))?;
    if node.ty() != NodeType::Primary {
        return Err(format!(
            "configured KMS DRM card {path:?} is a {:?} node, not a primary modeset node",
            node.ty()
        ));
    }
    card.set_client_capability(ClientCapability::UniversalPlanes, true)
        .map_err(|error| {
            format!("configured KMS DRM card {path:?} cannot enable universal planes: {error}")
        })?;
    card.set_client_capability(ClientCapability::Atomic, true)
        .map_err(|error| {
            format!("configured KMS DRM card {path:?} cannot enable atomic modesetting: {error}")
        })?;
    probe_card(card, requested_size)
}

#[cfg(feature = "drm-vulkan")]
pub(super) fn probe_card(
    card: Card,
    requested_size: Option<(u32, u32)>,
) -> Result<KmsOutputProbe, String> {
    let resources = card
        .resource_handles()
        .map_err(|error| format!("KMS DRM card exposes no control resources: {error}"))?;
    let (connector, mode, crtc, encoder) =
        first_connected_connector(&card, &resources, requested_size)?;
    let primary = find_strict_primary_plane(&card, &resources, crtc)?;
    let info = card
        .get_plane(primary)
        .map_err(|error| format!("failed to inspect selected primary plane: {error}"))?;
    let primary_supports_xrgb8888 = info.formats().contains(&u32::from_le_bytes(*b"XR24"));
    let primary_props = card
        .get_properties(primary)
        .and_then(|props| props.as_hashmap(&card))
        .map_err(|error| format!("failed to inspect selected primary-plane properties: {error}"))?;
    let size = mode.size();
    Ok(KmsOutputProbe {
        card,
        #[cfg(feature = "drm-vulkan")]
        resources,
        connector,
        mode,
        crtc,
        encoder,
        primary,
        dimensions: (u32::from(size.0), u32::from(size.1)),
        primary_supports_xrgb8888,
        primary_has_in_fence_fd: primary_props.contains_key("IN_FENCE_FD"),
    })
}

#[cfg(feature = "drm-vulkan")]
fn open_character_device(path: &str, role: &str) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("failed to open {role} {path:?}: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("failed to stat {role} {path:?}: {error}"))?;
    if !metadata.file_type().is_char_device() {
        return Err(format!(
            "configured {role} {path:?} is not a character device"
        ));
    }
    Ok(file)
}

pub(super) fn mode_distance(mode: &control::Mode, requested: (u32, u32)) -> i64 {
    let (width, height) = mode.size();
    let dx = width as i64 - requested.0 as i64;
    let dy = height as i64 - requested.1 as i64;
    dx * dx + dy * dy
}

pub(super) fn mode_area(mode: &control::Mode) -> i64 {
    let (width, height) = mode.size();
    width as i64 * height as i64
}

pub(super) fn mode_is_preferred(mode: &control::Mode) -> bool {
    mode.mode_type().contains(control::ModeTypeFlags::PREFERRED)
}

pub(super) fn preferred_size(modes: &[control::Mode]) -> Option<(u32, u32)> {
    modes
        .iter()
        .find(|mode| mode_is_preferred(mode))
        .map(|mode| {
            let (width, height) = mode.size();
            (width as u32, height as u32)
        })
}

pub(super) fn choose_mode(
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

pub(super) fn score_mode(
    mode: &control::Mode,
    target_size: Option<(u32, u32)>,
) -> (i64, i32, i32, i64) {
    let distance = target_size
        .map(|size| mode_distance(mode, size))
        .unwrap_or(0);
    let refresh = -(mode.vrefresh() as i32);
    let preferred = if mode_is_preferred(mode) { 0 } else { 1 };
    let area = -mode_area(mode);
    (distance, refresh, preferred, area)
}

pub(super) fn mode_refresh_hz(mode: &control::Mode) -> f64 {
    precise_mode_refresh_hz(
        mode.clock(),
        mode.hsync().2,
        mode.vsync().2,
        mode.vscan(),
        mode.flags(),
    )
    .unwrap_or_else(|| mode.vrefresh().max(1) as f64)
}

/// Exact timing identity, independent of kernel ordering, preferred flags and display names.
/// Never decode caller-provided timings: selection only matches modes advertised by the connector.
pub(super) fn mode_id(mode: &control::Mode) -> String {
    let (w, h) = mode.size();
    let (hs, he, ht) = mode.hsync();
    let (vs, ve, vt) = mode.vsync();
    format!(
        "{w}x{h}:{}:{hs}:{he}:{ht}:{}:{vs}:{ve}:{vt}:{}:{:x}",
        mode.clock(),
        mode.hskew(),
        mode.vscan(),
        mode.flags().bits()
    )
}

pub(super) fn select_mode(
    modes: &[control::Mode],
    requested: Option<(u32, u32)>,
    exact: Option<&str>,
) -> Result<control::Mode, String> {
    match exact {
        Some(id) => modes
            .iter()
            .find(|mode| mode_id(mode) == id)
            .copied()
            .ok_or_else(|| "selected DRM mode is not advertised by this output".to_string()),
        None => choose_mode(modes, requested),
    }
}

pub(crate) fn outputs(path: &str) -> Result<Vec<crate::display::DrmOutput>, String> {
    // Read-only discovery: no explicit master acquisition, capabilities or modeset.
    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|error| format!("failed to open {path}: {error}"))?;
    let card = Card(file, None);
    let resources = card
        .resource_handles()
        .map_err(|error| format!("failed to query DRM outputs on {path}: {error}"))?;
    resources
        .connectors()
        .iter()
        .map(|handle| {
            let info = card
                .get_connector(*handle, false)
                .map_err(|error| format!("failed to query DRM connector {handle:?}: {error}"))?;
            Ok(crate::display::DrmOutput {
                name: info.to_string(),
                connector_id: u32::from(*handle),
                connected: info.state() == connector::State::Connected,
                modes: info
                    .modes()
                    .iter()
                    .map(|mode| crate::display::DrmMode {
                        id: mode_id(mode),
                        name: mode.name().to_string_lossy().into_owned(),
                        width: u32::from(mode.size().0),
                        height: u32::from(mode.size().1),
                        refresh_hz: mode_refresh_hz(mode),
                        preferred: mode_is_preferred(mode),
                        interlaced: mode.flags().contains(control::ModeFlags::INTERLACE),
                    })
                    .collect(),
            })
        })
        .collect()
}

pub(super) fn mode_frame_interval(mode: &control::Mode) -> Duration {
    frame_interval_for_refresh_hz(mode_refresh_hz(mode))
}

pub(super) fn frame_interval_for_refresh_hz(refresh_hz: f64) -> Duration {
    Duration::from_secs_f64(1.0 / refresh_hz.max(1.0))
}

pub(super) fn precise_mode_refresh_hz(
    clock_khz: u32,
    htotal: u16,
    vtotal: u16,
    vscan: u16,
    flags: control::ModeFlags,
) -> Option<f64> {
    if clock_khz == 0 || htotal == 0 || vtotal == 0 {
        return None;
    }

    let mut refresh_hz = clock_khz as f64 * 1_000.0 / htotal as f64 / vtotal as f64;
    if flags.contains(control::ModeFlags::INTERLACE) {
        refresh_hz *= 2.0;
    }
    if flags.contains(control::ModeFlags::DBLSCAN) {
        refresh_hz /= 2.0;
    }
    refresh_hz /= vscan.max(1) as f64;

    refresh_hz
        .is_finite()
        .then_some(refresh_hz)
        .filter(|hz| *hz > 0.0)
}

pub(super) fn first_connected_connector(
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
    if let Some(lease) = card.1.as_ref() {
        let info = card
            .get_connector(lease.connector, false)
            .map_err(|e| format!("failed to read leased connector: {e}"))?;
        if info.state() != connector::State::Connected {
            return Err("selected DRM output is disconnected".into());
        }
        if !info
            .modes()
            .iter()
            .any(|mode| mode_id(mode) == mode_id(&lease.mode))
        {
            return Err("selected DRM mode is no longer available".into());
        }
        return Ok((lease.connector, lease.mode, lease.crtc, lease.encoder));
    }
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

pub(super) fn pick_encoder_and_crtc(
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

pub(super) fn is_primary_plane(card: &Card, plane: plane::Handle) -> Result<bool, String> {
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

pub(super) fn is_cursor_plane(card: &Card, plane: plane::Handle) -> Result<bool, String> {
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

pub(super) fn compatible_planes(
    card: &Card,
    resources: &ResourceHandles,
    crtc_handle: crtc::Handle,
) -> Result<Vec<plane::Handle>, String> {
    card.plane_handles()
        .map_err(|e| format!("could not list planes: {e}"))?
        .into_iter()
        .filter_map(|plane| {
            let result = card
                .get_plane(plane)
                .map_err(|e| format!("failed to read plane info: {e}"))
                .map(|info| {
                    resources
                        .filter_crtcs(info.possible_crtcs())
                        .contains(&crtc_handle)
                        .then_some(plane)
                });
            result.transpose()
        })
        .collect()
}

/// Compatibility selector used by the established GL presenter. Some older drivers did not expose
/// typed planes, so this deliberately retains the historical first-compatible fallback.
pub(super) fn find_primary_plane(
    card: &Card,
    resources: &ResourceHandles,
    crtc_handle: crtc::Handle,
) -> Result<plane::Handle, String> {
    let compatible = compatible_planes(card, resources, crtc_handle)?;
    compatible
        .iter()
        .copied()
        .map(|plane| is_primary_plane(card, plane).map(|primary| primary.then_some(plane)))
        .find_map(Result::transpose)
        .transpose()?
        .or_else(|| compatible.first().copied())
        .ok_or_else(|| "no compatible planes found".to_string())
}

/// Vulkan may not derive its scanout contract from an overlay/cursor fallback. Universal planes
/// are enabled by the caller before this selector runs, so absence of a typed primary is terminal.
#[cfg(feature = "drm-vulkan")]
fn find_strict_primary_plane(
    card: &Card,
    resources: &ResourceHandles,
    crtc_handle: crtc::Handle,
) -> Result<plane::Handle, String> {
    let primaries = compatible_planes(card, resources, crtc_handle)?
        .into_iter()
        .map(|plane| is_primary_plane(card, plane).map(|primary| primary.then_some(plane)))
        .filter_map(Result::transpose)
        .collect::<Result<Vec<_>, _>>()?;
    match primaries.as_slice() {
        [primary] => Ok(*primary),
        [] => Err("selected CRTC has no typed DRM primary plane".to_string()),
        _ => Err(format!(
            "selected CRTC has {} compatible typed DRM primary planes; selection is ambiguous",
            primaries.len()
        )),
    }
}

pub(super) fn find_cursor_plane(
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

pub(super) fn prop_handle(
    props: &HashMap<String, property::Info>,
    name: &str,
) -> Result<property::Handle, String> {
    props
        .get(name)
        .map(|info| info.handle())
        .ok_or_else(|| format!("missing property {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(clock: u32, flags: u32, preferred: bool) -> control::Mode {
        drm_ffi::drm_mode_modeinfo {
            clock,
            hdisplay: 1920,
            hsync_start: 2008,
            hsync_end: 2052,
            htotal: 2200,
            vdisplay: 1080,
            vsync_start: 1084,
            vsync_end: 1089,
            vtotal: 1125,
            vrefresh: 60,
            flags,
            type_: if preferred {
                control::ModeTypeFlags::PREFERRED.bits()
            } else {
                0
            },
            ..Default::default()
        }
        .into()
    }

    #[test]
    fn exact_mode_distinguishes_fractional_refresh_and_scan_flags() {
        let sixty = fixture(148500, 0, true);
        let fractional = fixture(148352, 0, false);
        let interlaced = fixture(148500, control::ModeFlags::INTERLACE.bits(), false);
        assert_ne!(mode_id(&sixty), mode_id(&fractional));
        assert_ne!(mode_id(&sixty), mode_id(&interlaced));
        assert_eq!(mode_id(&sixty), mode_id(&fixture(148500, 0, false)));
        assert_eq!(
            select_mode(
                &[sixty, fractional],
                Some((800, 600)),
                Some(&mode_id(&fractional))
            ),
            Ok(fractional)
        );
        assert!(select_mode(&[sixty], None, Some(&mode_id(&fractional))).is_err());
        assert!(select_mode(&[], None, None).is_err());
    }

    #[test]
    fn atomic_commit_errno_is_classified_without_string_parsing() {
        assert_eq!(
            classify_atomic_commit_error(&io::Error::from_raw_os_error(libc::EBUSY)),
            AtomicCommitErrorKind::Busy
        );
        assert_eq!(
            classify_atomic_commit_error(&io::Error::from_raw_os_error(libc::EINVAL)),
            AtomicCommitErrorKind::Invalid
        );
        assert_eq!(
            classify_atomic_commit_error(&io::Error::from_raw_os_error(libc::EPERM)),
            AtomicCommitErrorKind::Permission
        );
    }
}
