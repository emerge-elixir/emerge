use super::err;
use smithay_client_toolkit::shm::{
    Shm,
    slot::{Buffer, SlotPool},
};
use wayland_client::protocol::wl_shm::Format;
struct Slot {
    buffer: Buffer,
    pool: SlotPool,
    size: (u32, u32),
}
#[derive(Default)]
pub(super) struct Slots([Option<Slot>; 2]);
impl Slots {
    /// Prefer a released slot of the right size. Resize at most one free slot;
    /// a busy old-size buffer and its mapping remain intact until release.
    pub fn available(&self, size: (u32, u32)) -> Option<usize> {
        choose(
            self.0.iter().map(|s| {
                s.as_ref()
                    .map(|s| (s.size, s.buffer.slot().has_active_buffers()))
            }),
            size,
        )
    }
    pub fn write(
        &mut self,
        i: usize,
        shm: &Shm,
        size: (u32, u32),
        rgba: &[u8],
    ) -> Result<&Buffer, String> {
        let stride = (size.0 * 4) as i32;
        if self.0[i].as_ref().is_none_or(|s| s.size != size) {
            let mut pool = SlotPool::new(rgba.len(), shm).map_err(err)?;
            let buffer = pool
                .create_buffer(size.0 as i32, size.1 as i32, stride, Format::Argb8888)
                .map_err(err)?
                .0;
            self.0[i] = Some(Slot { buffer, pool, size });
        }
        let slot = self.0[i].as_mut().expect("initialized shm slot");
        let dst = slot
            .buffer
            .canvas(&mut slot.pool)
            .ok_or("Wayland buffer not released")?;
        to_argb(rgba, dst)?;
        Ok(&slot.buffer)
    }
}

type Size = (u32, u32);
fn choose(slots: impl Iterator<Item = Option<(Size, bool)>>, size: Size) -> Option<usize> {
    slots
        .enumerate()
        .filter(|(_, s)| !matches!(s, Some((_, true))))
        .min_by_key(|(_, s)| s.is_none_or(|(dims, _)| dims != size))
        .map(|(i, _)| i)
}
fn to_argb(rgba: &[u8], dst: &mut [u8]) -> Result<(), String> {
    if rgba.len() != dst.len() || !rgba.len().is_multiple_of(4) {
        return Err("inconsistent Wayland buffer stride".into());
    }
    for ([r, g, b, a], out) in rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(dst.as_chunks_mut::<4>().0)
    {
        *out = u32::from_be_bytes([*a, *r, *g, *b]).to_ne_bytes();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::geometry::{Geometry, Scale};
    use super::*;
    #[test]
    fn an_in_flight_slot_cannot_be_used_even_across_a_resize() {
        let old = (480, 240);
        let new = (240, 240);
        let free = |slots: [Option<(Size, bool)>; 2], size| choose(slots.into_iter(), size);
        assert_eq!(free([Some((old, true)), Some((new, true))], new), None);
        assert_eq!(free([Some((old, true)), None], new), Some(1));
        assert_eq!(free([Some((old, false)), Some((new, false))], new), Some(1));
        assert_eq!(free([Some((old, false)), Some((new, true))], new), Some(0));
    }
    #[test]
    fn geometry_and_shm_respect_device_units_checked_sizes_and_native_argb() {
        let g = Geometry {
            logical: (480, 240),
            scale: Scale::Integer(2),
        };
        assert_eq!(g.buffer().unwrap(), (960, 480));
        assert_eq!(g.mapping().unwrap().point((5.5, 3.)), (11., 6.));
        for logical in [(0, 1), (u32::MAX, 2), (8192, 8193), (16_777_217, 1)] {
            assert!(
                Geometry {
                    logical,
                    scale: Scale::Integer(1)
                }
                .buffer()
                .is_err()
            );
        }
        assert!(
            Geometry {
                logical: (u32::MAX, 1),
                ..g
            }
            .buffer()
            .is_err()
        );
        let mut dst = [0; 8];
        to_argb(&[30, 20, 10, 128, 1, 2, 3, 255], &mut dst).unwrap();
        assert_eq!(&dst[..4], &0x801e140au32.to_ne_bytes());
        assert_eq!(&dst[4..], &0xff010203u32.to_ne_bytes());
        assert!(to_argb(&[0; 8], &mut dst[..4]).is_err());
    }
}
