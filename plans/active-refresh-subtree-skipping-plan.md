# Active Plan: Refresh Subtree Skipping

Last updated: 2026-04-26.

Status: partially implemented. Refresh damage bookkeeping, clean-registry reuse,
render subtree caching/skipping, and the render-cache performance regression
slice are implemented. The next slice is registry subtree chunk caching/skipping.

## Motivation

Layout work is now skipped for paint-only updates, and layout-affecting changes
can reuse retained measurement/resolve state. The downstream refresh path is now
the next obvious cost center: `refresh(tree)` still rebuilds the full render
scene and full event registry even when only one small subtree changed.

Goal: make refresh work proportional to render/registry damage while preserving
Emerge's explicit pipeline:

1. prepare effective attrs
2. measure intrinsic sizes bottom-up
3. resolve geometry top-down
4. refresh render scene/event registry

Paint-only updates should continue to ask no layout question. This plan only
optimizes step 4.

## Current code shape

Relevant files:

- `native/emerge_skia/src/tree/layout.rs`
  - `refresh(...)`
  - `refresh_prepared_default(...)`
  - `layout_and_refresh_prepared_default(...)`
  - `LayoutOutput`
- `native/emerge_skia/src/tree/render.rs`
  - `render_tree(...)`
  - `build_element_subtree(...)`
  - `build_host_content_subtree(...)`
  - `build_nearby_mount_subtree(...)`
  - `RenderSubtree`
- `native/emerge_skia/src/events/registry_builder.rs`
  - `build_registry_rebuild(...)`
  - `accumulate_subtree_rebuild(...)`
  - `finalize_registry_rebuild(...)`
- `native/emerge_skia/src/runtime/tree_actor.rs`
  - `RefreshDecision::RefreshOnly`
  - `publish_layout_output(...)`
  - cached full `RegistryRebuildPayload`
- `native/emerge_skia/src/tree/element.rs`
  - node runtime/layout state and topology versions
- `native/emerge_skia/src/tree/invalidation.rs`
  - origin-agnostic update classification

Current refresh behavior:

```text
refresh(tree)
  -> render_tree_scene_cached(tree)
       -> reuse clean render subtrees when cheap/safe
       -> bypass render subtree lookup in volatile scrolling scene contexts
       -> do not store dirty scroll-container render caches
       -> build_registry_rebuild(tree)
            -> full event-registry traversal unless clean-registry reuse is used
```

The render scene and registry traversals are separate today. Treat them as
separate cache/skip problems even if they share damage metadata. Render subtree
skipping is implemented; registry subtree chunk skipping remains next.

## Target behavior

A clean subtree can be skipped during refresh when all dependencies relevant to
that refresh product are unchanged:

- geometry / scene context did not change
- paint-relevant attrs and runtime render state did not change
- registry-relevant attrs and runtime interaction state did not change
- child / paint-child / nearby topology relevant to traversal did not change
- descendants have no relevant refresh damage

Expected outcomes:

```text
paint-only update to one node
  -> layout_ms_count=0
  -> rebuild render output only along affected paint path
  -> reuse cached registry output when registry state is unchanged

registry-only update to one node
  -> layout_ms_count=0
  -> preserve render scene when paint/geometry is unchanged
  -> rebuild or reuse only affected registry chunks

geometry/layout update to one node
  -> layout may run as before
  -> refresh skips unaffected sibling subtrees after geometry settles
```

Scheduling must remain origin-agnostic. The refresh decision should depend on
combined invalidation/damage, not whether the update came from animation, patch,
scroll, hover, focus, or another source.

## Proposed model

Introduce refresh-specific retained state separate from layout-cache outcomes.
Exact names can change during implementation, but keep the concepts distinct.

Possible per-node state:

```rust
struct NodeRefreshState {
    render_dirty: bool,
    render_descendant_dirty: bool,
    registry_dirty: bool,
    registry_descendant_dirty: bool,
    render_cache: Option<RenderSubtreeCache>,
    registry_cache: Option<RegistrySubtreeCache>,
}
```

This state should not change layout-cache hit/miss/store semantics. It belongs
to downstream refresh, not measurement/resolve layout caching.

Potential dependency keys:

```rust
struct RenderSubtreeKey {
    kind: ElementKind,
    render_attrs: RenderAttrsKey,
    runtime_render: RuntimeRenderKey,
    frame: Option<Frame>,
    inherited_font: InheritedMeasureFontKey,
    scene_context: SceneContextKey,
    render_context: RenderContextKey,
    topology: RenderTopologyKey,
}

struct RegistrySubtreeKey {
    kind: ElementKind,
    registry_attrs: RegistryAttrsKey,
    runtime_registry: RuntimeRegistryKey,
    frame: Option<Frame>,
    scene_context: SceneContextKey,
    scroll_context: ScrollContextKey,
    topology: RegistryTopologyKey,
}
```

Correctness first: it is acceptable for early keys to be conservative and miss
more often than ideal. Do not add bypass taxonomies. If a cache/skip decision is
not safe, make the key/damage dependency more precise or rebuild.

Performance guardrail: correctness-first does not allow expensive broad key
construction in hot refresh paths. Mature engines generally use typed cache
inputs, dirty/damage flags, relayout/repaint boundaries, or property dependency
versions rather than debug-string render keys. Prefer damage/version checks that
make clean/dirty decisions cheap. Avoid `Debug`/string formatting, repeated broad
hashing, and cloning large retained layout/cache state while deciding whether to
reuse refresh output.

Cross-engine design notes to preserve:

- Taffy/Yoga: cache layout from small typed constraint inputs; dirty nodes drive
  recomputation.
- Servo: explicit damage flags control ancestor/descendant propagation.
- Flutter: layout, paint, compositing, and semantics dirtiness are separate;
  repaint/relayout boundaries stop propagation when dependencies allow it.
- Slint: dependency-tracked dirty properties request redraw lazily.
- Iced: retained widget/layout state is reused through explicit invalidation
  flags, not broad per-node render snapshots.

## Slice 1: refresh damage bookkeeping without behavior change — done

Goal: track render/registry dirtiness and descendant traversal dirtiness without
reusing cached refresh output yet.

Tasks:

- add refresh dirty/descendant dirty state to retained nodes
- add helpers for marking render and registry dirty paths
- mark damage from existing invalidation sources:
  - paint attrs -> render dirty
  - registry attrs -> registry dirty
  - measure/resolve/structure -> render + registry dirty conservatively
  - scroll changes -> render + registry dirty for affected scene/context paths
  - focus/hover/text-input runtime changes -> classify to render/registry damage
- clear refresh dirty bits only after a successful refresh
- keep `render_tree(...)` and `build_registry_rebuild(...)` behavior unchanged
- add tests that dirty bits propagate and clear as expected

Acceptance:

- no behavior change in render output or event matching
- paint-only refresh still skips layout
- dirty propagation is origin-agnostic
- existing Rust and Elixir tests pass

## Slice 2: reuse cached full registry payload when registry damage is clean — done

Goal: avoid rebuilding/sending a new full registry payload for paint-only refresh
when registry state did not change.

Rationale: the tree actor already keeps a cached full `RegistryRebuildPayload`.
Use that before implementing fine-grained registry subtree chunks.

Tasks:

- teach refresh output/publication whether registry output changed
- when render changed but registry damage is clean, reuse the cached rebuild
  instead of rebuilding the registry traversal
- avoid sending duplicate registry updates when the cached rebuild is unchanged
- keep render scene publication behavior unchanged
- add tests for paint-only patch/animation refresh that verify event registry
  remains valid without a rebuild

Acceptance:

- paint-only refresh can publish a new scene without rebuilding registry output
- explicit registry requests still use cached rebuild or rebuild as today
- no event hit-test/focus/text-input regressions

## Slice 3: render subtree cache/skip — done

Goal: reuse retained render subtrees when render dependencies are unchanged.

Tasks:

- make `RenderSubtree` cloneable and cacheable per node
- define a conservative `RenderSubtreeKey`
- include enough dependencies for correctness:
  - effective render attrs
  - runtime render state used by text input/multiline rendering
  - frame and resolved scene context
  - inherited font context
  - clip/transform context needed by wrapping nodes
  - child, paint-child, and nearby topology versions/counts
  - paragraph fragment dependency for paragraph rendering
- in `build_element_subtree(...)`, reuse cached subtree only when:
  - the key matches
  - the node has no render dirty bit
  - no descendant render dirty bit is set
- rebuild and store on miss/dirty
- keep escape-nearby behavior exact by caching both local and escape vectors
- add golden-style scene trace/pixel tests for skipped vs rebuilt output

Acceptance:

- cached and uncached render scenes are identical in focused tests
- paint-only updates rebuild only affected paths and ancestors needed to assemble
  ordering/wrapping
- scroll, transforms, clips, nearby escape overlays, alpha, paragraph fragments,
  text input cursor/selection/preedit, image/video primitives, and shadows do
  not produce stale output

Implemented shape:

- `NodeRefreshState` owns an optional retained render subtree cache
- render caches store local and escape render nodes plus text-input IME metadata
- render keys conservatively include render-relevant effective attrs, runtime
  render state, frame/scroll state, inherited font context, scene/render context,
  child/paint-child/nearby topology versions/counts, and paragraph fragments
- `refresh(tree)` and clean-registry refresh use cached scene rendering
- a subtree cache is reused only when the render key matches and the node has no
  render or descendant render damage
- uncached test rendering remains available for equality checks

Focused tests cover cached-vs-uncached scene equality after a sibling paint
patch, registry-only root refresh preserving render cache, clean registry reuse,
and transform paint changes rebuilding registry output.

## Slice 4: render subtree cache performance/redesign — done

Goal: keep render refresh correctness while removing the long max refresh,
layout, and patch-tree-actor timings observed after render subtree caching.

Problem statement:

- Renderer stats after render subtree caching showed refresh and patch actor max
  timings around tens of milliseconds.
- Native `layout` timing includes layout plus refresh-output generation, so
  expensive refresh cache work can appear as both `layout` and `refresh` spikes.
- The likely costs are broad render-key construction, `Debug`/string/hash work,
  `Element::render_snapshot(...)` cloning retained layout cache entries, and
  cloning/assembling cached `Vec<RenderNode>` subtrees.
- Registry subtree chunk work should not proceed while render cache overhead is
  unresolved.

Implemented mitigation:

- Replaced string fields in `RenderSubtreeKey` with compact hash fields.
- Streamed debug output into a hasher instead of allocating joined key strings.
- Avoided cloning measurement/resolve layout cache entries in
  `Element::render_snapshot(...)`.
- Changed cache lookup so dirty render paths rebuild first instead of building a
  lookup key before the rebuild.
- Added a safe uncached refresh baseline for tests and benchmarks.
- Added native regression benchmarks comparing cached and uncached refresh paths.
- Bypassed render subtree cache lookup under scrolling scene offsets and avoided
  storing large render caches on scroll containers during dirty refreshes. This
  prevents scroll-only frames from cloning large, immediately-stale scene
  subtrees.
- Avoided seeding render subtree caches on dirty/full rebuilds. Render caches are
  now a lazy clean-refresh optimization instead of extra work during app switch
  or full upload.
- If a damaged refresh has no retained render caches yet, use the safe uncached
  scene renderer instead of walking the cache path.
- Added a small per-refresh cache-store budget plus a render-node-count cap for
  stored subtrees so cache storage cannot clone many large scene chunks in one
  frame.
- Bypassed the cache path entirely for nodes with their own render dirty bit;
  descendant-dirty ancestors can still use existing cached clean siblings.

This is still a conservative design. Future work should replace broad debug-hash
fields with typed versions where profiles justify it.

Tasks:

1. Add a dedicated regression guard before accepting the performance fix:
   - a Criterion or Benchee benchmark that measures refresh-producing work, not
     layout-only work
   - include at least `paint_rich`, `nearby_rich`, and animated-shadow cases
   - cover cold full layout+refresh after upload/switch, warm cached refresh,
     first refresh after `paint_attr`, first refresh after `nearby_slot_change`,
     and patch-plus-refresh/actor-like paths where practical
   - compare the production cached path against a safe uncached or
     cache-disabled path so the benchmark can prove the optimization is a win
     rather than just measuring the current implementation
   - keep benchmark execution opt-in; normal `cargo test` / `mix test` must not
     run timing benchmarks
2. Add deterministic non-timing regression tests for the known failure modes
   where possible:
   - cached and uncached render scenes remain equal
   - render snapshots do not clone retained measurement/resolve layout caches
   - dirty render paths can avoid broad key construction before rebuilding
   - paint-only refresh still reports no layout/cache activity
3. Validate the current mitigation against the new regression guard, focused
   retained-layout benchmarks, and renderer stats for paint-heavy and
   nearby-heavy scenarios.
4. If spikes remain, gate or disable production render subtree caching while
   keeping refresh dirty bookkeeping and full-registry reuse enabled.
5. Move cache decisions toward cheap damage/version checks:
   - test `render_dirty || render_descendant_dirty` before constructing any
     render key
   - use topology versions/counts directly
   - introduce narrow render dependency versions where needed for attrs,
     runtime render state, paragraph fragments, frame/scroll state, inherited
     font context, and scene/render context
   - avoid `Debug`, string formatting, or broad hashing in normal refresh
     traversal
6. Reassess whether caching whole `Vec<RenderNode>` subtrees is worthwhile:
   - measure clone/extend cost
   - prefer explicit repaint/render boundaries or retained scene chunks if full
     subtree vector cloning dominates
   - consider caching only expensive leaf products first, such as paragraph text
     fragments, images, or shadows, if broad subtree caching is not profitable
7. Keep refresh diagnostics separate from layout-cache stats. If temporary or
   permanent counters are needed, add them behind the unified gated `stats` path
   only.
8. Preserve correctness tests comparing cached and uncached output while changing
   the implementation.

Concrete guard shape:

- Add a native benchmark group named something like
  `native/render_refresh_cache_regression`.
- Prefer a separate focused bench file if `layout.rs` becomes too broad; either
  way it must compile under `cargo test --benches --no-run`.
- Suggested cases:
  - `paint_rich_500/paint_attr`
  - `nearby_rich_500/paint_attr`
  - `nearby_rich_500/nearby_slot_change`
  - `layout_matrix_500/paint_attr`
  - `animated_shadow_showcase/paint_only_refresh_each_frame`
  - `scroll_shadow_showcase/paint_only_refresh_scroll_frame`
- Suggested measured variants:
  - `cold_cached_layout_refresh`: decode/upload-like cold tree, layout, and
    refresh through production cached path
  - `cold_uncached_layout_refresh`: same cold tree with render subtree cache
    bypassed
  - `cached_refresh`: warmed tree, render cache seeded, call refresh-producing
    path
  - `uncached_refresh` or `cache_disabled_refresh`: same tree/update with render
    subtree cache bypassed
  - `patch_cached_refresh`: apply patch then run the actor-equivalent
    refresh/layout decision
  - `patch_uncached_refresh`: same patch path with render subtree cache bypassed
- Suggested deterministic tests:
  - `render_snapshot_omits_layout_cache_entries`
  - `dirty_render_refresh_does_not_build_render_key_before_rebuild` if a
    test-only key-build counter is added
  - `paint_only_patch_refresh_has_zero_layout_stats`
- Stable command to document with the implementation:

  ```bash
  cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench layout -- render_refresh_cache_regression
  ```

Regression guard acceptance:

- The benchmark has a stable, documented command and produces grep-friendly case
  names/output.
- The benchmark measures the regression surface that was observed in renderer
  stats: refresh and patch-plus-refresh work, including max/long-tail behavior
  where the harness can report it.
- The benchmark includes a safe baseline (`uncached`, `cache_disabled`, or last
  known-good commit comparison) so it can validate both improvement and
  regression.
- Timing assertions are not part of normal tests; CI can compile the benchmark
  with `cargo test --benches --no-run` and humans/benchmark jobs can run it.
- Deterministic tests cover correctness and structural failure modes that are
  not timing-sensitive.

Implementation acceptance:

- Paint-only updates still skip layout entirely:
  - `layout_ms_count=0`
  - layout-cache hit/miss/store counters remain zero
- Cached and uncached render scenes remain identical in focused tests.
- Focused benchmark/renderer-stats smoke no longer shows the render-cache slice
  causing long refresh or patch-tree-actor max timings.
- Render cache decisions do not allocate debug strings or clone layout cache
  entries in hot refresh paths.
- If acceptable performance cannot be reached quickly, production render subtree
  caching is gated/disabled and the active plan returns to registry/full-refresh
  improvements from a safe baseline.

## Slice 5: registry subtree chunk cache/skip — next

Goal: make event registry rebuild proportional to registry damage, not tree size.

Tasks:

- factor registry traversal into cacheable per-subtree chunks before final window
  and focus-cycle listeners are added
- define a conservative `RegistrySubtreeKey`
- include dependencies used by listener construction:
  - event/focus/scrollbar/text-input attrs
  - runtime interaction state that affects listeners or retained metadata
  - frame, scene context, interaction transform, and scroll contexts
  - child/paint-child/nearby topology that affects precedence/focus order
- cache local registry contributions plus retained metadata needed by
  `finalize_registry_rebuild(...)`
- merge cached chunks in the same precedence order as the current full traversal
- preserve focused text input, focus-on-mount, scrollbars, hover/press ordering,
  overlay precedence, and window listeners
- add targeted registry tests for nested scroll, nearby overlays, focus order,
  text input, and scrollbar hit areas

Acceptance:

- registry output matches full rebuild for all focused tests
- registry-only updates avoid rebuilding unrelated sibling chunks
- event dispatch precedence remains unchanged

## Slice 6: stats and benchmark smoke

Goal: validate that refresh work decreases without complicating layout-cache
stats.

Guidelines:

- do not add layout-cache bypass counters
- keep layout-cache stats as hit / miss / store only
- if refresh counters are needed, keep them separate, gated/default-off, and
  exposed through the unified `stats` path only
- grep-friendly benchmark output is fine, but normal `cargo test` / `mix test`
  must not run benchmarks

Potential refresh counters:

- render subtrees visited
- render subtrees skipped
- registry subtrees visited
- registry subtrees skipped

Focused benchmark smoke:

```bash
EMERGE_BENCH_SCENARIOS=layout_matrix,text_rich,nearby_rich \
EMERGE_BENCH_SIZES=50 \
EMERGE_BENCH_MUTATIONS=paint_attr,event_attr,layout_attr,nearby_slot_change \
EMERGE_BENCH_WARMUP=0.1 \
EMERGE_BENCH_TIME=0.1 \
EMERGE_BENCH_MEMORY_TIME=0 \
mix bench.native.retained_layout
```

For paint-only animation, keep the existing expected shape:

```text
layout_ms_count=0
layout_cache_*_hits=0
layout_cache_*_misses=0
layout_cache_*_stores=0
```

Refresh counters, if added, should show skipped render/registry subtrees instead
of layout-cache activity.

## Suggested tests

Add focused tests near existing render/registry/layout cache tests:

- paint-only patch refresh keeps layout skipped and reuses unaffected render
  subtrees
- paint-only animation refresh keeps layout skipped and reuses unaffected render
  subtrees
- registry-only patch reuses render scene and updates only registry output
- event registry remains correct after render subtree skip
- hover/focus/text-input runtime changes mark only relevant refresh damage
- scroll changes invalidate descendants whose scene/interaction context changes
- nearby overlay slot/order changes invalidate render and registry precedence
- paragraph fragment shifts do not use stale rendered text
- transformed/clipped/alpha subtrees match full rebuild output
- cached and uncached refresh output compare equal for representative trees

## Non-goals

- Do not rework measurement or resolve layout caches in this slice.
- Do not add cache-bypass counters.
- Do not add per-stat or per-cache NIFs.
- Do not make scheduling depend on update source.
- Do not push render/registry output into layout-cache entries.
- Do not attempt viewport/repeater-aware caching here.
- Do not broaden relayout/dependency boundaries unless a tiny helper is required
  and separately tested.

## Validation

Implemented notes so far:

- `Element` now owns refresh-specific dirty/descendant-dirty state separate from
  layout cache state.
- Existing patch/runtime/scroll/animation sources mark render and/or registry
  refresh damage according to the changed dependency.
- Decorative paint changes can reuse the cached full registry payload.
- Registry-relevant paint changes such as transforms still rebuild registry
  output.
- The tree actor no longer sends duplicate registry updates when refresh reuses
  the cached registry payload.

Before committing implementation slices, run:

```bash
cargo fmt --manifest-path native/emerge_skia/Cargo.toml --check
mix format --check-formatted
git diff --check
cargo test --manifest-path native/emerge_skia/Cargo.toml
mix test
cargo test --manifest-path native/emerge_skia/Cargo.toml --benches --no-run
```

Validation status for implemented slices 1–4:

- `cargo fmt --manifest-path native/emerge_skia/Cargo.toml --check`
- `mix format --check-formatted`
- `git diff --check`
- `cargo test --manifest-path native/emerge_skia/Cargo.toml`
- `mix test`
- `cargo test --manifest-path native/emerge_skia/Cargo.toml --benches --no-run`
- focused retained-layout benchmark smoke with `layout_matrix`, `text_rich`, and
  `nearby_rich` for `paint_attr`, `event_attr`, `layout_attr`, and
  `nearby_slot_change`
- focused render-refresh regression benchmark smoke:

  ```bash
  cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench layout -- render_refresh_cache_regression --sample-size 10 --warm-up-time 0.1 --measurement-time 0.1
  ```

  The smoke compares cached and uncached refresh paths for paint-rich,
  nearby-rich, layout-matrix, animated shadow, and scrolling animated shadow
  cases. It caught the scroll-container regression, which was fixed by bypassing
  render subtree cache lookup under scrolling scene offsets and not storing dirty
  scroll-container render caches. It was then extended with cold
  layout+refresh variants for app-switch/upload regressions.

Run focused benchmark smoke after future behavior changes that should reduce
refresh work. For the render-cache performance slice, compare renderer stats and
benchmark output before moving on to registry subtree chunks.

## Completion protocol

When this slice is implemented and validated:

1. Fold durable notes into `layout-caching-roadmap.md`.
2. Fold implementation lessons into `native-tree-implementation-insights.md`.
3. Update `layout-caching-engine-insights.md` if the design meaningfully changes.
4. Update `plans/README.md` next-step ordering.
5. Ask before deleting this temporary active plan file.
