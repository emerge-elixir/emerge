//! Reduce a prefix expression once at ingress. Layout only needs the weight
//! and interval; final-site admission also needs the largest written px leaf.
use super::*;

const MAX_LENGTH_DEPTH: usize = 64;

impl Reader<'_> {
    pub(super) fn length(&mut self) -> Result<(Length, f64)> {
        let tag = self.byte()?;
        let mut largest = 0.;
        let l = if matches!(tag, 4 | 5) {
            self.bounded_length(tag, &mut largest)?
        } else {
            self.length_leaf(tag, &mut largest)?
        };
        Ok((l, largest))
    }
    fn length_leaf(&mut self, tag: u8, largest: &mut f64) -> Result<Length> {
        match tag {
            0 => Ok(Length::Fill),
            1 => Ok(Length::Content),
            2 => {
                let px = self.number()?;
                *largest = largest.max(px);
                Ok(Length::Px(px))
            }
            3 => {
                let w = f64::from_be_bytes(self.array()?);
                if w.is_finite() && w > 0. {
                    Ok(Length::Weighted(w))
                } else {
                    Err("fill weight must be finite and positive".into())
                }
            }
            v => Err(format!("unsupported length {v}")),
        }
    }
    fn bounded_length(&mut self, root: u8, largest: &mut f64) -> Result<Length> {
        // Each open operator needs at most its completed left operand. No
        // recursive calls or per-expression allocation on fitting edits.
        let mut stack = [(0, None); MAX_LENGTH_DEPTH];
        let mut depth = 0;
        let mut tag = root;
        loop {
            if matches!(tag, 4 | 5) {
                let slot = stack
                    .get_mut(depth)
                    .ok_or("length expression depth exceeds 64")?;
                *slot = (tag, None);
                depth += 1;
            } else {
                let mut part = self.length_leaf(tag, largest)?;
                while depth > 0 {
                    let (op, left) = &mut stack[depth - 1];
                    let Some(a) = left.take() else {
                        *left = Some(part);
                        break;
                    };
                    part = fold(*op, a, part)?;
                    depth -= 1;
                }
                if depth == 0 {
                    return if part.bounds().is_some() {
                        Ok(part)
                    } else {
                        Err(format!("unsupported length {root}"))
                    };
                }
            }
            tag = self.byte()?;
        }
    }
}

fn fold(op: u8, a: Length, b: Length) -> Result<Length> {
    let f = if op == 4 { f64::min } else { f64::max };
    match (a, b) {
        (Length::Px(x), Length::Px(y)) => Ok(Length::Px(f(x, y))),
        (Length::Px(px), l) | (l, Length::Px(px)) => {
            let weight = l
                .weight()
                .ok_or_else(|| format!("unsupported length {op}"))?;
            // Both endpoints see the outer operation. In particular a cap
            // below an inner floor collapses to the cap, not to that floor.
            let bounds = l.bounds().unwrap_or(Bounds::OPEN).map(|end| f(end, px));
            // A collapsed interval is constant at every offer; its written
            // weight still round-trips but cannot affect redistribution.
            let weight = if bounds.floor == bounds.cap {
                1.
            } else {
                weight
            };
            Ok(Length::Bounded { bounds, weight })
        }
        _ => Err(format!("unsupported length {op}")),
    }
}
