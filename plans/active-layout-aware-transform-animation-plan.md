# Active Layout-Aware Transform Animation Plan

Last updated: 2026-05-06.

Status: implemented in the current working tree. Focused benchmark coverage is
added, but Criterion before/after baselines still need a dedicated local run.

## Purpose

Make the top-level layout-aware transform attrs animatable:

- `Emerge.UI.scale/1`
- `Emerge.UI.rotate/1`

They should work in:

- `Animation.animate/4`
- `Animation.animate_enter/4`
- `Animation.animate_exit/4`

This is intentionally different from `Emerge.UI.Transform.scale/1` and
`Emerge.UI.Transform.rotate/1`. The `Transform` helpers stay paint-only and can
refresh without layout. The top-level `Emerge.UI` helpers stay layout-aware and
therefore relayout when they animate.

Do not silently downgrade layout-aware animation into paint-time animation.

## Current Code Facts

- `lib/emerge/engine/attr_schema.ex` does not list `:layout_scale` or
  `:layout_rotate` in `@animatable_keys`, so Elixir validation rejects them in
  animation keyframes today.
- Normal attrs already have EMRG tags:
  - `:layout_scale` -> `78`
  - `:layout_rotate` -> `79`
- Rust normal attr decoding already accepts both tags in `decode_attrs/1`.
  Animation keyframe payloads are decoded as normal attrs, so the wire format is
  already compatible once validation and sampling support exist.
- Native animation sampling does not yet include these fields:
  - exit retargeting retargets paint `rotate` and `scale`, not layout-aware
    `layout_rotate` and `layout_scale`
  - `scale_animation_keyframe/2` preserves paint `rotate` and `scale`, but does
    not preserve layout-aware transform fields
  - `interpolate_attrs/3` does not interpolate them
  - `apply_sample_attrs/2` does not apply them
  - `classify_animation_sample_attrs/1` does not mark them layout-affecting
- Static attr patches already classify `layout_scale` and `layout_rotate` as
  measure-affecting in `native/emerge_skia/src/tree/invalidation.rs`.
- Static layout scale has a special dirtiness path:
  `ElementTree::mark_layout_scale_dirty/1` dirties the scaled subtree because
  descendant effective attrs depend on the scale.
- Current frame attr preparation computes effective scaled attrs before applying
  animation overlays. That is correct for existing animated pixel attrs, but it
  is not sufficient for animated `layout_scale`, because `layout_scale` decides
  the scale factor used to prepare the same node and all descendants.
- The active-animation fast path prepares only active node ids. Animated
  `layout_scale` must either expand that set to the affected subtree or use a
  different source pass so descendant attrs are recomputed with the animated
  scale.

## Semantics

`Emerge.UI.scale/1` keyframes:

- are unitless positive finite numbers
- interpolate linearly in scale space
- affect the animated node and its full retained local subtree for that frame
- compose with the global window scale and ancestor layout scales
- are not multiplied by global scale when keyframes are scaled

`Emerge.UI.rotate/1` keyframes:

- are finite degree values
- interpolate linearly in degrees
- support arbitrary angles from the first implementation
- use the existing quarter-turn fast paths whenever the sampled value normalizes
  to `0`, `90`, `180`, or `270`
- reserve the sampled rotated AABB during layout for that frame

First-keyframe behavior should match existing layout animations: the first
keyframe establishes the initial layout state before animation time advances.
When a once animation completes, the final sampled keyframe remains the effective
state just like existing animated layout attrs.

Conflict rules should stay consistent with the static layout-aware transform
rules:

- a single keyframe cannot contain both `scale/1` and `Transform.scale/1`
- a single keyframe cannot contain both `rotate/1` and `Transform.rotate/1`
- an element should not combine static layout-aware scale/rotate with an
  animated paint-only transform on the same axis, or the reverse
- static and animated values for the same field are allowed because animation
  overlays replace the field while active

## Implementation Plan

### 1. Public Validation And Docs

- Add `:layout_scale` and `:layout_rotate` to
  `Emerge.Engine.AttrSchema.animatable_keys/0`.
- Add animation normalization for:
  - `:layout_scale`: finite number greater than `0.0`
  - `:layout_rotate`: finite number
- Add animation conflict validation for layout-aware vs paint-only scale/rotate
  inside keyframes.
- Add element-level conflict validation across static attrs and nested
  animation keyframes so an element cannot effectively use both transform
  systems on the same axis.
- Update `lib/emerge/ui/animation.ex` animatable attr docs to list
  layout-aware transforms separately from paint-only transforms.
- Add Elixir tests for accepted layout-aware transform keyframes, invalid
  values, and conflict errors.

### 2. Native Animation Field Support

Update `native/emerge_skia/src/tree/animation.rs`:

- retarget `layout_scale` and `layout_rotate` for interrupted exit animations
- preserve both fields in `scale_animation_keyframe/2`
- interpolate both fields in `interpolate_attrs/3`
- apply both fields in `apply_sample_attrs/2`
- classify both fields as layout-affecting

`layout_scale` must remain unitless when keyframes are scaled. `layout_rotate`
must also remain unitless/degrees.

Update `native/emerge_skia/src/tree/invalidation.rs`:

- include `layout_rotate` in animation registry-refresh relevance
- treat `layout_scale` as registry-relevant through its measure/subtree dirty
  path, not as a paint transform

### 3. Correct Layout Scale Sampling

Do not implement animated `layout_scale` by only applying an overlay after
`scale_attrs/2`. That would leave descendants prepared with the stale static
scale.

Refactor frame attr preparation around a raw frame source:

```text
declared attrs
  + raw animation sample for this frame
  -> effective local layout_scale for this node
  -> composed effective scale
  -> scale_attrs(merged_raw_attrs, composed_effective_scale)
```

Important details:

- sample `layout_scale` before computing subtree effective scales
- root `layout_scale` animation should stay on the flat global-scale-equivalent
  path whenever there are no descendant layout scales
- nested `layout_scale` animation should recompute the affected subtree's
  effective attrs, not the whole tree unless the animated node is the root
- keyframe pixel attrs on the same node should be scaled by the sampled
  `layout_scale` for that frame
- `layout_scale` inside `animate_enter` and `animate_exit` must preserve current
  capture-scale behavior for pixel attrs while keeping `layout_scale` itself
  unitless

The first implementation can use a simple raw-sample map keyed by `NodeId`.
Only optimize further if benchmarks show the map or traversal dominates.

### 4. Dirtying And Refresh Decisions

Animated `layout_rotate`:

- records `TreeInvalidation::Measure`
- marks render and registry refresh dirty through the existing measure path
- can use normal measure-boundary propagation

Animated `layout_scale`:

- records `TreeInvalidation::Measure`
- uses the same dirtiness semantics as a static layout-scale patch
- dirties the animated subtree because descendant effective attrs change
- propagates parent measure dirtiness through existing boundaries
- for root scale animation, intentionally dirties the whole tree each frame

Paint-only transform animations must keep the existing refresh-only path. Adding
layout-aware animation must not regress the decision path for
`Transform.rotate/1`, `Transform.scale/1`, or other paint-only animations.

### 5. Cache Behavior

- Do not add renderer-cache eligibility for layout-aware rotated payloads in
  this slice.
- Keep scale-only cache semantics based on the already-scaled effective attrs
  and frames.
- Ensure subtree measure, resolve, render, and registry cache keys see the
  sampled effective attrs/frame state.
- Verify unrelated sibling cache reuse still works when a nested layout-aware
  scale or rotate animates.
- Verify active layout-aware animation never reuses stale registry geometry.

### 6. Benchmark Gate

Before implementation, save a baseline that covers existing static transforms,
existing paint-only animation, and no-transform cases:

```bash
cargo bench \
  --manifest-path native/emerge_skia/Cargo.toml \
  --bench layout \
  --features bench-diagnostics \
  -- "layout_aware_transform|layout_animation_paint_only" \
  --save-baseline layout_aware_transform_animation_before

cargo bench \
  --manifest-path native/emerge_skia/Cargo.toml \
  --bench renderer \
  --features bench-diagnostics \
  -- cache_candidates_translated \
  --save-baseline layout_aware_transform_animation_before
```

Add focused Criterion cases before accepting the implementation:

- no-transform warm layout/refresh, to catch regressions when the new attrs are
  absent
- existing paint-only `Transform.scale/1` and `Transform.rotate/1` animation,
  to catch refresh-only regressions
- root `scale(1.0 -> 1.25)` and `scale(1.0 -> 1.5)`
- nested `scale(1.0 -> 1.25)` with unrelated sibling cache reuse
- root `rotate(0 -> 90)` under portrait physical constraints
- root `rotate(0 -> 45)`
- nested `rotate(0 -> 45)` in row/column/scroll layouts
- combined layout-scale animation with a pixel attr animation on the same node
- registry rebuild and pointer dispatch under animated scale and rotate

Acceptance gate:

- no statistically meaningful regression in no-transform cases
- paint-only animation still takes the refresh-only path after warm layout
- root layout-scale animation stays close to current global-scale cost for the
  same tree shape
- nested layout-scale animation avoids recomputing unrelated branches where the
  existing measure boundaries allow it
- arbitrary rotation cost is documented as an explicit layout-animation cost

### 7. Validation Tests

Rust tests:

- `sample_animation_spec` interpolates `layout_scale` and `layout_rotate`
- `scale_animation_spec` preserves both fields as unitless values
- exit retargeting starts from the current sampled layout-aware values
- animated root scale updates descendant effective width, padding, font, image,
  scroll, shadow, and motion attrs
- animated nested scale updates only the affected subtree plus required ancestor
  measure/resolve state
- animated layout scale combined with animated width is scaled by the sampled
  layout scale for the same frame
- animated layout rotate updates measured/render frames and AABB reservation
  each sampled frame
- animated root quarter-turn rotation swaps logical constraints at `90` and
  `270`
- animated arbitrary root rotation does not auto-fit the viewport
- hit testing and clip chains use the sampled rotated/scaled geometry
- scroll extents include sampled scaled and rotated children
- paint-only transform animation still refreshes without layout after warmup
- layout-aware transform animation performs layout each active frame

Elixir tests:

- `Animation.animate([[scale(1.0)], [scale(1.25)]], ...)` validates and
  round-trips
- `Animation.animate([[rotate(0)], [rotate(90)]], ...)` validates and
  round-trips
- invalid `scale(0)`, negative scale, infinity, and NaN are rejected
- invalid rotate infinity and NaN are rejected
- layout-aware and paint-only transform conflicts are rejected with clear errors
- docs examples compile

## Non-Goals

- no layout-aware scale or rotate in `mouse_over/1`, `focused/1`, or
  `mouse_down/1`
- no non-uniform layout scale
- no arbitrary matrix API
- no transform-origin API
- no automatic fit/scale for arbitrary root rotation
- no compositor-only or GPU-only shortcut for layout-aware animation
- no renderer-cache expansion for rotated payloads
- no change to the paint-only semantics of `Emerge.UI.Transform`
