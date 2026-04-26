mod support;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use emerge_skia::events::{
    RegistryRebuildPayload,
    registry_builder::{
        build_registry_rebuild_cached_for_benchmark, build_registry_rebuild_for_benchmark,
    },
};
use emerge_skia::tree::animation::AnimationRuntime;
use emerge_skia::tree::attrs::{Attrs, Length, Padding};
use emerge_skia::tree::deserialize::decode_tree;
use emerge_skia::tree::element::{
    Element, ElementKind, ElementTree, NearbyMount, NearbySlot, NodeId,
};
use emerge_skia::tree::invalidation::TreeInvalidation;
use emerge_skia::tree::layout::{
    Constraint, layout_and_refresh_default, layout_and_refresh_default_uncached_for_benchmark,
    layout_and_refresh_default_with_animation, layout_or_refresh_default_with_animation,
    layout_or_refresh_default_with_animation_uncached_for_benchmark, layout_tree,
    layout_tree_default, refresh, refresh_reusing_clean_registry_for_benchmark,
    refresh_uncached_reusing_clean_registry_for_benchmark,
};
use emerge_skia::tree::patch::{Patch, apply_patches, decode_patches};
use std::hint::black_box;
use std::time::{Duration, Instant};
use support::{
    CARD_COUNT, MockTextMeasurer, TEXT_ROW_COUNT, animated_shadow_showcase, large_text_column,
    load_fixture, nested_card_grid, scrollable_animated_shadow_showcase,
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

// Apply each patch during setup so the timed body is the first layout after invalidation.
fn bench_animated_shadow_showcase(c: &mut Criterion) {
    let mut group = c.benchmark_group("native/layout_animation_paint_only/shadow_showcase");
    let constraint = Constraint::new(960.0, 4_000.0);
    let start = Instant::now();
    let node_count = animated_shadow_showcase().len() as u64;
    group.throughput(Throughput::Elements(node_count));

    let mut full_tree = animated_shadow_showcase();
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

    let mut refresh_tree = animated_shadow_showcase();
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
    let mut group = c.benchmark_group("native/layout_scroll_paint_only_animation/shadow_showcase");
    let constraint = Constraint::new(960.0, 640.0);
    let start = Instant::now();
    let node_count = scrollable_animated_shadow_showcase().len() as u64;
    group.throughput(Throughput::Elements(node_count));

    let mut full_tree = scrollable_animated_shadow_showcase();
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
            let delta = if full_tick % 32 == 0 { 8.0 } else { -8.0 };
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

    let mut refresh_tree = scrollable_animated_shadow_showcase();
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
            let delta = if refresh_tick % 32 == 0 { 8.0 } else { -8.0 };
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

fn bench_fixture_retained_layout_after_patch(c: &mut Criterion) {
    let constraint = Constraint::new(960.0, 4_000.0);

    for fixture_id in RETAINED_FIXTURE_IDS {
        let fixture = load_fixture(fixture_id);
        let base_tree = decode_tree(&fixture.full_emrg).expect("fixture tree should decode");
        let node_count = base_tree.len() as u64;
        let mut warmed_base = base_tree.clone();
        layout_tree_default(&mut warmed_base, constraint, 1.0);

        let mut group =
            c.benchmark_group(format!("native/layout_retained_after_patch/{}", fixture.id));
        group.throughput(Throughput::Elements(node_count));

        for mutation in RETAINED_MUTATIONS {
            let decoded_patches =
                decode_patches(fixture.patch_bytes(mutation)).expect("fixture patch should decode");

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

fn bench_fixture_retained_patch_layout(c: &mut Criterion) {
    let constraint = Constraint::new(960.0, 4_000.0);

    for fixture_id in RETAINED_FIXTURE_IDS {
        let fixture = load_fixture(fixture_id);
        let base_tree = decode_tree(&fixture.full_emrg).expect("fixture tree should decode");
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
        let base_tree = decode_tree(&fixture.full_emrg).expect("fixture tree should decode");
        let node_count = base_tree.len() as u64;
        let mut warmed_base = base_tree;
        let warm_output = layout_and_refresh_default(&mut warmed_base, constraint, 1.0);
        let cached_rebuild = warm_output.event_rebuild;
        let decoded_patches =
            decode_patches(fixture.patch_bytes(mutation)).expect("fixture patch should decode");
        let patch_bytes = fixture.patch_bytes(mutation).to_vec();
        let case = format!("{fixture_id}/{mutation}");

        group.throughput(Throughput::Elements(node_count));
        bench_cold_layout_refresh_pair(&mut group, &case, &fixture.full_emrg, constraint);
        bench_warm_refresh_pair(
            &mut group,
            &case,
            warmed_base.clone(),
            cached_rebuild.clone(),
        );
        bench_after_patch_refresh_pair(
            &mut group,
            &case,
            warmed_base.clone(),
            cached_rebuild.clone(),
            decoded_patches,
            constraint,
        );
        bench_patch_refresh_pair(
            &mut group,
            &case,
            warmed_base,
            cached_rebuild,
            patch_bytes,
            constraint,
        );
    }

    bench_animation_refresh_regression_pair(
        &mut group,
        "animated_shadow_showcase/paint_only_refresh_each_frame",
        Constraint::new(960.0, 4_000.0),
        animated_shadow_showcase,
        false,
    );
    bench_animation_refresh_regression_pair(
        &mut group,
        "scroll_shadow_showcase/paint_only_refresh_scroll_frame",
        Constraint::new(960.0, 640.0),
        scrollable_animated_shadow_showcase,
        true,
    );

    group.finish();
}

fn bench_cold_layout_refresh_pair(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    case: &str,
    full_emrg: &[u8],
    constraint: Constraint,
) {
    let full_bytes = full_emrg.to_vec();
    group.bench_function(format!("{case}/cold_cached_layout_refresh"), move |b| {
        b.iter_batched(
            || decode_tree(&full_bytes).expect("fixture tree should decode"),
            |mut tree| {
                let output = layout_and_refresh_default(&mut tree, constraint, 1.0);
                consume_layout_output(output)
            },
            BatchSize::SmallInput,
        );
    });

    let full_bytes = full_emrg.to_vec();
    group.bench_function(format!("{case}/cold_uncached_layout_refresh"), move |b| {
        b.iter_batched(
            || decode_tree(&full_bytes).expect("fixture tree should decode"),
            |mut tree| {
                let output =
                    layout_and_refresh_default_uncached_for_benchmark(&mut tree, constraint, 1.0);
                consume_layout_output(output)
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_warm_refresh_pair(
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

    let mut uncached_tree = warmed_base;
    group.bench_function(format!("{case}/uncached_refresh"), move |b| {
        b.iter(|| {
            let output = refresh_uncached_reusing_clean_registry_for_benchmark(
                &mut uncached_tree,
                Some(&cached_rebuild),
            );
            consume_layout_output(output)
        });
    });
}

fn bench_after_patch_refresh_pair(
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

    group.bench_function(format!("{case}/after_patch_uncached_refresh"), move |b| {
        b.iter_batched(
            || prepare_after_patch_refresh_tree(&warmed_base, &decoded_patches, constraint),
            |mut tree| {
                let output = refresh_uncached_reusing_clean_registry_for_benchmark(
                    &mut tree,
                    Some(&cached_rebuild),
                );
                consume_layout_output(output)
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_patch_refresh_pair(
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

    group.bench_function(format!("{case}/patch_uncached_refresh"), move |b| {
        b.iter_batched(
            || (warmed_base.clone(), patch_bytes.clone()),
            |(mut tree, bytes)| {
                apply_patch_and_relayout_if_needed(&mut tree, &bytes, constraint);
                let output = refresh_uncached_reusing_clean_registry_for_benchmark(
                    &mut tree,
                    Some(&cached_rebuild),
                );
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

fn bench_animation_refresh_regression_pair(
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
                let delta = if cached_tick % 32 == 0 { 8.0 } else { -8.0 };
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

    let mut uncached_tree = make_tree();
    let uncached_root_id = uncached_tree.root_id();
    let mut uncached_runtime = AnimationRuntime::default();
    uncached_runtime.sync_with_tree(&uncached_tree, start);
    layout_and_refresh_default_with_animation(
        &mut uncached_tree,
        constraint,
        1.0,
        &uncached_runtime,
        start,
    );
    let mut uncached_tick = 0_u64;
    group.bench_function(format!("{case}/uncached_refresh"), move |b| {
        b.iter(|| {
            uncached_tick += 16;
            if scroll_each_frame {
                let delta = if uncached_tick % 32 == 0 { 8.0 } else { -8.0 };
                if let Some(root_id) = uncached_root_id {
                    black_box(uncached_tree.apply_scroll_y(&root_id, delta));
                }
            }
            let update = layout_or_refresh_default_with_animation_uncached_for_benchmark(
                &mut uncached_tree,
                constraint,
                1.0,
                &uncached_runtime,
                start + Duration::from_millis(uncached_tick),
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
        let base_tree = decode_tree(&fixture.full_emrg).expect("fixture tree should decode");
        let node_count = base_tree.len() as u64;
        let mut warmed_base = base_tree;
        let warm_output = layout_and_refresh_default(&mut warmed_base, constraint, 1.0);
        let cached_rebuild = warm_output.event_rebuild;
        let decoded_patches =
            decode_patches(fixture.patch_bytes(mutation)).expect("fixture patch should decode");
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

    group.finish();
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

fn cold_hidden_nearby_hover_tree_base(hidden_seed: u64) -> (ElementTree, NodeId) {
    let mut tree = ElementTree::new();
    let root_id = NodeId::from_u64(1);
    let host_id = NodeId::from_u64(2);
    let hidden_id = NodeId::from_u64(hidden_seed);

    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Px(920.0));
    root_attrs.spacing = Some(8.0);
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

            let mut card_attrs = Attrs::default();
            card_attrs.width = Some(Length::Px(280.0));
            card_attrs.padding = Some(Padding::Uniform(10.0));
            let mut text_attrs = Attrs::default();
            text_attrs.content = Some(format!("Border recipe card {index}"));
            text_attrs.font_size = Some(13.0);

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

    let mut host_attrs = Attrs::default();
    host_attrs.width = Some(Length::Px(360.0));
    host_attrs.padding = Some(Padding::Uniform(12.0));
    host_attrs.on_mouse_enter = Some(true);
    host_attrs.on_mouse_leave = Some(true);
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
    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Px(460.0));
    root_attrs.padding = Some(Padding::Uniform(12.0));
    root_attrs.spacing = Some(4.0);
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
            let mut attrs = Attrs::default();
            attrs.content = Some((*line).to_string());
            attrs.font_size = Some(if index == 0 { 11.0 } else { 12.0 });
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

fn current_nearby_id(tree: &ElementTree, host_id: NodeId) -> NodeId {
    tree.nearby_mounts_for(&host_id)
        .first()
        .expect("host should have a nearby mount")
        .id
}

criterion_group!(
    benches,
    bench_large_text_column,
    bench_nested_card_grid,
    bench_large_text_column_retained,
    bench_nested_card_grid_retained,
    bench_animated_shadow_showcase,
    bench_scrolling_animated_shadow_showcase,
    bench_fixture_retained_layout_after_patch,
    bench_fixture_retained_patch_layout,
    bench_render_refresh_cache_regression,
    bench_registry_refresh_cache_regression,
    bench_nearby_hover_toggle_refresh
);
criterion_main!(benches);
