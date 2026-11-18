# Single-line Row and element shapes

**Delivered on the ordinary binary CPU headless renderer.**

1. Freeze pre-cut geometry, delivered pixels and raw input separately. Keep the existing RGBA/BGRA pixel-order tests and two-child spacing test unchanged.
2. Use the sibling `emerge_clean` design: implement `ShapeMeta` (wire tag and typed payload) and `LayoutBehaviour` in one module under [`tree/shapes/`](../native/emerge_skia/src/tree/shapes/). Select `BoxPaint` or implement `PaintBehaviour` for custom output. Register the module once in [`define_shapes!`](../native/emerge_skia/src/tree/shapes.rs); no element switch in the codec, scheduler or rasterizer. El, Empty, Text, Column, WrappedRow and Row all use this interface. Shapes read borrowed data and settled child results; the engine owns the keyed caches, queues, pools and single child order.
3. Admit EMRG v9 Row (tag 1). Measure summed child widths plus gaps and insets, and the tallest child plus vertical insets. Split spare **definite** width equally among fills. A content-sized row offers each child its intrinsic width instead. Compare offers and offsets even when the row’s frame stays equal. Overflow does not wrap: preserve layout extents and clip descendant paint and hits at the row edge.
4. On one viewport, patch fixed A `20 → 60 → 20` alongside fill B/C, then swap B/C. Consume each real frame before sending input. At `(80,10)` the click goes to C `(5,5)` → B `(10,5)` → C `(5,5)` → B `(5,5)`; `(50,10)` misses in the middle state. Check empty/single rows, nested columns, intrinsic fills and fractional clips separately.
5. Verify incremental vs cold output, sparse recomputation, atomic rejection and allocation before qualifying the suite. No weighted/height fill, other alignment, animation, scrolling, topology edits or NIF changes.

## Results

- The three unchanged regressions and two new viewport tests pass on both candidate and pre-cut. Reference comparison: **20 geometry dumps, 14 delivered RGBA frames and 28 raw input probes matched**. The overflow control caught a child painting beyond its 20px row despite correct geometry. At a fractional edge paint uses device pixels; hits retain the unrounded edge and original local coordinates.
- **27 release Rust tests**, release build and strict Clippy passed. The fixed-width edit did not remeasure peers; the equal-size swap ran no measure/flow passes; a paint-only edit ran neither layout nor hit composition. First Row compute after reservation and fitting wire edits through paint/hit inputs measured **[0,0,0]** alloc/realloc/free. This excludes rasterization and BEAM delivery.
- First-slice parity (7 frames/15 input probes and detached controls) still matches. Full private Mix: **625/729 passed, 104 failed, 21 excluded**. Five of the previous 109 failures now pass: the three targets plus keyed and unkeyed reorder patch tests. No new failing names or exclusions. Unsupported features, including height fill and topology edits, remain visible; no hardware or speed claim.

Evidence: `/workspace/emerge-investigation-artifacts/row-slice-VaiO8l/` (`row-ref.sha256`, `clip-ref.sha256`, `logs/{identity-final,row-match,clip-match,final-heap,mix-final}`, `no-longer-failing`, `new-failures`).

### Scheduler follow-up

[`tree/queue.rs`](../native/emerge_skia/src/tree/queue.rs) now uses depth buckets instead of a binary heap for all seven passes. Each queued mount holds a next-link and merged output mask. A pass pops at its shallowest or deepest active depth; only emptying that bucket with work remaining scans the bitset for the next depth. Sibling execution order does not determine paint or hit precedence.

30 release Rust tests and strict Clippy passed. Fitting wide → deep → wide replacements through depth 512, including discarding undrawn work, measured `[0,0,0]` alloc/realloc/free. The frozen renderer controls matched, and full Mix had the same 104 failing names. No wall-time comparison was made. Evidence: `/workspace/emerge-investigation-artifacts/depth-workset-araqLb/`.
