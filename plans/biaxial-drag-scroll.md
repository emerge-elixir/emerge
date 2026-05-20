# Biaxial Drag Scroll

Status: completed on 2026-05-20

## Goal

Allow drag-scrolling an oversized two-axis scroll container to move in both X
and Y once the drag threshold is crossed, instead of locking all active
drag-scroll movement to the first resolved axis.

## Context

Current behavior lives in `native/emerge_skia/src/events/registry_builder.rs`
and `events/runtime.rs`:

- Candidate drag promotion calls `drag_scroll_activation_axis/5`, which
  dispatches one unlocked `ListenerInput::DragScroll` and picks the first scroll
  request's axis.
- `DragTrackerState::Active` stores one `locked_axis`.
- Active motion redispatches drag scroll with `locked_axis: Some(locked_axis)`.
- `scroll_component_delta_for_axis/3` zeroes the off-axis delta, so a scrollable
  with both X and Y range only receives one axis during drag.

Wheel scrolling already splits X/Y components and can produce batched scroll
requests; drag scrolling should use the same idea after activation for
containers where both directions are currently scrollable.

## Plan

1. Preserve swipe/gesture threshold behavior.
   - Keep `GestureAxis` for swipe intent and as the primary axis for drag
     inertia.
   - Do not make ambiguous non-scroll swipe gestures fire both axes.
2. Add active drag-scroll mode information.
   - Add a small flag or enum to active drag state / promotion runtime change,
     e.g. `free_axes: bool` or `DragScrollMode::{Locked, Both}`.
   - Keep the current primary `locked_axis` for velocity sampling and inertial
     scroll.
3. Change activation detection.
   - Replace/extend `drag_scroll_activation_axis/5` with a helper that probes
     horizontal and vertical drag-scroll dispatch separately using
     `locked_axis: Some(Horizontal)` and `Some(Vertical)`.
   - If both probes produce a nonzero scroll request, promote with biaxial/free
     mode and choose primary axis from the larger local scroll delta.
   - If only one probe matches, promote exactly like today.
4. Change active drag redispatch.
   - Locked mode keeps today's single-axis dispatch.
   - Biaxial mode dispatches two `ListenerInput::DragScroll` events, one
     horizontal and one vertical, and concatenates actions.
   - Keep one `UpdateDragTrackerPointer` action per cursor move.
   - Compute the optional inertia `axis_delta` from the primary axis request if
     present; otherwise fall back to pointer sampling as today.
5. Tests.
   - Add/adjust registry-builder tests for a two-axis element: active diagonal
     drag emits two `ScrollRequest`s plus one pointer update.
   - Keep one-axis lock coverage proving off-axis movement still does not scroll
     unsupported axes.
   - Add direct runtime coverage that a two-axis drag move sends a batch/flat
     pair of X and Y scroll requests.
   - Keep rotated local-axis tests passing.

## Validation

Final validation on 2026-05-20:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml --lib events::registry_builder
cargo test --manifest-path native/emerge_skia/Cargo.toml --lib events::runtime
cargo test --manifest-path native/emerge_skia/Cargo.toml --lib
cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings
cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check
mix test
mix format --check-formatted
git diff --check
```

## Reliability follow-up

Completed on 2026-05-20 after reports that oversized two-axis canvases could
miss drag-scroll activation, stick to one axis, or leave the next scroll
container needing a second drag.

Changes made:

- Scroll-only candidates now probe both the current drag vector and a small
  opposite vector before promotion, so a threshold-crossing move into a blocked
  edge can still detect scroll potential in the reverse direction.
- Scroll-only candidates no longer clear the drag tracker merely because the
  first threshold-crossing movement is blocked; click/press tracking is cleared
  while the drag candidate remains available for a reverse move.
- Candidates remember whether they originated from a scrollable element so
  click-only drags still clear normally and swipe candidates keep the existing
  swipe precedence behavior.
- Locked drag-scroll matching accepts movement whose current pointer is just
  outside the region when the previous drag point was inside, improving edge
  activation/probing without changing regular hover or wheel hit testing.
- Regression coverage now includes blocked-edge biaxial promotion, swipe edge
  precedence, and a direct runtime reverse-drag path that emits both X and Y
  scroll requests after initially moving into a blocked edge.

Follow-up validation on 2026-05-20:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml --lib events::registry_builder
cargo test --manifest-path native/emerge_skia/Cargo.toml --lib events::runtime
cargo test --manifest-path native/emerge_skia/Cargo.toml --lib
cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings
cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check
mix test
mix format --check-formatted
git diff --check
```

## Open questions

- For a biaxial drag release, should inertia remain primary-axis only for now,
  or should future work add two-axis inertial scroll state?
- If one axis is blocked on the child but open on an ancestor, should biaxial
  mode continue to allow per-axis propagation independently? The proposed split
  dispatch does.
