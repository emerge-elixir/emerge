//! Byte credits for the mutable compiler, retained records, and all frame
//! generations. Use wide arithmetic *before* provisioning either path. These
//! are capacities, not bytes touched during a layout or allocator statistics.
use super::*;
#[derive(Clone, Copy)]
pub(in crate::tree) struct Sites {
    pub owners: usize,
    pub ports: usize,
    pub images: usize,
    pub escapes: usize,
}
impl Sites {
    // One ordinary-content call per unbanded port. Mixed orders can submit
    // two spans per gap (float and inline). Each break contains a nested
    // port or an escape before the next port, and each gap needs an ordinal.
    pub(in crate::tree) fn markers(self) -> usize {
        if self.ports == 0 {
            return 0;
        }
        let gaps = if self.instances() == 0 {
            0
        } else {
            self.owners.min(2 * self.ports + self.escapes)
        };
        self.ports + 2 * gaps
    }
    pub(super) fn instances(self) -> usize {
        if self.escapes != 0 || self.ports > 1 {
            self.owners
        } else {
            0
        }
    }
}
pub(in crate::tree) fn charge(
    sites: Sites,
    d: [usize; 8],
    h: [usize; 4],
    readers: usize,
    b: Budget,
) -> Result<usize, String> {
    let Sites {
        owners: n,
        ports,
        images,
        ..
    } = sites;
    if ports == 0 {
        return Ok(0);
    }
    let (n, ports, r) = (n as u128, ports as u128, readers as u128);
    let v = r + 2;
    let [ops, ids, points, clips, styles, bytes, planes, guards] = d.map(|x| x as u128);
    let [hits, masks, inverses, views] = h.map(|x| x as u128);
    let typed = Pages::<(Scope, Command), 4>::charge(n, ops + n, v, b)
        + Pages::<u16, 32>::charge(n, ids, v, b)
        + Pages::<Point, 32>::charge(n, points, v, b)
        + Pages::<Hit, 4>::charge(n, hits + n, v, b)
        + Pages::<(Rounded, Mask), 4>::charge(2 * n, clips + masks, v, b)
        + Pages::<Plane, 2>::charge(2 * n, planes + inverses, v, b);
    let versions = Stock::<Drawing>::charge(n * v, 0, b)
        + Stock::<Hits>::charge(n * v, 0, b)
        + Stock::<Node<Local>>::charge(2 * (r + 3) * (n + 1), 0, b)
        + Bindings::charge(sites, r, b)
        + Pages::<Range<usize>, 8>::charge(2 * ports, 2 * n, r + 2, b)
        + ports * size_of::<Window>() as u128;
    let swatches = if styles == 0 {
        0
    } else {
        let arrays = if bytes == 0 {
            0
        } else {
            (0..=bytes.next_power_of_two().ilog2())
                .map(|i| {
                    let cap = 1u128 << i;
                    Stock::<Vec<u8>>::charge(v * styles.min(bytes / (cap / 2 + 1)), cap, b)
                })
                .sum()
        };
        arrays
            + Stock::<Swatch>::charge(v * styles, 0, b)
            + Pages::<Ref<Swatch>, 4>::charge(n, n + styles, v, b)
            + styles * (size_of::<Key>() + size_of::<Ref<Swatch>>()) as u128
    };
    let scratch = (n + 1) * (size_of::<(Key, [bool; 2], bool)>() as u128)
        + ops * size_of::<(Scope, Command)>() as u128
        + ids * 2
        + points * size_of::<Point>() as u128
        + (clips + masks) * size_of::<(Rounded, Mask)>() as u128
        + hits * size_of::<Hit>() as u128
        + (planes + inverses) * size_of::<Plane>() as u128
        + guards * size_of::<pose::ShadowGuard>() as u128
        + views * 2 * size_of::<(Attachment, ScrollHit)>() as u128
        + bytes
        + styles * size_of::<styled::Material>() as u128;
    let markers = sites.markers() as u128
        * ((r + 1) * (size_of::<(Scope, Command)>() + size_of::<Hit>()) as u128
            + size_of::<(Key, Visit, Scope)>() as u128);
    let intervals =
        3 * (n.div_ceil(2) + 1) * size_of::<Range<usize>>() as u128 * (100 + b.0 as u128) / 100;
    // An impossible detached extent is an admission error, not a partially
    // published tree or an OOM while first exposing a long row.
    usize::try_from(
        65536
            + fits::Fits::recording_charge(images as u128, v, b)
            + typed
            + versions
            + swatches
            + scratch
            + intervals
            + markers,
    )
    .ok()
    .filter(|n| *n <= 1024 * 1024 * 1024)
    .ok_or_else(|| storage::LIMIT.into())
}
