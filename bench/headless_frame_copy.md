# Headless frame-copy benchmark

Run from the repository root:

```sh
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench headless_frame_copy
```

This benchmarks 800×1280 four-byte frames (4,096,000 bytes each):

- `legacy_rgba`: the pre-`c9cc87c` zero-filled readback buffer, conversion clone,
  and final output copy.
- `current_rgba` / `current_bgra`: the production `RasterBackend` capture followed
  by a matching-byte-order output copy.

Criterion reports time/frame and frames/second. A separate warmed iteration
counts Rust frame-sized allocations and requested bytes, checking that the old
pipeline allocates three buffers (12,288,000 bytes) and the new one two
(8,192,000 bytes). It also reports zero-initialized allocations. Both pipelines
retain the snapshot and output buffers until the iteration ends.

## Scope and limitations

This isolates capture and copying with a warmed, unchanged scene and disabled
renderer caches; it is **not** an end-to-end rendering or display FPS benchmark.
The output allocation uses `Vec::to_vec` in place of `OwnedBinary` because Cargo
benchmarks do not run inside the BEAM. Both allocate and copy without explicitly
zero-filling the output. The current capture itself runs production code; the
legacy path reproduces the old code for comparison.

Allocation counts exclude Skia/C++ allocations, the persistent raster surface,
small Rust allocations, and BEAM allocator/message overhead. Allocated bytes are
cumulative requested bytes per frame, not peak RSS. Zeroed allocation counts do
not prove a physical memory pass: the allocator/OS can provide lazy zero pages.
Timings include buffer destruction, but not initial surface creation. Run on the
target device to assess its actual memory-bandwidth and allocator behavior.

## Local sample

AMD Ryzen 9 7950X, Linux x86_64, Rust 1.93.1, optimized Cargo bench profile:

```sh
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench headless_frame_copy -- \
  --warm-up-time 1 --measurement-time 3 --sample-size 30
```

Criterion point estimates (not a cross-machine performance guarantee):

| Pipeline | Time/frame | Frames/second | Frame allocations | Allocated bytes/frame |
|---|---:|---:|---:|---:|
| Legacy RGBA | 2.813 ms | 355 | 3 | 12,288,000 |
| Current RGBA | 1.834 ms | 545 | 2 | 8,192,000 |
| Current BGRA | 1.893 ms | 528 | 2 | 8,192,000 |

The matching RGBA copy pipeline took about 35% less time and requested 33% fewer
frame-buffer bytes. The legacy pipeline requested one zeroed frame allocation;
the current paths requested none. These results cover the modeled pipeline
above, not the BEAM allocator or rendering a changing scene.
