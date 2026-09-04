mod support;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use emerge_skia::assets::AssetRuntime;
use emerge_skia::events::{
    RegistryRebuildPayload,
    registry_builder::{
        build_registry_rebuild_cached_for_benchmark, build_registry_rebuild_for_benchmark,
    },
};
use emerge_skia::render_scene::RenderSceneSummary;
use emerge_skia::tree::animation::{
    AnimationCurve, AnimationRepeat, AnimationRuntime, AnimationSpec,
};
use emerge_skia::tree::attrs::{
    AlignY, Attrs, Background, BorderRadius, BoxShadow, Color, Font, FontWeight, Length, Padding,
};
use emerge_skia::tree::deserialize::decode_tree;
use emerge_skia::tree::element::{
    Element, ElementKind, ElementTree, Frame, NearbyMount, NearbySlot, NodeId,
};
use emerge_skia::tree::invalidation::TreeInvalidation;
#[cfg(feature = "bench-diagnostics")]
use emerge_skia::tree::layout::layout_or_refresh_default_with_animation_and_invalidation_profile_for_benchmark;
use emerge_skia::tree::layout::{
    Constraint, layout_and_refresh_default, layout_and_refresh_default_with_animation,
    layout_or_refresh_default_with_animation,
    layout_or_refresh_default_with_animation_and_dirty_ids_reusing_clean_registry_for_benchmark,
    layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark,
    layout_tree, layout_tree_default, refresh, refresh_render_scene_for_benchmark,
    refresh_reusing_clean_registry_for_benchmark,
};
use emerge_skia::tree::patch::{Patch, apply_patches, decode_patches};
#[cfg(feature = "bench-diagnostics")]
use emerge_skia::tree::render::{
    reset_render_traversal_diagnostics_for_benchmark,
    take_render_traversal_diagnostics_for_benchmark,
};
use std::hint::black_box;
use std::time::{Duration, Instant};
use support::{
    CARD_COUNT, MockTextMeasurer, SCROLL_VIEWPORT_ROW_COUNT, TEXT_ROW_COUNT,
    animated_shadow_showcase, large_paint_rich_scroll_column, large_simple_scroll_column,
    large_text_column, load_fixture, nested_card_grid, rich_borders_shadow_showcase,
    scrollable_animated_shadow_showcase, scrollable_rich_borders_shadow_showcase,
};

const RETAINED_FIXTURE_IDS: &[&str] = &[
    "list_text_500",
    "text_rich_500",
    "layout_matrix_500",
    "paint_rich_500",
    "nearby_rich_500",
];

const RETAINED_MUTATIONS: &[&str] = &[
    "noop",
    "paint_attr",
    "event_attr",
    "layout_attr",
    "text_content",
    "keyed_reorder",
    "insert_tail",
    "remove_tail",
    "nearby_slot_change",
    "nearby_reorder",
];

const RENDER_REFRESH_REGRESSION_FIXTURE_CASES: &[(&str, &str)] = &[
    ("paint_rich_500", "paint_attr"),
    ("nearby_rich_500", "paint_attr"),
    ("nearby_rich_500", "nearby_slot_change"),
    ("layout_matrix_500", "paint_attr"),
];

const REGISTRY_REFRESH_REGRESSION_FIXTURE_CASES: &[(&str, &str)] = &[
    ("interactive_rich_500", "event_attr"),
    ("nearby_rich_500", "event_attr"),
    ("nearby_rich_500", "nearby_slot_change"),
    ("scroll_rich_500", "event_attr"),
    ("text_rich_500", "event_attr"),
];

const EMERGE_DEMO_SHOWCASE_LAYOUT_EMRG: &[u8] =
    include_bytes!("../../../bench/external_fixtures/emerge_demo_showcase_layout/full.emrg");
const EMERGE_DEMO_SHOWCASE_BORDERS_EMRG: &[u8] =
    include_bytes!("../../../bench/external_fixtures/emerge_demo_showcase_borders/full.emrg");
const EMERGE_DEMO_SHOWCASE_INTERACTION_EMRG: &[u8] =
    include_bytes!("../../../bench/external_fixtures/emerge_demo_showcase_interaction/full.emrg");
const EMERGE_DEMO_SHOWCASE_INTERACTION_VIRTUAL_KEY_PATCH: &[u8] = include_bytes!(
    "../../../bench/external_fixtures/emerge_demo_showcase_interaction/virtual_key_text_echo.patch"
);
const EMERGE_DEMO_SHOWCASE_INTERACTION_VIRTUAL_KEY_REVERSE_PATCH: &[u8] = include_bytes!(
    "../../../bench/external_fixtures/emerge_demo_showcase_interaction/virtual_key_text_echo_reverse.patch"
);
const SHOWCASE_FRAME_MS: u64 = 16;
const SHOWCASE_SCROLL_ID: NodeId = NodeId(33);
const SHOWCASE_LAYOUT_VISIBLE_WIDTH: u32 = 1440;
const SHOWCASE_LAYOUT_VISIBLE_HEIGHT: u32 = 900;
const SHOWCASE_LAYOUT_VISIBLE_SCROLL_Y: f32 = 256.0;
const SHOWCASE_INTERACTION_WIDTH: u32 = 1440;
const SHOWCASE_INTERACTION_HEIGHT: u32 = 900;
const SHOWCASE_INTERACTION_INITIAL_TEXT: &str = "quick brown fox";
const SHOWCASE_INTERACTION_NEXT_TEXT: &str = "quick brown foxa";
const SHOWCASE_BORDERS_SCREENSHOT_WIDTH: u32 = 1909;
const SHOWCASE_BORDERS_SCREENSHOT_HEIGHT: u32 = 2148;
const SHOWCASE_BORDERS_SCREENSHOT_SCALE: f32 = 1.5;
const SHOWCASE_BORDERS_SCREENSHOT_SCROLL_Y: f32 = 952.0;

fn bench_large_text_column(c: &mut Criterion) {
    let mut group = c.benchmark_group(format!("native/layout/list_text_{TEXT_ROW_COUNT}"));
    let constraint = Constraint::new(900.0, 4_000.0);
    let measurer = MockTextMeasurer;
    let node_count = large_text_column(TEXT_ROW_COUNT).len() as u64;
    group.throughput(Throughput::Elements(node_count));

    group.bench_function("layout_only_mock_text", |b| {
        b.iter_batched(
            || large_text_column(TEXT_ROW_COUNT),
            |mut tree| {
                layout_tree(&mut tree, constraint, 1.0, &measurer);
                black_box(tree.len())
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("layout_only_skia_text", |b| {
        b.iter_batched(
            || large_text_column(TEXT_ROW_COUNT),
            |mut tree| {
                layout_tree_default(&mut tree, constraint, 1.0);
                black_box(tree.len())
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("layout_plus_refresh", |b| {
        b.iter_batched(
            || large_text_column(TEXT_ROW_COUNT),
            |mut tree| {
                let output = layout_and_refresh_default(&mut tree, constraint, 1.0);
                black_box((
                    output.scene.nodes.len(),
                    output.event_rebuild.text_inputs.len(),
                ))
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("refresh_only_after_layout", |b| {
        b.iter_batched(
            || {
                let mut tree = large_text_column(TEXT_ROW_COUNT);
                layout_tree_default(&mut tree, constraint, 1.0);
                tree
            },
            |mut tree| {
                let output = refresh(&mut tree);
                black_box((
                    output.scene.nodes.len(),
                    output.event_rebuild.text_inputs.len(),
                ))
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_nested_card_grid(c: &mut Criterion) {
    let mut group = c.benchmark_group(format!("native/layout/card_grid_{CARD_COUNT}"));
    let constraint = Constraint::new(960.0, 4_000.0);
    let measurer = MockTextMeasurer;
    let node_count = nested_card_grid(CARD_COUNT).len() as u64;
    group.throughput(Throughput::Elements(node_count));

    group.bench_function("layout_only_mock_text", |b| {
        b.iter_batched(
            || nested_card_grid(CARD_COUNT),
            |mut tree| {
                layout_tree(&mut tree, constraint, 1.0, &measurer);
                black_box(tree.len())
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("layout_plus_refresh", |b| {
        b.iter_batched(
            || nested_card_grid(CARD_COUNT),
            |mut tree| {
                let output = layout_and_refresh_default(&mut tree, constraint, 1.0);
                black_box((
                    output.scene.nodes.len(),
                    output.event_rebuild.text_inputs.len(),
                ))
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("refresh_only_after_layout", |b| {
        b.iter_batched(
            || {
                let mut tree = nested_card_grid(CARD_COUNT);
                layout_tree_default(&mut tree, constraint, 1.0);
                tree
            },
            |mut tree| {
                let output = refresh(&mut tree);
                black_box((
                    output.scene.nodes.len(),
                    output.event_rebuild.text_inputs.len(),
                ))
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

// Reuse one warmed tree across iterations to measure retained layout cache hits.
fn bench_large_text_column_retained(c: &mut Criterion) {
    let mut group = c.benchmark_group(format!("native/layout_retained/list_text_{TEXT_ROW_COUNT}"));
    let constraint = Constraint::new(900.0, 4_000.0);
    let measurer = MockTextMeasurer;
    let node_count = large_text_column(TEXT_ROW_COUNT).len() as u64;
    group.throughput(Throughput::Elements(node_count));

    let mut mock_tree = large_text_column(TEXT_ROW_COUNT);
    layout_tree(&mut mock_tree, constraint, 1.0, &measurer);
    group.bench_function("warm_layout_only_mock_text", |b| {
        b.iter(|| {
            layout_tree(&mut mock_tree, constraint, 1.0, &measurer);
            black_box(mock_tree.len())
        });
    });

    let mut skia_tree = large_text_column(TEXT_ROW_COUNT);
    layout_tree_default(&mut skia_tree, constraint, 1.0);
    group.bench_function("warm_layout_only_skia_text", |b| {
        b.iter(|| {
            layout_tree_default(&mut skia_tree, constraint, 1.0);
            black_box(skia_tree.len())
        });
    });

    let mut refresh_tree = large_text_column(TEXT_ROW_COUNT);
    layout_and_refresh_default(&mut refresh_tree, constraint, 1.0);
    group.bench_function("warm_layout_plus_refresh", |b| {
        b.iter(|| {
            let output = layout_and_refresh_default(&mut refresh_tree, constraint, 1.0);
            black_box((
                output.scene.nodes.len(),
                output.event_rebuild.text_inputs.len(),
            ))
        });
    });

    group.finish();
}

fn bench_nested_card_grid_retained(c: &mut Criterion) {
    let mut group = c.benchmark_group(format!("native/layout_retained/card_grid_{CARD_COUNT}"));
    let constraint = Constraint::new(960.0, 4_000.0);
    let measurer = MockTextMeasurer;
    let node_count = nested_card_grid(CARD_COUNT).len() as u64;
    group.throughput(Throughput::Elements(node_count));

    let mut mock_tree = nested_card_grid(CARD_COUNT);
    layout_tree(&mut mock_tree, constraint, 1.0, &measurer);
    group.bench_function("warm_layout_only_mock_text", |b| {
        b.iter(|| {
            layout_tree(&mut mock_tree, constraint, 1.0, &measurer);
            black_box(mock_tree.len())
        });
    });

    let mut refresh_tree = nested_card_grid(CARD_COUNT);
    layout_and_refresh_default(&mut refresh_tree, constraint, 1.0);
    group.bench_function("warm_layout_plus_refresh", |b| {
        b.iter(|| {
            let output = layout_and_refresh_default(&mut refresh_tree, constraint, 1.0);
            black_box((
                output.scene.nodes.len(),
                output.event_rebuild.text_inputs.len(),
            ))
        });
    });

    group.finish();
}

fn bench_layout_aware_transform(c: &mut Criterion) {
    let mut group = c.benchmark_group(format!(
        "native/layout_aware_transform/card_grid_{CARD_COUNT}"
    ));
    let node_count = nested_card_grid(CARD_COUNT).len() as u64;
    group.throughput(Throughput::Elements(node_count));

    let cases = [
        LayoutTransformBenchCase {
            name: "no_transform",
            constraint: Constraint::new(960.0, 4_000.0),
            global_scale: 1.0,
            transform: LayoutTransformCase::None,
        },
        LayoutTransformBenchCase {
            name: "global_scale_1_25",
            constraint: Constraint::new(960.0, 4_000.0),
            global_scale: 1.25,
            transform: LayoutTransformCase::None,
        },
        LayoutTransformBenchCase {
            name: "root_scale_1_25",
            constraint: Constraint::new(960.0, 4_000.0),
            global_scale: 1.0,
            transform: LayoutTransformCase::RootScale(1.25),
        },
        LayoutTransformBenchCase {
            name: "root_scale_1_5",
            constraint: Constraint::new(960.0, 4_000.0),
            global_scale: 1.0,
            transform: LayoutTransformCase::RootScale(1.5),
        },
        LayoutTransformBenchCase {
            name: "nested_scale_1_25",
            constraint: Constraint::new(960.0, 4_000.0),
            global_scale: 1.0,
            transform: LayoutTransformCase::NestedScale(1.25),
        },
        LayoutTransformBenchCase {
            name: "root_rotate_90_portrait",
            constraint: Constraint::new(540.0, 960.0),
            global_scale: 1.0,
            transform: LayoutTransformCase::RootRotate(90.0),
        },
        LayoutTransformBenchCase {
            name: "root_rotate_45",
            constraint: Constraint::new(960.0, 4_000.0),
            global_scale: 1.0,
            transform: LayoutTransformCase::RootRotate(45.0),
        },
        LayoutTransformBenchCase {
            name: "nested_rotate_45",
            constraint: Constraint::new(960.0, 4_000.0),
            global_scale: 1.0,
            transform: LayoutTransformCase::NestedRotate(45.0),
        },
    ];

    for case in cases {
        group.bench_function(case.name, |b| {
            b.iter_batched(
                || {
                    let mut tree = nested_card_grid(CARD_COUNT);
                    configure_layout_transform_case(&mut tree, case.transform);
                    tree
                },
                |mut tree| {
                    let output =
                        layout_and_refresh_default(&mut tree, case.constraint, case.global_scale);
                    consume_layout_output(output)
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn bench_layout_aware_transform_animation(c: &mut Criterion) {
    let mut group = c.benchmark_group(format!(
        "native/layout_aware_transform_animation/card_grid_{CARD_COUNT}"
    ));
    let node_count = nested_card_grid(CARD_COUNT).len() as u64;
    group.throughput(Throughput::Elements(node_count));

    let cases = [
        LayoutTransformAnimationBenchCase {
            name: "paint_only_scale",
            constraint: Constraint::new(960.0, 4_000.0),
            animation: LayoutTransformAnimationCase::PaintScale(1.0, 1.12),
        },
        LayoutTransformAnimationBenchCase {
            name: "paint_only_rotate",
            constraint: Constraint::new(960.0, 4_000.0),
            animation: LayoutTransformAnimationCase::PaintRotate(0.0, 12.0),
        },
        LayoutTransformAnimationBenchCase {
            name: "root_scale_1_to_1_25",
            constraint: Constraint::new(960.0, 4_000.0),
            animation: LayoutTransformAnimationCase::RootScale(1.0, 1.25),
        },
        LayoutTransformAnimationBenchCase {
            name: "root_scale_1_to_1_5",
            constraint: Constraint::new(960.0, 4_000.0),
            animation: LayoutTransformAnimationCase::RootScale(1.0, 1.5),
        },
        LayoutTransformAnimationBenchCase {
            name: "nested_scale_1_to_1_25",
            constraint: Constraint::new(960.0, 4_000.0),
            animation: LayoutTransformAnimationCase::NestedScale(1.0, 1.25),
        },
        LayoutTransformAnimationBenchCase {
            name: "root_rotate_0_to_90_portrait",
            constraint: Constraint::new(540.0, 960.0),
            animation: LayoutTransformAnimationCase::RootRotate(0.0, 90.0),
        },
        LayoutTransformAnimationBenchCase {
            name: "root_rotate_0_to_45",
            constraint: Constraint::new(960.0, 4_000.0),
            animation: LayoutTransformAnimationCase::RootRotate(0.0, 45.0),
        },
        LayoutTransformAnimationBenchCase {
            name: "nested_rotate_0_to_45",
            constraint: Constraint::new(960.0, 4_000.0),
            animation: LayoutTransformAnimationCase::NestedRotate(0.0, 45.0),
        },
        LayoutTransformAnimationBenchCase {
            name: "nested_scale_with_width",
            constraint: Constraint::new(960.0, 4_000.0),
            animation: LayoutTransformAnimationCase::NestedScaleWithWidth {
                from_scale: 1.0,
                to_scale: 1.25,
                from_width: 120.0,
                to_width: 180.0,
            },
        },
    ];

    for case in cases {
        group.bench_function(case.name, |b| {
            let start = Instant::now();
            let mut tree = nested_card_grid(CARD_COUNT);
            configure_layout_transform_animation_case(&mut tree, case.animation);
            let mut runtime = AnimationRuntime::default();
            runtime.sync_with_tree(&tree, start);
            layout_and_refresh_default_with_animation(
                &mut tree,
                case.constraint,
                1.0,
                &runtime,
                start,
            );
            let mut tick = 0_u64;

            b.iter(|| {
                tick += 16;
                let update = layout_or_refresh_default_with_animation(
                    &mut tree,
                    case.constraint,
                    1.0,
                    &runtime,
                    start + Duration::from_millis(tick),
                );
                consume_layout_update_output(update)
            });
        });
    }

    group.finish();
}

#[derive(Clone, Copy)]
struct LayoutTransformBenchCase {
    name: &'static str,
    constraint: Constraint,
    global_scale: f32,
    transform: LayoutTransformCase,
}

#[derive(Clone, Copy)]
enum LayoutTransformCase {
    None,
    RootScale(f64),
    RootRotate(f64),
    NestedScale(f64),
    NestedRotate(f64),
}

#[derive(Clone, Copy)]
struct LayoutTransformAnimationBenchCase {
    name: &'static str,
    constraint: Constraint,
    animation: LayoutTransformAnimationCase,
}

#[derive(Clone, Copy)]
enum LayoutTransformAnimationCase {
    PaintScale(f64, f64),
    PaintRotate(f64, f64),
    RootScale(f64, f64),
    RootRotate(f64, f64),
    NestedScale(f64, f64),
    NestedRotate(f64, f64),
    NestedScaleWithWidth {
        from_scale: f64,
        to_scale: f64,
        from_width: f64,
        to_width: f64,
    },
}

fn configure_layout_transform_case(tree: &mut ElementTree, transform: LayoutTransformCase) {
    match transform {
        LayoutTransformCase::None => {}
        LayoutTransformCase::RootScale(scale) => {
            if let Some(root_id) = tree.root_id()
                && let Some(root) = tree.get_mut(&root_id)
            {
                root.spec.declared.layout_scale = Some(scale);
            }
        }
        LayoutTransformCase::RootRotate(degrees) => {
            if let Some(root_id) = tree.root_id()
                && let Some(root) = tree.get_mut(&root_id)
            {
                root.spec.declared.layout_rotate = Some(degrees);
            }
        }
        LayoutTransformCase::NestedScale(scale) => {
            if let Some(child_id) = first_root_child_id(tree)
                && let Some(child) = tree.get_mut(&child_id)
            {
                child.spec.declared.layout_scale = Some(scale);
            }
        }
        LayoutTransformCase::NestedRotate(degrees) => {
            if let Some(child_id) = first_root_child_id(tree)
                && let Some(child) = tree.get_mut(&child_id)
            {
                child.spec.declared.layout_rotate = Some(degrees);
            }
        }
    }
}

fn configure_layout_transform_animation_case(
    tree: &mut ElementTree,
    animation: LayoutTransformAnimationCase,
) {
    match animation {
        LayoutTransformAnimationCase::PaintScale(from, to) => {
            set_root_animation(tree, paint_scale_animation_spec(from, to));
        }
        LayoutTransformAnimationCase::PaintRotate(from, to) => {
            set_root_animation(tree, paint_rotate_animation_spec(from, to));
        }
        LayoutTransformAnimationCase::RootScale(from, to) => {
            set_root_animation(tree, layout_scale_animation_spec(from, to));
        }
        LayoutTransformAnimationCase::RootRotate(from, to) => {
            set_root_animation(tree, layout_rotate_animation_spec(from, to));
        }
        LayoutTransformAnimationCase::NestedScale(from, to) => {
            set_first_child_animation(tree, layout_scale_animation_spec(from, to));
        }
        LayoutTransformAnimationCase::NestedRotate(from, to) => {
            set_first_child_animation(tree, layout_rotate_animation_spec(from, to));
        }
        LayoutTransformAnimationCase::NestedScaleWithWidth {
            from_scale,
            to_scale,
            from_width,
            to_width,
        } => {
            set_first_child_animation(
                tree,
                layout_scale_with_width_animation_spec(from_scale, to_scale, from_width, to_width),
            );
        }
    }
}

fn set_root_animation(tree: &mut ElementTree, spec: AnimationSpec) {
    if let Some(root_id) = tree.root_id()
        && let Some(root) = tree.get_mut(&root_id)
    {
        root.spec.declared.animate = Some(spec);
    }
}

fn set_first_child_animation(tree: &mut ElementTree, spec: AnimationSpec) {
    if let Some(child_id) = first_root_child_id(tree)
        && let Some(child) = tree.get_mut(&child_id)
    {
        child.spec.declared.animate = Some(spec);
    }
}

fn layout_scale_animation_spec(from: f64, to: f64) -> AnimationSpec {
    let from_attrs = Attrs {
        layout_scale: Some(from),
        ..Attrs::default()
    };
    let to_attrs = Attrs {
        layout_scale: Some(to),
        ..Attrs::default()
    };
    animation_spec(from_attrs, to_attrs)
}

fn layout_rotate_animation_spec(from: f64, to: f64) -> AnimationSpec {
    let from_attrs = Attrs {
        layout_rotate: Some(from),
        ..Attrs::default()
    };
    let to_attrs = Attrs {
        layout_rotate: Some(to),
        ..Attrs::default()
    };
    animation_spec(from_attrs, to_attrs)
}

fn layout_scale_with_width_animation_spec(
    from_scale: f64,
    to_scale: f64,
    from_width: f64,
    to_width: f64,
) -> AnimationSpec {
    let from_attrs = Attrs {
        layout_scale: Some(from_scale),
        width: Some(Length::Px(from_width)),
        ..Attrs::default()
    };
    let to_attrs = Attrs {
        layout_scale: Some(to_scale),
        width: Some(Length::Px(to_width)),
        ..Attrs::default()
    };
    animation_spec(from_attrs, to_attrs)
}

fn paint_scale_animation_spec(from: f64, to: f64) -> AnimationSpec {
    let from_attrs = Attrs {
        scale: Some(from),
        ..Attrs::default()
    };
    let to_attrs = Attrs {
        scale: Some(to),
        ..Attrs::default()
    };
    animation_spec(from_attrs, to_attrs)
}

fn paint_rotate_animation_spec(from: f64, to: f64) -> AnimationSpec {
    let from_attrs = Attrs {
        rotate: Some(from),
        ..Attrs::default()
    };
    let to_attrs = Attrs {
        rotate: Some(to),
        ..Attrs::default()
    };
    animation_spec(from_attrs, to_attrs)
}

fn animation_spec(from: Attrs, to: Attrs) -> AnimationSpec {
    AnimationSpec {
        keyframes: vec![from, to],
        duration_ms: 1_000.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    }
}

fn first_root_child_id(tree: &ElementTree) -> Option<NodeId> {
    tree.root_id()
        .and_then(|root_id| tree.child_ids(&root_id).into_iter().next())
}

// Apply each patch during setup so the timed body is the first layout after invalidation.
fn bench_animated_shadow_showcase(c: &mut Criterion) {
    bench_animation_paint_only_showcase(
        c,
        "native/layout_animation_paint_only/shadow_showcase",
        Constraint::new(960.0, 4_000.0),
        animated_shadow_showcase,
    );
}

fn bench_rich_borders_shadow_showcase(c: &mut Criterion) {
    bench_animation_paint_only_showcase(
        c,
        "native/layout_animation_paint_only/rich_borders_showcase",
        Constraint::new(960.0, 4_000.0),
        rich_borders_shadow_showcase,
    );
}

fn bench_animation_paint_only_showcase(
    c: &mut Criterion,
    group_name: &str,
    constraint: Constraint,
    make_tree: fn() -> ElementTree,
) {
    let mut group = c.benchmark_group(group_name);
    let start = Instant::now();
    let node_count = make_tree().len() as u64;
    group.throughput(Throughput::Elements(node_count));

    let mut full_tree = make_tree();
    let mut full_runtime = AnimationRuntime::default();
    full_runtime.sync_with_tree(&full_tree, start);
    layout_and_refresh_default_with_animation(
        &mut full_tree,
        constraint,
        1.0,
        &full_runtime,
        start,
    );
    let mut full_tick = 0_u64;
    group.bench_function("full_layout_plus_refresh_each_frame", |b| {
        b.iter(|| {
            full_tick += 16;
            let output = layout_and_refresh_default_with_animation(
                &mut full_tree,
                constraint,
                1.0,
                &full_runtime,
                start + Duration::from_millis(full_tick),
            );
            black_box((
                output.scene.nodes.len(),
                output.event_rebuild.text_inputs.len(),
                true,
            ))
        });
    });

    let mut refresh_tree = make_tree();
    let mut refresh_runtime = AnimationRuntime::default();
    refresh_runtime.sync_with_tree(&refresh_tree, start);
    layout_and_refresh_default_with_animation(
        &mut refresh_tree,
        constraint,
        1.0,
        &refresh_runtime,
        start,
    );
    let mut refresh_tick = 0_u64;
    group.bench_function("paint_only_refresh_each_frame", |b| {
        b.iter(|| {
            refresh_tick += 16;
            let update = layout_or_refresh_default_with_animation(
                &mut refresh_tree,
                constraint,
                1.0,
                &refresh_runtime,
                start + Duration::from_millis(refresh_tick),
            );
            black_box((
                update.output.scene.nodes.len(),
                update.output.event_rebuild.text_inputs.len(),
                update.layout_performed,
            ))
        });
    });

    group.finish();
}

fn bench_scrolling_animated_shadow_showcase(c: &mut Criterion) {
    bench_scrolling_animation_paint_only_showcase(
        c,
        "native/layout_scroll_paint_only_animation/shadow_showcase",
        Constraint::new(960.0, 640.0),
        scrollable_animated_shadow_showcase,
    );
}

fn bench_scrolling_rich_borders_shadow_showcase(c: &mut Criterion) {
    bench_scrolling_animation_paint_only_showcase(
        c,
        "native/layout_scroll_paint_only_animation/rich_borders_showcase",
        Constraint::new(960.0, 900.0),
        scrollable_rich_borders_shadow_showcase,
    );
}

fn bench_emerge_demo_showcase_layout_refresh(c: &mut Criterion) {
    let mut group = c.benchmark_group("native/layout_refresh/emerge_demo_showcase");

    let mut layout_case = ShowcaseLayoutVisibleAnimationCase::new();
    group.throughput(Throughput::Elements(
        layout_case.initial_summary.nodes as u64,
    ));
    group.bench_function("layout_page_visible_animated_row", |b| {
        b.iter(|| {
            let update = layout_case.next_frame();
            assert!(update.layout_performed);
            black_box(update.output.scene.nodes.len())
        });
    });

    let mut hover_case = ShowcaseBordersHoverCase::new();
    group.throughput(Throughput::Elements(
        hover_case.initial_summary.nodes as u64,
    ));
    group.bench_function("borders_screenshot_hover_visible_targets", |b| {
        b.iter(|| {
            let update = hover_case.next_hover_frame();
            black_box((update.layout_performed, update.output.scene.nodes.len()))
        });
    });

    let mut held_nearby_case = ShowcaseBordersHeldNearbyCase::new();
    group.throughput(Throughput::Elements(
        held_nearby_case.initial_summary.nodes as u64,
    ));
    group.bench_function("borders_screenshot_held_nearby_refresh", |b| {
        b.iter(|| {
            let update = held_nearby_case.next_frame();
            black_box((update.layout_performed, update.output.scene.nodes.len()))
        });
    });

    let mut interaction_case = ShowcaseInteractionVirtualKeyboardCase::new();
    group.throughput(Throughput::Elements(
        interaction_case.initial_summary.nodes as u64,
    ));
    group.bench_function("interaction_virtual_keyboard_text_echo", |b| {
        b.iter(|| {
            let update = interaction_case.next_text_echo_frame();
            black_box((update.layout_performed, update.output.scene.nodes.len()))
        });
    });

    let mut interaction_key_case = ShowcaseInteractionVirtualKeyFullLoopCase::new();
    group.throughput(Throughput::Elements(
        interaction_key_case.initial_summary.nodes as u64,
    ));
    group.bench_function("interaction_virtual_key_full_loop", |b| {
        b.iter(|| {
            let update = interaction_key_case.next_frame();
            black_box((update.layout_performed, update.output.scene.nodes.len()))
        });
    });

    let mut interaction_scroll_case = ShowcaseInteractionScrollCase::new();
    group.throughput(Throughput::Elements(
        interaction_scroll_case.initial_summary.nodes as u64,
    ));
    group.bench_function("interaction_scroll_step_cached_refresh", |b| {
        b.iter(|| {
            let update = interaction_scroll_case.next_scroll_frame();
            black_box((update.layout_performed, update.output.scene.nodes.len()))
        });
    });

    group.finish();
}

struct ShowcaseLayoutVisibleAnimationCase {
    tree: ElementTree,
    runtime: AnimationRuntime,
    cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    target: ShowcaseLayoutTarget,
    next_frame: u64,
    initial_summary: RenderSceneSummary,
}

impl ShowcaseLayoutVisibleAnimationCase {
    fn new() -> Self {
        let started_at = Instant::now();
        let tree =
            decode_tree(EMERGE_DEMO_SHOWCASE_LAYOUT_EMRG).expect("layout fixture should decode");
        let mut runtime = AnimationRuntime::default();
        runtime.sync_with_tree(&tree, started_at);
        let target = ShowcaseLayoutTarget::visible_animation_fixture();
        if std::env::var_os("EMERGE_BENCH_DIAGNOSTICS").is_some() {
            eprintln!("showcase layout visible target: {target:?}");
        }
        let constraint = Constraint::new(target.width as f32, target.height as f32);
        let mut tree = tree.clone();
        let initial = layout_and_refresh_default_with_animation(
            &mut tree, constraint, 1.0, &runtime, started_at,
        );
        tree.apply_scroll_y(&target.scroll_id, -target.scroll_y);
        let warm = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut tree,
            constraint,
            1.0,
            &runtime,
            started_at,
            TreeInvalidation::None,
            Some(&initial.event_rebuild),
        );
        assert!(
            warm.layout_performed,
            "showcase layout visible animation should require layout: target={target:?}"
        );
        let initial_summary = warm.output.scene.summary();
        assert!(
            initial_summary.cacheable_layers > 0
                && initial_summary.nodes >= 600
                && initial_summary.texts >= 100,
            "showcase layout visible animation selected the wrong scene: \
             target={target:?}, summary={initial_summary:?}"
        );

        let cached_rebuild = if warm.output.event_rebuild_changed {
            warm.output.event_rebuild
        } else {
            initial.event_rebuild
        };
        #[cfg(feature = "bench-diagnostics")]
        if std::env::var_os("EMERGE_BENCH_DIAGNOSTICS").is_some() {
            sample_showcase_layout_profile(
                tree.clone(),
                runtime.clone(),
                cached_rebuild.clone(),
                started_at,
                target,
            );
        }

        Self {
            tree,
            runtime,
            cached_rebuild,
            started_at,
            target,
            next_frame: 1,
            initial_summary,
        }
    }

    fn next_frame(&mut self) -> emerge_skia::tree::layout::LayoutUpdateOutput {
        self.next_frame = self.next_frame.saturating_add(1);
        let update = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut self.tree,
            Constraint::new(self.target.width as f32, self.target.height as f32),
            1.0,
            &self.runtime,
            self.started_at + Duration::from_millis(self.next_frame.saturating_mul(SHOWCASE_FRAME_MS)),
            TreeInvalidation::None,
            Some(&self.cached_rebuild),
        );

        if update.output.event_rebuild_changed {
            self.cached_rebuild = update.output.event_rebuild.clone();
        }

        update
    }
}

#[cfg(feature = "bench-diagnostics")]
fn sample_showcase_layout_profile(
    mut tree: ElementTree,
    runtime: AnimationRuntime,
    mut cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    target: ShowcaseLayoutTarget,
) {
    let constraint = Constraint::new(target.width as f32, target.height as f32);
    for frame in 1..=6_u64 {
        let (update, profile) =
            layout_or_refresh_default_with_animation_and_invalidation_profile_for_benchmark(
                &mut tree,
                constraint,
                1.0,
                &runtime,
                started_at + Duration::from_millis(frame.saturating_mul(SHOWCASE_FRAME_MS)),
                TreeInvalidation::None,
                Some(&cached_rebuild),
            );
        if update.output.event_rebuild_changed {
            cached_rebuild = update.output.event_rebuild;
        }
        eprintln!(
            "showcase layout profile frame={frame} prepare={:.3}ms layout={:.3}ms refresh={:.3}ms traversal={:.3}ms registry_post={:.3}ms layout_performed={} scene_nodes={} render_visits={} culled={} registry_visits={} registry_hits={} registry_stores={} registry_damaged={} registry_ineligible={} registry_misses={} pre_registry_damage={} registry_damage={} registry_damage_nodes={}",
            profile.prepare.as_secs_f64() * 1000.0,
            profile.layout.as_secs_f64() * 1000.0,
            profile.refresh.as_secs_f64() * 1000.0,
            profile.refresh_traversal.as_secs_f64() * 1000.0,
            profile.refresh_registry_post.as_secs_f64() * 1000.0,
            profile.layout_performed,
            profile.scene_nodes,
            profile.render_visits,
            profile.culled_subtrees,
            profile.registry_visits,
            profile.registry_cache_hits,
            profile.registry_cache_stores,
            profile.registry_cache_damaged,
            profile.registry_cache_ineligible,
            profile.registry_cache_misses,
            profile.pre_layout_registry_damage,
            profile.registry_damage,
            profile.registry_damage_nodes
        );
        if frame == 1 {
            eprintln!(
                "showcase layout scene summary: {:?}",
                update.output.scene.summary()
            );
            print_showcase_paint_layers(&update.output.scene.nodes, 0);
        }
    }
}

#[cfg(feature = "bench-diagnostics")]
fn print_showcase_paint_layers(nodes: &[emerge_skia::render_scene::RenderNode], depth: usize) {
    for node in nodes {
        match node {
            emerge_skia::render_scene::RenderNode::PaintLayer(layer) => {
                print_showcase_paint_layer(layer, depth)
            }
            emerge_skia::render_scene::RenderNode::Clip { children, .. }
            | emerge_skia::render_scene::RenderNode::RelaxedClip { children, .. }
            | emerge_skia::render_scene::RenderNode::Transform { children, .. }
            | emerge_skia::render_scene::RenderNode::Alpha { children, .. }
            | emerge_skia::render_scene::RenderNode::ShadowPass { children } => {
                print_showcase_paint_layers(children, depth);
            }
            emerge_skia::render_scene::RenderNode::Primitive(_) => {}
        }
    }
}

#[cfg(feature = "bench-diagnostics")]
fn print_showcase_paint_layer(layer: &emerge_skia::render_scene::RenderPaintLayer, depth: usize) {
    let indent = "  ".repeat(depth);
    eprintln!(
        "{indent}layer node_id={} reason={:?} policy={:?} bounds=({:.1},{:.1},{:.1},{:.1}) own_nodes={} own_primitives={} own_cost={} content_nodes={} generation={}",
        layer.id.node_id,
        layer.id.role,
        layer.policy,
        layer.bounds.x,
        layer.bounds.y,
        layer.bounds.width,
        layer.bounds.height,
        layer.metrics.own_node_count,
        layer.metrics.own_primitive_count,
        layer.metrics.own_primitive_cost,
        layer.content.nodes.len(),
        layer.content_generation
    );
    print_showcase_paint_layer_content(&layer.content.nodes, depth + 1);
}

#[cfg(feature = "bench-diagnostics")]
fn print_showcase_paint_layer_content(
    content: &[emerge_skia::render_scene::RenderPaintLayerContentNode],
    depth: usize,
) {
    for node in content {
        match node {
            emerge_skia::render_scene::RenderPaintLayerContentNode::Own(run) => {
                print_showcase_paint_layers(&run.nodes, depth)
            }
            emerge_skia::render_scene::RenderPaintLayerContentNode::Child(layer) => {
                print_showcase_paint_layer(layer, depth)
            }
            emerge_skia::render_scene::RenderPaintLayerContentNode::ShadowPass { children }
            | emerge_skia::render_scene::RenderPaintLayerContentNode::Clip { children, .. }
            | emerge_skia::render_scene::RenderPaintLayerContentNode::RelaxedClip {
                children,
                ..
            }
            | emerge_skia::render_scene::RenderPaintLayerContentNode::Transform {
                children, ..
            }
            | emerge_skia::render_scene::RenderPaintLayerContentNode::Alpha { children, .. } => {
                print_showcase_paint_layer_content(children, depth)
            }
        }
    }
}

struct ShowcaseBordersHoverCase {
    tree: ElementTree,
    runtime: AnimationRuntime,
    cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    target: ShowcaseBordersTarget,
    hover_ids: Vec<NodeId>,
    active_id: Option<NodeId>,
    next_hover: usize,
    next_frame: u64,
    initial_summary: RenderSceneSummary,
}

impl ShowcaseBordersHoverCase {
    fn new() -> Self {
        let started_at = Instant::now();
        let tree =
            decode_tree(EMERGE_DEMO_SHOWCASE_BORDERS_EMRG).expect("Borders fixture should decode");
        let mut runtime = AnimationRuntime::default();
        runtime.sync_with_tree(&tree, started_at);
        let target = ShowcaseBordersTarget::screenshot_fixture();
        if std::env::var_os("EMERGE_BENCH_DIAGNOSTICS").is_some() {
            eprintln!("showcase Borders hover target: {target:?}");
        }
        let constraint = target.constraint();
        let mut tree = tree.clone();
        let initial = layout_and_refresh_default_with_animation(
            &mut tree,
            constraint,
            target.scale,
            &runtime,
            started_at,
        );
        tree.apply_scroll_y(&target.scroll_id, -target.scroll_y);
        let warm = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut tree,
            constraint,
            target.scale,
            &runtime,
            started_at,
            TreeInvalidation::None,
            Some(&initial.event_rebuild),
        );
        let initial_summary = warm.output.scene.summary();
        assert!(
            initial_summary.nodes >= 500
                && initial_summary.texts >= 100
                && initial_summary.paint_layers >= 6,
            "Borders hover benchmark selected the wrong scene: \
             target={target:?}, summary={initial_summary:?}"
        );
        let hover_ids = visible_hover_targets(&tree, target.width, target.height);
        assert!(
            hover_ids.len() >= 3,
            "Borders hover benchmark needs several visible hover targets: \
             target={target:?}, summary={:?}, hover_ids={hover_ids:?}",
            initial_summary
        );

        let cached_rebuild = if warm.output.event_rebuild_changed {
            warm.output.event_rebuild
        } else {
            initial.event_rebuild
        };
        #[cfg(feature = "bench-diagnostics")]
        if std::env::var_os("EMERGE_BENCH_DIAGNOSTICS").is_some() {
            sample_showcase_borders_hover_profile(
                tree.clone(),
                runtime.clone(),
                cached_rebuild.clone(),
                started_at,
                target,
                hover_ids[0],
            );
        }

        Self {
            tree,
            runtime,
            cached_rebuild,
            started_at,
            target,
            hover_ids,
            active_id: None,
            next_hover: 0,
            next_frame: 1,
            initial_summary,
        }
    }

    fn next_hover_frame(&mut self) -> emerge_skia::tree::layout::LayoutUpdateOutput {
        let next_id = self.hover_ids[self.next_hover % self.hover_ids.len()];
        self.next_hover = self.next_hover.wrapping_add(1);
        self.next_frame = self.next_frame.saturating_add(1);

        let mut dirty_ids = Vec::new();
        let mut invalidation = self
            .active_id
            .map(|id| {
                let invalidation = self.tree.set_mouse_over_active(&id, false);
                record_frame_attr_dirty_id(&mut dirty_ids, id, invalidation);
                invalidation
            })
            .unwrap_or(TreeInvalidation::None);
        let next_invalidation = self.tree.set_mouse_over_active(&next_id, true);
        record_frame_attr_dirty_id(&mut dirty_ids, next_id, next_invalidation);
        invalidation.add(next_invalidation);
        self.active_id = Some(next_id);

        let update = layout_or_refresh_default_with_animation_and_dirty_ids_reusing_clean_registry_for_benchmark(
            &mut self.tree,
            self.target.constraint(),
            self.target.scale,
            &self.runtime,
            self.started_at + Duration::from_millis(self.next_frame.saturating_mul(SHOWCASE_FRAME_MS)),
            invalidation,
            &dirty_ids,
            Some(&self.cached_rebuild),
        );

        if update.output.event_rebuild_changed {
            self.cached_rebuild = update.output.event_rebuild.clone();
        }

        update
    }
}

struct ShowcaseBordersHeldNearbyCase {
    tree: ElementTree,
    runtime: AnimationRuntime,
    cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    target: ShowcaseBordersTarget,
    next_frame: u64,
    initial_summary: RenderSceneSummary,
}

impl ShowcaseBordersHeldNearbyCase {
    fn new() -> Self {
        let started_at = Instant::now();
        let tree =
            decode_tree(EMERGE_DEMO_SHOWCASE_BORDERS_EMRG).expect("Borders fixture should decode");
        let mut runtime = AnimationRuntime::default();
        runtime.sync_with_tree(&tree, started_at);
        let target = ShowcaseBordersTarget::screenshot_fixture();
        let constraint = target.constraint();
        let mut tree = tree.clone();
        let initial = layout_and_refresh_default_with_animation(
            &mut tree,
            constraint,
            target.scale,
            &runtime,
            started_at,
        );
        tree.apply_scroll_y(&target.scroll_id, -target.scroll_y);
        let warm = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut tree,
            constraint,
            target.scale,
            &runtime,
            started_at,
            TreeInvalidation::None,
            Some(&initial.event_rebuild),
        );
        let mut cached_rebuild = if warm.output.event_rebuild_changed {
            warm.output.event_rebuild
        } else {
            initial.event_rebuild
        };

        let hover_ids = visible_hover_targets(&tree, target.width, target.height);
        let host_id = *hover_ids
            .first()
            .expect("held nearby benchmark needs one visible hover host");
        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::InsertNearbySubtree {
                host_id,
                index: 0,
                slot: NearbySlot::Above,
                subtree: nearby_code_block_subtree(920_000),
            }],
        )
        .expect("held nearby patch should apply");
        let mounted = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut tree,
            constraint,
            target.scale,
            &runtime,
            started_at + Duration::from_millis(SHOWCASE_FRAME_MS),
            invalidation,
            Some(&cached_rebuild),
        );
        cached_rebuild = if mounted.output.event_rebuild_changed {
            mounted.output.event_rebuild
        } else {
            cached_rebuild
        };
        let warm_held = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut tree,
            constraint,
            target.scale,
            &runtime,
            started_at + Duration::from_millis(SHOWCASE_FRAME_MS * 2),
            TreeInvalidation::None,
            Some(&cached_rebuild),
        );
        if warm_held.output.event_rebuild_changed {
            cached_rebuild = warm_held.output.event_rebuild;
        }

        let initial_summary = warm_held.output.scene.summary();
        assert!(
            initial_summary.nodes >= 500
                && initial_summary.texts >= 100
                && initial_summary.paint_layers >= 6,
            "Borders held-nearby benchmark selected the wrong scene: \
             target={target:?}, summary={initial_summary:?}"
        );

        Self {
            tree,
            runtime,
            cached_rebuild,
            started_at,
            target,
            next_frame: 3,
            initial_summary,
        }
    }

    fn next_frame(&mut self) -> emerge_skia::tree::layout::LayoutUpdateOutput {
        self.next_frame = self.next_frame.saturating_add(1);
        let update = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut self.tree,
            self.target.constraint(),
            self.target.scale,
            &self.runtime,
            self.started_at + Duration::from_millis(self.next_frame.saturating_mul(SHOWCASE_FRAME_MS)),
            TreeInvalidation::None,
            Some(&self.cached_rebuild),
        );

        if update.output.event_rebuild_changed {
            self.cached_rebuild = update.output.event_rebuild.clone();
        }

        update
    }
}

#[cfg(feature = "bench-diagnostics")]
fn sample_showcase_borders_hover_profile(
    tree: ElementTree,
    runtime: AnimationRuntime,
    cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    target: ShowcaseBordersTarget,
    hover_id: NodeId,
) {
    sample_showcase_borders_steady_profile(
        "no_hover",
        tree.clone(),
        runtime.clone(),
        cached_rebuild.clone(),
        started_at,
        target,
        TreeInvalidation::None,
    );

    let mut hovered_tree = tree;
    let hover_invalidation = hovered_tree.set_mouse_over_active(&hover_id, true);
    sample_showcase_borders_steady_profile(
        "hovered",
        hovered_tree,
        runtime,
        cached_rebuild,
        started_at,
        target,
        hover_invalidation,
    );
}

#[cfg(feature = "bench-diagnostics")]
fn sample_showcase_borders_steady_profile(
    label: &str,
    mut tree: ElementTree,
    runtime: AnimationRuntime,
    mut cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    target: ShowcaseBordersTarget,
    first_invalidation: TreeInvalidation,
) {
    for frame in 1..=4_u64 {
        let invalidation = if frame == 1 {
            first_invalidation
        } else {
            TreeInvalidation::None
        };
        let (update, profile) =
            layout_or_refresh_default_with_animation_and_invalidation_profile_for_benchmark(
                &mut tree,
                target.constraint(),
                target.scale,
                &runtime,
                started_at + Duration::from_millis(frame.saturating_mul(SHOWCASE_FRAME_MS)),
                invalidation,
                Some(&cached_rebuild),
            );
        if update.output.event_rebuild_changed {
            cached_rebuild = update.output.event_rebuild;
        }
        eprintln!(
            "showcase Borders {label} profile frame={frame} invalidation={:?} prepare={:.3}ms layout={:.3}ms refresh={:.3}ms traversal={:.3}ms registry_post={:.3}ms layout_performed={} event_rebuild_changed={} scene_nodes={} render_visits={} culled={} registry_visits={} registry_hits={} registry_stores={} registry_damaged={} registry_ineligible={} registry_misses={} pre_registry_damage={} registry_damage={} registry_damage_nodes={} summary={:?}",
            invalidation,
            profile.prepare.as_secs_f64() * 1000.0,
            profile.layout.as_secs_f64() * 1000.0,
            profile.refresh.as_secs_f64() * 1000.0,
            profile.refresh_traversal.as_secs_f64() * 1000.0,
            profile.refresh_registry_post.as_secs_f64() * 1000.0,
            profile.layout_performed,
            update.output.event_rebuild_changed,
            profile.scene_nodes,
            profile.render_visits,
            profile.culled_subtrees,
            profile.registry_visits,
            profile.registry_cache_hits,
            profile.registry_cache_stores,
            profile.registry_cache_damaged,
            profile.registry_cache_ineligible,
            profile.registry_cache_misses,
            profile.pre_layout_registry_damage,
            profile.registry_damage,
            profile.registry_damage_nodes,
            update.output.scene.summary()
        );
    }
}

struct ShowcaseInteractionVirtualKeyboardCase {
    tree: ElementTree,
    runtime: AnimationRuntime,
    cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    target: ShowcaseInteractionTarget,
    forward_patches: Vec<Patch>,
    reverse_patches: Vec<Patch>,
    next_forward: bool,
    next_frame: u64,
    initial_summary: RenderSceneSummary,
}

impl ShowcaseInteractionVirtualKeyboardCase {
    fn new() -> Self {
        let started_at = Instant::now();
        let tree = decode_tree(EMERGE_DEMO_SHOWCASE_INTERACTION_EMRG)
            .expect("Interaction fixture should decode");
        let mut runtime = AnimationRuntime::default();
        runtime.sync_with_tree(&tree, started_at);
        let constraint = Constraint::new(
            SHOWCASE_INTERACTION_WIDTH as f32,
            SHOWCASE_INTERACTION_HEIGHT as f32,
        );
        let mut tree = tree.clone();
        let initial = layout_and_refresh_default_with_animation(
            &mut tree, constraint, 1.0, &runtime, started_at,
        );
        let target = ShowcaseInteractionTarget::from_laid_out_tree(
            &tree,
            SHOWCASE_INTERACTION_WIDTH,
            SHOWCASE_INTERACTION_HEIGHT,
        );
        if std::env::var_os("EMERGE_BENCH_DIAGNOSTICS").is_some() {
            eprintln!("showcase Interaction virtual keyboard target: {target:?}");
            if let Some(element) = tree.get(&target.text_input_id) {
                eprintln!(
                    "showcase Interaction virtual keyboard input kind={:?} width={:?} height={:?} frame={:?}",
                    element.spec.kind,
                    element.layout.effective.width,
                    element.layout.effective.height,
                    element.layout.frame
                );
            }
        }

        let mut warm_invalidation = tree.apply_scroll_y(&target.scroll_id, -target.scroll_y);
        warm_invalidation.add(tree.set_text_input_runtime(
            &target.text_input_id,
            true,
            Some(SHOWCASE_INTERACTION_INITIAL_TEXT.chars().count() as u32),
            None,
            None,
            None,
        ));
        let warm =
            layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
                &mut tree,
                constraint,
                1.0,
                &runtime,
                started_at,
                warm_invalidation,
                Some(&initial.event_rebuild),
            );
        let initial_summary = warm.output.scene.summary();
        let virtual_key_count = virtual_key_count(&tree);
        assert!(
            initial_summary.nodes >= 700
                && initial_summary.texts >= 180
                && virtual_key_count >= 30
                && tree
                    .get(&target.text_input_id)
                    .and_then(|element| element.layout.frame)
                    .is_some(),
            "Interaction virtual keyboard benchmark selected the wrong scene: \
             target={target:?}, summary={initial_summary:?}, virtual_keys={virtual_key_count}"
        );

        let cached_rebuild = if warm.output.event_rebuild_changed {
            warm.output.event_rebuild
        } else {
            initial.event_rebuild
        };
        let forward_patches = decode_patches(EMERGE_DEMO_SHOWCASE_INTERACTION_VIRTUAL_KEY_PATCH)
            .expect("Interaction virtual-key forward patch should decode");
        let reverse_patches =
            decode_patches(EMERGE_DEMO_SHOWCASE_INTERACTION_VIRTUAL_KEY_REVERSE_PATCH)
                .expect("Interaction virtual-key reverse patch should decode");
        assert!(
            !forward_patches.is_empty() && !reverse_patches.is_empty(),
            "Interaction virtual-key benchmark patches should not be empty"
        );
        #[cfg(feature = "bench-diagnostics")]
        if std::env::var_os("EMERGE_BENCH_DIAGNOSTICS").is_some() {
            sample_showcase_interaction_profile(
                tree.clone(),
                runtime.clone(),
                cached_rebuild.clone(),
                started_at,
                target,
                forward_patches.clone(),
                reverse_patches.clone(),
            );
        }

        Self {
            tree,
            runtime,
            cached_rebuild,
            started_at,
            target,
            forward_patches,
            reverse_patches,
            next_forward: true,
            next_frame: 1,
            initial_summary,
        }
    }

    fn next_text_echo_frame(&mut self) -> emerge_skia::tree::layout::LayoutUpdateOutput {
        self.next_frame = self.next_frame.saturating_add(1);
        let (content, patches) = if self.next_forward {
            (SHOWCASE_INTERACTION_NEXT_TEXT, &self.forward_patches)
        } else {
            (SHOWCASE_INTERACTION_INITIAL_TEXT, &self.reverse_patches)
        };
        self.next_forward = !self.next_forward;

        let cursor = content.chars().count() as u32;
        let mut dirty_ids = Vec::new();
        let mut invalidation = self
            .tree
            .set_text_input_content(&self.target.text_input_id, content.to_string());
        record_frame_attr_dirty_id(&mut dirty_ids, self.target.text_input_id, invalidation);
        let runtime_invalidation = self.tree.set_text_input_runtime(
            &self.target.text_input_id,
            true,
            Some(cursor),
            None,
            None,
            None,
        );
        record_frame_attr_dirty_id(
            &mut dirty_ids,
            self.target.text_input_id,
            runtime_invalidation,
        );
        invalidation.add(runtime_invalidation);
        let patch_dirty_ids = patch_set_attrs_ids(patches);
        let patch_invalidation = apply_patches(&mut self.tree, patches.clone())
            .expect("Interaction virtual-key echo patch applies");
        if patch_invalidation.can_refresh_only()
            && let Some(ids) = patch_dirty_ids
        {
            extend_frame_attr_dirty_ids(&mut dirty_ids, ids);
        }
        invalidation.add(patch_invalidation);

        let update =
            layout_or_refresh_default_with_animation_and_dirty_ids_reusing_clean_registry_for_benchmark(
                &mut self.tree,
                Constraint::new(
                    SHOWCASE_INTERACTION_WIDTH as f32,
                    SHOWCASE_INTERACTION_HEIGHT as f32,
                ),
                1.0,
                &self.runtime,
                self.started_at
                    + Duration::from_millis(self.next_frame.saturating_mul(SHOWCASE_FRAME_MS)),
                invalidation,
                &dirty_ids,
                Some(&self.cached_rebuild),
            );

        if update.output.event_rebuild_changed {
            self.cached_rebuild = update.output.event_rebuild.clone();
        }

        update
    }
}

struct ShowcaseInteractionVirtualKeyFullLoopCase {
    tree: ElementTree,
    runtime: AnimationRuntime,
    cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    target: ShowcaseInteractionTarget,
    virtual_key_id: NodeId,
    hover_id: Option<NodeId>,
    forward_patches: Vec<Patch>,
    reverse_patches: Vec<Patch>,
    next_forward: bool,
    next_frame: u64,
    phase: u8,
    initial_summary: RenderSceneSummary,
}

impl ShowcaseInteractionVirtualKeyFullLoopCase {
    fn new() -> Self {
        let started_at = Instant::now();
        let tree = decode_tree(EMERGE_DEMO_SHOWCASE_INTERACTION_EMRG)
            .expect("Interaction fixture should decode");
        let mut runtime = AnimationRuntime::default();
        runtime.sync_with_tree(&tree, started_at);
        let constraint = Constraint::new(
            SHOWCASE_INTERACTION_WIDTH as f32,
            SHOWCASE_INTERACTION_HEIGHT as f32,
        );
        let mut tree = tree.clone();
        let initial = layout_and_refresh_default_with_animation(
            &mut tree, constraint, 1.0, &runtime, started_at,
        );
        let target = ShowcaseInteractionTarget::from_laid_out_tree(
            &tree,
            SHOWCASE_INTERACTION_WIDTH,
            SHOWCASE_INTERACTION_HEIGHT,
        );
        let mut warm_invalidation = tree.apply_scroll_y(&target.scroll_id, -target.scroll_y);
        warm_invalidation.add(tree.set_text_input_runtime(
            &target.text_input_id,
            true,
            Some(SHOWCASE_INTERACTION_INITIAL_TEXT.chars().count() as u32),
            None,
            None,
            None,
        ));
        let warm =
            layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
                &mut tree,
                constraint,
                1.0,
                &runtime,
                started_at,
                warm_invalidation,
                Some(&initial.event_rebuild),
            );
        let initial_summary = warm.output.scene.summary();
        let virtual_key_id = visible_virtual_key_id(
            &tree,
            SHOWCASE_INTERACTION_WIDTH,
            SHOWCASE_INTERACTION_HEIGHT,
            target.scroll_y,
        )
        .expect("Interaction full-loop benchmark should have a visible virtual key");
        let hover_id = keyboard_hover_target_id(&tree, virtual_key_id);
        let cached_rebuild = if warm.output.event_rebuild_changed {
            warm.output.event_rebuild
        } else {
            initial.event_rebuild
        };
        let forward_patches = decode_patches(EMERGE_DEMO_SHOWCASE_INTERACTION_VIRTUAL_KEY_PATCH)
            .expect("Interaction virtual-key forward patch should decode");
        let reverse_patches =
            decode_patches(EMERGE_DEMO_SHOWCASE_INTERACTION_VIRTUAL_KEY_REVERSE_PATCH)
                .expect("Interaction virtual-key reverse patch should decode");

        if std::env::var_os("EMERGE_BENCH_DIAGNOSTICS").is_some() {
            eprintln!(
                "showcase Interaction virtual-key full-loop target: {target:?}, virtual_key={virtual_key_id:?}, hover={hover_id:?}, summary={initial_summary:?}"
            );
            #[cfg(feature = "bench-diagnostics")]
            sample_showcase_interaction_virtual_key_full_loop_profile(
                tree.clone(),
                runtime.clone(),
                cached_rebuild.clone(),
                started_at,
                target,
                virtual_key_id,
                hover_id,
                forward_patches.clone(),
                reverse_patches.clone(),
            );
        }

        Self {
            tree,
            runtime,
            cached_rebuild,
            started_at,
            target,
            virtual_key_id,
            hover_id,
            forward_patches,
            reverse_patches,
            next_forward: true,
            next_frame: 1,
            phase: 0,
            initial_summary,
        }
    }

    fn next_frame(&mut self) -> emerge_skia::tree::layout::LayoutUpdateOutput {
        self.next_frame = self.next_frame.saturating_add(1);
        let mut dirty_ids = Vec::new();
        let invalidation = match self.phase {
            0 => self.hover_id.map_or(TreeInvalidation::None, |id| {
                let invalidation = self.tree.set_mouse_over_active(&id, true);
                record_frame_attr_dirty_id(&mut dirty_ids, id, invalidation);
                invalidation
            }),
            1 => {
                let invalidation = self.tree.set_mouse_down_active(&self.virtual_key_id, true);
                record_frame_attr_dirty_id(&mut dirty_ids, self.virtual_key_id, invalidation);
                invalidation
            }
            2 => {
                let mut invalidation = self.tree.set_mouse_down_active(&self.virtual_key_id, false);
                record_frame_attr_dirty_id(&mut dirty_ids, self.virtual_key_id, invalidation);
                let (content, patches) = if self.next_forward {
                    (SHOWCASE_INTERACTION_NEXT_TEXT, &self.forward_patches)
                } else {
                    (SHOWCASE_INTERACTION_INITIAL_TEXT, &self.reverse_patches)
                };
                self.next_forward = !self.next_forward;
                let cursor = content.chars().count() as u32;
                let content_invalidation = self
                    .tree
                    .set_text_input_content(&self.target.text_input_id, content.to_string());
                record_frame_attr_dirty_id(
                    &mut dirty_ids,
                    self.target.text_input_id,
                    content_invalidation,
                );
                invalidation.add(content_invalidation);
                let runtime_invalidation = self.tree.set_text_input_runtime(
                    &self.target.text_input_id,
                    true,
                    Some(cursor),
                    None,
                    None,
                    None,
                );
                record_frame_attr_dirty_id(
                    &mut dirty_ids,
                    self.target.text_input_id,
                    runtime_invalidation,
                );
                invalidation.add(runtime_invalidation);
                let patch_dirty_ids = patch_set_attrs_ids(patches);
                let patch_invalidation = apply_patches(&mut self.tree, patches.clone())
                    .expect("Interaction virtual-key echo patch applies");
                if patch_invalidation.can_refresh_only()
                    && let Some(ids) = patch_dirty_ids
                {
                    extend_frame_attr_dirty_ids(&mut dirty_ids, ids);
                }
                invalidation.add(patch_invalidation);
                invalidation
            }
            _ => self.hover_id.map_or(TreeInvalidation::None, |id| {
                let invalidation = self.tree.set_mouse_over_active(&id, false);
                record_frame_attr_dirty_id(&mut dirty_ids, id, invalidation);
                invalidation
            }),
        };
        self.phase = (self.phase + 1) % 4;

        let update =
            layout_or_refresh_default_with_animation_and_dirty_ids_reusing_clean_registry_for_benchmark(
                &mut self.tree,
                Constraint::new(
                    SHOWCASE_INTERACTION_WIDTH as f32,
                    SHOWCASE_INTERACTION_HEIGHT as f32,
                ),
                1.0,
                &self.runtime,
                self.started_at
                    + Duration::from_millis(self.next_frame.saturating_mul(SHOWCASE_FRAME_MS)),
                invalidation,
                &dirty_ids,
                Some(&self.cached_rebuild),
            );

        if update.output.event_rebuild_changed {
            self.cached_rebuild = update.output.event_rebuild.clone();
        }

        update
    }
}

struct ShowcaseInteractionScrollCase {
    tree: ElementTree,
    runtime: AnimationRuntime,
    cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    target: ShowcaseInteractionTarget,
    next_frame: u64,
    scroll_forward: bool,
    initial_summary: RenderSceneSummary,
}

impl ShowcaseInteractionScrollCase {
    fn new() -> Self {
        let started_at = Instant::now();
        let tree = decode_tree(EMERGE_DEMO_SHOWCASE_INTERACTION_EMRG)
            .expect("Interaction fixture should decode");
        let mut runtime = AnimationRuntime::default();
        runtime.sync_with_tree(&tree, started_at);
        let constraint = Constraint::new(
            SHOWCASE_INTERACTION_WIDTH as f32,
            SHOWCASE_INTERACTION_HEIGHT as f32,
        );
        let mut tree = tree.clone();
        let initial = layout_and_refresh_default_with_animation(
            &mut tree, constraint, 1.0, &runtime, started_at,
        );
        let target = ShowcaseInteractionTarget::from_laid_out_tree(
            &tree,
            SHOWCASE_INTERACTION_WIDTH,
            SHOWCASE_INTERACTION_HEIGHT,
        );
        let warm_invalidation = tree.apply_scroll_y(&target.scroll_id, -target.scroll_y);
        let warm =
            layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
                &mut tree,
                constraint,
                1.0,
                &runtime,
                started_at,
                warm_invalidation,
                Some(&initial.event_rebuild),
        );
        let initial_summary = warm.output.scene.summary();
        if std::env::var_os("EMERGE_BENCH_DIAGNOSTICS").is_some() {
            eprintln!(
                "showcase Interaction scroll target: {target:?}, summary={initial_summary:?}"
            );
            print_showcase_paint_layers(&warm.output.scene.nodes, 0);
            #[cfg(feature = "bench-diagnostics")]
            {
                sample_showcase_interaction_scroll_profile(
                    tree.clone(),
                    runtime.clone(),
                    if warm.output.event_rebuild_changed {
                        warm.output.event_rebuild.clone()
                    } else {
                        initial.event_rebuild.clone()
                    },
                    started_at,
                    target,
                );
            }
        }
        assert!(
            initial_summary.nodes >= 700 && initial_summary.paint_layers >= 3,
            "Interaction scroll benchmark selected the wrong scene: \
             target={target:?}, summary={initial_summary:?}"
        );

        Self {
            tree,
            runtime,
            cached_rebuild: if warm.output.event_rebuild_changed {
                warm.output.event_rebuild
            } else {
                initial.event_rebuild
            },
            started_at,
            target,
            next_frame: 1,
            scroll_forward: true,
            initial_summary,
        }
    }

    fn next_scroll_frame(&mut self) -> emerge_skia::tree::layout::LayoutUpdateOutput {
        self.next_frame = self.next_frame.saturating_add(1);
        let delta = if self.scroll_forward { -24.0 } else { 24.0 };
        self.scroll_forward = !self.scroll_forward;
        let invalidation = self.tree.apply_scroll_y(&self.target.scroll_id, delta);

        let update =
            layout_or_refresh_default_with_animation_and_dirty_ids_reusing_clean_registry_for_benchmark(
                &mut self.tree,
                Constraint::new(
                    SHOWCASE_INTERACTION_WIDTH as f32,
                    SHOWCASE_INTERACTION_HEIGHT as f32,
                ),
                1.0,
                &self.runtime,
                self.started_at
                    + Duration::from_millis(self.next_frame.saturating_mul(SHOWCASE_FRAME_MS)),
                invalidation,
                &[],
                Some(&self.cached_rebuild),
            );

        if update.output.event_rebuild_changed {
            self.cached_rebuild = update.output.event_rebuild.clone();
        }

        update
    }
}

#[cfg(feature = "bench-diagnostics")]
fn sample_showcase_interaction_scroll_profile(
    mut tree: ElementTree,
    runtime: AnimationRuntime,
    mut cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    target: ShowcaseInteractionTarget,
) {
    let mut scroll_forward = true;
    for frame in 1..=6_u64 {
        let delta = if scroll_forward { -24.0 } else { 24.0 };
        scroll_forward = !scroll_forward;
        let invalidation = tree.apply_scroll_y(&target.scroll_id, delta);
        let (update, profile) =
            layout_or_refresh_default_with_animation_and_invalidation_profile_for_benchmark(
                &mut tree,
                Constraint::new(
                    SHOWCASE_INTERACTION_WIDTH as f32,
                    SHOWCASE_INTERACTION_HEIGHT as f32,
                ),
                1.0,
                &runtime,
                started_at + Duration::from_millis(frame.saturating_mul(SHOWCASE_FRAME_MS)),
                invalidation,
                Some(&cached_rebuild),
            );
        if update.output.event_rebuild_changed {
            cached_rebuild = update.output.event_rebuild;
        }
        eprintln!(
            "showcase interaction scroll profile frame={frame} invalidation={:?} prepare={:.3}ms layout={:.3}ms refresh={:.3}ms traversal={:.3}ms registry_post={:.3}ms layout_performed={} event_rebuild_changed={} scene_nodes={} render_visits={} culled={} registry_visits={} registry_hits={} registry_stores={} registry_damaged={} registry_ineligible={} registry_misses={} pre_registry_damage={} registry_damage={} registry_damage_nodes={} summary={:?}",
            invalidation,
            profile.prepare.as_secs_f64() * 1000.0,
            profile.layout.as_secs_f64() * 1000.0,
            profile.refresh.as_secs_f64() * 1000.0,
            profile.refresh_traversal.as_secs_f64() * 1000.0,
            profile.refresh_registry_post.as_secs_f64() * 1000.0,
            profile.layout_performed,
            update.output.event_rebuild_changed,
            profile.scene_nodes,
            profile.render_visits,
            profile.culled_subtrees,
            profile.registry_visits,
            profile.registry_cache_hits,
            profile.registry_cache_stores,
            profile.registry_cache_damaged,
            profile.registry_cache_ineligible,
            profile.registry_cache_misses,
            profile.pre_layout_registry_damage,
            profile.registry_damage,
            profile.registry_damage_nodes,
            update.output.scene.summary()
        );
    }
}

#[cfg(feature = "bench-diagnostics")]
fn sample_showcase_interaction_profile(
    mut tree: ElementTree,
    runtime: AnimationRuntime,
    mut cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    target: ShowcaseInteractionTarget,
    forward_patches: Vec<Patch>,
    reverse_patches: Vec<Patch>,
) {
    for frame in 1..=6_u64 {
        let (content, patches) = if frame % 2 == 1 {
            (SHOWCASE_INTERACTION_NEXT_TEXT, &forward_patches)
        } else {
            (SHOWCASE_INTERACTION_INITIAL_TEXT, &reverse_patches)
        };
        let cursor = content.chars().count() as u32;
        let mut invalidation = tree.set_text_input_content(&target.text_input_id, content.into());
        invalidation.add(tree.set_text_input_runtime(
            &target.text_input_id,
            true,
            Some(cursor),
            None,
            None,
            None,
        ));
        if frame == 1 {
            print_showcase_patch_summary(&tree, patches);
        }
        invalidation.add(
            apply_patches(&mut tree, patches.clone())
                .expect("Interaction virtual-key echo patch applies"),
        );

        let (update, profile) =
            layout_or_refresh_default_with_animation_and_invalidation_profile_for_benchmark(
                &mut tree,
                Constraint::new(
                    SHOWCASE_INTERACTION_WIDTH as f32,
                    SHOWCASE_INTERACTION_HEIGHT as f32,
                ),
                1.0,
                &runtime,
                started_at + Duration::from_millis(frame.saturating_mul(SHOWCASE_FRAME_MS)),
                invalidation,
                Some(&cached_rebuild),
            );
        if update.output.event_rebuild_changed {
            cached_rebuild = update.output.event_rebuild;
        }
        eprintln!(
            "showcase interaction profile frame={frame} invalidation={:?} prepare={:.3}ms layout={:.3}ms refresh={:.3}ms traversal={:.3}ms registry_post={:.3}ms layout_performed={} event_rebuild_changed={} scene_nodes={} render_visits={} culled={} registry_visits={} registry_hits={} registry_stores={} registry_damaged={} registry_ineligible={} registry_misses={} pre_registry_damage={} registry_damage={} registry_damage_nodes={}",
            invalidation,
            profile.prepare.as_secs_f64() * 1000.0,
            profile.layout.as_secs_f64() * 1000.0,
            profile.refresh.as_secs_f64() * 1000.0,
            profile.refresh_traversal.as_secs_f64() * 1000.0,
            profile.refresh_registry_post.as_secs_f64() * 1000.0,
            profile.layout_performed,
            update.output.event_rebuild_changed,
            profile.scene_nodes,
            profile.render_visits,
            profile.culled_subtrees,
            profile.registry_visits,
            profile.registry_cache_hits,
            profile.registry_cache_stores,
            profile.registry_cache_damaged,
            profile.registry_cache_ineligible,
            profile.registry_cache_misses,
            profile.pre_layout_registry_damage,
            profile.registry_damage,
            profile.registry_damage_nodes
        );
        if frame == 1 {
            eprintln!(
                "showcase interaction scene summary: {:?}",
                update.output.scene.summary()
            );
            print_showcase_paint_layers(&update.output.scene.nodes, 0);
        }
    }
}

#[cfg(feature = "bench-diagnostics")]
struct VirtualKeyFullLoopProfileState {
    tree: ElementTree,
    runtime: AnimationRuntime,
    cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    target: ShowcaseInteractionTarget,
    virtual_key_id: NodeId,
    hover_id: Option<NodeId>,
    forward_patches: Vec<Patch>,
    reverse_patches: Vec<Patch>,
    next_forward: bool,
    phase: u8,
}

#[cfg(feature = "bench-diagnostics")]
#[allow(clippy::too_many_arguments)]
fn sample_showcase_interaction_virtual_key_full_loop_profile(
    tree: ElementTree,
    runtime: AnimationRuntime,
    cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    target: ShowcaseInteractionTarget,
    virtual_key_id: NodeId,
    hover_id: Option<NodeId>,
    forward_patches: Vec<Patch>,
    reverse_patches: Vec<Patch>,
) {
    let mut state = VirtualKeyFullLoopProfileState {
        tree,
        runtime,
        cached_rebuild,
        started_at,
        target,
        virtual_key_id,
        hover_id,
        forward_patches,
        reverse_patches,
        next_forward: true,
        phase: 0,
    };

    for frame in 1..=8_u64 {
        let phase_name = match state.phase {
            0 => "hover_on",
            1 => "key_down",
            2 => "key_release_echo",
            _ => "hover_off",
        };
        let invalidation = virtual_key_full_loop_invalidation(&mut state);
        let (update, profile) =
            layout_or_refresh_default_with_animation_and_invalidation_profile_for_benchmark(
                &mut state.tree,
                Constraint::new(
                    SHOWCASE_INTERACTION_WIDTH as f32,
                    SHOWCASE_INTERACTION_HEIGHT as f32,
                ),
                1.0,
                &state.runtime,
                state.started_at + Duration::from_millis(frame.saturating_mul(SHOWCASE_FRAME_MS)),
                invalidation,
                Some(&state.cached_rebuild),
            );
        if update.output.event_rebuild_changed {
            state.cached_rebuild = update.output.event_rebuild;
        }
        eprintln!(
            "showcase interaction virtual-key profile frame={frame} phase={phase_name} invalidation={:?} prepare={:.3}ms layout={:.3}ms refresh={:.3}ms traversal={:.3}ms registry_post={:.3}ms layout_performed={} event_rebuild_changed={} scene_nodes={} render_visits={} culled={} registry_visits={} registry_hits={} registry_stores={} registry_damaged={} registry_ineligible={} registry_misses={} registry_damage={} registry_damage_nodes={} summary={:?}",
            invalidation,
            profile.prepare.as_secs_f64() * 1000.0,
            profile.layout.as_secs_f64() * 1000.0,
            profile.refresh.as_secs_f64() * 1000.0,
            profile.refresh_traversal.as_secs_f64() * 1000.0,
            profile.refresh_registry_post.as_secs_f64() * 1000.0,
            profile.layout_performed,
            update.output.event_rebuild_changed,
            profile.scene_nodes,
            profile.render_visits,
            profile.culled_subtrees,
            profile.registry_visits,
            profile.registry_cache_hits,
            profile.registry_cache_stores,
            profile.registry_cache_damaged,
            profile.registry_cache_ineligible,
            profile.registry_cache_misses,
            profile.registry_damage,
            profile.registry_damage_nodes,
            update.output.scene.summary()
        );
    }
}

#[cfg(feature = "bench-diagnostics")]
fn virtual_key_full_loop_invalidation(
    state: &mut VirtualKeyFullLoopProfileState,
) -> TreeInvalidation {
    let invalidation = match state.phase {
        0 => state
            .hover_id
            .map(|id| state.tree.set_mouse_over_active(&id, true))
            .unwrap_or(TreeInvalidation::None),
        1 => state
            .tree
            .set_mouse_down_active(&state.virtual_key_id, true),
        2 => {
            let mut invalidation = state
                .tree
                .set_mouse_down_active(&state.virtual_key_id, false);
            let (content, patches) = if state.next_forward {
                (SHOWCASE_INTERACTION_NEXT_TEXT, &state.forward_patches)
            } else {
                (SHOWCASE_INTERACTION_INITIAL_TEXT, &state.reverse_patches)
            };
            state.next_forward = !state.next_forward;
            let cursor = content.chars().count() as u32;
            invalidation.add(
                state
                    .tree
                    .set_text_input_content(&state.target.text_input_id, content.to_string()),
            );
            invalidation.add(state.tree.set_text_input_runtime(
                &state.target.text_input_id,
                true,
                Some(cursor),
                None,
                None,
                None,
            ));
            invalidation.add(
                apply_patches(&mut state.tree, patches.clone())
                    .expect("Interaction virtual-key echo patch applies"),
            );
            invalidation
        }
        _ => state
            .hover_id
            .map(|id| state.tree.set_mouse_over_active(&id, false))
            .unwrap_or(TreeInvalidation::None),
    };
    state.phase = (state.phase + 1) % 4;
    invalidation
}

#[cfg(feature = "bench-diagnostics")]
fn print_showcase_patch_summary(tree: &ElementTree, patches: &[Patch]) {
    patches.iter().for_each(|patch| match patch {
        Patch::SetAttrs { id, attrs_raw } => {
            match emerge_skia::tree::attrs::decode_attrs(attrs_raw) {
                Ok(attrs) => eprintln!(
                    "showcase patch SetAttrs id={id:?} kind={:?} frame={:?} parent={:?} content={:?} width={:?} height={:?} focused={:?} mouse_down={:?}",
                    tree.get(id).map(|element| element.spec.kind),
                    tree.get(id).and_then(|element| element.layout.frame),
                    patch_target_parent_summary(tree, id),
                    attrs.content,
                    attrs.width,
                    attrs.height,
                    attrs.focused.as_ref().map(|_| true),
                    attrs.mouse_down.as_ref().map(|_| true)
                ),
                Err(err) => eprintln!("showcase patch SetAttrs id={id:?} decode error={err}"),
            }
        }
        Patch::SetChildren { id, children } => {
            eprintln!(
                "showcase patch SetChildren id={id:?} child_count={}",
                children.len()
            );
        }
        Patch::SetNearbyMounts { host_id, mounts } => {
            eprintln!(
                "showcase patch SetNearbyMounts host={host_id:?} count={}",
                mounts.len()
            );
        }
        Patch::InsertSubtree {
            parent_id,
            index,
            subtree,
        } => {
            eprintln!(
                "showcase patch InsertSubtree parent={parent_id:?} index={index} nodes={}",
                subtree.len()
            );
        }
        Patch::InsertNearbySubtree {
            host_id,
            index,
            slot,
            subtree,
        } => {
            eprintln!(
                "showcase patch InsertNearbySubtree host={host_id:?} index={index} slot={slot:?} nodes={}",
                subtree.len()
            );
        }
        Patch::Remove { id } => {
            eprintln!("showcase patch Remove id={id:?}");
        }
    });
}

#[cfg(feature = "bench-diagnostics")]
#[allow(clippy::type_complexity)]
fn patch_target_parent_summary(
    tree: &ElementTree,
    id: &NodeId,
) -> Option<(
    NodeId,
    ElementKind,
    Option<Length>,
    Option<Length>,
    Option<Frame>,
)> {
    let parent_ix =
        tree.ix_of(id)
            .and_then(|ix| tree.parent_link_of(ix))
            .map(|link| match link {
                emerge_skia::tree::element::ParentLink::Child { parent }
                | emerge_skia::tree::element::ParentLink::Nearby { host: parent, .. } => parent,
            })?;
    tree.get_ix(parent_ix).map(|element| {
        (
            element.id,
            element.spec.kind,
            element.layout.effective.width.clone(),
            element.layout.effective.height.clone(),
            element.layout.frame,
        )
    })
}

#[derive(Clone, Copy, Debug)]
struct ShowcaseLayoutTarget {
    width: u32,
    height: u32,
    scroll_id: NodeId,
    scroll_y: f32,
}

#[derive(Clone, Copy, Debug)]
struct ShowcaseBordersTarget {
    width: u32,
    height: u32,
    scale: f32,
    scroll_id: NodeId,
    scroll_y: f32,
}

#[derive(Clone, Copy, Debug)]
struct ShowcaseInteractionTarget {
    scroll_id: NodeId,
    scroll_y: f32,
    text_input_id: NodeId,
}

impl ShowcaseInteractionTarget {
    fn from_laid_out_tree(tree: &ElementTree, width: u32, height: u32) -> Self {
        let (scroll_id, scroll_y_max) = tree
            .iter_node_pairs()
            .filter_map(|(id, element)| {
                (element.layout.effective.scrollbar_y == Some(true)
                    && element.layout.scroll_y_max > 0.0)
                    .then_some((id, element.layout.scroll_y_max))
            })
            .max_by(|(_, left), (_, right)| left.total_cmp(right))
            .expect("Interaction benchmark should have a vertical scroll container");

        let keyboard_min_y = tree
            .iter_node_pairs()
            .filter_map(|(_, element)| {
                element
                    .layout
                    .effective
                    .virtual_key
                    .is_some()
                    .then_some(element.layout.frame?)
            })
            .map(|frame| frame.y)
            .min_by(f32::total_cmp)
            .expect("Interaction benchmark should have virtual keys");

        let (text_input_id, text_input_frame) = tree
            .iter_node_pairs()
            .filter_map(|(id, element)| {
                let frame = element.layout.frame?;
                (element.spec.kind.is_text_input_family() && frame.y <= keyboard_min_y)
                    .then_some((id, frame))
            })
            .max_by(|(_, left), (_, right)| left.y.total_cmp(&right.y))
            .expect("Interaction benchmark should have a text input before virtual keys");

        let scroll_y = (text_input_frame.y - height as f32 * 0.20)
            .max(0.0)
            .min(scroll_y_max);
        let _ = width;

        Self {
            scroll_id,
            scroll_y,
            text_input_id,
        }
    }
}

impl ShowcaseBordersTarget {
    fn screenshot_fixture() -> Self {
        Self {
            width: SHOWCASE_BORDERS_SCREENSHOT_WIDTH,
            height: SHOWCASE_BORDERS_SCREENSHOT_HEIGHT,
            scale: SHOWCASE_BORDERS_SCREENSHOT_SCALE,
            scroll_id: SHOWCASE_SCROLL_ID,
            scroll_y: SHOWCASE_BORDERS_SCREENSHOT_SCROLL_Y,
        }
    }

    fn constraint(self) -> Constraint {
        Constraint::new(self.width as f32, self.height as f32)
    }
}

impl ShowcaseLayoutTarget {
    fn visible_animation_fixture() -> Self {
        Self {
            width: SHOWCASE_LAYOUT_VISIBLE_WIDTH,
            height: SHOWCASE_LAYOUT_VISIBLE_HEIGHT,
            scroll_id: SHOWCASE_SCROLL_ID,
            scroll_y: SHOWCASE_LAYOUT_VISIBLE_SCROLL_Y,
        }
    }
}

fn visible_hover_targets(tree: &ElementTree, width: u32, height: u32) -> Vec<NodeId> {
    tree.iter_node_pairs()
        .filter_map(|(id, element)| {
            let frame = element.layout.frame?;
            (element.layout.effective.mouse_over.is_some()
                && frame.width > 8.0
                && frame.height > 8.0
                && frame_intersects_viewport(frame, width as f32, height as f32))
            .then_some(id)
        })
        .collect()
}

fn virtual_key_count(tree: &ElementTree) -> usize {
    tree.iter_node_pairs()
        .filter(|(_, element)| element.layout.effective.virtual_key.is_some())
        .count()
}

fn visible_virtual_key_id(
    tree: &ElementTree,
    width: u32,
    height: u32,
    scroll_y: f32,
) -> Option<NodeId> {
    tree.iter_node_pairs()
        .filter_map(|(id, element)| {
            let frame = element.layout.frame?;
            (element.layout.effective.virtual_key.is_some()
                && scrolled_frame_intersects_viewport(frame, width as f32, height as f32, scroll_y))
            .then_some((id, frame))
        })
        .min_by(|(_, left), (_, right)| {
            left.y
                .total_cmp(&right.y)
                .then_with(|| left.x.total_cmp(&right.x))
        })
        .map(|(id, _)| id)
}

fn scrolled_frame_intersects_viewport(
    frame: Frame,
    width: f32,
    height: f32,
    scroll_y: f32,
) -> bool {
    frame.x < width
        && frame.y - scroll_y < height
        && frame.x + frame.width > 0.0
        && frame.y - scroll_y + frame.height > 0.0
}

fn keyboard_hover_target_id(tree: &ElementTree, virtual_key_id: NodeId) -> Option<NodeId> {
    let key_frame = tree
        .get(&virtual_key_id)
        .and_then(|element| element.layout.frame)?;
    let key_center_x = key_frame.x + key_frame.width * 0.5;
    let key_center_y = key_frame.y + key_frame.height * 0.5;

    if tree
        .get(&virtual_key_id)
        .is_some_and(|element| element.layout.effective.mouse_over.is_some())
    {
        return Some(virtual_key_id);
    }

    tree.iter_node_pairs()
        .filter_map(|(id, element)| {
            let frame = element.layout.frame?;
            (element.layout.effective.mouse_over.is_some()
                && frame.x <= key_center_x
                && frame.x + frame.width >= key_center_x
                && frame.y <= key_center_y
                && frame.y + frame.height >= key_center_y)
                .then_some((id, frame.width * frame.height))
        })
        .min_by(|(_, left_area), (_, right_area)| left_area.total_cmp(right_area))
        .map(|(id, _)| id)
}

fn frame_intersects_viewport(frame: Frame, width: f32, height: f32) -> bool {
    frame.x < width
        && frame.y < height
        && frame.x + frame.width > 0.0
        && frame.y + frame.height > 0.0
}

fn bench_scrolling_animation_paint_only_showcase(
    c: &mut Criterion,
    group_name: &str,
    constraint: Constraint,
    make_tree: fn() -> ElementTree,
) {
    let mut group = c.benchmark_group(group_name);
    let start = Instant::now();
    let node_count = make_tree().len() as u64;
    group.throughput(Throughput::Elements(node_count));

    let mut full_tree = make_tree();
    let full_root_id = full_tree.root_id().expect("scroll tree should have root");
    let mut full_runtime = AnimationRuntime::default();
    full_runtime.sync_with_tree(&full_tree, start);
    layout_and_refresh_default_with_animation(
        &mut full_tree,
        constraint,
        1.0,
        &full_runtime,
        start,
    );
    let mut full_tick = 0_u64;
    group.bench_function("full_layout_plus_refresh_scroll_frame", |b| {
        b.iter(|| {
            full_tick += 16;
            let delta = if full_tick.is_multiple_of(32) {
                8.0
            } else {
                -8.0
            };
            black_box(full_tree.apply_scroll_y(&full_root_id, delta));
            let output = layout_and_refresh_default_with_animation(
                &mut full_tree,
                constraint,
                1.0,
                &full_runtime,
                start + Duration::from_millis(full_tick),
            );
            black_box((
                output.scene.nodes.len(),
                output.event_rebuild.text_inputs.len(),
                true,
            ))
        });
    });

    let mut refresh_tree = make_tree();
    let refresh_root_id = refresh_tree
        .root_id()
        .expect("scroll tree should have root");
    let mut refresh_runtime = AnimationRuntime::default();
    refresh_runtime.sync_with_tree(&refresh_tree, start);
    layout_and_refresh_default_with_animation(
        &mut refresh_tree,
        constraint,
        1.0,
        &refresh_runtime,
        start,
    );
    let mut refresh_tick = 0_u64;
    group.bench_function("paint_only_refresh_scroll_frame", |b| {
        b.iter(|| {
            refresh_tick += 16;
            let delta = if refresh_tick.is_multiple_of(32) {
                8.0
            } else {
                -8.0
            };
            black_box(refresh_tree.apply_scroll_y(&refresh_root_id, delta));
            let update = layout_or_refresh_default_with_animation(
                &mut refresh_tree,
                constraint,
                1.0,
                &refresh_runtime,
                start + Duration::from_millis(refresh_tick),
            );
            black_box((
                update.output.scene.nodes.len(),
                update.output.event_rebuild.text_inputs.len(),
                update.layout_performed,
            ))
        });
    });

    group.finish();
}

fn bench_scroll_viewport_culling(c: &mut Criterion) {
    let mut group = c.benchmark_group("native/scroll_viewport_culling");
    let constraint = Constraint::new(900.0, 640.0);
    let cases = [
        ScrollViewportCase {
            name: "large_column_simple_rows",
            make_tree: simple_scroll_viewport_tree,
        },
        ScrollViewportCase {
            name: "large_column_paint_rich_rows",
            make_tree: paint_rich_scroll_viewport_tree,
        },
    ];

    for case in cases {
        let node_count = (case.make_tree)().len() as u64;
        group.throughput(Throughput::Elements(node_count));
        log_scroll_viewport_case_diagnostics(case, constraint);
        bench_scroll_viewport_case(&mut group, case, constraint);
    }

    group.finish();
}

#[derive(Clone, Copy)]
struct ScrollViewportCase {
    name: &'static str,
    make_tree: fn() -> ElementTree,
}

#[derive(Clone, Copy)]
enum ScrollViewportPosition {
    Top,
    Middle,
}

fn simple_scroll_viewport_tree() -> ElementTree {
    large_simple_scroll_column(SCROLL_VIEWPORT_ROW_COUNT)
}

fn paint_rich_scroll_viewport_tree() -> ElementTree {
    large_paint_rich_scroll_column(SCROLL_VIEWPORT_ROW_COUNT)
}

fn bench_scroll_viewport_case(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    case: ScrollViewportCase,
    constraint: Constraint,
) {
    let (mut top_tree, top_cached_rebuild, _) =
        prepare_scroll_viewport_tree(case.make_tree, constraint, ScrollViewportPosition::Top);
    group.bench_function(format!("{}/top_cached_refresh", case.name), move |b| {
        b.iter(|| {
            let output = refresh_reusing_clean_registry_for_benchmark(
                &mut top_tree,
                Some(&top_cached_rebuild),
            );
            consume_layout_output(output)
        });
    });

    let (mut middle_tree, middle_cached_rebuild, _) =
        prepare_scroll_viewport_tree(case.make_tree, constraint, ScrollViewportPosition::Middle);
    group.bench_function(format!("{}/middle_cached_refresh", case.name), move |b| {
        b.iter(|| {
            let output = refresh_reusing_clean_registry_for_benchmark(
                &mut middle_tree,
                Some(&middle_cached_rebuild),
            );
            consume_layout_output(output)
        });
    });

    let (mut render_only_middle_tree, _, _) =
        prepare_scroll_viewport_tree(case.make_tree, constraint, ScrollViewportPosition::Middle);
    group.bench_function(format!("{}/middle_render_only", case.name), move |b| {
        b.iter(|| {
            let scene = refresh_render_scene_for_benchmark(&mut render_only_middle_tree);
            consume_render_scene(scene)
        });
    });

    let (mut step_tree, mut step_cached_rebuild, step_root_id) =
        prepare_scroll_viewport_tree(case.make_tree, constraint, ScrollViewportPosition::Middle);
    let mut step_tick = 0_u64;
    group.bench_function(
        format!("{}/scroll_step_cached_refresh", case.name),
        move |b| {
            b.iter(|| {
                step_tick = step_tick.saturating_add(1);
                let delta = if step_tick.is_multiple_of(2) {
                    -24.0
                } else {
                    24.0
                };
                black_box(step_tree.apply_scroll_y(&step_root_id, delta));
                let output = refresh_reusing_clean_registry_for_benchmark(
                    &mut step_tree,
                    Some(&step_cached_rebuild),
                );
                if output.event_rebuild_changed {
                    step_cached_rebuild = output.event_rebuild.clone();
                }
                consume_layout_output(output)
            });
        },
    );

    let (mut render_only_step_tree, _, render_only_step_root_id) =
        prepare_scroll_viewport_tree(case.make_tree, constraint, ScrollViewportPosition::Middle);
    let mut render_only_step_tick = 0_u64;
    group.bench_function(format!("{}/scroll_step_render_only", case.name), move |b| {
        b.iter(|| {
            render_only_step_tick = render_only_step_tick.saturating_add(1);
            let delta = if render_only_step_tick.is_multiple_of(2) {
                -24.0
            } else {
                24.0
            };
            black_box(render_only_step_tree.apply_scroll_y(&render_only_step_root_id, delta));
            let scene = refresh_render_scene_for_benchmark(&mut render_only_step_tree);
            consume_render_scene(scene)
        });
    });
}

fn prepare_scroll_viewport_tree(
    make_tree: fn() -> ElementTree,
    constraint: Constraint,
    position: ScrollViewportPosition,
) -> (ElementTree, RegistryRebuildPayload, NodeId) {
    let mut tree = make_tree();
    let root_id = tree
        .root_id()
        .expect("scroll viewport benchmark tree should have a root");
    let mut output = layout_and_refresh_default(&mut tree, constraint, 1.0);
    match position {
        ScrollViewportPosition::Top => {}
        ScrollViewportPosition::Middle => {
            black_box(tree.apply_scroll_y(&root_id, -40_000.0));
            output = refresh_reusing_clean_registry_for_benchmark(
                &mut tree,
                Some(&output.event_rebuild),
            );
        }
    }

    (tree, output.event_rebuild, root_id)
}

fn log_scroll_viewport_case_diagnostics(case: ScrollViewportCase, constraint: Constraint) {
    let (mut top_tree, top_cached_rebuild, _) =
        prepare_scroll_viewport_tree(case.make_tree, constraint, ScrollViewportPosition::Top);
    sample_scroll_viewport_refresh_diagnostics(
        &format!("{}/top", case.name),
        &mut top_tree,
        &top_cached_rebuild,
    );

    let (mut middle_tree, middle_cached_rebuild, _) =
        prepare_scroll_viewport_tree(case.make_tree, constraint, ScrollViewportPosition::Middle);
    sample_scroll_viewport_refresh_diagnostics(
        &format!("{}/middle", case.name),
        &mut middle_tree,
        &middle_cached_rebuild,
    );
}

#[cfg(feature = "bench-diagnostics")]
fn sample_scroll_viewport_refresh_diagnostics(
    label: &str,
    tree: &mut ElementTree,
    cached_rebuild: &RegistryRebuildPayload,
) {
    reset_render_traversal_diagnostics_for_benchmark();
    let output = refresh_reusing_clean_registry_for_benchmark(tree, Some(cached_rebuild));
    let diagnostics = take_render_traversal_diagnostics_for_benchmark();
    let summary = output.scene.summary();
    eprintln!(
        concat!(
            "scroll_viewport_diag {}: retained_nodes={} traversal_visits={} ",
            "culled_subtrees={} scene={}"
        ),
        label,
        tree.len(),
        diagnostics.element_visits,
        diagnostics.culled_subtrees,
        summary
    );
    consume_layout_output(output);
}

#[cfg(not(feature = "bench-diagnostics"))]
fn sample_scroll_viewport_refresh_diagnostics(
    _label: &str,
    _tree: &mut ElementTree,
    _cached_rebuild: &RegistryRebuildPayload,
) {
}

fn bench_fixture_retained_layout_after_patch(c: &mut Criterion) {
    let constraint = Constraint::new(960.0, 4_000.0);

    for fixture_id in RETAINED_FIXTURE_IDS {
        let fixture = load_fixture(fixture_id);
        let Some(base_tree) = decode_fixture_tree_or_skip(&fixture.id, &fixture.full_emrg) else {
            continue;
        };
        let node_count = base_tree.len() as u64;
        let mut warmed_base = base_tree.clone();
        layout_tree_default(&mut warmed_base, constraint, 1.0);

        let mut group =
            c.benchmark_group(format!("native/layout_retained_after_patch/{}", fixture.id));
        group.throughput(Throughput::Elements(node_count));

        for mutation in RETAINED_MUTATIONS {
            let Some(decoded_patches) = decode_fixture_patches_or_skip(
                &fixture.id,
                mutation,
                fixture.patch_bytes(mutation),
            ) else {
                continue;
            };

            group.bench_function(*mutation, |b| {
                b.iter_batched(
                    || {
                        let mut tree = warmed_base.clone();
                        let invalidation = apply_patches(&mut tree, decoded_patches.clone())
                            .expect("patch applies");
                        black_box(invalidation);
                        tree
                    },
                    |mut tree| {
                        layout_tree_default(&mut tree, constraint, 1.0);
                        black_box(tree.len())
                    },
                    BatchSize::SmallInput,
                );
            });
        }

        group.finish();
    }
}

fn decode_fixture_tree_or_skip(id: &str, bytes: &[u8]) -> Option<ElementTree> {
    match decode_tree(bytes) {
        Ok(tree) => Some(tree),
        Err(err) => {
            eprintln!("Skipping benchmark fixture {id}: full.emrg does not decode: {err}");
            None
        }
    }
}

fn decode_fixture_patches_or_skip(id: &str, mutation: &str, bytes: &[u8]) -> Option<Vec<Patch>> {
    match decode_patches(bytes) {
        Ok(patches) => Some(patches),
        Err(err) => {
            eprintln!("Skipping benchmark fixture {id}/{mutation}: patch does not decode: {err}");
            None
        }
    }
}

fn bench_fixture_retained_patch_layout(c: &mut Criterion) {
    let constraint = Constraint::new(960.0, 4_000.0);

    for fixture_id in RETAINED_FIXTURE_IDS {
        let fixture = load_fixture(fixture_id);
        let Some(base_tree) = decode_fixture_tree_or_skip(&fixture.id, &fixture.full_emrg) else {
            continue;
        };
        let node_count = base_tree.len() as u64;
        let mut warmed_base = base_tree.clone();
        layout_tree_default(&mut warmed_base, constraint, 1.0);

        let mut group = c.benchmark_group(format!(
            "native/layout_retained_patch_layout/{}",
            fixture.id
        ));
        group.throughput(Throughput::Elements(node_count));

        for mutation in RETAINED_MUTATIONS {
            let patch_bytes = fixture.patch_bytes(mutation).to_vec();

            group.bench_function(*mutation, |b| {
                b.iter_batched(
                    || (warmed_base.clone(), patch_bytes.clone()),
                    |(mut tree, bytes)| {
                        let patches = decode_patches(black_box(&bytes)).expect("patch decodes");
                        let invalidation =
                            apply_patches(&mut tree, patches).expect("patch applies");
                        layout_tree_default(&mut tree, constraint, 1.0);
                        black_box((tree.len(), invalidation))
                    },
                    BatchSize::SmallInput,
                );
            });
        }

        group.finish();
    }
}

fn bench_render_refresh_cache_regression(c: &mut Criterion) {
    let constraint = Constraint::new(960.0, 4_000.0);
    let mut group = c.benchmark_group("native/render_refresh_cache_regression");

    for (fixture_id, mutation) in RENDER_REFRESH_REGRESSION_FIXTURE_CASES {
        let fixture = load_fixture(fixture_id);
        let Some(base_tree) = decode_fixture_tree_or_skip(&fixture.id, &fixture.full_emrg) else {
            continue;
        };
        let node_count = base_tree.len() as u64;
        let mut warmed_base = base_tree;
        let warm_output = layout_and_refresh_default(&mut warmed_base, constraint, 1.0);
        let cached_rebuild = warm_output.event_rebuild;
        let Some(decoded_patches) =
            decode_fixture_patches_or_skip(&fixture.id, mutation, fixture.patch_bytes(mutation))
        else {
            continue;
        };
        let patch_bytes = fixture.patch_bytes(mutation).to_vec();
        let case = format!("{fixture_id}/{mutation}");

        group.throughput(Throughput::Elements(node_count));
        bench_cold_layout_refresh(&mut group, &case, &fixture.full_emrg, constraint);
        bench_warm_refresh(
            &mut group,
            &case,
            warmed_base.clone(),
            cached_rebuild.clone(),
        );
        bench_after_patch_refresh(
            &mut group,
            &case,
            warmed_base.clone(),
            cached_rebuild.clone(),
            decoded_patches,
            constraint,
        );
        bench_patch_refresh(
            &mut group,
            &case,
            warmed_base,
            cached_rebuild,
            patch_bytes,
            constraint,
        );
    }

    bench_animation_refresh_regression(
        &mut group,
        "animated_shadow_showcase/paint_only_refresh_each_frame",
        Constraint::new(960.0, 4_000.0),
        animated_shadow_showcase,
        false,
    );
    bench_animation_refresh_regression(
        &mut group,
        "scroll_shadow_showcase/paint_only_refresh_scroll_frame",
        Constraint::new(960.0, 640.0),
        scrollable_animated_shadow_showcase,
        true,
    );

    group.finish();
}

fn bench_cold_layout_refresh(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    case: &str,
    full_emrg: &[u8],
    constraint: Constraint,
) {
    let full_bytes = full_emrg.to_vec();
    group.bench_function(format!("{case}/cold_layout_refresh"), move |b| {
        b.iter_batched(
            || decode_tree(&full_bytes).expect("fixture tree should decode"),
            |mut tree| {
                let output = layout_and_refresh_default(&mut tree, constraint, 1.0);
                consume_layout_output(output)
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_warm_refresh(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    case: &str,
    warmed_base: ElementTree,
    cached_rebuild: RegistryRebuildPayload,
) {
    let mut cached_tree = warmed_base.clone();
    let cached_registry = cached_rebuild.clone();
    group.bench_function(format!("{case}/cached_refresh"), move |b| {
        b.iter(|| {
            let output = refresh_reusing_clean_registry_for_benchmark(
                &mut cached_tree,
                Some(&cached_registry),
            );
            consume_layout_output(output)
        });
    });
}

fn bench_after_patch_refresh(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    case: &str,
    warmed_base: ElementTree,
    cached_rebuild: RegistryRebuildPayload,
    decoded_patches: Vec<Patch>,
    constraint: Constraint,
) {
    let cached_base = warmed_base.clone();
    let cached_patches = decoded_patches.clone();
    let cached_registry = cached_rebuild.clone();
    group.bench_function(format!("{case}/after_patch_cached_refresh"), move |b| {
        b.iter_batched(
            || prepare_after_patch_refresh_tree(&cached_base, &cached_patches, constraint),
            |mut tree| {
                let output =
                    refresh_reusing_clean_registry_for_benchmark(&mut tree, Some(&cached_registry));
                consume_layout_output(output)
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_patch_refresh(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    case: &str,
    warmed_base: ElementTree,
    cached_rebuild: RegistryRebuildPayload,
    patch_bytes: Vec<u8>,
    constraint: Constraint,
) {
    let cached_base = warmed_base.clone();
    let cached_patch_bytes = patch_bytes.clone();
    let cached_registry = cached_rebuild.clone();
    group.bench_function(format!("{case}/patch_cached_refresh"), move |b| {
        b.iter_batched(
            || (cached_base.clone(), cached_patch_bytes.clone()),
            |(mut tree, bytes)| {
                apply_patch_and_relayout_if_needed(&mut tree, &bytes, constraint);
                let output =
                    refresh_reusing_clean_registry_for_benchmark(&mut tree, Some(&cached_registry));
                consume_layout_output(output)
            },
            BatchSize::SmallInput,
        );
    });
}

fn prepare_after_patch_refresh_tree(
    warmed_base: &ElementTree,
    decoded_patches: &[Patch],
    constraint: Constraint,
) -> ElementTree {
    let mut tree = warmed_base.clone();
    let invalidation = apply_patches(&mut tree, decoded_patches.to_vec()).expect("patch applies");
    if invalidation.requires_recompute() {
        layout_tree_default(&mut tree, constraint, 1.0);
    }
    tree
}

fn apply_patch_and_relayout_if_needed(
    tree: &mut ElementTree,
    bytes: &[u8],
    constraint: Constraint,
) {
    let patches = decode_patches(black_box(bytes)).expect("patch decodes");
    let invalidation = apply_patches(tree, patches).expect("patch applies");
    if invalidation.requires_recompute() {
        layout_tree_default(tree, constraint, 1.0);
    }
}

fn bench_animation_refresh_regression(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    case: &str,
    constraint: Constraint,
    make_tree: fn() -> ElementTree,
    scroll_each_frame: bool,
) {
    let start = Instant::now();
    let node_count = make_tree().len() as u64;
    group.throughput(Throughput::Elements(node_count));

    let mut cached_tree = make_tree();
    let cached_root_id = cached_tree.root_id();
    let mut cached_runtime = AnimationRuntime::default();
    cached_runtime.sync_with_tree(&cached_tree, start);
    layout_and_refresh_default_with_animation(
        &mut cached_tree,
        constraint,
        1.0,
        &cached_runtime,
        start,
    );
    let mut cached_tick = 0_u64;
    group.bench_function(format!("{case}/cached_refresh"), move |b| {
        b.iter(|| {
            cached_tick += 16;
            if scroll_each_frame {
                let delta = if cached_tick.is_multiple_of(32) {
                    8.0
                } else {
                    -8.0
                };
                if let Some(root_id) = cached_root_id {
                    black_box(cached_tree.apply_scroll_y(&root_id, delta));
                }
            }
            let update = layout_or_refresh_default_with_animation(
                &mut cached_tree,
                constraint,
                1.0,
                &cached_runtime,
                start + Duration::from_millis(cached_tick),
            );
            consume_layout_update_output(update)
        });
    });
}

fn bench_registry_refresh_cache_regression(c: &mut Criterion) {
    let constraint = Constraint::new(960.0, 4_000.0);
    let mut group = c.benchmark_group("native/registry_refresh_cache_regression");

    for (fixture_id, mutation) in REGISTRY_REFRESH_REGRESSION_FIXTURE_CASES {
        let fixture = load_fixture(fixture_id);
        let Some(base_tree) = decode_fixture_tree_or_skip(&fixture.id, &fixture.full_emrg) else {
            continue;
        };
        let node_count = base_tree.len() as u64;
        let mut warmed_base = base_tree;
        let warm_output = layout_and_refresh_default(&mut warmed_base, constraint, 1.0);
        let cached_rebuild = warm_output.event_rebuild;
        let Some(decoded_patches) =
            decode_fixture_patches_or_skip(&fixture.id, mutation, fixture.patch_bytes(mutation))
        else {
            continue;
        };
        let patch_bytes = fixture.patch_bytes(mutation).to_vec();
        let case = format!("{fixture_id}/{mutation}");

        group.throughput(Throughput::Elements(node_count));
        bench_registry_full_rebuild_pair(&mut group, &case, warmed_base.clone());
        bench_registry_clean_reuse(
            &mut group,
            &case,
            warmed_base.clone(),
            cached_rebuild.clone(),
        );
        bench_registry_after_patch_rebuild_pair(
            &mut group,
            &case,
            warmed_base.clone(),
            decoded_patches,
            constraint,
        );
        bench_registry_patch_rebuild_pair(&mut group, &case, warmed_base, patch_bytes, constraint);
    }

    group.finish();
}

fn bench_registry_full_rebuild_pair(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    case: &str,
    warmed_base: ElementTree,
) {
    let full_tree = warmed_base.clone();
    group.bench_function(format!("{case}/full_registry_rebuild"), move |b| {
        b.iter(|| {
            let rebuild = build_registry_rebuild_for_benchmark(&full_tree);
            consume_registry_rebuild(rebuild)
        });
    });

    let mut chunked_tree = warmed_base;
    group.bench_function(format!("{case}/chunked_registry_rebuild"), move |b| {
        b.iter(|| {
            let rebuild = build_registry_rebuild_cached_for_benchmark(&mut chunked_tree);
            consume_registry_rebuild(rebuild)
        });
    });
}

fn bench_registry_clean_reuse(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    case: &str,
    warmed_base: ElementTree,
    cached_rebuild: RegistryRebuildPayload,
) {
    let mut tree = warmed_base;
    group.bench_function(format!("{case}/clean_registry_reuse"), move |b| {
        b.iter(|| {
            let output =
                refresh_reusing_clean_registry_for_benchmark(&mut tree, Some(&cached_rebuild));
            consume_layout_output(output)
        });
    });
}

fn bench_registry_after_patch_rebuild_pair(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    case: &str,
    warmed_base: ElementTree,
    decoded_patches: Vec<Patch>,
    constraint: Constraint,
) {
    let full_base = warmed_base.clone();
    let full_patches = decoded_patches.clone();
    group.bench_function(format!("{case}/after_patch_full_registry"), move |b| {
        b.iter_batched(
            || prepare_after_patch_refresh_tree(&full_base, &full_patches, constraint),
            |tree| {
                let rebuild = build_registry_rebuild_for_benchmark(&tree);
                consume_registry_rebuild(rebuild)
            },
            BatchSize::SmallInput,
        );
    });

    let chunked_base = warmed_base.clone();
    let chunked_patches = decoded_patches.clone();
    group.bench_function(format!("{case}/after_patch_chunked_registry"), move |b| {
        b.iter_batched(
            || prepare_after_patch_refresh_tree(&chunked_base, &chunked_patches, constraint),
            |mut tree| {
                let rebuild = build_registry_rebuild_cached_for_benchmark(&mut tree);
                consume_registry_rebuild(rebuild)
            },
            BatchSize::SmallInput,
        );
    });

    let seeded_base = seed_registry_subtree_cache(warmed_base.clone());
    let seeded_patches = decoded_patches.clone();
    group.bench_function(
        format!("{case}/after_patch_seeded_chunked_registry"),
        move |b| {
            b.iter_batched(
                || prepare_after_patch_refresh_tree(&seeded_base, &seeded_patches, constraint),
                |mut tree| {
                    let rebuild = build_registry_rebuild_cached_for_benchmark(&mut tree);
                    consume_registry_rebuild(rebuild)
                },
                BatchSize::SmallInput,
            );
        },
    );
}

fn bench_registry_patch_rebuild_pair(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    case: &str,
    warmed_base: ElementTree,
    patch_bytes: Vec<u8>,
    constraint: Constraint,
) {
    let full_base = warmed_base.clone();
    let full_patch_bytes = patch_bytes.clone();
    group.bench_function(format!("{case}/patch_full_registry"), move |b| {
        b.iter_batched(
            || (full_base.clone(), full_patch_bytes.clone()),
            |(mut tree, bytes)| {
                apply_patch_and_relayout_if_needed(&mut tree, &bytes, constraint);
                let rebuild = build_registry_rebuild_for_benchmark(&tree);
                consume_registry_rebuild(rebuild)
            },
            BatchSize::SmallInput,
        );
    });

    let chunked_base = warmed_base.clone();
    let chunked_patch_bytes = patch_bytes.clone();
    group.bench_function(format!("{case}/patch_chunked_registry"), move |b| {
        b.iter_batched(
            || (chunked_base.clone(), chunked_patch_bytes.clone()),
            |(mut tree, bytes)| {
                apply_patch_and_relayout_if_needed(&mut tree, &bytes, constraint);
                let rebuild = build_registry_rebuild_cached_for_benchmark(&mut tree);
                consume_registry_rebuild(rebuild)
            },
            BatchSize::SmallInput,
        );
    });

    let seeded_base = seed_registry_subtree_cache(warmed_base);
    group.bench_function(format!("{case}/patch_seeded_chunked_registry"), move |b| {
        b.iter_batched(
            || (seeded_base.clone(), patch_bytes.clone()),
            |(mut tree, bytes)| {
                apply_patch_and_relayout_if_needed(&mut tree, &bytes, constraint);
                let rebuild = build_registry_rebuild_cached_for_benchmark(&mut tree);
                consume_registry_rebuild(rebuild)
            },
            BatchSize::SmallInput,
        );
    });
}

fn seed_registry_subtree_cache(mut tree: ElementTree) -> ElementTree {
    let _ = build_registry_rebuild_cached_for_benchmark(&mut tree);
    tree
}

fn consume_registry_rebuild(rebuild: RegistryRebuildPayload) {
    black_box((
        rebuild.text_inputs.len(),
        rebuild.scrollbars.len(),
        rebuild.focused_id.is_some(),
        rebuild.focus_on_mount.is_some(),
    ));
    black_box(rebuild.base_registry);
}

fn consume_render_scene(scene: emerge_skia::render_scene::RenderScene) {
    black_box(scene.nodes.len());
}

fn consume_layout_update_output(output: emerge_skia::tree::layout::LayoutUpdateOutput) {
    black_box((
        output.output.scene.nodes.len(),
        output.output.event_rebuild.text_inputs.len(),
        output.output.event_rebuild_changed,
        output.output.ime_enabled,
        output.layout_performed,
    ));
}

fn consume_layout_output(output: emerge_skia::tree::layout::LayoutOutput) {
    black_box((
        output.scene.nodes.len(),
        output.event_rebuild.text_inputs.len(),
        output.event_rebuild_changed,
        output.ime_enabled,
    ));
}

fn patch_set_attrs_ids(patches: &[Patch]) -> Option<Vec<NodeId>> {
    patches
        .iter()
        .map(|patch| match patch {
            Patch::SetAttrs { id, .. } => Some(*id),
            Patch::SetChildren { .. }
            | Patch::SetNearbyMounts { .. }
            | Patch::InsertSubtree { .. }
            | Patch::InsertNearbySubtree { .. }
            | Patch::Remove { .. } => None,
        })
        .collect()
}

fn record_frame_attr_dirty_id(
    frame_attr_dirty_ids: &mut Vec<NodeId>,
    id: NodeId,
    invalidation: TreeInvalidation,
) {
    if invalidation.can_refresh_only() && !frame_attr_dirty_ids.contains(&id) {
        frame_attr_dirty_ids.push(id);
    }
}

fn extend_frame_attr_dirty_ids(frame_attr_dirty_ids: &mut Vec<NodeId>, ids: Vec<NodeId>) {
    ids.into_iter().for_each(|id| {
        record_frame_attr_dirty_id(frame_attr_dirty_ids, id, TreeInvalidation::Paint)
    });
}

const MACAW_VIEWPORT_SEED: u64 = 800_000;
const MACAW_VIEWPORT_WIDTH: f32 = 1024.0;
const MACAW_VIEWPORT_HEIGHT: f32 = 600.0;
const MACAW_STATUS_REPEAT_COUNT: usize = 10;

const MACAW_STATUS_LINES: &[(&str, &str)] = &[
    ("UI mode", "user"),
    ("Effective theme", ":light"),
    ("Configured theme", "light"),
    ("Brightness", "80%"),
    ("Sleeping", "false"),
    ("Locale", "en"),
    ("Home screen", ":climate"),
    ("User mode", ":standard"),
    ("Auto brightness", "false"),
];

fn bench_macaw_viewport_refresh(c: &mut Criterion) {
    let mut group = c.benchmark_group("native/macaw_viewport/full_viewport");
    let constraint = macaw_viewport_constraint();
    let node_count = macaw_viewport_tree(
        Some(MacawSidepaneAnimationKind::Settled),
        MACAW_VIEWPORT_SEED,
    )
    .tree
    .len() as u64;
    group.throughput(Throughput::Elements(node_count));

    #[cfg(feature = "bench-diagnostics")]
    if std::env::var_os("EMERGE_BENCH_DIAGNOSTICS").is_some() {
        sample_macaw_viewport_profiles();
    }

    let closed = prepared_macaw_closed_viewport_tree(constraint);
    group.bench_function("open_patch_first_frame_one_toggle", |b| {
        b.iter_batched(
            || closed.clone(),
            |(mut tree, cached_rebuild, host_id)| {
                let mut runtime = AnimationRuntime::default();
                let started_at = Instant::now();
                let invalidation = apply_patches(
                    &mut tree,
                    vec![Patch::InsertNearbySubtree {
                        host_id,
                        index: 0,
                        slot: NearbySlot::InFront,
                        subtree: macaw_sidepane_subtree(
                            NodeId::from_u64(MACAW_VIEWPORT_SEED + 50_000),
                            MACAW_VIEWPORT_SEED + 60_000,
                            MacawSidepaneAnimationKind::EnterMoveX,
                        ),
                    }],
                )
                .expect("macaw sidepane open patch should apply");
                runtime.sync_with_tree(&tree, started_at);
                let update = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
                    &mut tree,
                    constraint,
                    1.0,
                    &runtime,
                    started_at,
                    invalidation,
                    Some(&cached_rebuild),
                );
                consume_layout_update_output(update)
            },
            BatchSize::SmallInput,
        );
    });

    let open_for_close = prepared_macaw_open_viewport_tree_for_close(constraint);
    group.bench_function("close_patch_exit_first_frame_one_toggle", |b| {
        b.iter_batched(
            || open_for_close.clone(),
            |(mut tree, cached_rebuild, sidepane_id)| {
                let mut runtime = AnimationRuntime::default();
                let started_at = Instant::now();
                let invalidation = apply_patches(&mut tree, vec![Patch::Remove { id: sidepane_id }])
                    .expect("macaw sidepane close patch should apply");
                runtime.sync_with_tree(&tree, started_at);
                let update = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
                    &mut tree,
                    constraint,
                    1.0,
                    &runtime,
                    started_at,
                    invalidation,
                    Some(&cached_rebuild),
                );
                consume_layout_update_output(update)
            },
            BatchSize::SmallInput,
        );
    });

    let closed_after_exit = prepared_macaw_closed_after_exit_tree(constraint);
    group.bench_function("second_open_patch_first_frame_after_exit", |b| {
        b.iter_batched(
            || closed_after_exit.clone(),
            |(mut tree, cached_rebuild, host_id, seed)| {
                let mut runtime = AnimationRuntime::default();
                let started_at = Instant::now();
                let invalidation = apply_patches(
                    &mut tree,
                    vec![Patch::InsertNearbySubtree {
                        host_id,
                        index: 0,
                        slot: NearbySlot::InFront,
                        subtree: macaw_sidepane_subtree(
                            NodeId::from_u64(seed + 50_000),
                            seed + 60_000,
                            MacawSidepaneAnimationKind::EnterMoveX,
                        ),
                    }],
                )
                .expect("macaw cached sidepane reopen patch should apply");
                runtime.sync_with_tree(&tree, started_at);
                let update = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
                    &mut tree,
                    constraint,
                    1.0,
                    &runtime,
                    started_at,
                    invalidation,
                    Some(&cached_rebuild),
                );
                consume_layout_update_output(update)
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("enter_transient_pulse_retained_payload", |b| {
        let mut state = MacawViewportTransientEnterPulseState::new(MACAW_VIEWPORT_SEED + 90_000);
        b.iter(|| consume_layout_update_output(state.next_frame()));
    });

    group.bench_function("move_x_pulse_retained_payload", |b| {
        let mut state = MacawViewportPulseState::new(MACAW_VIEWPORT_SEED + 100_000, false);
        b.iter(|| consume_layout_update_output(state.next_frame()));
    });

    group.bench_function("move_x_pulse_content_dirty_control", |b| {
        let mut state = MacawViewportPulseState::new(MACAW_VIEWPORT_SEED + 120_000, true);
        b.iter(|| consume_layout_update_output(state.next_frame()));
    });

    group.finish();
}

#[derive(Clone, Copy)]
enum MacawSidepaneAnimationKind {
    EnterMoveX,
    LoopMoveX,
    Settled,
}

#[derive(Clone)]
struct MacawViewportFixture {
    tree: ElementTree,
    root_id: NodeId,
    sidepane_id: Option<NodeId>,
}

#[derive(Clone)]
struct MacawViewportBuilder {
    tree: ElementTree,
    next_id: u64,
}

impl MacawViewportBuilder {
    fn new(seed: u64) -> Self {
        Self {
            tree: ElementTree::new(),
            next_id: seed,
        }
    }

    fn node(&mut self, kind: ElementKind, attrs: Attrs, children: Vec<NodeId>) -> NodeId {
        self.next_id = self.next_id.saturating_add(1);
        let id = NodeId::from_u64(self.next_id);
        self.tree
            .insert(Element::with_attrs(id, kind, Vec::new(), attrs));
        if !children.is_empty() {
            self.tree
                .set_children(&id, children)
                .expect("macaw viewport children should attach");
        }
        id
    }

    fn text(&mut self, content: impl Into<String>, attrs: Attrs) -> NodeId {
        self.node(
            ElementKind::Text,
            Attrs {
                content: Some(content.into()),
                ..attrs
            },
            Vec::new(),
        )
    }
}

struct MacawViewportTransientEnterPulseState {
    tree: ElementTree,
    runtime: AnimationRuntime,
    cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    frame: usize,
}

impl MacawViewportTransientEnterPulseState {
    fn new(seed: u64) -> Self {
        let constraint = macaw_viewport_constraint();
        let started_at = Instant::now();
        let fixture = macaw_viewport_tree(Some(MacawSidepaneAnimationKind::EnterMoveX), seed);
        let mut tree = fixture.tree;
        let mut runtime = AnimationRuntime::default();
        runtime.sync_with_tree(&tree, started_at);
        let output = layout_and_refresh_default_with_animation(
            &mut tree, constraint, 1.0, &runtime, started_at,
        );
        let mut cached_rebuild = output.event_rebuild;

        let warm = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut tree,
            constraint,
            1.0,
            &runtime,
            started_at + Duration::from_millis(SHOWCASE_FRAME_MS),
            TreeInvalidation::None,
            Some(&cached_rebuild),
        );
        if warm.output.event_rebuild_changed {
            cached_rebuild = warm.output.event_rebuild;
        }

        Self {
            tree,
            runtime,
            cached_rebuild,
            started_at,
            frame: 1,
        }
    }

    fn next_frame(&mut self) -> emerge_skia::tree::layout::LayoutUpdateOutput {
        const TRANSIENT_SAMPLE_OFFSETS_MS: [u64; 6] = [16, 32, 48, 64, 80, 96];
        self.frame = (self.frame + 1) % TRANSIENT_SAMPLE_OFFSETS_MS.len();
        let sample_time =
            self.started_at + Duration::from_millis(TRANSIENT_SAMPLE_OFFSETS_MS[self.frame]);
        let update = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut self.tree,
            macaw_viewport_constraint(),
            1.0,
            &self.runtime,
            sample_time,
            TreeInvalidation::None,
            Some(&self.cached_rebuild),
        );
        if update.output.event_rebuild_changed {
            self.cached_rebuild = update.output.event_rebuild.clone();
        }
        update
    }
}

struct MacawViewportPulseState {
    tree: ElementTree,
    runtime: AnimationRuntime,
    cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    sidepane_id: NodeId,
    force_content_dirty: bool,
    frame: u64,
}

impl MacawViewportPulseState {
    fn new(seed: u64, force_content_dirty: bool) -> Self {
        let constraint = macaw_viewport_constraint();
        let started_at = Instant::now();
        let fixture = macaw_viewport_tree(Some(MacawSidepaneAnimationKind::LoopMoveX), seed);
        let sidepane_id = fixture
            .sidepane_id
            .expect("looping macaw viewport should include sidepane");
        let mut tree = fixture.tree;
        let mut runtime = AnimationRuntime::default();
        runtime.sync_with_tree(&tree, started_at);
        let output = layout_and_refresh_default_with_animation(
            &mut tree, constraint, 1.0, &runtime, started_at,
        );
        let mut cached_rebuild = output.event_rebuild;

        let warm = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut tree,
            constraint,
            1.0,
            &runtime,
            started_at + Duration::from_millis(SHOWCASE_FRAME_MS),
            TreeInvalidation::None,
            Some(&cached_rebuild),
        );
        if warm.output.event_rebuild_changed {
            cached_rebuild = warm.output.event_rebuild;
        }

        Self {
            tree,
            runtime,
            cached_rebuild,
            started_at,
            sidepane_id,
            force_content_dirty,
            frame: 1,
        }
    }

    fn next_frame(&mut self) -> emerge_skia::tree::layout::LayoutUpdateOutput {
        self.frame = self.frame.saturating_add(1);
        let sample_time =
            self.started_at + Duration::from_millis(self.frame.saturating_mul(SHOWCASE_FRAME_MS));
        let invalidation = if self.force_content_dirty {
            self.tree
                .mark_refresh_dirty_for_invalidation(&self.sidepane_id, TreeInvalidation::Paint);
            TreeInvalidation::Paint
        } else {
            TreeInvalidation::None
        };
        let update = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut self.tree,
            macaw_viewport_constraint(),
            1.0,
            &self.runtime,
            sample_time,
            invalidation,
            Some(&self.cached_rebuild),
        );
        if update.output.event_rebuild_changed {
            self.cached_rebuild = update.output.event_rebuild.clone();
        }
        update
    }
}

fn prepared_macaw_closed_viewport_tree(
    constraint: Constraint,
) -> (ElementTree, RegistryRebuildPayload, NodeId) {
    let fixture = macaw_viewport_tree(None, MACAW_VIEWPORT_SEED);
    let host_id = fixture.root_id;
    let mut tree = fixture.tree;
    let output = layout_and_refresh_default(&mut tree, constraint, 1.0);
    (tree, output.event_rebuild, host_id)
}

fn prepared_macaw_open_viewport_tree_for_close(
    constraint: Constraint,
) -> (ElementTree, RegistryRebuildPayload, NodeId) {
    let fixture = macaw_viewport_tree(
        Some(MacawSidepaneAnimationKind::Settled),
        MACAW_VIEWPORT_SEED + 20_000,
    );
    let sidepane_id = fixture
        .sidepane_id
        .expect("open macaw viewport should include sidepane");
    let mut tree = fixture.tree;
    let output = layout_and_refresh_default(&mut tree, constraint, 1.0);
    (tree, output.event_rebuild, sidepane_id)
}

fn prepared_macaw_closed_after_exit_tree(
    constraint: Constraint,
) -> (ElementTree, RegistryRebuildPayload, NodeId, u64) {
    let seed = MACAW_VIEWPORT_SEED + 40_000;
    let fixture = macaw_viewport_tree(Some(MacawSidepaneAnimationKind::Settled), seed);
    let host_id = fixture.root_id;
    let sidepane_id = fixture
        .sidepane_id
        .expect("open macaw viewport should include sidepane");
    let mut tree = fixture.tree;
    let initial = layout_and_refresh_default(&mut tree, constraint, 1.0);
    let mut cached_rebuild = initial.event_rebuild;
    let mut runtime = AnimationRuntime::default();
    let started_at = Instant::now();

    let invalidation = apply_patches(&mut tree, vec![Patch::Remove { id: sidepane_id }])
        .expect("macaw sidepane close patch should apply");
    runtime.sync_with_tree(&tree, started_at);
    let close = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
        &mut tree,
        constraint,
        1.0,
        &runtime,
        started_at,
        invalidation,
        Some(&cached_rebuild),
    );
    if close.output.event_rebuild_changed {
        cached_rebuild = close.output.event_rebuild;
    }

    let final_time = started_at + Duration::from_millis(200);
    if runtime.prune_completed_exit_ghosts(&mut tree, Some(final_time)) {
        let pruned = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut tree,
            constraint,
            1.0,
            &runtime,
            final_time,
            TreeInvalidation::Structure,
            Some(&cached_rebuild),
        );
        if pruned.output.event_rebuild_changed {
            cached_rebuild = pruned.output.event_rebuild;
        }
    }

    (tree, cached_rebuild, host_id, seed)
}

fn macaw_viewport_constraint() -> Constraint {
    Constraint::new(MACAW_VIEWPORT_WIDTH, MACAW_VIEWPORT_HEIGHT)
}

fn macaw_viewport_tree(
    sidepane_kind: Option<MacawSidepaneAnimationKind>,
    seed: u64,
) -> MacawViewportFixture {
    let mut builder = MacawViewportBuilder::new(seed);
    let header = macaw_header(&mut builder);
    let status_panel = macaw_status_panel(&mut builder);
    let root_id = builder.node(
        ElementKind::Column,
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            padding: Some(Padding::Uniform(24.0)),
            spacing: Some(20.0),
            background: Some(Background::Color(macaw_rgb(2, 6, 23))),
            ..Default::default()
        },
        vec![header, status_panel],
    );
    builder.tree.set_root_id(root_id);

    let sidepane_id = sidepane_kind.map(|kind| {
        let sidepane_id = NodeId::from_u64(seed + 50_000);
        let subtree = macaw_sidepane_subtree(sidepane_id, seed + 60_000, kind);
        append_benchmark_subtree(&mut builder.tree, &subtree);
        builder
            .tree
            .set_nearby_mounts(
                &root_id,
                vec![NearbyMount {
                    slot: NearbySlot::InFront,
                    id: sidepane_id,
                }],
            )
            .expect("macaw sidepane nearby mount should attach");
        sidepane_id
    });

    MacawViewportFixture {
        tree: builder.tree,
        root_id,
        sidepane_id,
    }
}

fn macaw_header(builder: &mut MacawViewportBuilder) -> NodeId {
    let button_text = builder.text(
        "Open sidepane",
        Attrs {
            font_size: Some(12.0),
            ..Default::default()
        },
    );
    let button = builder.node(
        ElementKind::El,
        Attrs {
            on_click: Some(true),
            background: Some(Background::Color(macaw_rgb(255, 255, 255))),
            padding: Some(Padding::Uniform(10.0)),
            font_size: Some(12.0),
            border_radius: Some(BorderRadius::Uniform(3.0)),
            ..Default::default()
        },
        vec![button_text],
    );
    let title = builder.text(
        "Macaw UI running on EMERGE",
        Attrs {
            font_size: Some(28.0),
            font_weight: Some(FontWeight("bold".to_string())),
            font_color: Some(macaw_rgb(255, 255, 255)),
            ..Default::default()
        },
    );
    let subtitle = builder.text(
        "Emerge placeholder screen is running in user mode.",
        Attrs {
            font_size: Some(16.0),
            font_color: Some(macaw_rgb(203, 213, 225)),
            ..Default::default()
        },
    );

    builder.node(
        ElementKind::Column,
        Attrs {
            width: Some(Length::Fill),
            spacing: Some(6.0),
            ..Default::default()
        },
        vec![button, title, subtitle],
    )
}

fn macaw_status_panel(builder: &mut MacawViewportBuilder) -> NodeId {
    let rows: Vec<NodeId> = (0..MACAW_STATUS_REPEAT_COUNT)
        .flat_map(|_| MACAW_STATUS_LINES.iter().copied())
        .map(|(label, value)| macaw_status_line(builder, label, value))
        .collect();

    builder.node(
        ElementKind::Column,
        Attrs {
            spacing: Some(12.0),
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            padding: Some(Padding::Uniform(20.0)),
            background: Some(Background::Color(macaw_rgb(15, 23, 42))),
            border_radius: Some(BorderRadius::Uniform(12.0)),
            scrollbar_y: Some(true),
            ..Default::default()
        },
        rows,
    )
}

fn macaw_status_line(builder: &mut MacawViewportBuilder, label: &str, value: &str) -> NodeId {
    let label = builder.text(
        label,
        Attrs {
            font_size: Some(14.0),
            font_color: Some(macaw_rgb(148, 163, 184)),
            ..Default::default()
        },
    );
    let value = builder.text(
        value,
        Attrs {
            font_size: Some(18.0),
            font_color: Some(macaw_rgb(255, 255, 255)),
            ..Default::default()
        },
    );

    builder.node(
        ElementKind::Column,
        Attrs {
            spacing: Some(4.0),
            ..Default::default()
        },
        vec![label, value],
    )
}

fn macaw_sidepane_subtree(
    root_id: NodeId,
    seed: u64,
    kind: MacawSidepaneAnimationKind,
) -> ElementTree {
    let mut builder = MacawViewportBuilder::new(seed);
    let scrim = builder.node(
        ElementKind::El,
        Attrs {
            on_click: Some(true),
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            ..Default::default()
        },
        Vec::new(),
    );
    let mode_selector = macaw_mode_selector(&mut builder);
    let mut attrs = Attrs {
        width: Some(Length::Fill),
        height: Some(Length::Fill),
        background: Some(Background::Color(macaw_rgba(0, 0, 0, 64))),
        ..Default::default()
    };

    match kind {
        MacawSidepaneAnimationKind::EnterMoveX | MacawSidepaneAnimationKind::Settled => {
            attrs.animate_enter = Some(macaw_move_x_spec(
                500.0,
                0.0,
                AnimationCurve::EaseIn,
                AnimationRepeat::Once,
            ));
            attrs.animate_exit = Some(macaw_move_x_spec(
                0.0,
                500.0,
                AnimationCurve::EaseOut,
                AnimationRepeat::Once,
            ));
        }
        MacawSidepaneAnimationKind::LoopMoveX => {
            attrs.animate = Some(macaw_move_x_spec(
                500.0,
                0.0,
                AnimationCurve::EaseIn,
                AnimationRepeat::Loop,
            ));
        }
    }

    builder.tree.insert(Element::with_attrs(
        root_id,
        ElementKind::Row,
        Vec::new(),
        attrs,
    ));
    builder
        .tree
        .set_children(&root_id, vec![scrim, mode_selector])
        .expect("macaw sidepane root children should attach");
    builder.tree.set_root_id(root_id);
    builder.tree
}

fn macaw_mode_selector(builder: &mut MacawViewportBuilder) -> NodeId {
    let mut children = vec![
        macaw_close_header(builder),
        macaw_separator(builder),
        macaw_mode_section_header(builder),
    ];
    for (mode, label) in [
        ("heat", "Heat"),
        ("cool", "Cool"),
        ("auto", "Auto"),
        ("off", "Off"),
    ] {
        children.push(macaw_separator(builder));
        children.push(macaw_mode_row(builder, mode, label));
    }

    builder.node(
        ElementKind::Column,
        Attrs {
            width: Some(Length::Px(500.0)),
            height: Some(Length::Fill),
            background: Some(Background::Color(macaw_rgb(251, 252, 253))),
            font: Some(Font::String("Open Sans".to_string())),
            font_color: Some(macaw_rgb(26, 31, 39)),
            box_shadows: Some(vec![BoxShadow {
                offset_x: -8.0,
                offset_y: 0.0,
                blur: 24.0,
                size: 0.0,
                color: macaw_rgba(0, 0, 0, 46),
                inset: false,
            }]),
            ..Default::default()
        },
        children,
    )
}

fn macaw_close_header(builder: &mut MacawViewportBuilder) -> NodeId {
    let icon = builder.node(
        ElementKind::El,
        Attrs {
            width: Some(Length::Px(32.0)),
            height: Some(Length::Px(32.0)),
            background: Some(Background::Color(macaw_rgb(26, 31, 39))),
            border_radius: Some(BorderRadius::Uniform(16.0)),
            ..Default::default()
        },
        Vec::new(),
    );
    let label = builder.text(
        "Close",
        Attrs {
            align_y: Some(AlignY::Center),
            font_size: Some(20.0),
            font_weight: Some(FontWeight("600".to_string())),
            ..Default::default()
        },
    );
    builder.node(
        ElementKind::Row,
        Attrs {
            on_click: Some(true),
            padding: Some(Padding::Uniform(24.0)),
            spacing: Some(24.0),
            width: Some(Length::Fill),
            ..Default::default()
        },
        vec![icon, label],
    )
}

fn macaw_mode_section_header(builder: &mut MacawViewportBuilder) -> NodeId {
    let text = builder.text(
        "Select mode",
        Attrs {
            font_size: Some(16.0),
            font_weight: Some(FontWeight("600".to_string())),
            ..Default::default()
        },
    );
    builder.node(
        ElementKind::El,
        Attrs {
            width: Some(Length::Fill),
            padding: Some(Padding::Sides {
                top: 16.0,
                right: 24.0,
                bottom: 16.0,
                left: 24.0,
            }),
            border_color: Some(macaw_rgb(190, 194, 205)),
            background: Some(Background::Color(macaw_rgb(235, 238, 244))),
            ..Default::default()
        },
        vec![text],
    )
}

fn macaw_mode_row(builder: &mut MacawViewportBuilder, mode: &str, label: &str) -> NodeId {
    let icon = builder.node(
        ElementKind::El,
        Attrs {
            align_y: Some(AlignY::Center),
            width: Some(Length::Px(48.0)),
            height: Some(Length::Px(48.0)),
            background: Some(Background::Color(macaw_rgb(190, 194, 205))),
            border_radius: Some(BorderRadius::Uniform(24.0)),
            ..Default::default()
        },
        Vec::new(),
    );
    let label_text = builder.text(
        label,
        Attrs {
            font_size: Some(32.0),
            font_weight: Some(FontWeight("600".to_string())),
            ..Default::default()
        },
    );
    let label = builder.node(
        ElementKind::El,
        Attrs {
            width: Some(Length::Fill),
            align_y: Some(AlignY::Center),
            ..Default::default()
        },
        vec![label_text],
    );
    let selected = if mode == "cool" {
        builder.node(
            ElementKind::El,
            Attrs {
                align_y: Some(AlignY::Center),
                width: Some(Length::Px(40.0)),
                height: Some(Length::Px(40.0)),
                background: Some(Background::Color(macaw_rgb(26, 31, 39))),
                border_radius: Some(BorderRadius::Uniform(20.0)),
                ..Default::default()
            },
            Vec::new(),
        )
    } else {
        builder.node(ElementKind::None, Attrs::default(), Vec::new())
    };

    builder.node(
        ElementKind::Row,
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            padding: Some(Padding::Sides {
                top: 24.0,
                right: 64.0,
                bottom: 24.0,
                left: 24.0,
            }),
            spacing: Some(24.0),
            ..Default::default()
        },
        vec![icon, label, selected],
    )
}

fn macaw_separator(builder: &mut MacawViewportBuilder) -> NodeId {
    builder.node(
        ElementKind::El,
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Px(1.0)),
            background: Some(Background::Color(macaw_rgb(190, 194, 205))),
            ..Default::default()
        },
        Vec::new(),
    )
}

fn append_benchmark_subtree(tree: &mut ElementTree, subtree: &ElementTree) {
    let links: Vec<(NodeId, Vec<NodeId>, Vec<NearbyMount>)> = subtree
        .iter_node_pairs()
        .map(|(id, _)| (id, subtree.child_ids(&id), subtree.nearby_mounts_for(&id)))
        .collect();

    subtree.iter_nodes().cloned().for_each(|element| {
        tree.insert(element);
    });

    links.into_iter().for_each(|(id, children, nearby)| {
        if !children.is_empty() {
            tree.set_children(&id, children)
                .expect("benchmark subtree children should attach");
        }
        if !nearby.is_empty() {
            tree.set_nearby_mounts(&id, nearby)
                .expect("benchmark subtree nearby should attach");
        }
    });
}

fn macaw_move_x_spec(
    from_x: f64,
    to_x: f64,
    curve: AnimationCurve,
    repeat: AnimationRepeat,
) -> AnimationSpec {
    AnimationSpec {
        keyframes: vec![
            Attrs {
                move_x: Some(from_x),
                ..Default::default()
            },
            Attrs {
                move_x: Some(to_x),
                ..Default::default()
            },
        ],
        duration_ms: 125.0,
        curve,
        repeat,
    }
}

fn macaw_rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb { r, g, b }
}

fn macaw_rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::Rgba { r, g, b, a }
}

#[cfg(feature = "bench-diagnostics")]
fn sample_macaw_viewport_profiles() {
    let constraint = macaw_viewport_constraint();
    let (mut tree, mut cached_rebuild, host_id) = prepared_macaw_closed_viewport_tree(constraint);
    let mut runtime = AnimationRuntime::default();
    let started_at = Instant::now();
    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::InsertNearbySubtree {
            host_id,
            index: 0,
            slot: NearbySlot::InFront,
            subtree: macaw_sidepane_subtree(
                NodeId::from_u64(MACAW_VIEWPORT_SEED + 70_000),
                MACAW_VIEWPORT_SEED + 80_000,
                MacawSidepaneAnimationKind::EnterMoveX,
            ),
        }],
    )
    .expect("macaw diagnostic open patch should apply");
    runtime.sync_with_tree(&tree, started_at);
    let (update, profile) =
        layout_or_refresh_default_with_animation_and_invalidation_profile_for_benchmark(
            &mut tree,
            constraint,
            1.0,
            &runtime,
            started_at,
            invalidation,
            Some(&cached_rebuild),
        );
    if update.output.event_rebuild_changed {
        cached_rebuild = update.output.event_rebuild;
    }
    eprintln!(
        "macaw viewport open profile prepare={:.3}ms layout={:.3}ms refresh={:.3}ms traversal={:.3}ms registry_post={:.3}ms layout_performed={} event_rebuild_changed={} scene_nodes={} render_visits={} culled={} registry_visits={} registry_hits={} registry_stores={} registry_damaged={} registry_ineligible={} registry_misses={} pre_registry_damage={} registry_damage={} registry_damage_nodes={} summary={:?}",
        profile.prepare.as_secs_f64() * 1000.0,
        profile.layout.as_secs_f64() * 1000.0,
        profile.refresh.as_secs_f64() * 1000.0,
        profile.refresh_traversal.as_secs_f64() * 1000.0,
        profile.refresh_registry_post.as_secs_f64() * 1000.0,
        profile.layout_performed,
        update.output.event_rebuild_changed,
        profile.scene_nodes,
        profile.render_visits,
        profile.culled_subtrees,
        profile.registry_visits,
        profile.registry_cache_hits,
        profile.registry_cache_stores,
        profile.registry_cache_damaged,
        profile.registry_cache_ineligible,
        profile.registry_cache_misses,
        profile.pre_layout_registry_damage,
        profile.registry_damage,
        profile.registry_damage_nodes,
        update.output.scene.summary()
    );

    for frame in 1..=4_u64 {
        let sample_time =
            started_at + Duration::from_millis(frame.saturating_mul(SHOWCASE_FRAME_MS));
        let (update, profile) =
            layout_or_refresh_default_with_animation_and_invalidation_profile_for_benchmark(
                &mut tree,
                constraint,
                1.0,
                &runtime,
                sample_time,
                TreeInvalidation::None,
                Some(&cached_rebuild),
            );
        if update.output.event_rebuild_changed {
            cached_rebuild = update.output.event_rebuild;
        }
        eprintln!(
            "macaw viewport pulse profile frame={frame} prepare={:.3}ms layout={:.3}ms refresh={:.3}ms traversal={:.3}ms registry_post={:.3}ms layout_performed={} event_rebuild_changed={} scene_nodes={} render_visits={} culled={} registry_visits={} registry_hits={} registry_stores={} registry_damaged={} registry_ineligible={} registry_misses={} registry_damage={} registry_damage_nodes={} summary={:?}",
            profile.prepare.as_secs_f64() * 1000.0,
            profile.layout.as_secs_f64() * 1000.0,
            profile.refresh.as_secs_f64() * 1000.0,
            profile.refresh_traversal.as_secs_f64() * 1000.0,
            profile.refresh_registry_post.as_secs_f64() * 1000.0,
            profile.layout_performed,
            update.output.event_rebuild_changed,
            profile.scene_nodes,
            profile.render_visits,
            profile.culled_subtrees,
            profile.registry_visits,
            profile.registry_cache_hits,
            profile.registry_cache_stores,
            profile.registry_cache_damaged,
            profile.registry_cache_ineligible,
            profile.registry_cache_misses,
            profile.registry_damage,
            profile.registry_damage_nodes,
            update.output.scene.summary()
        );
    }
}

fn bench_sidepane_animation_smoothness(c: &mut Criterion) {
    let mut group = c.benchmark_group("native/sidepane_animation_smoothness/in_front_move_x");
    let constraint = sidepane_animation_constraint();
    let node_count = sidepane_animation_tree(None, 700_000).0.len() as u64;
    group.throughput(Throughput::Elements(node_count));

    let (host_only, cached_rebuild) = prepared_sidepane_host_only_tree(constraint);
    group.bench_function("enter_patch_first_frame", |b| {
        b.iter_batched(
            || (host_only.clone(), cached_rebuild.clone()),
            |(mut tree, cached_rebuild)| {
                let mut runtime = AnimationRuntime::default();
                let started_at = Instant::now();
                let invalidation = apply_patches(
                    &mut tree,
                    vec![Patch::InsertNearbySubtree {
                        host_id: sidepane_host_id(),
                        index: 0,
                        slot: NearbySlot::InFront,
                        subtree: sidepane_subtree(
                            710_000,
                            SidepaneAnimationKind::EnterMoveX,
                        ),
                    }],
                )
                .expect("sidepane enter patch should apply");
                runtime.sync_with_tree(&tree, started_at);
                let update = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
                    &mut tree,
                    constraint,
                    1.0,
                    &runtime,
                    started_at,
                    invalidation,
                    Some(&cached_rebuild),
                );
                consume_layout_update_output(update)
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("move_x_pulse_retained_payload", |b| {
        let mut state = SidepaneAnimationPulseState::new(720_000, false);
        b.iter(|| consume_layout_update_output(state.next_frame()));
    });

    group.bench_function("move_x_pulse_content_dirty_control", |b| {
        let mut state = SidepaneAnimationPulseState::new(730_000, true);
        b.iter(|| consume_layout_update_output(state.next_frame()));
    });

    group.finish();
}

#[derive(Clone, Copy)]
enum SidepaneAnimationKind {
    EnterMoveX,
    LoopMoveX,
}

struct SidepaneAnimationPulseState {
    tree: ElementTree,
    runtime: AnimationRuntime,
    cached_rebuild: RegistryRebuildPayload,
    started_at: Instant,
    sidepane_id: NodeId,
    force_content_dirty: bool,
    frame: u64,
}

impl SidepaneAnimationPulseState {
    fn new(seed: u64, force_content_dirty: bool) -> Self {
        let constraint = sidepane_animation_constraint();
        let started_at = Instant::now();
        let (mut tree, _) = sidepane_animation_tree(Some(SidepaneAnimationKind::LoopMoveX), seed);
        let sidepane_id = NodeId::from_u64(seed + 10);
        let mut runtime = AnimationRuntime::default();
        runtime.sync_with_tree(&tree, started_at);
        let output = layout_and_refresh_default_with_animation(
            &mut tree, constraint, 1.0, &runtime, started_at,
        );
        let mut cached_rebuild = output.event_rebuild;

        // Warm one clean pulse so the retained moving payload cache is populated
        // before measuring steady-state transform-only frames.
        let warm = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut tree,
            constraint,
            1.0,
            &runtime,
            started_at + Duration::from_millis(16),
            TreeInvalidation::None,
            Some(&cached_rebuild),
        );
        if warm.output.event_rebuild_changed {
            cached_rebuild = warm.output.event_rebuild;
        }

        Self {
            tree,
            runtime,
            cached_rebuild,
            started_at,
            sidepane_id,
            force_content_dirty,
            frame: 1,
        }
    }

    fn next_frame(&mut self) -> emerge_skia::tree::layout::LayoutUpdateOutput {
        self.frame = self.frame.saturating_add(1);
        let sample_time =
            self.started_at + Duration::from_millis(self.frame.saturating_mul(SHOWCASE_FRAME_MS));
        let invalidation = if self.force_content_dirty {
            self.tree
                .mark_refresh_dirty_for_invalidation(&self.sidepane_id, TreeInvalidation::Paint);
            TreeInvalidation::Paint
        } else {
            TreeInvalidation::None
        };
        let update = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
            &mut self.tree,
            sidepane_animation_constraint(),
            1.0,
            &self.runtime,
            sample_time,
            invalidation,
            Some(&self.cached_rebuild),
        );
        if update.output.event_rebuild_changed {
            self.cached_rebuild = update.output.event_rebuild.clone();
        }
        update
    }
}

fn prepared_sidepane_host_only_tree(
    constraint: Constraint,
) -> (ElementTree, RegistryRebuildPayload) {
    let (mut tree, _) = sidepane_animation_tree(None, 700_000);
    let output = layout_and_refresh_default(&mut tree, constraint, 1.0);
    (tree, output.event_rebuild)
}

fn sidepane_animation_constraint() -> Constraint {
    Constraint::new(1024.0, 600.0)
}

fn sidepane_root_id() -> NodeId {
    NodeId::from_u64(700_001)
}

fn sidepane_host_id() -> NodeId {
    NodeId::from_u64(700_002)
}

fn sidepane_animation_tree(
    sidepane_kind: Option<SidepaneAnimationKind>,
    seed: u64,
) -> (ElementTree, NodeId) {
    let mut tree = ElementTree::new();
    let root_id = sidepane_root_id();
    let host_id = sidepane_host_id();
    let root = Element::with_attrs(
        root_id,
        ElementKind::Column,
        Vec::new(),
        Attrs {
            width: Some(Length::Px(1024.0)),
            height: Some(Length::Px(600.0)),
            padding: Some(Padding::Uniform(12.0)),
            spacing: Some(8.0),
            ..Default::default()
        },
    );

    let card_ids: Vec<NodeId> = (0..96)
        .map(|index| {
            let card_id = NodeId::from_u64(seed + 1_000 + index as u64);
            let text_id = NodeId::from_u64(seed + 2_000 + index as u64);
            tree.insert(Element::with_attrs(
                card_id,
                ElementKind::El,
                Vec::new(),
                Attrs {
                    width: Some(Length::Px(300.0)),
                    height: Some(Length::Px(38.0)),
                    padding: Some(Padding::Uniform(8.0)),
                    background: Some(Background::Color(Color::Rgba {
                        r: 24,
                        g: 28,
                        b: 34,
                        a: 255,
                    })),
                    ..Default::default()
                },
            ));
            tree.insert(Element::with_attrs(
                text_id,
                ElementKind::Text,
                Vec::new(),
                Attrs {
                    content: Some(format!("Device status row {index}")),
                    font_size: Some(13.0),
                    font_color: Some(Color::Rgba {
                        r: 230,
                        g: 235,
                        b: 240,
                        a: 255,
                    }),
                    ..Default::default()
                },
            ));
            tree.set_children(&card_id, vec![text_id])
                .expect("card text should attach");
            card_id
        })
        .collect();

    let host = Element::with_attrs(
        host_id,
        ElementKind::El,
        Vec::new(),
        Attrs {
            width: Some(Length::Px(1024.0)),
            height: Some(Length::Px(600.0)),
            on_mouse_move: Some(true),
            ..Default::default()
        },
    );

    tree.insert(root);
    tree.insert(host);
    tree.set_children(&root_id, std::iter::once(host_id).chain(card_ids).collect())
        .expect("root children should attach");

    if let Some(kind) = sidepane_kind {
        let sidepane_id = NodeId::from_u64(seed + 10);
        let subtree = sidepane_subtree_with_id(sidepane_id, seed + 20, kind);
        for element in subtree.iter_nodes().cloned() {
            tree.insert(element);
        }
        tree.set_children(&sidepane_id, subtree.child_ids(&sidepane_id))
            .expect("sidepane children should attach");
        tree.set_nearby_mounts(
            &host_id,
            vec![NearbyMount {
                slot: NearbySlot::InFront,
                id: sidepane_id,
            }],
        )
        .expect("sidepane nearby should attach");
    }

    tree.set_root_id(root_id);
    (tree, host_id)
}

fn sidepane_subtree(seed: u64, kind: SidepaneAnimationKind) -> ElementTree {
    sidepane_subtree_with_id(NodeId::from_u64(seed), seed + 1, kind)
}

fn sidepane_subtree_with_id(
    root_id: NodeId,
    seed: u64,
    kind: SidepaneAnimationKind,
) -> ElementTree {
    let mut tree = ElementTree::new();
    let mut attrs = Attrs {
        width: Some(Length::Px(500.0)),
        height: Some(Length::Px(600.0)),
        padding: Some(Padding::Uniform(18.0)),
        spacing: Some(6.0),
        background: Some(Background::Color(Color::Rgba {
            r: 12,
            g: 18,
            b: 24,
            a: 255,
        })),
        on_mouse_move: Some(true),
        ..Default::default()
    };
    match kind {
        SidepaneAnimationKind::EnterMoveX => {
            attrs.animate_enter = Some(sidepane_move_x_spec(
                500.0,
                0.0,
                AnimationCurve::EaseIn,
                AnimationRepeat::Once,
            ));
        }
        SidepaneAnimationKind::LoopMoveX => {
            attrs.animate = Some(sidepane_move_x_spec(
                500.0,
                0.0,
                AnimationCurve::EaseIn,
                AnimationRepeat::Loop,
            ));
        }
    }

    tree.set_root_id(root_id);
    tree.insert(Element::with_attrs(
        root_id,
        ElementKind::Column,
        Vec::new(),
        attrs,
    ));
    let child_ids: Vec<NodeId> = (0..24)
        .map(|index| {
            let id = NodeId::from_u64(seed + index as u64);
            tree.insert(Element::with_attrs(
                id,
                ElementKind::Text,
                Vec::new(),
                Attrs {
                    content: Some(format!("Climate setting {index}: active")),
                    font_size: Some(14.0),
                    font_color: Some(Color::Rgba {
                        r: 240,
                        g: 244,
                        b: 248,
                        a: 255,
                    }),
                    ..Default::default()
                },
            ));
            id
        })
        .collect();
    tree.set_children(&root_id, child_ids)
        .expect("sidepane rows should attach");
    tree
}

fn sidepane_move_x_spec(
    from_x: f64,
    to_x: f64,
    curve: AnimationCurve,
    repeat: AnimationRepeat,
) -> AnimationSpec {
    AnimationSpec {
        keyframes: vec![
            Attrs {
                move_x: Some(from_x),
                ..Default::default()
            },
            Attrs {
                move_x: Some(to_x),
                ..Default::default()
            },
        ],
        duration_ms: 125.0,
        curve,
        repeat,
    }
}

fn bench_nearby_hover_toggle_refresh(c: &mut Criterion) {
    let mut group = c.benchmark_group("native/nearby_hover_toggle_refresh/borders_like");
    let constraint = Constraint::new(960.0, 4_000.0);
    let host_id = NodeId::from_u64(2);

    let (hidden_with_detached, cached_rebuild) = prepared_hidden_nearby_hover_tree(constraint);
    group.bench_function("restored_show_refresh_only", |b| {
        b.iter_batched(
            || (hidden_with_detached.clone(), cached_rebuild.clone()),
            |(mut tree, cached_rebuild)| {
                let hidden_id = current_nearby_id(&tree, host_id);
                let invalidation = apply_patches(
                    &mut tree,
                    vec![
                        Patch::Remove { id: hidden_id },
                        Patch::InsertNearbySubtree {
                            host_id,
                            index: 0,
                            slot: NearbySlot::Above,
                            subtree: nearby_code_block_subtree(50_000),
                        },
                    ],
                )
                .expect("restored nearby show patch should apply");
                debug_assert_eq!(invalidation, TreeInvalidation::Paint);
                let output =
                    refresh_reusing_clean_registry_for_benchmark(&mut tree, Some(&cached_rebuild));
                consume_layout_output(output)
            },
            BatchSize::SmallInput,
        );
    });

    let (held_visible, held_cached_rebuild) = prepared_held_nearby_hover_tree(constraint);
    group.bench_function("held_show_refresh_only", |b| {
        b.iter_batched(
            || (held_visible.clone(), held_cached_rebuild.clone()),
            |(mut tree, cached_rebuild)| {
                let output =
                    refresh_reusing_clean_registry_for_benchmark(&mut tree, Some(&cached_rebuild));
                consume_layout_output(output)
            },
            BatchSize::SmallInput,
        );
    });

    let (cold_hidden, cold_cached_rebuild) = cold_hidden_nearby_hover_tree(constraint);
    group.bench_function("cold_show_layout_refresh", |b| {
        b.iter_batched(
            || (cold_hidden.clone(), cold_cached_rebuild.clone()),
            |(mut tree, _cached_rebuild)| {
                let hidden_id = current_nearby_id(&tree, host_id);
                let invalidation = apply_patches(
                    &mut tree,
                    vec![
                        Patch::Remove { id: hidden_id },
                        Patch::InsertNearbySubtree {
                            host_id,
                            index: 0,
                            slot: NearbySlot::Above,
                            subtree: nearby_code_block_subtree(60_000),
                        },
                    ],
                )
                .expect("cold nearby show patch should apply");
                debug_assert_eq!(invalidation, TreeInvalidation::Resolve);
                let output = layout_and_refresh_default(&mut tree, constraint, 1.0);
                consume_layout_output(output)
            },
            BatchSize::SmallInput,
        );
    });

    let (reused_hidden, reused_overlay_id, reused_code_ids) =
        reused_node_hidden_nearby_hover_tree(constraint);
    group.bench_function("reused_node_show_layout_refresh", |b| {
        b.iter_batched(
            || (reused_hidden.clone(), reused_code_ids.clone()),
            |(mut tree, code_ids)| {
                let invalidation = apply_patches(
                    &mut tree,
                    vec![Patch::SetChildren {
                        id: reused_overlay_id,
                        children: code_ids,
                    }],
                )
                .expect("reused-node nearby show patch should apply");
                debug_assert_eq!(invalidation, TreeInvalidation::Resolve);
                let output = layout_and_refresh_default(&mut tree, constraint, 1.0);
                consume_layout_output(output)
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn prepared_held_nearby_hover_tree(
    constraint: Constraint,
) -> (ElementTree, RegistryRebuildPayload) {
    let (mut tree, host_id) = cold_hidden_nearby_hover_tree_base(90_000);
    let _ = layout_and_refresh_default(&mut tree, constraint, 1.0);

    let hidden_id = current_nearby_id(&tree, host_id);
    let invalidation = apply_patches(
        &mut tree,
        vec![
            Patch::Remove { id: hidden_id },
            Patch::InsertNearbySubtree {
                host_id,
                index: 0,
                slot: NearbySlot::Above,
                subtree: nearby_code_block_subtree(91_000),
            },
        ],
    )
    .expect("held nearby show patch should apply");
    debug_assert_eq!(invalidation, TreeInvalidation::Resolve);
    let output = layout_and_refresh_default(&mut tree, constraint, 1.0);
    let cached_rebuild = output.event_rebuild;
    let _ = refresh_reusing_clean_registry_for_benchmark(&mut tree, Some(&cached_rebuild));

    (tree, cached_rebuild)
}

fn prepared_hidden_nearby_hover_tree(
    constraint: Constraint,
) -> (ElementTree, RegistryRebuildPayload) {
    let (mut tree, host_id) = cold_hidden_nearby_hover_tree_base(10_000);
    let _ = layout_and_refresh_default(&mut tree, constraint, 1.0);
    let mut cached_rebuild;

    let hidden_id = current_nearby_id(&tree, host_id);
    let invalidation = apply_patches(
        &mut tree,
        vec![
            Patch::Remove { id: hidden_id },
            Patch::InsertNearbySubtree {
                host_id,
                index: 0,
                slot: NearbySlot::Above,
                subtree: nearby_code_block_subtree(20_000),
            },
        ],
    )
    .expect("cold show should apply");
    debug_assert_eq!(invalidation, TreeInvalidation::Resolve);
    let output = layout_and_refresh_default(&mut tree, constraint, 1.0);
    cached_rebuild = output.event_rebuild;

    let code_id = current_nearby_id(&tree, host_id);
    let invalidation = apply_patches(
        &mut tree,
        vec![
            Patch::Remove { id: code_id },
            Patch::InsertNearbySubtree {
                host_id,
                index: 0,
                slot: NearbySlot::Above,
                subtree: nearby_none_subtree(30_000),
            },
        ],
    )
    .expect("hide should apply");
    debug_assert_eq!(invalidation, TreeInvalidation::Paint);
    let output = refresh_reusing_clean_registry_for_benchmark(&mut tree, Some(&cached_rebuild));
    if output.event_rebuild_changed {
        cached_rebuild = output.event_rebuild;
    }

    (tree, cached_rebuild)
}

fn cold_hidden_nearby_hover_tree(constraint: Constraint) -> (ElementTree, RegistryRebuildPayload) {
    let (mut tree, _host_id) = cold_hidden_nearby_hover_tree_base(40_000);
    let output = layout_and_refresh_default(&mut tree, constraint, 1.0);
    (tree, output.event_rebuild)
}

fn reused_node_hidden_nearby_hover_tree(
    constraint: Constraint,
) -> (ElementTree, NodeId, Vec<NodeId>) {
    let (mut tree, host_id) = cold_hidden_nearby_hover_tree_base(70_000);
    let hidden_id = current_nearby_id(&tree, host_id);
    let overlay_attrs = Attrs {
        width: Some(Length::Px(460.0)),
        padding: Some(Padding::Uniform(12.0)),
        spacing: Some(4.0),
        ..Default::default()
    };
    if let Some(hidden) = tree.get_mut(&hidden_id) {
        hidden.spec.kind = ElementKind::Column;
        hidden.spec.declared = overlay_attrs.clone();
        hidden.layout.effective = overlay_attrs;
    }

    let code_ids = insert_detached_code_line_nodes(&mut tree, 80_000);
    let _ = layout_and_refresh_default(&mut tree, constraint, 1.0);

    (tree, hidden_id, code_ids)
}

fn cold_hidden_nearby_hover_tree_base(hidden_seed: u64) -> (ElementTree, NodeId) {
    let mut tree = ElementTree::new();
    let root_id = NodeId::from_u64(1);
    let host_id = NodeId::from_u64(2);
    let hidden_id = NodeId::from_u64(hidden_seed);

    let root_attrs = Attrs {
        width: Some(Length::Px(920.0)),
        spacing: Some(8.0),
        ..Default::default()
    };
    tree.set_root_id(root_id);
    tree.insert(Element::with_attrs(
        root_id,
        ElementKind::Column,
        Vec::new(),
        root_attrs,
    ));

    let child_ids: Vec<NodeId> = std::iter::once(host_id)
        .chain((0..72).map(|index| {
            let card_id = NodeId::from_u64(1_000 + index as u64);
            let text_id = NodeId::from_u64(2_000 + index as u64);

            let card_attrs = Attrs {
                width: Some(Length::Px(280.0)),
                padding: Some(Padding::Uniform(10.0)),
                ..Default::default()
            };
            let text_attrs = Attrs {
                content: Some(format!("Border recipe card {index}")),
                font_size: Some(13.0),
                ..Default::default()
            };

            tree.insert(Element::with_attrs(
                card_id,
                ElementKind::El,
                Vec::new(),
                card_attrs,
            ));
            tree.insert(Element::with_attrs(
                text_id,
                ElementKind::Text,
                Vec::new(),
                text_attrs,
            ));
            tree.set_children(&card_id, vec![text_id])
                .expect("card text should exist");
            card_id
        }))
        .collect();

    let host_attrs = Attrs {
        width: Some(Length::Px(360.0)),
        padding: Some(Padding::Uniform(12.0)),
        on_mouse_enter: Some(true),
        on_mouse_leave: Some(true),
        ..Default::default()
    };
    tree.insert(Element::with_attrs(
        host_id,
        ElementKind::El,
        Vec::new(),
        host_attrs,
    ));
    tree.insert(Element::with_attrs(
        hidden_id,
        ElementKind::None,
        Vec::new(),
        Attrs::default(),
    ));
    tree.set_nearby_mounts(
        &host_id,
        vec![NearbyMount {
            slot: NearbySlot::Above,
            id: hidden_id,
        }],
    )
    .expect("hidden nearby should attach");
    tree.set_children(&root_id, child_ids)
        .expect("root children should exist");

    (tree, host_id)
}

fn nearby_code_block_subtree(seed: u64) -> ElementTree {
    let mut tree = ElementTree::new();
    let root_id = NodeId::from_u64(seed);
    let root_attrs = Attrs {
        width: Some(Length::Px(460.0)),
        padding: Some(Padding::Uniform(12.0)),
        spacing: Some(4.0),
        ..Default::default()
    };
    tree.set_root_id(root_id);
    tree.insert(Element::with_attrs(
        root_id,
        ElementKind::Column,
        Vec::new(),
        root_attrs,
    ));

    let lines = [
        "Code",
        "el([",
        "  Border.rounded(8),",
        "  Border.width(2),",
        "  Border.color(:orange),",
        "  Border.dashed()",
        "], text(\"Dashed medium round\"))",
    ];
    let child_ids: Vec<NodeId> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let id = NodeId::from_u64(seed + 1 + index as u64);
            let attrs = Attrs {
                content: Some((*line).to_string()),
                font_size: Some(if index == 0 { 11.0 } else { 12.0 }),
                ..Default::default()
            };
            tree.insert(Element::with_attrs(
                id,
                ElementKind::Text,
                Vec::new(),
                attrs,
            ));
            id
        })
        .collect();
    tree.set_children(&root_id, child_ids)
        .expect("code lines should attach");
    tree
}

fn nearby_none_subtree(seed: u64) -> ElementTree {
    let mut tree = ElementTree::new();
    let root_id = NodeId::from_u64(seed);
    tree.set_root_id(root_id);
    tree.insert(Element::with_attrs(
        root_id,
        ElementKind::None,
        Vec::new(),
        Attrs::default(),
    ));
    tree
}

fn insert_detached_code_line_nodes(tree: &mut ElementTree, seed: u64) -> Vec<NodeId> {
    [
        "Code",
        "el([",
        "  Border.rounded(8),",
        "  Border.width(2),",
        "  Border.color(:orange),",
        "  Border.dashed()",
        "], text(\"Dashed medium round\"))",
    ]
    .iter()
    .enumerate()
    .map(|(index, line)| {
        let id = NodeId::from_u64(seed + index as u64);
        let attrs = Attrs {
            content: Some((*line).to_string()),
            font_size: Some(if index == 0 { 11.0 } else { 12.0 }),
            ..Default::default()
        };
        tree.insert(Element::with_attrs(
            id,
            ElementKind::Text,
            Vec::new(),
            attrs,
        ));
        id
    })
    .collect()
}

fn current_nearby_id(tree: &ElementTree, host_id: NodeId) -> NodeId {
    tree.nearby_mounts_for(&host_id)
        .first()
        .expect("host should have a nearby mount")
        .id
}

fn bench_layout(c: &mut Criterion) {
    let asset_runtime = AssetRuntime::new();
    let _asset_context_guard = asset_runtime.enter();

    bench_large_text_column(c);
    bench_nested_card_grid(c);
    bench_large_text_column_retained(c);
    bench_nested_card_grid_retained(c);
    bench_layout_aware_transform(c);
    bench_layout_aware_transform_animation(c);
    bench_animated_shadow_showcase(c);
    bench_rich_borders_shadow_showcase(c);
    bench_scrolling_animated_shadow_showcase(c);
    bench_scrolling_rich_borders_shadow_showcase(c);
    bench_emerge_demo_showcase_layout_refresh(c);
    bench_scroll_viewport_culling(c);
    bench_fixture_retained_layout_after_patch(c);
    bench_fixture_retained_patch_layout(c);
    bench_render_refresh_cache_regression(c);
    bench_registry_refresh_cache_regression(c);
    bench_macaw_viewport_refresh(c);
    bench_sidepane_animation_smoothness(c);
    bench_nearby_hover_toggle_refresh(c);
}

criterion_group!(benches, bench_layout);
criterion_main!(benches);
