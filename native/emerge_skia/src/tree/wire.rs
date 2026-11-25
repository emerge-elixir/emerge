//! EMRG v10. One replacement proof and commit for all declaration entry points.
use super::*;
mod batch;
mod length;
pub(super) mod media;
mod patch;
mod upload;
#[derive(Default)]
pub(super) struct Scratch {
    patch: Option<batch::Batch>,
}
type Result<T> = std::result::Result<T, String>;
const MAX_BYTES: usize = 64 * 1024 * 1024;
pub(super) struct Reader<'a>(pub(super) &'a [u8]);
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
    pub(super) fn long(&mut self) -> Result<usize> {
        Ok(u32::from_be_bytes(self.array()?) as usize)
    }
    fn id(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn alignment(&mut self, name: &str) -> Result<Alignment> {
        Ok(match self.byte()? {
            0 => Alignment::Start,
            1 => Alignment::Center,
            2 => Alignment::End,
            v => return Err(format!("invalid {name} {v}")),
        })
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
    pub(super) fn signed(&mut self) -> Result<f64> {
        let v = f64::from_be_bytes(self.array()?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err("expected finite number".into())
        }
    }
    pub(super) fn flag(&mut self) -> Result<bool> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            v => Err(format!("invalid boolean {v}")),
        }
    }
    fn quad(&mut self) -> Result<[f64; 4]> {
        match self.byte()? {
            0 => Ok([self.number()?; 4]),
            1 => Ok([
                self.number()?,
                self.number()?,
                self.number()?,
                self.number()?,
            ]),
            v => Err(format!("invalid four-edge value {v}")),
        }
    }
    pub(super) fn ink(&mut self, raw: &[u8]) -> Result<(Option<u32>, decoration::Bytes)> {
        if self.0.first() != Some(&3) {
            return Ok((Some(self.color()?), Default::default()));
        }
        let start = self.0;
        self.byte()?;
        let n = self.long()?;
        if n < 2 || n > self.0.len() / 3 {
            return Err("invalid gradient stop count".into());
        }
        for _ in 0..n {
            self.color()?;
        }
        self.signed()?;
        Ok((
            None,
            decoration::Bytes::of(raw, &start[..start.len() - self.0.len()]),
        ))
    }
    fn shadows(&mut self, raw: &[u8]) -> Result<(decoration::Bytes, f64)> {
        let start = self.0;
        let mut largest = 0f64;
        for _ in 0..self.byte()? {
            for n in [
                self.signed()?,
                self.signed()?,
                self.number()?,
                self.signed()?,
            ] {
                largest = largest.max(n.abs());
            }
            self.ink(raw)?;
            self.flag()?;
        }
        Ok((
            decoration::Bytes::of(raw, &start[..start.len() - self.0.len()]),
            largest,
        ))
    }
    pub(super) fn color(&mut self) -> Result<u32> {
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
                "green" => 0x00ff00ff,
                "cyan" => 0x00ffffff,
                "magenta" => 0xff00ffff,
                "yellow" => 0xffff00ff,
                "orange" => 0xffa500ff,
                "purple" => 0x800080ff,
                "pink" => 0xffc0cbff,
                "gray" | "grey" => 0x808080ff,
                "navy" => 0x000080ff,
                "teal" => 0x008080ff,
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
            1 => (a.sizing.width, largest_px[0], a.line_floor) = r.length(raw)?,
            2 => (a.sizing.height, largest_px[1], a.height_floor) = r.length(raw)?,
            3 => {
                a.image_host_clip = true;
                a.sizing.padding = match r.byte()? {
                    0 => [r.number()?; 4],
                    1 | 2 => [r.number()?, r.number()?, r.number()?, r.number()?],
                    v => return Err(format!("unsupported padding {v}")),
                }
            }
            4 => a.sizing.spacing = [r.number()?; 2],
            36 => a.sizing.spacing = [r.number()?, r.number()?],
            37 => {
                a.space_evenly = match r.byte()? {
                    0 => false,
                    1 => true,
                    v => return Err(format!("invalid space_evenly flag {v}")),
                }
            }
            tag @ (5 | 6) => a.align[usize::from(tag - 5)] = Some(r.alignment("alignment")?),
            30 => a.text_align = Some(r.alignment("text alignment")?),
            12 if r.byte()? == 0 => (a.background, a.decor.background_gradient) = r.ink(raw)?,
            13 => {
                a.decor.radius = r.quad()?;
                a.image_host_clip = true;
            }
            14 => {
                a.decor.border = r.quad()?;
                a.image_host_clip = true;
            }
            15 => (a.decor.border_color, a.decor.border_gradient) = r.ink(raw)?,
            51 => {
                a.decor.style = r.byte()?;
                if a.decor.style > 2 {
                    return Err("invalid border style".into());
                }
            }
            52 => (a.decor.shadows, a.decor.max_shadow) = r.shadows(raw)?,
            35 if matches!(shape, Shape::Image | Shape::El) => {
                let alpha = (r.signed()?.clamp(0., 1.) as f32 * 255.).round() as u8;
                a.opacity = (alpha != 255).then_some(alpha);
            }
            31 => a.offset[0] = r.signed()?,
            32 => a.offset[1] = r.signed()?,
            47 => a.underline = Some(r.flag()?),
            48 => a.strike = Some(r.flag()?),
            16 => a.font_size = Some(r.number()?),
            78 => {
                let s = f64::from_be_bytes(r.array()?);
                if !s.is_finite() || s <= 0. {
                    return Err("layout scale must be finite and positive".into());
                }
                a.layout_scale = Some(s);
            }
            17 => (a.color, a.font_gradient) = r.ink(raw)?,
            18 if r.byte()? <= 1 => {
                a.family = Some(match r.string()? {
                    "default" | "sans-serif" => Family::Sans,
                    "monospace" | "JetBrains Mono NL" | "JetBrains Mono" => Family::Mono,
                    v => return Err(format!("unported font family {v}")),
                })
            }
            19 => {
                a.weight = Some(match r.string()? {
                    "normal" | "regular" => text::Weight::Regular,
                    "bold" => text::Weight::Bold,
                    "thin" | "extra_light" | "light" | "medium" => text::Weight::OtherLight,
                    "semi_bold" | "extra_bold" | "black" => text::Weight::OtherHeavy,
                    s => return Err(format!("unsupported font weight {s}")),
                })
            }
            20 => {
                a.italic = Some(match r.string()? {
                    "italic" => true,
                    "normal" => false,
                    s => return Err(format!("unsupported font style {s}")),
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
    if shape == Shape::Image {
        if a.image_src.len == 0 {
            return Err("image requires a source".into());
        }
        if !a.svg_expected && (a.svg_color.is_some() || a.svg_gradient.len != 0) {
            return Err("template color requires an SVG leaf".into());
        }
        if !matches!(
            a.sizing.width,
            Length::Px(_) | Length::Fill | Length::Weighted(_) | Length::Auto | Length::Content
        ) || !matches!(
            a.sizing.height,
            Length::Px(_) | Length::Fill | Length::Weighted(_) | Length::Auto | Length::Content
        ) || (!a.sizing.width.definite() && a.sizing.height.weight().is_some())
        {
            return Err(
                "unported image sizing: declare a pixel axis or fill width with automatic height"
                    .into(),
            );
        }
    }
    // Layout reads padding plus the border inset; paint retains the border itself.
    a.sizing.padding = std::array::from_fn(|i| a.sizing.padding[i] + a.decor.border[i]);
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
        let mut seen = std::collections::HashSet::new();
        let mut stack: Vec<_> = self.root.into_iter().collect();
        while let Some(k) = stack.pop() {
            let e = &self.elements;
            let n = &e.slots[k];
            out.extend(n.id.to_be_bytes());
            out.extend([n.shape as u8, 3]);
            let raw = e.raw(k);
            e.media.write(&e.attrs[k], raw, &mut seen, &mut out);
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
