# Element-local mouse coordinates

Status: implemented; automated validation passed. Native-window smoke checks remain.
Branch: `feat/mouse-local-coordinates`
Worktree: `/workspace/emerge-mouse-local-coordinates`

## Agreed scope and contract

Add position payloads to these existing helpers only:

- `on_mouse_move/1`
- `on_mouse_down/1`
- `on_mouse_up/1`
- `on_mouse_enter/1`
- `on_mouse_leave/1`
- `on_click/1`

Payload: `%{x: float(), y: float()}`. Coordinates are positions, not movement
increments or normalized fractions. The origin is the listener element's outer
border-box top-left, including the border/padding area. Undo inherited and local
paint transforms, then express the result in that element's logical layout pixels.

For an element with logical width W and height H, inside positions span those
logical dimensions regardless of display/layout/paint scaling. A 100px-wide
element displayed 200 physical pixels wide reports `x: 35.5` at 71 physical pixels
from its displayed left edge. Always deliver floats, including at scale 1.
Do not round to integer pixels or clamp: leave positions can be negative or exceed
W/H. Fill/content/animated sizes use the actual resolved logical box, not a ratio
against the declared width/height attrs.

Do not change `on_press`, swipes, scroll progress, keyboard/focus/change events,
raw observers, button eligibility, hit testing, propagation, capture, coalescing,
or dispatch order. Existing touch-to-pointer projection receives the same payloads
where it already triggers these events; this is not a new touch API.

## Public delivery and compatibility

Keep helper signatures and encoded listener-presence flags unchanged. Use the
existing payload-bearing element envelope and viewport `wrap_payload/3` path:

```elixir
# Event.on_mouse_move(:moved)
{:moved, %{x: 35.5, y: 20.0}}

# Event.on_click({self(), {:selected, item_id}})
{:selected, item_id, %{x: 35.5, y: 20.0}}

# Low-level renderer input target
{:emerge_skia_event, {element_id_binary, :mouse_move, %{x: 35.5, y: 20.0}}}
```

This deliberately changes delivered message shapes for the six events. Document
migration from bare messages, including handlers targeting another PID and custom
`wrap_payload/3` callbacks. Do not emit a second legacy message or add an opt-in
flag. `on_press` keeps its current payload-free pointer/keyboard behavior.

## Implementation steps

### 1. Shared coordinate geometry and typed payload

- [x] Add a dedicated position payload with f64 fields and a typed variant in
  `events/registry_builder.rs`; encode it as the fixed-shape Elixir map, separate
  from scroll's normalized-fraction semantics.
- [x] Reuse `element.layout.dimension_facts.scale`, which records the effective
  scale used during layout preparation, including inherited/animated
  `layout_scale`. Do not infer scale from width ratios or walk ancestors per event.
- [x] Expose that scale through resolved interaction geometry in `tree/scene.rs`
  and retain a compact surface-to-element-logical mapping alongside `PointerRegion`.
  Use the existing interaction inverse and scroll-adjusted **render** border-box
  origin, not the transformed screen-space AABB or a scrollbar subregion origin.
  Conceptually: inverse-transform the surface point, subtract the adjusted render
  origin, then divide by the effective layout scale. Paint scaling is already
  undone by the inverse; do not divide it out twice.
- [x] Keep coordinate conversion independent from `contains`/clip checks so leave
  points remain reportable outside the shape. Preserve existing non-invertible
  transform eligibility; never manufacture NaN/infinite coordinates.
- [x] Include the new mapping/scale in pointer/hover equality and cache hashes,
  plus registry cache dependencies. A scale change must refresh coordinates even
  when a fill-sized physical frame is unchanged. Reuse existing retained metadata;
  no new full-tree pass or BEAM-side geometry lookup.

### 2. Compute positions at dispatch, including hover and click followups

- [x] Use one shared position-materialization helper for raw down/up/move actions,
  click completion, and synthetic hover transitions. Store geometry in listeners
  or trackers, not precomputed event positions in cached action lists.
- [x] Down uses its press sample; up uses its release sample and remains targeted
  to the element under the release. Click uses the completing release sample and
  the click tracker's source element, rebound to the accepted registry geometry.
  A release may produce up/click for different elements: calculate each separately.
- [x] Compute enter/leave payloads using each affected hover tracker's own mapping,
  including parent/child hover stacks. Do not decorate every action with the
  winning listener's coordinates. Update tracker extraction/reconciliation if
  action computation changes its current static-list assumptions.
- [x] Preserve leave -> raw -> enter ordering, style cleanup, drag cancellation,
  mount ownership, registry freshness barriers and stationary-hover revalidation.
  Geometry-driven enter/leave uses the stationary pointer's known surface position.
- [x] Fix the window-leave path's current synthetic `(0, 0)` placeholder for
  coordinate delivery. Use the last real position preceding that leave in listener
  order; retain this separately from the ingress snapshot if necessary so later
  buffered input cannot overwrite it. Do not invent an exit point or change the
  raw window-enter/leave observer contract. Test the invariant that active hover
  has a known position; do not introduce a public nil-coordinate payload.

### 3. Native and macOS delivery

- [x] Extend `events.rs` and `events/runtime.rs` to send the typed map through the
  existing `{id, event_kind, payload}` NIF envelope and `HostEventSink` interface.
- [x] Extend `native/emerge_skia/src/bin/macos_host.rs` and
  `lib/emerge_skia/macos/protocol.ex` so all six kinds deliver the identical map.
  Keep event kind IDs and the outer notification envelope; encode the coordinate
  payload as two fixed-width f64 values, preserving negative/fractional values.
  Reject missing/malformed coordinate payloads without applying scroll's [0, 1]
  validation. Leave unrelated payload codecs unchanged.
- [x] Bump the Rust/Elixir macOS protocol versions together (currently 17) so old
  host binaries cannot silently deliver payload-free events or opaque binaries.
  Update handshake/protocol fixtures and require a matching rebuilt host.
- [x] Reuse `lib/emerge/runtime/viewport.ex`'s generic payload routing rather than
  adding special per-mouse routing or rebuilding the event registry.

### 4. Regression tests

- [x] Rust geometry/registry tests for all six kinds: offsets, non-square boxes,
  borders/padding, fractional positions, and scales 1, 2 and fractional values.
  Verify the exact 71 -> 35.5 example and always-float payloads.
- [x] Test global scale, nested/local/animated `layout_scale`, local/inherited paint
  scale, and combined scaling without double conversion. Cover translation,
  rotation, nested scrolling, nearby elements, and layout-vs-render frame origins.
- [x] Test negative/oversized leave coordinates, window leave's ordered fallback,
  ancestor/child hover coordinates, stationary geometry changes, and degenerate
  transforms. Rounded/clipped geometry must retain existing hit eligibility.
- [x] Extend runtime/delayed-pointer recorders to inspect payloads. Cover replay
  against accepted geometry, same-mount capture updates, remount cancellation,
  click release vs press position, and up/click with different target elements.
  Raw observers must remain unchanged and must not fire again during replay.
- [x] Cover cached/uncached registry equivalence and scale-only changes with an
  unchanged physical fill frame. Preserve hover/style/click/press ordering tests.
- [x] ExUnit routing tests for every kind: bare/tuple messages, external PID,
  default/custom `wrap_payload/3`, unchanged helper flags and unaffected `on_press`
  and raw observers. Existing generic viewport routing should need no logic change.
- [x] macOS codec round-trip/fixture tests for all six kinds, negative/fractional
  values, malformed/missing payloads and version mismatch. Exercise the native
  encoder as well as the Elixir decoder; do not rely only on hand-built frames.

### 5. Documentation and validation

- [x] Update `lib/emerge/ui/event.ex` docs/types with the payload, coordinate space,
  scaling example, unclamped leave behavior and new `handle_info/2` shapes.
- [x] Add a public example to `guides/tutorials/handle_input.md` distinguishing
  element-local positions from unchanged raw surface positions. Update stale
  internal statements in `guides/internals/events.md` and add an Unreleased
  breaking-message-shape note to `CHANGELOG.md`. Do not link public docs to internal
  guides or include internal references in release-facing documentation.
- [x] Run focused Rust and ExUnit tests during implementation, then required
  `cargo test --manifest-path native/emerge_skia/Cargo.toml` and `mix test`.
- [x] Run `./ci-tests.sh all` for formatting, warnings-as-errors compilation,
  Clippy, tests and Dialyzer; run `mix docs`. Record environmental blockers rather
  than claiming unexecuted checks passed. Use the documented macOS feature flags
  when running native tests on macOS.
- [ ] Smoke actual pointer move/down/up/click/enter/leave at scale 1 and 2 on
  available backends; qualify the rebuilt macOS host on macOS. Codec tests alone
  do not establish native window behavior on an unavailable platform.

## Completion criteria

Every emitted in-scope event carries `%{x: float(), y: float()}` in its own
listener element's logical pixels, with no clamping or extra integer rounding.
Existing eligibility, lifecycle ordering and mount safety remain intact, and
out-of-scope payloads/raw observers are unchanged. Both delivery transports agree;
message-shape migration and any unexecuted platform checks are documented.

## Validation results

- `./ci-tests.sh all`: passed formatting, warnings-as-errors compilation, Credo,
  Clippy, 702 ExUnit/doctest cases (12 hardware exclusions), 1,626 Rust unit tests
  plus 14 integration tests, and Dialyzer (zero errors).
- Standalone debug `cargo test` and focused ExUnit checks also passed during
  development. The final complete Rust run used CI’s release profile.
- `mix docs`: passed, including 31 documentation screenshots.
- No desktop display is available in this Linux environment. Actual window mouse
  smoke tests and a macOS host build/run remain unexecuted; the shared macOS
  encoder and Elixir protocol decoder were tested on Linux.
