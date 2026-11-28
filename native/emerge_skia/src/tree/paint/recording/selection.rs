//! Only ordinal intervals enter a frame. The common case is an inline pair;
//! a disjoint selection pins pages of run endpoints, not a list of children.
use super::*;
#[derive(Clone)]
pub(super) enum Selection {
    One(Range<usize>),
    Many(pages::Frozen<Range<usize>>),
}
impl Default for Selection {
    fn default() -> Self {
        Self::One(0..0)
    }
}
impl Selection {
    fn len(&self) -> usize {
        match self {
            Self::One(r) => usize::from(!r.is_empty()),
            Self::Many(p) => p.len(),
        }
    }
    fn at(&self, i: usize) -> &Range<usize> {
        match self {
            Self::One(r) => r,
            Self::Many(p) => p.at(i),
        }
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Range<usize>> {
        (0..self.len()).map(|i| self.at(i))
    }
    // On a mixed order each segment sees only its overlapping runs, not
    // every disjoint interval once per exporter (also in reverse hit order).
    pub fn within(&self, slice: Option<Segment>) -> impl DoubleEndedIterator<Item = &Range<usize>> {
        let cut = |mut lo, mut hi, p: &dyn Fn(&Range<usize>) -> bool| {
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if p(self.at(mid)) {
                    lo = mid + 1
                } else {
                    hi = mid
                }
            }
            lo
        };
        let span = slice.map_or(0..self.len(), |s| {
            let lo = cut(0, self.len(), &|r| r.end <= s.first as usize);
            lo..cut(lo, self.len(), &|r| r.start < s.end as usize)
        });
        span.map(|i| self.at(i))
    }
    pub fn contains(&self, i: usize) -> bool {
        let (mut l, mut r) = (0, self.len());
        while l < r {
            let mid = l + (r - l) / 2;
            let v = self.at(mid);
            if i < v.start {
                r = mid
            } else if i >= v.end {
                l = mid + 1
            } else {
                return true;
            }
        }
        false
    }
    pub fn difference(&self, b: &Self, mut f: impl FnMut(Range<usize>)) {
        let mut j = 0;
        for a in self.iter() {
            let mut i = a.start;
            while i < a.end {
                while j < b.len() && b.at(j).end <= i {
                    j += 1;
                }
                if j == b.len() || b.at(j).start >= a.end {
                    f(i..a.end);
                    break;
                }
                let r = b.at(j);
                if i < r.start {
                    f(i..r.start);
                }
                i = i.max(r.end);
            }
        }
    }
    pub fn save(&self, runs: &[Range<usize>], pages: &Pages<Range<usize>, 8>) -> Self {
        match runs {
            [] => Self::default(),
            [r] => Self::One(r.clone()),
            _ if self.iter().eq(runs) => self.clone(),
            _ => Self::Many(pages.save(runs)),
        }
    }
}
// Front-to-back occlusion is independent of hit selection. Use only proofs
// valid in an untranslated pixel phase; fractional/rounded/posed ancestors
// can reveal edge pixels of an otherwise fully covered sibling.
pub(super) fn allows_coverage(e: &Elements, l: &Layers, k: Key, shift: [f32; 2]) -> bool {
    let ctx = l.painted(k);
    if ctx.transformed || e.slots[k].stats.mounts != 0 || e.attrs[k].opacity.is_some() {
        return false;
    }
    let r = ctx.child_clip;
    [shift[0], shift[1], r.x, r.y, r.x + r.width, r.y + r.height]
        .into_iter()
        .all(|x| x.fract() == 0.)
        && std::iter::successors(Some(k), |&p| e.slots[p].parent)
            .all(|p| e.attrs[p].decor.radius == [0.; 4])
}
