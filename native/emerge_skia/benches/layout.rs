mod support;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use emerge_skia::tree::deserialize::decode_tree;
use emerge_skia::tree::layout::{
    Constraint, layout_and_refresh_default, layout_tree, layout_tree_default, refresh,
};
use emerge_skia::tree::patch::{apply_patches, decode_patches};
use std::hint::black_box;
use support::{
    CARD_COUNT, MockTextMeasurer, TEXT_ROW_COUNT, large_text_column, load_fixture, nested_card_grid,
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

criterion_group!(
    benches,
    bench_large_text_column,
    bench_nested_card_grid,
    bench_large_text_column_retained,
    bench_nested_card_grid_retained,
    bench_fixture_retained_layout_after_patch,
    bench_fixture_retained_patch_layout
);
criterion_main!(benches);
