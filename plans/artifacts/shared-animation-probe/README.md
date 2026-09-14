# Shared animation native publication probe

Raw observations from one scenario per process, using the release build on the
host in `host.txt`. These are **not device qualification or end-to-end frame
latencies**. No compositor, GPU, raster drawing, BEAM messaging, CPU affinity or
frequency control is included. RSS includes the process, live model, font/runtime
state, query workspace, scene/registry caches and allocator-retained pages; it is
not a per-track byte estimate.

## Reproduce

```sh
cargo bench --manifest-path native/emerge_skia/Cargo.toml \
  --bench shared_animation --features bench-diagnostics -- 20000 64 moving
```

Arguments are total nodes, animated leaves, and `paint`, `pixel`, `length` or
`moving`. The fixture has a small animated row and a separate tall static column.
Paint animates alpha; pixel animates 40→80; length animates 40→fill. Moving adds a
looping pixel-sized parent, so finite leaf targets follow a continuous context.
The normal native synchronization/preparation/publication helpers run 119 warm
samples at 120Hz, followed by the 1s release. Diagnostic/RSS collection and sorting
are outside the timed warm calls. Cold and release timings include workspace
creation/destruction. All measurements below are milliseconds except RSS (MiB).

| Nodes | Owners | Case | Warm p50 | Warm p95 | Release | Warm RSS |
|---:|---:|---|---:|---:|---:|---:|
| 5,000 | 1 | paint | 0.514 | 0.523 | 0.514 | 53.3 |
| 5,000 | 1 | pixel | 0.538 | 0.552 | 0.562 | 53.1 |
| 5,000 | 1 | length | 0.541 | 0.761 | 3.995 | 90.2 |
| 5,000 | 1 | moving | 0.597 | 0.696 | 5.035 | 90.3 |
| 5,000 | 64 | paint | 0.747 | 0.835 | 0.745 | 53.5 |
| 5,000 | 64 | pixel | 1.000 | 1.123 | 0.847 | 53.7 |
| 5,000 | 64 | length | 1.285 | 1.346 | 5.345 | 91.9 |
| 5,000 | 64 | moving | 1.640 | 1.777 | 5.342 | 92.1 |
| 20,000 | 1 | paint | 1.354 | 1.701 | 1.480 | 163.9 |
| 20,000 | 1 | pixel | 1.458 | 1.783 | 1.498 | 164.1 |
| 20,000 | 1 | length | 1.469 | 1.827 | 14.253 | 313.1 |
| 20,000 | 1 | moving | 1.619 | 2.147 | 15.543 | 313.0 |
| 20,000 | 64 | paint | 1.735 | 2.488 | 1.649 | 164.6 |
| 20,000 | 64 | pixel | 2.184 | 2.786 | 2.259 | 164.7 |
| 20,000 | 64 | length | 2.347 | 3.013 | 15.753 | 314.7 |
| 20,000 | 64 | moving | 3.202 | 3.960 | 19.097 | 314.5 |

## Work and retention observations

- Paint and direct pixels retain no dimension workspace or tracks.
- Length and moving cases make **one full model copy** (5k or 20k nodes), not a
  group-sized copy. Static-target cases execute two cold queries, with no further
  warm query layouts. Moving cases execute 240 queries through the last warm
  frame (two cold plus two per warm frame). The counters here do not include the
  terminal query: successful release drops its workspace and counters.
- Retained projection counts include unique track and workspace Arcs. Length has
  two projections with 2/128 node slots for 1/64 owners. Moving has two with 4/130
  slots. This does **not** mean evaluation or total memory is O(group-size).
- Every case asserts that the dimension workspace and tracks are gone after
  successful release. The moving parent's ordinary pixel loop remains active.
  Regular declarations/specifications and ordinary rendering caches are separate
  from these dimension-record counts.
- 20k-node release costs exceed the 12ms tree-work guardrail even on this desktop.
  Full-model memory and cold/release work therefore remain optimization and
  constrained-device qualification work. Green functional tests do not qualify
  these costs. `/proc` RSS/HWM values are OS observations, not exact live heaps.

## Registry eligibility optimization

`before-registry-cache/` retains the previous observations for comparison (not a
statistical performance study). Previously, the derived subtree eligibility map
was recomputed for every geometry pulse. The updated probe counts actual visits:
5k/20k cold visits, unchanged through all warm frames and release. External mutable
access, revisions, and topology changes invalidate it. Native effective-layout
writes preserve only that derived-cache flag, not publication authority or input
mutation validation. Runtime/handler/Nearby regressions and the raster/hit matrix
cover this distinction. Trees with no registry-affecting subtree also avoid the
otherwise empty geometry-snapshot scan. Full-model memory/release costs remain.
