use super::*;
use std::cell::Cell;

fn extent(raw: f32, bound: Option<Bounds<f32>>) -> f32 {
    bound.map_or(raw, |b| raw.clamp(b.floor, b.cap))
}

// Construct a host from an independently chosen feasible allocation. No
// second search or tolerance to mask a lost or wrongly frozen sibling.
fn check_unit(fills: &[(f64, Option<Bounds<f32>>)], unit: f32) {
    let room: f32 = fills.iter().map(|&(w, b)| extent(unit * w as f32, b)).sum();
    let weights: f32 = fills.iter().map(|&(w, _)| w as f32).sum();
    let scans = Cell::new(0);
    let split = FillShares::Device(room / weights).redistribute(f64::from(room), || {
        scans.set(scans.get() + 1);
        assert!(scans.get() <= 66);
        fills.iter().copied()
    });
    for &(w, b) in fills {
        assert_eq!(extent(split.share(w), b), extent(unit * w as f32, b));
    }
}

#[test]
fn mixed_free_and_limited_weights_recover_known_solutions() {
    let bounds = [
        None,
        Some(Bounds { floor: 0., cap: 0. }),
        Some(Bounds {
            floor: 0.,
            cap: 7.5,
        }),
        Some(Bounds {
            floor: 0.,
            cap: 29.,
        }),
        Some(Bounds {
            floor: 11.,
            cap: f32::INFINITY,
        }),
        Some(Bounds {
            floor: 39.,
            cap: f32::INFINITY,
        }),
        Some(Bounds {
            floor: 3.,
            cap: 25.,
        }),
        Some(Bounds {
            floor: 7.5,
            cap: 7.5,
        }),
    ];
    for a in bounds {
        for b in bounds {
            for c in bounds {
                for d in bounds {
                    for unit in [0., 0.5, 7., 12., 38.] {
                        check_unit(&[(0.5, a), (1., b), (3., c), (2., d)], unit);
                    }
                }
            }
        }
    }
    let wide: Vec<_> = (0..10000)
        .map(|i| (1. + (i % 3) as f64, bounds[i % bounds.len()]))
        .collect();
    check_unit(&wide, 8.);
}

#[test]
fn a_floor_that_takes_the_whole_host_leaves_zero_even_for_a_much_smaller_peer() {
    let size = f32::MAX;
    let split = FillShares::Device(1.).redistribute(f64::from(size), || {
        [
            (
                f64::from(size),
                Some(Bounds {
                    floor: size,
                    cap: f32::INFINITY,
                }),
            ),
            (1., None),
        ]
        .into_iter()
    });
    assert_eq!(split.share(1.), 0.);
}
