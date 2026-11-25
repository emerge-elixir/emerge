use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Png,
    Jpeg,
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
        } else if ext.eq_ignore_ascii_case("svg") {
            Some(Self::Svg)
        } else {
            None
        }
    }

    pub(super) fn inline(bytes: &[u8]) -> Self {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Self::Png
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
            Self::Svg => return None,
        }
        self.reopen(data)
    }
    // Only for immutable bytes already checked by codec().
    pub(super) fn reopen(self, data: Data) -> Option<Codec<'static>> {
        let decoder = match self {
            Self::Png => codec::png_decoder::decoder(),
            Self::Jpeg => codec::jpeg_decoder::decoder(),
            Self::Svg => return None,
        };
        Codec::from_data_with_decoders(data, &[decoder])
    }
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
