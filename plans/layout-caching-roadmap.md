# Retained-layout caching follow-up

## Remaining work

1. Broaden dependency boundaries beyond fixed-size `El`/`None` and nearby overlays,
   one proven case at a time: fixed-size Row/Column, scroll containers whose extents
   change independently of parent size, then text-flow wrapping/floats/fragments.
   Track actual parent-use-of-child-size dependencies rather than guessing solely
   from element kind. Dirty descendants must remain reachable beneath a reused
   parent cache.
2. Profile attrs/key construction and id-facing traversal helpers. Replace costly
   cloning/debug-hash fields with typed attr/dependency generations and resolve to
   `NodeIx` once for hot topology traversal only where measurements justify it.
   Keep `NodeId` at protocol/runtime boundaries; native indices must not reach BEAM.
3. Investigate `text_rich_50/keyed_reorder` resolve misses and compatible-constraint
   measurement reuse. For paragraph/multiline, consider height-for-used-width or
   constrained fragment data only with exact wrapping, caret and geometry tests.
4. Revisit conservative registry chunk seeding/skipping if profiles show material
   cost, especially damaged/no-cache and escape-nearby rebuilds. Preserve full
   fallback and exact listener/blocker precedence.
5. Preserve cache identity through viewport/repeater insert/remove/reorder and
   virtualization. Build on retained runtime identity and attachment scope;
   reparenting must not silently reuse stale absolute geometry.

## Guardrails

Keep measure, resolve, render and registry reuse separate. Select work from joined
invalidation/damage, not its source. Cache lookup/store stays centralized at phase
entry points; any additional cache slots must be bounded and counter-justified.

Each new boundary/reuse rule must compare cached against uncached measured and
resolved frames, scroll extents, nearby placement and paragraph fragments. Preserve
paint-only refresh behavior, unaffected-sibling reuse and no-op patch precision.

## Validation

Run `cargo test` and `mix test` after code changes, preferably via `./ci-tests.sh`.
Stats stay gated/default-off; use unified snapshots, not per-stat NIFs. Tests must
not run benchmarks. For focused performance work, use the performance lock and
retained-layout benchmark counters, for example:

```sh
EMERGE_BENCH_SCENARIOS=list_text \
EMERGE_BENCH_SIZES=50 \
EMERGE_BENCH_MUTATIONS=layout_attr \
EMERGE_BENCH_WARMUP=0.1 \
EMERGE_BENCH_TIME=0.1 \
EMERGE_BENCH_MEMORY_TIME=0 \
mix bench.native.retained_layout
```

Relevant implementation/test entry points:

- `native/emerge_skia/src/tree/{invalidation,element,layout}.rs`
- `native/emerge_skia/src/tree/layout/tests/cache.rs`
- `native/emerge_skia/src/stats.rs`
- `bench/native_retained_layout_bench.exs`

Architecture and refresh contracts live in the
[maintained internals](../guides/internals/layout-refresh-render-flow.md).
