mod support;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use emerge_skia::assets::AssetRuntime;
#[cfg(target_os = "linux")]
use emerge_skia::backend::skia_gpu::GlFrameSurface;
#[cfg(target_os = "linux")]
use emerge_skia::events::RegistryRebuildPayload;
use emerge_skia::render_scene::{
    DrawPrimitive, PaintLayerPlacement, PaintLayerPolicy, PaintLayerReason, RenderNode,
    RenderPaintLayer, RenderScene, RenderSceneSummary,
};
use emerge_skia::renderer::{
    RenderFrame, RenderState, RendererCacheConfig, RendererCachePaintLayerFrameStats,
    RendererPaintLayerCacheConfig, SceneRenderer, insert_raster_asset,
};
#[cfg(target_os = "linux")]
use emerge_skia::tree::animation::AnimationRuntime;
use emerge_skia::tree::attrs::{Attrs, BorderStyle, ImageFit, Length, Padding};
#[cfg(target_os = "linux")]
use emerge_skia::tree::deserialize::decode_tree;
#[cfg(target_os = "linux")]
use emerge_skia::tree::element::{Element, ElementKind, ElementTree, Frame, NearbySlot, NodeId};
use emerge_skia::tree::geometry::{ClipShape, CornerRadii, Rect};
#[cfg(target_os = "linux")]
use emerge_skia::tree::layout::{
    Constraint, layout_and_refresh_default, layout_and_refresh_default_with_animation,
    layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark,
    layout_or_refresh_default_with_animation_reusing_clean_registry_for_benchmark,
};
#[cfg(target_os = "linux")]
use emerge_skia::tree::patch::{Patch, apply_patches};
use emerge_skia::tree::transform::Affine2;
#[cfg(target_os = "linux")]
use glutin_egl_sys::egl;
#[cfg(target_os = "linux")]
use glutin_egl_sys::egl::types::{EGLConfig, EGLContext, EGLDisplay, EGLSurface, EGLenum, EGLint};
#[cfg(target_os = "linux")]
use libloading::Library;
use skia_safe::{
    AlphaType, Color, ColorType, ImageInfo, Path, PathBuilder, PathDirection, Point3, RRect,
    Rect as SkRect, surfaces, utils::shadow_utils::ShadowFlags,
};
use std::hint::black_box;
use std::sync::Once;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};
#[cfg(target_os = "linux")]
use std::{
    ffi::{CStr, CString},
    os::raw::c_void,
    ptr,
};
#[cfg(target_os = "linux")]
use support::scrollable_rich_borders_shadow_showcase;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 720;
const BENCH_IMAGE_ID: &str = "renderer_bench_static";
static BENCH_ASSETS: Once = Once::new();

#[cfg(target_os = "linux")]
const EMERGE_DEMO_SHOWCASE_LAYOUT_EMRG: &[u8] =
    include_bytes!("../../../bench/external_fixtures/emerge_demo_showcase_layout/full.emrg");
#[cfg(target_os = "linux")]
const EMERGE_DEMO_SHOWCASE_BORDERS_EMRG: &[u8] =
    include_bytes!("../../../bench/external_fixtures/emerge_demo_showcase_borders/full.emrg");
#[cfg(target_os = "linux")]
const CAMERA_ACTIVE_SHUTTER_SLIDER_EMRG: [&[u8]; 8] = [
    include_bytes!("../../../bench/external_fixtures/camera_active_slider/phase_0.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_slider/phase_1.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_slider/phase_2.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_slider/phase_3.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_slider/phase_4.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_slider/phase_5.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_slider/phase_6.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_slider/phase_7.emrg"),
];
#[cfg(target_os = "linux")]
const CAMERA_ACTIVE_FOCUS_SLIDER_EMRG: [&[u8]; 8] = [
    include_bytes!("../../../bench/external_fixtures/camera_active_focus_slider/phase_0.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_focus_slider/phase_1.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_focus_slider/phase_2.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_focus_slider/phase_3.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_focus_slider/phase_4.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_focus_slider/phase_5.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_focus_slider/phase_6.emrg"),
    include_bytes!("../../../bench/external_fixtures/camera_active_focus_slider/phase_7.emrg"),
];
#[cfg(target_os = "linux")]
const CAMERA_ACTIVE_SLIDER_WIDTH: u32 = 1440;
#[cfg(target_os = "linux")]
const CAMERA_ACTIVE_SLIDER_HEIGHT: u32 = 2560;
#[cfg(target_os = "linux")]
const CAMERA_ACTIVE_SHUTTER_SLIDER_ID: u64 = 48;
#[cfg(target_os = "linux")]
const CAMERA_ACTIVE_FOCUS_SLIDER_ID: u64 = 74;
#[cfg(target_os = "linux")]
const EMERGE_DEMO_SHOWCASE_LAYOUT_FRAME_MS: [u64; 8] = [0, 16, 32, 48, 64, 80, 96, 112];
#[cfg(target_os = "linux")]
const EMERGE_DEMO_SHOWCASE_LAYOUT_SCROLL_STEP: f32 = 8.0;
#[cfg(target_os = "linux")]
const EMERGE_DEMO_SHOWCASE_BORDERS_FRAME_MS: [u64; 8] = [0, 16, 32, 48, 64, 80, 96, 112];
#[cfg(target_os = "linux")]
const EMERGE_DEMO_SHOWCASE_BORDERS_SCROLL_STEP: f32 = 8.0;
#[cfg(target_os = "linux")]
const RICH_BORDERS_SHOWCASE_FRAME_MS: [u64; 8] = [0, 16, 32, 48, 64, 80, 96, 112];
#[cfg(target_os = "linux")]
const RICH_BORDERS_SHOWCASE_SCROLL_STEP: f32 = 8.0;
#[cfg(target_os = "linux")]
const EMERGE_DEMO_SHOWCASE_BORDERS_SCREENSHOT_WIDTH: u32 = 1909;
#[cfg(target_os = "linux")]
const EMERGE_DEMO_SHOWCASE_BORDERS_SCREENSHOT_HEIGHT: u32 = 2148;
#[cfg(target_os = "linux")]
const EMERGE_DEMO_SHOWCASE_BORDERS_SCREENSHOT_SCALE: f32 = 1.5;
const EMERGE_DEMO_SHOWCASE_LAYOUT_VIEWPORTS: &[(u32, u32)] = &[
    (800, 600),
    (960, 720),
    (1024, 768),
    (1200, 720),
    (1280, 720),
    (1280, 800),
    (1366, 768),
    (1440, 900),
];
#[cfg(target_os = "linux")]
const EMERGE_DEMO_SHOWCASE_BORDERS_VIEWPORTS: &[(u32, u32)] = &[
    (1909, 2148),
    (1920, 1080),
    (1440, 900),
    (1280, 720),
    (960, 900),
];

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
fn bench_renderer_paint_layer_cache(c: &mut Criterion) {
    let Ok(surface_probe) = EglBenchSurface::new((WIDTH, HEIGHT)) else {
        eprintln!("Skipping native/renderer/paint_layer_cache: EGL surfaceless setup failed");
        return;
    };
    eprintln!(
        "paint-layer benchmark GL_RENDERER={} GL_VERSION={}",
        current_gl_string(gl::RENDERER),
        current_gl_string(gl::VERSION)
    );
    drop(surface_probe);

    let mut group = c.benchmark_group("native/renderer/paint_layer_cache");

    group.throughput(Throughput::Elements(
        scrolling_paint_layer_scene(0.0).summary().nodes as u64,
    ));
    group.bench_function("scrolling/direct_children", |b| {
        let states = scrolling_direct_states();
        let mut state_index = 0usize;
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let state = &states[state_index];
            state_index = (state_index + 1) % states.len();
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.bench_function("scrolling/cache_reposition_hits", |b| {
        let states = scrolling_paint_layer_states();
        let mut state_index = 2usize;
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
            enabled: true,
            ..RendererCacheConfig::default()
        });
        assert_paint_layer_cache_store_then_hit(
            &mut renderer,
            &mut surface,
            &states[0],
            &states[1],
            "scrolling",
            true,
        );

        b.iter(|| {
            let state = &states[state_index];
            state_index = (state_index + 1) % states.len();
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.throughput(Throughput::Elements(
        animated_paint_layer_scene(0).summary().nodes as u64,
    ));
    group.bench_function("animation/direct_children", |b| {
        let states = animated_direct_states();
        let mut state_index = 0usize;
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let state = &states[state_index];
            state_index = (state_index + 1) % states.len();
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.bench_function("animation/cache_static_hits_dynamic_slot", |b| {
        let states = animated_paint_layer_states();
        let mut state_index = 2usize;
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
            enabled: true,
            ..RendererCacheConfig::default()
        });
        assert_paint_layer_cache_store_then_hit(
            &mut renderer,
            &mut surface,
            &states[0],
            &states[1],
            "animation",
            false,
        );

        b.iter(|| {
            let state = &states[state_index];
            state_index = (state_index + 1) % states.len();
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.throughput(Throughput::Elements(
        offscreen_layout_animation_scene(0).summary().nodes as u64,
    ));
    group.bench_function("offscreen_layout_animation/cache_steady_hits", |b| {
        let states = offscreen_layout_animation_states();
        let mut state_index = 2usize;
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
            enabled: true,
            ..RendererCacheConfig::default()
        });
        assert_offscreen_layout_animation_steady_hits(&mut renderer, &mut surface, &states);

        b.iter(|| {
            let state = &states[state_index];
            state_index = (state_index + 1) % states.len();
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.bench_function("offscreen_layout_animation/visible_frame_noop_skip", |b| {
        let states = offscreen_layout_animation_states();
        let mut state_index;
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
            enabled: true,
            ..RendererCacheConfig::default()
        });
        state_index = assert_offscreen_layout_animation_visible_frame_skip(
            &mut renderer,
            &mut surface,
            &states,
        );

        b.iter(|| {
            let state = &states[state_index];
            state_index = (state_index + 1) % states.len();
            let skipped = renderer.can_skip_unchanged_visible_frame(state, (WIDTH, HEIGHT));
            assert!(
                skipped,
                "offscreen layout animation visible frame should be skippable"
            );
            black_box(skipped);
        });
    });

    group.throughput(Throughput::Elements(
        stable_descendant_layout_animation_scene(0).summary().nodes as u64,
    ));
    group.bench_function(
        "stable_descendant_layout_animation/cache_steady_hits",
        |b| {
            let states = stable_descendant_layout_animation_states();
            let mut state_index = 2usize;
            let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
                .expect("EGL surfaceless setup should stay available after probe");
            let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
                enabled: true,
                ..RendererCacheConfig::default()
            });
            assert_stable_descendant_layout_animation_hits(&mut renderer, &mut surface, &states);

            b.iter(|| {
                let state = &states[state_index];
                state_index = (state_index + 1) % states.len();
                let mut frame = surface.frame();
                black_box(renderer.render(&mut frame, state));
            });
        },
    );

    group.throughput(Throughput::Elements(
        scroll_return_scene(0.0).summary().nodes as u64,
    ));
    group.bench_function("scroll_return/cache_after_clipped_frames", |b| {
        let states = [
            scroll_return_state(0.0),
            scroll_return_state(160.0),
            scroll_return_state(160.0),
            scroll_return_state(0.0),
        ];
        let mut state_index = 0usize;
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::with_cache_config(scroll_return_cache_config());
        assert_scroll_return_cache_reuses_after_clipped_frames(
            &mut renderer,
            &mut surface,
            &states,
        );

        b.iter(|| {
            let state = &states[state_index];
            state_index = (state_index + 1) % states.len();
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, state));
        });
    });

    let page = emerge_demo_showcase_layout_page_benchmark();
    group.throughput(Throughput::Elements(page.summary.nodes as u64));
    group.bench_function("emerge_demo_showcase_layout_page/cache_steady_hits", |b| {
        let mut state_index = 2usize;
        let mut surface = EglBenchSurface::new((page.width, page.height))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
            enabled: true,
            ..RendererCacheConfig::default()
        });
        assert_emerge_demo_showcase_layout_page_steady_hits(&mut renderer, &mut surface, &page);

        b.iter(|| {
            let state = &page.states[state_index];
            state_index = (state_index + 1) % page.states.len();
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, state));
        });
    });

    group.bench_function(
        "emerge_demo_showcase_layout_page/cache_steady_hits_8mb_entry_128mb_total",
        |b| {
            let mut state_index = 2usize;
            let mut surface = EglBenchSurface::new((page.width, page.height))
                .expect("EGL surfaceless setup should stay available after probe");
            let mut renderer = SceneRenderer::with_cache_config(small_payload_cache_config());
            for state in page.states.iter().take(2) {
                let _ = render_paint_layer_cache_stats(&mut renderer, &mut surface, state);
            }

            b.iter(|| {
                let state = &page.states[state_index];
                state_index = (state_index + 1) % page.states.len();
                let mut frame = surface.frame();
                black_box(renderer.render(&mut frame, state));
            });
        },
    );

    let borders = rich_borders_showcase_benchmark();
    group.throughput(Throughput::Elements(borders.summary.nodes as u64));
    group.bench_function("rich_borders_showcase/cache_steady_hits", |b| {
        let mut state_index = 2usize;
        let mut surface = EglBenchSurface::new((borders.width, borders.height))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
            enabled: true,
            ..RendererCacheConfig::default()
        });
        assert_rich_borders_showcase_cache_hits(&mut renderer, &mut surface, &borders);

        b.iter(|| {
            let state = &borders.states[state_index];
            state_index = (state_index + 1) % borders.states.len();
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, state));
        });
    });

    let demo_borders = emerge_demo_showcase_borders_benchmark();
    group.throughput(Throughput::Elements(demo_borders.summary.nodes as u64));
    group.bench_function("emerge_demo_showcase_borders/cache_steady_hits", |b| {
        let mut state_index = 2usize;
        let mut surface = EglBenchSurface::new((demo_borders.width, demo_borders.height))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
            enabled: true,
            ..RendererCacheConfig::default()
        });
        assert_emerge_demo_showcase_borders_steady_hits(&mut renderer, &mut surface, &demo_borders);

        b.iter(|| {
            let state = &demo_borders.states[state_index];
            state_index = (state_index + 1) % demo_borders.states.len();
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, state));
        });
    });

    let demo_borders_screenshot = emerge_demo_showcase_borders_screenshot_benchmark();
    group.throughput(Throughput::Elements(
        demo_borders_screenshot.summary.nodes as u64,
    ));
    group.bench_function(
        "emerge_demo_showcase_borders/screenshot_1909x2148_scale_1_5/cache_steady_hits",
        |b| {
            let mut state_index = 2usize;
            let mut surface = EglBenchSurface::new((
                demo_borders_screenshot.width,
                demo_borders_screenshot.height,
            ))
            .expect("EGL surfaceless setup should stay available after probe");
            let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
                enabled: true,
                ..RendererCacheConfig::default()
            });
            assert_emerge_demo_showcase_borders_steady_hits(
                &mut renderer,
                &mut surface,
                &demo_borders_screenshot,
            );

            b.iter(|| {
                let state = &demo_borders_screenshot.states[state_index];
                state_index = (state_index + 1) % demo_borders_screenshot.states.len();
                let mut frame = surface.frame();
                black_box(renderer.render(&mut frame, state));
            });
        },
    );
    group.bench_function(
        "emerge_demo_showcase_borders/screenshot_1909x2148_scale_1_5/refresh_scene",
        |b| {
            let mut case = emerge_demo_showcase_borders_screenshot_refresh_benchmark();
            b.iter(|| {
                black_box(case.refresh_next_frame());
            });
        },
    );
    group.bench_function(
        "emerge_demo_showcase_borders/screenshot_1909x2148_scale_1_5/hover_transition_replay",
        |b| {
            let replay = emerge_demo_showcase_borders_screenshot_hover_replay();
            let mut state_index = 0usize;
            let mut surface = EglBenchSurface::new((replay.width, replay.height))
                .expect("EGL surfaceless setup should stay available after probe");
            let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
                enabled: true,
                ..RendererCacheConfig::default()
            });
            assert_emerge_demo_showcase_borders_hover_transition_bounds(
                &mut renderer,
                &mut surface,
                &replay,
            );

            b.iter(|| {
                let state = &replay.transition_states[state_index];
                state_index = (state_index + 1) % replay.transition_states.len();
                let mut frame = surface.frame();
                black_box(renderer.render(&mut frame, state));
            });
        },
    );

    group.throughput(Throughput::Elements(
        large_simple_paint_layer_scene().summary().nodes as u64,
    ));
    group.bench_function("large_simple_layer/cache_disabled_direct", |b| {
        let state = large_simple_paint_layer_state();
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::new();

        b.iter(|| {
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, &state));
        });
    });

    group.bench_function("large_simple_layer/cache_low_value_bypass", |b| {
        let state = large_simple_paint_layer_state();
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
            enabled: true,
            ..RendererCacheConfig::default()
        });
        assert_large_simple_layer_bypasses_cache(&mut renderer, &mut surface, &state);

        b.iter(|| {
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, &state));
        });
    });

    group.throughput(Throughput::Elements(
        text_heavy_paint_layer_scene().summary().nodes as u64,
    ));
    group.bench_function("text_heavy_layer/cache_hits", |b| {
        let state = text_heavy_paint_layer_state();
        let mut surface = EglBenchSurface::new((WIDTH, HEIGHT))
            .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
            enabled: true,
            ..RendererCacheConfig::default()
        });
        assert_text_heavy_layer_cache_hits(&mut renderer, &mut surface, &state);

        b.iter(|| {
            let mut frame = surface.frame();
            black_box(renderer.render(&mut frame, &state));
        });
    });

    let camera_active_shutter_states = camera_active_slider_fixture_states(
        &CAMERA_ACTIVE_SHUTTER_SLIDER_EMRG,
        NodeId::from_u64(CAMERA_ACTIVE_SHUTTER_SLIDER_ID),
    );
    bench_camera_active_slider(
        &mut group,
        "camera_active_shutter_slider/frame_sequence_gpu_complete",
        &camera_active_shutter_states,
        true,
    );

    let camera_active_focus_states = camera_active_slider_fixture_states(
        &CAMERA_ACTIVE_FOCUS_SLIDER_EMRG,
        NodeId::from_u64(CAMERA_ACTIVE_FOCUS_SLIDER_ID),
    );
    bench_camera_active_slider(
        &mut group,
        "camera_active_focus_slider/frame_sequence_gpu_complete",
        &camera_active_focus_states,
        true,
    );
    bench_camera_active_slider(
        &mut group,
        "camera_active_focus_slider/frame_sequence_cache_disabled_gpu_complete",
        &camera_active_focus_states,
        false,
    );

    group.finish();
}

#[cfg(target_os = "linux")]
fn bench_camera_active_slider(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    states: &[RenderState],
    cache_enabled: bool,
) {
    states.iter().enumerate().for_each(|(phase, state)| {
        let summary = state.scene.summary();
        assert_eq!(summary.paint_layers, 9, "phase={phase} {summary:?}");
        assert_eq!(summary.direct_only_layers, 1, "phase={phase} {summary:?}");
        assert_eq!(summary.videos, 1, "phase={phase} {summary:?}");
        let expected_focus_shadow = usize::from(phase > 0);
        assert_eq!(
            summary.shadow_passes, expected_focus_shadow,
            "phase={phase} {summary:?}"
        );
        assert_eq!(
            summary.shadows, expected_focus_shadow,
            "phase={phase} {summary:?}"
        );
    });
    group.throughput(Throughput::Elements(states[1].scene.summary().nodes as u64));
    group.bench_function(name, |b| {
        let mut state_index = 1usize;
        let mut surface =
            EglBenchSurface::new((CAMERA_ACTIVE_SLIDER_WIDTH, CAMERA_ACTIVE_SLIDER_HEIGHT))
                .expect("EGL surfaceless setup should stay available after probe");
        let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
            enabled: cache_enabled,
            max_new_payloads_per_frame: 64,
            ..RendererCacheConfig::default()
        });

        if cache_enabled {
            let cold = render_paint_layer_cache_stats(&mut renderer, &mut surface, &states[0]);
            let warm = render_paint_layer_cache_stats(&mut renderer, &mut surface, &states[0]);
            let active = render_paint_layer_cache_stats(&mut renderer, &mut surface, &states[1]);
            assert!(cold.stores > 0, "Camera fixture did not warm: {cold:?}");
            assert!(warm.hits > 0, "Camera fixture did not hit: {warm:?}");
            assert!(
                active.hits > 0 && active.misses.saturating_add(active.rejected_admission) > 0,
                "Camera fixture should mix static hits and changing direct runs: {active:?}"
            );
        } else {
            [&states[0], &states[0], &states[1]]
                .into_iter()
                .for_each(|state| {
                    let mut frame = surface.frame();
                    let _ = renderer.render(&mut frame, state);
                });
        }
        // Setup rendering is asynchronous. Complete it before Criterion starts timing so the
        // first measured phase drains only work submitted by that phase.
        unsafe { gl::Finish() };
        b.iter(|| {
            let state = &states[state_index];
            state_index = (state_index + 1) % states.len().max(2);
            if state_index == 0 {
                state_index = 1;
            }
            let timings = {
                let mut frame = surface.frame();
                renderer.render(&mut frame, state)
            };
            // Surfaceless EGL does not present or otherwise throttle. Include completion so
            // Criterion measures submitted GPU work rather than only command enqueue time.
            unsafe { gl::Finish() };
            black_box(timings);
        });
    });
}

#[cfg(not(target_os = "linux"))]
fn bench_renderer_paint_layer_cache(_c: &mut Criterion) {}

struct RenderCase {
    name: &'static str,
    scene: RenderScene,
}

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

#[cfg(target_os = "linux")]
const PAINT_LAYER_SCROLL_OFFSETS: [f32; 8] = [0.0, 5.0, 12.0, 21.0, 31.0, 42.0, 54.0, 67.0];
#[cfg(target_os = "linux")]
const PAINT_LAYER_ANIMATION_PHASES: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

#[cfg(target_os = "linux")]
fn assert_paint_layer_cache_store_then_hit(
    renderer: &mut SceneRenderer,
    surface: &mut EglBenchSurface,
    first_state: &RenderState,
    second_state: &RenderState,
    label: &str,
    _expect_moved_hit: bool,
) {
    let first_stats = {
        let mut frame = surface.frame();
        renderer
            .render(&mut frame, first_state)
            .renderer_cache
            .expect("paint-layer cache warm frame should produce cache stats")
            .paint_layer
    };
    assert!(
        first_stats.stores > 0,
        "{label} paint-layer cache did not store on the warm frame: {first_stats:?}"
    );

    let second_stats = {
        let mut frame = surface.frame();
        renderer
            .render(&mut frame, second_state)
            .renderer_cache
            .expect("paint-layer cache hit frame should produce cache stats")
            .paint_layer
    };
    assert!(
        second_stats.hits > 0,
        "{label} paint-layer cache did not hit after warmup: {second_stats:?}"
    );
}

#[cfg(target_os = "linux")]
fn render_paint_layer_cache_stats(
    renderer: &mut SceneRenderer,
    surface: &mut EglBenchSurface,
    state: &RenderState,
) -> RendererCachePaintLayerFrameStats {
    let mut frame = surface.frame();
    renderer
        .render(&mut frame, state)
        .renderer_cache
        .expect("paint-layer cache benchmark frame should produce cache stats")
        .paint_layer
}

#[cfg(target_os = "linux")]
fn steady_paint_layer_coverage(stats: RendererCachePaintLayerFrameStats) -> u64 {
    stats.hits.saturating_add(stats.bypassed_low_value)
}

#[cfg(target_os = "linux")]
struct EmergeDemoShowcaseLayoutPageBenchmark {
    states: Vec<RenderState>,
    width: u32,
    height: u32,
    scroll_y: f32,
    summary: RenderSceneSummary,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct EmergeDemoShowcaseLayoutTarget {
    width: u32,
    height: u32,
    scroll_id: NodeId,
    scroll_y: f32,
    summary: RenderSceneSummary,
    score: usize,
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_layout_page_benchmark() -> EmergeDemoShowcaseLayoutPageBenchmark {
    let started_at = Instant::now();
    let tree = decode_tree(EMERGE_DEMO_SHOWCASE_LAYOUT_EMRG)
        .expect("emerge_demo showcase layout fixture should decode");
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, started_at);

    let target = emerge_demo_showcase_layout_target(&tree, &runtime, started_at);
    let states = emerge_demo_showcase_layout_states(&tree, &runtime, started_at, target);
    let summary = states
        .first()
        .expect("emerge_demo showcase layout benchmark should build states")
        .scene
        .summary();

    assert!(
        summary.nodes >= 300 && summary.primitives >= 100 && summary.texts >= 50,
        "emerge_demo showcase layout benchmark did not select the expected rich layout scene: \
         size={}x{}, scroll_y={}, score={}, summary={summary:?}",
        target.width,
        target.height,
        target.scroll_y,
        target.score
    );
    assert!(
        summary.paint_layers >= 2,
        "emerge_demo showcase layout benchmark did not emit paint layers: \
         size={}x{}, scroll_y={}, score={}, summary={summary:?}",
        target.width,
        target.height,
        target.scroll_y,
        target.score
    );

    EmergeDemoShowcaseLayoutPageBenchmark {
        states,
        width: target.width,
        height: target.height,
        scroll_y: target.scroll_y,
        summary,
    }
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_layout_states(
    tree: &ElementTree,
    runtime: &AnimationRuntime,
    started_at: Instant,
    target: EmergeDemoShowcaseLayoutTarget,
) -> Vec<RenderState> {
    let mut tree = tree.clone();
    let initial = layout_and_refresh_default_with_animation(
        &mut tree,
        emerge_demo_showcase_layout_constraint(target.width, target.height),
        1.0,
        runtime,
        started_at,
    );
    tree.apply_scroll_y(&target.scroll_id, -target.scroll_y);

    EMERGE_DEMO_SHOWCASE_LAYOUT_FRAME_MS
        .iter()
        .enumerate()
        .map(|(index, frame_ms)| {
            let update =
                layout_or_refresh_default_with_animation_reusing_clean_registry_for_benchmark(
                    &mut tree,
                    emerge_demo_showcase_layout_constraint(target.width, target.height),
                    1.0,
                    runtime,
                    started_at + Duration::from_millis(*frame_ms),
                    Some(&initial.event_rebuild),
                );
            RenderState::new(update.output.scene, Color::WHITE, index as u64 + 1, false)
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn assert_emerge_demo_showcase_layout_page_steady_hits(
    renderer: &mut SceneRenderer,
    surface: &mut EglBenchSurface,
    page: &EmergeDemoShowcaseLayoutPageBenchmark,
) {
    assert!(
        page.summary.nodes >= 300 && page.summary.primitives >= 100 && page.summary.texts >= 50,
        "{:?}",
        page.summary
    );
    assert!(
        page.summary.moving_layers > 0,
        "emerge_demo showcase layout page did not emit scroll-moving paint layers: \
         size={}x{}, scroll_y={}, summary={:?}",
        page.width,
        page.height,
        page.scroll_y,
        page.summary
    );

    let warm_stats = render_paint_layer_cache_stats(renderer, surface, &page.states[0]);
    assert!(
        warm_stats.stores > 0,
        "emerge_demo showcase layout page did not warm paint-layer payloads: \
         scroll_y={}, summary={:?}, stats={warm_stats:?}",
        page.scroll_y,
        page.summary
    );

    let second_warm_stats = render_paint_layer_cache_stats(renderer, surface, &page.states[1]);
    let steady_stats = render_paint_layer_cache_stats(renderer, surface, &page.states[2]);
    assert!(
        steady_paint_layer_coverage(steady_stats) >= warm_stats.visible_candidates,
        "emerge_demo showcase layout page lost warmed visible paint-layer payload coverage: \
         scroll_y={}, summary={:?}, stats={steady_stats:?}",
        page.scroll_y,
        page.summary
    );
    assert!(steady_stats.misses <= 1, "{steady_stats:?}");
    assert!(steady_stats.stores <= 1, "{steady_stats:?}");
    assert_eq!(steady_stats.evictions, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.stale_evictions, 0, "{steady_stats:?}");
    assert!(steady_stats.gpu_payload_stores <= 1, "{steady_stats:?}");

    let mut draw_total = Duration::ZERO;
    let mut draw_count = 0u32;
    for state in page.states.iter().skip(3) {
        let timings = {
            let mut frame = surface.frame();
            renderer.render(&mut frame, state)
        };
        let stats = timings
            .renderer_cache
            .as_ref()
            .expect("steady paint-layer cache frame should produce cache stats")
            .paint_layer;
        assert!(
            steady_paint_layer_coverage(stats) >= warm_stats.visible_candidates,
            "{stats:?}"
        );
        assert!(stats.misses <= 1, "{stats:?}");
        assert!(stats.stores <= 1, "{stats:?}");
        assert_eq!(stats.evictions, 0, "{stats:?}");
        assert_eq!(stats.stale_evictions, 0, "{stats:?}");
        assert!(stats.gpu_payload_stores <= 1, "{stats:?}");
        draw_total += timings.draw;
        draw_count += 1;
    }
    let draw_avg = draw_total / draw_count;
    if emerge_bench_diagnostics_enabled() {
        eprintln!(
            "emerge_demo showcase layout page: scroll_y={}, summary={:?}, warm={:?}, second_warm={:?}, steady={:?}, steady_draw_avg={:?}",
            page.scroll_y, page.summary, warm_stats, second_warm_stats, steady_stats, draw_avg
        );
    }
    assert!(
        draw_avg < Duration::from_micros(500),
        "emerge_demo showcase layout page steady draw average exceeded target: \
         draw_avg={draw_avg:?}, scroll_y={}, summary={:?}",
        page.scroll_y,
        page.summary
    );
}

#[cfg(target_os = "linux")]
struct EmergeDemoShowcaseBordersBenchmark {
    states: Vec<RenderState>,
    width: u32,
    height: u32,
    scale: f32,
    scroll_y: f32,
    summary: RenderSceneSummary,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct EmergeDemoShowcaseBordersTarget {
    width: u32,
    height: u32,
    scale: f32,
    scroll_id: NodeId,
    scroll_y: f32,
    summary: RenderSceneSummary,
    score: usize,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct EmergeDemoShowcaseBordersViewport {
    width: u32,
    height: u32,
    scale: f32,
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_borders_benchmark() -> EmergeDemoShowcaseBordersBenchmark {
    let started_at = Instant::now();
    let tree = decode_tree(EMERGE_DEMO_SHOWCASE_BORDERS_EMRG)
        .expect("emerge_demo showcase Borders fixture should decode");
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, started_at);

    let target = emerge_demo_showcase_borders_target(&tree, &runtime, started_at);
    let states = emerge_demo_showcase_borders_states(&tree, &runtime, started_at, target);
    let summary = states
        .first()
        .expect("emerge_demo showcase Borders benchmark should build states")
        .scene
        .summary();

    assert!(
        summary.nodes >= 500 && summary.primitives >= 150 && summary.texts >= 100,
        "emerge_demo showcase Borders benchmark did not select the expected rich scene: \
         size={}x{}, scroll_y={}, score={}, summary={summary:?}",
        target.width,
        target.height,
        target.scroll_y,
        target.score
    );
    assert!(
        summary.paint_layers >= 6 && summary.cacheable_layers > 0,
        "emerge_demo showcase Borders benchmark did not select the animated Borders viewport: \
         size={}x{}, scroll_y={}, score={}, summary={summary:?}",
        target.width,
        target.height,
        target.scroll_y,
        target.score
    );

    EmergeDemoShowcaseBordersBenchmark {
        states,
        width: target.width,
        height: target.height,
        scale: target.scale,
        scroll_y: target.scroll_y,
        summary,
    }
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_borders_screenshot_benchmark() -> EmergeDemoShowcaseBordersBenchmark {
    let started_at = Instant::now();
    let tree = decode_tree(EMERGE_DEMO_SHOWCASE_BORDERS_EMRG)
        .expect("emerge_demo showcase Borders fixture should decode");
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, started_at);

    let target = emerge_demo_showcase_borders_exact_target(
        &tree,
        &runtime,
        started_at,
        EMERGE_DEMO_SHOWCASE_BORDERS_SCREENSHOT_WIDTH,
        EMERGE_DEMO_SHOWCASE_BORDERS_SCREENSHOT_HEIGHT,
        EMERGE_DEMO_SHOWCASE_BORDERS_SCREENSHOT_SCALE,
    );
    let states = emerge_demo_showcase_borders_states(&tree, &runtime, started_at, target);
    let summary = states
        .first()
        .expect("emerge_demo showcase Borders screenshot benchmark should build states")
        .scene
        .summary();

    assert!(
        summary.nodes >= 500 && summary.primitives >= 150 && summary.texts >= 100,
        "emerge_demo showcase Borders screenshot benchmark did not select the expected rich scene: \
         size={}x{} scale={} scroll_y={}, score={}, summary={summary:?}",
        target.width,
        target.height,
        target.scale,
        target.scroll_y,
        target.score
    );
    assert!(
        summary.paint_layers >= 6 && summary.cacheable_layers > 0,
        "emerge_demo showcase Borders screenshot benchmark did not select the animated Borders viewport: \
         size={}x{} scale={} scroll_y={}, score={}, summary={summary:?}",
        target.width,
        target.height,
        target.scale,
        target.scroll_y,
        target.score
    );

    EmergeDemoShowcaseBordersBenchmark {
        states,
        width: target.width,
        height: target.height,
        scale: target.scale,
        scroll_y: target.scroll_y,
        summary,
    }
}

#[cfg(target_os = "linux")]
struct EmergeDemoShowcaseBordersRefreshBenchmark {
    tree: ElementTree,
    runtime: AnimationRuntime,
    started_at: Instant,
    cached_rebuild: RegistryRebuildPayload,
    width: u32,
    height: u32,
    scale: f32,
    next_frame: u64,
}

#[cfg(target_os = "linux")]
impl EmergeDemoShowcaseBordersRefreshBenchmark {
    fn refresh_next_frame(&mut self) -> (bool, usize) {
        self.next_frame = self.next_frame.saturating_add(1);
        let update = layout_or_refresh_default_with_animation_reusing_clean_registry_for_benchmark(
            &mut self.tree,
            emerge_demo_showcase_borders_constraint(self.width, self.height),
            self.scale,
            &self.runtime,
            self.started_at + Duration::from_millis(self.next_frame.saturating_mul(16)),
            Some(&self.cached_rebuild),
        );

        (update.layout_performed, update.output.scene.nodes.len())
    }
}

#[cfg(target_os = "linux")]
struct EmergeDemoShowcaseBordersHoverReplay {
    warm_state: RenderState,
    transition_states: Vec<RenderState>,
    width: u32,
    height: u32,
    scale: f32,
    scroll_y: f32,
    summary: RenderSceneSummary,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Default)]
struct EmergeDemoShowcaseBordersHoverReplayMaxFrame {
    total: Duration,
    draw: Duration,
    flush: Duration,
    gpu_flush: Duration,
    stores: u64,
    misses: u64,
    prepare_successes: u64,
    prepare_time: Duration,
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_borders_screenshot_hover_replay() -> EmergeDemoShowcaseBordersHoverReplay {
    let started_at = Instant::now();
    let tree = decode_tree(EMERGE_DEMO_SHOWCASE_BORDERS_EMRG)
        .expect("emerge_demo showcase Borders fixture should decode");
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, started_at);
    let target = emerge_demo_showcase_borders_exact_target(
        &tree,
        &runtime,
        started_at,
        EMERGE_DEMO_SHOWCASE_BORDERS_SCREENSHOT_WIDTH,
        EMERGE_DEMO_SHOWCASE_BORDERS_SCREENSHOT_HEIGHT,
        EMERGE_DEMO_SHOWCASE_BORDERS_SCREENSHOT_SCALE,
    );

    let constraint = emerge_demo_showcase_borders_constraint(target.width, target.height);
    let mut tree = tree.clone();
    let initial = layout_and_refresh_default_with_animation(
        &mut tree,
        constraint,
        target.scale,
        &runtime,
        started_at,
    );
    tree.apply_scroll_y(&target.scroll_id, -target.scroll_y);
    let warm = layout_or_refresh_default_with_animation_reusing_clean_registry_for_benchmark(
        &mut tree,
        constraint,
        target.scale,
        &runtime,
        started_at,
        Some(&initial.event_rebuild),
    );
    let mut cached_rebuild = if warm.output.event_rebuild_changed {
        warm.output.event_rebuild
    } else {
        initial.event_rebuild
    };
    let summary = warm.output.scene.summary();
    assert!(
        summary.nodes >= 500 && summary.texts >= 100 && summary.paint_layers >= 8,
        "emerge_demo showcase Borders hover replay selected the wrong scene: \
         target=({}x{} scale={} scroll_y={}), summary={summary:?}",
        target.width,
        target.height,
        target.scale,
        target.scroll_y
    );

    let hover_ids = visible_hover_targets(&tree, target.width, target.height);
    assert!(
        hover_ids.len() >= 3,
        "emerge_demo showcase Borders hover replay needs several visible hover targets: \
         target=({}x{} scale={} scroll_y={}), summary={summary:?}, hover_ids={hover_ids:?}",
        target.width,
        target.height,
        target.scale,
        target.scroll_y
    );

    let mut active_nearby_id = None;
    let transition_states = (0..hover_ids.len().max(8))
        .map(|index| {
            let next_id = hover_ids[index % hover_ids.len()];
            let subtree_seed = 920_000 + index as u64 * 100;
            let subtree = nearby_code_block_subtree(subtree_seed);
            let subtree_root_id = subtree
                .root_id()
                .expect("nearby code block subtree should have a root");
            let mut patches = active_nearby_id
                .map(|id| vec![Patch::Remove { id }])
                .unwrap_or_default();
            patches.push(Patch::InsertNearbySubtree {
                host_id: next_id,
                index: 0,
                slot: NearbySlot::Above,
                subtree,
            });
            let invalidation =
                apply_patches(&mut tree, patches).expect("nearby hover transition patch applies");
            active_nearby_id = Some(subtree_root_id);
            let update = layout_or_refresh_default_with_animation_and_invalidation_reusing_clean_registry_for_benchmark(
                &mut tree,
                constraint,
                target.scale,
                &runtime,
                started_at + Duration::from_millis((index as u64 + 1).saturating_mul(16)),
                invalidation,
                Some(&cached_rebuild),
            );
            if update.output.event_rebuild_changed {
                cached_rebuild = update.output.event_rebuild.clone();
            }
            RenderState::new(update.output.scene, Color::WHITE, index as u64 + 2, false)
        })
        .collect();

    EmergeDemoShowcaseBordersHoverReplay {
        warm_state: RenderState::new(warm.output.scene, Color::WHITE, 1, false),
        transition_states,
        width: target.width,
        height: target.height,
        scale: target.scale,
        scroll_y: target.scroll_y,
        summary,
    }
}

#[cfg(target_os = "linux")]
fn assert_emerge_demo_showcase_borders_hover_transition_bounds(
    renderer: &mut SceneRenderer,
    surface: &mut EglBenchSurface,
    replay: &EmergeDemoShowcaseBordersHoverReplay,
) {
    let warm_stats = render_paint_layer_cache_stats(renderer, surface, &replay.warm_state);
    assert!(
        warm_stats.stores > 0,
        "emerge_demo showcase Borders hover replay did not warm payloads: \
         scale={} scroll_y={} summary={:?} stats={warm_stats:?}",
        replay.scale,
        replay.scroll_y,
        replay.summary
    );
    let steady_warm_stats = render_paint_layer_cache_stats(renderer, surface, &replay.warm_state);
    assert_eq!(steady_warm_stats.misses, 0, "{steady_warm_stats:?}");
    assert_eq!(steady_warm_stats.stores, 0, "{steady_warm_stats:?}");

    let max_frame = replay.transition_states.iter().fold(
        EmergeDemoShowcaseBordersHoverReplayMaxFrame::default(),
        |mut max_frame, state| {
            let timings = {
                let mut frame = surface.frame();
                renderer.render(&mut frame, state)
            };
            let stats = timings
                .renderer_cache
                .as_ref()
                .expect("hover transition replay should produce cache stats")
                .paint_layer;
            max_frame.total = max_frame.total.max(timings.total);
            max_frame.draw = max_frame.draw.max(timings.draw);
            max_frame.flush = max_frame.flush.max(timings.flush);
            max_frame.gpu_flush = max_frame.gpu_flush.max(timings.gpu_flush);
            max_frame.stores = max_frame.stores.max(stats.stores);
            max_frame.misses = max_frame.misses.max(stats.misses);
            max_frame.prepare_successes = max_frame.prepare_successes.max(stats.prepare_successes);
            max_frame.prepare_time = max_frame.prepare_time.max(stats.prepare_time);
            max_frame
        },
    );

    assert!(
        max_frame.stores <= 4,
        "emerge_demo showcase Borders hover transition burst-stored too many payloads: \
         scale={} scroll_y={} summary={:?} max={max_frame:?}",
        replay.scale,
        replay.scroll_y,
        replay.summary
    );
    assert!(
        max_frame.prepare_time <= Duration::from_millis(2),
        "emerge_demo showcase Borders hover transition spent too long preparing payloads: \
         scale={} scroll_y={} summary={:?} max={max_frame:?}",
        replay.scale,
        replay.scroll_y,
        replay.summary
    );

    if emerge_bench_diagnostics_enabled() {
        eprintln!(
            "emerge_demo showcase Borders hover transition replay: scale={} scroll_y={} summary={:?} warm={:?} steady_warm={:?} max={:?}",
            replay.scale, replay.scroll_y, replay.summary, warm_stats, steady_warm_stats, max_frame
        );
    }
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_borders_screenshot_refresh_benchmark()
-> EmergeDemoShowcaseBordersRefreshBenchmark {
    let started_at = Instant::now();
    let tree = decode_tree(EMERGE_DEMO_SHOWCASE_BORDERS_EMRG)
        .expect("emerge_demo showcase Borders fixture should decode");
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, started_at);
    let target = emerge_demo_showcase_borders_exact_target(
        &tree,
        &runtime,
        started_at,
        EMERGE_DEMO_SHOWCASE_BORDERS_SCREENSHOT_WIDTH,
        EMERGE_DEMO_SHOWCASE_BORDERS_SCREENSHOT_HEIGHT,
        EMERGE_DEMO_SHOWCASE_BORDERS_SCREENSHOT_SCALE,
    );

    let mut tree = tree.clone();
    let initial = layout_and_refresh_default_with_animation(
        &mut tree,
        emerge_demo_showcase_borders_constraint(target.width, target.height),
        target.scale,
        &runtime,
        started_at,
    );
    tree.apply_scroll_y(&target.scroll_id, -target.scroll_y);
    let warm = layout_or_refresh_default_with_animation_reusing_clean_registry_for_benchmark(
        &mut tree,
        emerge_demo_showcase_borders_constraint(target.width, target.height),
        target.scale,
        &runtime,
        started_at,
        Some(&initial.event_rebuild),
    );
    let cached_rebuild = if warm.output.event_rebuild_changed {
        warm.output.event_rebuild
    } else {
        initial.event_rebuild
    };
    let second = layout_or_refresh_default_with_animation_reusing_clean_registry_for_benchmark(
        &mut tree,
        emerge_demo_showcase_borders_constraint(target.width, target.height),
        target.scale,
        &runtime,
        started_at + Duration::from_millis(16),
        Some(&cached_rebuild),
    );
    assert!(
        !second.layout_performed,
        "emerge_demo showcase Borders screenshot refresh benchmark should stay refresh-only: \
         target={:?}, summary={:?}",
        (target.width, target.height, target.scale, target.scroll_y),
        target.summary
    );

    if emerge_bench_diagnostics_enabled() {
        eprintln!(
            "emerge_demo showcase Borders screenshot refresh target: size={}x{} scale={} scroll_y={} score={} summary={:?}",
            target.width,
            target.height,
            target.scale,
            target.scroll_y,
            target.score,
            target.summary
        );
    }

    EmergeDemoShowcaseBordersRefreshBenchmark {
        tree,
        runtime,
        started_at,
        cached_rebuild,
        width: target.width,
        height: target.height,
        scale: target.scale,
        next_frame: 1,
    }
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_borders_states(
    tree: &ElementTree,
    runtime: &AnimationRuntime,
    started_at: Instant,
    target: EmergeDemoShowcaseBordersTarget,
) -> Vec<RenderState> {
    let mut tree = tree.clone();
    let initial = layout_and_refresh_default_with_animation(
        &mut tree,
        emerge_demo_showcase_borders_constraint(target.width, target.height),
        target.scale,
        runtime,
        started_at,
    );
    tree.apply_scroll_y(&target.scroll_id, -target.scroll_y);

    EMERGE_DEMO_SHOWCASE_BORDERS_FRAME_MS
        .iter()
        .enumerate()
        .map(|(index, frame_ms)| {
            let update =
                layout_or_refresh_default_with_animation_reusing_clean_registry_for_benchmark(
                    &mut tree,
                    emerge_demo_showcase_borders_constraint(target.width, target.height),
                    target.scale,
                    runtime,
                    started_at + Duration::from_millis(*frame_ms),
                    Some(&initial.event_rebuild),
                );
            RenderState::new(update.output.scene, Color::WHITE, index as u64 + 1, false)
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_borders_target(
    tree: &ElementTree,
    runtime: &AnimationRuntime,
    started_at: Instant,
) -> EmergeDemoShowcaseBordersTarget {
    let target = EMERGE_DEMO_SHOWCASE_BORDERS_VIEWPORTS
        .iter()
        .filter_map(|(width, height)| {
            let mut layout_tree = tree.clone();
            layout_and_refresh_default_with_animation(
                &mut layout_tree,
                emerge_demo_showcase_borders_constraint(*width, *height),
                1.0,
                runtime,
                started_at,
            );
            let scroll_id = largest_vertical_scroll_node(&layout_tree)?;
            let (scroll_y, summary, score) = emerge_demo_showcase_borders_target_scroll_y(
                &layout_tree,
                scroll_id,
                runtime,
                started_at,
                EmergeDemoShowcaseBordersViewport {
                    width: *width,
                    height: *height,
                    scale: 1.0,
                },
            );

            Some(EmergeDemoShowcaseBordersTarget {
                width: *width,
                height: *height,
                scale: 1.0,
                scroll_id,
                scroll_y,
                summary,
                score,
            })
        })
        .min_by_key(|target| target.score)
        .expect("emerge_demo showcase Borders page should have a vertical scroll container");

    if emerge_bench_diagnostics_enabled() {
        eprintln!(
            "emerge_demo showcase Borders selected target: size={}x{} scroll_y={} score={} summary={:?}",
            target.width, target.height, target.scroll_y, target.score, target.summary
        );
    }

    target
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_borders_exact_target(
    tree: &ElementTree,
    runtime: &AnimationRuntime,
    started_at: Instant,
    width: u32,
    height: u32,
    scale: f32,
) -> EmergeDemoShowcaseBordersTarget {
    let mut layout_tree = tree.clone();
    layout_and_refresh_default_with_animation(
        &mut layout_tree,
        emerge_demo_showcase_borders_constraint(width, height),
        scale,
        runtime,
        started_at,
    );
    let scroll_id = largest_vertical_scroll_node(&layout_tree)
        .expect("emerge_demo showcase Borders page should have a vertical scroll container");
    let (scroll_y, summary, score) = emerge_demo_showcase_borders_target_scroll_y(
        &layout_tree,
        scroll_id,
        runtime,
        started_at,
        EmergeDemoShowcaseBordersViewport {
            width,
            height,
            scale,
        },
    );
    let target = EmergeDemoShowcaseBordersTarget {
        width,
        height,
        scale,
        scroll_id,
        scroll_y,
        summary,
        score,
    };

    if emerge_bench_diagnostics_enabled() {
        eprintln!(
            "emerge_demo showcase Borders exact target: size={}x{} scale={} scroll_y={} score={} summary={:?}",
            target.width,
            target.height,
            target.scale,
            target.scroll_y,
            target.score,
            target.summary
        );
    }

    target
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_borders_target_scroll_y(
    tree: &ElementTree,
    scroll_id: NodeId,
    runtime: &AnimationRuntime,
    started_at: Instant,
    viewport: EmergeDemoShowcaseBordersViewport,
) -> (f32, RenderSceneSummary, usize) {
    let max_y = tree
        .get(&scroll_id)
        .map(|element| element.layout.scroll_y_max.max(0.0))
        .unwrap_or(0.0);
    let sample_count = (max_y / EMERGE_DEMO_SHOWCASE_BORDERS_SCROLL_STEP).ceil() as usize;

    (0..=sample_count)
        .map(|sample| {
            let scroll_y = (sample as f32 * EMERGE_DEMO_SHOWCASE_BORDERS_SCROLL_STEP).min(max_y);
            let summary = emerge_demo_showcase_borders_summary_at_scroll(
                tree, scroll_id, runtime, started_at, scroll_y, viewport,
            );
            (
                emerge_demo_showcase_borders_target_score(summary),
                scroll_y,
                summary,
            )
        })
        .min_by_key(|(score, _, _)| *score)
        .map(|(score, scroll_y, summary)| (scroll_y, summary, score))
        .unwrap_or((0.0, RenderSceneSummary::default(), usize::MAX))
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_borders_summary_at_scroll(
    tree: &ElementTree,
    scroll_id: NodeId,
    runtime: &AnimationRuntime,
    started_at: Instant,
    scroll_y: f32,
    viewport: EmergeDemoShowcaseBordersViewport,
) -> RenderSceneSummary {
    let mut frame_tree = tree.clone();
    frame_tree.apply_scroll_y(&scroll_id, -scroll_y);
    layout_and_refresh_default_with_animation(
        &mut frame_tree,
        emerge_demo_showcase_borders_constraint(viewport.width, viewport.height),
        viewport.scale,
        runtime,
        started_at,
    )
    .scene
    .summary()
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_borders_target_score(summary: RenderSceneSummary) -> usize {
    summary.nodes.abs_diff(750)
        + summary.primitives.abs_diff(281) * 8
        + summary.texts.abs_diff(201) * 4
        + summary.shadows.abs_diff(14) * 8
        + summary.inset_shadows.abs_diff(0) * 8
        + summary.gradients.abs_diff(0) * 8
        + summary.borders.abs_diff(18) * 4
        + summary.paint_layers.abs_diff(7) * 16
        + summary.moving_layers.abs_diff(1) * 8
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_borders_constraint(width: u32, height: u32) -> Constraint {
    Constraint::new(width as f32, height as f32)
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
fn frame_intersects_viewport(frame: Frame, width: f32, height: f32) -> bool {
    frame.x < width
        && frame.y < height
        && frame.x + frame.width > 0.0
        && frame.y + frame.height > 0.0
}

#[cfg(target_os = "linux")]
fn nearby_code_block_subtree(seed: u64) -> ElementTree {
    let mut tree = ElementTree::new();
    let root_id = NodeId::from_u64(seed);
    tree.set_root_id(root_id);
    tree.insert(Element::with_attrs(
        root_id,
        ElementKind::Column,
        Vec::new(),
        Attrs {
            width: Some(Length::Px(460.0)),
            padding: Some(Padding::Uniform(12.0)),
            spacing: Some(4.0),
            ..Default::default()
        },
    ));

    let child_ids: Vec<NodeId> = [
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
        let id = NodeId::from_u64(seed + 1 + index as u64);
        tree.insert(Element::with_attrs(
            id,
            ElementKind::Text,
            Vec::new(),
            Attrs {
                content: Some((*line).to_string()),
                font_size: Some(if index == 0 { 11.0 } else { 12.0 }),
                ..Default::default()
            },
        ));
        id
    })
    .collect();
    tree.set_children(&root_id, child_ids)
        .expect("code lines should attach");
    tree
}

#[cfg(target_os = "linux")]
fn assert_emerge_demo_showcase_borders_steady_hits(
    renderer: &mut SceneRenderer,
    surface: &mut EglBenchSurface,
    page: &EmergeDemoShowcaseBordersBenchmark,
) {
    let warm_stats = render_paint_layer_cache_stats(renderer, surface, &page.states[0]);
    assert!(
        warm_stats.stores > 0,
        "emerge_demo showcase Borders did not warm paint-layer payloads: \
         scale={}, scroll_y={}, summary={:?}, stats={warm_stats:?}",
        page.scale,
        page.scroll_y,
        page.summary
    );

    let second_warm_stats = render_paint_layer_cache_stats(renderer, surface, &page.states[1]);
    let steady_stats = render_paint_layer_cache_stats(renderer, surface, &page.states[2]);
    assert!(
        steady_paint_layer_coverage(steady_stats) >= warm_stats.visible_candidates,
        "emerge_demo showcase Borders lost warmed cache-hit coverage: \
         scale={}, scroll_y={}, summary={:?}, stats={steady_stats:?}",
        page.scale,
        page.scroll_y,
        page.summary
    );
    assert_eq!(steady_stats.misses, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.stores, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.evictions, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.stale_evictions, 0, "{steady_stats:?}");

    let mut total = Duration::ZERO;
    let mut draw = Duration::ZERO;
    let mut flush = Duration::ZERO;
    let mut count = 0u32;
    for state in page.states.iter().skip(3) {
        let timings = {
            let mut frame = surface.frame();
            renderer.render(&mut frame, state)
        };
        let stats = timings
            .renderer_cache
            .as_ref()
            .expect("steady emerge_demo Borders frame should produce cache stats")
            .paint_layer;
        assert!(
            steady_paint_layer_coverage(stats) >= warm_stats.visible_candidates,
            "{stats:?}"
        );
        assert_eq!(stats.misses, 0, "{stats:?}");
        assert_eq!(stats.stores, 0, "{stats:?}");
        assert_eq!(stats.evictions, 0, "{stats:?}");
        assert_eq!(stats.stale_evictions, 0, "{stats:?}");
        total += timings.total;
        draw += timings.draw;
        flush += timings.flush;
        count += 1;
    }
    if emerge_bench_diagnostics_enabled() {
        eprintln!(
            "emerge_demo showcase Borders: scale={}, scroll_y={}, summary={:?}, warm={:?}, second_warm={:?}, steady={:?}, steady_total_avg={:?}, steady_draw_avg={:?}, steady_flush_avg={:?}",
            page.scale,
            page.scroll_y,
            page.summary,
            warm_stats,
            second_warm_stats,
            steady_stats,
            total / count,
            draw / count,
            flush / count
        );
    }
}

#[cfg(target_os = "linux")]
struct RichBordersShowcaseBenchmark {
    states: Vec<RenderState>,
    width: u32,
    height: u32,
    scroll_y: f32,
    summary: RenderSceneSummary,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct RichBordersShowcaseTarget {
    width: u32,
    height: u32,
    scroll_id: NodeId,
    scroll_y: f32,
    summary: RenderSceneSummary,
    score: usize,
}

#[cfg(target_os = "linux")]
fn rich_borders_showcase_benchmark() -> RichBordersShowcaseBenchmark {
    let started_at = Instant::now();
    let tree = scrollable_rich_borders_shadow_showcase();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, started_at);

    let target = rich_borders_showcase_target(&tree, &runtime, started_at);
    let states = rich_borders_showcase_states(&tree, &runtime, started_at, target);
    let summary = states
        .first()
        .expect("rich borders showcase benchmark should build states")
        .scene
        .summary();

    assert!(
        summary.nodes >= 500 && summary.primitives >= 150 && summary.texts >= 100,
        "rich borders benchmark did not select the expected rich viewport: \
         size={}x{}, scroll_y={}, score={}, summary={summary:?}",
        target.width,
        target.height,
        target.scroll_y,
        target.score
    );
    assert!(
        summary.cacheable_layers > 0,
        "rich borders benchmark should select the animated-shadow viewport: \
         size={}x{}, scroll_y={}, score={}, summary={summary:?}",
        target.width,
        target.height,
        target.scroll_y,
        target.score
    );

    RichBordersShowcaseBenchmark {
        states,
        width: target.width,
        height: target.height,
        scroll_y: target.scroll_y,
        summary,
    }
}

#[cfg(target_os = "linux")]
fn rich_borders_showcase_states(
    tree: &ElementTree,
    runtime: &AnimationRuntime,
    started_at: Instant,
    target: RichBordersShowcaseTarget,
) -> Vec<RenderState> {
    let mut tree = tree.clone();
    let initial = layout_and_refresh_default_with_animation(
        &mut tree,
        rich_borders_showcase_constraint(target.width, target.height),
        1.0,
        runtime,
        started_at,
    );
    tree.apply_scroll_y(&target.scroll_id, -target.scroll_y);

    RICH_BORDERS_SHOWCASE_FRAME_MS
        .iter()
        .enumerate()
        .map(|(index, frame_ms)| {
            let update =
                layout_or_refresh_default_with_animation_reusing_clean_registry_for_benchmark(
                    &mut tree,
                    rich_borders_showcase_constraint(target.width, target.height),
                    1.0,
                    runtime,
                    started_at + Duration::from_millis(*frame_ms),
                    Some(&initial.event_rebuild),
                );
            RenderState::new(update.output.scene, Color::WHITE, index as u64 + 1, false)
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn rich_borders_showcase_target(
    tree: &ElementTree,
    runtime: &AnimationRuntime,
    started_at: Instant,
) -> RichBordersShowcaseTarget {
    let width = 960;
    let height = 900;
    let mut layout_tree = tree.clone();
    layout_and_refresh_default_with_animation(
        &mut layout_tree,
        rich_borders_showcase_constraint(width, height),
        1.0,
        runtime,
        started_at,
    );
    let scroll_id = largest_vertical_scroll_node(&layout_tree)
        .expect("rich borders showcase should have a vertical scroll container");
    let (scroll_y, summary, score) = rich_borders_showcase_target_scroll_y(
        &layout_tree,
        scroll_id,
        runtime,
        started_at,
        width,
        height,
    );
    let target = RichBordersShowcaseTarget {
        width,
        height,
        scroll_id,
        scroll_y,
        summary,
        score,
    };

    if emerge_bench_diagnostics_enabled() {
        eprintln!(
            "rich borders showcase selected target: size={}x{} scroll_y={} score={} summary={:?}",
            target.width, target.height, target.scroll_y, target.score, target.summary
        );
    }

    target
}

#[cfg(target_os = "linux")]
fn rich_borders_showcase_target_scroll_y(
    tree: &ElementTree,
    scroll_id: NodeId,
    runtime: &AnimationRuntime,
    started_at: Instant,
    width: u32,
    height: u32,
) -> (f32, RenderSceneSummary, usize) {
    let max_y = tree
        .get(&scroll_id)
        .map(|element| element.layout.scroll_y_max.max(0.0))
        .unwrap_or(0.0);
    let sample_count = (max_y / RICH_BORDERS_SHOWCASE_SCROLL_STEP).ceil() as usize;

    (0..=sample_count)
        .map(|sample| {
            let scroll_y = (sample as f32 * RICH_BORDERS_SHOWCASE_SCROLL_STEP).min(max_y);
            let summary = rich_borders_showcase_summary_at_scroll(
                tree, scroll_id, runtime, started_at, scroll_y, width, height,
            );
            (
                rich_borders_showcase_target_score(summary),
                scroll_y,
                summary,
            )
        })
        .min_by_key(|(score, _, _)| *score)
        .map(|(score, scroll_y, summary)| (scroll_y, summary, score))
        .unwrap_or((0.0, RenderSceneSummary::default(), usize::MAX))
}

#[cfg(target_os = "linux")]
fn rich_borders_showcase_summary_at_scroll(
    tree: &ElementTree,
    scroll_id: NodeId,
    runtime: &AnimationRuntime,
    started_at: Instant,
    scroll_y: f32,
    width: u32,
    height: u32,
) -> RenderSceneSummary {
    let mut frame_tree = tree.clone();
    frame_tree.apply_scroll_y(&scroll_id, -scroll_y);
    layout_and_refresh_default_with_animation(
        &mut frame_tree,
        rich_borders_showcase_constraint(width, height),
        1.0,
        runtime,
        started_at,
    )
    .scene
    .summary()
}

#[cfg(target_os = "linux")]
fn rich_borders_showcase_target_score(summary: RenderSceneSummary) -> usize {
    summary.nodes.abs_diff(750)
        + summary.primitives.abs_diff(281) * 8
        + summary.texts.abs_diff(201) * 4
        + summary.shadows.abs_diff(14) * 8
        + summary.borders.abs_diff(18) * 4
        + summary.paint_layers.abs_diff(12) * 12
        + summary.moving_layers.abs_diff(7) * 6
}

#[cfg(target_os = "linux")]
fn rich_borders_showcase_constraint(width: u32, height: u32) -> Constraint {
    Constraint::new(width as f32, height as f32)
}

#[cfg(target_os = "linux")]
fn assert_rich_borders_showcase_cache_hits(
    renderer: &mut SceneRenderer,
    surface: &mut EglBenchSurface,
    page: &RichBordersShowcaseBenchmark,
) {
    let warm_stats = render_paint_layer_cache_stats(renderer, surface, &page.states[0]);
    assert!(
        warm_stats.stores > 0,
        "rich borders showcase did not warm paint-layer payloads: \
         scroll_y={}, summary={:?}, stats={warm_stats:?}",
        page.scroll_y,
        page.summary
    );

    let second_warm_stats = render_paint_layer_cache_stats(renderer, surface, &page.states[1]);
    let steady_stats = render_paint_layer_cache_stats(renderer, surface, &page.states[2]);
    assert!(
        steady_paint_layer_coverage(steady_stats) > 0,
        "rich borders showcase lost warmed cache-hit coverage: \
         scroll_y={}, summary={:?}, stats={steady_stats:?}",
        page.scroll_y,
        page.summary
    );
    assert!(steady_stats.misses <= 2, "{steady_stats:?}");
    assert!(steady_stats.stores <= 2, "{steady_stats:?}");
    assert_eq!(steady_stats.evictions, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.stale_evictions, 0, "{steady_stats:?}");

    let mut total = Duration::ZERO;
    let mut draw = Duration::ZERO;
    let mut flush = Duration::ZERO;
    let mut count = 0u32;
    for state in page.states.iter().skip(3) {
        let timings = {
            let mut frame = surface.frame();
            renderer.render(&mut frame, state)
        };
        let stats = timings
            .renderer_cache
            .as_ref()
            .expect("steady rich borders frame should produce cache stats")
            .paint_layer;
        assert!(steady_paint_layer_coverage(stats) > 0, "{stats:?}");
        assert!(stats.misses <= 2, "{stats:?}");
        assert!(stats.stores <= 2, "{stats:?}");
        assert_eq!(stats.evictions, 0, "{stats:?}");
        assert_eq!(stats.stale_evictions, 0, "{stats:?}");
        total += timings.total;
        draw += timings.draw;
        flush += timings.flush;
        count += 1;
    }
    if emerge_bench_diagnostics_enabled() {
        eprintln!(
            "rich borders showcase: scroll_y={}, summary={:?}, warm={:?}, second_warm={:?}, steady={:?}, steady_total_avg={:?}, steady_draw_avg={:?}, steady_flush_avg={:?}",
            page.scroll_y,
            page.summary,
            warm_stats,
            second_warm_stats,
            steady_stats,
            total / count,
            draw / count,
            flush / count
        );
    }
}

#[cfg(target_os = "linux")]
fn emerge_bench_diagnostics_enabled() -> bool {
    std::env::var_os("EMERGE_BENCH_DIAGNOSTICS").is_some()
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_layout_target(
    tree: &ElementTree,
    runtime: &AnimationRuntime,
    started_at: Instant,
) -> EmergeDemoShowcaseLayoutTarget {
    let target = EMERGE_DEMO_SHOWCASE_LAYOUT_VIEWPORTS
        .iter()
        .filter_map(|(width, height)| {
            let mut layout_tree = tree.clone();
            layout_and_refresh_default_with_animation(
                &mut layout_tree,
                emerge_demo_showcase_layout_constraint(*width, *height),
                1.0,
                runtime,
                started_at,
            );
            let scroll_id = largest_vertical_scroll_node(&layout_tree)?;
            let (scroll_y, summary, score) = emerge_demo_showcase_layout_target_scroll_y(
                &layout_tree,
                scroll_id,
                runtime,
                started_at,
                *width,
                *height,
            );

            Some(EmergeDemoShowcaseLayoutTarget {
                width: *width,
                height: *height,
                scroll_id,
                scroll_y,
                summary,
                score,
            })
        })
        .min_by_key(|target| target.score)
        .expect("emerge_demo showcase layout page should have a vertical scroll container");

    if emerge_bench_diagnostics_enabled() {
        eprintln!(
            "emerge_demo showcase layout selected target: size={}x{} scroll_y={} score={} summary={:?}",
            target.width, target.height, target.scroll_y, target.score, target.summary
        );
    }

    target
}

#[cfg(target_os = "linux")]
fn largest_vertical_scroll_node(tree: &ElementTree) -> Option<NodeId> {
    tree.iter_node_pairs()
        .filter(|(_, element)| element.layout.scroll_y_max > f32::EPSILON)
        .max_by(|(_, left), (_, right)| {
            left.layout
                .scroll_y_max
                .total_cmp(&right.layout.scroll_y_max)
        })
        .map(|(id, _)| id)
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_layout_target_scroll_y(
    tree: &ElementTree,
    scroll_id: NodeId,
    runtime: &AnimationRuntime,
    started_at: Instant,
    width: u32,
    height: u32,
) -> (f32, RenderSceneSummary, usize) {
    let max_y = tree
        .get(&scroll_id)
        .map(|element| element.layout.scroll_y_max.max(0.0))
        .unwrap_or(0.0);
    let sample_count = (max_y / EMERGE_DEMO_SHOWCASE_LAYOUT_SCROLL_STEP).ceil() as usize;

    let samples = (0..=sample_count)
        .map(|sample| {
            let scroll_y = (sample as f32 * EMERGE_DEMO_SHOWCASE_LAYOUT_SCROLL_STEP).min(max_y);
            let summary = emerge_demo_showcase_layout_summary_at_scroll(
                tree, scroll_id, runtime, started_at, scroll_y, width, height,
            );
            (
                emerge_demo_showcase_layout_target_score(summary),
                scroll_y,
                summary,
            )
        })
        .collect::<Vec<_>>();

    samples
        .into_iter()
        .min_by_key(|(score, _, _)| *score)
        .map(|(score, scroll_y, summary)| (scroll_y, summary, score))
        .unwrap_or((0.0, RenderSceneSummary::default(), usize::MAX))
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_layout_summary_at_scroll(
    tree: &ElementTree,
    scroll_id: NodeId,
    runtime: &AnimationRuntime,
    started_at: Instant,
    scroll_y: f32,
    width: u32,
    height: u32,
) -> RenderSceneSummary {
    let mut frame_tree = tree.clone();
    frame_tree.apply_scroll_y(&scroll_id, -scroll_y);
    layout_and_refresh_default_with_animation(
        &mut frame_tree,
        emerge_demo_showcase_layout_constraint(width, height),
        1.0,
        runtime,
        started_at,
    )
    .scene
    .summary()
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_layout_target_score(summary: RenderSceneSummary) -> usize {
    summary.nodes.abs_diff(684)
        + summary.primitives.abs_diff(241) * 8
        + summary.texts.abs_diff(150) * 4
        + summary.transforms.abs_diff(4) * 8
        + summary.clips.abs_diff(420)
        + summary.rects.abs_diff(58) * 2
        + summary.borders.abs_diff(21) * 2
        + summary.shadows.abs_diff(11) * 2
}

#[cfg(target_os = "linux")]
fn emerge_demo_showcase_layout_constraint(width: u32, height: u32) -> Constraint {
    Constraint::new(width as f32, height as f32)
}

#[cfg(target_os = "linux")]
fn assert_offscreen_layout_animation_steady_hits(
    renderer: &mut SceneRenderer,
    surface: &mut EglBenchSurface,
    states: &[RenderState],
) {
    let warm_stats = render_paint_layer_cache_stats(renderer, surface, &states[0]);
    assert!(
        warm_stats.stores > 0,
        "offscreen layout animation did not warm the fixed paint-layer cache: {warm_stats:?}"
    );

    let steady_stats = render_paint_layer_cache_stats(renderer, surface, &states[1]);
    assert!(
        steady_stats.hits > 0,
        "offscreen layout animation steady frame did not hit cache: {steady_stats:?}"
    );
    assert_eq!(steady_stats.misses, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.stores, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.evictions, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.stale_evictions, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.gpu_payload_stores, 0, "{steady_stats:?}");
}

#[cfg(target_os = "linux")]
fn assert_offscreen_layout_animation_visible_frame_skip(
    renderer: &mut SceneRenderer,
    surface: &mut EglBenchSurface,
    states: &[RenderState],
) -> usize {
    let warm_stats = render_paint_layer_cache_stats(renderer, surface, &states[0]);
    assert!(
        warm_stats.stores > 0 || warm_stats.hits > 0,
        "offscreen layout animation did not warm visible payloads: {warm_stats:?}"
    );

    states
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, state)| {
            if renderer.can_skip_unchanged_visible_frame(state, (WIDTH, HEIGHT)) {
                Some(index)
            } else {
                let _ = render_paint_layer_cache_stats(renderer, surface, state);
                None
            }
        })
        .unwrap_or_else(|| {
            panic!(
                "offscreen layout animation did not skip any unchanged visible frame after warmup"
            )
        })
}

#[cfg(target_os = "linux")]
fn assert_scroll_return_cache_reuses_after_clipped_frames(
    renderer: &mut SceneRenderer,
    surface: &mut EglBenchSurface,
    states: &[RenderState],
) {
    let warm_stats = render_paint_layer_cache_stats(renderer, surface, &states[0]);
    assert!(
        warm_stats.stores > 0,
        "scroll return did not warm paint-layer payloads: {warm_stats:?}"
    );

    let _clipped_stats = render_paint_layer_cache_stats(renderer, surface, &states[1]);
    let stale_window_stats = render_paint_layer_cache_stats(renderer, surface, &states[2]);
    assert_eq!(
        stale_window_stats.stale_evictions, 0,
        "{stale_window_stats:?}"
    );
    assert_eq!(
        stale_window_stats.current_entries,
        warm_stats.current_entries
    );

    let return_stats = render_paint_layer_cache_stats(renderer, surface, &states[3]);
    assert!(
        return_stats.hits > 0,
        "scroll return did not hit retained paint-layer payloads: {return_stats:?}"
    );
    assert_eq!(return_stats.misses, 0, "{return_stats:?}");
    assert_eq!(return_stats.stores, 0, "{return_stats:?}");
    assert_eq!(return_stats.evictions, 0, "{return_stats:?}");
    assert_eq!(return_stats.stale_evictions, 0, "{return_stats:?}");
    assert_eq!(return_stats.gpu_payload_stores, 0, "{return_stats:?}");
}

#[cfg(target_os = "linux")]
fn assert_stable_descendant_layout_animation_hits(
    renderer: &mut SceneRenderer,
    surface: &mut EglBenchSurface,
    states: &[RenderState],
) {
    let warm_stats = render_paint_layer_cache_stats(renderer, surface, &states[0]);
    assert!(
        warm_stats.stores > 0,
        "stable descendant layout animation did not warm payloads: {warm_stats:?}"
    );

    let steady_stats = render_paint_layer_cache_stats(renderer, surface, &states[1]);
    assert!(
        steady_stats.hits >= 3,
        "stable descendant layout animation did not hit stable payloads: {steady_stats:?}"
    );
    assert_eq!(steady_stats.misses, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.stores, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.evictions, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.stale_evictions, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.gpu_payload_stores, 0, "{steady_stats:?}");
}

#[cfg(target_os = "linux")]
fn assert_large_simple_layer_bypasses_cache(
    renderer: &mut SceneRenderer,
    surface: &mut EglBenchSurface,
    state: &RenderState,
) {
    let warm_stats = render_paint_layer_cache_stats(renderer, surface, state);
    assert!(
        warm_stats.bypassed_low_value > 0,
        "large/simple paint layer should bypass payload caching: {warm_stats:?}"
    );
    assert_eq!(warm_stats.stores, 0, "{warm_stats:?}");
    assert_eq!(warm_stats.hits, 0, "{warm_stats:?}");

    let steady_stats = render_paint_layer_cache_stats(renderer, surface, state);
    assert!(
        steady_stats.bypassed_low_value > 0,
        "large/simple paint layer should keep using direct fallback: {steady_stats:?}"
    );
    assert_eq!(steady_stats.stores, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.hits, 0, "{steady_stats:?}");
}

#[cfg(target_os = "linux")]
fn assert_text_heavy_layer_cache_hits(
    renderer: &mut SceneRenderer,
    surface: &mut EglBenchSurface,
    state: &RenderState,
) {
    let warm_stats = render_paint_layer_cache_stats(renderer, surface, state);
    assert!(
        warm_stats.stores > 0,
        "text-heavy paint layer should still be admitted into cache: {warm_stats:?}"
    );
    assert_eq!(warm_stats.bypassed_low_value, 0, "{warm_stats:?}");

    let steady_stats = render_paint_layer_cache_stats(renderer, surface, state);
    assert!(
        steady_stats.hits > 0,
        "text-heavy paint layer did not hit after warmup: {steady_stats:?}"
    );
    assert_eq!(steady_stats.bypassed_low_value, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.misses, 0, "{steady_stats:?}");
    assert_eq!(steady_stats.stores, 0, "{steady_stats:?}");
}

#[cfg(target_os = "linux")]
fn large_simple_paint_layer_state() -> RenderState {
    RenderState::new(large_simple_paint_layer_scene(), Color::WHITE, 1, false)
}

#[cfg(target_os = "linux")]
fn large_simple_paint_layer_scene() -> RenderScene {
    RenderScene {
        nodes: vec![RenderNode::PaintLayer(RenderPaintLayer::from_children(
            9_100,
            Rect {
                x: 0.0,
                y: 0.0,
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            PaintLayerPlacement::Fixed,
            PaintLayerPolicy::Cacheable,
            PaintLayerReason::Nearby,
            1,
            vec![
                RenderNode::Primitive(DrawPrimitive::Rect(
                    0.0,
                    0.0,
                    WIDTH as f32,
                    HEIGHT as f32,
                    0xF6F8FBFF,
                )),
                RenderNode::Primitive(DrawPrimitive::RoundedRect(
                    64.0,
                    72.0,
                    WIDTH as f32 - 128.0,
                    HEIGHT as f32 - 144.0,
                    18.0,
                    0xFFFFFFFF,
                )),
                RenderNode::Primitive(DrawPrimitive::Border(
                    64.5,
                    72.5,
                    WIDTH as f32 - 129.0,
                    HEIGHT as f32 - 145.0,
                    18.0,
                    1.0,
                    0xD7DEE8FF,
                    BorderStyle::Solid,
                )),
            ],
        ))],
    }
}

#[cfg(target_os = "linux")]
fn text_heavy_paint_layer_state() -> RenderState {
    RenderState::new(text_heavy_paint_layer_scene(), Color::WHITE, 1, false)
}

#[cfg(target_os = "linux")]
fn text_heavy_paint_layer_scene() -> RenderScene {
    RenderScene {
        nodes: vec![RenderNode::PaintLayer(RenderPaintLayer::from_children(
            9_200,
            Rect {
                x: 58.0,
                y: 46.0,
                width: WIDTH as f32 - 116.0,
                height: HEIGHT as f32 - 92.0,
            },
            PaintLayerPlacement::Fixed,
            PaintLayerPolicy::Cacheable,
            PaintLayerReason::Nearby,
            1,
            std::iter::once(RenderNode::Primitive(DrawPrimitive::RoundedRect(
                58.0,
                46.0,
                WIDTH as f32 - 116.0,
                HEIGHT as f32 - 92.0,
                16.0,
                0xFFFFFFFF,
            )))
            .chain((0..96).map(|index| {
                let col = index % 4;
                let row = index / 4;
                RenderNode::Primitive(DrawPrimitive::TextWithFont(
                    86.0 + col as f32 * 202.0,
                    88.0 + row as f32 * 22.0,
                    format!("Cached text group {index:03}"),
                    14.0,
                    0x172033FF,
                    "default".to_string(),
                    if index % 5 == 0 { 700 } else { 400 },
                    false,
                ))
            }))
            .collect(),
        ))],
    }
}

#[cfg(target_os = "linux")]
fn camera_active_slider_fixture_states(
    encoded_states: &[&[u8]],
    focused_slider_id: NodeId,
) -> Vec<RenderState> {
    encoded_states
        .iter()
        .enumerate()
        .map(|(phase, encoded)| {
            let mut tree = decode_tree(encoded).unwrap_or_else(|error| {
                panic!("Camera phase {phase} fixture should decode: {error}")
            });
            if phase > 0 {
                assert!(
                    !tree.set_focused_active(&focused_slider_id, true).is_none(),
                    "Camera phase {phase} should activate slider {focused_slider_id:?}"
                );
            }
            let output = layout_and_refresh_default(
                &mut tree,
                Constraint::new(
                    CAMERA_ACTIVE_SLIDER_WIDTH as f32,
                    CAMERA_ACTIVE_SLIDER_HEIGHT as f32,
                ),
                1.0,
            );
            RenderState::new(output.scene, Color::BLACK, phase as u64 + 1, false)
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn scrolling_direct_states() -> Vec<RenderState> {
    paint_layer_cache_states(&PAINT_LAYER_SCROLL_OFFSETS, scrolling_direct_scene)
}

#[cfg(target_os = "linux")]
fn scrolling_paint_layer_states() -> Vec<RenderState> {
    paint_layer_cache_states(&PAINT_LAYER_SCROLL_OFFSETS, scrolling_paint_layer_scene)
}

#[cfg(target_os = "linux")]
fn animated_direct_states() -> Vec<RenderState> {
    paint_layer_cache_states(&PAINT_LAYER_ANIMATION_PHASES, animated_direct_scene)
}

#[cfg(target_os = "linux")]
fn animated_paint_layer_states() -> Vec<RenderState> {
    paint_layer_cache_states(&PAINT_LAYER_ANIMATION_PHASES, animated_paint_layer_scene)
}

#[cfg(target_os = "linux")]
fn offscreen_layout_animation_states() -> Vec<RenderState> {
    paint_layer_cache_states(
        &PAINT_LAYER_ANIMATION_PHASES,
        offscreen_layout_animation_scene,
    )
}

#[cfg(target_os = "linux")]
fn stable_descendant_layout_animation_states() -> Vec<RenderState> {
    paint_layer_cache_states(
        &PAINT_LAYER_ANIMATION_PHASES,
        stable_descendant_layout_animation_scene,
    )
}

#[cfg(target_os = "linux")]
fn paint_layer_cache_states<T: Copy>(
    samples: &[T],
    scene: impl Fn(T) -> RenderScene,
) -> Vec<RenderState> {
    samples
        .iter()
        .copied()
        .map(|sample| RenderState::new(scene(sample), Color::WHITE, 1, false))
        .collect()
}

#[cfg(target_os = "linux")]
fn scrolling_direct_scene(offset_y: f32) -> RenderScene {
    RenderScene {
        nodes: vec![
            RenderNode::Primitive(DrawPrimitive::Rect(
                0.0,
                0.0,
                WIDTH as f32,
                HEIGHT as f32,
                0xF3F6FAFF,
            )),
            RenderNode::Transform {
                transform: Affine2::translation(60.0, 54.0 - offset_y),
                children: scrolling_paint_layer_content(),
            },
        ],
    }
}

#[cfg(target_os = "linux")]
fn scrolling_paint_layer_scene(offset_y: f32) -> RenderScene {
    RenderScene {
        nodes: vec![
            RenderNode::Primitive(DrawPrimitive::Rect(
                0.0,
                0.0,
                WIDTH as f32,
                HEIGHT as f32,
                0xF3F6FAFF,
            )),
            RenderNode::Transform {
                transform: Affine2::translation(60.0, 54.0 - offset_y),
                children: vec![RenderNode::PaintLayer(RenderPaintLayer::from_children(
                    4_100,
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 840.0,
                        height: 620.0,
                    },
                    PaintLayerPlacement::ScrollMoving,
                    PaintLayerPolicy::Cacheable,
                    PaintLayerReason::ScrollContent,
                    1,
                    scrolling_paint_layer_content(),
                ))],
            },
        ],
    }
}

#[cfg(target_os = "linux")]
fn scrolling_paint_layer_content() -> Vec<RenderNode> {
    (0..84)
        .flat_map(|index| {
            let col = index % 7;
            let row = index / 7;
            let x = 14.0 + col as f32 * 116.0;
            let y = 16.0 + row as f32 * 48.0;
            let fill = if index % 2 == 0 {
                0xFFFFFFFF
            } else {
                0xEEF4F8FF
            };
            vec![
                RenderNode::ShadowPass {
                    children: vec![RenderNode::Primitive(DrawPrimitive::Shadow(
                        x, y, 94.0, 34.0, 0.0, 4.0, 9.0, 0.0, 8.0, 0x1720331F,
                    ))],
                },
                RenderNode::Primitive(DrawPrimitive::RoundedRect(x, y, 94.0, 34.0, 8.0, fill)),
                RenderNode::Primitive(DrawPrimitive::Gradient(
                    x + 10.0,
                    y + 9.0,
                    52.0,
                    7.0,
                    0x6CA9E6FF,
                    0x3D6F96FF,
                    0.0,
                )),
                RenderNode::Primitive(DrawPrimitive::Border(
                    x + 0.5,
                    y + 0.5,
                    93.0,
                    33.0,
                    8.0,
                    1.0,
                    0xC5CEDAFF,
                    BorderStyle::Solid,
                )),
            ]
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn animated_direct_scene(phase: usize) -> RenderScene {
    RenderScene {
        nodes: animated_static_before()
            .into_iter()
            .chain(animated_dynamic_nodes(phase))
            .chain(animated_static_after())
            .collect(),
    }
}

#[cfg(target_os = "linux")]
fn animated_paint_layer_scene(phase: usize) -> RenderScene {
    RenderScene {
        nodes: vec![
            RenderNode::PaintLayer(RenderPaintLayer::from_children(
                5_200,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: WIDTH as f32,
                    height: HEIGHT as f32,
                },
                PaintLayerPlacement::Fixed,
                PaintLayerPolicy::Cacheable,
                PaintLayerReason::Nearby,
                1,
                animated_static_before(),
            )),
            RenderNode::PaintLayer(RenderPaintLayer::from_children(
                5_201,
                Rect {
                    x: 286.0,
                    y: 246.0,
                    width: 400.0,
                    height: 130.0,
                },
                PaintLayerPlacement::Fixed,
                PaintLayerPolicy::Cacheable,
                PaintLayerReason::Animation,
                phase as u64 + 1,
                animated_dynamic_nodes(phase),
            )),
            RenderNode::PaintLayer(RenderPaintLayer::from_children(
                5_202,
                Rect {
                    x: 52.0,
                    y: 610.0,
                    width: 820.0,
                    height: 90.0,
                },
                PaintLayerPlacement::Fixed,
                PaintLayerPolicy::Cacheable,
                PaintLayerReason::Nearby,
                1,
                animated_static_after(),
            )),
        ],
    }
}

#[cfg(target_os = "linux")]
fn animated_static_before() -> Vec<RenderNode> {
    let background = vec![
        RenderNode::Primitive(DrawPrimitive::Rect(
            0.0,
            0.0,
            WIDTH as f32,
            HEIGHT as f32,
            0xF5F7FAFF,
        )),
        RenderNode::Primitive(DrawPrimitive::Gradient(
            0.0,
            0.0,
            WIDTH as f32,
            132.0,
            0xE6EEF8FF,
            0xF5F7FAFF,
            90.0,
        )),
    ];
    let cards = (0..30).flat_map(|index| {
        let col = index % 5;
        let row = index / 5;
        let x = 36.0 + col as f32 * 178.0;
        let y = 34.0 + row as f32 * 88.0;
        vec![
            RenderNode::ShadowPass {
                children: vec![RenderNode::Primitive(DrawPrimitive::Shadow(
                    x, y, 142.0, 58.0, 0.0, 5.0, 12.0, 0.0, 10.0, 0x17203321,
                ))],
            },
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                x, y, 142.0, 58.0, 10.0, 0xFFFFFFFF,
            )),
            RenderNode::Primitive(DrawPrimitive::Border(
                x + 0.5,
                y + 0.5,
                141.0,
                57.0,
                10.0,
                1.0,
                0xD3DAE5FF,
                BorderStyle::Solid,
            )),
        ]
    });

    background.into_iter().chain(cards).collect()
}

#[cfg(target_os = "linux")]
fn animated_dynamic_nodes(phase: usize) -> Vec<RenderNode> {
    let x = 310.0 + (phase as f32 * 19.0) % 190.0;
    let colors = [0xD94F70FF, 0x2F80EDFF, 0x17A673FF, 0x9B6BFFFF];
    let fill = colors[phase % colors.len()];

    vec![
        RenderNode::ShadowPass {
            children: vec![RenderNode::Primitive(DrawPrimitive::Shadow(
                x, 278.0, 138.0, 52.0, 0.0, 7.0, 16.0, 0.0, 14.0, 0x11182738,
            ))],
        },
        RenderNode::Primitive(DrawPrimitive::RoundedRect(
            x, 278.0, 138.0, 52.0, 14.0, fill,
        )),
        RenderNode::Primitive(DrawPrimitive::RoundedRect(
            x + 18.0,
            294.0,
            78.0,
            10.0,
            5.0,
            0xFFFFFFFF,
        )),
    ]
}

#[cfg(target_os = "linux")]
fn animated_static_after() -> Vec<RenderNode> {
    (0..42)
        .map(|index| {
            let col = index % 7;
            let row = index / 7;
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                52.0 + col as f32 * 124.0,
                610.0 + row as f32 * 14.0,
                76.0,
                5.0,
                2.5,
                if index % 3 == 0 {
                    0x6D7B8DFF
                } else {
                    0xCBD4E0FF
                },
            ))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn offscreen_layout_animation_scene(phase: usize) -> RenderScene {
    RenderScene {
        nodes: vec![
            RenderNode::Primitive(DrawPrimitive::Rect(
                0.0,
                0.0,
                WIDTH as f32,
                HEIGHT as f32,
                0xF5F7FAFF,
            )),
            RenderNode::PaintLayer(RenderPaintLayer::from_children(
                8_200,
                Rect {
                    x: 52.0,
                    y: 44.0,
                    width: 856.0,
                    height: 360.0,
                },
                PaintLayerPlacement::Fixed,
                PaintLayerPolicy::Cacheable,
                PaintLayerReason::ScrollContent,
                1,
                offscreen_layout_animation_children(phase),
            )),
        ],
    }
}

#[cfg(target_os = "linux")]
fn offscreen_layout_animation_children(phase: usize) -> Vec<RenderNode> {
    offscreen_visible_layout_rows()
        .into_iter()
        .chain(std::iter::once(RenderNode::PaintLayer(
            RenderPaintLayer::from_children(
                8_201,
                Rect {
                    x: 78.0,
                    y: 820.0,
                    width: 776.0,
                    height: 88.0,
                },
                PaintLayerPlacement::Fixed,
                PaintLayerPolicy::Cacheable,
                PaintLayerReason::Animation,
                phase as u64 + 1,
                offscreen_animated_layout_row(phase),
            ),
        )))
        .chain(offscreen_static_rows_after_animation(phase))
        .collect()
}

#[cfg(target_os = "linux")]
fn offscreen_visible_layout_rows() -> Vec<RenderNode> {
    (0..5)
        .map(|index| {
            let y = 70.0 + index as f32 * 62.0;
            RenderNode::PaintLayer(RenderPaintLayer::from_children(
                8_210 + index,
                Rect {
                    x: 78.0,
                    y,
                    width: 776.0,
                    height: 42.0,
                },
                PaintLayerPlacement::Fixed,
                PaintLayerPolicy::Cacheable,
                PaintLayerReason::Nearby,
                1,
                offscreen_visible_layout_row_nodes(index, y),
            ))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn offscreen_visible_layout_row_nodes(index: u64, y: f32) -> Vec<RenderNode> {
    let fill = if index.is_multiple_of(2) {
        0xFFFFFFFF
    } else {
        0xEDF4F8FF
    };
    vec![
        RenderNode::ShadowPass {
            children: vec![RenderNode::Primitive(DrawPrimitive::Shadow(
                78.0, y, 776.0, 42.0, 0.0, 4.0, 9.0, 0.0, 8.0, 0x1720331F,
            ))],
        },
        RenderNode::Primitive(DrawPrimitive::RoundedRect(78.0, y, 776.0, 42.0, 8.0, fill)),
        RenderNode::Primitive(DrawPrimitive::Gradient(
            102.0,
            y + 14.0,
            180.0,
            9.0,
            0x7CB7E6FF,
            0x3D6F96FF,
            0.0,
        )),
        RenderNode::Primitive(DrawPrimitive::Border(
            78.5,
            y + 0.5,
            775.0,
            41.0,
            8.0,
            1.0,
            0xC5CEDAFF,
            BorderStyle::Solid,
        )),
    ]
}

#[cfg(target_os = "linux")]
fn offscreen_animated_layout_row(phase: usize) -> Vec<RenderNode> {
    let scale = [0.92, 1.04, 1.18, 1.04, 0.92, 1.04, 1.18, 1.04][phase % 8];
    let width = 580.0 * scale;
    let x = 176.0 + (580.0 - width) * 0.5;
    vec![
        RenderNode::ShadowPass {
            children: vec![RenderNode::Primitive(DrawPrimitive::Shadow(
                x, 828.0, width, 64.0, 0.0, 7.0, 16.0, 0.0, 14.0, 0x11182738,
            ))],
        },
        RenderNode::Primitive(DrawPrimitive::RoundedRect(
            x, 828.0, width, 64.0, 14.0, 0xD94F70FF,
        )),
        RenderNode::Primitive(DrawPrimitive::RoundedRect(
            x + 28.0,
            854.0,
            190.0,
            10.0,
            5.0,
            0xFFFFFFFF,
        )),
    ]
}

#[cfg(target_os = "linux")]
fn offscreen_static_rows_after_animation(phase: usize) -> Vec<RenderNode> {
    let animated_width = 580.0 * [0.92, 1.04, 1.18, 1.04, 0.92, 1.04, 1.18, 1.04][phase % 8];
    (0..4)
        .map(|index| {
            let row_width = if index == 0 {
                120.0 + animated_width * 0.25
            } else {
                320.0
            };
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                90.0 + (animated_width - 580.0) * 0.1,
                938.0 + index as f32 * 20.0,
                row_width,
                8.0,
                4.0,
                0xCBD4E0FF,
            ))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn stable_descendant_layout_animation_scene(phase: usize) -> RenderScene {
    RenderScene {
        nodes: vec![
            RenderNode::Primitive(DrawPrimitive::Rect(
                0.0,
                0.0,
                WIDTH as f32,
                HEIGHT as f32,
                0xF5F7FAFF,
            )),
            RenderNode::PaintLayer(RenderPaintLayer::from_children(
                8_400,
                Rect {
                    x: 52.0,
                    y: 44.0,
                    width: 856.0,
                    height: 360.0,
                },
                PaintLayerPlacement::Fixed,
                PaintLayerPolicy::Cacheable,
                PaintLayerReason::ScrollContent,
                1,
                stable_descendant_layout_animation_children(phase),
            )),
        ],
    }
}

#[cfg(target_os = "linux")]
fn stable_descendant_layout_animation_children(phase: usize) -> Vec<RenderNode> {
    let offscreen_shift = [0.0, 16.0, 34.0, 18.0, 0.0, 16.0, 34.0, 18.0][phase % 8];
    vec![RenderNode::Clip {
        clips: vec![ClipShape {
            rect: Rect {
                x: 52.0,
                y: 44.0,
                width: 856.0,
                height: 360.0,
            },
            radii: None,
        }],
        children: vec![
            RenderNode::Primitive(DrawPrimitive::RoundedRect(
                52.0, 44.0, 856.0, 360.0, 12.0, 0xFFFFFFFF,
            )),
            stable_descendant_layer(8_401, 78.0, 70.0, 776.0, 58.0, 0xEEF7F5FF),
            stable_descendant_layer(8_402, 78.0, 146.0, 776.0, 58.0, 0xF7F3FFFF),
            stable_descendant_layer(8_403, 78.0, 222.0, 776.0, 58.0, 0xF2F6FFFF),
            stable_descendant_layer(8_404, 108.0, 300.0, 716.0, 38.0, 0xEDF8FFFF),
            stable_descendant_layer(8_405, 108.0, 348.0, 716.0, 38.0, 0xF7FAFCFF),
            RenderNode::PaintLayer(RenderPaintLayer::from_children(
                8_406,
                Rect {
                    x: 108.0,
                    y: 430.0,
                    width: 716.0,
                    height: 86.0 + offscreen_shift,
                },
                PaintLayerPlacement::Fixed,
                PaintLayerPolicy::Cacheable,
                PaintLayerReason::Animation,
                phase as u64 + 1,
                stable_descendant_animated_row(phase),
            )),
            stable_descendant_layer(
                8_407,
                78.0,
                548.0 + offscreen_shift,
                776.0,
                58.0,
                0xFFFBEBFF,
            ),
        ],
    }]
}

#[cfg(target_os = "linux")]
fn stable_descendant_layer(
    stable_id: u64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fill: u32,
) -> RenderNode {
    RenderNode::PaintLayer(RenderPaintLayer::from_children(
        stable_id,
        Rect {
            x,
            y,
            width,
            height,
        },
        PaintLayerPlacement::ScrollMoving,
        PaintLayerPolicy::Cacheable,
        PaintLayerReason::Nearby,
        1,
        vec![
            RenderNode::ShadowPass {
                children: vec![RenderNode::Primitive(DrawPrimitive::Shadow(
                    x, y, width, height, 0.0, 4.0, 9.0, 0.0, 8.0, 0x1720331F,
                ))],
            },
            RenderNode::Primitive(DrawPrimitive::RoundedRect(x, y, width, height, 8.0, fill)),
            RenderNode::Primitive(DrawPrimitive::Gradient(
                x + 24.0,
                y + height * 0.5 - 5.0,
                180.0,
                10.0,
                0x7CB7E6FF,
                0x3D6F96FF,
                0.0,
            )),
            RenderNode::Primitive(DrawPrimitive::Border(
                x + 0.5,
                y + 0.5,
                width - 1.0,
                height - 1.0,
                8.0,
                1.0,
                0xC5CEDAFF,
                BorderStyle::Solid,
            )),
        ],
    ))
}

#[cfg(target_os = "linux")]
fn stable_descendant_animated_row(phase: usize) -> Vec<RenderNode> {
    let width = 520.0 + [0.0, 24.0, 58.0, 28.0, 0.0, 24.0, 58.0, 28.0][phase % 8];
    let x = 108.0 + (716.0 - width) * 0.5;
    vec![
        RenderNode::ShadowPass {
            children: vec![RenderNode::Primitive(DrawPrimitive::Shadow(
                x, 430.0, width, 70.0, 0.0, 7.0, 16.0, 0.0, 14.0, 0x11182738,
            ))],
        },
        RenderNode::Primitive(DrawPrimitive::RoundedRect(
            x, 430.0, width, 70.0, 14.0, 0xD94F70FF,
        )),
        RenderNode::Primitive(DrawPrimitive::RoundedRect(
            x + 28.0,
            460.0,
            190.0,
            10.0,
            5.0,
            0xFFFFFFFF,
        )),
    ]
}

#[cfg(target_os = "linux")]
fn scroll_return_cache_config() -> RendererCacheConfig {
    RendererCacheConfig {
        enabled: true,
        paint_layer: RendererPaintLayerCacheConfig {
            max_stale_frames: 1,
            ..RendererPaintLayerCacheConfig::default()
        },
        ..RendererCacheConfig::default()
    }
}

#[cfg(target_os = "linux")]
fn small_payload_cache_config() -> RendererCacheConfig {
    RendererCacheConfig {
        enabled: true,
        paint_layer: RendererPaintLayerCacheConfig {
            max_entry_bytes: 8 * 1024 * 1024,
            max_bytes: 128 * 1024 * 1024,
            ..RendererPaintLayerCacheConfig::default()
        },
        ..RendererCacheConfig::default()
    }
}

#[cfg(target_os = "linux")]
fn scroll_return_state(scroll_y: f32) -> RenderState {
    RenderState::new(scroll_return_scene(scroll_y), Color::WHITE, 1, false)
}

#[cfg(target_os = "linux")]
fn scroll_return_scene(scroll_y: f32) -> RenderScene {
    RenderScene {
        nodes: vec![
            RenderNode::Primitive(DrawPrimitive::Rect(
                0.0,
                0.0,
                WIDTH as f32,
                HEIGHT as f32,
                0xF3F6FAFF,
            )),
            RenderNode::Clip {
                clips: vec![ClipShape {
                    rect: Rect {
                        x: 42.0,
                        y: 40.0,
                        width: 340.0,
                        height: 96.0,
                    },
                    radii: None,
                }],
                children: vec![RenderNode::Transform {
                    transform: Affine2::translation(58.0, 58.0 - scroll_y),
                    children: vec![RenderNode::PaintLayer(RenderPaintLayer::from_children(
                        8_300,
                        Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 260.0,
                            height: 54.0,
                        },
                        PaintLayerPlacement::ScrollMoving,
                        PaintLayerPolicy::Cacheable,
                        PaintLayerReason::Nearby,
                        1,
                        scroll_return_layer_content(),
                    ))],
                }],
            },
        ],
    }
}

#[cfg(target_os = "linux")]
fn scroll_return_layer_content() -> Vec<RenderNode> {
    vec![
        RenderNode::ShadowPass {
            children: vec![RenderNode::Primitive(DrawPrimitive::Shadow(
                0.0, 0.0, 260.0, 54.0, 0.0, 5.0, 12.0, 0.0, 10.0, 0x17203321,
            ))],
        },
        RenderNode::Primitive(DrawPrimitive::RoundedRect(
            0.0, 0.0, 260.0, 54.0, 10.0, 0xFFFFFFFF,
        )),
        RenderNode::Primitive(DrawPrimitive::Gradient(
            22.0, 21.0, 132.0, 9.0, 0x6CA9E6FF, 0x3D6F96FF, 0.0,
        )),
        RenderNode::Primitive(DrawPrimitive::Border(
            0.5,
            0.5,
            259.0,
            53.0,
            10.0,
            1.0,
            0xC5CEDAFF,
            BorderStyle::Solid,
        )),
    ]
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
fn current_gl_string(name: u32) -> String {
    let value = unsafe { gl::GetString(name) };
    if value.is_null() {
        return "unavailable".to_string();
    }

    unsafe { CStr::from_ptr(value.cast()) }
        .to_string_lossy()
        .into_owned()
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

fn bench_renderer(c: &mut Criterion) {
    let asset_runtime = AssetRuntime::new();
    let _asset_context_guard = asset_runtime.enter();

    bench_renderer_raster_direct(c);
    bench_renderer_direct_candidates(c);
    bench_renderer_cold_frames(c);
    bench_renderer_gpu_surfaceless(c);
    bench_renderer_gpu_cold_frames(c);
    bench_renderer_paint_layer_cache(c);
}

criterion_group!(benches, bench_renderer);
criterion_main!(benches);
