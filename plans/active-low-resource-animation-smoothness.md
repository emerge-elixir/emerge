# Active plan: low-resource animation smoothness

## Status

Initial implementation committed in this work series. User validation after `b01caeb` confirmed enter animations now start and finish correctly, but animations still feel choppy on a resource-constrained DRM device. Desktop is smooth. Emerge 0.3.1 felt somewhat smoother on the same device.

Implemented so far:

- Added Criterion fixture `native/sidepane_animation_smoothness/in_front_move_x` with:
  - `enter_patch_first_frame`
  - `move_x_pulse_retained_payload`
  - `move_x_pulse_content_dirty_control`
- Added Criterion fixture `native/macaw_viewport/full_viewport` modeled after `../smartrent_hub/smartrent_hub/macaw_support/lib/macaw_support/ui/viewport.ex` and `view/climate.ex`, including 10x repeated status lines:
  - `open_patch_first_frame_one_toggle`
  - `close_patch_exit_first_frame_one_toggle`
  - `move_x_pulse_retained_payload`
  - `move_x_pulse_content_dirty_control`
- Added a `PatchTree` regression proving inserted nearby enter animations render the first frame at the start transform.
- Added paint-generation regression for transform-only animation refresh dirtying.
- Kept the final-frame no-stale-transform regression and extended it to assert transform-only enter samples do not bump content paint generation.
- Added transform-only animation dirtying that clears stale render fragments and marks ancestors for traversal without bumping content paint generation or clearing render layer payload caches.
- Avoid root cacheable paint-layer wrapping while any descendant render damage is active, to avoid root cache churn during animated descendant frames.
- Allow active move-x/move-y animation roots outside scroll containers to use the existing retained moving paint-layer path, so clean pulse frames can reuse stable sidepane payload content and update only the wrapper transform.
- Let animation pulse frames for transient `animate_enter` / `animate_exit` use active-node-only frame-attr preparation when no other invalidation is pending. This avoids full-tree attr prep during the 125ms sidepane animation while keeping completion frames on the broader dirty path.
- Tried a small tree-actor patch decode cache for repeated patch binaries; device validation showed no improvement, so the cache was removed to avoid extra overhead/noise.
- Added patch actor split timings: decode, apply, animation sync, frame-attr prepare, layout, and refresh.
- Device split showed patch-frame costs mostly in prepare attrs (`~7.3ms`), refresh (`~6.7ms`), apply (`~4.4ms`), and layout (`~3.5ms`).
- Added partial frame-attr preparation for patch recompute frames when the patch dirty set is known (animated nearby insert/remove sidepane case): prepare active animation ids and inserted subtree roots instead of the whole retained tree.
- Device validation showed patch prepare attrs dropped to `~0.49ms`, making patch refresh the largest remaining cost, followed by apply and layout.
- Added deeper patch refresh split timings for render-scene build vs registry rebuild/refresh.
- Device split showed patch refresh is roughly half render-scene build (`~3.34ms`) and half registry rebuild/refresh (`~3.23ms`).
- Implemented a paired render+registry traversal for dirty-registry layout refresh frames, preserving clean-registry reuse and refresh-only animation pulse paths.
- Benchmarked detached layout-cache storage/restore for transform-only animated nearby subtrees; local Criterion showed no statistically significant improvement for the repeated sidepane reopen path, so the experiment was dropped.
- Tried transform-only animated nearby insertion with refresh-local nearby layout, but reverted it after a device freeze report on second pane open. Animated nearby inserts are back to `Resolve` for safety; local `enter_patch_first_frame` did not show a statistically significant Criterion improvement from the risky shortcut anyway.

## Device timing evidence

Observed stats window on the device:

```text
backend: drm
frames: 37 over 5004 ms => 7.4 fps
display: 60.0 fps (16.667 ms/frame)
render: avg=4.028 ms
present submit: avg=0.810 ms
pipeline submit->frame callback: avg=56.531 ms count=7
pipeline tree: avg=24.819 ms count=7
pipeline submit->swap: avg=30.561 ms count=7
pipeline swap->frame callback: avg=25.969 ms count=7
layout: avg=2.673 ms count=11
refresh: avg=10.040 ms count=36
patch tree actor: avg=24.768 ms count=7
renderer cache paint_layer:
  candidates=349 visible=349 admitted=0 hits=313 stores=0
  evictions=126 stale_evictions=126
```

## Measurement log

| Build/run | Device fps | Frames/5s | Refresh avg | Patch actor avg | Pipeline tree avg | Paint stale evictions | Notes |
|---|---:|---:|---:|---:|---:|---:|---|
| Before smoothness pass | 7.4 | 37 | 10.040ms | 24.768ms | 24.819ms | 126 | Correct final-frame fix, but cache churn high. |
| After transform dirty/root-cache pass | 9.2 | 46 | 10.155ms | 24.090ms | 24.140ms | 1 | Cache churn fixed; tree/refresh still bottleneck. |
| After active move-x payload reuse | 8.2 | 41 | 9.861ms | 24.496ms | 24.553ms | 0 | Device run had 8 patch/open-close operations in the 5s window. Local sidepane benchmark: retained move-x pulse `~26.7µs` vs content-dirty control `~110.9µs` (~76% faster). Full Macaw benchmark: retained pulse `~37.0µs` vs content-dirty control `~132.7µs` (~72% faster). |
| After transient active-only prep | 10.6 | 53 | 5.476ms | 24.768ms | 24.818ms | 0 | Device run had 9 patch/open-close operations in the 5s window. Local Macaw diagnostic transient pulse prepare dropped from about `0.079ms` to `0.002-0.004ms`; refresh/profile frames are now about `0.036-0.045ms` after warmup. |
| Patch decode cache experiment | 9.6 | 48 | 5.615ms | 26.222ms | 26.271ms | 2 | Did not help; removed. |
| Patch actor split timings | 10.4 | 52 | 5.779ms | 24.457ms | 24.515ms | 1 | Split: decode `1.563ms`, apply `4.434ms`, animation sync `0.699ms`, prepare attrs `7.279ms`, layout `3.457ms`, refresh `6.670ms`. |
| Patch recompute partial attrs | 8.4 | 42 | 5.780ms | 17.427ms | 17.486ms | 0 | Patch prepare attrs dropped to `0.487ms`; remaining patch split: decode `1.810ms`, apply `3.974ms`, animation sync `0.702ms`, layout `3.489ms`, refresh `6.621ms`. |
| Patch refresh deeper split | 9.6 | 48 | 5.531ms | 17.482ms | 17.568ms | 4 | Refresh split: render scene `3.342ms`, registry `3.225ms`; remaining cost is balanced between both. |
| Unified combined refresh traversal | pending | pending | pending | pending | pending | pending | Needs target-device run. Local Macaw open patch profile uses one combined bucket: `refresh=0.175ms`, `render_scene/traversal=0.174ms`, `registry_post=0.000ms`; patch-frame Criterion remains roughly open `~436µs`, close `~517µs`, reopen `~505µs`. Retained `move_x` pulse is `~46µs`, so watch pulse cost on device too. |
| Transform-only detached layout restore experiment | n/a | n/a | n/a | n/a | n/a | n/a | Dropped. Local `second_open_patch_first_frame_after_exit`: no-cache baseline `466.69µs`, experiment `453.03µs`, Criterion change `-3.77%` with `p=0.38`; no significant improvement. |

Interpretation:

- GPU/Skia drawing is not the only problem: render draw+flush is about `4ms`, present submit is about `0.8ms`.
- The tree actor is too expensive on patch frames: `pipeline tree` / `patch tree actor` is about `25ms`, already more than one 60Hz frame.
- The 5s `fps` number must be normalized by sidepane toggles: the latest run rendered `41` frames for `8` open/close patches, about `5.1` rendered frames per 125ms animation window (ideal 60Hz would be about `7.5`).
- Animation pulse frames still spend about `10ms` in native `refresh`, leaving little budget for render and present on weak hardware.
- The cache churn problem is fixed in the latest run (`stale_evictions=0`, paint-layer hits `369`), so remaining work is patch-frame tree cost and per-pulse refresh/render cost.
- With a 125ms animation, even a few missed vblanks produce visibly large jumps, especially for `ease_out` exits.

## Goal

Recover smoother animation on constrained devices without changing app code or animation specs.

Target budgets on the device:

- transform-only animation refresh: ideally `< 3ms`, acceptable `< 5ms`
- patch frame tree work during animation: ideally `< 8ms`, acceptable `< 12ms`
- render+present: keep below one vblank where possible
- no regressions to animation correctness: first frame starts animated, final frame settles, exit ghosts prune correctly

## Regression guardrails from the start/end fixes

Do not begin cache/invalidation simplification until these existing regressions are kept and the missing coverage below is added.

Existing regression coverage to preserve:

- `runtime::tree_update::tests::completed_enter_animation_refreshes_final_base_attrs_before_stopping`
  - covers a sidepane-like `Nearby.in_front` enter animation through the real tree update path.
  - proves the final render output has no stale transform before `animations_active` becomes false.
- `tree::animation::tests::completed_enter_hands_off_to_base_attrs_when_no_animate_is_present`
  - proves completed enter sync reports paint dirty effects and hands off to base attrs.
- `tree::animation::tests::completed_enter_starts_regular_animation_from_zero_progress`
  - proves a regular `animate` begins correctly after transient enter completes.
- `tree::patch::tests::test_insert_nearby_subtree_with_animation_requires_resolve_instead_of_refresh_layout`
  - protects against the first-mount nearby refresh shortcut that could flash the final/open state before runtime sync.
- Exit ghost runtime tests in `tree::animation` must keep passing so exit pruning is not broken while optimizing enter.

Additional required coverage before or alongside transform-only dirtying:

1. **First-frame no-flash through `PatchTree`.** Insert an animated `Nearby.in_front` subtree through the encoded patch/tree-update path and assert the first emitted scene uses the enter start transform, not the final/base transform. **Done.**
2. **Final-frame no-stale-cache with cache reuse enabled.** Warm nearby render fragment / paint-layer caches, run enter to completion, and assert the final scene is fully open with `animations_active == false` and no stale previous transform. **Done for enter in the tree-update regression, including stable paint-layer payload reuse across clean move-x pulses.**
3. **Exit final/prune visual correctness.** Run a sidepane exit ghost through start/mid/final samples and assert the final output either shows the completed exit frame before prune when required or removes the ghost only after no stale partially-exited content can remain. **Still required before further exit-path optimization.**
4. **Interrupted enter-to-exit handoff.** Start enter, trigger exit before completion, and assert exit starts from the current visual transform without jumping to either endpoint. **Partially covered for retargeting alpha; still add sidepane move-x coverage before changing exit dirtying.**
5. **Hit-test registry follows transform-only motion.** For an animated overlay with mouse listeners/blockers, assert hit results change with sampled positions and settle at final geometry. **Existing animated nearby hit-case coverage exercises this; keep it in the validation set.**

When an optimization changes animation invalidation, these tests should fail first if the old bugs are reintroduced. Do not replace them with only performance assertions.

## Main hypothesis

Emerge currently treats transform-only animation samples (`move_x`, `move_y`, `rotate`, `scale`, `alpha`) as generic paint damage. That calls render dirtying paths that bump paint generations and clear render fragment/layer caches. For a sidepane animation, the subtree content is stable; only placement/opacity changes. Invalidating content caches on every sample forces too much refresh traversal and cache churn.

The fix direction is to split "placement/transform damage" from "content paint damage" so transform-only animation frames can reuse cached subtree content and only update wrapper transforms / hit-test geometry.

## Plan

### 1. Add better device-oriented instrumentation first

Current stats combine several costs. Add temporary or gated timing/counter breakdowns before optimizing:

- tree update phases:
  - patch decode
  - patch apply
  - animation sync / attr prep
  - layout
  - render scene build
  - registry rebuild / cached registry refresh
- render refresh damage counters:
  - render dirty node count
  - registry dirty node count
  - active animation ids count
  - active animation invalidation class counts: transform-only, content-paint, resolve, measure
- cache churn counters:
  - paint generation bumps caused by transform-only animation
  - render fragment cache clears caused by transform-only animation
  - render layer cache clears caused by transform-only animation

Use this to confirm whether the `10ms refresh` is mostly render scene build, registry rebuild, or cache churn.

### 2. Build a reproducible benchmark fixture. **Done for synthetic sidepane.**

Create a native benchmark/test fixture matching the production sidepane shape:

- large/root viewport similar to the device app
- `Nearby.in_front` sidepane
- enter/exit specs matching SmartRent climate:
  - enter: `move_x(500 -> 0), 125ms, ease_in`
  - exit: `move_x(0 -> 500), 125ms, ease_out`
- enough background content to match current tree/cache pressure

Measure:

- first mount frame
- mid enter pulse
- final enter pulse
- exit start/mid/final
- repeated open/close after caches are warm

Keep the benchmark runnable on desktop and compare relative changes before target-device validation.

Current commands:

```bash
cd native/emerge_skia && cargo bench --features bench-diagnostics --bench layout -- sidepane_animation_smoothness --sample-size 10 --warm-up-time 1 --measurement-time 2
cd native/emerge_skia && cargo bench --features bench-diagnostics --bench layout -- macaw_viewport --sample-size 10 --warm-up-time 1 --measurement-time 2
EMERGE_BENCH_DIAGNOSTICS=1 cargo bench --features bench-diagnostics --bench layout -- macaw_viewport/full_viewport/enter_transient_pulse_retained_payload --sample-size 10 --warm-up-time 1 --measurement-time 1
```

Latest local synthetic sidepane results:

- `enter_patch_first_frame`: ~`322.8µs`; no significant change in the now-reverted refresh-local nearby insert experiment.
- `move_x_pulse_retained_payload`: ~`26.7µs`.
- `move_x_pulse_content_dirty_control`: ~`110.9µs`.
- retained payload pulse is ~`76%` faster than the content-dirty control.

Latest local Macaw full-viewport results:

- `open_patch_first_frame_one_toggle`: ~`502µs`.
- `close_patch_exit_first_frame_one_toggle`: ~`613µs`.
- `second_open_patch_first_frame_after_exit`: ~`540µs`.
- `enter_transient_pulse_retained_payload`: ~`97µs` Criterion / `~0.036-0.045ms` diagnostic-profile warm frames.
- `move_x_pulse_retained_payload`: ~`37µs`.
- `move_x_pulse_content_dirty_control`: ~`138µs`.
- retained payload pulse is ~`73%` faster than the content-dirty control.
- diagnostic transient pulse frame-attr prep is now active-only (`~0.002-0.004ms`) instead of full-tree (`~0.079ms` observed before the change).

### 3. Split transform-only animation damage from content paint damage. **Initial implementation done.**

Introduce a narrower invalidation/effect category for animation attrs that only affect placement/compositing:

- `move_x`, `move_y`
- `rotate`, `scale`
- `alpha`
- possibly `layout_rotate/layout_scale` only when they do not affect measured size; audit carefully before including

Desired behavior:

- mark the node/path for render scene rebuild or transform wrapper update,
- update registry geometry when hit testing can change,
- do **not** bump content `paint_generation`,
- do **not** clear render fragment/layer payload caches when only transform/alpha changed,
- still clear/invalidate content caches for real paint/content changes: colors, borders, shadows, text, image, video, etc.

Implementation options to evaluate:

1. Add a new internal refresh damage kind, e.g. placement/transform damage, separate from `TreeInvalidation::Paint`.
2. Keep public `TreeInvalidation::Paint`, but add animation effect flags so `mark_animation_refresh_effects_dirty` can call a lighter dirtying path.
3. Add separate generations:
   - content generation for cached payloads,
   - placement generation for transform wrappers / registry geometry.

The lowest-risk first step is option 2: specialize animation dirtying while leaving normal patch invalidation unchanged. The current worktree implements this first step for transform-only animation effects.

### 4. Reuse animated subtree content during transform-only frames. **Implemented for active move-x/move-y animation roots.**

After damage is split, make sure render refresh can actually reuse cached content:

- `try_reuse_moving_paint_layer_cache` should be allowed when only transform placement changed. **Done for active move-x/move-y animation roots, while preserving tests that forbid opportunistic non-scroll moving layers for ordinary static/dirty transforms.**
- nearby render fragment cache keys should not include transform-only animation state as content damage.
- paint-layer `content_generation` should stay stable across transform-only frames.
- wrapper transform can change every frame while the payload image/content stays cached.

Success indicators in stats:

- `refresh` drops significantly on animation pulse frames,
- `stale_evictions` no longer climbs during transform-only sidepane animations,
- paint layer hits remain high,
- stores/admissions do not churn every animation.

### 5. Avoid full recompute for first-mount transform-only nearby enter

The correctness fix currently forces animated nearby inserts through `Resolve` to avoid first-frame flash. That is safe but may be too expensive on weak hardware.

Refine it after the completion fix remains covered:

- classify inserted nearby animation specs as:
  - transform/composite-only,
  - content-paint,
  - layout-affecting.
- for transform/composite-only enter:
  - sync transient animation runtime even on refresh-only patches,
  - apply the first animation sample before render refresh,
  - use nearby-local refresh/layout shortcut when host and subtree frames are already known or can be restored,
  - keep `Resolve` only for layout-affecting animation or missing geometry.

Regression requirements:

- no first-open full-position flash,
- final frame still settles,
- hit testing follows animated position,
- no stale registry while overlay moves.

### 6. Reduce registry work for transform-only animation

Moving overlays can affect hit testing, but the registry does not always need a full rebuild.

Audit whether we can update only geometry/runtime state for active animated ids:

- reuse listener/action definitions from cached registry,
- recompute screen transforms/clips only for animated subtree and affected overlay blocker entries,
- leave unrelated listener lists untouched.

This should help the `refresh` metric if registry rebuild is a meaningful part of the `10ms` cost.

### 7. Patch-frame scheduling/coalescing during active animations

Patch frames cost about `25ms` on the device. If app patches arrive while an animation is active, they can cause visible jumps.

After instrumentation identifies what those 7 patch frames are:

- coalesce multiple pending `PatchTree` messages into one tree update before rendering,
- avoid rendering intermediate patch states when a newer patch is already queued,
- consider scheduling non-visual/cold patches after the current animation pulse when safe,
- ensure input/registry correctness still wins over visual coalescing.

Do not drop semantic final states; only skip superseded intermediate render work.

### 8. DRM pacing audit only after core refresh cost improves

The current device bottleneck is mostly tree/refresh work, not physical display timing. Keep DRM page-flip-driven backpressure.

After CPU-side improvements:

- compare frame callback gaps and pulse cadence again,
- verify `display` remains physical 60Hz,
- only revisit DRM timestamp/future-vblank prediction if refresh/render work is below budget and cadence is still uneven.

## Validation

Automated:

- `cd native/emerge_skia && cargo test`
- `cd native/emerge_skia && cargo test --no-default-features --features drm`
- `mix test`
- sidepane benchmark before/after with stats assertions for refresh/cache churn

Manual target-device checks:

- SmartRent climate sidepane first open
- repeated open/close
- exit animation smoothness
- renderer stats before/after:
  - `refresh` lower
  - `patch tree actor` lower or fewer visible patch frames
  - `stale_evictions` stable during transform-only animation
  - achieved frames during animation closer to display cadence

## Non-goals

- Do not change SmartRent app animation specs.
- Do not slow desktop behavior for the device case.
- Do not paper over CPU refresh cost by only changing animation duration or easing.
- Do not reintroduce final-frame stuck or first-frame flash bugs.
