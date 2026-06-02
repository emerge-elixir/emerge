# Active plan: transient enter animation completion

## Status

Implemented in the current worktree; target app/device validation pending.

The native regression below failed before the core fix with a stale partial translate (`[432.89114]`) on the final frame, then passed after completion dirtying.

Current symptom from `../smartrent_hub` sidepane:

- `animate_enter` starts and advances.
- The visual state stops before the final keyframe, often around 75% open.
- No further animation pulses are requested after it stops.
- Reproduces on both Wayland and DRM, so the primary bug is in tree animation/update/retained refresh, not in a single presentation backend.

## Relevant production case

`../smartrent_hub/smartrent_hub/macaw_support/lib/macaw_support/ui/view/climate.ex` mounts the sidepane through `Nearby.in_front(Climate.sidepane())` with an enter animation equivalent to:

```elixir
Animation.animate_enter(
  [[Transform.move_x(500)], [Transform.move_x(0)]],
  125,
  :ease_in
)
```

The expected final render is the base attrs with `move_x == nil/0`, fully open, and `animations_active == false` only after that final state has been refreshed/rendered.

## Audit notes

Primary code paths to audit before changing behavior:

- `native/emerge_skia/src/tree/animation.rs`
  - `AnimationRuntime::sync_with_tree/2`
  - `sample_animation_overlays/3`
  - `sample_animation_overlays_for_ids/4`
  - `sample_animation_for_element/3`
  - `AnimationOverlayResult::record_sample/2`
- `native/emerge_skia/src/runtime/tree_update.rs`
  - animation sampling and invalidation planning around `had_animation_runtime`, `had_transient_animations`, `dynamic_invalidation`, and `plan.animations_active`
  - refresh decision after the final transient frame
- `native/emerge_skia/src/tree/layout.rs`
  - `prepare_frame_attrs_for_update/4`
  - `prepare_animation_frame_attrs_for_update/4`
  - `prepare_dirty_frame_attrs_for_update/5`
  - `mark_animation_refresh_effects_dirty/2`
  - `mark_animation_layout_effects_dirty/2`
- `native/emerge_skia/src/tree/patch.rs`
  - nearby insert shortcuts for animated mounted subtrees
- Backends only after core audit:
  - Wayland: `native/emerge_skia/src/backend/wayland/runtime.rs`
  - DRM: `native/emerge_skia/src/backend/drm.rs`

Observed suspicious interaction:

1. While the enter animation is active, samples produce animation effects and mark refresh/layout dirtiness.
2. When `animate_enter` completes, `AnimationRuntime::sync_with_tree/2` can remove the transient `enter_entry` before frame attr preparation.
3. `sample_animation_for_element/3` filters transient samples with `sample.active`; inactive final transient samples are not recorded.
4. The final frame can therefore have:
   - no active transient entry,
   - no recorded animation effect for the completed node,
   - `dynamic_invalidation == TreeInvalidation::None`,
   - `plan.animations_active == false`.
5. Before the fix, `TreeUpdateEngine` added only a broad `Paint` invalidation for `animation_sample_requested && had_animation_runtime && !plan.animations_active && dynamic_invalidation.is_none()`, but that did not identify and dirty the completed node/subtree whose cached render fragment still contained the previous sampled transform.
6. Retained refresh can reuse the previous animated subtree/frame, then backends stop sending pulses because `animations_active == false`.

This would explain a cross-backend stuck partial-open frame.

## Fix plan

### 1. Reproduce with a native regression test first. **Done.**

Added a test that uses the real tree update/refresh path, not just pure animation sampling.

Required shape:

- host element with a nearby `InFront` sidepane root,
- sidepane has fixed size and `animate_enter(move_x: 500 -> 0, duration: 125ms, curve: ease_in)`,
- sample at first frame, middle frame, and after duration,
- inspect the rendered scene or prepared tree attrs/frames to prove:
  - first frame uses the enter transform,
  - middle frame is partially translated,
  - final frame uses the base fully-open transform,
  - final update still emits render output/refresh for the completed node,
  - final output reports `animations_active == false` only after the fully-open state is present.

Prefer placing focused coverage near existing animation/update tests. If direct `TreeUpdateEngine` setup is too large, start with a lower-level layout/refresh retained-cache test, then add actor/update coverage.

### 2. Make transient completion observable. **Done.**

Changed animation sync/preparation so completed transient animations are reported with node ids and final dirty requirements.

Preferred shape:

- Add a small completion result from `AnimationRuntime::sync_with_tree`, for example:
  - completed enter node ids and their spec/effect classification,
  - completed exit ghost ids if needed for symmetry.
- Do not rely on a global `Paint` fallback to express completion.
- Keep completed transient reporting separate from active samples so `animations_active` can remain false on the final settled frame.

Important: final enter completion should mean "remove the overlay and render base attrs", not "sample and apply another active overlay".

### 3. Mark completed enter nodes dirty precisely. **Done.**

For each completed enter node, mark the affected node/subtree dirty using the same classification rules as animation samples:

- paint/registry refresh for paint/transform-only attrs such as `move_x`, `move_y`, `rotate`, `scale`, `alpha`, colors, shadows, etc.
- resolve/measure dirtiness for layout-affecting attrs such as width, height, padding, spacing, font size, layout scale/rotate as currently classified.
- registry refresh when the completed transform/geometry can affect hit testing or nearby blockers.

Implementation should reuse or expose existing animation classification helpers instead of duplicating attr lists.

### 4. Ensure the final frame cannot be skipped/reused stale. **Done.**

After completion dirtying:

- `plan.invalidation` must be dirty for the completed node.
- `mark_animation_refresh_effects_dirty/2` / layout dirty marking must invalidate cached render fragments/layers for the completed node and affected ancestors.
- Refresh-only should be valid for transform-only completion, but it must rebuild the render fragment from base attrs rather than reusing the last animated fragment.
- A final output with `animations_active == false` is valid only if that output contains the final settled render state.

### 5. Preserve first-mount enter behavior for nearby inserts. **Done.**

Animated nearby insertion should not take refresh-only shortcuts before the runtime has created/sampled the enter entry.

Keep or rework the current local idea:

- if `Patch::InsertNearbySubtree` inserts a subtree containing `animate`, `animate_enter`, or `animate_exit`, classify it as at least `Resolve`/recompute rather than the fast nearby refresh-only path.
- Add a test proving first mount does not flash fully open before the first sampled enter frame.

### 6. Remove backend-specific completion workarounds. **Done for the enter-completion workaround.**

Because the bug reproduces on Wayland and DRM, backend changes should not be the primary fix.

After the core final-frame regression passes:

- revert/remove DRM-only "do not skip after animated primary" style workaround unless a separate test proves it is still needed,
- keep the DRM physical display interval/stat seeding fix if still valid,
- ensure Wayland draw/noop paths are untouched unless tests identify a separate backend issue.

## Validation checklist

Automated:

- `cd native/emerge_skia && cargo test --no-default-features --features drm` ✅
- `cd native/emerge_skia && cargo test` ✅
- `mix test` ✅

Manual:

- `../smartrent_hub` sidepane first open on Wayland:
  - no full-open flash,
  - enter reaches fully open,
  - no stuck partial-open state.
- Same on DRM target.
- Repeated open/close cycles.
- Confirm exit animation still completes and cleanup/ghost pruning still works.

## Non-goals

- Do not change `../emerge_demo` or `../smartrent_hub` app code.
- Do not add GPU/layout offload work.
- Do not paper over the bug with backend pulse timing changes unless core tests show backend-specific loss after the final frame is correct.
