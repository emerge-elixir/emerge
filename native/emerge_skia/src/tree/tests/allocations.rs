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
        let patch = set(2, 5, text(s));
        c.zero("fitting text patch", || {
            r.tree.patch(&patch).unwrap();
            r.tree.refresh(180., 170., 1.).unwrap()
        });
        draw(&mut r);
    }
    let full = at(full, 1);
    c.zero("fitting full replacement", || {
        r.tree.upload(&full).unwrap();
        r.tree.refresh(180., 170., 1.).unwrap()
    });
    draw(&mut r);
    let first_key = r.tree.root.unwrap();
    let before = r.tree.encode();
    let big = patch(1, 8, &[node(8, 5, text(&"Q".repeat(512)), &[])]);
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
        let p = set(1, 5, text(&s.repeat(n)));
        c.zero("spend headroom", || {
            r.tree.patch(&p).unwrap();
            r.tree.refresh(180., 170., 1.).unwrap()
        });
        draw(&mut r);
    }
    let p = set(1, 5, text(&"Z".repeat(181)));
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
    let p = set(1, 5, text(&"A".repeat(181)));
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
    let clear = patch(1, 0, &[]);
    c.zero(
        "full replacement with accepted pixels installs new paint and hit inputs",
        || {
            r.tree.patch(&clear).unwrap();
            r.tree.upload(&full).unwrap();
            draw(&mut r);
            assert_ne!(r.target_at(5., 45.).unwrap().0, previous)
        },
    );
    assert_eq!(r.draws, draws);
    for p in [
        set(1, 3, col(100.)),
        set(1, 3, col(170.)),
        order(2, 2, &[4, 3, 5]),
        set(6, 4, below([0, 255, 0], 1)),
    ] {
        c.zero("width/order/color edit", || {
            r.tree.patch(&p).unwrap();
            r.tree.refresh(180., 170., 1.).unwrap()
        });
        draw(&mut r);
    }
    let listener = set(6, 4, below([0, 255, 0], 0));
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
        set(2, 4, row::red(px(1, 60.))),
        set(2, 4, row::red(px(1, 20.))),
        order(1, 1, &[2, 4, 3]),
        set(2, 4, row::red(vec![1, 0])),
        set(1, 1, row::row_attrs(px(1, 120.), 8., 7.)),
        set(1, 1, row::row_attrs(vec![1, 1], 8., 7.)),
        set(1, 1, row::row_attrs(px(1, 19.5), 5., 5.)),
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
    let full = at(full, 1);
    c.zero("fitting row replacement", || {
        line.tree.upload(&full).unwrap();
        line.tree.refresh(180., 170., 1.).unwrap()
    });
    let mut stack = Raster::default();
    stack.tree.upload(&column::document()).unwrap();
    c.zero("first height split after reservation", || {
        assert!(stack.settle(180., 170., 1.).unwrap());
    });
    let visible = draw(&mut stack).to_vec();
    stack
        .tree
        .patch(&set(2, 4, column::header(px(2, 60.))))
        .unwrap();
    let pending = stack.tree.encode();
    let bigger = patch(
        3,
        3,
        &[
            node(3, 4, column::panel([0, 255, 0]), &[6]),
            node(6, 5, text(&"W".repeat(512)), &[]),
        ],
    );
    (c.fail_next_reserve)();
    assert_eq!(
        stack.tree.patch(&bigger).unwrap_err(),
        "tree workspace capacity exceeded"
    );
    assert_eq!(stack.tree.encode(), pending);
    assert_eq!(stack.pixels(), visible);
    assert_eq!(stack.hit_at(10., 80.), Some((4, 5., 5.)));
    c.zero(
        "rejected growth keeps the earlier height edit ready",
        || {
            assert!(stack.settle(180., 170., 1.).unwrap());
        },
    );
    draw(&mut stack);
    assert_eq!(stack.hit_at(10., 80.), Some((3, 5., 10.)));
    for edit in [
        set(2, 4, column::header(px(2, 20.))),
        set(2, 4, column::header(vec![2, 0])),
        order(1, 3, &[2, 4, 3]),
        set(1, 3, column::stack(vec![2, 1], 4.)),
        set(1, 3, column::stack(vec![2, 0], 4.)),
    ] {
        c.zero(
            "fitting height split: wire edit through paint/hit inputs",
            || {
                stack.tree.patch(&edit).unwrap();
                assert!(stack.settle(180., 170., 1.).unwrap());
            },
        );
        draw(&mut stack);
    }
    c.zero("height-only resize through paint/hit inputs", || {
        assert!(stack.settle(180., 53., 1.).unwrap());
    });
    // 100 slots + 80% = 180. The inserted subtree and edited parent's
    // first input coexist with the old glyph/command buffers and batch scratch.
    let ids: Vec<u64> = (2..=100).collect();
    let fill = || block(&[vec![2, 0]]);
    let mut nodes = vec![node(1, 3, fill(), &ids)];
    nodes.extend(ids.iter().map(|&id| node(id, 6, fill(), &[])));
    let mut topology = Tree::default();
    topology.upload(&upload(&nodes)).unwrap();
    topology.refresh(80., 20., 1.).unwrap();
    let ids: Vec<u64> = (102..=180).collect();
    let mut nodes = vec![node(101, 3, fill(), &ids)];
    nodes.extend(ids.iter().map(|&id| node(id, 6, fill(), &[])));
    let mut children: Vec<_> = (2..=100).collect();
    children.insert(50, 101);
    nodes.insert(0, edges(1, 3, &children));
    let add = patch(1, 1, &nodes);
    let remove = patch(101, 0, &[]);
    for _ in 0..2 {
        c.zero("insert at the last fitting topology slot", || {
            topology.patch(&add).unwrap();
            topology.refresh(80., 20., 1.).unwrap();
        });
        c.zero("detach subtree including retained declarations", || {
            topology.patch(&remove).unwrap();
            topology.refresh(80., 20., 1.).unwrap();
        });
    }
    c.zero("back-to-back topology edits plus pending reattach", || {
        for p in [&add, &remove, &add] {
            topology.patch(p).unwrap();
        }
        topology.refresh(80., 20., 1.).unwrap();
    });
    topology
        .patch(&set(1, 3, block(&[px(1, 73.), px(2, 100.)])))
        .unwrap();
    children.insert(0, 181);
    let over = patch(1, 1, &[edges(1, 3, &children), node(181, 6, fill(), &[])]);
    let pending = topology.encode();
    (c.fail_next_reserve)();
    assert_eq!(
        topology.patch(&over).unwrap_err(),
        "tree workspace capacity exceeded"
    );
    assert_eq!(topology.encode(), pending);
    c.zero(
        "earlier edit still computes after failed topology admission",
        || topology.refresh(80., 20., 1.).unwrap(),
    );
    assert_eq!(topology.layers.frames[topology.root.unwrap()].width, 73.);
    assert_eq!(
        topology.layers.frames[topology.elements.ids[&101]].height,
        1.
    );
    let topology_growth = c.count(|| topology.patch(&over).unwrap());
    assert!(topology_growth[0] + topology_growth[1] > 0);
    c.zero("first compute after topology growth", || {
        topology.refresh(80., 20., 1.).unwrap()
    });
    let mut moving = Tree::default();
    moving
        .upload(&upload(&[
            node(1, 1, block(&[]), &[2, 3]),
            node(2, 3, block(&[px(1, 60.)]), &[17]),
            node(3, 3, block(&[px(1, 60.)]), &[]),
            node(17, 4, block(&[vec![40, 1]]), &[18]),
            node(18, 5, text("Pay"), &[]),
        ]))
        .unwrap();
    moving.refresh(120., 20., 1.).unwrap();
    let transfer = patch(
        1,
        1,
        &[edges(1, 1, &[2, 3]), edges(2, 3, &[]), edges(3, 3, &[17])],
    );
    let promote = patch(1, 17, &[]);
    for p in [&transfer, &promote] {
        c.zero(
            "continuous sparse move or bare promotion through draw inputs",
            || {
                moving.patch(p).unwrap();
                moving.refresh(120., 20., 1.).unwrap();
            },
        );
    }
    assert_eq!(moving.layers.text.shapes, 1);
    for declared in [Some(12.), None] {
        let mut scaled = Raster::default();
        let at_scale = |s| match declared {
            Some(size) => block(&[number(78, s), number(16, size)]),
            None => block(&[number(78, s)]),
        };
        let edits = [set(1, 4, at_scale(2.)), set(1, 4, at_scale(1.))];
        scaled
            .tree
            .upload(&upload(&[
                node(1, 4, at_scale(1.), &[2]),
                node(2, 5, text("Pay"), &[]),
            ]))
            .unwrap();
        draw(&mut scaled);
        let cold = c.count(|| scaled.tree.patch(&edits[0]).unwrap());
        c.zero("first compute after admitting a cold scaled strike", || {
            assert!(scaled.settle(180., 170., 1.).unwrap());
        });
        draw(&mut scaled); // Rasterization is outside the measured interval.
        for p in [&edits[1], &edits[0], &edits[1]] {
            c.zero("warmed scale edit: wire through draw inputs", || {
                scaled.tree.patch(p).unwrap();
                assert!(scaled.settle(180., 170., 1.).unwrap());
            });
            draw(&mut scaled);
        }
        eprintln!(
            "font {declared:?}: cold scale admission {cold:?}; first settle and warmed edits [0, 0, 0]"
        );
        scaled.tree.patch(&edits[0]).unwrap();
        let rejected = set(1, 4, at_scale(1.25));
        let pending = scaled.tree.encode();
        (c.fail_next_reserve)();
        assert_eq!(
            scaled.tree.patch(&rejected).unwrap_err(),
            "tree workspace capacity exceeded"
        );
        assert_eq!(scaled.tree.encode(), pending);
        c.zero("rejected strike leaves previous scale ready", || {
            scaled.tree.refresh(180., 170., 1.).unwrap();
        });
        assert_eq!(
            scaled.tree.layers.runs[scaled.tree.elements.ids[&2]]
                .metrics
                .size,
            2. * declared.unwrap_or(16.) as f32
        );
    }
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
        let mut r = Raster::default();
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
                    r.tree.patch(&clear).unwrap();
                    r.tree.upload(next).unwrap();
                    r.tree.refresh(180., 170., 1.).unwrap();
                },
            );
            assert_eq!(r.tree.frames().count(), n);
            assert_eq!(r.tree.elements.slots[r.tree.root.unwrap()].stats.count, n);
        }
    }
    // Replacement must also discard an undrained workset without allocating.
    let ids: Vec<_> = (1..=(MAX_DEPTH + 1) as u64).collect();
    let deep = column_tree(&ids, true);
    let wide = column_tree(&ids, false);
    let mut r = Raster::default();
    r.tree.upload(&deep).unwrap();
    let ids: Vec<_> = (1..=(MAX_DEPTH + 2) as u64).collect();
    assert_eq!(
        r.tree.patch(&at(column_tree(&ids, true), 1)).unwrap_err(),
        "tree depth exceeds 512"
    );
    c.zero("drain work preserved by a rejected upload", || {
        r.tree.refresh(180., 170., 1.).unwrap()
    });
    assert_eq!(r.tree.frames().count(), MAX_DEPTH + 1);
    c.zero("replace pending work", || {
        r.tree.patch(&clear).unwrap();
        r.tree.upload(&deep).unwrap();
        r.tree.patch(&clear).unwrap();
        r.tree.upload(&wide).unwrap();
        r.tree.refresh(180., 170., 1.).unwrap();
    });
    assert_eq!(r.tree.frames().count(), MAX_DEPTH + 1);
    eprintln!(
        "alloc/realloc/free: Rust control {control:?}; Skia control {skia:?}; initial reservation {initial:?}; one-over text growth {growth:?}; one-over topology growth {topology_growth:?}; cold and fitting intervals (including topology) [0, 0, 0]"
    );
}
