//! Preflight the still subset before libwebp opens a demuxer. No chunk table,
//! metadata copies or probing decoders. The pixel worker still has to decode
//! the entire requested frame successfully.
use super::*;

const ICC: u8 = 0x20;
const ALPHA: u8 = 0x10;
const EXIF: u8 = 0x08;
const XMP: u8 = 0x04;
const MAX_METADATA: usize = MIB;

fn u24(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], 0])
}
fn chunk<'a>(rest: &mut &'a [u8]) -> Option<(&'a [u8; 4], &'a [u8])> {
    let (tag, tail) = rest.split_first_chunk::<4>()?;
    let (len, tail) = tail.split_first_chunk::<4>()?;
    let n = u32::from_le_bytes(*len) as usize;
    let (payload, tail) = tail.split_at_checked(n)?;
    *rest = if n.is_multiple_of(2) {
        tail
    } else {
        tail.strip_prefix(&[0])?
    };
    Some((tag, payload))
}
fn bitstream(tag: &[u8; 4], data: &[u8]) -> Option<((i32, i32), bool)> {
    match tag {
        b"VP8 " => {
            let (header, partitions) = data.split_first_chunk::<10>()?;
            let frame = u24(header);
            // Key frame, displayed, valid version and a complete first partition.
            let n = (frame >> 5) as usize;
            if frame & 1 != 0
                || (frame >> 1) & 7 > 3
                || frame & 16 == 0
                || header[3..6] != [0x9d, 1, 0x2a]
                || n == 0
                || n > partitions.len()
            {
                return None;
            }
            let axis = |i| i32::from(u16::from_le_bytes([header[i], header[i + 1]]) & 0x3fff);
            Some(((axis(6), axis(8)), false))
        }
        b"VP8L" => {
            let (dims, pixels) = data.strip_prefix(&[0x2f])?.split_first_chunk::<4>()?;
            let bits = u32::from_le_bytes(*dims);
            if bits >> 29 != 0 || pixels.is_empty() {
                return None;
            }
            let axis = |shift: u32| 1 + ((bits >> shift) & 0x3fff) as i32;
            Some(((axis(0), axis(14)), bits & (1 << 28) != 0))
        }
        _ => None,
    }
}

pub(super) fn complete(bytes: &[u8]) -> Option<()> {
    let (len, body) = bytes.strip_prefix(b"RIFF")?.split_first_chunk::<4>()?;
    if u32::from_le_bytes(*len) as usize != body.len() {
        return None;
    }
    let mut rest = body.strip_prefix(b"WEBP")?;
    let (tag, data) = chunk(&mut rest)?;
    if tag != b"VP8X" {
        let ((w, h), _) = bitstream(tag, data)?;
        bitmap_bytes(w, h)?;
        return rest.is_empty().then_some(());
    }
    let header: &[u8; 10] = data.try_into().ok()?;
    let flags = header[0];
    if flags & !(ICC | ALPHA | EXIF | XMP) != 0 || header[1..4] != [0; 3] {
        return None;
    }
    let canvas = (1 + u24(&header[4..7]) as i32, 1 + u24(&header[7..]) as i32);
    let pixels = bitmap_bytes(canvas.0, canvas.1)? / 4;
    let mut seen = 0;
    let mut image = false;
    let mut metadata = 0usize;
    while !rest.is_empty() {
        let (tag, payload) = chunk(&mut rest)?;
        let flag = match tag {
            b"ICCP" if !image && seen == 0 => ICC,
            b"EXIF" if image => EXIF,
            b"XMP " if image => XMP,
            b"ALPH" if !image && seen & ALPHA == 0 => {
                let (&mode, data) = payload.split_first()?;
                if mode & 0xc0 != 0
                    || mode & 3 > 1
                    || (mode >> 4) & 3 > 1
                    || data.is_empty()
                    || (mode & 3 == 0 && data.len() != pixels)
                {
                    return None;
                }
                ALPHA
            }
            b"VP8 " | b"VP8L" if !image => {
                let (size, alpha) = bitstream(tag, payload)?;
                if size != canvas || (tag == b"VP8L" && seen & ALPHA != 0) {
                    return None;
                }
                if alpha {
                    seen |= ALPHA;
                }
                image = true;
                continue;
            }
            _ => return None, // including ANIM, ANMF and duplicate images
        };
        if flag & flags == 0 || flag & seen != 0 {
            return None;
        }
        if flag != ALPHA {
            metadata = metadata.checked_add(payload.len())?;
            if metadata > MAX_METADATA || payload.is_empty() {
                return None;
            }
        }
        seen |= flag;
    }
    (image && seen == flags).then_some(())
}
