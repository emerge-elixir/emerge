//! Copy-pipeline microbenchmark; see bench/headless_frame_copy.md for scope.
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use emerge_skia::backend::raster::{RasterBackend, RasterConfig, RasterPixelFormat};
use emerge_skia::render_scene::RenderScene;
use emerge_skia::renderer::{
    RenderFrame, RenderState, RendererCacheConfig, SceneRenderer, text_surface_props,
};
use skia_safe::{AlphaType, Color, ColorType, ImageInfo, surfaces};
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const WIDTH: u32 = 800;
const HEIGHT: u32 = 1280;
const ROW_BYTES: usize = WIDTH as usize * 4;
const FRAME_BYTES: usize = ROW_BYTES * HEIGHT as usize;

// Count only Rust frame-sized buffers during a single, warmed-up iteration.
// Skia's C++ allocations and the BEAM allocator are intentionally not included.
struct FrameAllocator;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ZEROED: AtomicUsize = AtomicUsize::new(0);

fn record_allocation(ptr: *mut u8, size: usize, zeroed: bool) {
    if !ptr.is_null() && size == FRAME_BYTES && COUNTING.load(Ordering::Relaxed) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        if zeroed {
            ZEROED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// SAFETY: Every allocation/deallocation is forwarded unchanged to System.
// Recording counters neither accesses nor changes the allocated storage.
unsafe impl GlobalAlloc for FrameAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        record_allocation(ptr, layout.size(), false);
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        record_allocation(ptr, layout.size(), true);
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let ptr = unsafe { System.realloc(ptr, layout, size) };
        record_allocation(ptr, size, false);
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: FrameAllocator = FrameAllocator;

fn report_allocations(
    name: &str,
    capture: &mut impl FnMut() -> (Vec<u8>, Vec<u8>),
    expected: usize,
) {
    drop(capture()); // Warm the renderer before counting.
    ALLOCATIONS.store(0, Ordering::Relaxed);
    ZEROED.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let (snapshot, output) = capture();
    COUNTING.store(false, Ordering::Relaxed);
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let zeroed = ZEROED.load(Ordering::Relaxed);
    assert_eq!(snapshot.len(), FRAME_BYTES);
    assert_eq!(snapshot, output);
    assert_eq!(
        allocations, expected,
        "unexpected frame allocations for {name}"
    );
    eprintln!(
        "{name}: {allocations} frame allocations, {} allocated bytes/frame, {zeroed} zeroed allocations",
        allocations * FRAME_BYTES
    );
}

fn bench_headless_frame_copy(c: &mut Criterion) {
    let mut group = c.benchmark_group("headless_frame_copy/800x1280");
    group.throughput(Throughput::Elements(1));
    let cache_config = RendererCacheConfig {
        enabled: false,
        ..RendererCacheConfig::default()
    };
    let state = RenderState::new(
        RenderScene::default(),
        Color::from_argb(128, 200, 100, 50),
        1,
        false,
    );

    // Reproduce the pre-c9cc87c RGBA path: zero + readback, conversion clone,
    // final output copy. Both old and new paths retain the snapshot buffer.
    let info = ImageInfo::new(
        (WIDTH as i32, HEIGHT as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let mut surface = surfaces::raster(&info, None, Some(&text_surface_props())).unwrap();
    let mut renderer = SceneRenderer::with_cache_config(cache_config);
    let mut legacy = || {
        renderer.render(&mut RenderFrame::new(&mut surface, None), &state);
        let mut snapshot = vec![0; FRAME_BYTES];
        assert!(surface.read_pixels(&info, &mut snapshot, ROW_BYTES, (0, 0)));
        let converted = black_box(snapshot.as_slice()).to_vec();
        let output = black_box(converted.as_slice()).to_vec();
        (snapshot, output)
    };
    report_allocations("legacy_rgba", &mut legacy, 3);
    group.bench_function("legacy_rgba", |b| b.iter(&mut legacy));

    for (name, format) in [
        ("current_rgba", RasterPixelFormat::Rgba8888Premul),
        ("current_bgra", RasterPixelFormat::Bgra8888Premul),
    ] {
        let mut backend = RasterBackend::with_cache_config_and_format(
            &RasterConfig {
                width: WIDTH,
                height: HEIGHT,
            },
            cache_config,
            format,
        )
        .unwrap();
        let mut current = || {
            let frame = backend.render(&state);
            // Vec stands in for OwnedBinary: allocate and copy without an
            // extra zero-fill. No live BEAM is available in cargo bench.
            let output = black_box(frame.data.as_slice()).to_vec();
            (frame.data, output)
        };
        report_allocations(name, &mut current, 2);
        group.bench_function(name, |b| b.iter(&mut current));
    }
    group.finish();
}

criterion_group!(benches, bench_headless_frame_copy);
criterion_main!(benches);
