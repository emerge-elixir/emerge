//! One bounded prefix walk. At ingress reduce common policies to a constant
//! or an interval; keep byte offsets only for a genuinely two-input formula.
use super::*;

const MAX_LENGTH_DEPTH: usize = 64;

impl Reader<'_> {
    pub(super) fn length(&mut self, raw: &[u8]) -> Result<(Length, f64, Option<Bounds<f64>>)> {
        let start = raw.len() - self.0.len();
        let mut largest = 0f64;
        let (mut length, floor) = self.expression(
            |leaf| {
                let length = match leaf {
                    Leaf::Fill => Length::Fill,
                    Leaf::Weight(w) => Length::Weighted(w),
                    Leaf::Content => Length::Content,
                    Leaf::Px(px) => {
                        largest = largest.max(px);
                        Length::Px(px)
                    }
                };
                let floor = length.weight().is_none().then(|| length.natural_bounds());
                (length, floor)
            },
            |min, (a, left), (b, right)| {
                // Fill alone promises no floor (different from an authored
                // zero). Min needs a guarantee from both sides; max only one.
                let floor = match (left, right) {
                    (Some(x), Some(y)) => Some(x.combine(y, min)),
                    _ if min => None,
                    _ => left.or(right),
                };
                (fold(min, a, b), floor)
            },
        )?;
        if let Length::Formula(f) = &mut length {
            // An EMRG body is limited to 64 MiB before attr parsing.
            f.start = start as u32;
            f.end = (raw.len() - self.0.len()) as u32;
        }
        Ok((length, largest, length.weight().and(floor)))
    }
    fn leaf(&mut self, tag: u8) -> Result<Leaf> {
        match tag {
            0 => Ok(Leaf::Fill),
            1 => Ok(Leaf::Content),
            2 => Ok(Leaf::Px(self.number()?)),
            3 => {
                let w = f64::from_be_bytes(self.array()?);
                if w.is_finite() && w > 0. {
                    Ok(Leaf::Weight(w))
                } else {
                    Err("fill weight must be finite and positive".into())
                }
            }
            v => Err(format!("unsupported length {v}")),
        }
    }
    fn expression<T: Copy>(
        &mut self,
        mut leaf: impl FnMut(Leaf) -> T,
        mut op: impl FnMut(bool, T, T) -> T,
    ) -> Result<T> {
        let mut tag = self.byte()?;
        if !matches!(tag, 4 | 5) {
            return Ok(leaf(self.leaf(tag)?));
        }
        let mut stack = [(false, None); MAX_LENGTH_DEPTH];
        let mut depth = 0;
        loop {
            if matches!(tag, 4 | 5) {
                let slot = stack
                    .get_mut(depth)
                    .ok_or("length expression depth exceeds 64")?;
                *slot = (tag == 4, None);
                depth += 1;
            } else {
                let mut part = leaf(self.leaf(tag)?);
                loop {
                    if depth == 0 {
                        return Ok(part);
                    }
                    let (min, left) = &mut stack[depth - 1];
                    let Some(a) = left.take() else {
                        *left = Some(part);
                        break;
                    };
                    part = op(*min, a, part);
                    depth -= 1;
                }
            }
            tag = self.byte()?;
        }
    }
}

enum Leaf {
    Fill,
    Content,
    Px(f64),
    Weight(f64),
}

impl Formula {
    pub(in crate::tree) fn bytes(self, raw: &[u8]) -> &[u8] {
        &raw[self.start as usize..self.end as usize]
    }
    pub(in crate::tree) fn eval<T: Copy>(
        self,
        raw: &[u8],
        content: T,
        px: impl Fn(f64) -> T,
        share: impl Fn(f64) -> T,
        op: impl FnMut(bool, T, T) -> T,
    ) -> T {
        Reader(self.bytes(raw))
            .expression(
                |leaf| match leaf {
                    Leaf::Fill => share(1.),
                    Leaf::Weight(w) => share(w),
                    Leaf::Content => content,
                    Leaf::Px(n) => px(n),
                },
                op,
            )
            .expect("validated length in the same owner's attr block")
    }
}

fn fold(min: bool, a: Length, b: Length) -> Length {
    // These natural bounds are also global output limits: clipping an
    // input (including content) to them cannot change the result.
    let (x, y) = (a.natural_bounds(), b.natural_bounds());
    let bounds = x.combine(y, min);
    let reads_content = |l: Length| {
        matches!(l, Length::Content | Length::BoundedContent(_)) || l.reads_fill_content()
    };
    let content = reads_content(a) || reads_content(b);
    let (weight, same) = match (a.weight(), b.weight()) {
        (None, None) => {
            return if !content {
                Length::Px(bounds.floor)
            } else if bounds == Bounds::OPEN {
                Length::Content
            } else {
                Length::BoundedContent(bounds)
            };
        }
        (Some(w), None) | (None, Some(w)) => (w, true),
        (Some(x), Some(y)) => (if min { x.min(y) } else { x.max(y) }, x == y),
    };
    if bounds.floor == bounds.cap {
        return Length::BoundedFill { bounds, weight: 1. };
    }
    // For equal slopes min/max combines just the two endpoints. Without
    // bounds, even unequal rays never cross on the nonnegative half-line.
    if !content
        && !matches!(a, Length::Formula(_))
        && !matches!(b, Length::Formula(_))
        && (same || (x == Bounds::OPEN && y == Bounds::OPEN))
    {
        return Length::BoundedFill { bounds, weight };
    }
    Length::Formula(Formula {
        start: 0,
        end: 0,
        natural: bounds,
        weight,
        reads_content: content,
    })
}
