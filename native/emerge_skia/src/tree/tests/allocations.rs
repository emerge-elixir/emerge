//! Process isolation matters: Skia calls malloc outside Rust's GlobalAlloc.
use super::*;
use std::{ffi::CStr, hint::black_box, process::Command};
struct Counter {
    start: extern "C" fn(),
    fail_next_reserve: extern "C" fn(),
    stop: extern "C" fn(*mut u64),
}
impl Counter {
    fn load() -> Self {
        fn symbol(name: &CStr) -> *mut libc::c_void {
            let p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
            assert!(!p.is_null(), "interposer not loaded");
            p
        }
        // Both symbols are defined with these C signatures in heap_calls.c.
        unsafe {
            Self {
                start: std::mem::transmute::<*mut libc::c_void, extern "C" fn()>(symbol(
                    c"emerge_heap_start",
                )),
                fail_next_reserve: std::mem::transmute::<*mut libc::c_void, extern "C" fn()>(
                    symbol(c"emerge_fail_next_reserve"),
                ),
                stop: std::mem::transmute::<*mut libc::c_void, extern "C" fn(*mut u64)>(symbol(
                    c"emerge_heap_stop",
                )),
            }
        }
    }
    fn count(&self, f: impl FnOnce()) -> [u64; 3] {
        let mut c = [0; 3];
        (self.start)();
        f();
        (self.stop)(c.as_mut_ptr());
        c
    }
    fn zero(&self, operation: &str, f: impl FnOnce()) {
        assert_eq!(
            self.count(f),
            [0; 3],
            "{operation}: allocations/reallocations/frees"
        )
    }
}
#[test]
fn fitting_wire_through_draw_inputs_uses_no_heap() {
    if std::env::var_os("EMERGE_TREE_HEAP_CHILD").is_none() {
        let file = std::env::temp_dir().join(format!("emerge-heap-{}.so", std::process::id()));
        assert!(
            Command::new("cc")
                .args(["-O2", "-shared", "-fPIC", "-Wall", "-Werror"])
                .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/heap_calls.c"))
                .arg("-o")
                .arg(&file)
                .status()
                .unwrap()
                .success()
        );
        let result = Command::new(std::env::current_exe().unwrap())
            .env("LD_PRELOAD", &file)
            .env("EMERGE_TREE_HEAP_CHILD", "1")
            .args([
                "--exact",
                "tree::tests::allocations::fitting_wire_through_draw_inputs_uses_no_heap",
                "--nocapture",
            ])
            .output()
            .unwrap();
        std::fs::remove_file(file).unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        eprint!("{}", String::from_utf8_lossy(&result.stderr));
        return;
    }
    let c = Counter::load();
    let control = c.count(|| {
        let mut v = Vec::with_capacity(4);
        v.extend_from_slice(black_box(b"abc"));
        v.reserve(black_box(100));
        drop(black_box(v))
    });
    assert!(
        control.iter().all(|&n| n > 0),
        "Rust allocator sentinel {control:?}"
    );
    let skia = c.count(|| drop(black_box(skia_safe::Data::new_copy(black_box(&[42; 256])))));
    assert!(
        skia[0] > 0 && skia[2] > 0,
        "Skia allocator sentinel {skia:?}"
    );
    let full = upload(&[
        node(
            1,
            4,
            block(&[
                [3, 0].into_iter().chain(4f64.to_be_bytes()).collect(),
                [18, 0, 0, 9].into_iter().chain(*b"monospace").collect(),
            ]),
            &[2],
        ),
        node(2, 5, text("AA"), &[]),
    ]);
    let mut r = Raster::default();
    let initial = c.count(|| r.tree.upload(&full).unwrap());
    assert!(initial[0] > 0);
    c.zero("cold padded text", || {
        r.tree.refresh(180., 170., 1.).unwrap()
    });
    draw(&mut r);
    for s in ["BB", "BBBB", "BBBB"] {
        let patch = set(2, text(s));
        c.zero("fitting text patch", || {
            r.tree.patch(&patch).unwrap();
            r.tree.refresh(180., 170., 1.).unwrap()
        });
        draw(&mut r);
    }
    c.zero("fitting full replacement", || {
        r.tree.upload(&full).unwrap();
        r.tree.refresh(180., 170., 1.).unwrap()
    });
    draw(&mut r);
    let first_key = r.tree.root.unwrap();
    let before = r.tree.encode();
    let big = upload(&[node(8, 5, text(&"Q".repeat(512)), &[])]);
    (c.fail_next_reserve)();
    assert_eq!(
        r.tree.upload(&big).unwrap_err(),
        "tree workspace capacity exceeded"
    );
    assert_eq!(r.tree.root, Some(first_key));
    assert_eq!(r.tree.encode(), before);
    c.zero("failed full upload preserves reused workspace", || {
        r.tree.upload(&full).unwrap();
        r.tree.refresh(180., 170., 1.).unwrap()
    });
    let mut r = Raster::default();
    let base = "X".repeat(100);
    r.tree
        .upload(&upload(&[node(1, 5, text(&base), &[])]))
        .unwrap();
    c.zero("cold 100 glyphs", || {
        r.tree.refresh(180., 170., 1.).unwrap()
    });
    draw(&mut r);
    // At the last fit the 180 installed glyphs and 180 different candidate
    // glyphs coexist in the 360-slot run pool; the draw buffer also uses 180/180.
    for (s, n) in [("X", 140), ("Y", 180), ("Z", 180)] {
        let p = set(1, text(&s.repeat(n)));
        c.zero("spend headroom", || {
            r.tree.patch(&p).unwrap();
            r.tree.refresh(180., 170., 1.).unwrap()
        });
        draw(&mut r);
    }
    let p = set(1, text(&"Z".repeat(181)));
    let before = r.tree.encode();
    let pixels = r.pixels().to_vec();
    let draws = r.draws;
    (c.fail_next_reserve)();
    assert_eq!(
        r.tree.patch(&p).unwrap_err(),
        "tree workspace capacity exceeded"
    );
    assert_eq!(r.tree.encode(), before);
    c.zero("rejected reserve leaves previous tree drawable", || {
        assert_eq!(draw(&mut r), pixels);
    });
    assert_eq!(r.draws, draws);
    assert_eq!(r.tree.layers.bounds.desired(), 180);
    let growth = c.count(|| r.tree.patch(&p).unwrap());
    assert!(growth[0] + growth[1] > 0, "one over last slot did not grow");
    c.zero("first compute after genuine growth", || {
        r.tree.refresh(180., 170., 1.).unwrap()
    });
    draw(&mut r);
    let p = set(1, text(&"A".repeat(181)));
    c.zero("subsequent fit", || {
        r.tree.patch(&p).unwrap();
        r.tree.refresh(180., 170., 1.).unwrap()
    });
    let col = |w| block(&[px(1, w), number(4, 10.)]);
    let chip = block(&[px(1, 50.), px(2, 30.), color([0, 0, 255])]);
    let below = |rgb, click| block(&[vec![1, 0], px(2, 40.), vec![40, click], color(rgb)]);
    let full = upload(&[
        node(1, 3, col(170.), &[2, 6]),
        node(2, 2, block(&[vec![1, 0], number(4, 10.)]), &[3, 4, 5]),
        node(3, 4, chip.clone(), &[]),
        node(4, 4, chip.clone(), &[]),
        node(5, 4, chip, &[]),
        node(6, 4, below([255, 0, 0], 1), &[]),
    ]);
    let mut r = Raster::default();
    r.tree.upload(&full).unwrap();
    c.zero("cold wrap", || r.tree.refresh(180., 170., 1.).unwrap());
    draw(&mut r);
    let previous = r.target_at(5., 45.).unwrap().0;
    let draws = r.draws;
    c.zero(
        "full replacement with accepted pixels installs new paint and hit inputs",
        || {
            r.tree.upload(&full).unwrap();
            draw(&mut r);
            assert_ne!(r.target_at(5., 45.).unwrap().0, previous)
        },
    );
    assert_eq!(r.draws, draws);
    for p in [
        set(1, col(100.)),
        set(1, col(170.)),
        order(2, &[4, 3, 5]),
        set(6, below([0, 255, 0], 1)),
    ] {
        c.zero("width/order/color edit", || {
            r.tree.patch(&p).unwrap();
            r.tree.refresh(180., 170., 1.).unwrap()
        });
        draw(&mut r);
    }
    let listener = set(6, below([0, 255, 0], 0));
    c.zero("listener edit + reuse/install + hit lookup", || {
        r.tree.patch(&listener).unwrap();
        draw(&mut r);
        assert!(r.target_at(5., 45.).is_none())
    });
    // Row offers and paint clips are keyed results reserved at admission too.
    let mut line = Raster::default();
    let attrs = row::row_attrs(px(1, 120.), 5., 5.);
    let full = row::band(attrs.clone(), row::red(px(1, 20.)), &[2, 3, 4]);
    line.tree.upload(&full).unwrap();
    c.zero("cold single-line row", || {
        line.tree.refresh(180., 170., 1.).unwrap()
    });
    draw(&mut line);
    for p in [
        set(2, row::red(px(1, 60.))),
        set(2, row::red(px(1, 20.))),
        order(1, &[2, 4, 3]),
        set(2, row::red(vec![1, 0])),
        set(1, row::row_attrs(px(1, 120.), 8., 7.)),
        set(1, row::row_attrs(vec![1, 1], 8., 7.)),
        set(1, row::row_attrs(px(1, 19.5), 5., 5.)),
    ] {
        c.zero(
            "single-line row: wire edit through paint/hit inputs",
            || {
                line.tree.patch(&p).unwrap();
                line.tree.refresh(180., 170., 1.).unwrap()
            },
        );
        draw(&mut line);
    }
    c.zero("fitting row replacement", || {
        line.tree.upload(&full).unwrap();
        line.tree.refresh(180., 170., 1.).unwrap()
    });
    let column_tree = |ids: &[u64], deep| {
        let nodes: Vec<_> = ids
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                let kids = if deep {
                    ids.get(i + 1..(i + 2).min(ids.len())).unwrap_or_default()
                } else if i == 0 {
                    &ids[1..]
                } else {
                    &[]
                };
                node(id, 3, block(&[]), kids)
            })
            .collect();
        upload(&nodes)
    };
    for n in [400, MAX_DEPTH + 1] {
        let ids: Vec<_> = (1..=n as u64).collect();
        let wide = column_tree(&ids, false);
        let deep = column_tree(&ids, true);
        r.tree.upload(&wide).unwrap();
        c.zero("wide cold", || r.tree.refresh(180., 170., 1.).unwrap());
        assert_eq!(r.tree.layers.trace.len(), 7 * n);
        for next in [&deep, &wide] {
            c.zero(
                "redistribute fitting work across depths, from wire through compute",
                || {
                    r.tree.upload(next).unwrap();
                    r.tree.refresh(180., 170., 1.).unwrap();
                },
            );
            assert_eq!(r.tree.layers.trace.len(), 7 * n);
        }
    }
    // Replacement must also discard an undrained workset without allocating.
    let ids: Vec<_> = (1..=(MAX_DEPTH + 1) as u64).collect();
    let deep = column_tree(&ids, true);
    let wide = column_tree(&ids, false);
    r.tree.upload(&deep).unwrap();
    let ids: Vec<_> = (1..=(MAX_DEPTH + 2) as u64).collect();
    assert_eq!(
        r.tree.upload(&column_tree(&ids, true)).unwrap_err(),
        "tree depth exceeds 512"
    );
    c.zero("drain work preserved by a rejected upload", || {
        r.tree.refresh(180., 170., 1.).unwrap()
    });
    assert_eq!(r.tree.layers.trace.len(), 7 * (MAX_DEPTH + 1));
    c.zero("replace pending work", || {
        r.tree.upload(&deep).unwrap();
        r.tree.upload(&wide).unwrap();
        r.tree.refresh(180., 170., 1.).unwrap();
    });
    assert_eq!(r.tree.layers.trace.len(), 7 * (MAX_DEPTH + 1));
    eprintln!(
        "alloc/realloc/free: Rust control {control:?}; Skia control {skia:?}; initial reservation {initial:?}; one-over growth {growth:?}; cold and fitting intervals (including Row) [0, 0, 0]"
    );
}
