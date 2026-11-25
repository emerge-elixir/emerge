//! An inline definition is owned by the accepted declaration, not the decoder LRU.
use super::*;
use sha2::{Digest as _, Sha256};

pub(crate) type Digest = [u8; 32];
pub(crate) fn digest(bytes: &[u8]) -> Digest {
    Sha256::digest(bytes).into()
}

#[derive(Clone, Default)]
pub(crate) struct InputBudget(Arc<AtomicUsize>);
#[derive(Debug)]
pub(crate) struct Media {
    bytes: Box<[u8]>,
    _lease: Charge,
}
impl InputBudget {
    pub fn reserve(&self, len: usize) -> Result<Charge, String> {
        // Includes concurrent staging, the accepted declaration, and superseded
        // tickets still reading their input. Reserve before copying BEAM memory.
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(len).filter(|&n| n <= MAX_RESIDENT)
            })
            .map_err(|_| "resident inline source limit")?;
        Ok(Charge {
            resident: self.0.clone(),
            bytes: len,
        })
    }
    pub fn copy(&self, bytes: &[u8]) -> Result<Arc<Media>, String> {
        let lease = self.reserve(bytes.len() + 256)?; // Arc and definition/index entries
        Ok(Arc::new(Media {
            bytes: bytes.into(),
            _lease: lease,
        }))
    }
}
impl Media {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
#[derive(Debug)]
pub(crate) enum Input {
    Logical(String),
    Runtime(String),
    Bytes(Digest, Arc<Media>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum AssetKey {
    Name(Arc<str>),
    Content(Digest),
}
