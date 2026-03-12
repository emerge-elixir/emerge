# Nearby Semantics

## Purpose

This document records the retained nearby model for `above`, `below`,
`on_left`, `on_right`, `in_front`, and `behind_content`.

Public Elixir API remains attr-shaped, but nearby is now a first-class retained
mount relation internally.

## Retained Model

- Elixir still exposes nearby through `Emerge.UI` attrs.
- Reconciliation splits nearby out of normal attrs and treats each slot as a
  host-owned mount.
- Nearby root ids are host-scoped. The nearby root id is derived from
  `{host_id, slot, local_identity}`.
- EMRG v4 stores nearby as node-level mount refs, not attr-embedded EMRG blobs.
- Rust stores nearby roots on `Element.nearby`, not in `Attrs`.
- Layout computes nearby frames once and render, interaction, and listener
  rebuild all consume that retained geometry.

## Canonical Traversal Order

Nearby uses the same retained order for paint-sensitive traversal, hit testing,
listener precedence, and focus order:

1. host element
2. `behind_content`
3. normal `children`
4. `above`
5. `on_right`
6. `below`
7. `on_left`
8. `in_front`

This is the order exposed by `Element::for_each_paint_child(...)` on the Rust
side.

## Slot Geometry

### `in_front`

- slot rect: host border-box (`Frame`)
- default origin: host top-left
- active alignment axes: horizontal and vertical
- `width fill`: fills slot width
- `height fill`: fills slot height
- explicit sizes may overflow the slot

### `behind_content`

- same slot geometry as `in_front`
- same fill and overflow rules as `in_front`
- rendered after host background/pre-layers and before normal children

### `above`

- slot width: host width
- slot height: content-sized
- default origin: host left edge, above host top
- active alignment axis: horizontal only
- `width fill`: fills host width
- `height fill`: remains content-sized

### `below`

- same slot rules as `above`
- default origin: host left edge, below host bottom

### `on_left`

- slot width: content-sized
- slot height: host height
- default origin: host top edge, left of host
- active alignment axis: vertical only
- `height fill`: fills host height
- `width fill`: remains content-sized

### `on_right`

- same slot rules as `on_left`
- default origin: host top edge, right of host

## Alignment Rules

Nearby root alignment uses the nearby root element's own `align_x` / `align_y`.

- `above` / `below`: use `align_x`; ignore `align_y`
- `on_left` / `on_right`: use `align_y`; ignore `align_x`
- `in_front` / `behind_content`: use both axes

Default nearby alignment is start/start (`left`, `top`).

## Clip And Scroll Semantics

- nearby inherits ancestor clip lineage
- nearby also respects the host overflow clip when rendered/hit-tested through a
  clipped host
- nearby does **not** inherit the host content scroll translation
- normal children do inherit the host content scroll translation

That distinction is important: nearby is retained with the host, but it is not a
flow child and it is not part of the host scrollable content plane.

## Consequences

- nearby is hit-testable
- nearby participates in listener rebuilds
- nearby participates in focus order
- nearby hover/focus/text-input runtime state is preserved like normal retained
  nodes
- listener precedence now matches nearby paint order automatically

## Key Files

- `lib/emerge/reconcile.ex`
- `lib/emerge/serialization.ex`
- `lib/emerge/patch.ex`
- `native/emerge_skia/src/tree/element.rs`
- `native/emerge_skia/src/tree/layout.rs`
- `native/emerge_skia/src/tree/render.rs`
- `native/emerge_skia/src/tree/interaction.rs`
- `native/emerge_skia/src/events/registry_builder.rs`
