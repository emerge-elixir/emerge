//! Backlight primitives for DRM power integration. Presenters do not use these
//! yet; public power/backlight config remains fail-closed. All I/O belongs on the
//! native owner thread, never in a normal-scheduler NIF or a resource destructor.
mod sysfs;
pub use sysfs::{SysfsAttributes, discover};

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeviceId {
    pub device: u64,
    pub inode: u64,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub path: PathBuf,
    pub identity: DeviceId,
    /// Proven connector ancestry, not a shared card/PCI device association.
    pub connector: Option<PathBuf>,
    // Keep the discovered inode alive through selection/open, preventing identity
    // reuse or path replacement from silently redirecting a stale inventory.
    directory: Arc<std::fs::File>,
}

pub enum Selection {
    Unmanaged,
    Auto,
    Explicit(PathBuf),
}

/// Connector paths and discovery candidates must come from the same sysfs
/// inventory. Unknown association permits only an explicit operator override.
pub fn select(
    candidates: &[Candidate],
    connector: &Path,
    selection: &Selection,
) -> Result<Option<Candidate>, String> {
    if matches!(selection, Selection::Unmanaged) {
        return Ok(None);
    }
    let connector = connector.canonicalize().map_err(|e| e.to_string())?;
    match selection {
        Selection::Auto => {
            let mut matches = candidates
                .iter()
                .filter(|candidate| candidate.connector.as_ref() == Some(&connector));
            let selected = matches.next();
            if matches.next().is_some() {
                return Err(
                    "multiple backlights are associated with this connector; select one explicitly"
                        .into(),
                );
            }
            Ok(selected.cloned())
        }
        Selection::Explicit(path) => {
            if !path.is_absolute() {
                return Err("backlight device path must be absolute".into());
            }
            let identity = sysfs::identity(path)?;
            let candidate = candidates
                .iter()
                .find(|candidate| candidate.identity == identity)
                .ok_or("explicit backlight is not in the discovered backlight class")?;
            if candidate
                .connector
                .as_ref()
                .is_some_and(|bound| *bound != connector)
            {
                return Err("explicit backlight belongs to a different connector".into());
            }
            Ok(Some(candidate.clone()))
        }
        Selection::Unmanaged => Ok(None),
    }
}

/// Production can obtain only the process-wide instance (or a clone of it).
/// Tests use isolated registries. Pinned entries survive drops and fail closed.
#[derive(Clone)]
pub struct Reservations(Arc<Mutex<HashSet<DeviceId>>>);

impl Reservations {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(HashSet::new())))
    }

    pub fn process_wide() -> Self {
        static REGISTRY: OnceLock<Reservations> = OnceLock::new();
        REGISTRY.get_or_init(Self::new).clone()
    }

    pub fn reserve(&self, identity: DeviceId) -> Result<Reservation, String> {
        let mut entries = self
            .0
            .lock()
            .map_err(|_| "backlight reservations are poisoned")?;
        if !entries.insert(identity) {
            return Err("backlight is busy or retained after uncertain cleanup".into());
        }
        Ok(Reservation {
            registry: self.clone(),
            identity,
            retained: false,
        })
    }
}

pub struct Reservation {
    registry: Reservations,
    identity: DeviceId,
    retained: bool,
}

impl Reservation {
    /// Use when surrounding KMS/GPU cleanup cannot prove safe reuse either.
    pub fn retain(&mut self) {
        self.retained = true;
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.retained
            && let Ok(mut entries) = self.registry.0.lock()
        {
            entries.remove(&self.identity);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Attribute {
    Maximum,
    Brightness,
    Power,
    Actual,
}

pub trait Attributes {
    /// Only Actual may be absent in a valid binding.
    fn read(&mut self, attribute: Attribute) -> Result<Option<u32>, String>;
    /// One complete sysfs store, not a stream/retry of a partially written value.
    fn write(&mut self, attribute: Attribute, value: u32) -> Result<(), String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Baseline {
    pub identity: DeviceId,
    pub maximum: u32,
    /// Read from brightness, never actual_brightness or a later darkened level.
    pub requested: u32,
}

pub fn quantize(value: f64, maximum: u32) -> Result<u32, String> {
    if maximum == 0 || !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err("brightness requires a finite value in 0..1 and a positive maximum".into());
    }
    Ok((value * f64::from(maximum)).round() as u32)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Untouched,
    Dark,
    Visible,
    Unknown,
}

pub struct Backlight<A: Attributes> {
    attributes: A,
    reservation: Reservation,
    baseline: Baseline,
    desired: u32,
    phase: Phase,
    release_safe: bool,
}

impl Backlight<SysfsAttributes> {
    /// Open/validate everything before any write. Pinned file descriptors prevent
    /// a symlink/path replacement from redirecting later operations.
    pub fn open(
        candidate: &Candidate,
        reservations: &Reservations,
        previous: Option<Baseline>,
    ) -> Result<Self, String> {
        let reservation = reservations.reserve(candidate.identity)?;
        let attributes = SysfsAttributes::open(candidate)?;
        Self::prepare(attributes, reservation, previous)
    }
}

impl<A: Attributes> Backlight<A> {
    pub fn prepare(
        mut attributes: A,
        reservation: Reservation,
        previous: Option<Baseline>,
    ) -> Result<Self, String> {
        let maximum = attributes
            .read(Attribute::Maximum)?
            .ok_or("max_brightness is missing")?;
        if maximum == 0 {
            return Err("max_brightness must be positive".into());
        }
        let requested = attributes
            .read(Attribute::Brightness)?
            .ok_or("brightness is missing")?;
        if requested > maximum {
            return Err("requested brightness exceeds max_brightness".into());
        }
        let power = attributes
            .read(Attribute::Power)?
            .ok_or("bl_power is missing")?;
        if power > 4 {
            return Err("invalid backlight power value".into());
        }
        let baseline = previous.unwrap_or(Baseline {
            identity: reservation.identity,
            maximum,
            requested,
        });
        if baseline.identity != reservation.identity
            || baseline.maximum != maximum
            || baseline.requested > maximum
        {
            return Err("backlight baseline does not match the bound device".into());
        }
        Ok(Self {
            attributes,
            reservation,
            baseline,
            desired: baseline.requested,
            phase: Phase::Untouched,
            release_safe: true,
        })
    }

    pub fn baseline(&self) -> Baseline {
        self.baseline
    }
    pub fn desired_level(&self) -> u32 {
        self.desired
    }

    pub fn actual_level(&mut self) -> Result<Option<u32>, String> {
        self.attributes.read(Attribute::Actual)
    }

    /// Dark/unknown devices retain intent without writes. Updating an already
    /// visible device changes brightness only; zero is not a power-off request.
    pub fn set_brightness(&mut self, value: Option<f64>) -> Result<(), String> {
        let level = value
            .map(|value| quantize(value, self.baseline.maximum))
            .transpose()?
            .unwrap_or(self.baseline.requested);
        self.desired = level;
        if self.phase == Phase::Unknown {
            return Err(
                "backlight state is uncertain; suppress it before applying brightness".into(),
            );
        }
        if self.phase == Phase::Visible {
            self.phase = Phase::Unknown;
            self.attributes.write(Attribute::Brightness, level)?;
            self.phase = Phase::Visible;
        }
        Ok(())
    }

    /// Both suppression mechanisms are attempted, including after the first
    /// fails. A failure is never treated as proof that illumination is off.
    pub fn darken(&mut self) -> Result<(), String> {
        self.release_safe = false;
        self.phase = Phase::Unknown;
        let power = self.attributes.write(Attribute::Power, 4);
        let brightness = self.attributes.write(Attribute::Brightness, 0);
        match (power, brightness) {
            (Ok(()), Ok(())) => {
                self.phase = Phase::Dark;
                Ok(())
            }
            (Err(a), Err(b)) => Err(format!(
                "power-down failed: {a}; zero brightness failed: {b}"
            )),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }

    /// The presenter must hold its current-intent guard and have observed matching
    /// frame/presentation completion before calling this. This primitive does not
    /// manufacture that proof. Public controls stay disabled until that wiring lands.
    pub fn illuminate_after_present(&mut self) -> Result<(), String> {
        if self.phase != Phase::Dark {
            return Err(
                "backlight must be suppressed before presentation-gated illumination".into(),
            );
        }
        self.phase = Phase::Unknown;
        // Set brightness while power is still down. A failed brightness write
        // must never proceed to unblanking.
        if let Err(error) = self.attributes.write(Attribute::Brightness, self.desired) {
            return self.failed_enable(error);
        }
        if let Err(error) = self.attributes.write(Attribute::Power, 0) {
            return self.failed_enable(error);
        }
        self.phase = Phase::Visible;
        Ok(())
    }

    fn failed_enable(&mut self, error: String) -> Result<(), String> {
        match self.darken() {
            Ok(()) => Err(error),
            Err(cleanup) => Err(format!(
                "{error}; suppression after failed enable also failed: {cleanup}"
            )),
        }
    }

    /// Surrounding presenter cleanup failure pins this identity even if subsequent
    /// sysfs suppression succeeds. No sibling may reuse uncertain ownership.
    pub fn retain(&mut self) {
        self.reservation.retain();
    }

    /// Only after pending enables are cancelled and KMS/GPU owners are safely
    /// detached. Leave dark; never restore the captured brightness on shutdown.
    pub fn release_after_detach(mut self) -> Result<(), String> {
        self.darken()?;
        self.release_safe = true;
        Ok(())
    }
}

impl<A: Attributes> Drop for Backlight<A> {
    fn drop(&mut self) {
        // No sysfs I/O in Drop. Forgetting explicit cleanup, or failed cleanup,
        // retains admission rather than enabling unsafe sibling reuse.
        if !self.release_safe {
            self.reservation.retain();
        }
    }
}

#[cfg(test)]
mod tests;
