# Combined render/registry refresh traversal

## Status

Cleanup implemented.

Current shape:

- one layout refresh entry path selects a `RegistryRefreshPlan` (`Rebuild` or
  `ReuseClean`) and calls `refresh_with_registry_plan{,_timed}`;
- one render refresh builder, `build_refresh_output_with_scroll_layers`, builds
  the scene through the combined traversal machinery;
- registry work is controlled by traversal sinks:
  - build sink collects registry data during render traversal;
  - reuse-clean sink is a no-op during traversal and refreshes cached runtime
    state afterward;
- dirty-registry patch frames report their combined render+registry walk in
  `patch tree refresh traversal` and should report near-zero
  `patch tree refresh registry post`;
- removed the layout-level dirty-registry-only helper branch and the separate
  render+registry builder name.

## Motivation

Patch-frame refresh on the SmartRent/Macaw sidepane currently spends roughly the
same time in the two refresh sub-stages:

- render scene build: `~3.34ms`
- event registry rebuild/refresh: `~3.23ms`

The current refresh path does those stages as separate tree walks:

1. `render_tree_scene_with_scroll_layers(tree)` builds the `RenderScene` and IME
   render metadata.
2. `refresh_from_render_output(...)` calls
   `registry_builder::build_registry_rebuild_cached(tree)` to rebuild the event
   registry.

For patch frames with registry damage, the clean-registry reuse path cannot be
used, so both passes walk much of the same retained structure. A combined pass
should be able to share traversal context and avoid repeated parent/child/nearby
walk overhead.

## Important caveat

The render and registry walks are not identical, even when their costs line up:

- render can viewport-cull or reuse render fragment / paint-layer caches;
- registry can skip subtrees by registry-affects metadata or reuse registry
  subtree chunks;
- registry traversal has event precedence requirements, scroll contexts, hover
  stacks, focus/text-input/sliders/scrollbars, and deferred escape-nearby
  overlay ordering;
- render traversal has paint ordering, host/self clip contexts, font context,
  moving-paint-layer policy, dynamic paint-layer emission, and IME cursor output.

So the goal is **not** to blindly make render own registry output. The goal is a
paired traversal that preserves the independent render and registry semantics
while sharing the physical walk where both need to visit the same subtree.

## Non-goals

- Do not change app code or animation specs.
- Do not change layout/measure/resolve behavior.
- Do not change event precedence, hover ownership, focus, text input, slider,
  scrollbar, or nearby blocker semantics.
- Do not replace the clean-registry reuse path; when registry is clean, the
  current render-only refresh is already cheaper.
- Do not remove the existing separate-pass path until combined traversal proves
  equivalent and faster.

## Current path to preserve

### Dirty-registry refresh

```text
refresh_timed(tree)
  render_tree_scene_with_scroll_layers(tree)
    render::build_element_subtree(...)
  refresh_from_render_output(tree, render_output)
    registry_builder::build_registry_rebuild_cached(tree)
      accumulate_subtree_rebuild_cached(...)
  clear_refresh_dirty()
```

### Clean-registry refresh

```text
refresh_reusing_clean_registry_timed(tree, cached_rebuild)
  if !tree.has_registry_refresh_damage()
    render_tree_scene_with_scroll_layers(tree)
    refresh_runtime_state_in_cached_rebuild(tree, cached_rebuild)
    clear_render_refresh_dirty()
```

Keep the clean-registry path separate.

## Proposed design

### 1. Add a combined refresh builder behind the existing API

Introduce a dirty-registry-only path conceptually like:

```rust
pub(crate) fn refresh_combined_dirty_registry_timed(
    tree: &mut ElementTree,
) -> (LayoutOutput, RefreshTiming)
```

It returns the same `LayoutOutput` as the current separate path. Timing should
still report:

- total refresh;
- combined render/registry walk time;
- any fallback/separate registry time if a subtree cannot be safely combined.

Initially call it only from a benchmark/test-only entry point. After it proves
safe, wire it into `refresh_timed()` / dirty branch of
`refresh_reusing_clean_registry_timed()`.

### 2. Use a paired collector, not a generic walker rewrite

Avoid a large generic traversal abstraction first. Instead, build a paired
collector with two independent sides:

- `RenderCollector` / existing render state:
  - `SceneContext`
  - `RenderBuildContext`
  - `FontContext`
  - moving/dynamic paint-layer policy
  - render fragment cache handling
  - text-input focused/cursor output
- `RegistryCollector` adapter:
  - `RegistryBuildAcc`
  - current revision
  - `ScrollContext` stack
  - `HoverTracker` stack
  - event `SceneContext`
  - registry subtree cache handling
  - deferred escape-nearby queue

Each node visit runs both collectors when needed. Either collector may skip or
cache-hit independently.

### 3. Extract minimal registry-builder primitives

`registry_builder` currently owns the relevant internals. Extract only the small
pieces needed by the combined path, keeping existing separate traversal intact:

- create/init a `RegistryBuildAcc` for a tree;
- visit the current element and return next scroll/hover contexts;
- decide registry child-subtree skip;
- try/merge/store registry subtree cache chunks;
- enqueue/drain deferred escape-nearby subtrees;
- finalize a `RegistryRebuildPayload`.

Keep the existing `build_registry_rebuild_cached()` implementation using these
pieces or leave it untouched until the combined path is proven.

### 4. Combined subtree behavior

At each element:

1. Resolve shared scene state once from the current element and scene context.
2. Feed that state to render context construction.
3. Feed equivalent scene context to registry collection.
4. Build own render output exactly as before.
5. Visit local nearby, children, paragraph content, scrollbars, and escape nearby
   in the same effective order as the current render and registry builders.
6. Preserve render `local` vs `escapes` ordering.
7. Preserve registry deferred escape-nearby ordering and final precedence.

Combined output shape:

```rust
struct CombinedRefreshOutput {
    scene: RenderScene,
    event_rebuild: RegistryRebuildPayload,
    text_input_focused: bool,
    text_input_cursor_area: Option<(f32, f32, f32, f32)>,
}
```

### 5. Cache and skip rules

Do not let one side's cache skip hide work required by the other side.

Cases:

- Render cache hit + registry cache hit: skip both subtree walks and merge both
  cached outputs.
- Render cache hit + registry needs work: reuse render subtree, but still run a
  registry-only walk for that subtree.
- Registry cache hit + render needs work: merge registry chunk, but still render
  subtree.
- Render culls subtree + registry needs work: return empty render subtree but
  still collect registry if registry semantics require it.
- Registry skips subtree + render needs work: skip registry work but still render.

This is the main correctness guardrail.

### 6. Enablement strategy

Phase enabling conservatively:

1. Benchmark/test-only combined path.
2. Production path behind a local internal flag or function branch only for
   dirty-registry refresh frames.
3. Keep clean-registry reuse untouched.
4. Keep a fallback to the existing separate path for unsupported cases until all
   fixture equivalence tests pass.

## Cleanup plan: single combined refresh path

### Goal

Replace the current conditional wiring with one refresh pipeline:

```text
refresh_with_registry_plan(tree, cached_rebuild)
  build_refresh_output_with_scroll_layers(tree, registry_plan)
  finalize LayoutOutput
  clear appropriate dirty flags
```

All public/internal refresh entry points should call this same function:

- `refresh`
- `refresh_timed`
- `refresh_reusing_clean_registry`
- `refresh_reusing_clean_registry_timed`
- layout-and-refresh helpers
- refresh-only helpers
- benchmark/profile helpers

The only branch should be inside a `RegistryPlan` / `RegistrySink`, not spread
through layout call sites.

### Target API shape

In `tree/layout.rs`:

```rust
enum RegistryRefreshPlan<'a> {
    Rebuild,
    ReuseClean(&'a RegistryRebuildPayload),
}

fn registry_refresh_plan<'a>(
    tree: &ElementTree,
    cached: Option<&'a RegistryRebuildPayload>,
) -> RegistryRefreshPlan<'a> {
    if let Some(cached) = cached && !tree.has_registry_refresh_damage() {
        RegistryRefreshPlan::ReuseClean(cached)
    } else {
        RegistryRefreshPlan::Rebuild
    }
}

fn refresh_with_registry_plan(
    tree: &mut ElementTree,
    plan: RegistryRefreshPlan<'_>,
) -> LayoutOutput
```

In `tree/render.rs`:

```rust
struct RefreshBuildOutput {
    scene: RenderScene,
    registry: RegistryBuildOutput,
    text_input_focused: bool,
    text_input_cursor_area: Option<(f32, f32, f32, f32)>,
}

enum RegistryBuildOutput {
    Rebuilt(RegistryRebuildPayload),
    ReusedClean,
}

fn build_refresh_output_with_scroll_layers(
    tree: &ElementTree,
    registry: RegistryBuildMode,
) -> RefreshBuildOutput
```

`RegistryBuildMode` should be a sink used by the traversal:

```rust
enum RegistryBuildMode {
    Build(RegistryRefreshCollector),
    ReuseClean,
}
```

Render traversal always receives a registry side object. In `ReuseClean`, registry
methods are no-ops during traversal. In `Build`, they collect event registry data
while render walks.

### Refactor steps

1. **Create one builder function**
   - Rename/replace `render_tree_scene_and_registry_with_scroll_layers` with
     `build_refresh_output_with_scroll_layers`.
   - Make the old render-only function a thin wrapper for tests/legacy callers,
     or remove it from production call sites.

2. **Replace `Option<RegistryBuildTraversal>` with a registry sink**
   - Current render traversal passes `Option<RegistryBuildTraversal>` through
     several functions and has many `if let Some(registry)` checks.
   - Replace with a small enum or trait-like helper whose methods no-op for
     `ReuseClean`:
     - `visit_element`
     - `should_skip_child`
     - `collect_subtree`
     - `defer_escape_subtree`
     - `child_context`
     - `local_nearby_context`
     - `escape_nearby_context`
   - This makes the render traversal code read as one combined path, not as
     render code with optional registry bolted on.

3. **Centralize registry-plan selection**
   - Remove `refresh_combined_dirty_registry*`.
   - Remove `refresh_after_layout_reusing_clean_registry*`.
   - `refresh(tree)` becomes `refresh_with_registry_plan(tree, Rebuild)`.
   - `refresh_reusing_clean_registry(tree, cached)` becomes
     `refresh_with_registry_plan(tree, registry_refresh_plan(tree, cached))`.
   - Timed variants call the same timed helper.

4. **Clean up finalization**
   - If plan is `Rebuild`, finalize the collector into a real
     `RegistryRebuildPayload` and mark `event_rebuild_changed=true`.
   - If plan is `ReuseClean`, call
     `refresh_runtime_state_in_cached_rebuild(tree, cached)` after traversal and
     set `event_rebuild_changed` based on whether runtime state changed.
   - IME text state always comes from the effective rebuild source:
     - rebuilt payload for `Rebuild`;
     - refreshed/cached payload for `ReuseClean`.

5. **Stats cleanup**
   - The current metric name `patch tree refresh render scene` is misleading once
     it includes combined render+registry traversal.
   - Update labels/plan to treat it as:
     - `patch tree refresh traversal` = combined render scene + registry walk;
     - `patch tree refresh registry post` = cached runtime refresh/finalization
       work outside traversal.
   - If preserving metric enum names is cheaper for compatibility, at least
     update log labels to reflect the new meaning.

6. **Keep clean-registry performance**
   - The unified path must not rebuild registry when cached registry is clean.
   - `ReuseClean` should avoid retained registry collection entirely, so
     refresh-only animation pulses remain close to the current retained-payload
     numbers.
   - This addresses the earlier pulse regression seen when combined traversal was
     wired too broadly.

7. **Remove old duplicated APIs after call sites converge**
   - Remove production uses of `render_tree_scene_with_scroll_layers`.
   - Keep only test/benchmark wrappers if needed.
   - Remove duplicate refresh helper functions and any separate render+registry
     finalization paths.

### Test plan for cleanup

Add/keep differential tests that call the new single builder directly:

- static tree;
- focused text input;
- slider;
- hover/click listeners;
- local nearby;
- escape nearby / `Nearby.in_front` blocker;
- paragraph inline-event-only child;
- render-cullable subtree where registry still matters;
- render fragment cache hit where registry still matters;
- clean cached registry reuse path;
- dirty registry rebuild path.

Assertions:

- scene equality with old render-only output while old path exists;
- registry equivalence with old `build_registry_rebuild*` while old path exists;
- `event_rebuild_changed` behavior for rebuild vs reuse;
- IME enabled/cursor/text state;
- dirty flag clearing behavior.

### Benchmark gate for cleanup

Run before/after cleanup:

```bash
cd native/emerge_skia
cargo bench --features bench-diagnostics --bench layout -- \
  macaw_viewport/full_viewport \
  --sample-size 30 --warm-up-time 1 --measurement-time 3
```

Must watch both patch frames and pulse frames:

- `open_patch_first_frame_one_toggle`
- `close_patch_exit_first_frame_one_toggle`
- `second_open_patch_first_frame_after_exit`
- `enter_transient_pulse_retained_payload`
- `move_x_pulse_retained_payload`
- `move_x_pulse_content_dirty_control`

Acceptance:

- patch first-frame refresh keeps the combined traversal win;
- retained pulse benchmarks do not regress beyond noise;
- target device shows lower `patch tree refresh` / `patch tree actor` normalized
  by sidepane toggle count;
- no stale paint-layer or registry churn increase.

## Implementation notes

Implemented pieces:

- `RegistryRefreshCollector` and `RegistryTraversalContext` in
  `events/registry_builder.rs` expose the minimal registry collection primitives
  needed by render traversal while keeping finalization and deferred escape-nearby
  ordering in the registry builder.
- `build_refresh_output_with_scroll_layers` in `tree/render.rs` is the single
  production render-refresh builder.
- Registry collection is represented by traversal sinks instead of layout-level
  dirty-registry branches:
  - build sink collects registry data during the render walk;
  - reuse-clean sink is no-op during traversal and refreshes cached runtime state
    after traversal.
- Render cache/cull fallbacks collect the registry subtree separately when render
  skips a subtree that registry still needs.
- Escape-nearby registry collection remains deferred and is drained after the
  local tree, preserving existing overlay precedence.
- `refresh`, `refresh_reusing_clean_registry`, timed variants, layout refresh,
  and benchmark/profile helpers now route through `refresh_with_registry_plan`.
- Added differential tests for a focused interactive tree and local/escape nearby
  tree comparing combined output with separate render + registry output.

Local profile for Macaw open patch first frame after combined traversal:

```text
macaw viewport open profile
  refresh=0.175ms
  render_scene=0.174ms  # combined render+registry traversal bucket
  registry=0.000ms
  render_visits=64
  registry_visits=37
```

Local Criterion absolute patch-frame times after the change:

```text
open_patch_first_frame_one_toggle:          ~434.8µs
close_patch_exit_first_frame_one_toggle:   ~501.7µs
second_open_patch_first_frame_after_exit:   ~460.1µs
```

Refresh-only animation pulse benchmarks use the same builder with the
reuse-clean sink. Local retained `move_x` pulse is currently around `46µs` in
Criterion after cleanup, so target-device validation should watch animation pulse
cost as well as patch-frame cost.

Validation after cleanup implementation:

- `cargo clippy -- -D warnings` passed
- `cargo test` passed (`883` native unit tests + fixture/doc tests)
- `cargo test --no-default-features --features drm` passed (`890` native unit
  tests + fixture/doc tests)
- `cargo bench --features bench-diagnostics --bench layout --no-run` passed
- `mix test` passed (`387` tests, `13` doctests)

## Implementation phases

### Phase A — prove duplicate work precisely

Add/confirm diagnostics for patch frames:

- render element visits;
- registry element visits;
- registry cache hits/stores/misses/damaged/ineligible;
- render fragment cache hits/misses where relevant;
- patch refresh total and split.

Use existing Macaw diagnostics first; add only missing counters.

### Phase B — differential test harness

Before optimizing, add a test helper:

```rust
assert_refresh_outputs_equivalent(separate, combined)
```

Compare:

- render scene exact structure if feasible, otherwise stable debug/summary plus
  targeted node-order assertions;
- registry payload via existing `assert_registry_rebuild_payloads_equivalent`;
- IME enabled/cursor/text state;
- event rebuild changed flag behavior;
- dirty flag clearing behavior.

Fixture matrix:

- plain static tree;
- scroll container with scrollbars;
- text input focused/unfocused;
- slider;
- hover/click/mouse move listeners;
- local nearby and escape nearby;
- `Nearby.in_front` blocker with and without explicit listeners;
- paragraph with nearby branches;
- `clip_nearby` host;
- viewport-cullable offscreen subtree;
- transform-only animation frame;
- layout-affecting animation frame;
- moving paint-layer eligible subtree;
- render fragment cache hit subtree;
- registry subtree cache hit subtree.

### Phase C — uncached combined traversal

Build a first combined traversal with caches disabled or ignored for registry
cache hits. This should establish ordering and semantic parity before optimizing
caches.

Acceptance for Phase C:

- combined output equals separate output on the fixture matrix;
- no behavior regression in existing event/render tests;
- local benchmark is not catastrophically slower.

### Phase D — add independent cache support

Add registry subtree-cache support and render fragment-cache support with the
independent skip rules above.

Acceptance for Phase D:

- same equivalence tests pass with warm caches;
- benchmark does not regress clean static/scroll cases;
- cache stats remain sane: no extra stale evictions or admission churn.

### Phase E — wire into dirty patch refresh

Use combined traversal only when registry refresh damage exists:

```text
if cached_rebuild.is_some() && !tree.has_registry_refresh_damage()
  existing clean-registry reuse
else
  combined dirty-registry refresh
```

Keep current separate path available as fallback.

## Benchmarks

### Local Criterion

Save a separate-pass baseline:

```bash
cd native/emerge_skia
cargo bench --features bench-diagnostics --bench layout -- \
  macaw_viewport/full_viewport \
  --sample-size 30 --warm-up-time 1 --measurement-time 3 \
  --save-baseline separate_refresh
```

After combined traversal:

```bash
cargo bench --features bench-diagnostics --bench layout -- \
  macaw_viewport/full_viewport \
  --sample-size 30 --warm-up-time 1 --measurement-time 3 \
  --baseline separate_refresh
```

Key benchmarks:

- `open_patch_first_frame_one_toggle`
- `close_patch_exit_first_frame_one_toggle`
- `second_open_patch_first_frame_after_exit`
- `enter_transient_pulse_retained_payload`
- `move_x_pulse_retained_payload`

### Diagnostic profile

```bash
EMERGE_BENCH_DIAGNOSTICS=1 cargo bench --features bench-diagnostics \
  --bench layout -- macaw_viewport/full_viewport/open_patch_first_frame_one_toggle \
  --sample-size 10 --warm-up-time 1 --measurement-time 1
```

Expected local diagnostic change:

- `refresh render scene + registry` should become one combined bucket;
- total patch refresh should drop if duplicate traversal overhead was material;
- render/registry visit counts should show fewer separate full-tree walks.

### Target device

Compare normalized by sidepane toggle count:

- `patch tree actor`
- `patch tree refresh`
- combined refresh sub-buckets
- `patch tree layout`
- frames per open/close animation
- `stale_evictions`
- paint-layer hit/resident bytes

Expected device win is bounded. The best possible result is not
`3.34ms + 3.23ms -> 3.34ms`; both collectors still do per-node work. A realistic
first target is reducing patch refresh from `~6.6ms` to `~4-5ms`.

## Risks

- Event precedence changes, especially escape-nearby overlays and in-front
  blockers.
- Hover duplicate/ownership regressions if registry update ordering changes.
- Text input/focus regressions if render IME state and registry focus state drift.
- Incorrect culling if render and registry skip rules are conflated.
- Cache staleness if registry subtree cache keys or render fragment keys are
  reused under mismatched contexts.
- Code complexity regressions if the combined walker duplicates large parts of
  both existing traversals.
- Local benchmarks may understate device wins or losses; keep target-device
  validation mandatory.

## Acceptance criteria

- All existing tests pass:
  - `cargo test`
  - `cargo test --no-default-features --features drm`
  - `cargo bench --features bench-diagnostics --bench layout --no-run`
  - `mix test`
- Differential tests prove combined output equals separate output across the
  fixture matrix.
- Local Macaw patch benchmarks show a statistically significant improvement or
  no regression with clear device-facing rationale.
- Target device shows lower `patch tree refresh` and/or `patch tree actor` when
  normalized by sidepane toggle count.
- No increase in stale paint-layer evictions or registry/render cache churn.

## Rollback plan

Keep the existing separate refresh path callable. If combined traversal fails a
fixture, causes target-device freezes, or does not improve patch refresh, revert
wiring to the separate path and keep only useful diagnostics/tests.
