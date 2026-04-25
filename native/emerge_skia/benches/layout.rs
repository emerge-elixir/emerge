mod support;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use emerge_skia::tree::layout::{
    Constraint, layout_and_refresh_default, layout_tree, layout_tree_default, refresh,
};
use std::hint::black_box;
use support::{CARD_COUNT, MockTextMeasurer, TEXT_ROW_COUNT, large_text_column, nested_card_grid};

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

criterion_group!(benches, bench_large_text_column, bench_nested_card_grid);
criterion_main!(benches);
