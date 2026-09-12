# Shared animation core: layout lengths and change transitions

Status: code/plan audit at `641d355`; revised design, not implemented.

## Recommendation and complexity

Keep one sampler and the existing live layout path. Add **sparse change-trigger
handling plus lazy numeric endpoint resolution**, not an animation compiler,
second scheduler, persistent target-history registry or general dependency graph.

The main integration boundary is still native animation ↔ layout preparation.
`change/3` additionally touches attr normalization/encoding and retained patch
handling. Endpoint context and source capture are correctness-sensitive; ordinary
reflow, rendering and input geometry already work and should be reused.

## Audit findings

Rust paths below are under `native/emerge_skia/src/`. Findings describe code
structure and risks in the previous plan, not reproduced production failures.

| Finding | Evidence | Required correction |
| --- | --- | --- |
| A separate idle target-history map duplicates existing state | `Element.spec.declared`; `Patch::SetAttrs` already reads `before_declared_attrs` before replacement | Use patch preimages; retain runtime entries only for running/pending transitions |
| Two layout probes per mixed transition are often unnecessary | `resolve_length` directly evaluates pixels and pixel-only min/max; change/interruption has a known source | Resolve only unknown endpoints; reuse valid source anchors and adjacent endpoint results |
| A child's current constraint is not its future fill allocation | `build_row_layout_plan` / `build_column_layout_plan` allocate sibling portions before child resolution | Do not implement mixed interpolation solely inside `resolve_length` |
| Full-tree cloning copies irrelevant retained state | `ElementTree` contains refresh fragments, registry caches and detached layout caches | Full clone is an oracle; prefer a narrow layout-copy helper, not cached render-tree copies per track |
| Running target layout on the live tree is not side-effect-free | `resolve_element` updates paint order, scroll state and nearby geometry; `update_scroll_state` clamps/follows scroll positions | Do not replace isolation with live layout plus frame-only restoration |
| Existing invalidation is too conservative to be an endpoint-cache revision | `classify_attrs_change` requests `Measure` when animation attrs exist, even for other paint-only edits; `AssetStateChanged` requests it even with no changed media dimensions | Distinguish actual endpoint inputs from generic recompute requests |
| A paint-only change trigger could fail to start | `patches_may_start_animation_runtime` returns false for `SetAttrs`; paint damage alone need not synchronize an idle runtime | Report actual change requests explicitly from patch processing |
| New triggers could inherit an old clock | Tree updates initialize sample time from `latest_animation_sample_time`; only transient entries currently have pending presentation anchors | Use a fresh/presentation-aligned start, including after idle |
| Patches are not atomic transactions | `apply_patches` mutates sequentially and returns on the first error | Do not promise whole-batch rollback or discard effects of successfully applied prefixes |
| Source capture after patching can be too late | `SetAttrs` overwrites effective attrs; text patch fast paths can also update frames before normal layout | Capture before destructive attr/geometry writes, not just at runtime sync |
| Expanding legacy discovery can add work each tick | `sync_with_tree` can scan all nodes; `active_node_ids` deduplicates with repeated `Vec::contains`; fingerprints allocate a debug string | Drive changes from affected ids, poll active runs only, and avoid per-pulse spec construction/hashing |

Two checked arithmetic counterexamples:

- 600px row, 40px → equal fill beside one fill sibling: target **300px**, linear
  resolved-box midpoint **170px**. Interpolating fixed basis and fill weight instead
  gives **213⅓px** at halfway: a different animation, not an equivalent optimization.
- Two 40px siblings simultaneously becoming fill: probing each while leaving the
  other at 40px gives **560px each**; the joint endpoint is **300px each**. Cache
  sharing or independent probing alone does not solve coupled endpoint semantics.

## Public contract

```elixir
Animation.animate([[width(px(40))], [width(fill())]], 1000, :linear)

# First declaration on an element:
Animation.change(width(px(40)), 1000, :linear)
# Later declaration on the same retained element:
Animation.change(width(fill()), 1000, :linear)
```

The latter update must create the same transition through the same core. Support
the full ordered length matrix: pixels, fill, content, weighted fill and recursive
min/max expressions of differing shapes. Both axes, all explicit animation owners,
reverse/multiple segments and change-triggered transitions are included.

Preserve existing compatible interpolation: pixels numerically, weighted fills
by **weight**, and compatible min/max trees recursively. Resolve whole dimension
expressions only for incompatible shapes. Weighted fill 1 → 3 beside weight 1
currently has midpoint 400px in a 600px row; changing that to box interpolation
would give 375px and is not a backwards-compatible simplification.

Keep existing keyframe attr-set requirements and other attribute-family restrictions.
A `change` call wraps one ordinary animatable attr; different properties may use
separate calls/timings. Other families share existing interpolation/compatibility
rules. Change instances run once; explicit repeat behavior remains unchanged.

### Change lifecycle

- First mount applies the target immediately. A retained target change uses the
  incoming duration/curve. Equality is normalized **target** equality, never a
  comparison with the changing presentation sample.
- Identical-target rerenders do not restart. Timing-only edits affect the next
  transition, not the active clock. A new target interrupts from the current
  native presentation sample over the new duration.
- Same element means existing `NodeId` **and mount identity**. Reorder follows
  existing reconciliation; removal/replacement/full upload resets ownership even
  if ids are reused. No new reconciliation identity scheme.
- Adding a policy on an existing node can use its old value/frame when available;
  otherwise seed immediately. Removing a policy cancels its run and applies the
  ordinary declaration. A later plain attr overrides both the wrapped target and
  its policy, following existing override rules.
- A settled symbolic target stays symbolic. Resize/content changes without a new
  target are normal layout; during a run they refresh endpoint context over its
  remaining interval, not a new change-trigger duration.
- Coalesce to the latest applied target before the next live frame. Do not queue
  intermediate destinations. If the final target equals the original desired
  target, keep the original active run rather than restarting it unnecessarily.

## Simplified implementation

### 1. One data path; no duplicate idle state

```text
explicit keyframes ─────────────────────────────┐
retained attr changes → once transition instance ├→ existing timeline/easing
enter/exit lifecycle ───────────────────────────┘
  → compatible interpolation OR lazy resolved endpoints
  → ordinary samples → existing preparation, damage, layout and refresh
```

State has three homes:

| State | Home / lifetime |
| --- | --- |
| Desired value and change policy | Existing declared attrs; no second last-target map |
| Pre-update source and original target | Sparse update journal for affected properties, discarded after changes are materialized/cancelled |
| Clock, optional source anchor, resolved endpoints | Running/pending instance only; cleanup on completion/removal |

Construct generated keyframes once per real target change, not on each sample.
Reuse `sample_animation_spec` and ordinary attr application. A small pending/owner
flag is enough for lifecycle differences; do not rewrite all existing runtime maps
into a general track graph. Poll change completion in the active set and reuse
completion damage handling so dropping an overlay still renders the symbolic end.

### 2. Derive changes from patch effects

Extend the existing patch application path to expose relevant old/new attr data
and source captures, alongside invalidation. This is a narrow patch-effect report,
not a second tree diff. Reconciliation already emits the necessary `SetAttrs`.

- Use preimages already available during decoding/application. Capture only fields
  needed by changed policies/runs, before effective attrs are overwritten.
- Preserve a source before text/other patch fast paths mutate its geometry too;
  taking only a late `SetAttrs` frame is insufficient. Use prepared candidates or
  capture hooks on those existing paths, without snapshotting every scene node.
- Keep the first source/original target and the latest successfully applied target
  for each retained property until the live-frame boundary. Delay replacing the
  original active instance until this coalescing decision is made.
- Validate synthesized non-length pairs before committing that node's invalid
  target. Preserve reports for successful prefixes when a later patch fails;
  follow existing error policy, not an invented atomic-batch contract.
- Wake change handling directly from these effects. Do not route each pulse through
  whole-tree policy discovery or add idle policies to the active-node list.
- Capture compatible sampled values in logical units; a width frame alone loses
  weighted-fill semantics. For mixed-length interruption use a resolved-size
  anchor. Use the existing presentation clock convention, not a new GPU readback.

Normal width/height samples already produce `Measure`; paint-only changes keep
paint/compositor handling. Policy metadata alone must not imply `Measure`.

### 3. Resolve only missing information

Use an ordered endpoint lookup, shared by both APIs:

1. **Compatible pair:** current interpolator; no endpoint resolver.
2. **Constant expression:** evaluate `Px` and pixel-only min/max directly, with
   normal numeric/scale rules. No layout or new symbolic algebra is needed.
3. **Known anchor/result:** use a supplied change/interruption source, or a prior
   adjacent endpoint with matching context. Never reuse an arbitrary old frame
   as an explicit keyframe that was not actually laid out there.
4. **Unknown symbolic endpoint:** use the existing layout engine in an isolated
   endpoint context, then retain only the resulting numbers/context token.

Typical extra endpoint work, excluding the ordinary live layout each tick:

| Case | Endpoint layout passes |
| --- | --- |
| Existing compatible animation / constant-only mixed expressions | 0 |
| `change(content/fill → px)` with a valid captured source | 0 |
| `px → content/fill`, or `change(... → symbolic)` with a known source | 1 unknown endpoint |
| Cold explicit symbolic → different symbolic, no reusable endpoint | Up to 2 |
| Warm segment, unchanged relevant context | 0 |

These are conditional work counts, not measured timing guarantees. Different
endpoint projections/context changes can require further evaluations.

A targeted next fast path is a **pure allocation query for a proven independent,
definite-size row/column**: reuse the planner's portion arithmetic and valid fixed
sibling measurements to obtain a fill endpoint without laying out a copied tree.
Extract/share that arithmetic rather than inventing another fill formula. Reject
the shortcut when wrapping, cross-axis reflow, ancestor sizing or stale measurements
make the context uncertain. It can reduce work to the relevant siblings; add it
only with oracle parity and evidence that remaining probes matter, not as a second
sampling pipeline or a new dependency framework.

For each required projection, apply the whole keyframe's layout-affecting attrs,
resolve width/height jointly and collect all requested results from that layout.
Deduplicate **identical projections**, not merely requests with the same parent.
Reuse measurement/allocation, wrapping, image ratios, insets and bounds. Return
unrotated logical border-box dimensions and scale once through normal preparation.

A narrow `clone_for_layout`-style helper can copy the existing node/topology/layout
representation while omitting refresh fragments, registry payloads and detached-
subtree cache copies. Keep anything layout actually reads; validate against a
full-clone oracle. This is not a new scratch-tree allocator or a new layout engine.
Keep one temporary workspace at a time where possible; never retain cloned scenes
per animation. Reuse workspace allocations only with a proven clean reset.

Probes must not publish frames, alter live scroll/topology/cache state, advance
clocks or start asset loads. Media measurement currently calls `ensure_source`;
provide a lookup-only path using available metrics. Reuse immutable fonts and safe
measurement memoization. Numeric substitution must match the symbolic endpoint's
relevant layout; test parent fill discovery/content sizing before declaring parity.

### 4. Small cache, honest context invalidation

Cache optional resolved endpoints on the active instance/segment, not in another
LRU/registry. Keep original expressions and source anchors separate. Drop obsolete
segments and reuse a shared adjacent keyframe only when its context matches.

Produce a small endpoint-context revision from **actual layout input changes**:
viewport/scale, relevant attr/topology/interaction edits, media/font dimensions,
and other layout animation samples. Existing measure/resolve key projections can
inform this comparison, but generic `TreeInvalidation::Measure` is not sufficient.
Animation-presence flags, policy-only edits and unchanged-dimension asset notices
must not churn the endpoint cache. The run's own numeric writes are not new inputs.

Begin conservatively for external changes; no dependency graph is required.
Refresh a moving target from the current value over the remaining segment interval,
without rewriting the spec or resetting its original clock. Further locality work
requires measurements. Do not promise cheap warm-cache behavior when other layout
inputs actually move each tick.

### 5. Coupled endpoints are a correctness gate, not a cache optimization

Synchronized tracks sharing allocation need a coherent endpoint projection; the
560px/300px counterexample must pass. Do not batch unrelated phases as though every
animation were at its final keyframe simultaneously.

For asynchronous siblings/animated parents, prototype a frozen sampled context
and at-most-once-per-frame retargeting, with no recursive animation/layout fixed
point. The exact grouping/context rule must pass continuity and terminal-symbolic
handoff tests before implementation is considered ready. Freezing another animated
node's previous numeric output without considering its completion is not sufficient.

This remains the main unresolved design gate. Full-matrix support is not permission
to silently narrow concurrent behavior or claim that two global probes solve it.

### 6. Keep lifecycle adapters small

Preserve explicit exit > enter > regular precedence and ordinary spec restart
semantics. Merge change samples on disjoint fields; reject regular/change overlap.
For enter-owned fields, keep only one latest pending change (no queue), capture
its source at handoff and leave the explicit regular animation's waiting behavior
unchanged. An unchanged mount baseline does not create a post-enter change.

Use the same source capture for interruption and exit; strip policies from ghosts.
New triggers use fresh/presentation-anchored starts after idle. Endpoints remain
original expressions after completion. An enter ending differently from its base
still follows the existing base-handoff contract; do not silently change it.

### 7. Minimal declarative metadata

Normalize the wrapper into an ordinary target plus a policy, e.g.:

```elixir
%{width: :fill, animate_change: %{width: %{duration: 1000, curve: :linear}}}
```

Merge policies per canonical property/field group in UI normalization; keep target
and policy in normal attr equality/hashing. Reuse attr/duration/curve validation.
Remove only width/height cross-variant rejection; other families retain their rules.
Use maps/linear list traversal, not repeated BEAM indexing or appended reducers.

Add one policy attribute to Elixir/native codecs; do not duplicate target values,
serialize runtime sources or overload `:animate` / `:on_change`. Update EMRG/host
compatibility and documentation for the new tag. Length tags and NIF entry points
stay unchanged. An Elixir-only keyframe rewrite is insufficient for native in-flight
interruption unless it also introduces native trigger/source semantics.

## Alternatives considered

| Alternative | Assessment |
| --- | --- |
| Evaluate both lengths using the current child slot/intrinsic frame | Small code change but wrong fill/content endpoints |
| Blend fixed basis and fill weight inside layout | Potentially avoids probes; changes the midpoint/curve semantics and requires planner/content reflow changes. Keep as an explicit alternative requiring approval, not a transparent optimization |
| Layout target on the live tree, then restore frames | Incorrect isolation: scroll, topology, dirty/cache state also change |
| Convert every length to pixels | Changes existing weighted/min-max interpolation and performs unnecessary work |
| Permanent old-target map + full-tree scans | Redundant with declarations and patch effects |
| Dedicated dependency graph/general animation IR | Not justified before the small shared resolver is measured |

## Implementation gates and validation

1. Freeze reference behavior for existing numeric/weighted/min-max animations.
   Prove endpoint arithmetic, numeric/symbolic parity and coupled projection rules.
2. Add shared lazy endpoint resolution to full, active-only, dirty-subtree and
   direct/headless preparation paths. Keep normal live layout/refresh unchanged.
3. Add sparse patch effects/change instances, source capture, completion and policy
   codec support. Audit error prefixes, geometry fast paths and idle-clock startup.
4. Verify the complete matrix and both APIs before public acceptance; optimize
   measured hot spots only. No reduced matrix or separate `change` interpolator.

Required regressions:

- `change(A) → change(B)` versus explicit `animate([A, B])` under matching context,
  including legacy weighted interpolation and both-axis mixed expressions.
- First mount/first policy, stable rerenders, policy-only edits, coalesced A→B→A,
  interruption/reversal, per-property timings, idle start, malformed updates and
  successful patch prefixes followed by failure.
- Keyed reorder, ordinary unkeyed identity, remount/full upload/id reuse, conflicting
  owners, enter/base handoff, ghost interruption/pruning, repeats and segment joins.
- Shared-fill examples, asynchronous tracks, wrapped text, image ratios, nested
  content parents, min/max, nearby/hit geometry, scroll positions and scale/rotation.
- Source capture before patch-side geometry changes; captured vs explicitly probed
  sources must not be confused. No probe frame/cache/scroll state leaks.
- Cold/warm endpoint counts above; any allocation-local shortcut versus the full
  endpoint oracle; no extra probes from compatible/paint-only paths or unchanged
  metrics. No idle runtime entries, per-frame tree scans for change
  discovery, or regenerated specs/debug fingerprints.

Measure extra layout passes, visited nodes, copied bytes/peak workspace, cache hits
and active-update work—not just the number of nominally changed subsystems.
Core edits remain animation/layout preparation, patch/update adapters, attr
normalization/codec and narrow metadata handling; no backend or reconciler redesign.

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml
EMERGE_SKIA_BUILD=1 mix test
EMERGE_SKIA_BUILD=1 ./ci-tests.sh
```

Use the performance lock in `active-low-resource-animation-smoothness.md` for
benchmarks/device qualification. This audit checked code and arithmetic examples;
no implementation, Rust/Elixir test run or runtime benchmark was performed.
