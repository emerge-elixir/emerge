//! Reproducible native publication/RSS probe; one scenario per process.
//! cargo bench --bench shared_animation --features bench-diagnostics -- 5000 32 moving
use emerge_skia::tree::{
    animation::{
        AnimationCurve, AnimationRepeat, AnimationRuntime, AnimationSpec,
        lengths::diagnostics_for_benchmark,
    },
    attrs::{Attrs, Background, Color, Length},
    element::{Element, ElementKind, ElementTree, NodeId},
    invalidation::TreeInvalidation,
    layout::{
        Constraint, layout_and_refresh_default_with_animation,
        layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark as publish,
    },
};
use std::time::{Duration, Instant};

fn rss() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .map(|status| {
            status
                .lines()
                .filter(|line| line.starts_with("VmRSS:") || line.starts_with("VmHWM:"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|| "RSS unavailable on this platform".into())
}
fn motion(from: Attrs, to: Attrs, repeat: AnimationRepeat, duration_ms: f64) -> AnimationSpec {
    AnimationSpec {
        keyframes: vec![from, to],
        duration_ms,
        curve: AnimationCurve::Linear,
        repeat,
    }
}
fn fixture(nodes: usize, owners: usize, kind: &str) -> ElementTree {
    assert!(nodes > owners + 4 && owners > 0);
    let mut tree = (1..=nodes)
        .map(|n| {
            let animate = (4..4 + owners).contains(&n).then(|| {
                let (from, to) = if kind == "paint" {
                    (
                        Attrs {
                            alpha: Some(1.0),
                            ..Default::default()
                        },
                        Attrs {
                            alpha: Some(0.4),
                            ..Default::default()
                        },
                    )
                } else {
                    (
                        Attrs {
                            width: Some(Length::Px(40.0)),
                            ..Default::default()
                        },
                        Attrs {
                            width: Some(if kind == "pixel" {
                                Length::Px(80.0)
                            } else {
                                Length::Fill
                            }),
                            ..Default::default()
                        },
                    )
                };
                motion(from, to, AnimationRepeat::Once, 1000.0)
            });
            Element::with_attrs(
                NodeId(n as u64),
                if n == 1 || n == 3 {
                    ElementKind::Column
                } else if n == 2 {
                    ElementKind::Row
                } else {
                    ElementKind::El
                },
                vec![],
                Attrs {
                    width: Some(if n <= 3 {
                        Length::Fill
                    } else {
                        Length::Px(4.0)
                    }),
                    height: Some(if n == 1 {
                        Length::Fill
                    } else if n == 3 {
                        Length::Content
                    } else {
                        Length::Px(4.0)
                    }),
                    background: (n >= 4).then_some(Background::Color(Color::Rgba {
                        r: 30,
                        g: 100,
                        b: 200,
                        a: 255,
                    })),
                    animate,
                    ..Default::default()
                },
            )
        })
        .fold(ElementTree::new(), |mut tree, node| {
            tree.insert(node);
            tree
        });
    tree.set_root_id(NodeId(1));
    tree.set_children(&NodeId(1), vec![NodeId(2), NodeId(3)])
        .unwrap();
    tree.set_children(
        &NodeId(2),
        (4..4 + owners).map(|n| NodeId(n as u64)).collect(),
    )
    .unwrap();
    tree.set_children(
        &NodeId(3),
        (4 + owners..=nodes).map(|n| NodeId(n as u64)).collect(),
    )
    .unwrap();
    if ["moving", "mixed", "independent"].contains(&kind) {
        tree.get_mut(&NodeId(2)).unwrap().spec.declared.animate = Some(motion(
            Attrs {
                width: Some(Length::Px(600.0)),
                ..Default::default()
            },
            Attrs {
                width: Some(if kind == "moving" {
                    Length::Px(800.0)
                } else {
                    Length::Fill
                }),
                ..Default::default()
            },
            AnimationRepeat::Loop,
            if kind == "mixed" { 1000.0 } else { 2000.0 },
        ));
    }
    if kind == "independent" {
        tree.set_children(
            &NodeId(3),
            (4 + owners..nodes).map(|n| NodeId(n as u64)).collect(),
        )
        .unwrap();
        tree.set_children(&NodeId(1), vec![NodeId(2), NodeId(3), NodeId(nodes as u64)])
            .unwrap();
        tree.get_mut(&NodeId(nodes as u64))
            .unwrap()
            .spec
            .declared
            .animate = Some(motion(
            Attrs {
                width: Some(Length::Px(300.0)),
                ..Default::default()
            },
            Attrs {
                width: Some(Length::Fill),
                ..Default::default()
            },
            AnimationRepeat::Loop,
            3000.0,
        ));
    }
    if kind == "coupled" {
        tree.set_children(
            &NodeId(3),
            (4 + owners..nodes).map(|n| NodeId(n as u64)).collect(),
        )
        .unwrap();
        tree.set_children(
            &NodeId(2),
            (4..4 + owners)
                .chain(std::iter::once(nodes))
                .map(|n| NodeId(n as u64))
                .collect(),
        )
        .unwrap();
        tree.get_mut(&NodeId(nodes as u64))
            .unwrap()
            .spec
            .declared
            .animate = Some(motion(
            Attrs {
                width: Some(Length::Px(200.0)),
                ..Default::default()
            },
            Attrs {
                width: Some(Length::Fill),
                ..Default::default()
            },
            AnimationRepeat::Loop,
            2000.0,
        ));
    }
    if kind == "upward" {
        for node in 4..4 + owners {
            let spec = tree
                .get_mut(&NodeId(node as u64))
                .unwrap()
                .spec
                .declared
                .animate
                .as_mut()
                .unwrap();
            spec.repeat = AnimationRepeat::Loop;
            spec.duration_ms = 2000.0;
        }
        tree.get_mut(&NodeId(2)).unwrap().spec.declared.animate = Some(motion(
            Attrs {
                width: Some(Length::Px(600.0)),
                ..Default::default()
            },
            Attrs {
                width: Some(Length::Content),
                ..Default::default()
            },
            AnimationRepeat::Once,
            1000.0,
        ));
    }
    tree
}
fn main() {
    let assets = emerge_skia::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let smoke = args.iter().any(|arg| arg == "--test");
    let mut args = args
        .into_iter()
        .filter(|arg| arg != "--bench" && arg != "--test");
    let nodes = args
        .next()
        .map(|n| n.parse().unwrap())
        .unwrap_or(if smoke { 32 } else { 5000 });
    let owners = args.next().map(|n| n.parse().unwrap()).unwrap_or(1);
    let kind = args.next().unwrap_or_else(|| "length".into());
    assert!(
        [
            "paint",
            "pixel",
            "length",
            "moving",
            "mixed",
            "independent",
            "coupled",
            "upward"
        ]
        .contains(&kind.as_str())
    );
    println!(
        "nodes={nodes} owners={owners} case={kind} baseline {}",
        rss()
    );
    let mut tree = fixture(nodes, owners, &kind);
    println!("model {}", rss());
    let mut runtime = AnimationRuntime::default();
    let start = Instant::now();
    let constraint = Constraint::new(1024.0, 1024.0);
    runtime.sync_with_tree(&tree, start);
    let begin = Instant::now();
    let initial =
        layout_and_refresh_default_with_animation(&mut tree, constraint, 1.0, &mut runtime, start);
    println!(
        "cold_us={} {} {}",
        begin.elapsed().as_micros(),
        rss(),
        diagnostics_for_benchmark(&tree)
    );
    let mut rebuild = initial.unwrap().event_rebuild;
    let mut times = (1..120)
        .map(|frame| {
            let begin = Instant::now();
            let output = publish(
                &mut tree,
                constraint,
                1.0,
                &mut runtime,
                start + Duration::from_micros(frame * 1_000_000 / 120),
                TreeInvalidation::None,
                Some(&rebuild),
            )
            .unwrap();
            let elapsed = begin.elapsed().as_nanos();
            if output.output.event_rebuild_changed {
                rebuild = output.output.event_rebuild;
            }
            elapsed
        })
        .collect::<Vec<_>>();
    println!("warm_ns={times:?}");
    times.sort_unstable();
    println!(
        "warm_p50_us={} warm_p95_us={} warm_max_us={} {} {}",
        times[times.len() / 2] / 1000,
        times[times.len() * 95 / 100] / 1000,
        times[times.len() - 1] / 1000,
        rss(),
        diagnostics_for_benchmark(&tree)
    );
    let begin = Instant::now();
    let output = publish(
        &mut tree,
        constraint,
        1.0,
        &mut runtime,
        start + Duration::from_secs(1),
        TreeInvalidation::None,
        Some(&rebuild),
    )
    .unwrap();
    println!(
        "release_us={} {} {}",
        begin.elapsed().as_micros(),
        rss(),
        diagnostics_for_benchmark(&tree)
    );
    let mixed_loops = ["mixed", "independent", "coupled", "upward"].contains(&kind.as_str());
    assert_eq!(
        diagnostics_for_benchmark(&tree).starts_with("workspace=true"),
        mixed_loops
    );
    println!("retained_after_finite_release_due_to_mixed_loop={mixed_loops}");
    if mixed_loops {
        if output.output.event_rebuild_changed {
            rebuild = output.output.event_rebuild.clone();
        }
        let begin = Instant::now();
        for node in std::iter::once(2).chain(if kind == "upward" {
            4..4 + owners
        } else {
            nodes..nodes + 1
        }) {
            tree.get_mut(&NodeId(node as u64))
                .unwrap()
                .spec
                .declared
                .animate = None;
        }
        let settled = publish(
            &mut tree,
            constraint,
            1.0,
            &mut runtime,
            start + Duration::from_micros(1_000_001),
            TreeInvalidation::Measure,
            Some(&rebuild),
        )
        .unwrap();
        println!(
            "settle_us={} {} {}",
            begin.elapsed().as_micros(),
            rss(),
            diagnostics_for_benchmark(&tree)
        );
        assert!(diagnostics_for_benchmark(&tree).starts_with("workspace=false"));
        assert!(!settled.output.animations_active);
        std::hint::black_box(settled);
    }
    if !["moving", "mixed", "independent", "coupled", "upward"].contains(&kind.as_str()) {
        assert!(!output.output.animations_active);
    }
    println!(
        "publication_only=true raster_timing=false allocator_retained_pages_are_not_live_record_counts=true"
    );
}
