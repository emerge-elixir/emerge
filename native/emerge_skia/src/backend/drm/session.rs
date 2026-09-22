//! One master per device, one independently readable lease per output.
//! Registry locks are used only for admission; presentation never touches them.
use super::core::{self, Card};
use drm::{
    ClientCapability, Device as _,
    control::{self, Device as _, connector, crtc, encoder},
};
use std::{
    collections::{HashMap, HashSet},
    os::unix::fs::{FileTypeExt, MetadataExt},
    sync::{Arc, Mutex, OnceLock, Weak},
};

struct Master {
    card: Card,
    reservations: Mutex<HashSet<u32>>,
}

fn masters() -> &'static Mutex<HashMap<u64, Weak<Master>>> {
    static MASTERS: OnceLock<Mutex<HashMap<u64, Weak<Master>>>> = OnceLock::new();
    MASTERS.get_or_init(Default::default)
}

pub(super) struct Lease {
    master: Arc<Master>,
    // Keep the lessee alive until Drop explicitly revokes it, even after the presenter fd closes.
    _fd: std::os::fd::OwnedFd,
    lessee_id: control::LeaseId,
    lifecycle: Option<Arc<crate::runtime::lifecycle::Lifecycle>>,
    objects: Vec<u32>,
    pub(super) connector: connector::Handle,
    pub(super) crtc: crtc::Handle,
    pub(super) encoder: encoder::Handle,
    pub(super) mode: control::Mode,
}

impl Drop for Lease {
    fn drop(&mut self) {
        // Mesa can cache this DRM file in a GPU-wide winsys shared by a sibling EGL
        // context. Closing our fd alone therefore does NOT release the kernel lease.
        // All of this session's GPU/KMS owners have retired before the final guard drops.
        if let Err(error) = self.master.card.revoke_lease(self.lessee_id) {
            let reason = format!("DRM lease revocation failed; output remains reserved: {error}");
            if let Some(lifecycle) = &self.lifecycle {
                lifecycle.quarantine_output(reason.clone());
            }
            eprintln!("{reason}");
            // Keep reservations pinned even when the last healthy sibling exits.
            std::mem::forget(Arc::clone(&self.master));
            return;
        }
        // A poisoned admission table is never reopened for reuse.
        if let Ok(mut reserved) = self.master.reservations.lock() {
            self.objects.iter().for_each(|object| {
                reserved.remove(object);
            });
        }
    }
}

fn enable_atomic(card: &Card) -> Result<(), String> {
    card.set_client_capability(ClientCapability::UniversalPlanes, true)
        .and_then(|()| card.set_client_capability(ClientCapability::Atomic, true))
        .map_err(|e| format!("failed to enable DRM atomic/universal planes: {e}"))
}

fn master(path: Option<&str>) -> Result<Arc<Master>, String> {
    let path = path.unwrap_or("/dev/dri/card0");
    let metadata = std::fs::metadata(path).map_err(|e| format!("failed to stat {path}: {e}"))?;
    if !metadata.file_type().is_char_device() {
        return Err(format!("DRM card {path} is not a character device"));
    }
    let mut registry = masters()
        .lock()
        .map_err(|_| "DRM master registry is poisoned")?;
    registry.retain(|_, entry| entry.strong_count() > 0);
    if let Some(master) = registry.get(&metadata.rdev()).and_then(Weak::upgrade) {
        return Ok(master);
    }
    let card = core::open_card(Some(path))?;
    let node = drm::node::DrmNode::from_file(&card.0)
        .map_err(|e| format!("invalid DRM card {path}: {e}"))?;
    if node.ty() != drm::node::NodeType::Primary {
        return Err(format!("DRM card {path} is not a primary modeset node"));
    }
    card.acquire_master_lock()
        .map_err(|e| format!("could not acquire DRM master on {path}: {e}"))?;
    enable_atomic(&card)?;
    let master = Arc::new(Master {
        card,
        reservations: Mutex::new(HashSet::new()),
    });
    registry.insert(metadata.rdev(), Arc::downgrade(&master));
    Ok(master)
}

/// Returns a lease with exactly one connector, CRTC, primary and optional cursor plane.
/// Encoders are reserved in userspace (the kernel includes them implicitly, not as lease objects).
pub(super) fn open_output(
    path: Option<&str>,
    output: Option<&str>,
    mode_id: Option<&str>,
    requested: Option<(u32, u32)>,
    cursor: bool,
    lifecycle: Option<Arc<crate::runtime::lifecycle::Lifecycle>>,
) -> Result<Card, String> {
    let master = master(path)?;
    let card = &master.card;
    let mut reserved = master
        .reservations
        .lock()
        .map_err(|_| "DRM output reservations are poisoned")?;
    let resources = card
        .resource_handles()
        .map_err(|e| format!("failed to query DRM resources: {e}"))?;
    let connectors = resources
        .connectors()
        .iter()
        .map(|handle| {
            card.get_connector(*handle, false)
                .map_err(|e| format!("failed to read connector: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let candidates = connectors
        .iter()
        .filter(|info| output.is_none_or(|name| name == info.to_string()));
    let mut last_error = output
        .map(|name| format!("DRM output {name:?} not found"))
        .unwrap_or_else(|| "no available connected DRM outputs".into());
    for info in candidates {
        if reserved.contains(&u32::from(info.handle())) {
            last_error = format!(
                "DRM output {} is busy (running, stopping or quarantined)",
                info
            );
            continue;
        }
        if info.state() != connector::State::Connected {
            last_error = format!("DRM output {} is disconnected", info);
            continue;
        }
        let mode = match core::select_mode(info.modes(), requested, mode_id) {
            Ok(mode) => mode,
            Err(e) => {
                last_error = e;
                continue;
            }
        };
        // Prefer current routing, but never reuse a CRTC or encoder held by another viewport.
        let encoders = info
            .current_encoder()
            .into_iter()
            .chain(info.encoders().iter().copied())
            .filter(|encoder| !reserved.contains(&u32::from(*encoder)));
        for encoder in encoders {
            let encoder_info = card
                .get_encoder(encoder)
                .map_err(|e| format!("failed to read encoder: {e}"))?;
            let compatible = resources.filter_crtcs(encoder_info.possible_crtcs());
            let crtcs = encoder_info
                .crtc()
                .into_iter()
                .chain(compatible.iter().copied())
                .filter(|crtc| compatible.contains(crtc) && !reserved.contains(&u32::from(*crtc)));
            for crtc in crtcs {
                let planes = core::compatible_planes(card, &resources, crtc)?;
                let primary = planes
                    .iter()
                    .copied()
                    .filter(|p| !reserved.contains(&u32::from(*p)))
                    .map(|p| core::is_primary_plane(card, p).map(|yes| yes.then_some(p)))
                    .find_map(Result::transpose)
                    .transpose()?;
                let Some(primary) = primary else {
                    continue;
                };
                let cursor_plane = if cursor {
                    planes
                        .iter()
                        .copied()
                        .filter(|p| !reserved.contains(&u32::from(*p)))
                        .map(|p| core::is_cursor_plane(card, p).map(|yes| yes.then_some(p)))
                        .find_map(Result::transpose)
                        .transpose()?
                } else {
                    None
                };
                let objects: Vec<control::RawResourceHandle> =
                    [info.handle().into(), crtc.into(), primary.into()]
                        .into_iter()
                        .chain(cursor_plane.map(Into::into))
                        .collect();
                let (lessee_id, fd) = card.create_lease(&objects, drm::CLOEXEC).map_err(|e| {
                    format!("failed to lease DRM output {info} (DRM leasing is required): {e}")
                })?;
                let lease_fd = fd
                    .try_clone()
                    .map_err(|e| format!("failed to retain lease fd: {e}"))?;
                let owned: Vec<u32> = objects
                    .iter()
                    .map(|handle| handle.get())
                    .chain([u32::from(encoder)])
                    .collect();
                reserved.extend(owned.iter().copied());
                let lease = Arc::new(Lease {
                    master: Arc::clone(&master),
                    _fd: lease_fd,
                    lessee_id,
                    lifecycle,
                    objects: owned,
                    connector: info.handle(),
                    crtc,
                    encoder,
                    mode,
                });
                drop(reserved);
                let leased = Card(fd.into(), Some(lease));
                enable_atomic(&leased)?;
                return Ok(leased);
            }
        }
        last_error = format!("DRM output {info} has no free compatible encoder/CRTC/primary plane");
    }
    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires explicit EMERGE_DRM_TEST_CARD and two connected displays; acquires DRM master"]
    fn hardware_independent_leases_release_and_reacquire() {
        let path = std::env::var("EMERGE_DRM_TEST_CARD").expect("set explicit test card");
        let outputs = core::outputs(&path).unwrap();
        let connected: Vec<_> = outputs
            .iter()
            .filter(|o| o.connected && !o.modes.is_empty())
            .collect();
        assert!(connected.len() >= 2, "test requires two connected outputs");
        let a = open_output(
            Some(&path),
            Some(&connected[0].name),
            Some(&connected[0].modes[0].id),
            None,
            true,
            None,
        )
        .unwrap();
        let b = open_output(
            Some(&path),
            Some(&connected[1].name),
            Some(&connected[1].modes[0].id),
            None,
            true,
            None,
        )
        .unwrap();
        assert_ne!(a.1.as_ref().unwrap().crtc, b.1.as_ref().unwrap().crtc);
        let canonical = std::fs::canonicalize(&path).unwrap();
        assert!(
            open_output(
                canonical.to_str(),
                Some(&connected[0].name),
                None,
                None,
                false,
                None,
            )
            .is_err()
        );
        // A driver may retain the old open-file description after our renderer retires.
        // Explicit revocation must permit reuse even while that duplicate still exists.
        let _driver_cached_fd = a.0.try_clone().unwrap();
        drop(a);
        let replacement = open_output(
            Some(&path),
            Some(&connected[0].name),
            None,
            None,
            true,
            None,
        )
        .unwrap();
        assert!(b.resource_handles().is_ok());
        assert_ne!(
            replacement.1.as_ref().unwrap().crtc,
            b.1.as_ref().unwrap().crtc
        );
        drop(replacement);
        drop(b);
        assert!(
            open_output(
                Some(&path),
                Some(&connected[0].name),
                None,
                None,
                true,
                None
            )
            .is_ok()
        );
    }
}
