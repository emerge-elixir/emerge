use criterion::{Criterion, criterion_group, criterion_main};
use emerge_skia::renderer::{RendererCacheFrameStats, RendererCacheKindFrameStats};
use emerge_skia::stats::RendererStatsCollector;
use std::hint::black_box;
use std::time::{Duration, Instant};

fn bench_stats_collector(c: &mut Criterion) {
    let mut group = c.benchmark_group("native/stats/collector");

    group.bench_function("record_single_timing", |b| {
        let stats = RendererStatsCollector::new();
        let duration = Duration::from_micros(750);
        b.iter(|| stats.record_render(black_box(duration)));
    });

    group.bench_function("record_pipeline_sequence", |b| {
        let stats = RendererStatsCollector::new();
        let submitted_at = Instant::now();
        let tree_started_at = submitted_at + Duration::from_micros(200);
        let render_queued_at = submitted_at + Duration::from_micros(1_700);
        let render_received_at = submitted_at + Duration::from_micros(1_800);
        let swap_done_at = submitted_at + Duration::from_micros(3_400);
        let presented_at = submitted_at + Duration::from_micros(16_700);

        b.iter(|| {
            stats.record_pipeline_submit_to_tree_start(
                black_box(submitted_at),
                black_box(tree_started_at),
            );
            stats.record_pipeline_tree(black_box(tree_started_at), black_box(render_queued_at));
            stats.record_pipeline_render_queue(
                black_box(render_queued_at),
                black_box(render_received_at),
            );
            stats.record_pipeline_submit_to_swap(black_box(submitted_at), black_box(swap_done_at));
            stats.record_pipeline_swap_to_frame_callback(
                black_box(swap_done_at),
                black_box(presented_at),
            );
            stats.record_pipeline(black_box(submitted_at), black_box(presented_at));
        });
    });

    group.bench_function("snapshot_populated", |b| {
        let stats = RendererStatsCollector::new();
        let duration = Duration::from_micros(750);
        let submitted_at = Instant::now();
        let presented_at = submitted_at + Duration::from_micros(16_700);

        for _ in 0..100 {
            stats.record_frame_present();
            stats.record_render(duration);
            stats.record_render_draw(duration);
            stats.record_render_flush(duration);
            stats.record_render_gpu_flush(duration);
            stats.record_render_submit(duration);
            stats.record_present_submit(duration);
            stats.record_pipeline(submitted_at, presented_at);
            stats.record_layout(duration);
            stats.record_refresh(duration);
            stats.record_event_resolve(duration);
            stats.record_patch_tree_process(duration);
            stats.record_renderer_cache(RendererCacheFrameStats {
                clean_subtree: RendererCacheKindFrameStats {
                    candidates: 4,
                    visible_candidates: 4,
                    hits: 3,
                    stores: 1,
                    current_entries: 1,
                    current_bytes: 4096,
                    draw_hit_time: Duration::from_micros(12),
                    prepare_time: Duration::from_micros(40),
                    ..RendererCacheKindFrameStats::default()
                },
                ..RendererCacheFrameStats::default()
            });
        }

        b.iter(|| black_box(stats.peek()));
    });

    group.finish();
}

criterion_group!(benches, bench_stats_collector);
criterion_main!(benches);
