//! Session admission credits, not allocator or driver RSS. A draft and the
//! preceding declaration must each fit in half their account. Other revisions
//! may overlap them, but retry only when the *whole* failed reservation (not
//! just its last increment) can be made again after rollback.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering::*},
};
pub(crate) const BUSY: &str = "live storage is pinned";
pub(crate) const LIMIT: &str = "live transaction exceeds storage limit";
const MIB: usize = 1 << 20;
type Wake = Arc<dyn Fn() + Send + Sync + std::panic::RefUnwindSafe>;
struct Account {
    limit: usize,
    used: AtomicUsize,
    need: AtomicUsize,
    peak: AtomicUsize,
    wake: Wake,
}
#[derive(Clone)]
pub(crate) struct Quota(Arc<Account>);
impl Quota {
    fn new(limit: usize, wake: Wake) -> Self {
        Self(Arc::new(Account {
            limit,
            used: 0.into(),
            need: 0.into(),
            peak: 0.into(),
            wake,
        }))
    }
    pub fn detached_inline() -> Self {
        Self::new(256 * MIB, Arc::new(|| {}))
    }
    pub fn single_limit(&self) -> usize {
        self.0.limit / 2
    }
    pub fn allowance(&self) -> Allowance {
        Allowance {
            quota: self.clone(),
            total: 0,
        }
    }
    pub fn reserve(&self, n: usize) -> Result<Grant, String> {
        self.allowance().reserve(n)
    }
    // Detached staging has no version writer or credit protocol.
    pub fn reserve_inline(&self, n: usize) -> Result<Grant, String> {
        self.take(n, n)
    }
    fn take(&self, n: usize, total: usize) -> Result<Grant, String> {
        let a = &self.0;
        if total > a.limit {
            return Err(LIMIT.into());
        }
        let claim = || {
            a.used.fetch_update(AcqRel, Acquire, |s| {
                s.checked_add(n).filter(|&v| v <= a.limit)
            })
        };
        let old = match claim() {
            Ok(s) => s,
            Err(_) => {
                a.need.store(total, Release);
                // Arm before rechecking: a release racing the failed claim cannot
                // be lost. The provisioner may also have optional pages to trim.
                claim().map_err(|_| {
                    (a.wake)();
                    BUSY
                })?
            }
        };
        a.peak.fetch_max(old + n, Relaxed);
        a.need.store(0, Release);
        Ok(Grant {
            quota: self.clone(),
            n,
        })
    }
    fn available(&self) -> bool {
        self.0.used.load(Acquire) <= self.0.limit - self.0.need.load(Acquire)
    }
    fn pressured(&self) -> bool {
        self.0.need.load(Acquire) != 0
    }
    #[cfg(any(test, feature = "stage-probes"))]
    pub fn bytes(&self) -> (usize, usize) {
        (self.0.used.load(Acquire), self.0.peak.load(Acquire))
    }
}
/// One writer. The transaction's peak promised extents follow it across
/// FEED and PROVE; individual live objects receive splits of these grants.
pub(crate) struct Allowance {
    quota: Quota,
    total: usize,
}
impl Allowance {
    pub fn reserve(&mut self, n: usize) -> Result<Grant, String> {
        let total = self.total.checked_add(n).ok_or(LIMIT)?;
        if total > self.quota.single_limit() {
            return Err(LIMIT.into());
        }
        let g = self.quota.take(n, total)?;
        self.total = total;
        Ok(g)
    }
}
pub(crate) struct Grant {
    quota: Quota,
    n: usize,
}
impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Grant").field(&self.n).finish()
    }
}
impl Grant {
    pub fn bytes(&self) -> usize {
        self.n
    }
    pub fn split(&mut self, n: usize) -> Self {
        self.n = self.n.checked_sub(n).expect("provisioned extent");
        Self {
            quota: self.quota.clone(),
            n,
        }
    }
}
impl Drop for Grant {
    fn drop(&mut self) {
        let a = &self.quota.0;
        let old = a.used.fetch_sub(self.n, AcqRel);
        let limit = a.limit - a.need.load(Acquire);
        if old > limit && old - self.n <= limit {
            (a.wake)();
        }
    }
}
#[derive(Clone)]
pub(crate) struct Storage {
    pub document: Quota,
    pub workspace: Quota,
    pub media: Quota,
}
impl Storage {
    pub fn new(wake: impl Fn() + Send + Sync + std::panic::RefUnwindSafe + 'static) -> Self {
        let wake: Wake = Arc::new(wake);
        Self {
            document: Quota::new(512 * MIB, wake.clone()),
            workspace: Quota::new(1024 * MIB, wake.clone()),
            media: Quota::new(256 * MIB, wake),
        }
    }
    #[cfg(test)]
    pub fn with_limits(document: usize, workspace: usize, media: usize) -> Self {
        let wake: Wake = Arc::new(|| {});
        Self {
            document: Quota::new(document, wake.clone()),
            workspace: Quota::new(workspace, wake.clone()),
            media: Quota::new(media, wake),
        }
    }
    pub fn available(&self) -> bool {
        self.document.available() && self.workspace.available() && self.media.available()
    }
    pub fn trim_pages(&self) -> bool {
        self.document.pressured() || self.media.pressured()
    }
}
impl Default for Storage {
    fn default() -> Self {
        Self::new(|| {})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlap_is_busy_until_the_whole_retry_fits() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let q = Quota::new(
            100,
            Arc::new(move || {
                c.fetch_add(1, Relaxed);
            }),
        );
        let _current = q.reserve(45).unwrap();
        let old = Arc::new(q.reserve(25).unwrap());
        let pin = old.clone();
        let mut draft = q.allowance();
        let wire = draft.reserve(20).unwrap();
        assert_eq!(draft.reserve(20).unwrap_err(), BUSY);
        let armed = calls.load(Relaxed);
        drop(wire);
        drop(old);
        assert!(!q.available());
        assert_eq!(calls.load(Relaxed), armed);
        drop(pin);
        assert!(q.available());
        assert_eq!(calls.load(Relaxed), armed + 1);
        let mut next = q.allowance();
        let _wire = next.reserve(20).unwrap();
        let _records = next.reserve(20).unwrap();
        assert_eq!(q.bytes(), (85, 90));
        assert_eq!(next.reserve(11).unwrap_err(), LIMIT);
    }
}
