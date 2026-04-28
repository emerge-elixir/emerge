use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
#[cfg(target_os = "linux")]
use emerge_skia::backend::skia_gpu::GlFrameSurface;
use emerge_skia::render_scene::{
    DrawPrimitive, RenderCacheCandidate, RenderCacheCandidateKind, RenderNode, RenderScene,
};
use emerge_skia::renderer::{RenderFrame, RenderState, SceneRenderer, insert_raster_asset};
use emerge_skia::tree::attrs::{BorderStyle, ImageFit};
use emerge_skia::tree::geometry::{ClipShape, CornerRadii, Rect};
use emerge_skia::tree::transform::Affine2;
#[cfg(target_os = "linux")]
use glutin_egl_sys::egl;
#[cfg(target_os = "linux")]
use glutin_egl_sys::egl::types::{EGLConfig, EGLContext, EGLDisplay, EGLSurface, EGLenum, EGLint};
#[cfg(target_os = "linux")]
use libloading::Library;
use skia_safe::{
    AlphaType, Color, ColorType, Image, ImageInfo, Matrix, Path, PathBuilder, PathDirection,
    Picture, PictureRecorder, Point3, RRect, Rect as SkRect, Surface, surfaces,
    utils::shadow_utils::ShadowFlags,
};
use std::hint::black_box;
use std::sync::Once;
#[cfg(target_os = "linux")]
use std::{ffi::CString, os::raw::c_void, ptr};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 720;
const BENCH_IMAGE_ID: &str = "renderer_bench_static";
static BENCH_ASSETS: Once = Once::new();

fn bench_renderer_raster_direct(c: &mut Criterion) {
    let mut group = c.benchmark_group("native/renderer/raster_direct");
    let cases = render_cases();

    for case in &cases {
        let summary = case.scene.summary();
        group.throughput(Throughput::Elements(summary.nodes as u64));
        group.bench_function(case.name, |b| {
            let state = RenderState::new(case.scene.clone(), Color::WHITE, 1, false);
            let info = ImageInfo::new(
                (WIDTH as i32, HEIGHT as i32),
                ColorType::RGBA8888,
                AlphaType::Premul,
                None,
            );
            let mut surface = surfaces::raster(&info, None, None)
                .expect("raster surface should be created for renderer benchmark");
            let mut renderer = SceneRenderer::new();

            b.iter(|| {
                let mut frame = RenderFrame::new(&mut surface, None);
                black_box(renderer.render(&mut frame, &state));
            });
        });
    }

    group.finish();
}

fn bench_renderer_direct_candidates(c: &mut Criterion) {
    // Candidate-only benchmarks live here until they prove a win and pass visual
    // parity. A neutral or slower result is a decision to keep renderer code
    // simpler, not a reason to wire the candidate into production drawing.
    let mut group = c.benchmark_group("native/renderer/direct_candidates");
    let paths = shadow_utils_paths();

    group.throughput(Throughput::Elements(paths.len() as u64));
    group.bench_function("shadow_skia_utils", |b| {
        let info = ImageInfo::new(
            (WIDTH as i32, HEIGHT as i32),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );
        let mut surface = surfaces::raster(&info, None, None)
            .expect("raster surface should be created for shadow utils benchmark");
        let ambient = Color::from_argb(48, 27, 36, 48);
        let spot = Color::from_argb(64, 27, 36, 48);

        b.iter(|| {
            let canvas = surface.canvas();
            canvas.clear(Color::WHITE);
            for (index, path) in paths.iter().enumerate() {
                canvas.draw_shadow(
                    path,
                    Point3::new(0.0, 0.0, 2.0 + (index % 4) as f32),
                    Point3::new(0.0, -120.0, 480.0),
                    72.0,
                    ambient,
                    spot,
                    Some(ShadowFlags::TRANSPARENT_OCCLUDER),
                );
            }
            black_box(surface.image_snapshot());
        });
    });

    group.finish();
}

fn bench_renderer_clean_subtree_cache_candidates(c: &mut Criterion) {
    // The historical `candidate_direct_fallback` benchmark name is kept for
    // Criterion baseline continuity. Now that the clean-subtree raster payload
    // path is enabled for eligible candidates, that case measures the production
    // lifecycle: first visibility misses, repeated visibility stores, and warm
    // frames draw the cached payload.
    //
    // The first clean-subtree pass showed retained pictures are only a modest
    // warm-hit win for these scenes, while raster payloads have expensive
    // miss/store cost and much faster warm hits. Keep the picture path as a
    // benchmark-only candidate unless future measurements change that result.
    ensure_benchmark_assets();

    let mut group = c.benchmark_group("native/renderer/cache_candidates");
    let cases = clean_subtree_cases();

    for case in &cases {
        let summary = RenderScene {
            nodes: case.nodes.clone(),
        }
        .summary();
        group.throughput(Throughput::Elements(summary.nodes as u64));

        group.bench_function(format!("{}/direct_children", case.name), |b| {
            let state = RenderState::new(
                RenderScene {
                    nodes: case.nodes.clone(),
                },
                Color::WHITE,
                1,
                false,
            );
            let mut surface = raster_bench_surface(WIDTH, HEIGHT);
            let mut renderer = SceneRenderer::new();

            b.iter(|| {
                let mut frame = RenderFrame::new(&mut surface, None);
                black_box(renderer.render(&mut frame, &state));
            });
        });

        group.bench_function(format!("{}/candidate_direct_fallback", case.name), |b| {
            let state =
                RenderState::new(clean_subtree_candidate_scene(case), Color::WHITE, 1, false);
            let mut surface = raster_bench_surface(WIDTH, HEIGHT);
            let mut renderer = SceneRenderer::new();

            b.iter(|| {
                let mut frame = RenderFrame::new(&mut surface, None);
                black_box(renderer.render(&mut frame, &state));
            });
        });

        group.bench_function(format!("{}/picture_miss_store", case.name), |b| {
            let mut surface = raster_bench_surface(WIDTH, HEIGHT);

            b.iter(|| {
                let canvas = surface.canvas();
                canvas.clear(Color::WHITE);
                SceneRenderer::render_nodes_for_cache_candidate_benchmark(canvas, &case.nodes);
                black_box(record_clean_subtree_picture(case));
                black_box(surface.image_snapshot());
            });
        });

        group.bench_function(format!("{}/picture_warm_hit", case.name), |b| {
            let picture = record_clean_subtree_picture(case);
            let matrix = Matrix::translate((case.bounds.x, case.bounds.y));
            let mut surface = raster_bench_surface(WIDTH, HEIGHT);

            b.iter(|| {
                let canvas = surface.canvas();
                canvas.clear(Color::WHITE);
                canvas.draw_picture(&picture, Some(&matrix), None);
                black_box(surface.image_snapshot());
            });
        });

        group.bench_function(format!("{}/raster_miss_store", case.name), |b| {
            let mut surface = raster_bench_surface(WIDTH, HEIGHT);

            b.iter(|| {
                let canvas = surface.canvas();
                canvas.clear(Color::WHITE);
                SceneRenderer::render_nodes_for_cache_candidate_benchmark(canvas, &case.nodes);
                black_box(rasterize_clean_subtree(case));
                black_box(surface.image_snapshot());
            });
        });

        group.bench_function(format!("{}/raster_warm_hit", case.name), |b| {
            let image = rasterize_clean_subtree(case);
            let mut surface = raster_bench_surface(WIDTH, HEIGHT);

            b.iter(|| {
                let canvas = surface.canvas();
                canvas.clear(Color::WHITE);
                canvas.draw_image(&image, (case.bounds.x, case.bounds.y), None);
                black_box(surface.image_snapshot());
            });
        });
    }

    group.finish();
}

fn bench_renderer_translated_cache_candidates(c: &mut Criterion) {
    // Paint-only animation attributes that do not change layout. The benchmark
    // group name stays translated for continuity with existing baselines, but
    // the cases now cover move_x, move_y, combined move_x/move_y, rotate,
    // scale, and alpha. These are the cache shapes where local content might
    // stay cached while composite placement or paint attributes change. The
    // production cache admits integer translation and root element alpha as
    // composition state; rotate and scale remain direct-fallback cases until
    // parity and full-frame benchmarks justify the extra sampling/layer
    // complexity.
    let mut group = c.benchmark_group("native/renderer/cache_candidates_translated");
    let cases = translated_cache_cases();

    for case in &cases {
        let summary = translated_cache_scene(case, DEFAULT_TRANSFORM_SAMPLE, false).summary();
        group.throughput(Throughput::Elements(summary.nodes as u64));

        group.bench_function(format!("{}/direct_translated_children", case.name), |b| {
            let states = translated_cache_states(case, false);
            let mut offset_index = 0usize;
            let mut surface = raster_bench_surface(WIDTH, HEIGHT);
            let mut renderer = SceneRenderer::new();

            b.iter(|| {
                let state = &states[offset_index];
                offset_index = (offset_index + 1) % states.len();
                let mut frame = RenderFrame::new(&mut surface, None);
                black_box(renderer.render(&mut frame, state));
            });
        });

        group.bench_function(format!("{}/candidate_direct_fallback", case.name), |b| {
            let states = translated_cache_states(case, true);
            let mut offset_index = 0usize;
            let mut surface = raster_bench_surface(WIDTH, HEIGHT);
            let mut renderer = SceneRenderer::new();

            b.iter(|| {
                let state = &states[offset_index];
                offset_index = (offset_index + 1) % states.len();
                let mut frame = RenderFrame::new(&mut surface, None);
                black_box(renderer.render(&mut frame, state));
            });
        });

        group.bench_function(format!("{}/picture_miss_store", case.name), |b| {
            let states = translated_cache_states(case, false);
            let mut offset_index = 0usize;
            let mut surface = raster_bench_surface(WIDTH, HEIGHT);
            let mut renderer = SceneRenderer::new();

            b.iter(|| {
                let state = &states[offset_index];
                offset_index = (offset_index + 1) % states.len();
                let mut frame = RenderFrame::new(&mut surface, None);
                black_box(renderer.render(&mut frame, state));
                black_box(record_translated_content_picture(case));
            });
        });

        group.bench_function(format!("{}/picture_warm_hit", case.name), |b| {
            let picture = record_translated_content_picture(case);
            let mut samples = case.samples.iter().cycle();
            let mut surface = raster_bench_surface(WIDTH, HEIGHT);

            b.iter(|| {
                let sample = samples.next().copied().unwrap_or_default();
                let canvas = surface.canvas();
                canvas.clear(Color::WHITE);
                draw_cached_picture_sample(canvas, &picture, case, sample);
                black_box(surface.image_snapshot());
            });
        });

        group.bench_function(format!("{}/raster_miss_store", case.name), |b| {
            let states = translated_cache_states(case, false);
            let mut offset_index = 0usize;
            let mut surface = raster_bench_surface(WIDTH, HEIGHT);
            let mut renderer = SceneRenderer::new();

            b.iter(|| {
                let state = &states[offset_index];
                offset_index = (offset_index + 1) % states.len();
                let mut frame = RenderFrame::new(&mut surface, None);
                black_box(renderer.render(&mut frame, state));
                black_box(rasterize_translated_content(case));
            });
        });

        group.bench_function(format!("{}/raster_warm_hit", case.name), |b| {
            let image = rasterize_translated_content(case);
            let mut samples = case.samples.iter().cycle();
            let mut surface = raster_bench_surface(WIDTH, HEIGHT);

            b.iter(|| {
                let sample = samples.next().copied().unwrap_or_default();
                let canvas = surface.canvas();
                canvas.clear(Color::WHITE);
                draw_cached_image_sample(canvas, &image, case, sample);
                black_box(surface.image_snapshot());
            });
        });
    }

    group.finish();
}

fn bench_renderer_layout_reflow_cache_candidates(c: &mut Criterion) {
    // Resize/reflow benchmark: card contents stay stable while the layout moves
    // cards between columns as the available width changes. This is the shape
    // expected to pay off for showcase layout resizing once tree rendering emits
    // local clean-subtree content plus a placement transform for reflowed boxes.
    let mut group = c.benchmark_group("native/renderer/cache_candidates_layout_reflow");
    let summary = layout_reflow_scene(DEFAULT_REFLOW_SAMPLE, false).summary();
    group.throughput(Throughput::Elements(summary.nodes as u64));

    group.bench_function("direct_reflowed_children", |b| {
        let states = layout_reflow_states(false);
        let mut sample_index = 0usize;
        let mut surface = raster_bench_surface(WIDTH, HEIGHT);
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let state = &states[sample_index];
            sample_index = (sample_index + 1) % states.len();
            let mut frame = RenderFrame::new(&mut surface, None);
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.bench_function("candidate_local_content", |b| {
        let states = layout_reflow_states(true);
        let mut sample_index = 0usize;
        let mut surface = raster_bench_surface(WIDTH, HEIGHT);
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let state = &states[sample_index];
            sample_index = (sample_index + 1) % states.len();
            let mut frame = RenderFrame::new(&mut surface, None);
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.finish();
}

fn bench_renderer_cache_children(c: &mut Criterion) {
    // This group is also the measured record for the rejected "new alpha
    // children cache kind" idea. The small overlapping-alpha workload did not
    // beat direct GPU drawing, so production code keeps alpha caching to the
    // existing root clean-subtree composition path and only adds lifecycle
    // accounting for parent/child cache interaction.
    let mut group = c.benchmark_group("native/renderer/cache_children");
    group.sample_size(30);

    group.bench_function("nested_alpha/direct_children", |b| {
        let states = alpha_children_states(false);
        let mut sample_index = 0usize;
        let mut surface = raster_bench_surface(WIDTH, HEIGHT);
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let state = &states[sample_index];
            sample_index = (sample_index + 1) % states.len();
            let mut frame = RenderFrame::new(&mut surface, None);
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.bench_function("nested_alpha/candidate_children", |b| {
        let states = alpha_children_states(true);
        let mut sample_index = 0usize;
        let mut surface = raster_bench_surface(WIDTH, HEIGHT);
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let state = &states[sample_index];
            sample_index = (sample_index + 1) % states.len();
            let mut frame = RenderFrame::new(&mut surface, None);
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.bench_function("parent_hit/direct_children", |b| {
        let state = RenderState::new(parent_child_cache_scene(false), Color::WHITE, 1, false);
        let mut surface = raster_bench_surface(WIDTH, HEIGHT);
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let mut frame = RenderFrame::new(&mut surface, None);
            black_box(renderer.render(&mut frame, &state));
        });
    });

    group.bench_function("parent_hit/nested_candidates", |b| {
        let state = RenderState::new(parent_child_cache_scene(true), Color::WHITE, 1, false);
        let mut surface = raster_bench_surface(WIDTH, HEIGHT);
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let mut frame = RenderFrame::new(&mut surface, None);
            black_box(renderer.render(&mut frame, &state));
        });
    });

    group.finish();
}

fn bench_renderer_cold_frames(c: &mut Criterion) {
    // This is a measurement gate, not a warmup implementation. Keep cold-frame
    // optimizations out of the renderer until this benchmark or a scripted demo
    // trace shows a repeatable total-frame improvement.
    ensure_benchmark_assets();

    let mut group = c.benchmark_group("native/renderer/cold_frame");
    group.sample_size(10);

    let mixed_state = RenderState::new(mixed_ui_scene(), Color::WHITE, 1, false);
    group.bench_function("raster_first_frame_mixed_ui", |b| {
        b.iter_batched(
            || {
                let info = ImageInfo::new(
                    (WIDTH as i32, HEIGHT as i32),
                    ColorType::RGBA8888,
                    AlphaType::Premul,
                    None,
                );
                let surface = surfaces::raster(&info, None, None)
                    .expect("raster surface should be created for cold-frame benchmark");
                let renderer = SceneRenderer::new();
                (surface, renderer)
            },
            |(mut surface, mut renderer)| {
                let mut frame = RenderFrame::new(&mut surface, None);
                black_box(renderer.render(&mut frame, &mixed_state));
            },
            BatchSize::SmallInput,
        );
    });

    let image_state = RenderState::new(raster_images_scene(), Color::WHITE, 1, false);
    group.bench_function("raster_first_frame_after_asset_insert", |b| {
        b.iter_batched(
            || {
                insert_raster_asset(
                    BENCH_IMAGE_ID,
                    include_bytes!("../../../priv/sample_assets/static.jpg"),
                )
                .expect("renderer benchmark raster asset should decode");

                let info = ImageInfo::new(
                    (WIDTH as i32, HEIGHT as i32),
                    ColorType::RGBA8888,
                    AlphaType::Premul,
                    None,
                );
                let surface = surfaces::raster(&info, None, None)
                    .expect("raster surface should be created for cold-frame benchmark");
                let renderer = SceneRenderer::new();
                (surface, renderer)
            },
            |(mut surface, mut renderer)| {
                let mut frame = RenderFrame::new(&mut surface, None);
                black_box(renderer.render(&mut frame, &image_state));
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

#[cfg(target_os = "linux")]
fn bench_renderer_gpu_surfaceless(c: &mut Criterion) {
    let Ok(surface_probe) = EglBenchSurface::new((WIDTH, HEIGHT)) else {
        eprintln!("Skipping native/renderer/gpu_surfaceless: EGL surfaceless setup failed");
        return;
    };
    drop(surface_probe);

    let mut group = c.benchmark_group("native/renderer/gpu_surfaceless");
    let cases = render_cases();

    for case in &cases {
        let summary = case.scene.summary();
        group.throughput(Throughput::Elements(summary.nodes as u64));
        group.bench_function(case.name, |b| {
            let state = RenderState::new(case.scene.clone(), Color::WHITE, 1, false);
            let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
                .expect("EGL surfaceless setup should stay available after probe");
            let mut renderer = SceneRenderer::new();

            b.iter(|| {
                let mut frame = surface.frame();
                black_box(renderer.render(&mut frame, &state));
            });
        });
    }

    group.finish();
}

#[cfg(not(target_os = "linux"))]
fn bench_renderer_gpu_surfaceless(_c: &mut Criterion) {}

#[cfg(target_os = "linux")]
fn bench_renderer_gpu_cold_frames(c: &mut Criterion) {
    if EglBenchSurface::new((WIDTH, HEIGHT)).is_err() {
        eprintln!("Skipping native/renderer/gpu_cold_frame: EGL surfaceless setup failed");
        return;
    }

    ensure_benchmark_assets();

    let mut group = c.benchmark_group("native/renderer/gpu_cold_frame");
    group.sample_size(10);

    let mixed_state = RenderState::new(mixed_ui_scene(), Color::WHITE, 1, false);
    group.bench_function("first_frame_mixed_ui", |b| {
        b.iter_batched(
            || {
                let surface = EglBenchSurface::new((WIDTH, HEIGHT))
                    .expect("EGL surfaceless setup should stay available after probe");
                let renderer = SceneRenderer::new();
                (surface, renderer)
            },
            |(mut surface, mut renderer)| {
                let mut frame = surface.frame();
                black_box(renderer.render(&mut frame, &mixed_state));
            },
            BatchSize::SmallInput,
        );
    });

    let image_state = RenderState::new(raster_images_scene(), Color::WHITE, 1, false);
    group.bench_function("first_frame_after_asset_insert", |b| {
        b.iter_batched(
            || {
                insert_raster_asset(
                    BENCH_IMAGE_ID,
                    include_bytes!("../../../priv/sample_assets/static.jpg"),
                )
                .expect("renderer benchmark raster asset should decode");
                let surface = EglBenchSurface::new((WIDTH, HEIGHT))
                    .expect("EGL surfaceless setup should stay available after probe");
                let renderer = SceneRenderer::new();
                (surface, renderer)
            },
            |(mut surface, mut renderer)| {
                let mut frame = surface.frame();
                black_box(renderer.render(&mut frame, &image_state));
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

#[cfg(not(target_os = "linux"))]
fn bench_renderer_gpu_cold_frames(_c: &mut Criterion) {}

#[cfg(target_os = "linux")]
fn bench_renderer_gpu_cache_candidates(c: &mut Criterion) {
    let Ok(surface_probe) = EglBenchSurface::new((WIDTH, HEIGHT)) else {
        eprintln!("Skipping native/renderer/gpu_cache_candidates: EGL surfaceless setup failed");
        return;
    };
    drop(surface_probe);

    ensure_benchmark_assets();

    let mut group = c.benchmark_group("native/renderer/gpu_cache_candidates");
    group.sample_size(30);
    let cases = clean_subtree_cases();

    for case in &cases {
        let summary = RenderScene {
            nodes: case.nodes.clone(),
        }
        .summary();
        group.throughput(Throughput::Elements(summary.nodes as u64));

        group.bench_function(format!("{}/direct_children", case.name), |b| {
            let state = RenderState::new(
                RenderScene {
                    nodes: case.nodes.clone(),
                },
                Color::WHITE,
                1,
                false,
            );
            let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
                .expect("EGL surfaceless setup should stay available after probe");
            let mut renderer = SceneRenderer::new();

            b.iter(|| {
                let mut frame = surface.frame();
                black_box(renderer.render(&mut frame, &state));
            });
        });

        group.bench_function(format!("{}/candidate_direct_fallback", case.name), |b| {
            let state =
                RenderState::new(clean_subtree_candidate_scene(case), Color::WHITE, 1, false);
            let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
                .expect("EGL surfaceless setup should stay available after probe");
            let mut renderer = SceneRenderer::new();

            b.iter(|| {
                let mut frame = surface.frame();
                black_box(renderer.render(&mut frame, &state));
            });
        });

        group.bench_function(format!("{}/picture_warm_hit", case.name), |b| {
            let picture = record_clean_subtree_picture(case);
            let matrix = Matrix::translate((case.bounds.x, case.bounds.y));
            let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
                .expect("EGL surfaceless setup should stay available after probe");

            b.iter(|| {
                let mut frame = surface.frame();
                let canvas = frame.surface_mut().canvas();
                canvas.clear(Color::WHITE);
                canvas.draw_picture(&picture, Some(&matrix), None);
                black_box(frame.flush());
            });
        });

        group.bench_function(format!("{}/raster_warm_hit", case.name), |b| {
            let image = rasterize_clean_subtree(case);
            let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
                .expect("EGL surfaceless setup should stay available after probe");

            b.iter(|| {
                let mut frame = surface.frame();
                let canvas = frame.surface_mut().canvas();
                canvas.clear(Color::WHITE);
                canvas.draw_image(&image, (case.bounds.x, case.bounds.y), None);
                black_box(frame.flush());
            });
        });
    }

    group.finish();
}

#[cfg(not(target_os = "linux"))]
fn bench_renderer_gpu_cache_candidates(_c: &mut Criterion) {}

#[cfg(target_os = "linux")]
fn bench_renderer_gpu_translated_cache_candidates(c: &mut Criterion) {
    let Ok(surface_probe) = EglBenchSurface::new((WIDTH, HEIGHT)) else {
        eprintln!(
            "Skipping native/renderer/gpu_cache_candidates_translated: EGL surfaceless setup failed"
        );
        return;
    };
    drop(surface_probe);

    let cases = translated_cache_cases();
    let mut group = c.benchmark_group("native/renderer/gpu_cache_candidates_translated");
    group.sample_size(30);

    for case in &cases {
        let summary = translated_cache_scene(case, DEFAULT_TRANSFORM_SAMPLE, false).summary();
        group.throughput(Throughput::Elements(summary.nodes as u64));

        group.bench_function(format!("{}/direct_translated_children", case.name), |b| {
            let states = translated_cache_states(case, false);
            let mut offset_index = 0usize;
            let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
                .expect("EGL surfaceless setup should stay available after probe");
            let mut renderer = SceneRenderer::new();

            b.iter(|| {
                let state = &states[offset_index];
                offset_index = (offset_index + 1) % states.len();
                let mut frame = surface.frame();
                black_box(renderer.render(&mut frame, state));
            });
        });

        group.bench_function(format!("{}/candidate_direct_fallback", case.name), |b| {
            let states = translated_cache_states(case, true);
            let mut offset_index = 0usize;
            let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
                .expect("EGL surfaceless setup should stay available after probe");
            let mut renderer = SceneRenderer::new();

            b.iter(|| {
                let state = &states[offset_index];
                offset_index = (offset_index + 1) % states.len();
                let mut frame = surface.frame();
                black_box(renderer.render(&mut frame, state));
            });
        });

        group.bench_function(format!("{}/picture_warm_hit", case.name), |b| {
            let picture = record_translated_content_picture(case);
            let mut samples = case.samples.iter().cycle();
            let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
                .expect("EGL surfaceless setup should stay available after probe");

            b.iter(|| {
                let sample = samples.next().copied().unwrap_or_default();
                let mut frame = surface.frame();
                let canvas = frame.surface_mut().canvas();
                canvas.clear(Color::WHITE);
                draw_cached_picture_sample(canvas, &picture, case, sample);
                black_box(frame.flush());
            });
        });

        group.bench_function(format!("{}/raster_warm_hit", case.name), |b| {
            let image = rasterize_translated_content(case);
            let mut samples = case.samples.iter().cycle();
            let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
                .expect("EGL surfaceless setup should stay available after probe");

            b.iter(|| {
                let sample = samples.next().copied().unwrap_or_default();
                let mut frame = surface.frame();
                let canvas = frame.surface_mut().canvas();
                canvas.clear(Color::WHITE);
                draw_cached_image_sample(canvas, &image, case, sample);
                black_box(frame.flush());
            });
        });
    }

    group.finish();
}

#[cfg(not(target_os = "linux"))]
fn bench_renderer_gpu_translated_cache_candidates(_c: &mut Criterion) {}

#[cfg(target_os = "linux")]
fn bench_renderer_gpu_layout_reflow_cache_candidates(c: &mut Criterion) {
    let Ok(surface_probe) = EglBenchSurface::new((WIDTH, HEIGHT)) else {
        eprintln!(
            "Skipping native/renderer/gpu_cache_candidates_layout_reflow: EGL surfaceless setup failed"
        );
        return;
    };
    drop(surface_probe);

    let mut group = c.benchmark_group("native/renderer/gpu_cache_candidates_layout_reflow");
    group.sample_size(30);
    let summary = layout_reflow_scene(DEFAULT_REFLOW_SAMPLE, false).summary();
    group.throughput(Throughput::Elements(summary.nodes as u64));

    group.bench_function("direct_reflowed_children", |b| {
        let states = layout_reflow_states(false);
        let mut sample_index = 0usize;
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let state = &states[sample_index];
            sample_index = (sample_index + 1) % states.len();
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.bench_function("candidate_local_content", |b| {
        let states = layout_reflow_states(true);
        let mut sample_index = 0usize;
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let state = &states[sample_index];
            sample_index = (sample_index + 1) % states.len();
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.finish();
}

#[cfg(not(target_os = "linux"))]
fn bench_renderer_gpu_layout_reflow_cache_candidates(_c: &mut Criterion) {}

#[cfg(target_os = "linux")]
fn bench_renderer_gpu_cache_children(c: &mut Criterion) {
    let Ok(surface_probe) = EglBenchSurface::new((WIDTH, HEIGHT)) else {
        eprintln!("Skipping native/renderer/gpu_cache_children: EGL surfaceless setup failed");
        return;
    };
    drop(surface_probe);

    let mut group = c.benchmark_group("native/renderer/gpu_cache_children");
    group.sample_size(30);

    group.bench_function("nested_alpha/direct_children", |b| {
        let states = alpha_children_states(false);
        let mut sample_index = 0usize;
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let state = &states[sample_index];
            sample_index = (sample_index + 1) % states.len();
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.bench_function("nested_alpha/candidate_children", |b| {
        let states = alpha_children_states(true);
        let mut sample_index = 0usize;
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let state = &states[sample_index];
            sample_index = (sample_index + 1) % states.len();
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.bench_function("parent_hit/direct_children", |b| {
        let state = RenderState::new(parent_child_cache_scene(false), Color::WHITE, 1, false);
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, &state));
        });
    });

    group.bench_function("parent_hit/nested_candidates", |b| {
        let state = RenderState::new(parent_child_cache_scene(true), Color::WHITE, 1, false);
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, &state));
        });
    });

    group.finish();
}

#[cfg(not(target_os = "linux"))]
fn bench_renderer_gpu_cache_children(_c: &mut Criterion) {}

struct RenderCase {
    name: &'static str,
    scene: RenderScene,
}

struct CleanSubtreeCase {
    name: &'static str,
    stable_id: u64,
    generation: u64,
    bounds: Rect,
    nodes: Vec<RenderNode>,
}

struct TranslatedCacheCase {
    name: &'static str,
    stable_id: u64,
    generation: u64,
    base_x: f32,
    base_y: f32,
    bounds: Rect,
    samples: &'static [PaintTransformSample],
    nodes: Vec<RenderNode>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LayoutReflowSample {
    container_width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PaintTransformSample {
    move_x: f32,
    move_y: f32,
    rotate_degrees: f32,
    scale: f32,
    alpha: f32,
}

impl Default for PaintTransformSample {
    fn default() -> Self {
        Self {
            move_x: 0.0,
            move_y: 0.0,
            rotate_degrees: 0.0,
            scale: 1.0,
            alpha: 1.0,
        }
    }
}

const MOVE_X_SAMPLES: [PaintTransformSample; 5] = [
    PaintTransformSample {
        move_x: -300.0,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    DEFAULT_TRANSFORM_SAMPLE,
    PaintTransformSample {
        move_x: 300.0,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    DEFAULT_TRANSFORM_SAMPLE,
    PaintTransformSample {
        move_x: -300.0,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
];
const MOVE_Y_SAMPLES: [PaintTransformSample; 5] = [
    PaintTransformSample {
        move_y: -180.0,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    DEFAULT_TRANSFORM_SAMPLE,
    PaintTransformSample {
        move_y: 180.0,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    DEFAULT_TRANSFORM_SAMPLE,
    PaintTransformSample {
        move_y: -180.0,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
];
const MOVE_XY_SAMPLES: [PaintTransformSample; 5] = [
    PaintTransformSample {
        move_x: -160.0,
        move_y: -90.0,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    DEFAULT_TRANSFORM_SAMPLE,
    PaintTransformSample {
        move_x: 160.0,
        move_y: 90.0,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    DEFAULT_TRANSFORM_SAMPLE,
    PaintTransformSample {
        move_x: -160.0,
        move_y: -90.0,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
];
const ROTATE_SAMPLES: [PaintTransformSample; 5] = [
    PaintTransformSample {
        rotate_degrees: -12.0,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    DEFAULT_TRANSFORM_SAMPLE,
    PaintTransformSample {
        rotate_degrees: 12.0,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    DEFAULT_TRANSFORM_SAMPLE,
    PaintTransformSample {
        rotate_degrees: -12.0,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
];
const SCALE_SAMPLES: [PaintTransformSample; 5] = [
    PaintTransformSample {
        scale: 0.92,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    DEFAULT_TRANSFORM_SAMPLE,
    PaintTransformSample {
        scale: 1.08,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    DEFAULT_TRANSFORM_SAMPLE,
    PaintTransformSample {
        scale: 0.92,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
];
const ALPHA_SAMPLES: [PaintTransformSample; 5] = [
    PaintTransformSample {
        alpha: 0.42,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    PaintTransformSample {
        alpha: 0.72,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    DEFAULT_TRANSFORM_SAMPLE,
    PaintTransformSample {
        alpha: 0.72,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    PaintTransformSample {
        alpha: 0.42,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
];
const APP_SELECTOR_ALPHA_SAMPLES: [PaintTransformSample; 5] = [
    PaintTransformSample {
        alpha: 0.36,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    PaintTransformSample {
        alpha: 0.68,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    DEFAULT_TRANSFORM_SAMPLE,
    PaintTransformSample {
        alpha: 0.68,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    PaintTransformSample {
        alpha: 0.36,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
];
const TODO_ENTRY_TRANSLATE_ALPHA_SAMPLES: [PaintTransformSample; 5] = [
    PaintTransformSample {
        move_y: -32.0,
        alpha: 0.28,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    PaintTransformSample {
        move_y: -16.0,
        alpha: 0.64,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    DEFAULT_TRANSFORM_SAMPLE,
    PaintTransformSample {
        move_y: 16.0,
        alpha: 0.64,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
    PaintTransformSample {
        move_y: 32.0,
        alpha: 0.28,
        ..DEFAULT_TRANSFORM_SAMPLE
    },
];
const DEFAULT_TRANSFORM_SAMPLE: PaintTransformSample = PaintTransformSample {
    move_x: 0.0,
    move_y: 0.0,
    rotate_degrees: 0.0,
    scale: 1.0,
    alpha: 1.0,
};
const REFLOW_CARD_COUNT: usize = 18;
const REFLOW_CARD_WIDTH: f32 = 206.0;
const REFLOW_CARD_HEIGHT: f32 = 108.0;
const REFLOW_CARD_GAP: f32 = 14.0;
const DEFAULT_REFLOW_SAMPLE: LayoutReflowSample = LayoutReflowSample {
    container_width: 920.0,
};
const LAYOUT_REFLOW_SAMPLES: [LayoutReflowSample; 6] = [
    DEFAULT_REFLOW_SAMPLE,
    LayoutReflowSample {
        container_width: 742.0,
    },
    LayoutReflowSample {
        container_width: 520.0,
    },
    LayoutReflowSample {
        container_width: 860.0,
    },
    LayoutReflowSample {
        container_width: 632.0,
    },
    LayoutReflowSample {
        container_width: 458.0,
    },
];

fn render_cases() -> Vec<RenderCase> {
    ensure_benchmark_assets();

    vec![
        RenderCase {
            name: "text_heavy",
            scene: text_heavy_scene(),
        },
        RenderCase {
            name: "solid_uniform_borders",
            scene: solid_uniform_borders_scene(),
        },
        RenderCase {
            name: "solid_edge_borders",
            scene: solid_edge_borders_scene(),
        },
        RenderCase {
            name: "dashed_borders",
            scene: dashed_borders_scene(),
        },
        RenderCase {
            name: "border_clip_heavy",
            scene: border_clip_heavy_scene(),
        },
        RenderCase {
            name: "template_tinted_images",
            scene: template_tinted_images_scene(),
        },
        RenderCase {
            name: "raster_images",
            scene: raster_images_scene(),
        },
        RenderCase {
            name: "alpha_single_primitive",
            scene: alpha_single_primitive_scene(),
        },
        RenderCase {
            name: "alpha_group_overlap",
            scene: alpha_group_overlap_scene(),
        },
        RenderCase {
            name: "shadow_mask_filter",
            scene: shadow_mask_filter_scene(),
        },
        RenderCase {
            name: "gradient_rects",
            scene: gradient_rects_scene(),
        },
        RenderCase {
            name: "clip_rect_vs_rrect",
            scene: clip_rect_vs_rrect_scene(),
        },
        RenderCase {
            name: "mixed_ui_scene",
            scene: mixed_ui_scene(),
        },
    ]
}

fn clean_subtree_cases() -> Vec<CleanSubtreeCase> {
    vec![
        CleanSubtreeCase {
            name: "borders_like_static_siblings",
            stable_id: 1,
            generation: 1,
            bounds: Rect {
                x: 20.0,
                y: 20.0,
                width: 920.0,
                height: 650.0,
            },
            nodes: borders_like_clean_subtree_nodes(),
        },
        CleanSubtreeCase {
            name: "assets_like_loaded_tiles",
            stable_id: 2,
            generation: 1,
            bounds: Rect {
                x: 18.0,
                y: 18.0,
                width: 912.0,
                height: 612.0,
            },
            nodes: assets_like_clean_subtree_nodes(),
        },
    ]
}

fn clean_subtree_candidate_scene(case: &CleanSubtreeCase) -> RenderScene {
    RenderScene {
        nodes: vec![RenderNode::CacheCandidate(RenderCacheCandidate {
            kind: RenderCacheCandidateKind::CleanSubtree,
            stable_id: case.stable_id,
            content_generation: case.generation,
            bounds: case.bounds,
            children: case.nodes.clone(),
        })],
    }
}

fn translated_cache_cases() -> Vec<TranslatedCacheCase> {
    vec![
        TranslatedCacheCase {
            name: "nerves_animated_counter_move_x",
            stable_id: 10,
            generation: 1,
            base_x: 320.0,
            base_y: 317.0,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 320.0,
                height: 86.0,
            },
            samples: &MOVE_X_SAMPLES,
            nodes: nerves_counter_content_nodes(),
        },
        TranslatedCacheCase {
            name: "toast_panel_move_y",
            stable_id: 11,
            generation: 1,
            base_x: 330.0,
            base_y: 290.0,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 300.0,
                height: 138.0,
            },
            samples: &MOVE_Y_SAMPLES,
            nodes: toast_panel_content_nodes(),
        },
        TranslatedCacheCase {
            name: "floating_card_move_xy",
            stable_id: 12,
            generation: 1,
            base_x: 360.0,
            base_y: 280.0,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 248.0,
                height: 154.0,
            },
            samples: &MOVE_XY_SAMPLES,
            nodes: floating_card_content_nodes(),
        },
        TranslatedCacheCase {
            name: "floating_card_rotate",
            stable_id: 13,
            generation: 1,
            base_x: 356.0,
            base_y: 278.0,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 248.0,
                height: 154.0,
            },
            samples: &ROTATE_SAMPLES,
            nodes: floating_card_content_nodes(),
        },
        TranslatedCacheCase {
            name: "floating_card_scale",
            stable_id: 14,
            generation: 1,
            base_x: 356.0,
            base_y: 278.0,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 248.0,
                height: 154.0,
            },
            samples: &SCALE_SAMPLES,
            nodes: floating_card_content_nodes(),
        },
        TranslatedCacheCase {
            name: "floating_card_alpha",
            stable_id: 15,
            generation: 1,
            base_x: 356.0,
            base_y: 278.0,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 248.0,
                height: 154.0,
            },
            samples: &ALPHA_SAMPLES,
            nodes: floating_card_content_nodes(),
        },
        TranslatedCacheCase {
            name: "app_selector_menu_alpha",
            stable_id: 16,
            generation: 1,
            base_x: 344.0,
            base_y: 138.0,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 276.0,
                height: 218.0,
            },
            samples: &APP_SELECTOR_ALPHA_SAMPLES,
            nodes: app_selector_menu_content_nodes(),
        },
        TranslatedCacheCase {
            name: "todo_entry_translate_alpha",
            stable_id: 17,
            generation: 1,
            base_x: 312.0,
            base_y: 306.0,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 360.0,
                height: 74.0,
            },
            samples: &TODO_ENTRY_TRANSLATE_ALPHA_SAMPLES,
            nodes: todo_entry_content_nodes(),
        },
    ]
}

fn translated_cache_states(case: &TranslatedCacheCase, cache_candidate: bool) -> Vec<RenderState> {
    case.samples
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            RenderState::new(
                translated_cache_scene(case, *sample, cache_candidate),
                Color::WHITE,
                index as u64 + 1,
                true,
            )
        })
        .collect()
}

fn translated_cache_scene(
    case: &TranslatedCacheCase,
    sample: PaintTransformSample,
    cache_candidate: bool,
) -> RenderScene {
    let children = if cache_candidate {
        vec![RenderNode::CacheCandidate(RenderCacheCandidate {
            kind: RenderCacheCandidateKind::CleanSubtree,
            stable_id: case.stable_id,
            content_generation: case.generation,
            bounds: case.bounds,
            children: case.nodes.clone(),
        })]
    } else {
        case.nodes.clone()
    };

    let transformed = vec![RenderNode::Transform {
        transform: sampled_affine(case, sample),
        children,
    }];

    if sample.alpha >= 1.0 {
        RenderScene { nodes: transformed }
    } else {
        RenderScene {
            nodes: vec![RenderNode::Alpha {
                alpha: sample.alpha,
                children: transformed,
            }],
        }
    }
}

fn layout_reflow_states(cache_candidate: bool) -> Vec<RenderState> {
    LAYOUT_REFLOW_SAMPLES
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            RenderState::new(
                layout_reflow_scene(*sample, cache_candidate),
                Color::WHITE,
                index as u64 + 1,
                false,
            )
        })
        .collect()
}

fn layout_reflow_scene(sample: LayoutReflowSample, cache_candidate: bool) -> RenderScene {
    RenderScene {
        nodes: (0..REFLOW_CARD_COUNT)
            .map(|index| {
                let (x, y) = layout_reflow_card_position(index, sample.container_width);
                let children = if cache_candidate {
                    vec![RenderNode::CacheCandidate(RenderCacheCandidate {
                        kind: RenderCacheCandidateKind::CleanSubtree,
                        stable_id: 20_000 + index as u64,
                        content_generation: 1,
                        bounds: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: REFLOW_CARD_WIDTH,
                            height: REFLOW_CARD_HEIGHT,
                        },
                        children: layout_reflow_card_content_nodes(index),
                    })]
                } else {
                    layout_reflow_card_content_nodes(index)
                };

                RenderNode::Transform {
                    transform: Affine2::translation(x, y),
                    children,
                }
            })
            .collect(),
    }
}

fn alpha_children_states(cache_candidate: bool) -> Vec<RenderState> {
    APP_SELECTOR_ALPHA_SAMPLES
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            RenderState::new(
                alpha_children_scene(sample.alpha, cache_candidate),
                Color::WHITE,
                index as u64 + 1,
                true,
            )
        })
        .collect()
}

fn alpha_children_scene(alpha: f32, cache_candidate: bool) -> RenderScene {
    let children = if cache_candidate {
        vec![RenderNode::CacheCandidate(RenderCacheCandidate {
            kind: RenderCacheCandidateKind::CleanSubtree,
            stable_id: 30_000,
            content_generation: 1,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 360.0,
                height: 210.0,
            },
            children: alpha_children_content_nodes(0.0, 0.0),
        })]
    } else {
        alpha_children_content_nodes(0.0, 0.0)
    };

    RenderScene {
        nodes: vec![
            RenderNode::Primitive(DrawPrimitive::Rect(
                0.0,
                0.0,
                WIDTH as f32,
                HEIGHT as f32,
                0xF8FAFCFF,
            )),
            RenderNode::Transform {
                transform: Affine2::translation(300.0, 220.0),
                children: vec![RenderNode::Alpha { alpha, children }],
            },
        ],
    }
}

fn parent_child_cache_scene(cache_candidates: bool) -> RenderScene {
    let child_nodes = alpha_children_content_nodes(26.0, 52.0);
    let nested_child = if cache_candidates {
        vec![RenderNode::CacheCandidate(RenderCacheCandidate {
            kind: RenderCacheCandidateKind::CleanSubtree,
            stable_id: 31_001,
            content_generation: 1,
            bounds: Rect {
                x: 26.0,
                y: 52.0,
                width: 360.0,
                height: 210.0,
            },
            children: child_nodes,
        })]
    } else {
        child_nodes
    };

    let mut parent_children = vec![
        RenderNode::Primitive(DrawPrimitive::RoundedRect(
            0.0, 0.0, 430.0, 312.0, 14.0, 0xFFFFFFFF,
        )),
        RenderNode::Primitive(DrawPrimitive::Border(
            0.5,
            0.5,
            429.0,
            311.0,
            14.0,
            1.0,
            0xCBD5E1FF,
            BorderStyle::Solid,
        )),
        RenderNode::Primitive(DrawPrimitive::TextWithFont(
            24.0,
            31.0,
            "Parent cache candidate".to_string(),
            16.0,
            0x0F172AFF,
            "default".to_string(),
            700,
            false,
        )),
    ];
    parent_children.extend(nested_child);

    let children = if cache_candidates {
        vec![RenderNode::CacheCandidate(RenderCacheCandidate {
            kind: RenderCacheCandidateKind::CleanSubtree,
            stable_id: 31_000,
            content_generation: 1,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 430.0,
                height: 312.0,
            },
            children: parent_children,
        })]
    } else {
        parent_children
    };

    RenderScene {
        nodes: vec![RenderNode::Transform {
            transform: Affine2::translation(252.0, 184.0),
            children,
        }],
    }
}

fn alpha_children_content_nodes(offset_x: f32, offset_y: f32) -> Vec<RenderNode> {
    vec![
        RenderNode::Primitive(DrawPrimitive::RoundedRect(
            offset_x, offset_y, 360.0, 210.0, 12.0, 0xE0F2FEF2,
        )),
        RenderNode::Primitive(DrawPrimitive::RoundedRect(
            offset_x + 22.0,
            offset_y + 26.0,
            188.0,
            78.0,
            12.0,
            0x2563EBF0,
        )),
        RenderNode::Primitive(DrawPrimitive::RoundedRect(
            offset_x + 122.0,
            offset_y + 62.0,
            190.0,
            86.0,
            14.0,
            0xF97316E8,
        )),
        RenderNode::Primitive(DrawPrimitive::TextWithFont(
            offset_x + 34.0,
            offset_y + 54.0,
            "alpha children".to_string(),
            16.0,
            0xFFFFFFFF,
            "default".to_string(),
            700,
            false,
        )),
        RenderNode::Primitive(DrawPrimitive::TextWithFont(
            offset_x + 142.0,
            offset_y + 116.0,
            "overlap group".to_string(),
            14.0,
            0x111827FF,
            "default".to_string(),
            600,
            false,
        )),
        RenderNode::Primitive(DrawPrimitive::Border(
            offset_x + 0.5,
            offset_y + 0.5,
            359.0,
            209.0,
            12.0,
            1.0,
            0x0369A1FF,
            BorderStyle::Solid,
        )),
    ]
}

fn layout_reflow_card_position(index: usize, container_width: f32) -> (f32, f32) {
    let columns = (((container_width + REFLOW_CARD_GAP) / (REFLOW_CARD_WIDTH + REFLOW_CARD_GAP))
        .floor() as usize)
        .max(1);
    let column = index % columns;
    let row = index / columns;
    let occupied_width =
        columns as f32 * REFLOW_CARD_WIDTH + columns.saturating_sub(1) as f32 * REFLOW_CARD_GAP;
    let start_x = 24.0 + ((container_width - occupied_width) / 2.0).max(0.0).round();
    let x = start_x + column as f32 * (REFLOW_CARD_WIDTH + REFLOW_CARD_GAP);
    let y = 24.0 + row as f32 * (REFLOW_CARD_HEIGHT + REFLOW_CARD_GAP);
    (x.round(), y.round())
}

fn layout_reflow_card_content_nodes(index: usize) -> Vec<RenderNode> {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: REFLOW_CARD_WIDTH,
        height: REFLOW_CARD_HEIGHT,
    };
    let radii = Some(CornerRadii {
        tl: 8.0,
        tr: 8.0,
        br: 8.0,
        bl: 8.0,
    });
    let accent = match index % 4 {
        0 => 0x2563EBFF,
        1 => 0x16A34AFF,
        2 => 0xD97706FF,
        _ => 0x7C3AEDFF,
    };

    vec![RenderNode::Clip {
        clips: vec![ClipShape { rect, radii }],
        children: vec![
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                0.0,
                0.0,
                REFLOW_CARD_WIDTH,
                REFLOW_CARD_HEIGHT,
                8.0,
                0xF8FAFCFF,
            )),
            RenderNode::Primitive(DrawPrimitive::Border(
                0.5,
                0.5,
                REFLOW_CARD_WIDTH - 1.0,
                REFLOW_CARD_HEIGHT - 1.0,
                8.0,
                1.0,
                0xCBD5E1FF,
                BorderStyle::Solid,
            )),
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                12.0, 14.0, 34.0, 34.0, 6.0, accent,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                58.0,
                30.0,
                format!("Layout card {:02}", index + 1),
                15.0,
                0x0F172AFF,
                "default".to_string(),
                700,
                false,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                58.0,
                50.0,
                "stable content".to_string(),
                12.0,
                0x475569FF,
                "default".to_string(),
                400,
                false,
            )),
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                12.0, 70.0, 78.0, 22.0, 5.0, 0xE2E8F0FF,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                24.0,
                85.0,
                "metrics".to_string(),
                10.0,
                0x334155FF,
                "default".to_string(),
                500,
                false,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                108.0,
                85.0,
                format!("{} items", 12 + index),
                11.0,
                0x64748BFF,
                "default".to_string(),
                400,
                false,
            )),
        ],
    }]
}

fn record_translated_content_picture(case: &TranslatedCacheCase) -> Picture {
    let mut recorder = PictureRecorder::new();
    let bounds = SkRect::from_xywh(0.0, 0.0, case.bounds.width, case.bounds.height);
    let canvas = recorder.begin_recording(bounds, false);
    SceneRenderer::render_nodes_for_cache_candidate_benchmark(canvas, &case.nodes);
    recorder
        .finish_recording_as_picture(Some(&bounds))
        .expect("picture recorder should produce a translated cache candidate picture")
}

fn rasterize_translated_content(case: &TranslatedCacheCase) -> Image {
    let width = case.bounds.width.ceil().max(1.0) as u32;
    let height = case.bounds.height.ceil().max(1.0) as u32;
    let mut surface = raster_bench_surface(width, height);
    let canvas = surface.canvas();
    canvas.clear(Color::TRANSPARENT);
    SceneRenderer::render_nodes_for_cache_candidate_benchmark(canvas, &case.nodes);
    surface.image_snapshot()
}

fn draw_cached_picture_sample(
    canvas: &skia_safe::Canvas,
    picture: &Picture,
    case: &TranslatedCacheCase,
    sample: PaintTransformSample,
) {
    let matrix = matrix_from_affine2(sampled_affine(case, sample));

    if sample.alpha < 1.0 {
        canvas.save_layer_alpha(None, paint_alpha_u8(sample.alpha).into());
    }
    canvas.draw_picture(picture, Some(&matrix), None);
    if sample.alpha < 1.0 {
        canvas.restore();
    }
}

fn draw_cached_image_sample(
    canvas: &skia_safe::Canvas,
    image: &Image,
    case: &TranslatedCacheCase,
    sample: PaintTransformSample,
) {
    let matrix = matrix_from_affine2(sampled_affine(case, sample));

    if sample.alpha < 1.0 {
        canvas.save_layer_alpha(None, paint_alpha_u8(sample.alpha).into());
    }
    canvas.save();
    canvas.concat(&matrix);
    canvas.draw_image(image, (0.0, 0.0), None);
    canvas.restore();
    if sample.alpha < 1.0 {
        canvas.restore();
    }
}

fn sampled_affine(case: &TranslatedCacheCase, sample: PaintTransformSample) -> Affine2 {
    let center_x = case.bounds.width / 2.0;
    let center_y = case.bounds.height / 2.0;

    Affine2::translation(case.base_x + sample.move_x, case.base_y + sample.move_y)
        .then(Affine2::translation(center_x, center_y))
        .then(Affine2::rotation_degrees(sample.rotate_degrees))
        .then(Affine2::scale(sample.scale, sample.scale))
        .then(Affine2::translation(-center_x, -center_y))
}

fn matrix_from_affine2(transform: Affine2) -> Matrix {
    Matrix::new_all(
        transform.xx,
        transform.xy,
        transform.tx,
        transform.yx,
        transform.yy,
        transform.ty,
        0.0,
        0.0,
        1.0,
    )
}

fn paint_alpha_u8(alpha: f32) -> u8 {
    (alpha.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn raster_bench_surface(width: u32, height: u32) -> Surface {
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    surfaces::raster(&info, None, None)
        .expect("raster surface should be created for renderer cache candidate benchmark")
}

fn record_clean_subtree_picture(case: &CleanSubtreeCase) -> Picture {
    let mut recorder = PictureRecorder::new();
    let bounds = SkRect::from_xywh(0.0, 0.0, case.bounds.width, case.bounds.height);
    let canvas = recorder.begin_recording(bounds, false);
    canvas.save();
    canvas.translate((-case.bounds.x, -case.bounds.y));
    SceneRenderer::render_nodes_for_cache_candidate_benchmark(canvas, &case.nodes);
    canvas.restore();
    recorder
        .finish_recording_as_picture(Some(&bounds))
        .expect("picture recorder should produce a cache candidate picture")
}

fn rasterize_clean_subtree(case: &CleanSubtreeCase) -> Image {
    let width = case.bounds.width.ceil().max(1.0) as u32;
    let height = case.bounds.height.ceil().max(1.0) as u32;
    let mut surface = raster_bench_surface(width, height);
    let canvas = surface.canvas();
    canvas.clear(Color::TRANSPARENT);
    canvas.save();
    canvas.translate((-case.bounds.x, -case.bounds.y));
    SceneRenderer::render_nodes_for_cache_candidate_benchmark(canvas, &case.nodes);
    canvas.restore();
    surface.image_snapshot()
}

fn ensure_benchmark_assets() {
    BENCH_ASSETS.call_once(|| {
        insert_raster_asset(
            BENCH_IMAGE_ID,
            include_bytes!("../../../priv/sample_assets/static.jpg"),
        )
        .expect("renderer benchmark raster asset should decode");
    });
}

fn text_heavy_scene() -> RenderScene {
    RenderScene {
        nodes: (0..144)
            .map(|index| {
                let col = index % 3;
                let row = index / 3;
                let x = 24.0 + col as f32 * 300.0;
                let y = 28.0 + row as f32 * 14.0;
                RenderNode::Primitive(DrawPrimitive::TextWithFont(
                    x,
                    y,
                    format!("Renderer cache benchmark row {index:03}"),
                    13.0,
                    0x18202AFF,
                    "default".to_string(),
                    if index % 7 == 0 { 700 } else { 400 },
                    index % 11 == 0,
                ))
            })
            .collect(),
    }
}

fn solid_uniform_borders_scene() -> RenderScene {
    RenderScene {
        nodes: (0..144)
            .map(|index| {
                let col = index % 9;
                let row = index / 9;
                let x = 12.0 + col as f32 * 104.0;
                let y = 14.0 + row as f32 * 42.0;
                RenderNode::Primitive(DrawPrimitive::Border(
                    x,
                    y,
                    86.0,
                    28.0,
                    if index % 3 == 0 { 0.0 } else { 8.0 },
                    1.0 + (index % 3) as f32,
                    0x526071FF,
                    BorderStyle::Solid,
                ))
            })
            .collect(),
    }
}

fn solid_edge_borders_scene() -> RenderScene {
    RenderScene {
        nodes: (0..144)
            .map(|index| {
                let col = index % 9;
                let row = index / 9;
                let x = 12.0 + col as f32 * 104.0;
                let y = 14.0 + row as f32 * 42.0;
                let edge = 2.0 + (index % 3) as f32;
                let (top, right, bottom, left) = match index % 4 {
                    0 => (edge, 0.0, 0.0, 0.0),
                    1 => (0.0, edge, 0.0, 0.0),
                    2 => (0.0, 0.0, edge, 0.0),
                    _ => (0.0, 0.0, 0.0, edge),
                };
                RenderNode::Primitive(DrawPrimitive::BorderEdges(
                    x,
                    y,
                    86.0,
                    28.0,
                    0.0,
                    top,
                    right,
                    bottom,
                    left,
                    0x3E536CFF,
                    BorderStyle::Solid,
                ))
            })
            .collect(),
    }
}

fn dashed_borders_scene() -> RenderScene {
    RenderScene {
        nodes: (0..120)
            .map(|index| {
                let col = index % 8;
                let row = index / 8;
                let x = 14.0 + col as f32 * 116.0;
                let y = 16.0 + row as f32 * 44.0;
                RenderNode::Primitive(DrawPrimitive::Border(
                    x,
                    y,
                    94.0,
                    30.0,
                    if index % 2 == 0 { 0.0 } else { 9.0 },
                    1.5 + (index % 3) as f32,
                    0x5E6E82FF,
                    if index % 2 == 0 {
                        BorderStyle::Dashed
                    } else {
                        BorderStyle::Dotted
                    },
                ))
            })
            .collect(),
    }
}

fn borders_like_clean_subtree_nodes() -> Vec<RenderNode> {
    (0..48)
        .flat_map(|index| {
            let col = index % 4;
            let row = index / 4;
            let x = 28.0 + col as f32 * 224.0;
            let y = 28.0 + row as f32 * 52.0;
            let w = 198.0;
            let h = 42.0;
            let radii = Some(CornerRadii {
                tl: 9.0,
                tr: 9.0,
                br: 9.0,
                bl: 9.0,
            });
            let rect = Rect {
                x,
                y,
                width: w,
                height: h,
            };

            [
                RenderNode::Clip {
                    clips: vec![ClipShape { rect, radii }],
                    children: vec![
                        RenderNode::Primitive(DrawPrimitive::RoundedRect(
                            x,
                            y,
                            w,
                            h,
                            9.0,
                            if index % 2 == 0 {
                                0xF7F9FCFF
                            } else {
                                0xEEF3F8FF
                            },
                        )),
                        RenderNode::Primitive(DrawPrimitive::Border(
                            x + 0.5,
                            y + 0.5,
                            w - 1.0,
                            h - 1.0,
                            9.0,
                            1.0 + (index % 3) as f32 * 0.5,
                            0xC8D1DDFF,
                            if index % 5 == 0 {
                                BorderStyle::Dashed
                            } else {
                                BorderStyle::Solid
                            },
                        )),
                        RenderNode::Primitive(DrawPrimitive::TextWithFont(
                            x + 12.0,
                            y + 18.0,
                            format!("Border recipe {index:02}"),
                            12.0,
                            0x273142FF,
                            "default".to_string(),
                            700,
                            false,
                        )),
                        RenderNode::Primitive(DrawPrimitive::TextWithFont(
                            x + 12.0,
                            y + 34.0,
                            "static sibling content".to_string(),
                            11.0,
                            0x667386FF,
                            "default".to_string(),
                            400,
                            false,
                        )),
                    ],
                },
                RenderNode::Primitive(DrawPrimitive::BorderEdges(
                    x,
                    y + h + 4.0,
                    w,
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    0.0,
                    0xD7DEE8FF,
                    BorderStyle::Solid,
                )),
            ]
        })
        .collect()
}

fn assets_like_clean_subtree_nodes() -> Vec<RenderNode> {
    (0..36)
        .flat_map(|index| {
            let col = index % 6;
            let row = index / 6;
            let x = 26.0 + col as f32 * 150.0;
            let y = 28.0 + row as f32 * 96.0;
            let w = 126.0;
            let h = 74.0;
            let rect = Rect {
                x,
                y,
                width: w,
                height: h,
            };
            let radii = Some(CornerRadii {
                tl: 10.0,
                tr: 10.0,
                br: 10.0,
                bl: 10.0,
            });

            [
                RenderNode::Clip {
                    clips: vec![ClipShape { rect, radii }],
                    children: vec![
                        RenderNode::Primitive(DrawPrimitive::Rect(x, y, w, h, 0xF8FAFCFF)),
                        RenderNode::Primitive(DrawPrimitive::Image(
                            x + 8.0,
                            y + 8.0,
                            w - 16.0,
                            42.0,
                            BENCH_IMAGE_ID.to_string(),
                            if index % 2 == 0 {
                                ImageFit::Contain
                            } else {
                                ImageFit::Cover
                            },
                            None,
                        )),
                        RenderNode::Primitive(DrawPrimitive::Border(
                            x + 0.5,
                            y + 0.5,
                            w - 1.0,
                            h - 1.0,
                            10.0,
                            1.0,
                            0xD5DDE8FF,
                            BorderStyle::Solid,
                        )),
                        RenderNode::Primitive(DrawPrimitive::TextWithFont(
                            x + 10.0,
                            y + 64.0,
                            format!("Loaded asset {index:02}"),
                            11.0,
                            0x334155FF,
                            "default".to_string(),
                            500,
                            false,
                        )),
                    ],
                },
                RenderNode::Primitive(DrawPrimitive::TextWithFont(
                    x,
                    y + h + 15.0,
                    "warm image path".to_string(),
                    10.0,
                    0x718096FF,
                    "default".to_string(),
                    400,
                    false,
                )),
            ]
        })
        .collect()
}

fn nerves_counter_content_nodes() -> Vec<RenderNode> {
    let row_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 320.0,
        height: 86.0,
    };
    let row_radii = Some(CornerRadii {
        tl: 6.0,
        tr: 6.0,
        br: 6.0,
        bl: 6.0,
    });

    vec![RenderNode::Clip {
        clips: vec![ClipShape {
            rect: row_rect,
            radii: row_radii,
        }],
        children: [
            vec![
                RenderNode::Primitive(DrawPrimitive::RoundedRect(
                    0.0, 0.0, 320.0, 86.0, 6.0, 0x1E293BFF,
                )),
                RenderNode::Primitive(DrawPrimitive::Border(
                    0.5,
                    0.5,
                    319.0,
                    85.0,
                    6.0,
                    1.0,
                    0x475569FF,
                    BorderStyle::Solid,
                )),
            ],
            nerves_counter_button_nodes(10.0, "+"),
            nerves_counter_label_nodes(),
            nerves_counter_button_nodes(246.0, "-"),
        ]
        .into_iter()
        .flatten()
        .collect(),
    }]
}

fn nerves_counter_button_nodes(x: f32, label: &str) -> Vec<RenderNode> {
    let rect = Rect {
        x,
        y: 10.0,
        width: 64.0,
        height: 66.0,
    };
    let radii = Some(CornerRadii {
        tl: 6.0,
        tr: 6.0,
        br: 6.0,
        bl: 6.0,
    });

    vec![RenderNode::Clip {
        clips: vec![ClipShape { rect, radii }],
        children: vec![
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                x, 10.0, 64.0, 66.0, 6.0, 0x334155FF,
            )),
            RenderNode::Primitive(DrawPrimitive::Border(
                x + 0.5,
                10.5,
                63.0,
                65.0,
                6.0,
                1.0,
                0x64748BFF,
                BorderStyle::Solid,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                x + 26.0,
                52.0,
                label.to_string(),
                28.0,
                0xF8FAFCFF,
                "default".to_string(),
                700,
                false,
            )),
        ],
    }]
}

fn nerves_counter_label_nodes() -> Vec<RenderNode> {
    let rect = Rect {
        x: 84.0,
        y: 10.0,
        width: 152.0,
        height: 66.0,
    };
    let radii = Some(CornerRadii {
        tl: 4.0,
        tr: 4.0,
        br: 4.0,
        bl: 4.0,
    });

    vec![RenderNode::Clip {
        clips: vec![ClipShape { rect, radii }],
        children: vec![
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                84.0, 10.0, 152.0, 66.0, 4.0, 0x334155FF,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                104.0,
                51.0,
                "Count: 123".to_string(),
                20.0,
                0xF8FAFCFF,
                "default".to_string(),
                500,
                false,
            )),
        ],
    }]
}

fn toast_panel_content_nodes() -> Vec<RenderNode> {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 300.0,
        height: 138.0,
    };
    let radii = Some(CornerRadii {
        tl: 12.0,
        tr: 12.0,
        br: 12.0,
        bl: 12.0,
    });

    vec![RenderNode::Clip {
        clips: vec![ClipShape { rect, radii }],
        children: [
            vec![
                RenderNode::Primitive(DrawPrimitive::RoundedRect(
                    0.0, 0.0, 300.0, 138.0, 12.0, 0xF8FAFCFF,
                )),
                RenderNode::Primitive(DrawPrimitive::Border(
                    0.5,
                    0.5,
                    299.0,
                    137.0,
                    12.0,
                    1.0,
                    0xCBD5E1FF,
                    BorderStyle::Solid,
                )),
                RenderNode::Primitive(DrawPrimitive::TextWithFont(
                    18.0,
                    34.0,
                    "Sync complete".to_string(),
                    18.0,
                    0x0F172AFF,
                    "default".to_string(),
                    700,
                    false,
                )),
                RenderNode::Primitive(DrawPrimitive::TextWithFont(
                    18.0,
                    58.0,
                    "Updated 128 records".to_string(),
                    13.0,
                    0x475569FF,
                    "default".to_string(),
                    400,
                    false,
                )),
            ],
            toast_metric_chip_nodes(18.0, 82.0, "Latency", "18 ms"),
            toast_metric_chip_nodes(154.0, 82.0, "Queue", "empty"),
        ]
        .into_iter()
        .flatten()
        .collect(),
    }]
}

fn toast_metric_chip_nodes(x: f32, y: f32, label: &str, value: &str) -> Vec<RenderNode> {
    vec![
        RenderNode::Primitive(DrawPrimitive::RoundedRect(
            x, y, 118.0, 38.0, 8.0, 0xE2E8F0FF,
        )),
        RenderNode::Primitive(DrawPrimitive::TextWithFont(
            x + 10.0,
            y + 15.0,
            label.to_string(),
            10.0,
            0x64748BFF,
            "default".to_string(),
            500,
            false,
        )),
        RenderNode::Primitive(DrawPrimitive::TextWithFont(
            x + 10.0,
            y + 31.0,
            value.to_string(),
            13.0,
            0x1E293BFF,
            "default".to_string(),
            700,
            false,
        )),
    ]
}

fn floating_card_content_nodes() -> Vec<RenderNode> {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 248.0,
        height: 154.0,
    };
    let radii = Some(CornerRadii {
        tl: 10.0,
        tr: 10.0,
        br: 10.0,
        bl: 10.0,
    });

    vec![RenderNode::Clip {
        clips: vec![ClipShape { rect, radii }],
        children: vec![
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                0.0, 0.0, 248.0, 154.0, 10.0, 0x111827FF,
            )),
            RenderNode::Primitive(DrawPrimitive::Border(
                0.5,
                0.5,
                247.0,
                153.0,
                10.0,
                1.0,
                0x374151FF,
                BorderStyle::Solid,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                18.0,
                32.0,
                "Floating action".to_string(),
                17.0,
                0xF9FAFBFF,
                "default".to_string(),
                700,
                false,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                18.0,
                56.0,
                "move_x + move_y".to_string(),
                13.0,
                0xD1D5DBFF,
                "default".to_string(),
                400,
                false,
            )),
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                18.0, 84.0, 96.0, 40.0, 7.0, 0x2563EBFF,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                42.0,
                110.0,
                "Open".to_string(),
                14.0,
                0xFFFFFFFF,
                "default".to_string(),
                700,
                false,
            )),
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                124.0, 84.0, 96.0, 40.0, 7.0, 0x1F2937FF,
            )),
            RenderNode::Primitive(DrawPrimitive::Border(
                124.5,
                84.5,
                95.0,
                39.0,
                7.0,
                1.0,
                0x4B5563FF,
                BorderStyle::Solid,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                146.0,
                110.0,
                "Close".to_string(),
                14.0,
                0xE5E7EBFF,
                "default".to_string(),
                500,
                false,
            )),
        ],
    }]
}

fn app_selector_menu_content_nodes() -> Vec<RenderNode> {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 276.0,
        height: 218.0,
    };
    let radii = Some(CornerRadii {
        tl: 12.0,
        tr: 12.0,
        br: 12.0,
        bl: 12.0,
    });

    vec![RenderNode::Clip {
        clips: vec![ClipShape { rect, radii }],
        children: [
            vec![
                RenderNode::Primitive(DrawPrimitive::RoundedRect(
                    0.0, 0.0, 276.0, 218.0, 12.0, 0xF8FAFCFF,
                )),
                RenderNode::Primitive(DrawPrimitive::Border(
                    0.5,
                    0.5,
                    275.0,
                    217.0,
                    12.0,
                    1.0,
                    0xCBD5E1FF,
                    BorderStyle::Solid,
                )),
                RenderNode::Primitive(DrawPrimitive::TextWithFont(
                    18.0,
                    30.0,
                    "Showcase".to_string(),
                    16.0,
                    0x0F172AFF,
                    "default".to_string(),
                    700,
                    false,
                )),
                RenderNode::Primitive(DrawPrimitive::TextWithFont(
                    18.0,
                    50.0,
                    "Select a demo surface".to_string(),
                    12.0,
                    0x64748BFF,
                    "default".to_string(),
                    400,
                    false,
                )),
            ],
            app_selector_item_nodes(16.0, 66.0, "Layout", "Cached reflow", true),
            app_selector_item_nodes(16.0, 114.0, "Interaction", "Typing and focus", false),
            app_selector_item_nodes(16.0, 162.0, "Assets", "Images and vectors", false),
        ]
        .into_iter()
        .flatten()
        .collect(),
    }]
}

fn app_selector_item_nodes(
    x: f32,
    y: f32,
    label: &str,
    detail: &str,
    active: bool,
) -> Vec<RenderNode> {
    let fill = if active { 0xE0F2FEFF } else { 0xFFFFFFFF };
    let border = if active { 0x0EA5E9FF } else { 0xE2E8F0FF };
    let accent = if active { 0x0284C7FF } else { 0x94A3B8FF };

    vec![
        RenderNode::Primitive(DrawPrimitive::RoundedRect(x, y, 244.0, 38.0, 8.0, fill)),
        RenderNode::Primitive(DrawPrimitive::Border(
            x + 0.5,
            y + 0.5,
            243.0,
            37.0,
            8.0,
            1.0,
            border,
            BorderStyle::Solid,
        )),
        RenderNode::Primitive(DrawPrimitive::RoundedRect(
            x + 10.0,
            y + 10.0,
            18.0,
            18.0,
            5.0,
            accent,
        )),
        RenderNode::Primitive(DrawPrimitive::TextWithFont(
            x + 38.0,
            y + 16.0,
            label.to_string(),
            12.0,
            0x0F172AFF,
            "default".to_string(),
            700,
            false,
        )),
        RenderNode::Primitive(DrawPrimitive::TextWithFont(
            x + 38.0,
            y + 31.0,
            detail.to_string(),
            10.0,
            0x64748BFF,
            "default".to_string(),
            400,
            false,
        )),
    ]
}

fn todo_entry_content_nodes() -> Vec<RenderNode> {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 360.0,
        height: 74.0,
    };
    let radii = Some(CornerRadii {
        tl: 10.0,
        tr: 10.0,
        br: 10.0,
        bl: 10.0,
    });

    vec![RenderNode::Clip {
        clips: vec![ClipShape { rect, radii }],
        children: vec![
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                0.0, 0.0, 360.0, 74.0, 10.0, 0xFFFFFFFF,
            )),
            RenderNode::Primitive(DrawPrimitive::Border(
                0.5,
                0.5,
                359.0,
                73.0,
                10.0,
                1.0,
                0xCBD5E1FF,
                BorderStyle::Solid,
            )),
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                18.0, 24.0, 26.0, 26.0, 13.0, 0xDCFCE7FF,
            )),
            RenderNode::Primitive(DrawPrimitive::Border(
                18.5,
                24.5,
                25.0,
                25.0,
                13.0,
                1.5,
                0x22C55EFF,
                BorderStyle::Solid,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                60.0,
                31.0,
                "Ship animated exit".to_string(),
                15.0,
                0x111827FF,
                "default".to_string(),
                700,
                false,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                60.0,
                52.0,
                "translate + fade common case".to_string(),
                12.0,
                0x64748BFF,
                "default".to_string(),
                400,
                false,
            )),
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                278.0, 22.0, 58.0, 28.0, 14.0, 0xEFF6FFFF,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                294.0,
                40.0,
                "todo".to_string(),
                11.0,
                0x2563EBFF,
                "default".to_string(),
                700,
                false,
            )),
        ],
    }]
}

fn border_clip_heavy_scene() -> RenderScene {
    RenderScene {
        nodes: (0..84)
            .map(|index| {
                let col = index % 7;
                let row = index / 7;
                let x = 18.0 + col as f32 * 132.0;
                let y = 18.0 + row as f32 * 54.0;
                let rect = Rect {
                    x,
                    y,
                    width: 112.0,
                    height: 38.0,
                };
                let radii = Some(CornerRadii {
                    tl: 8.0,
                    tr: 8.0,
                    br: 8.0,
                    bl: 8.0,
                });
                RenderNode::Clip {
                    clips: vec![ClipShape { rect, radii }],
                    children: vec![
                        RenderNode::Primitive(DrawPrimitive::Rect(
                            x,
                            y,
                            rect.width,
                            rect.height,
                            if index % 2 == 0 {
                                0xF6F8FAFF
                            } else {
                                0xEEF3F7FF
                            },
                        )),
                        RenderNode::Primitive(DrawPrimitive::Border(
                            x + 0.5,
                            y + 0.5,
                            rect.width - 1.0,
                            rect.height - 1.0,
                            8.0,
                            1.5 + (index % 3) as f32,
                            0x596579FF,
                            match index % 5 {
                                0 => BorderStyle::Dashed,
                                1 => BorderStyle::Dotted,
                                _ => BorderStyle::Solid,
                            },
                        )),
                    ],
                }
            })
            .collect(),
    }
}

fn template_tinted_images_scene() -> RenderScene {
    image_grid_scene(Some(0x2F80EDFF))
}

fn raster_images_scene() -> RenderScene {
    image_grid_scene(None)
}

fn image_grid_scene(tint: Option<u32>) -> RenderScene {
    RenderScene {
        nodes: (0..96)
            .map(|index| {
                let col = index % 8;
                let row = index / 8;
                RenderNode::Primitive(DrawPrimitive::Image(
                    18.0 + col as f32 * 112.0,
                    18.0 + row as f32 * 54.0,
                    92.0,
                    42.0,
                    BENCH_IMAGE_ID.to_string(),
                    if index % 3 == 0 {
                        ImageFit::Cover
                    } else {
                        ImageFit::Contain
                    },
                    tint,
                ))
            })
            .collect(),
    }
}

fn alpha_single_primitive_scene() -> RenderScene {
    RenderScene {
        nodes: (0..144)
            .map(|index| {
                let col = index % 9;
                let row = index / 9;
                RenderNode::Alpha {
                    alpha: 0.45 + (index % 4) as f32 * 0.1,
                    children: vec![RenderNode::Primitive(DrawPrimitive::RoundedRect(
                        12.0 + col as f32 * 104.0,
                        14.0 + row as f32 * 42.0,
                        86.0,
                        28.0,
                        7.0,
                        0x246B9FFF,
                    ))],
                }
            })
            .collect(),
    }
}

fn alpha_group_overlap_scene() -> RenderScene {
    RenderScene {
        nodes: (0..80)
            .map(|index| {
                let col = index % 8;
                let row = index / 8;
                let x = 16.0 + col as f32 * 116.0;
                let y = 18.0 + row as f32 * 58.0;
                RenderNode::Alpha {
                    alpha: 0.62,
                    children: vec![
                        RenderNode::Primitive(DrawPrimitive::RoundedRect(
                            x, y, 64.0, 34.0, 8.0, 0x1E6A8DFF,
                        )),
                        RenderNode::Primitive(DrawPrimitive::RoundedRect(
                            x + 28.0,
                            y + 10.0,
                            64.0,
                            34.0,
                            8.0,
                            0xC85252FF,
                        )),
                    ],
                }
            })
            .collect(),
    }
}

fn shadow_mask_filter_scene() -> RenderScene {
    RenderScene {
        nodes: (0..24)
            .flat_map(|index| {
                let col = index % 6;
                let row = index / 6;
                let x = 26.0 + col as f32 * 150.0;
                let y = 32.0 + row as f32 * 120.0;
                let w = 118.0;
                let h = 76.0;
                vec![
                    RenderNode::ShadowPass {
                        children: vec![RenderNode::Primitive(DrawPrimitive::Shadow(
                            x,
                            y,
                            w,
                            h,
                            0.0,
                            10.0,
                            20.0 + (index % 4) as f32 * 2.0,
                            0.0,
                            14.0,
                            0x1B243040,
                        ))],
                    },
                    RenderNode::Primitive(DrawPrimitive::RoundedRect(x, y, w, h, 14.0, 0xFFFFFFFF)),
                    RenderNode::Primitive(DrawPrimitive::TextWithFont(
                        x + 14.0,
                        y + 34.0,
                        format!("Card {index}"),
                        15.0,
                        0x202936FF,
                        "default".to_string(),
                        700,
                        false,
                    )),
                ]
            })
            .collect(),
    }
}

fn gradient_rects_scene() -> RenderScene {
    RenderScene {
        nodes: (0..120)
            .map(|index| {
                let col = index % 8;
                let row = index / 8;
                RenderNode::Primitive(DrawPrimitive::Gradient(
                    14.0 + col as f32 * 116.0,
                    16.0 + row as f32 * 44.0,
                    94.0,
                    30.0,
                    0xDDEBFFFF,
                    0x557AA6FF,
                    (index % 12) as f32 * 15.0,
                ))
            })
            .collect(),
    }
}

fn clip_rect_vs_rrect_scene() -> RenderScene {
    RenderScene {
        nodes: (0..120)
            .map(|index| {
                let col = index % 8;
                let row = index / 8;
                let x = 14.0 + col as f32 * 116.0;
                let y = 16.0 + row as f32 * 44.0;
                RenderNode::Clip {
                    clips: vec![ClipShape {
                        rect: Rect {
                            x,
                            y,
                            width: 94.0,
                            height: 30.0,
                        },
                        radii: (index % 2 == 1).then_some(CornerRadii {
                            tl: 8.0,
                            tr: 8.0,
                            br: 8.0,
                            bl: 8.0,
                        }),
                    }],
                    children: vec![RenderNode::Primitive(DrawPrimitive::Gradient(
                        x - 6.0,
                        y - 4.0,
                        106.0,
                        38.0,
                        0xEEF6FFFF,
                        0x496B9AFF,
                        45.0,
                    ))],
                }
            })
            .collect(),
    }
}

fn mixed_ui_scene() -> RenderScene {
    let background = vec![
        RenderNode::Primitive(DrawPrimitive::Rect(
            0.0,
            0.0,
            WIDTH as f32,
            HEIGHT as f32,
            0xF4F7FAFF,
        )),
        RenderNode::Primitive(DrawPrimitive::Gradient(
            0.0,
            0.0,
            WIDTH as f32,
            120.0,
            0xEAF2FFFF,
            0xF4F7FAFF,
            90.0,
        )),
    ];

    let cards = (0..18).flat_map(|index| {
        let col = index % 3;
        let row = index / 3;
        let x = 28.0 + col as f32 * 300.0;
        let y = 30.0 + row as f32 * 108.0;
        let w = 260.0;
        let h = 84.0;
        vec![
            RenderNode::ShadowPass {
                children: vec![RenderNode::Primitive(DrawPrimitive::Shadow(
                    x, y, w, h, 0.0, 6.0, 14.0, 0.0, 10.0, 0x11182726,
                ))],
            },
            RenderNode::Clip {
                clips: vec![ClipShape {
                    rect: Rect {
                        x,
                        y,
                        width: w,
                        height: h,
                    },
                    radii: Some(CornerRadii {
                        tl: 10.0,
                        tr: 10.0,
                        br: 10.0,
                        bl: 10.0,
                    }),
                }],
                children: vec![
                    RenderNode::Primitive(DrawPrimitive::RoundedRect(x, y, w, h, 10.0, 0xFFFFFFFF)),
                    RenderNode::Primitive(DrawPrimitive::Border(
                        x + 0.5,
                        y + 0.5,
                        w - 1.0,
                        h - 1.0,
                        10.0,
                        1.0,
                        0xD2D8E0FF,
                        BorderStyle::Solid,
                    )),
                    RenderNode::Primitive(DrawPrimitive::TextWithFont(
                        x + 18.0,
                        y + 32.0,
                        format!("Metric {index}"),
                        15.0,
                        0x2F3744FF,
                        "default".to_string(),
                        700,
                        false,
                    )),
                    RenderNode::Primitive(DrawPrimitive::TextWithFont(
                        x + 18.0,
                        y + 58.0,
                        "stable renderer baseline".to_string(),
                        13.0,
                        0x677385FF,
                        "default".to_string(),
                        400,
                        false,
                    )),
                ],
            },
        ]
    });

    let overlays = (0..12).map(|index| {
        let x = 70.0 + index as f32 * 64.0;
        RenderNode::Transform {
            transform: Affine2::translation(x, 660.0).then(Affine2::rotation_degrees(index as f32)),
            children: vec![RenderNode::Alpha {
                alpha: 0.72,
                children: vec![RenderNode::Primitive(DrawPrimitive::RoundedRect(
                    0.0, 0.0, 42.0, 22.0, 6.0, 0x375F9AFF,
                ))],
            }],
        }
    });

    RenderScene {
        nodes: background
            .into_iter()
            .chain(cards)
            .chain(overlays)
            .collect(),
    }
}

fn shadow_utils_paths() -> Vec<Path> {
    (0..24)
        .map(|index| {
            let col = index % 6;
            let row = index / 6;
            let x = 26.0 + col as f32 * 150.0;
            let y = 32.0 + row as f32 * 120.0;
            let rect = SkRect::from_xywh(x, y, 118.0, 76.0);
            let mut builder = PathBuilder::new();
            builder.add_rrect(
                RRect::new_rect_xy(rect, 14.0, 14.0),
                PathDirection::CW,
                None,
            );
            builder.detach()
        })
        .collect()
}

#[cfg(target_os = "linux")]
const EGL_PLATFORM_SURFACELESS_MESA: EGLenum = 0x31DD;

#[cfg(target_os = "linux")]
type RawEglGetProcAddress = unsafe extern "system" fn(*const std::ffi::c_char) -> *const c_void;

#[cfg(target_os = "linux")]
struct EglBenchSurface {
    egl: egl::Egl,
    _egl_lib: Library,
    display: EGLDisplay,
    context: EGLContext,
    surface: EGLSurface,
    frame_surface: Option<GlFrameSurface>,
}

#[cfg(target_os = "linux")]
impl EglBenchSurface {
    fn new(dimensions: (u32, u32)) -> Result<Self, String> {
        let (egl_lib, egl_api) = load_egl()?;
        let (display, context, surface) = init_surfaceless_egl(&egl_api, dimensions)?;
        let frame_surface = create_gpu_frame_surface(&egl_api, dimensions)?;

        Ok(Self {
            egl: egl_api,
            _egl_lib: egl_lib,
            display,
            context,
            surface,
            frame_surface: Some(frame_surface),
        })
    }

    fn frame(&mut self) -> RenderFrame<'_> {
        self.frame_surface
            .as_mut()
            .expect("EGL bench frame surface should exist")
            .frame()
    }
}

#[cfg(target_os = "linux")]
impl Drop for EglBenchSurface {
    fn drop(&mut self) {
        self.frame_surface.take();
        unsafe {
            let _ = self.egl.MakeCurrent(
                self.display,
                egl::NO_SURFACE,
                egl::NO_SURFACE,
                egl::NO_CONTEXT,
            );
            let _ = self.egl.DestroySurface(self.display, self.surface);
            let _ = self.egl.DestroyContext(self.display, self.context);
            let _ = self.egl.Terminate(self.display);
        }
    }
}

#[cfg(target_os = "linux")]
fn load_egl() -> Result<(Library, egl::Egl), String> {
    let lib =
        unsafe { Library::new("libEGL.so.1") }.map_err(|err| format!("load libEGL: {err}"))?;
    let get_proc = unsafe {
        *lib.get::<RawEglGetProcAddress>(b"eglGetProcAddress\0")
            .map_err(|err| format!("load eglGetProcAddress: {err}"))?
    };

    let egl = egl::Egl::load_with(|name| unsafe {
        let symbol = CString::new(name).expect("EGL symbol name should not contain nul");
        let ptr = get_proc(symbol.as_ptr());
        if !ptr.is_null() {
            return ptr;
        }

        let raw = format!("{name}\0");
        lib.get::<*const c_void>(raw.as_bytes())
            .map(|symbol| *symbol)
            .unwrap_or(ptr::null())
    });

    Ok((lib, egl))
}

#[cfg(target_os = "linux")]
fn init_surfaceless_egl(
    egl: &egl::Egl,
    dimensions: (u32, u32),
) -> Result<(EGLDisplay, EGLContext, EGLSurface), String> {
    let display = if egl.GetPlatformDisplayEXT.is_loaded() {
        unsafe {
            egl.GetPlatformDisplayEXT(EGL_PLATFORM_SURFACELESS_MESA, ptr::null_mut(), ptr::null())
        }
    } else if egl.GetPlatformDisplay.is_loaded() {
        unsafe {
            egl.GetPlatformDisplay(EGL_PLATFORM_SURFACELESS_MESA, ptr::null_mut(), ptr::null())
        }
    } else {
        unsafe { egl.GetDisplay(egl::DEFAULT_DISPLAY as egl::EGLNativeDisplayType) }
    };
    if display == egl::NO_DISPLAY {
        return Err("eglGetPlatformDisplay surfaceless returned NO_DISPLAY".to_string());
    }

    let mut major: EGLint = 0;
    let mut minor: EGLint = 0;
    if unsafe { egl.Initialize(display, &mut major, &mut minor) } == egl::FALSE {
        return Err("eglInitialize failed".to_string());
    }

    if unsafe { egl.BindAPI(egl::OPENGL_ES_API) } == egl::FALSE {
        return Err("eglBindAPI(OpenGL ES) failed".to_string());
    }

    let config_attribs: [EGLint; 13] = [
        egl::SURFACE_TYPE as EGLint,
        egl::PBUFFER_BIT as EGLint,
        egl::RENDERABLE_TYPE as EGLint,
        egl::OPENGL_ES2_BIT as EGLint,
        egl::RED_SIZE as EGLint,
        8,
        egl::GREEN_SIZE as EGLint,
        8,
        egl::BLUE_SIZE as EGLint,
        8,
        egl::ALPHA_SIZE as EGLint,
        8,
        egl::NONE as EGLint,
    ];

    let mut config: EGLConfig = ptr::null();
    let mut num_configs: EGLint = 0;
    if unsafe {
        egl.ChooseConfig(
            display,
            config_attribs.as_ptr(),
            &mut config,
            1,
            &mut num_configs,
        )
    } == egl::FALSE
        || num_configs == 0
    {
        return Err("eglChooseConfig failed".to_string());
    }

    let context_attribs: [EGLint; 3] = [
        egl::CONTEXT_CLIENT_VERSION as EGLint,
        2,
        egl::NONE as EGLint,
    ];
    let context =
        unsafe { egl.CreateContext(display, config, egl::NO_CONTEXT, context_attribs.as_ptr()) };
    if context == egl::NO_CONTEXT {
        return Err("eglCreateContext failed".to_string());
    }

    let surface_attribs: [EGLint; 5] = [
        egl::WIDTH as EGLint,
        dimensions.0 as EGLint,
        egl::HEIGHT as EGLint,
        dimensions.1 as EGLint,
        egl::NONE as EGLint,
    ];
    let surface = unsafe { egl.CreatePbufferSurface(display, config, surface_attribs.as_ptr()) };
    if surface == egl::NO_SURFACE {
        unsafe {
            let _ = egl.DestroyContext(display, context);
            let _ = egl.Terminate(display);
        }
        return Err("eglCreatePbufferSurface failed".to_string());
    }

    if unsafe { egl.MakeCurrent(display, surface, surface, context) } == egl::FALSE {
        unsafe {
            let _ = egl.DestroySurface(display, surface);
            let _ = egl.DestroyContext(display, context);
            let _ = egl.Terminate(display);
        }
        return Err("eglMakeCurrent failed".to_string());
    }

    unsafe {
        let _ = egl.SwapInterval(display, 0);
    }

    Ok((display, context, surface))
}

#[cfg(target_os = "linux")]
fn create_gpu_frame_surface(
    egl: &egl::Egl,
    dimensions: (u32, u32),
) -> Result<GlFrameSurface, String> {
    gl::load_with(|name| unsafe {
        let symbol = CString::new(name).expect("GL symbol name should not contain nul");
        egl.GetProcAddress(symbol.as_ptr()) as *const _
    });

    let interface = skia_safe::gpu::gl::Interface::new_load_with(|name| unsafe {
        if name == "eglGetCurrentDisplay" {
            return ptr::null();
        }

        let symbol = CString::new(name).expect("GL symbol name should not contain nul");
        egl.GetProcAddress(symbol.as_ptr()) as *const _
    })
    .ok_or_else(|| "could not create Skia GL interface".to_string())?;

    let gr_context = skia_safe::gpu::direct_contexts::make_gl(interface, None)
        .ok_or_else(|| "could not create Skia GL direct context".to_string())?;

    let fb_info = {
        let mut fboid: i32 = 0;
        unsafe { gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut fboid) };

        skia_safe::gpu::gl::FramebufferInfo {
            fboid: fboid as u32,
            format: skia_safe::gpu::gl::Format::RGBA8.into(),
            ..Default::default()
        }
    };

    Ok(GlFrameSurface::new(dimensions, fb_info, gr_context, 0, 0))
}

criterion_group!(
    benches,
    bench_renderer_raster_direct,
    bench_renderer_direct_candidates,
    bench_renderer_clean_subtree_cache_candidates,
    bench_renderer_translated_cache_candidates,
    bench_renderer_layout_reflow_cache_candidates,
    bench_renderer_cache_children,
    bench_renderer_cold_frames,
    bench_renderer_gpu_surfaceless,
    bench_renderer_gpu_cold_frames,
    bench_renderer_gpu_cache_candidates,
    bench_renderer_gpu_translated_cache_candidates,
    bench_renderer_gpu_layout_reflow_cache_candidates,
    bench_renderer_gpu_cache_children
);
criterion_main!(benches);
