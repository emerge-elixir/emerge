# Investigation: architecture changes that simplify animation

Status: investigation only, based on code at `641d355`. No refactor is approved
or implemented. The [incremental animation plan](active-layout-resolved-animation-lengths.md)
remains unchanged; this document evaluates an alternative foundation.

## Conclusion

**Yes: making geometry evaluation independent of live-state commit would simplify
both full-matrix length animation and `Animation.change/3` substantially.** It
removes the need for special-purpose tree-copy isolation and scattered source
capture around mutable layout state.

However, that is a larger native-layout refactor than adding the feature through
the current interfaces. It makes the feature code simpler, not necessarily the
total work smaller. Prefer staged boundaries over replacing the engine:

1. Extract shared frame-input composition and explicit update effects.
2. If investing in architecture, separate geometry workspace/results from commit.
3. Extract pure allocation arithmetic where it offers proven local queries.
4. Keep the existing sampler and lifecycle rules; share small clock/run helpers,
   not a general animation graph.

No time estimates. Complexity below means code ownership, coupling and migration
risk. Performance improvements are hypotheses to validate, not benchmark results.

## What currently makes the feature difficult

The code already separates `NodeSpec`, `NodeRuntime`, `NodeLayoutState` and
`NodeRefreshState`. The missing boundary is **operational**, not another rename
of those structs: layout helpers can still mutate the entire `ElementTree`.

| Existing code | Architectural coupling |
| --- | --- |
| `tree/layout.rs::prepare_*_frame_attrs_for_update` | Sampling, attr composition/scaling, runtime normalization and dirtying are interleaved across full/active/dirty preparation paths |
| `NodeLayoutState` | Effective attrs, committed-looking frames, measurement workspace/caches, paragraph output and scroll state share one mutable container |
| `resolve_element` | Geometry calculation also updates paint order, scroll state, nearby geometry and cache/registry bookkeeping |
| `update_scroll_state` | A speculative layout can clamp offsets or follow a new scroll end, not merely produce a different rectangle |
| Media measurement/cache-key construction | Calls `assets::ensure_source` as well as reading dimensions |
| `Patch::SetAttrs`, text patch fast paths | Declared/effective attrs and sometimes frames change before the animation runtime sees the completed update |
| `TreeInvalidation` | Encodes required work, not the exact input changes needed for endpoint-cache validity |
| Animation runtime maps | Timing/sampling are reusable, but ownership-specific storage and start/completion paths are duplicated |

Rust paths are under `native/emerge_skia/src/`. Existing `LayoutOutput` is a
**refresh output containing render/event data**, not a reusable geometry-query result.

## Candidate refactors

| Refactor | Breadth | What becomes simpler | Assessment |
| --- | --- | --- | --- |
| Shared frame-input composition | Animation + layout preparation | One place to apply samples/endpoint overrides with correct units/styles | Small, useful prerequisite |
| Typed update effects | Patches + tree update + invalidation | Change detection, coalescing, wake-up and precise context invalidation | Bounded integration; useful even without the larger refactor |
| Geometry evaluation / commit split | Layout helpers, node layout storage and caches; adapters at update/refresh | Safe endpoint queries, stable source geometry, no whole render-tree copies | Strongest architectural benefit; largest migration risk |
| Pure axis allocation | Row/column planning and seed preparation | Cheap fill queries in independently constrained containers | Focused extraction, not a universal solution |
| Shared clock/run mechanics | Native animation runtime | Change/enter/exit start and completion reuse | Extract common pieces; avoid a wholesale runtime rewrite |
| General dependency graph or replacement layout engine | Broad layout/runtime migration | Potentially broader incremental evaluation | Not justified by these features alone |

### A. Separate frame-input composition from its side effects

Introduce one reusable composition operation for a node's frame attrs. It accepts
its declaration, runtime/style context, animation sample or endpoint override, and
scale context. Preserve the current ordering: raw animation overlay, normal scale
preparation, then interaction-style application.

Full-tree, active-only, dirty-subtree and endpoint callers select **which nodes**
to prepare, rather than implementing different composition rules. Runtime input
normalization and dirty marking remain explicit update operations, not hidden
inside a read-only endpoint query.

This removes duplicated preparation/scale handling without requiring a new public
attr system. Keep existing `Attrs` initially; typed layout/paint projections may
be views/keys, not a rewrite of the DSL or wire format. Preserve partial preparation:
returning a newly cloned full-tree attr map each pulse would be a regression.

**Limit:** clean composition alone does not make measurement/allocation read-only.

### B. Make applied update effects explicit

Have patch processing expose a bounded report of successful changes:

- retained node/mount identity and affected properties;
- old/new declared values and policy changes where needed;
- actual layout-input changes, separate from generic work invalidation;
- removals/remounts and any partial-application error.

The animation runtime consumes this report to create/retarget instances. It need
not rediscover changes by scanning nodes or maintain an idle last-target registry.
The same report supplies endpoint-context revisions and wakes paint-only changes.

Keep existing patch semantics: `apply_patches` currently commits a prefix before
an error. Reporting that prefix is not atomic rollback. Coalescing must distinguish
first source, latest applied target and whether a new native frame has been committed.

Without refactor C, source snapshots are still needed before destructive geometry
writes. With C, old geometry remains stable and reports only need the old semantic
values that declaration replacement would otherwise erase.

### C. Make layout a geometry query with an explicit commit

This is the substantial architecture change worth investigating.

Proposed internal interfaces, shown schematically—not new public APIs:

```text
LayoutInputs
  read-only model/topology + runtime inputs
  frame-attr view / sparse endpoint overrides
  viewport, scale and available font/media metrics
        │
        ▼
evaluate_geometry(inputs, reusable_workspace)
        │
        ▼
GeometryResult
  frames, unrotated boxes, content extents, paragraph fragments
  derived paint ordering and proposed scroll state
  context-tagged cache updates
        │
        ├─ endpoint query: read dimensions; discard temporary results
        └─ live frame: commit_geometry → existing refresh / RenderScene / registry
```

#### Ownership rules

- Evaluation may mutate its workspace and correctly keyed measurement memoization,
  but cannot mutate the model, committed presentation, event registry or asset jobs.
  Read available media dimensions through an injected lookup; reuse immutable font
  resources. This is observable-state isolation, not a ban on safe cache warming.
  Resource registration/loading remains an explicit normal-update setup step;
  direct/headless paths need that setup too, not globally disabled asset loading.
- Results retain their scale/frame context. Existing layout frames are not raw
  logical animation values: convert unrotated endpoint boxes once at the animation
  boundary, using the captured context rather than newly patched ancestor scales.
- Requested scroll/runtime state is input. Compute scroll clamping/end-following in
  the workspace as needed, but commit the live proposal only once. Simply omitting
  scroll handling in queries could change their layout/cache behavior.
- Paint order and paragraph fragments are derived outputs. Nearby placement still
  uses the proper host border-box, not a separate overlay shortcut.
- Overrides participate in normal measurement/resolve dependencies. A copied
  "clean" flag is not proof that a subtree is valid under a different projection.
- Distinguish committed native presentation from physically displayed GPU output:
  render queues can drop scenes. Reuse existing presentation timing/anchoring;
  this refactor does not add GPU readback or promise knowledge of the last shown pixel.

#### Why this simplifies the two features

For explicit animation, an unresolved keyframe is just another `LayoutInputs`
projection through the same evaluator as live layout. For `change`, the old
committed box survives declaration updates while the new target is queried.
Both then hand numeric endpoints to the existing interpolation path.

No animation-specific clone of registry/render fragments, live-tree rollback,
probe-only source-capture hooks or asset-load suppression scattered through callers
is needed once that separation is complete. There is still one layout algorithm.

#### How to avoid a renderer-wide rewrite

Keep `element.layout.effective`, frames and scroll fields as compatibility outputs
initially. A commit adapter installs changed results there, marks appropriate
render/registry damage and preserves the existing refresh APIs. Render and event
builders continue reading their current fields; backends still receive `RenderScene`.

Move measurement/resolve workspace and cache ownership behind the evaluator
incrementally. Do not require every render/event helper to consume a new snapshot
API before the feature can work.

Patch-side geometry fast paths must produce pending geometry deltas or use that
same commit path. Leaving them writing into the old source presentation would
violate the central invariant even if the main layout pass is isolated.

#### Cost and performance caveats

This touches many helpers taking `&mut ElementTree`, not merely `run_layout_passes`.
Measure/resolve reuse, subtree shifting, paragraph flow, nearby topology and detached
caches all need parity coverage. The benefit is replacing accidental access with
explicit inputs/outputs, not rewriting their sizing rules.

Do not mandate persistent trees, copy-on-write every field, a new ECS or full-scene
double buffering. Borrow the stable model during synchronous evaluation, reuse
native `NodeIx`-indexed storage where suitable, and retain only needed committed
outputs/cache data. Existing native dense storage does not imply new BEAM arrays.

A paint-only frame must still bypass geometry. A small update must not acquire an
unconditional full-tree materialization/commit pass. Compare copied bytes and
retained-cache hits, not just the number of function calls.

**Limit:** isolated evaluation removes copying/state hazards, not the mathematical
need to evaluate layout-dependent endpoints or define concurrent endpoint context.

### D. Extract allocation from child resolution

`build_row_layout_plan` and `build_column_layout_plan` currently gather seeds,
resolve cross-axis-dependent children, distribute fill and build alignment groups.
Separate their already-existing numeric allocation from those tree mutations:

```text
measure/reflow required child inputs
→ allocate_axis(lengths, intrinsic sizes, available space, spacing)
→ resolve/place children using allocated sizes
```

Normal layout and endpoint queries can share the allocator. A definite independent
row/column with valid sibling measurements may answer a fill endpoint without a
whole geometry query.

Do not confuse pure allocation with pure measurement: wrapped text, intrinsic
images and content parents may require fresh cross-axis work first. Preserve
existing min/max and weighted-fill allocation rules, including their current
bounded-space behavior. Share arithmetic, not a new flexbox interpretation.

This extraction is compatible with C but can stand alone. It extends the existing
[layout-caching roadmap](layout-caching-roadmap.md)'s constrained-container work,
not a replacement roadmap or a new general dependency graph.

### E. Share run mechanics, not an animation compiler

A small shared clock/run primitive can represent pending presentation start,
segment progress, completion and optional resolved source/target state. Explicit
keyframes, generated change keyframes and lifecycle triggers use it while retaining
owner-specific admission/priority rules and active indices.

Keep logical sampling separate from attr scaling. Cache an immutable spec or its
identity at admission; do not generate keyframes or debug-string fingerprints each
pulse. No need for a dependency-scheduled track graph or a second interpolator.

**Limit:** this improves lifecycle code but does not solve speculative layout or
make a `Fill` value numeric. Unifying runtime structs alone is not the answer.

## What no behavior-preserving refactor eliminates

1. **Endpoint context.** Two simultaneous 40px → fill siblings in a 600px row
   target 300px each, not the 560px obtained from independent stale-peer queries.
   Asynchronous siblings/parents still require a coherent projection and retargeting
   rule; neither an immutable model nor a pure query chooses that policy for us.
2. **Interpolation semantics.** Blending fixed basis with fill weight can produce
   213⅓px where the current plan specifies a 170px midpoint. Existing weighted-fill
   interpolation also differs from always interpolating resolved pixel widths.
   A single-pass symbolic blend is a semantic alternative requiring approval.
3. **Lifecycle distinctions.** First mount, identical-target updates, interruption,
   enter/base handoff, exit ghosts and symbolic completion still need explicit rules.
4. **Declarative encoding.** Native change triggers still need policy metadata;
   a refactor does not remove the codec/host compatibility work or introduce a need
   for new NIF entry points/per-frame BEAM calls.

## Recommended decision and staged proof

**If the goal is only to ship these features, retain the incremental plan.** Take
A/B and small pieces of E as focused extractions where they remove duplication.

**If an architectural investment is desired, choose C with A/B as prerequisites.**
It has the strongest payoff for both animations and future geometry queries.
D then provides targeted fast paths. Do not make a replacement layout engine,
full dependency graph or semantic interpolation change part of that investment.

Suggested independent proof steps, not a committed implementation schedule:

1. Extract composition/update-effect seams with existing behavior unchanged.
2. Introduce a geometry-result/commit boundary; keep compatibility outputs and
   old behavior as a differential oracle. An initial adapter may still copy layout
   state—name that limitation rather than claiming cloning has been eliminated.
3. Move geometry workspace, resource lookups and derived side effects behind the
   evaluator. Migrate ordinary live layout and endpoint queries onto the same core;
   remove the adapter/copy path only after parity and cost checks.
4. Extract allocator queries for independently constrained cases if useful.
5. Implement both animation frontends against the shared interfaces and verify the
   full matrix; promote architectural changes to the active plan only after a decision.

Acceptance for the investigation to justify a refactor:

- Ordinary layout pixels/frames, hits, nearby semantics, scroll end-following,
  paragraph layout, scaling and retained-cache behavior remain equivalent.
- Querying an endpoint changes neither committed presentation nor asset requests;
  committing live geometry applies derived effects exactly once.
- Old semantic/source values survive coalesced updates, patch fast paths, remounts
  and partial failures without a duplicate persistent target-history registry.
- Paint-only/active-only paths retain their limited traversal; measure allocations,
  copied bytes, query misses and cache reuse for small changes in large trees.
- Explicit/change animations share results and preserve compatible interpolation;
  concurrency tests prove the chosen context policy separately from storage isolation.
- Shared `TreeUpdateEngine` and direct/headless helpers behave alike. Linux actor
  and macOS host orchestration need integration tests, not a backend/thread redesign.

No implementation or performance experiment was performed for this note. A future
prototype must run `cargo test` and `mix test`, use `./ci-tests.sh` for full validation,
and respect the existing benchmark performance lock/device qualification plan.

## Sources and related notes

- [Architecture](../guides/internals/architecture.md)
- [Layout/refresh/cache flow](../guides/internals/layout-refresh-render-flow.md)
- [Layout caching roadmap](layout-caching-roadmap.md)
- [Platform orchestration differences](platform-runtime-architecture-differences.md)
- [Animation performance guardrails](active-low-resource-animation-smoothness.md)
- Code: `tree/{element,layout,animation,patch,invalidation}.rs`,
  `runtime/tree_update.rs`, `tree/render.rs`, `events/registry_builder.rs`.
