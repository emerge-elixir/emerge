# Ongoing-feedback baseline 03

**Assertions pass; performance gates still fail.** Native publication only, not
raster/GPU/BEAM/device latency or exact live heap.

Source: `source-sha256:4182dc987c5f2aeb6a309ec4979f545dfce9a27e82e61cfe8ccf6b50ead9be05`.
`baseline-03/` preserves source archive/manifest, actual executable digest, build
and host identities, and all raw ordered samples. One exclusive lock covered the
snapshot, build and 96 separate-process trials. No concurrent builds/tests; source
hashes verified afterward. Three rotated trials per 5k/20k × 1/64 × eight cases,
with the same runner and workload shapes as [baseline 02](README-02.md).

Reproduce with the command in [baseline 01](README.md), using a new directory.
Generate the table without rerunning measurements:

```sh
python3 plans/artifacts/shared-animation-remaining/performance/summarize.py \
  plans/artifacts/shared-animation-remaining/performance/baseline-03
```

## Results and work attribution

[All 32 combinations](table-03.md). Selected 20k-node medians, milliseconds:

| Case / count | Warm p50 / p95 | Finite release | Later settle |
|---|---:|---:|---:|
| length / 64 | 2.601 / 3.296 | 16.579 | — |
| moving / 64 | 3.639 / 4.326 | 16.640 | — |
| independent / 64 | 6.067 / 7.623 | 6.030 | 32.280 |
| coupled / 64 | 5.323 / 6.476 | 5.183 | 31.679 |
| upward / 64 | **75.974 / 88.526** | **86.899** | **35.419** |

For `upward/64`, 64 counts looping children under **one finite Content parent**:

- Steady warm attempts now use **194 native queries**, versus 257 in baseline 02.
  Total through warm motion is 23,152: 130 cold + 130 first warm + 118 × 194.
  Finite feedback now prevents unnecessary continuation certification for loops
  that will instead use ordinary frozen-presentation retargeting. Required native
  forecast/current/boundary goal checks and actual-sample guards remain.
- Conversely, exact forecast provenance adds an old-goal query to some ancestor
  and independent fixtures. Equality of published samples no longer chooses a
  different retained goal instead of validating the recorded query input. Native
  test query bounds change from 4→5 and 8→9 in those fixtures; this is extra proof
  work, not an optimization or weakened correctness check.
- Warm retention is still **130 projections / 8,450 slots**, one 64-input leaf
  forecast map, and a whole 20k-node private model. Warm RSS median is **376.7MiB**
  versus 398MiB in baseline 02, but equal record counts and allocator RSS do not
  establish reduced live heap. Continuation parent-link counts remain millions.
- Forecasts clear at finite release, but 64 mixed loops and the workspace remain
  until cancellation. Synchronous disposal and subsequent publication/settle remain
  expensive. The ordinary length/moving release still exceeds 12ms.

The previous warm p50/p95 was 106.732/123.903ms; release was 85.623ms. The query
reduction is directly counted. These three-trial timing differences are not a
causally isolated optimization benchmark: motion semantics changed, host load is
recorded but frequency/affinity are not isolated. Release ranges and ordered warm
samples remain in the table/raw files; three releases do not qualify tails.
P1 phase/live/peak attribution, P7 optimization/budgets and P8 device presentation
are still open. No deferred disposal or timing-boundary shift was introduced.
