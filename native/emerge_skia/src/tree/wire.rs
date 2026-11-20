//! EMRG v10. One replacement proof and commit for all declaration entry points.
use super::*;
mod batch;
mod length;
mod patch;
mod upload;
#[derive(Default)]
pub(super) struct Scratch {
    patch: Option<batch::Batch>,
}
type Result<T> = std::result::Result<T, String>;
const MAX_BYTES: usize = 64 * 1024 * 1024;
pub(super) struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    pub(super) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let (v, rest) = self.0.split_at_checked(n).ok_or("truncated EMRG data")?;
        self.0 = rest;
        Ok(v)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    pub(super) fn byte(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }
    pub(super) fn count(&mut self) -> Result<usize> {
        Ok(u16::from_be_bytes(self.array()?) as usize)
    }
    fn long(&mut self) -> Result<usize> {
        Ok(u32::from_be_bytes(self.array()?) as usize)
    }
    fn id(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn number(&mut self) -> Result<f64> {
        let n = f64::from_be_bytes(self.array()?);
        if n.is_finite() && n >= 0. {
            Ok(n)
        } else {
            Err("expected finite non-negative size".into())
        }
    }
    fn block(&mut self) -> Result<&'a [u8]> {
        let n = self.long()?;
        self.take(n)
    }
    pub(super) fn string(&mut self) -> Result<&'a str> {
        let n = self.count()?;
        std::str::from_utf8(self.take(n)?).map_err(|_| "invalid UTF-8".into())
    }
    fn children(&mut self) -> Result<&'a [u8]> {
        let n = self.count()?;
        self.take(n * 8)
    }
    fn color(&mut self) -> Result<u32> {
        Ok(match self.byte()? {
            0 => {
                let [r, g, b] = self.array()?;
                u32::from_be_bytes([r, g, b, 255])
            }
            1 => u32::from_be_bytes(self.array()?),
            2 => match self.string()? {
                "black" => 0xff,
                "white" => 0xffffffff,
                "red" => 0xff0000ff,
                "blue" => 0xffff,
                "transparent" => 0,
                v => return Err(format!("unsupported named color {v}")),
            },
            v => return Err(format!("unsupported color {v}")),
        })
    }
}
fn ids(bytes: &[u8]) -> impl Iterator<Item = u64> + '_ {
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .copied()
        .map(u64::from_be_bytes)
}
pub(super) fn range(all: &[u8], part: &[u8]) -> Range<usize> {
    let start = part.as_ptr() as usize - all.as_ptr() as usize;
    start..start + part.len()
}
fn attrs(raw: &[u8], shape: Shape) -> Result<Attrs> {
    let mut r = Reader(raw);
    let mut a = Attrs::default();
    let mut largest_px = [0.; 2];
    for _ in 0..r.count()? {
        match r.byte()? {
            1 => (a.sizing.width, largest_px[0], a.line_minimum) = r.length(raw)?,
            2 => (a.sizing.height, largest_px[1], _) = r.length(raw)?,
            3 => {
                a.sizing.padding = match r.byte()? {
                    0 => [r.number()?; 4],
                    1 | 2 => [r.number()?, r.number()?, r.number()?, r.number()?],
                    v => return Err(format!("unsupported padding {v}")),
                }
            }
            4 => a.sizing.spacing = [r.number()?; 2],
            36 => a.sizing.spacing = [r.number()?, r.number()?],
            5 | 6 | 30 if r.byte()? == 0 => (),
            12 if r.byte()? == 0 => a.background = Some(r.color()?),
            16 => a.font_size = Some(r.number()?),
            78 => {
                let s = f64::from_be_bytes(r.array()?);
                if !s.is_finite() || s <= 0. {
                    return Err("layout scale must be finite and positive".into());
                }
                a.layout_scale = Some(s);
            }
            17 => a.color = Some(r.color()?),
            18 if r.byte()? <= 1 => {
                a.family = Some(match r.string()? {
                    "default" | "sans-serif" => Family::Sans,
                    "monospace" => Family::Mono,
                    v => return Err(format!("unported font family {v}")),
                })
            }
            40 => {
                a.click = match r.byte()? {
                    0 => false,
                    1 => true,
                    _ => return Err("invalid click flag".into()),
                }
            }
            tag => shape.decode_attr(tag, &mut r, raw, &mut a)?,
        }
    }
    if !r.0.is_empty() {
        return Err("trailing attribute bytes".into());
    }
    a.largest_length_px = largest_px[0].max(largest_px[1]);
    Ok(a)
}
impl Tree {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = b"EMRG\x0a".to_vec();
        out.extend(0u64.to_be_bytes());
        out.extend(
            self.root
                .map_or(0, |k| self.elements.slots[k].id)
                .to_be_bytes(),
        );
        out.extend((self.elements.attached_count() as u32).to_be_bytes());
        let mut stack: Vec<_> = self.root.into_iter().collect();
        while let Some(k) = stack.pop() {
            let e = &self.elements;
            let n = &e.slots[k];
            out.extend(n.id.to_be_bytes());
            out.extend([n.shape as u8, 3]);
            let raw = e.raw(k);
            out.extend((raw.len() as u32).to_be_bytes());
            out.extend(raw);
            let children = e.children(k);
            out.extend((children.len() as u16).to_be_bytes());
            for &c in children {
                out.extend(e.slots[c].id.to_be_bytes())
            }
            out.extend([0, 0]);
            stack.extend(children.iter().rev())
        }
        out
    }
}
