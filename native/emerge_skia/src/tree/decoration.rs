//! Fixed fields and borrowed ranges into one validated attribute block. A
//! prepared drawing copies variable inputs once per contributing owner.
use super::*;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Bytes {
    pub start: u32,
    pub len: u32,
}
impl Bytes {
    pub fn of(raw: &[u8], part: &[u8]) -> Self {
        Self {
            start: wire::range(raw, part).start as u32,
            len: part.len() as u32,
        }
    }
    pub fn get(self, raw: &[u8]) -> &[u8] {
        &raw[self.start as usize..self.start as usize + self.len as usize]
    }
    pub fn differs(self, raw: &[u8], other: Self, next: &[u8]) -> bool {
        self.get(raw) != other.get(next)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Decor {
    pub max_shadow: f64,
    pub radius: [f64; 4],
    pub border: [f64; 4],
    pub border_color: Option<u32>,
    pub style: u8,
    pub border_gradient: Bytes,
    pub background_gradient: Bytes,
    pub shadows: Bytes,
}
impl Decor {
    pub fn active(self) -> bool {
        self != Self::default()
    }
    pub fn differs(self, raw: &[u8], other: Self, next: &[u8]) -> bool {
        self != other
            || self.shadows.differs(raw, other.shadows, next)
            || self
                .border_gradient
                .differs(raw, other.border_gradient, next)
            || self
                .background_gradient
                .differs(raw, other.background_gradient, next)
    }
}
impl Attrs {
    pub fn material(&self) -> bool {
        self.decor.active() || self.font_gradient.len != 0
    }
    pub fn material_bytes(&self) -> usize {
        [
            self.font_gradient,
            self.decor.shadows,
            self.decor.background_gradient,
            self.decor.border_gradient,
        ]
        .into_iter()
        .map(|s| s.len as usize)
        .sum()
    }
    pub fn has_rules(&self) -> bool {
        self.underline == Some(true) || self.strike == Some(true)
    }
}
