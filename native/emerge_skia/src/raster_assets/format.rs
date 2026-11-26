use super::*;
mod webp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Png,
    Jpeg,
    Bmp,
    Webp,
    Gif,
    Svg,
}
impl Format {
    pub fn logical(name: &str) -> Option<Self> {
        files::valid_logical(name).then(|| Self::extension(name))?
    }
    pub(super) fn extension(name: &str) -> Option<Self> {
        let ext = Path::new(name).extension()?;
        if ext.eq_ignore_ascii_case("png") {
            Some(Self::Png)
        } else if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") {
            Some(Self::Jpeg)
        } else if ext.eq_ignore_ascii_case("gif") {
            Some(Self::Gif)
        } else if ext.eq_ignore_ascii_case("webp") {
            Some(Self::Webp)
        } else if ext.eq_ignore_ascii_case("bmp") {
            Some(Self::Bmp)
        } else if ext.eq_ignore_ascii_case("svg") {
            Some(Self::Svg)
        } else {
            None
        }
    }

    pub(super) fn inline(bytes: &[u8]) -> Self {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Self::Png
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            Self::Gif
        } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()) {
            Self::Webp
        } else if bytes.starts_with(b"BM") {
            Self::Bmp
        } else if bytes.starts_with(b"\xff\xd8") {
            Self::Jpeg
        } else {
            Self::Svg
        }
    }
    pub(super) fn limit(self) -> usize {
        if self == Self::Svg { 4 * MIB } else { MAX_FILE }
    }

    pub(super) fn codec(self, data: Data) -> Option<Codec<'static>> {
        match self {
            Self::Png => {
                let ihdr = data.strip_prefix(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")?;
                let (w, h) = ihdr.split_first_chunk::<4>()?;
                bitmap_bytes(
                    i32::from_be_bytes(*w),
                    i32::from_be_bytes(*h.first_chunk()?),
                )?;
                if !data.ends_with(b"\0\0\0\0IEND\xaeB`\x82") {
                    return None;
                }
            }
            Self::Jpeg => complete_jpeg(&data)?,
            Self::Bmp => complete_rgb_bmp(&data)?,
            Self::Webp => webp::complete(&data)?,
            Self::Gif => complete_gif(&data)?,
            Self::Svg => return None,
        }
        self.reopen(data)
    }
    // Only for immutable bytes already checked by codec().
    pub(super) fn reopen(self, data: Data) -> Option<Codec<'static>> {
        let decoder = match self {
            Self::Png => codec::png_decoder::decoder(),
            Self::Jpeg => codec::jpeg_decoder::decoder(),
            Self::Bmp => codec::bmp_decoder::decoder(),
            Self::Webp => codec::webp_decoder::decoder(),
            Self::Gif => codec::gif_decoder::decoder(),
            Self::Svg => return None,
        };
        Codec::from_data_with_decoders(data, &[decoder])
    }
}

// Count no frames and allocate no frame table. Validate the bounded canvas,
// palettes and sub-block framing on the worker; Skia decodes frame zero.
fn complete_gif(data: &[u8]) -> Option<()> {
    let (screen, rest) = data.split_first_chunk::<13>()?;
    if !matches!(&screen[..6], b"GIF87a" | b"GIF89a") {
        return None;
    }
    let u16le = |pair: &[u8]| i32::from(u16::from_le_bytes([pair[0], pair[1]]));
    let (w, h) = (u16le(&screen[6..]), u16le(&screen[8..]));
    bitmap_bytes(w, h)?;
    let palette = |flags: u8| {
        if flags & 0x80 == 0 {
            0
        } else {
            6usize << (flags & 7)
        }
    };
    let mut rest = rest.get(palette(screen[10])..)?;
    let mut image = false;
    loop {
        let (&tag, tail) = rest.split_first()?;
        rest = match tag {
            0x3b => return image.then_some(()),
            0x21 => tail.get(1..)?, // extension label followed by sub-blocks
            0x2c => {
                let (desc, tail) = tail.split_first_chunk::<9>()?;
                let (fw, fh) = (u16le(&desc[4..]), u16le(&desc[6..]));
                if fw == 0 || fh == 0 || u16le(desc) + fw > w || u16le(&desc[2..]) + fh > h {
                    return None;
                }
                image = true;
                tail.get(1 + palette(desc[8])..)? // skip the local table and LZW code size
            }
            _ => return None,
        };
        loop {
            let (&n, tail) = rest.split_first()?;
            rest = tail.get(n as usize..)?;
            if n == 0 {
                break;
            }
        }
    }
}

// Windows BITMAPINFOHEADER, BI_RGB. Indexed pixels use an opaque BGR-reserved
// table, MSB first; unused bits, row padding and the reserved byte aren't alpha.
fn complete_rgb_bmp(bytes: &[u8]) -> Option<()> {
    let header = bytes.first_chunk::<54>()?;
    let word = |i| u32::from_le_bytes(header[i..i + 4].try_into().unwrap());
    let offset = word(10) as usize;
    let bpp = usize::from(u16::from_le_bytes([header[28], header[29]]));
    if &header[..2] != b"BM"
        || word(14) != 40
        || offset < 54
        || header[26..28] != [1, 0]
        || !matches!(bpp, 1 | 4 | 8 | 24)
        || word(30) != 0
    {
        return None;
    }
    let w = word(18) as i32;
    let h = (word(22) as i32).checked_abs()?;
    bitmap_bytes(w, h)?;
    let width = w as usize;
    let stride = width.checked_mul(bpp)?.div_ceil(32).checked_mul(4)?;
    let size = stride.checked_mul(h as usize)?;
    let end = offset.checked_add(size)?;
    let file = bytes.get(..word(2) as usize)?;
    let rows = file.get(offset..end)?;
    if word(34) != 0 && word(34) as usize != size {
        return None;
    }
    if bpp == 24 {
        return Some(());
    }

    let capacity = 1 << bpp;
    let colors = match word(46) as usize {
        0 => capacity,
        count => count,
    };
    if colors > capacity || offset < 54 + 4 * colors {
        return None;
    }
    // A full palette covers every index. For a short one, check only actual
    // pixels: Skia otherwise silently substitutes a color for missing entries.
    if colors < capacity
        && !rows.chunks_exact(stride).all(|row| {
            (0..width).all(|x| {
                let bit = x * bpp;
                let index = (usize::from(row[bit / 8]) >> (8 - bpp - bit % 8)) & (capacity - 1);
                index < colors
            })
        })
    {
        return None;
    }
    Some(())
}

/// Skia can produce pixels from an incomplete progressive scan. Require a real
/// end-of-image marker, not FF D9 inside a length-delimited EXIF thumbnail.
/// Trailing data after the primary image is fine. Skia validates the decode.
fn complete_jpeg(data: &[u8]) -> Option<()> {
    let mut data = data.strip_prefix(b"\xff\xd8")?;
    let mut in_scan = false;
    loop {
        if in_scan {
            data = &data[data.iter().position(|&b| b == 0xff)?..];
        }
        let marker = data.strip_prefix(&[0xff])?;
        let fill = marker.iter().position(|&b| b != 0xff)?;
        let (&code, rest) = marker[fill..].split_first()?;
        match code {
            0xd9 => return Some(()),
            0x00 | 0xd0..=0xd7 if in_scan => data = rest, // stuffed byte / restart
            0x00 | 0xd0..=0xd8 => return None,
            0x01 => data = rest, // stand-alone TEM
            _ => {
                let (length, payload) = rest.split_first_chunk::<2>()?;
                let size = u16::from_be_bytes(*length).checked_sub(2)?;
                data = payload.get(usize::from(size)..)?;
                in_scan = code == 0xda; // start of scan
            }
        }
    }
}
