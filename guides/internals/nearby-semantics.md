# Nearby Semantics

## Purpose

This document records the intended nearby behavior for `above`, `below`,
`on_left`, `on_right`, `in_front`, and `behind_content`.

It also records the current architectural split:

- visual nearby semantics are implemented now
- event/focus/state parity is deferred until nearby is integrated into the
  retained event system

The target reference is elm-ui's intended nearby model, not current CSS border-box
quirks.

## Current Data Model

- Elixir exposes nearby through `Emerge.UI` attrs (`above`, `below`, `on_left`,
  `on_right`, `in_front`, `behind_content`).
- EMRG encodes each nearby attr as `u32 len + EMRG subtree bytes`.
- Rust stores those payloads in `Attrs` as raw nested subtree bytes.
- nearby roots still receive normal element ids during Elixir reconciliation
  before serialization.
- Rust currently decodes, lays out, and renders nearby on demand during the render
  pass.

Consequences of the current model:

- nearby visuals can be correct without changing the main tree model
- nearby is not yet a first-class retained subtree for interaction/runtime state
- id stability exists at serialization time, but Rust does not yet preserve nearby
  runtime state against those ids because nearby is not retained in the main tree

## Visual Slot Model

Nearby layout is driven by a slot owned by the host element.

### `in_front`

- slot rect: host border-box (`Frame`)
- default origin: host top-left
- active alignment axes: horizontal and vertical
- `width fill`: fills slot width
- `height fill`: fills slot height
- explicit sizes may exceed the slot and overflow

### `behind_content`

- same slot geometry as `in_front`
- same fill and overflow rules as `in_front`
- rendered between host background and host content

### `above`

- slot width: host width
- slot height: content-sized
- default origin: host left edge, above host top
- active alignment axis: horizontal only
- `width fill`: fills host width
- `height fill`: does not stretch; it behaves like content-sized height

### `below`

- same slot rules as `above`
- default origin: host left edge, below host bottom

### `on_left`

- slot width: content-sized
- slot height: host height
- default origin: host top edge, left of host
- active alignment axis: vertical only
- `height fill`: fills host height
- `width fill`: does not stretch; it behaves like content-sized width

### `on_right`

- same slot rules as `on_left`
- default origin: host top edge, right of host

## Alignment Rules

Nearby root alignment uses the nearby root element's own `align_x` / `align_y`.

- `above` / `below`: use `align_x`; ignore `align_y`
- `on_left` / `on_right`: use `align_y`; ignore `align_x`
- `in_front` / `behind_content`: use both axes

Default nearby alignment is start/start:

- horizontal default: left
- vertical default: top

Examples:

- `in_front` + `centerX`: center horizontally inside the host slot
- `in_front` + `alignBottom`: pin bottom edge to host bottom edge
- `above` + `alignRight`: pin right edge to host right edge
- `on_right` + `centerY`: center vertically against host height

## Fill And Overflow Rules

- `fill` only stretches on the slot axis
- explicit `px` sizes are never auto-clamped to the slot
- when explicit size exceeds slot size, overflow direction depends on alignment:
  - start alignment overflows toward positive space
  - center alignment overflows on both sides
  - end alignment overflows toward negative space

Examples for a `100px` wide host and a `160px` wide `in_front` child:

- default / `alignLeft`: `x = 0`, visible span `0..160`
- `centerX`: `x = -30`, visible span `-30..130`
- `alignRight`: `x = -60`, visible span `-60..100`

## Render And Clip Order

Current visual ordering:

1. host transform
2. host shadows/background
3. host overflow clip begins
4. `behind_content`
5. host content / children
6. host border / scrollbars
7. front nearby (`above`, `below`, `on_left`, `on_right`, `in_front`)
8. host transform restore

Clip behavior:

- `behind_content` renders under the active host overflow clip
- front nearby re-applies the host overflow clip before drawing
- ancestor clips remain active, so nearby still respects ancestor clip lineage
- no clip is introduced from border radius alone; clip semantics come from overflow
  clipping (`clip_x`, `clip_y`, scroll clipping)

Important distinction:

- slot geometry uses the host border-box for `in_front` / `behind_content`
- clip geometry still uses normal overflow clip rules, which may clip to padded
  content bounds on clipped axes

## Ordering Semantics Across The Tree

Current render traversal preserves these useful elm-ui-style properties:

- parent `in_front` renders after child `in_front`
- parent `behind_content` renders before child overlays but after the parent's
  background
- later siblings render after earlier siblings, so overlapping outside-nearby on
  sibling hosts follows source order

## Known Current Limits

Nearby is still visual-only.

- nearby is not included in rebuilt listener data
- nearby is not hit-testable
- nearby focus/text-input runtime state is not preserved as first-class node state
- root `in_front` does not yet have a viewport-fixed special case

## Deferred Full-Parity Requirements

When nearby is integrated into the retained event system, it should move from
render-local subtrees to first-class retained mounts.

Requirements for that later phase:

- nearby geometry must be computed once and shared by render, hit testing, and
  listener registry construction
- nearby nodes need host-scoped stable identity so runtime hover/focus/text-input
  state survives tree rebuilds
- listener precedence must match nearby render order
- clip and rounded-corner decisions must come from the same geometry source used
  by rendering
- root `in_front` should gain a viewport-fixed mode equivalent to elm-ui layout
  overlays

## Files To Revisit Later

- `native/emerge_skia/src/tree/render.rs`
- `native/emerge_skia/src/tree/layout.rs`
- `native/emerge_skia/src/tree/interaction.rs`
- `native/emerge_skia/src/events.rs`
- `native/emerge_skia/src/events/registry_builder.rs`
- `lib/emerge/reconcile.ex`
- `lib/emerge/diff_state.ex`
