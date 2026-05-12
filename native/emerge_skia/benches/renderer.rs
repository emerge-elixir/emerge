use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
#[cfg(target_os = "linux")]
use emerge_skia::backend::skia_gpu::GlFrameSurface;
use emerge_skia::render_scene::{
    DrawPrimitive, PaintLayerPlacement, PaintLayerPolicy, PaintLayerReason, RenderNode,
    RenderPaintLayer, RenderScene,
};
use emerge_skia::renderer::{
    RenderFrame, RenderState, RendererCacheConfig, SceneRenderer, insert_raster_asset,
};
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
    AlphaType, Color, ColorType, ImageInfo, Path, PathBuilder, PathDirection, Point3, RRect,
    Rect as SkRect, surfaces, utils::shadow_utils::ShadowFlags,
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

    group.bench_function("scrolling/cache_moved_hits", |b| {
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

    group.finish();
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
    expect_moved_hit: bool,
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
    assert!(
        !expect_moved_hit || second_stats.moved_hits > 0,
        "{label} paint-layer cache did not record a moved hit: {second_stats:?}"
    );
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
                children: vec![RenderNode::PaintLayer(RenderPaintLayer {
                    stable_id: 4_100,
                    bounds: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 840.0,
                        height: 620.0,
                    },
                    placement: PaintLayerPlacement::ScrollMoving,
                    policy: PaintLayerPolicy::Cacheable,
                    reason: PaintLayerReason::ScrollContainer,
                    content_generation: 1,
                    children: scrolling_paint_layer_content(),
                })],
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
        nodes: vec![RenderNode::PaintLayer(RenderPaintLayer {
            stable_id: 5_200,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            placement: PaintLayerPlacement::Fixed,
            policy: PaintLayerPolicy::Cacheable,
            reason: PaintLayerReason::StableSubtree,
            content_generation: 1,
            children: animated_paint_layer_children(phase),
        })],
    }
}

#[cfg(target_os = "linux")]
fn animated_paint_layer_children(phase: usize) -> Vec<RenderNode> {
    animated_static_before()
        .into_iter()
        .chain(std::iter::once(RenderNode::PaintLayer(RenderPaintLayer {
            stable_id: 5_201,
            bounds: Rect {
                x: 286.0,
                y: 246.0,
                width: 400.0,
                height: 130.0,
            },
            placement: PaintLayerPlacement::Fixed,
            policy: PaintLayerPolicy::DynamicRedraw,
            reason: PaintLayerReason::Animation,
            content_generation: 0,
            children: animated_dynamic_nodes(phase),
        })))
        .chain(animated_static_after())
        .collect()
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
    bench_renderer_cold_frames,
    bench_renderer_gpu_surfaceless,
    bench_renderer_gpu_cold_frames,
    bench_renderer_paint_layer_cache
);
criterion_main!(benches);
