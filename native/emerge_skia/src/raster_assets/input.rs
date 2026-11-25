//! An inline definition is owned by the accepted declaration, not the decoder LRU.
use super::*;
use crate::tree::storage::{Grant, Quota};
use sha2::{Digest as _, Sha256};

pub(crate) type Digest = [u8; 32];
pub(crate) fn digest(bytes: &[u8]) -> Digest {
    Sha256::digest(bytes).into()
}

#[derive(Clone)]
pub(crate) struct InputBudget(Quota);
impl Default for InputBudget {
    fn default() -> Self {
        Self(Quota::detached_inline())
    }
}
#[derive(Debug)]
pub(crate) struct Media {
    bytes: Vec<u8>,
    span: std::ops::Range<usize>,
    lease: Grant,
}
impl InputBudget {
    pub fn reserve(&self, len: usize) -> Result<Grant, String> {
        self.0
            .reserve_inline(len)
            .map_err(|_| "resident inline source limit".into())
    }
    pub fn copy(&self, bytes: &[u8]) -> Result<Arc<Media>, String> {
        let lease = self.reserve(bytes.len() + 256)?;
        Ok(Media::adopt(bytes.to_vec(), 0..bytes.len(), lease))
    }
}
impl Media {
    /// Adopt the streamed allocation. Charge the entire backing, not just the
    /// payload span, and do not shrink/copy it when switching ownership.
    pub fn adopt(bytes: Vec<u8>, span: std::ops::Range<usize>, lease: Grant) -> Arc<Self> {
        Arc::new(Self { bytes, span, lease })
    }
    pub fn extent(&self) -> usize {
        self.lease.bytes()
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[self.span.clone()]
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
