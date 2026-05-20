# Elixir Reconciliation Optimization

Status: completed on 2026-05-20

## Goal

Reduce Elixir-side frame/update overhead while preserving current semantics:
Elixir owns stable element ids, event handler payloads, and patch generation;
Rust applies compact patches in place.

Target: make `Engine.diff_state_update/2` materially cheaper for 500-2k node
trees without changing the public event-forwarding model.

## Context

Recent benchmark findings:

- After native in-place patching, `Native.tree_patch/2` is usually microseconds:
  no-op/attr patches around 2-6 µs, structural patches tens of µs.
- Current Elixir update cost dominates:
  - `Reconcile.reconcile/3`: roughly 0.8-1.7 ms p50 on 500-size benchmark trees.
  - `Engine.diff_state_update/2`: roughly 1.0-2.0 ms p50.
  - `DiffState.build_event_registry/1`: roughly 150-300 µs p50.
  - `Patch.encode/1`: usually negligible, around 0-14 µs in sampled cases.
- Raw no-id full-tree serialization + Rust decode is only a lower bound for a
  Rust-reconcile design and is not clearly better once event identity is
  considered.
- Event forwarding depends on Elixir-side stable ids and handler payloads; moving
  identity entirely to Rust would require a new event identity protocol.

Profiler/prototype findings:

- `maybe_set_attrs/4` currently strips runtime attrs for every matched node.
  A prototype `old_attrs == new_attrs` fast path roughly halved reconcile p50 in
  common no-op/small-patch cases.
- A prototype event-registry rewrite that scanned each attr map once was not a
  universal win; keep event-registry changes targeted and benchmarked.
- Hotspots include repeated `Map.drop/2`, `maps:remove/2`, list `--` in child /
  nearby comparison paths, full event-registry rebuild, and rebuilding old key
  indexes on every update.

## Plan

### 1. Add durable benchmark coverage

- Add a dedicated benchmark or script for update breakdown:
  - `Reconcile.reconcile/3`
  - `Patch.encode/1`
  - `DiffState.build_event_registry/1`
  - `Native.tree_patch/2`
  - combined current path
- Use fixed scenario/mutation subsets so results are quick and comparable.
- Record representative p50 numbers in `bench/README.md` or this plan before
  each optimization.

### 2. Land low-risk attr comparison fast path

- In `maybe_set_attrs/4`, skip `TreeAttrs.strip_runtime_attrs/1` when full attr
  maps are equal.
- Validate patch output is unchanged for existing reconcile/diff tests.
- Benchmark before/after on the breakdown suite.

### 3. Avoid building assigned trees on runtime hot path

- Add an internal hot-path API that returns only `{patch_bin, next_state}` for
  viewport rendering.
- Keep existing public/test APIs that return assigned trees.
- Refactor reconciliation so the hot path can build only:
  - next `VNode`
  - patch list / patch binary
  - event-registry data
- Avoid reconstructing `%Element{}` trees unless callers explicitly need them.

### 4. Make event registry incremental or cheaper

Options to evaluate, in order:

1. Build event registry from `VNode` instead of assigned `%Element{}` if assigned
   tree construction is removed.
2. Update registry during reconciliation while old/new attrs are already in hand.
3. Emit/apply explicit event-registry deltas for:
   - `set_attrs` on event attrs
   - inserted subtrees
   - removed subtrees

Keep support for ordinary pointer events, key bindings, and `virtual_key` hold
handlers. Avoid broad attr-map scanning rewrites unless benchmarks prove a win
across interactive and non-interactive scenarios.

### 5. Reduce list and key-index overhead

- Replace repeated list `--` operations in `maybe_set_children/3` and
  `maybe_set_nearby_mounts/3` with one-pass or `MapSet`-based logic where sibling
  counts justify it.
- Avoid rebuilding a global old-key index for trees without keyed siblings.
- Consider per-parent keyed lookup indexes in `DiffState` if keyed lists remain a
  major cost after assigned-tree removal.

### 6. Recheck full pipeline tradeoffs

After each significant change, compare:

- Elixir current update path + Rust patch apply
- raw/full-tree serialize/decode lower bound
- full renderer/demo frame timings where available

Do not move reconciliation to Rust unless the full-tree wire cost plus a realistic
Rust diff/event-identity implementation beats the optimized current path.

## Progress

Implemented in current slice:

- Added `bench/engine_update_breakdown_bench.exs` and `mix bench.engine.update_breakdown`.
- Added attr equality fast path before stripping runtime attrs in reconciliation.
- Added runtime patch path that avoids constructing a full assigned tree:
  - `Reconcile.reconcile_patches/3`
  - `DiffState.diff_and_encode_binary/2`
  - `Engine.diff_state_update_binary/2`
  - optional renderer callback `patch_tree_runtime/3`
  - viewport runtime uses `patch_tree_runtime/3` when available.
- Kept public `diff_state_update/2`, `EmergeSkia.patch_tree/3`, and test/dev
  assigned-tree semantics intact.
- Extracted event-registry construction into `Emerge.Engine.EventRegistry` and
  build the runtime registry during runtime reconciliation instead of from the
  assigned tree.
- Replaced large sibling-order list-difference checks with a thresholded
  MapSet-based path while keeping the small-list path simple.

Representative p50 after this slice, from
`EMERGE_BENCH_SCENARIOS=list_text,scroll_rich EMERGE_BENCH_SIZES=500
EMERGE_BENCH_WARMUP=0.2 EMERGE_BENCH_TIME=0.5 mix bench.engine.update_breakdown`:

| Scenario / mutation | Runtime update | Public assigned update | Runtime reconcile only |
|---|---:|---:|---:|
| `list_text_500/noop` | 565 µs | 648 µs | 417 µs |
| `list_text_500/keyed_reorder` | 688 µs | 760 µs | 523 µs |
| `list_text_500/insert_tail` | 689 µs | 756 µs | 527 µs |
| `scroll_rich_500/noop` | 1.126 ms | 1.245 ms | 843 µs |
| `scroll_rich_500/keyed_reorder` | 1.147 ms | 1.268 ms | 859 µs |

Follow-up considerations after this slice:

- Evaluate persistent/per-parent keyed indexes only if keyed sibling workloads
  remain hot after the runtime binary path.
- Consider making the public API expose an assigned-tree-free update variant for
  callers outside viewport runtime.

## Baseline/current benchmark comparison

Goal: compare the previous implementation at `HEAD` against the current working
slice without changing source.

Method:

1. Create a temporary baseline worktree at `HEAD` outside the repo.
2. Run the same standalone benchmark script in both trees.
3. Use the same scenarios, mutations, warmup, and reps for both runs.
4. Compare:
   - previous `Engine.diff_state_update/2` public assigned path
   - current `Engine.diff_state_update/2` public assigned path
   - current `Engine.diff_state_update_binary/2` runtime path
   - `Reconcile.reconcile/3`
   - current runtime reconcile with event registry
   - `DiffState.build_event_registry/1`
   - `Patch.encode/1`
5. Include a focused native patch smoke only as context; native patching is not
   expected to change in this slice.

Suggested benchmark matrix:

- Scenarios: `list_text`, `interactive_rich`, `nearby_rich`, `layout_matrix`,
  `scroll_rich`
- Size: `500`
- Mutations: `noop`, `event_attr`, `keyed_reorder`, `insert_tail`,
  `remove_tail`, `nearby_reorder`, `nearby_slot_change`
- Reps: `300-500`, warmup `30-50`

Primary report rows:

| Scenario / mutation | Previous public update | Current public update | Current runtime update | Speedup vs previous |
|---|---:|---:|---:|---:|

Also report stage breakdown for representative cases:

| Scenario / mutation | Reconcile | Event registry | Patch encode | Runtime update |
|---|---:|---:|---:|---:|

### Comparison run: 2026-05-20

Command shape:

```bash
rm -rf /tmp/emerge-baseline
git worktree add --detach /tmp/emerge-baseline HEAD
ln -s /workspace/emerge/deps /tmp/emerge-baseline/deps
cd /tmp/emerge-baseline && \
  MIX_BUILD_ROOT=/tmp/emerge-baseline-build \
  EMERGE_BENCH_SCENARIOS=list_text,interactive_rich,nearby_rich,layout_matrix,scroll_rich \
  EMERGE_BENCH_SIZES=500 BENCH_LABEL=previous REPS=500 WARMUP=50 \
  mix run /tmp/update_path_compare.exs > /tmp/update_compare_previous.csv
cd /workspace/emerge && \
  EMERGE_BENCH_SCENARIOS=list_text,interactive_rich,nearby_rich,layout_matrix,scroll_rich \
  EMERGE_BENCH_SIZES=500 BENCH_LABEL=current REPS=500 WARMUP=50 \
  mix run /tmp/update_path_compare.exs > /tmp/update_compare_current.csv
```

Raw outputs:

- `/tmp/update_compare_previous.csv`
- `/tmp/update_compare_current.csv`
- `/tmp/update_compare_summary.md`

Geomean median speedups across the selected mutations:

| Scenario | Current public vs previous | Current runtime vs previous |
|---|---:|---:|
| `list_text_500` | 1.56x | 1.72x |
| `interactive_rich_500` | 1.17x | 1.07x |
| `nearby_rich_500` | 1.72x | 1.89x |
| `layout_matrix_500` | 1.35x | 1.74x |
| `scroll_rich_500` | 1.58x | 1.79x |
| **all** | 1.46x | 1.61x |

Representative medians in µs:

| Scenario / mutation | Previous public | Current public | Current runtime |
|---|---:|---:|---:|
| `list_text_500/noop` | 1031 | 584 | 523 |
| `list_text_500/keyed_reorder` | 1037 | 750 | 674 |
| `nearby_rich_500/nearby_reorder` | 1629 | 983 | 907 |
| `layout_matrix_500/noop` | 1395 | 1106 | 681 |
| `scroll_rich_500/noop` | 1949 | 1161 | 1027 |
| `scroll_rich_500/keyed_reorder` | 1963 | 1259 | 1119 |

Notes:

- The current runtime path improves broad update cost versus previous public
  update, with the strongest wins on list/nearby/scroll/layout scenarios.
- `interactive_rich_500` is mixed: wins are smaller because event-registry work
  and event-heavy attrs dominate, so this is the best next optimization target.
- Manual microbenchmarks are noisier than Benchee/Criterion, but both baseline
  and current were measured with the same script and matrix.

## Event-registry/event-heavy update optimization

### Goal

Reduce event-heavy viewport update cost, especially `interactive_rich_500`,
without changing event forwarding semantics or moving stable ids out of Elixir.

Target evidence from the comparison run:

- Broad runtime update geomean improved ~1.61x over previous public update.
- `interactive_rich_500` runtime geomean improved only ~1.07x; event-heavy attrs
  and registry work dominate this scenario.
- Current runtime reconciliation with registry still visits every node and checks
  every event attr family for each node.

### Design direction

Keep Elixir-owned event payloads, but stop reconstructing the whole flat event
registry from attr maps on every update.

Preferred incremental design:

1. Add per-node event entries to `VNode`.
   - `VNode` stores either an empty marker or `%{event_ref => {pid, message}}`.
   - `EventRegistry.node_events(attrs)` extracts handlers from attrs once.
   - Fresh subtree construction computes `vnode.events` while assigning ids.
2. Add event-registry helpers.
   - `EventRegistry.put_events(registry, id, events)`.
   - `EventRegistry.delete_node(registry, id)`.
   - `EventRegistry.merge_vnode_subtree(registry, vnode)` for inserted trees.
   - `EventRegistry.delete_vnode_subtree(registry, old_vnode)` for removed trees.
3. Change runtime reconciliation to update from the previous registry.
   - `DiffState.diff_and_encode_binary/2` passes `state.event_registry` into
     reconciliation.
   - Matched node with `old.attrs == new.attrs`: reuse `old.events`, no registry
     mutation.
   - Matched node with changed attrs: compute new node events, replace that id in
     the registry only if `old.events != new_events`.
   - Inserted subtree: merge event entries from the inserted new vnode subtree.
   - Removed subtree: delete event entries from the removed old vnode subtree.
   - Root replacement / initial upload: build a fresh registry from the new vnode
     subtree.
4. Keep public assigned-tree path stable first.
   - `diff_state_update/2` can continue returning assigned trees and rebuilding
     via `EventRegistry.build/1` until runtime path is proven.
   - Runtime/public registry equality tests should cover every representative
     mutation before changing public behavior.

### Implementation checklist

- [x] Extend `Emerge.Engine.VNode` with an event entry field.
- [x] Refactor `Emerge.Engine.EventRegistry` so node extraction is reusable:
  - [x] `node_events/1`
  - [x] `put_events/3`
  - [x] `delete_node/2`
  - [x] subtree merge/delete helpers for `VNode`
- [x] Populate `vnode.events` in fresh subtree construction.
- [x] Reuse old `vnode.events` for unchanged attrs during matched reconciliation.
- [x] Thread previous `state.event_registry` through runtime reconciliation.
- [x] Update keyed/unkeyed child and nearby removal paths to delete only removed
  subtrees from the registry.
- [x] Update insert/replacement paths to merge only inserted subtree events.
- [x] Preserve current public API behavior.
- [x] Add correctness tests comparing runtime binary state to public state across:
  - [x] no-op
  - [x] event attr mutation
  - [x] keyed reorder
  - [x] insert tail
  - [x] remove tail
  - [x] nearby reorder / slot change
  - [x] key events and virtual-key hold handlers
- [x] Re-run the current comparison benchmark and record deltas here.

### Benchmark gate

Use the comparison benchmark added in this slice:

```bash
EMERGE_BENCH_SCENARIOS=list_text,interactive_rich,nearby_rich,layout_matrix,scroll_rich \
EMERGE_BENCH_SIZES=500 BENCH_LABEL=current REPS=500 WARMUP=50 \
mix bench.engine.update_compare > /tmp/update_compare_event_registry_current.csv
```

Primary pass criteria:

- `interactive_rich_500` runtime update geomean improves materially versus the
  comparison run above.
- `list_text_500`, `nearby_rich_500`, `layout_matrix_500`, and
  `scroll_rich_500` do not regress by more than noise.
- `runtime_reconcile_registry` approaches `runtime_reconcile_patches_only` for
  no-op and non-event attr updates, because unchanged nodes should reuse their
  event entries without attr scanning.

### Risks and guardrails

- Do not drop events for removed descendants; subtree delete helpers must walk the
  old `VNode` subtree and delete only nodes with stored events.
- Do not miss key-route events or virtual-key hold events; these are not simple
  atom event attrs.
- Be careful with root replacement: easiest safe path is to discard the old
  registry and merge the new root subtree.
- Keep event payload terms on the Elixir side; no serialization/protocol redesign
  in this slice.
- Avoid a broad attr-map scanning rewrite unless benchmarks prove it helps
  interactive and non-interactive scenarios.

### Event-registry implementation progress

Implemented in the current working slice:

- Extended `VNode` with per-node `events` extracted from attrs.
- Refactored `EventRegistry` with reusable node/subtree helpers:
  - `node_events/1`
  - `put_events/3`
  - `delete_node/2`
  - `delete_vnode_subtree/2`
- Runtime binary reconciliation now receives the previous `state.event_registry`
  and updates it incrementally.
- Matched nodes reuse old event entries when attrs are unchanged.
- Changed attrs recompute only that node's events and update the flat registry
  only if event entries changed.
- Inserted subtrees merge their new vnode event entries.
- Removed child/nearby subtrees delete only those old vnode ids from the flat
  registry.
- Initial trees and root replacements reset the registry and merge the new root
  subtree.
- Added event-heavy sequential parity coverage for click, key, virtual-key hold,
  nearby slot changes, reorder, insert, and removal.

Comparison after this slice, from
`EMERGE_BENCH_SCENARIOS=list_text,interactive_rich,nearby_rich,layout_matrix,scroll_rich
EMERGE_BENCH_SIZES=500 BENCH_LABEL=current_event_registry REPS=500 WARMUP=50
mix bench.engine.update_compare`:

| Scenario | Current public vs previous | Current runtime vs previous |
|---|---:|---:|
| `list_text_500` | 1.52x | 2.25x |
| `interactive_rich_500` | 1.42x | 2.15x |
| `nearby_rich_500` | 1.64x | 2.45x |
| `layout_matrix_500` | 1.46x | 1.98x |
| `scroll_rich_500` | 1.54x | 2.32x |
| **all** | 1.51x | 2.22x |

Representative current medians in µs:

| Scenario / mutation | Runtime update | Runtime reconcile+registry | Patches-only reconcile | Registry assigned |
|---|---:|---:|---:|---:|
| `list_text_500/noop` | 373 | 367 | 367 | 178 |
| `list_text_500/keyed_reorder` | 538 | 530 | 529 | 179 |
| `interactive_rich_500/event_attr` | 519 | 509 | 520 | 178 |
| `nearby_rich_500/nearby_reorder` | 727 | 695 | 690 | 253 |
| `scroll_rich_500/noop` | 746 | 749 | 753 | 324 |
| `scroll_rich_500/keyed_reorder` | 850 | 851 | 862 | 322 |

The registry-inclusive runtime reconcile path is now effectively at patches-only
cost for representative no-op/reorder/event-heavy cases.

## Validation

Final validation on 2026-05-20:

```bash
mix compile
mix test test/emerge/diff_state_test.exs
mix test
mix quality.fast
mix dialyzer
cargo test --manifest-path native/emerge_skia/Cargo.toml --lib
mix format --check-formatted
git diff --check
```

Benchmark gates used during development:

```bash
EMERGE_BENCH_SIZES=500 EMERGE_BENCH_WARMUP=0.2 EMERGE_BENCH_TIME=0.5 mix bench.engine.diff
EMERGE_BENCH_SCENARIOS=list_text,scroll_rich EMERGE_BENCH_SIZES=500 EMERGE_BENCH_WARMUP=0.2 EMERGE_BENCH_TIME=0.5 mix bench.engine.update_breakdown
```

For native patch interaction:

```bash
mix bench.native.patch
```

## Open questions

- Which public callers truly need assigned trees on every update, versus only
  tests/dev helpers?
- Should `VNode` store stripped/wire attrs or event entries to avoid repeated
  map filtering?
- Can event-registry deltas be made simple enough to be safer than a full scan?
- Are keyed-list workloads common enough to justify persistent per-parent key
  indexes in `DiffState`?
