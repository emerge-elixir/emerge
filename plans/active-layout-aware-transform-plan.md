# Active Layout-Aware Scale And Rotate Plan

Last updated: 2026-05-06.

Status: implemented in the current working tree; full Criterion comparison is
still pending because the pre-implementation benchmark run was interrupted.

## Purpose

Emerge already has paint-time transforms in `Emerge.UI.Transform`. Those
transforms affect rendering and hit testing, but they do not affect measured
size, sibling placement, scroll extents, or parent content size.

This plan adds layout-aware top-level UI attrs:

- `Emerge.UI.scale/1`: per-element layout scale, equivalent to the current
  global Wayland/window scale but scoped to a subtree.
- `Emerge.UI.rotate/1`: layout-aware rotation. `width` and `height` still name
  the authored unrotated box; parents reserve the rotated axis-aligned bounds.

Main use cases:

- root `scale/1` for accessibility or user preference scaling
- root `rotate(90 | 180 | 270)` for embedded/kiosk orientation override
- local `scale/1` for scaling a subtree with normal layout consequences
- local arbitrary `rotate/1` for badges, cards, labels, and other elements whose
  rotated visual footprint should participate in layout, hit testing, culling,
  and scroll extents

## External Validation

Primary references checked:

- Flutter
  [`Transform`](https://api.flutter.dev/flutter/widgets/Transform-class.html)
  is paint-time, while
  [`RotatedBox`](https://api.flutter.dev/flutter/widgets/RotatedBox-class.html)
  is layout-time and restricted to quarter turns. Flutter
  [`BoxHitTestResult.addWithPaintTransform`](https://api.flutter.dev/flutter/rendering/BoxHitTestResult/addWithPaintTransform.html)
  inverts a paint transform for hit testing, which validates transformed hit
  semantics but is not the only possible implementation strategy for a retained
  registry.
- Qt
  [`QTransform::mapRect` and `mapToPolygon`](https://doc.qt.io/qt-6/qtransform.html)
  distinguish mapped bounding rectangles from mapped polygon corners. This
  matches the Emerge split between AABB layout reservation and exact screen-space
  hit geometry.
- Skia
  [`SkMatrix::mapRect` and `mapRectToQuad`](https://api.skia.org/classSkMatrix.html)
  similarly expose both mapped bounds and mapped rectangle corners. This supports
  storing quads for exact transformed rectangle hit regions.
  [`SkPath::contains`](https://api.skia.org/classSkPath.html)
  and [`SkRegion::contains`](https://api.skia.org/classSkRegion.html) also
  provide low-level geometry containment checks. They validate the general shape
  of the solution, but they are not a retained UI hit-test registry with listener
  ordering, scrollbars, text-input subregions, clip chains, and event semantics.
- The
  [CSS Transforms spec](https://www.w3.org/TR/css-transforms-1/)
  applies transforms after sizing/positioning for normal CSS layout, but
  transformed geometry affects client rectangles and overflow. This supports
  keeping `Emerge.UI.Transform` paint-only while making `Emerge.UI.rotate/1`
  explicitly layout-aware.
- Wayland
  [`wl_surface.set_buffer_scale` and `set_buffer_transform`](https://wayland.freedesktop.org/docs/html/apa.html)
  model scale and display orientation as surface/buffer coordinate conversion.
  This supports treating root scale and quarter-turn root rotation as fast paths.

Design consequence:

- Do not retrofit layout behavior into `Emerge.UI.Transform`.
- Do not limit `Emerge.UI.rotate/1` to quarter turns.
- Optimize quarter turns because they are exact swaps/sign changes.
- Keep runtime pointer dispatch simple: one precomputed registry region, one
  `contains(screen_x, screen_y)` call.

## Current Code Facts

- Public paint transform helpers live in `lib/emerge/ui/transform.ex`.
- Existing paint attrs are `:move_x`, `:move_y`, `:rotate`, `:scale`, and
  `:alpha`.
- Existing EMRG transform tags:
  - `move_x` -> `31`
  - `move_y` -> `32`
  - `rotate` -> `33`
  - `scale` -> `34`
  - `alpha` -> `35`
- `native/emerge_skia/src/tree/layout.rs` applies global scale by calling
  `scale_attrs(&element.spec.declared, scale_factor)` before measurement and
  resolution.
- `scale_attrs` already scales width, height, padding, spacing, border
  width/radius, shadows, font size/spacing, image size, movement offsets, scroll
  offsets, and animation numeric fields.
- `NodeLayoutState.frame` is currently both the parent-visible layout slot and
  the visual/render geometry for backgrounds, content, children, and transform
  center.
- The current registry already stores transformed hit data through
  `PointerRegion`, `screen_bounds`, `screen_to_local`, and clip chains.

## API Shape

Add top-level helpers to `Emerge.UI`:

```elixir
Emerge.UI.scale(1.2)
Emerge.UI.rotate(-8)

# Inside modules that `use Emerge` or `use Emerge.UI`:
scale(1.2)
rotate(-8)
```

These helpers live in `Emerge.UI`, not `Emerge.UI.Transform` and not the root
`Emerge` module. `use Emerge` already calls `use Emerge.UI`, so viewport modules
still get bare `scale/1` and `rotate/1`.

Internal attrs:

- `:layout_scale`
- `:layout_rotate`

The internal names stay layout-specific to avoid colliding with existing
paint-only `:scale` and `:rotate`.

Validation:

- values must be finite numbers
- `scale/1` must be greater than `0.0`
- `rotate/1` accepts finite degrees
- first implementation rejects duplicate axes:
  - `Emerge.UI.scale/1` cannot be combined with `Transform.scale/1`
  - `Emerge.UI.rotate/1` cannot be combined with `Transform.rotate/1`

First slice is static attrs only. Interaction styles and animations can follow
after invalidation and cache behavior are proven.

## Data Model

Add Rust attrs:

```rust
pub layout_scale: Option<f64>,
pub layout_rotate: Option<f64>,
```

Add private EMRG tags after the current last tag:

```text
layout_scale  -> 78
layout_rotate -> 79
```

Update:

- `lib/emerge/ui.ex`
- `lib/emerge/engine/attr_codec.ex`
- `lib/emerge/engine/attr_validation.ex`
- `lib/emerge/engine/attr_schema.ex`
- `lib/emerge/ui/internal/validation.ex`
- `native/emerge_skia/src/tree/attrs.rs`
- `guides/internals/emrg-format.md`

## Scale Semantics

`scale/1` is not a render transform. It is scalar attr conversion before layout:

```text
effective_scale(node) =
  global_window_scale * ancestor_layout_scales * local_layout_scale

effective_attrs(node) =
  scale_attrs(spec.declared(node), effective_scale(node))
```

Rules:

- Always scale from `spec.declared`, never from already-scaled attrs.
- Keep `layout_scale` itself unitless.
- Measurement, resolution, rendering, registry, and cache keys continue to read
  already-scaled `layout.effective` attrs.
- No `RenderNode::Transform` is emitted for layout scale.
- No inverse transform is needed for scale-only hit testing because frames and
  clips are already scaled.
- Root `scale/1` behaves like global/window scale applied to the root subtree.
- A root scale change intentionally invalidates the whole app; unchanged root
  scale must be no more expensive than current unchanged global scale.
- Non-root scale changes dirty the scaled subtree and propagate measure dirtiness
  through existing boundaries.

Performance metadata:

```rust
pub effective_scale_bits: u32,
pub effective_attrs_dirty: bool,
```

Skip `scale_attrs` for a clean node/subtree when inherited scale, local scale,
declared attrs, interaction state, and animation state are unchanged.

## Rotate Semantics

`rotate/1` is layout-aware. It reserves the rotated AABB, but the element itself
is laid out unrotated.

Add render-specific frames:

```rust
pub measured_render_frame: Option<Frame>,
pub render_frame: Option<Frame>,
```

Frame meaning:

- `frame`: parent-visible reserved slot, normally the rotated AABB
- `render_frame`: unrotated visual/layout box used for backgrounds, borders,
  children, text, media, transform center, and hit-geometry construction
- `measured_frame`: measured parent-visible size
- `measured_render_frame`: measured unrotated size

When there is no layout-aware rotation:

```text
render_frame == frame
measured_render_frame == measured_frame
```

For a non-root rotated element:

1. Measure/resolve the unrotated element from already-scaled effective attrs.
2. Compute the rotated AABB.
3. Store the AABB in `frame` / `measured_frame`.
4. Center the unrotated box inside that AABB as `render_frame`.
5. Resolve children and internal decoration against `render_frame`.

For root rotation:

- `frame` is always the physical viewport. The root cannot reserve a larger
  parent slot.
- `rotate(90)` and `rotate(270)` use swapped logical constraints for
  `render_frame`. A physical `1080 x 1920` viewport lays out a logical
  `1920 x 1080` root.
- `rotate(0)` and `rotate(180)` use physical constraints.
- Arbitrary root angles are allowed, but they do not auto-fit or auto-scale.
  The root `render_frame` is laid out under physical constraints, rotated around
  its center, and clipped/overflowed by normal root behavior if its visual AABB
  exceeds the viewport.

This intentionally removes inverse constraint solving from the first
implementation. Automatic fit for arbitrary root angles can be a later explicit
API if there is a real product need.

Rotation math:

```text
outer_w = abs(cos(theta)) * inner_w + abs(sin(theta)) * inner_h
outer_h = abs(sin(theta)) * inner_w + abs(cos(theta)) * inner_h
```

Represent normalized rotation as:

```rust
enum RotationKind {
    None,
    QuarterTurns(i32),
    Arbitrary {
        radians: f32,
        sin: f32,
        cos: f32,
        abs_sin: f32,
        abs_cos: f32,
    },
}
```

Quarter-turn fast paths:

- `0`: identity
- `90` / `270`: swap width and height
- `180`: preserve width and height, flip render/hit transform

## Hit Testing

Keep dispatch simple. The event runtime should not know about broad vs precise
rotated matching. It should only call:

```rust
region.contains(screen_x, screen_y)
```

Build hit geometry during registry rebuild from the same transform/render frame
used by rendering.

Simplified registry geometry:

```rust
enum HitGeometry {
    Empty,
    Rect(Rect),
    Quad { points: [Point; 4], edges: [HalfPlane; 4], bounds: Rect },
    Local { shape: ShapeBounds, screen_to_local: Affine2, bounds: Rect },
}
```

Construction:

- identity, translation, uniform scale, and quarter turns collapse to `Rect`
  when the hit shape is rectangular
- arbitrary transformed rectangles become `Quad`
- rounded rectangles, rounded clips, and future non-rectangular shapes use
  `Local`
- clip chains are `Vec<HitGeometry>` and use the same `contains` path as pointer
  regions

Quad hit math:

```text
edge_i = p_{i+1} - p_i
inside_i(point) = cross(edge_i, point - p_i) has the expected sign
inside_quad = inside_0 && inside_1 && inside_2 && inside_3
```

Implementation should precompute edge coefficients so runtime hit testing does
not multiply by an inverse matrix for common transformed rectangles.

This `HitGeometry` path should be registry-wide:

- existing paint-only `Transform.move_x/1`
- existing paint-only `Transform.move_y/1`
- existing paint-only `Transform.scale/1`
- existing paint-only `Transform.rotate/1`
- layout-aware `rotate/1`
- scrollbar subregions
- text-input subregions
- clips

The current inverse-based path remains the correctness fallback for complex
shapes. The migration must preserve existing paint-only transform behavior.

Do not route common pointer matching through Skia by default. Emerge already
builds a retained event registry, and rectangular UI can be tested cheaper with
precomputed `Rect`/`Quad` geometry than by constructing or retaining Skia paths.
Skia path/region containment can be considered later as a fallback for genuinely
path-shaped hit areas if benchmarks show it is useful.

## Rendering And Culling

Rendering:

- Layout scale emits no render transform.
- Layout-aware rotate emits one transform for the rotated element/root.
- That transform is built around `render_frame` center.
- Reuse `RenderNode::Transform`; do not add a new render command.

Culling:

- Use `frame` as the conservative parent-visible AABB for layout-aware rotate.
- Existing conservative visual-bound logic still handles paint-only transforms,
  shadows, clips, and nearby mounts.
- Culling does not need exact quad tests in the first slice.

## Invalidation And Caches

Classify `layout_scale` and `layout_rotate` as layout-affecting:

- attr changes require `TreeInvalidation::Measure`
- registry refresh is required
- render refresh is required

Cache keys must include enough state to avoid stale output:

- `SubtreeMeasureAttrs`
- `ResolveAttrs`
- `RenderSubtreeKey` through attrs/frame/render-frame state
- `RegistrySubtreeKey` through attrs/frame/render-frame/hit-geometry source
  state
- detached nearby layout cache signatures when these attrs appear in nearby
  subtrees

Renderer cache:

- Do not reject scale-only clean subtrees just because they use
  `Emerge.UI.scale/1`; scale is already reflected in effective attrs, frames,
  and cache keys.
- Keep layout-aware rotated nodes out of clean-subtree renderer-cache candidates
  in the first implementation.

## Implementation Order

### 0. Pre-Implementation Benchmark Baseline

Before changing transform behavior, save Criterion baselines for the existing
native hot paths:

```bash
cargo bench \
  --manifest-path native/emerge_skia/Cargo.toml \
  --bench layout \
  --features bench-diagnostics \
  -- --save-baseline layout_aware_transform_before

cargo bench \
  --manifest-path native/emerge_skia/Cargo.toml \
  --bench renderer \
  --features bench-diagnostics \
  -- --save-baseline layout_aware_transform_before
```

Also run one Elixir retained-layout smoke so the BEAM serialization/diff path has
a pre-change reference:

```bash
mix bench.native.retained_layout
```

The baseline must include existing no-transform cases so we can detect
regressions when `Emerge.UI.scale/1` and `Emerge.UI.rotate/1` are not used. It
should also include current comparable behavior:

- global scale through `layout_and_refresh_default(..., scale = 1.25 | 1.5)` as
  the baseline for root `Emerge.UI.scale/1`
- existing paint-only `Transform.rotate/1` and `Transform.scale/1` renderer
  cases as the baseline for render-transform overhead
- existing registry rebuild and pointer-region benchmarks as the baseline for
  hit-testing overhead

### 1. Public Attrs

- add `Emerge.UI.scale/1` and `Emerge.UI.rotate/1`
- add validation and duplicate-axis checks
- add Elixir codec/schema entries
- add Rust attr decoding
- update EMRG docs
- add codec and validation tests

### 2. Layout Scale

- replace flat global-scale attr preparation with topology-aware scale
  propagation
- compose global, ancestor, and local layout scales
- add effective-scale metadata and skip unchanged clean subtrees
- keep ghost capture behavior correct
- add parity tests against global scale for size, padding, border, shadow, font,
  image, scroll, motion, and animation fields

### 3. Rotation Geometry

- add `RotationKind`
- add AABB helper
- add quarter-turn fast paths
- add `render_frame` and `measured_render_frame`
- add non-root rotated layout reservation
- add root quarter-turn logical-constraint swapping
- add arbitrary root rotation without auto-fit

### 4. Registry Hit Geometry

- add `HitGeometry`
- make `PointerRegion::contains` delegate to `HitGeometry::contains`
- migrate existing paint-only transformed pointer regions first
- migrate clips, scrollbars, and text-input subregions where rectangular/quad
  geometry is enough
- add fallback `Local` geometry for rounded and complex shapes

### 5. Render And Registry Integration

- render layout-aware rotate from `render_frame`
- build registry hit geometry from the same transform
- verify root rotation is baked into registry geometry, not a separate global
  input-coordinate remap
- keep viewport culling conservative

### 6. Cache And Invalidation Audit

- add new attrs to invalidation classification
- update measure/resolve/render/registry cache keys
- keep scale-only render-cache eligibility
- reject layout-aware rotate from renderer cache initially

### 7. Post-Implementation Benchmark Gate

Add focused Criterion cases before judging the implementation:

- no-transform retained layout/refresh fixtures, unchanged, to catch regressions
  when the new attrs are absent
- root `scale(1.25)` and `scale(1.5)`, compared with current global-scale cost
- nested local scale with unrelated branch reuse
- root `rotate(90)` under portrait physical constraints
- root `rotate(180)` and `rotate(270)` to cover the other quarter-turn fast
  paths
- arbitrary root rotation
- non-root arbitrary rotated element in row/column/scroll layouts
- render refresh for layout-aware rotated nodes, including the first version
  where rotated nodes are not clean-subtree cache candidates
- registry rebuild timing after scale/rotate changes
- pointer dispatch for `HitGeometry::Rect`, `Quad`, and `Local`

Then compare against the saved baseline:

```bash
cargo bench \
  --manifest-path native/emerge_skia/Cargo.toml \
  --bench layout \
  --features bench-diagnostics \
  -- --baseline layout_aware_transform_before

cargo bench \
  --manifest-path native/emerge_skia/Cargo.toml \
  --bench renderer \
  --features bench-diagnostics \
  -- --baseline layout_aware_transform_before

mix bench.native.retained_layout
```

Acceptance gate:

- no statistically meaningful regression in no-transform layout, refresh,
  renderer, registry rebuild, and pointer dispatch cases
- root `Emerge.UI.scale/1` stays close to current global-scale cost because it
  should be implemented as the same effective-attrs preparation, only scoped by
  topology
- quarter-turn root rotation should be near a constraint-swap and transform
  bookkeeping cost, not a full arbitrary-angle solve
- arbitrary rotation costs must be documented as an explicit feature cost
- any no-transform regression outside normal Criterion noise must be explained
  and either fixed or intentionally accepted in this plan before landing

## Validation

Run:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml
mix test
```

Focused Rust tests:

- local scale matches global scale for numeric attrs
- nested scale multiplication
- root accessibility scale under fill/default root constraints
- non-root rotate AABB math
- root quarter-turn constraint swapping
- arbitrary root rotation stays under hard viewport frame without auto-fit
- width/height name the unrotated authored box
- row/column sibling placement with scaled and rotated children
- scroll extents include scaled and rotated children
- render transform center matches `render_frame`
- scale-only hit testing uses normal scaled frames
- `HitGeometry::Rect`, `Quad`, and `Local` hit/miss tests
- paint-only transform hit-test parity after `HitGeometry` migration
- scrollbar and text-input hit parity after `HitGeometry` migration
- clip-chain parity
- root rotation registry matching in physical/screen coordinates
- cache misses when layout-aware scale/rotate attrs change
- unchanged root scale/rotate does not add avoidable steady-state work
- existing `Transform.rotate/1` and `Transform.scale/1` remain paint-only

## Non-Goals

- no skew, arbitrary matrix, or transform-origin API
- no non-uniform layout scale
- no text-only scaling API in the first slice
- no automatic fit/scale for arbitrary root rotation
- no transform-aware text shaping for rotation
- no renderer-cache expansion for rotated payloads
- no animation or hover-state layout-aware transforms in the first slice
- no layout reservation for existing `Emerge.UI.Transform` helpers

## Open Questions

- Should layout-aware rotation be allowed on scroll containers in the first
  implementation, or rejected until scrollbar geometry has dedicated coverage?
- Should decorative state and animation support come only after a real UI need
  appears?
