# Combined tree-walk cleanup pass

## Status

Implemented in the current refresh traversal work series.

Implementation notes:

- Moved registry traversal sink plumbing into `native/emerge_skia/src/tree/render/registry_walk.rs`.
- Added `RenderTraversal` derivation helpers and shared scene-context helpers.
- Renamed render-skip registry fallback to `collect_registry_for_render_skipped_subtree`.
- Kept the historical registry subtree cache as test/bench-only builder code and documented the retained cache structs.
- Renamed timing fields/metrics to `refresh_traversal` and `refresh_registry_post`.
- Added an Interaction-style top/right rounded scroll-panel regression test.

## Goal

Clean up the combined render/registry refresh traversal without changing
behavior or app-visible semantics.

The previous work proved the right architecture direction: one refresh builder
can walk the retained tree once and either collect registry data during the walk
or no-op the registry side for clean cached-registry refreshes. The current code
works, but the combined walk is still too hard to read and maintain.

This pass should make the combined walk boring and explicit:

- one refresh builder remains the only production path;
- registry collection/no-op behavior remains a traversal sink, not layout-level
  branching;
- render traversal code should not be cluttered with registry sink type plumbing;
- duplicated child/nearby traversal setup should be factored into named helpers;
- stale/dead registry-cache scaffolding should be intentionally handled rather
  than hidden with broad `allow(dead_code)` attributes;
- timing names should match the new model (`traversal` and `registry_post`).

## Current evidence

Target-device comparison normalized by the same `9` sidepane patch cycles:

```text
initial combined, before full unifying:
  patch tree actor:                 18.059ms
  patch tree refresh:                6.837ms
  patch tree refresh render scene:   6.699ms
  patch tree refresh registry:       0.000ms
  frames: 50

unified combined path:
  patch tree actor:                 17.419ms
  patch tree refresh:                6.623ms
  patch tree refresh traversal:      6.491ms
  patch tree refresh registry post:  0.122ms
  frames: 54
```

Interpretation:

- unified path is not the bottleneck regression; patch actor and refresh improved
  slightly on the device;
- the new `registry_post` cost is real but small compared with traversal;
- the remaining large cost is the combined traversal itself (`~6.5ms` on target);
- cleanup should be behavior/perf neutral first, then make later traversal
  optimization easier.

Local checks after unification:

- Macaw patch open diagnostic: `refresh~0.175ms`, traversal `~0.174ms`, registry
  post `~0.000ms` locally.
- Retained `move_x` pulse benchmark is around `46µs` locally after cleanup work;
  this benchmark must be monitored because clean-registry/no-op traversal is a
  hot path.

## Investigation findings

### 1. Combined traversal types live in the middle of `render.rs`

`render.rs` currently contains:

- `RegistryTraversalSink`
- `HostRegistryTraversalSink`
- `BuildRegistryTraversal`
- `BuildHostRegistryTraversal`
- `BuildRegistryBranch`
- `BuildHostRegistryBranch`
- `ReuseCleanRegistryTraversal`
- `ReuseCleanHostRegistryTraversal`

This makes the top of the render module harder to scan. The render walk itself
is now conceptually simple, but its sink plumbing occupies a lot of visual space
and leaks implementation details such as branch skip enums.

### 2. Registry skip/no-op semantics are correct but visually noisy

Current behavior is right:

- build sink visits elements and collects registry output;
- reuse-clean sink is no-op during traversal;
- render cull/cache hit still calls `collect_subtree` so registry can rebuild a
  subtree when render skipped it;
- escape nearby registry work is deferred and drained after the local tree.

But the code expresses this through generic associated types plus branch enums.
That is acceptable for performance, but it should be isolated and documented so
render traversal reads like render traversal.

### 3. Render child traversal setup is duplicated

Several branches manually rebuild similar `RenderTraversal` values:

- local nearby under paragraph;
- local nearby under non-paragraph;
- normal child;
- paragraph child;
- escape nearby;
- nearby root recursion.

Each duplicate block is a chance for render context, scene context, registry
context, paint-layer policy, or culling policy to drift.

### 4. Render scene context and registry scene context are parallel but separate

The render side and registry side both derive scene context from the same
`ResolvedNodeState` and `RetainedPaintPhase`. Today this is done in nearby helper
functions and registry sink helpers separately.

A cleanup pass should make phase derivation explicit and shared where possible,
so future changes to nearby/overlay semantics do not need to be applied in two
places.

### 5. Old cached-registry subtree code is now dead in production

The old `build_registry_rebuild_cached` path and registry subtree-cache helpers
are no longer used by production refresh. Some were silenced with
`allow(dead_code)` after unification.

That is a smell. We need an explicit decision:

- either intentionally retire/remove the old cached-registry subtree path, or
- intentionally adapt it into the combined collector.

For this cleanup pass, avoid mixing a new cache algorithm into the readability
cleanup. Decide and document the cache path, then make code warnings honest.

### 6. Timing names are halfway migrated

Renderer stats labels now say:

- `patch tree refresh traversal`
- `patch tree refresh registry post`

But internal struct/enum names still use `refresh_render_scene` and
`refresh_registry` in several places. This creates friction when reading stats
and code together.

## Known visual regression guardrail

While planning this cleanup, a separate visual regression was reported in
`../emerge_demo`:

- Showcase Borders page has some elements scaled too small/too large and clipped.
- Showcase Interaction page again clips top/right rounded corners.

This cleanup pass must not mask or explain away those bugs. Before changing the
walk further, capture targeted regression coverage around the Interaction clipped
corner case and at least one Borders scaling/clipping fixture. If the cleanup
changes rendering output for those fixtures, stop and fix the rendering bug as a
separate slice.

Likely relevant areas to inspect before implementation:

- `native/emerge_skia/src/tree/render.rs` clip/transform wrapping order;
- moving paint-layer eligibility and placement for active transform animation;
- root/nearby/dynamic paint-layer wrapping and ancestor clip propagation;
- `native/emerge_skia/src/tree/render/tests/pipeline.rs` existing corner/clip
  coverage;
- `native/emerge_skia/src/tree/render/tests/paint.rs` border/rounded/scale
  coverage;
- demo fixtures in `native/emerge_skia/benches/layout.rs` that model Showcase
  Interaction/Borders.

## Non-goals for this cleanup pass

- Do not change application code or animation specs.
- Do not change layout, measure, resolve, or patch semantics.
- Do not change event precedence or nearby overlay ordering.
- Do not introduce a new registry subtree-cache algorithm in the same pass.
- Do not optimize the remaining `~6.5ms` traversal cost yet; first make the walk
  easy to reason about.
- Do not remove the test-only direct/render-only baseline helpers used for
  equivalence tests and renderer-cache tests.

## Proposed cleanup design

### A. Extract registry traversal sinks out of `render.rs`

Create a focused private module, for example:

```text
native/emerge_skia/src/tree/render/registry_walk.rs
```

Move these there:

- sink traits;
- build/no-op sink structs;
- branch skip structs/enums;
- `registry_scene_context_for_phase`;
- branch context helpers.

Render module should import a small surface:

```rust
use self::registry_walk::{
    BuildRegistryTraversal,
    RegistryTraversalSink,
    ReuseCleanRegistryTraversal,
};
```

Acceptance for this phase:

- no behavior changes;
- no benchmark changes beyond noise;
- `render.rs` top-level scan is significantly shorter;
- unavoidable `large_enum_variant` allowances, if any, are confined to the sink
  module and documented there.

### B. Rename sink types around behavior, not implementation detail

Preferred naming:

```rust
BuildRegistryTraversal      // active registry collection
ReuseCleanRegistryTraversal // no retained registry collection
BuildHostRegistryTraversal
ReuseCleanHostRegistryTraversal
```

If branch skip enums remain, document them as render-needed/registry-skipped
subtree adapters:

```rust
BuildRegistryBranch::Collect(...)
BuildRegistryBranch::SkipRegistry
```

Avoid bare `Skip`, because it can be misread as skipping render. It only skips
registry collection while render continues.

### C. Introduce render traversal derivation helpers

Add helper methods on `RenderTraversal` or small free functions:

```rust
impl<'a> RenderTraversal<'a> {
    fn for_host_content(
        &self,
        render_ctx: &'a RenderBuildContext,
        allow_moving_paint_layers: bool,
        disable_viewport_culling: bool,
        inside_dynamic_paint_layer: bool,
    ) -> Self

    fn for_local_nearby(&self, child_ctx: &'a RenderBuildContext) -> Self
    fn for_child(&self, scene_ctx: SceneContext, child_ctx: &'a RenderBuildContext) -> Self
    fn for_escape_nearby(&self, child_ctx: &'a RenderBuildContext) -> Self
    fn for_nearby_root(&self, scene_ctx: SceneContext) -> Self
}
```

The exact API can be simpler, but the result should remove repeated literal
`RenderTraversal { ... }` blocks from:

- `build_element_subtree`;
- `build_host_content_subtree`;
- `build_nearby_mount_subtree`;
- `build_paragraph_subtree`.

Acceptance:

- fewer repeated render traversal literals;
- no change in scene output or registry output;
- no new lifetime complexity that makes code harder to read.

### D. Add shared scene-phase helpers

Make render and registry phase derivation use the same named operation when a
child/nearby/escape phase is selected.

Candidate helpers:

```rust
fn scene_context_for_phase(
    scene_state: Option<&ResolvedNodeState>,
    phase: RetainedPaintPhase,
) -> SceneContext

fn children_scene_context(scene_state: Option<&ResolvedNodeState>) -> SceneContext
fn behind_content_scene_context(scene_state: Option<&ResolvedNodeState>) -> SceneContext
fn overlay_scene_context(scene_state: Option<&ResolvedNodeState>, slot: NearbySlot) -> SceneContext
```

This should replace the current mix of:

- inline `scene_state.clone().map(|state| next_scene_context(...))`;
- registry-specific `registry_scene_context_for_phase`;
- paragraph-specific `paragraph_children_scene_context`.

Acceptance:

- phase logic has one home;
- tests covering local nearby, escape nearby, and paragraph inline-event-only
  children still pass.

### E. Clarify registry subtree fallback semantics

Render skip/cache hit currently calls `registry.collect_subtree(tree, ix)`. This
is correct but too implicit.

Rename method or wrap usage so intent is obvious:

```rust
registry.collect_registry_for_render_skipped_subtree(tree, ix)
```

or at least document `collect_subtree` in the sink trait:

> Called when render culls/reuses a subtree but registry collection still needs
> to consider it. No-op for clean registry reuse and for registry-skipped
> branches.

Acceptance:

- future maintainers do not confuse render skip with registry skip;
- render cache/cull behavior remains equivalent.

### F. Decide old registry subtree-cache fate

Do not leave broad dead-code allowances as the final state.

Decision options:

1. **Retire old cached-registry rebuild path**
   - Remove or `cfg(test, feature = "bench-diagnostics")` old cached rebuild
     helpers and tests that only prove the old path.
   - Remove `registry_cache` storage from production state only if this does not
     create excessive churn; otherwise mark it as intentionally retained for a
     later cache-adaptation slice with a narrow comment.

2. **Keep as benchmark-only historical baseline**
   - Move old cached-rebuild API behind test/bench cfg.
   - Narrow dead-code allowances to cfg-only code.
   - Document that production combined traversal no longer uses subtree registry
     cache.

3. **Adapt cache into combined traversal**
   - Not recommended for this cleanup pass because it changes behavior/perf.
   - If selected later, it needs its own plan and differential tests.

Recommended for this cleanup pass: option 2, unless removing the old path is
small after inspection. The goal is to stop pretending the old cache is live
production code.

### G. Rename timing code to match stats labels

Update internal names where practical:

```rust
RefreshTiming {
    traversal: Duration,
    registry_post: Duration,
}

LayoutUpdateTiming {
    refresh_traversal: Duration,
    refresh_registry_post: Duration,
}
```

Also consider renaming enum variants:

```rust
PatchTreeRefreshTraversal
PatchTreeRefreshRegistryPost
```

If public stats schema compatibility argues against enum variant rename, at
least keep log labels and recorder method names accurate.

Acceptance:

- stats logs, code field names, and plan language agree;
- no schema-breaking surprise for Elixir stats consumers unless intentionally
  versioned.

## Test plan

### Differential tests to keep/extend

Keep existing tests:

- focused interactive tree;
- local + escape nearby tree.

Add or verify coverage for:

- Showcase Interaction clipped top/right rounded-corner regression;
- Showcase Borders scaled/clipped recipe regression or a reduced fixture that
  reproduces the same clip/scale failure;
- clean-registry reuse path: output has `event_rebuild_changed=false` unless
  runtime text/slider state changed;
- render-cullable subtree where registry still matters;
- render fragment cache hit where registry still matters;
- paragraph with inline-event-only child;
- text input IME state;
- slider state;
- `Nearby.in_front` blocker without explicit listeners.

### Commands

```bash
cd native/emerge_skia
cargo clippy -- -D warnings
cargo test
cargo test --no-default-features --features drm
cargo bench --features bench-diagnostics --bench layout --no-run
cd /workspace/emerge && mix test
```

## Benchmark plan

Run the Macaw group before and after the cleanup:

```bash
cd native/emerge_skia
cargo bench --features bench-diagnostics --bench layout -- \
  macaw_viewport/full_viewport \
  --sample-size 30 --warm-up-time 1 --measurement-time 3
```

Watch:

- `open_patch_first_frame_one_toggle`
- `close_patch_exit_first_frame_one_toggle`
- `second_open_patch_first_frame_after_exit`
- `enter_transient_pulse_retained_payload`
- `move_x_pulse_retained_payload`
- `move_x_pulse_content_dirty_control`

Diagnostic command:

```bash
EMERGE_BENCH_DIAGNOSTICS=1 cargo bench --features bench-diagnostics \
  --bench layout -- macaw_viewport/full_viewport/open_patch_first_frame_one_toggle \
  --sample-size 10 --warm-up-time 1 --measurement-time 1
```

Expected cleanup result:

- patch open profile remains one traversal bucket;
- retained pulse benchmarks remain within noise;
- no increase in registry post time;
- no increase in stale paint-layer evictions.

## Target-device validation

After local validation, compare against the latest unified device run:

```text
patch tree actor:                 17.419ms
patch tree refresh:                6.623ms
patch tree refresh traversal:      6.491ms
patch tree refresh registry post:  0.122ms
frames: 54 over 9 patch cycles
```

Acceptance on target:

- patch tree actor does not regress by more than noise;
- patch tree refresh traversal does not regress;
- registry post remains small;
- frames per sidepane toggle do not drop;
- no cache churn increase (`stale_evictions`, paint-layer hits/resident bytes).

## Rollback plan

This pass should be mostly mechanical. If benchmarks or tests regress:

1. keep the single `refresh_with_registry_plan` layout path;
2. revert only the extraction/helper refactors causing the regression;
3. do not return to layout-level dirty-registry conditional wiring.
