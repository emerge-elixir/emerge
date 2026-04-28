# Drawing Optimization Investigation

Last updated: 2026-04-28.

Status: investigation/reference; no active direct-drawing implementation plan is
open.

## Purpose

This document covers direct rendering optimizations only. It intentionally avoids
renderer caches, subtree rasterization, retained pictures, retained text blobs,
shader-object reuse, tile reuse, and dirty-region/back-buffer reuse. Those topics
belong in `rendering-cache-engine-investigation.md` and
`render-cache-flutter-comparison.md`.

The focus here is narrower: make the direct Skia draw path cheaper and make cold
GPU behavior less visible without retaining Emerge-rendered output.

## Boundary

Included:

- better Skia primitive selection
- path and clip reduction
- avoiding unnecessary `saveLayer`
- paint/color/filter formulation for one-pass drawing
- shader/pipeline warmup by drawing representative primitives, without storing
  Emerge-managed output
- backend/context options that affect direct drawing
- draw diagnostics and benchmarks

Excluded:

- caching rendered subtrees or layers
- retaining Skia pictures or display lists
- retaining text blobs or glyph output
- retaining shader objects in Emerge
- image variant reuse beyond the current asset behavior
- tile caches and dirty-region redraw
- public Elixir cache or shader hints

## Relationship to Renderer Caches

Use this document to establish the direct-rendering baseline. Work here may
change how the current frame is drawn, but it must not keep Emerge-rendered
output for later frames.

`rendering-cache-engine-investigation.md` starts after this baseline for any
cost center that still shows repeated work, or when a direct optimization is
explicitly deferred with a reason. The two investigations share the same
benchmark-first rule:

- drawing work saves a direct-rendering baseline such as `drawing_opt_before`
  and must improve the uncached path
- cache work saves a renderer-cache baseline such as `render_cache_before` and
  must beat the current direct path in steady state without hiding miss or
  warm-up regressions

The direct-drawing implementation slice for this investigation has completed
and was folded back into this document. Future direct drawing work should start
here and create a new plan only when it has a fresh benchmark target.

## Completed Direct Drawing Pass

The 2026-04-27 direct drawing pass established `drawing_opt_before`, added the
renderer benchmark suite, and landed only optimizations that beat that baseline.

Landed:

- solid border fast paths:
  - unclipped solid rounded/uniform borders use `draw_drrect`
  - zero-radius single-edge solid borders draw one rect directly
  - clipped solid-border fast paths remain disabled
- template image tint uses a direct color-filter draw with the previous
  `saveLayer` tint path as fallback
- alpha scopes avoid `save_layer_alpha` only for one direct rect, rounded rect,
  or text primitive where multiplying paint alpha is equivalent

Measured results against `drawing_opt_before`:

- `solid_uniform_borders`: improved by about 6.65%
- `solid_edge_borders`: improved by about 62.38%
- `template_tinted_images`: improved by about 16.27%
- `alpha_single_primitive`: improved by about 99.31%
- `mixed_ui_scene`: improved by about 84.48% after the alpha fast path

Rejected or deferred:

- clipped solid-border fast paths did not prove a `border_clip_heavy` win
- adjacent clip combining was not implemented because `clip_rect_vs_rrect` stayed
  within noise and shadow/scroll escape semantics are fragile
- Skia `Canvas::draw_shadow` stayed benchmark-only because it was slower than the
  current mask-filter path and does not match Emerge's CSS-like shadow semantics
- renderer warmup behavior was not implemented; only cold-frame benchmark gates
  were added
- standalone paint-helper refactors stayed out because abstraction without a
  measured renderer win is not enough

Decision rule going forward:

- if a direct drawing candidate is neutral, noisy, or regresses, keep the simpler
  renderer path and document the rejection near the candidate benchmark or code
  path
- cache work may start only after the matching direct draw path has landed, been
  benchmarked and rejected, or been explicitly deferred

## Current Emerge Renderer Shape

Emerge renders a `RenderScene` by walking `RenderNode`s and issuing Skia canvas
commands. The normal Wayland/DRM path is GPU-backed through Skia GL, so many
commands already become Skia-managed GPU programs internally.

Relevant current code:

- `SceneRenderer::render_with_draw_profile` clears, walks nodes, records optional
  category timings, then flushes.
  Source: `native/emerge_skia/src/renderer.rs:831`.
- GPU-backed frames carry an optional `gpu::DirectContext`.
  Source: `native/emerge_skia/src/renderer.rs:741`.
- `RenderFrame::flush` splits GPU flush from submit.
  Source: `native/emerge_skia/src/renderer.rs:761`.
- Wayland/DRM use `GlFrameSurface`, wrapping a backend GL render target as a
  Skia `Surface`.
  Source: `native/emerge_skia/src/backend/skia_gpu.rs:8`.
- Explicit hand-written OpenGL shaders currently exist only for external video
  blitting.
  Source: `native/emerge_skia/src/video.rs:1079`.

Current direct primitive behavior:

- rects and rounded rects create a paint and call `draw_rect` or `draw_rrect`
- borders go through `draw_border`, which builds paths for many cases
- shadows use `MaskFilter::blur` on rounded rect geometry
- text calls `draw_str`
- gradients create a linear-gradient shader for the draw
- tiled images use an image shader
- template tint uses `saveLayer` plus `BlendMode::SrcIn`
- alpha groups below `1.0` use `save_layer_alpha`

Sources:

- `native/emerge_skia/src/renderer.rs:1403`
- `native/emerge_skia/src/renderer.rs:1581`
- `native/emerge_skia/src/renderer.rs:1589`
- `native/emerge_skia/src/renderer.rs:2203`
- `native/emerge_skia/src/renderer.rs:2426`
- `native/emerge_skia/src/renderer.rs:2793`
- `native/emerge_skia/src/renderer.rs:2875`

## Measured Signals

Recent demo traces showed separate bottleneck shapes:

```text
render=8.384 ms draw=2.445 ms flush=5.939 ms gpu_flush=5.935 ms
draw detail: shadows=2.012 ms inset_shadows=0.046 ms texts=0.289 ms images=0.000 ms
```

```text
render=4.482 ms draw=2.155 ms flush=2.327 ms gpu_flush=2.310 ms
draw detail: clips=0.174 ms borders=0.939 ms texts=0.973 ms
```

```text
render=5.566 ms draw=4.073 ms flush=1.493 ms gpu_flush=1.474 ms
draw detail: texts=0.528 ms images=3.043 ms
image[1] kind=Raster source=640x420 draw=432x240 draw=3.032 ms
```

Interpretation:

- `draw` spikes can come from CPU path construction, clip state, text calls,
  layer setup, and issuing expensive Skia commands.
- `gpu_flush` spikes can come from GPU program compilation, backend synchronization,
  deferred GPU work, or first-use resource work.
- Image first-use cost can appear in draw or flush depending on where Skia does
  the work.

The rule for this document: an optimization is only useful if it improves the
complete frame shape, not just one timing bucket.

## Engine Findings

### Flutter and Impeller

Relevant non-retention mechanisms:

- Flutter can trace Skia calls and dump the SKP that triggers new shader
  compilation. Its switch text calls this useful for custom shader warmup.
  Source: `/workspace/tmp-layout-engines/flutter_engine/shell/common/switches.h:179`.
- Flutter creates the Skia GL context with explicit context options. On OpenGL it
  avoids stencil buffers because they caused memory and performance regressions.
  Source: `/workspace/tmp-layout-engines/flutter_engine/shell/common/context_options.cc:23`.
- Flutter exposes warm-up frames that run before normal vsync-driven frames, so
  expensive first-frame work can happen earlier.
  Sources:
  `/workspace/tmp-layout-engines/flutter_engine/lib/ui/platform_dispatcher.dart:819`,
  `/workspace/tmp-layout-engines/flutter_engine/shell/common/animator.h:69`,
  `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/binding.dart:570`.
- Impeller's display-list dispatcher tracks whether opacity can be distributed
  instead of forcing an offscreen layer.
  Source:
  `/workspace/tmp-layout-engines/flutter_engine/impeller/display_list/dl_dispatcher.cc:300`.
- Display-list playback can cull operations with safe transform constraints.
  Source:
  `/workspace/tmp-layout-engines/flutter_engine/impeller/display_list/dl_dispatcher.cc:822`.

Fit for Emerge:

- Add an optional warmup draw pass after GL context creation.
- Add diagnostics that identify first frame after context creation and first draw
  after asset insertion.
- Reduce `saveLayer` usage where paint alpha or color filters can express the
  same result.
- Treat backend options as measured changes. Do not enable stencil/MSAA-like
  features just because another engine has them.

### WebRender

Relevant non-retention mechanisms:

- Draws are grouped by batch keys: blend mode, primitive kind, textures, and clip
  mask compatibility.
  Source: `/workspace/tmp-layout-engines/webrender/webrender/src/batch.rs:54`.
- Text, brush, quad, image, and gradient primitives are distinct batch kinds.
  Source: `/workspace/tmp-layout-engines/webrender/webrender/src/batch.rs:73`.
- Brush primitives can be segmented when clipping or edge antialiasing requires
  per-segment draw data.
  Source: `/workspace/tmp-layout-engines/webrender/webrender/src/batch.rs:3327`.

Fit for Emerge:

- Full WebRender-style batching does not map directly to Skia canvas playback.
- The practical lesson is to feed Skia better primitive forms: rect/rrect/DRRect
  over generic paths, simple clips over path clips, and fewer layer boundaries.
- Preserve z-order. Only adjacent compatible run shaping is plausible.

### Iced WGPU

Relevant non-retention mechanisms:

- Engine startup creates explicit quad, text, triangle, and image pipelines.
  Source: `/workspace/tmp-layout-engines/iced/wgpu/src/engine.rs:24`.
- A quad instance carries position, size, border color, radii, border width,
  shadow color, shadow offset, blur, and snapping state.
  Source: `/workspace/tmp-layout-engines/iced/wgpu/src/quad.rs:17`.
- Quad batches preserve order as runs of solid or gradient quads.
  Source: `/workspace/tmp-layout-engines/iced/wgpu/src/quad.rs:235`.

Fit for Emerge:

- A renderer rewrite is not justified.
- The useful idea is command shaping: common UI rectangles can become more direct
  draw operations instead of several path-heavy primitives.
- Adjacent run information can guide diagnostics without changing order.

### Slint

Relevant non-retention mechanisms:

- Slint's software renderer represents rounded rectangles and gradients with
  specialized command data rather than generic paths everywhere.
  Source: `/workspace/tmp-layout-engines/slint/internal/renderers/software/scene.rs:500`.
- Its Skia renderer is structured around an item renderer that can choose
  specialized drawing paths per item.
  Source: `/workspace/tmp-layout-engines/slint/internal/renderers/skia/lib.rs:668`.

Fit for Emerge:

- Specialized draw commands should be evaluated before the cache investigation
  chooses a retained-output solution for the same cost center.
- Direct primitive choice matters even when a higher-level renderer exists.

### Scenic Driver Skia

Relevant non-retention mechanisms:

- Fill and stroke shaders live in explicit draw state; setting a color clears the
  corresponding shader.
  Source:
  `/workspace/tmp-layout-engines/scenic_driver_skia/native/scenic_driver_skia/src/renderer.rs:452`.
- Paint application is centralized through `apply_fill_paint` and
  `apply_stroke_paint`.
  Source:
  `/workspace/tmp-layout-engines/scenic_driver_skia/native/scenic_driver_skia/src/renderer.rs:961`.
- Draw state snapshots preserve paint state across save/restore.
  Source:
  `/workspace/tmp-layout-engines/scenic_driver_skia/native/scenic_driver_skia/src/renderer.rs:1212`.

Fit for Emerge:

- Emerge currently builds paint locally per primitive.
- A small paint helper could centralize alpha, color, shader, filter, and
  antialiasing setup without retaining output.

### Skia APIs Available Locally

Useful direct APIs:

- `Canvas::draw_drrect` draws an outer and inner rounded rectangle pair. The
  local docs say GPU-backed platforms optimize this case and may draw it faster
  than an equivalent path.
  Source:
  `/home/dev/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/skia-safe-0.93.1/src/core/canvas.rs:1422`.
- `Paint::set_color_filter` is available.
  Source:
  `/home/dev/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/skia-safe-0.93.1/src/core/paint.rs:230`.
- `color_filters::blend` can create a one-pass color filter from a color and
  blend mode.
  Source:
  `/home/dev/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/skia-safe-0.93.1/src/core/color_filter.rs:124`.
- `Canvas::draw_shadow` is available through Skia shadow utils.
  Source:
  `/home/dev/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/skia-safe-0.93.1/src/utils/shadow_utils.rs:68`.

## Optimization Candidates

### 1. Benchmark and Diagnostic Gate

Before implementation, add benchmark cases that isolate direct drawing work:

- `solid_uniform_borders`
- `solid_edge_borders`
- `dashed_borders`
- `template_tinted_images`
- `alpha_single_primitive`
- `alpha_group_overlap`
- `shadow_mask_filter`
- `shadow_skia_utils`
- `gradient_rects`
- `clip_rect_vs_rrect`

Baseline:

```bash
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench renderer -- --save-baseline drawing_opt_before
```

Comparison:

```bash
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench renderer -- --baseline drawing_opt_before
```

Diagnostics to add before changing behavior:

- border detail: solid/dashed/dotted, uniform/asymmetric, zero-radius/rounded,
  path build time, clip time, draw time
- layer detail: alpha `saveLayer` count, tint `saveLayer` count, layer bounds area
- shadow detail: mask-filter path vs alternative path, clip time, draw time
- warmup detail: first frame after context creation and first frame after asset
  insertion

Success rule:

- Direct renderer benchmarks must not regress.
- Slow-frame logs must not show cost merely moving from `draw` to `gpu_flush`,
  `submit`, or `present submit`.

### 2. Solid Border Fast Paths

Current code builds a border band path for every solid border. Dashed/dotted
paths also use clipping around the border band.

Source: `native/emerge_skia/src/renderer.rs:2875`.

Direct alternatives:

- solid rounded border: use `draw_drrect(outer_rrect, inner_rrect, paint)`
- solid zero-radius uniform border: benchmark `draw_drrect` against four rects
- solid single-edge border: draw one rect directly
- keep current path implementation for dashed, dotted, complex, or ambiguous
  cases

Why this belongs here:

- It changes the primitive sent to Skia.
- It retains no output or prepared object.
- It targets measured border cost.

Validation:

- Pixel parity for uniform, per-corner, asymmetric edge, pill, and clipped cases.
- `border_clip_heavy` plus focused border benchmarks.
- Slow-frame border detail should improve without higher flush cost.

### 3. Template Tint Without `saveLayer`

Current template tint draws into a layer, draws a tint rect with `SrcIn`, then
restores.

Source: `native/emerge_skia/src/renderer.rs:2203`.

Direct alternative:

- Draw the image once with a paint color filter, such as
  `color_filters::blend(tint, BlendMode::SrcIn)`.
- Keep the current layer path as fallback if pixel tests show semantic drift.

Why this belongs here:

- It avoids an offscreen layer.
- It keeps one draw operation and one paint object.
- It does not retain the tinted result.

Validation:

- Pixel parity for transparent, semi-transparent, and opaque template assets.
- Focused `template_tinted_images` benchmark.
- Confirm non-tinted image draws are unchanged.

### 4. Opacity Distribution Before `saveLayer`

Current `RenderNode::Alpha` uses `save_layer_alpha` whenever alpha is below
`1.0`.

Source: `native/emerge_skia/src/renderer.rs:1391`.

Direct alternatives:

- single primitive child: multiply alpha into that primitive's paint color
- single image child: set paint alpha instead of layering
- single text child: set paint alpha instead of layering
- single shadow child: test separately, because blur and clipping can make alpha
  semantics sensitive
- keep `saveLayer` for overlapping children, nested alpha, uncertain blend cases,
  and any multi-child group until proven safe

Why this belongs here:

- It replaces an offscreen layer with direct paint attributes.
- It preserves fallback behavior for complex cases.

Validation:

- Pixel parity for single rect, rounded rect, image, text, shadow, overlapping
  children, and nested alpha.
- Benchmarks for `alpha_single_primitive` and `alpha_group_overlap`.
- No eligibility expansion without tests.

### 5. Shadow Direct-Draw Alternatives

Current outer shadow draws a blurred rounded rect with `MaskFilter::blur`, then
clips out the occluder.

Source: `native/emerge_skia/src/renderer.rs:2793`.

Direct alternatives:

- Benchmark current `MaskFilter::blur` path against `Canvas::draw_shadow` from
  Skia shadow utils.
- Test `GEOMETRIC_ONLY`, `TRANSPARENT_OCCLUDER`, and tonal color options only if
  they can reproduce Emerge's current visual model closely enough.
- Keep current path as fallback for CSS-like shadows that Skia shadow utils cannot
  match.

Why this belongs here:

- It compares two direct Skia draw commands.
- It does not retain a blurred shadow bitmap.

Validation:

- Pixel tests for blur, radius, spread, offset, transparent center, clipped
  ancestors, and scroll-panel shadow overflow.
- `shadow_mask_filter` and `shadow_skia_utils` benchmarks.
- Slow-frame logs should split shadow prepare, clip, and draw.

### 6. Clip Simplification

Current clip nodes use rect or rrect clips, and some border/image paths add path
clips.

Direct alternatives:

- Prefer `clip_rect` and `clip_rrect` over `clip_path` where geometry is still
  rectangular or rounded-rectangular.
- Avoid nested save/restore churn around clips when the child sequence has no
  shadow pass escape.
- Add clip detail to slow-frame logs before changing behavior.

Validation:

- Pixel tests for rounded image clipping, shadow escape, nearby overlays, and
  nested scroll clips.
- Benchmarks for `clip_rect_vs_rrect` and border-heavy scenes.

### 7. Shader/Pipeline Warmup Without Retained Emerge Output

Skia may compile GPU programs lazily. A warmup pass can draw representative
primitives after context creation and flush before first visible content.

Warmup scene should include:

- clear
- solid rect
- rounded rect
- solid border fast path
- dashed border fallback
- text draw
- linear gradient
- image draw
- tinted image direct path
- clipped image
- shadow current path
- alpha direct path and alpha layer fallback

Rules:

- Do not store Emerge-rendered output.
- Do not add persistent SkSL or shader-object management in this document.
- Make warmup optional or stats-gated at first.
- Log warmup duration.

Validation:

- Compare first visible frame, first post-asset frame, and steady-state frames.
- Warmup must not increase startup cost enough to hide the benefit.
- Direct renderer benchmarks should remain unchanged, because warmup is a
  startup/cold-frame behavior change.

### 8. Backend and Flush Discipline

Backend settings can affect direct drawing even without retaining output.

Candidates:

- Record GL config details in stats: samples, stencil size, color format.
- Compare current samples/stencil settings against a conservative no-stencil
  configuration if the backend permits it.
- Keep the current split between Skia `flush` and `submit`, but make sure
  experiments report both.

Why this belongs here:

- Flutter's context options explicitly avoid stencil buffers on OpenGL due to
  regressions.
- Emerge's slow frames often show `gpu_flush`, so backend state matters.

Validation:

- No backend option change without showcase/assets/todo stats before and after.
- Verify DRM and Wayland separately when possible.

### 9. Paint Setup Consolidation

Current primitives construct local `Paint` values repeatedly. That is simple, but
it scatters alpha, antialiasing, shader, filter, and color-filter policy across
the renderer.

Direct alternative:

- Add helper functions for fill paint, stroke paint, image paint, and text paint.
- Keep them allocation-light and explicit.
- Use them to implement tint and opacity changes consistently.

Why this belongs here:

- It does not retain output.
- It reduces implementation risk for direct draw optimizations.

Validation:

- Existing renderer pixel tests.
- Focused benchmarks should not regress from extra abstraction.

## Completed and Deferred Order

### Slice 1: Direct Drawing Benchmarks and Diagnostics

Status: completed.

Work:

- Add focused renderer benchmark cases listed above.
- Save `drawing_opt_before`.
- Add border, layer, shadow, clip, and cold-frame diagnostics.

Exit criteria:

- The next direct optimization target is selected from measured cost.

### Slice 2: Solid Border Fast Paths

Status: completed.

Work:

- Add `draw_drrect` path for solid rounded borders.
- Add direct rect path for simple solid edge borders.
- Keep existing fallback.

Exit criteria:

- Border benchmarks improve or stay neutral.
- Pixel parity passes.

### Slice 3: Tint and Alpha Layer Reduction

Status: completed.

Work:

- Replace eligible template tint layers with color-filter drawing.
- Replace eligible single-child alpha layers with direct paint alpha.
- Keep fallback paths.

Exit criteria:

- Tinted image and alpha benchmarks improve or stay neutral.
- Complex layer cases remain visually identical.

### Slice 4: Shadow Alternative Benchmark

Status: completed; no renderer change landed.

Work:

- Benchmark current `MaskFilter::blur` shadows against Skia shadow utils.
- Keep the current path unless parity and timings justify a switch.

Exit criteria:

- Shadow choice is evidence-based and does not rely on retained bitmaps.

### Slice 5: Warmup Experiment

Status: benchmark gate added; no warmup behavior landed.

Work:

- Add optional warmup draw pass after GL context creation.
- Record warmup duration and first-frame stats.

Exit criteria:

- Cold-frame slow logs improve without unacceptable startup cost.

### Slice 6: Backend/Flush Option Pass

Status: deferred until fresh backend traces justify it.

Work:

- Log GL config details.
- Compare conservative backend options where possible.

Exit criteria:

- Backend changes are backed by stats and do not regress presentation.

## Design Rules

- Benchmark before implementation.
- Keep all changes direct-rendering only.
- Prefer documented Skia primitives over equivalent generic paths.
- Treat `saveLayer` as a correctness fallback, not the default implementation for
  tint and simple opacity.
- Preserve scene order.
- Keep fallback paths until pixel parity and benchmark results prove the new path.
- Check max frame time, draw, GPU flush, submit, and present, not only averages.
- Any retained-output idea belongs in the render-cache documents, not here.
- If a cache and a direct draw change target the same cost, measure the direct
  path first or document why it is deferred.

## Source Index

Emerge:

- `native/emerge_skia/src/renderer.rs`
- `native/emerge_skia/src/video.rs`
- `native/emerge_skia/src/backend/skia_gpu.rs`

Local engine sources:

- `/workspace/tmp-layout-engines/flutter_engine/shell/common/switches.h`
- `/workspace/tmp-layout-engines/flutter_engine/shell/common/context_options.cc`
- `/workspace/tmp-layout-engines/flutter_engine/lib/ui/platform_dispatcher.dart`
- `/workspace/tmp-layout-engines/flutter_engine/shell/common/animator.h`
- `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/binding.dart`
- `/workspace/tmp-layout-engines/flutter_engine/impeller/display_list/dl_dispatcher.cc`
- `/workspace/tmp-layout-engines/webrender/webrender/src/batch.rs`
- `/workspace/tmp-layout-engines/iced/wgpu/src/engine.rs`
- `/workspace/tmp-layout-engines/iced/wgpu/src/quad.rs`
- `/workspace/tmp-layout-engines/slint/internal/renderers/software/scene.rs`
- `/workspace/tmp-layout-engines/slint/internal/renderers/skia/lib.rs`
- `/workspace/tmp-layout-engines/scenic_driver_skia/native/scenic_driver_skia/src/renderer.rs`

Local dependency sources:

- `/home/dev/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/skia-safe-0.93.1/src/core/canvas.rs`
- `/home/dev/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/skia-safe-0.93.1/src/core/paint.rs`
- `/home/dev/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/skia-safe-0.93.1/src/core/color_filter.rs`
- `/home/dev/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/skia-safe-0.93.1/src/utils/shadow_utils.rs`
