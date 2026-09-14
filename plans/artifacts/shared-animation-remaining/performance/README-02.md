# Coupled-release baseline 02

**Functional assertions pass; performance gates do not.** This is a native
publication probe, not raster/GPU/BEAM/device latency or live-heap qualification.

Source: `source-sha256:298b7b3da06e0331cc223ee3d9d923f075d17f1f0a2b59ceaef7a1de492b28f9`.
`baseline-02/` retains the immutable source archive/manifest, actual binary digest,
compiler/build/host identities, raw ordered samples and all process outputs.
One exclusive performance lock covered snapshot, build and **96 separate-process
trials**; no builds/tests ran concurrently. Three trials per 5k/20k × 1/64 × eight
cases. Native source hashes were verified again afterward.

Reproduction uses the current `run.py` command documented in [baseline 01](README.md),
with a new output directory. The source archive is evidence, not a rollback script.

## Additional cases and retention

- `coupled`: finite 40→Fill owners share a pool with a 200→Fill/2s loop.
- `upward`: **one finite 600→Content parent**, with the selected count of
  40→Fill/2s looping children. Thus `64` here counts loops, not 64 finite parents.
- Existing paint/pixel/length/moving/mixed/independent controls remain.
- Mixed loops retain the ordinary dimension workspace after finite release; later
  loop cancellation and full settle/disposal are separately timed and must become
  inactive. A cheap finite release alone is not successful cleanup qualification.
- `forecast_sets` and `forecast_inputs` count unique retained maps/entries. Retained
  projection counts now include their leaf clock inputs. There is no history chain;
  `ClockInput` cannot contain another forecast. Numeric retirement counters still
  survive ordinary workspace disposal.

## Results

[All 32 combinations](table-02.md). Warm columns are medians of three per-process
quantiles. Release/settle values are medians of three observations, not sufficient
release-tail qualification. Host load/affinity are recorded; no frequency isolation
or affinity pin was imposed. Do not infer a causal speedup from differences between
these three-trial controls and earlier measurements.

Selected 20k-node medians, milliseconds:

| Case / count | Warm p50 / p95 | Finite release | Later settle |
|---|---:|---:|---:|
| length / 64 | 2.723 / 3.420 | 16.307 | — |
| moving / 64 | 3.665 / 4.368 | 18.572 | — |
| independent / 64 | 6.112 / 8.209 | 6.065 | 32.906 |
| coupled / 1 | 2.255 / 3.634 | 1.993 | 33.103 |
| coupled / 64 | 5.641 / 6.866 | 6.563 | 33.123 |
| upward / 1 | 2.002 / 2.833 | 2.624 | 32.557 |
| upward / 64 | **106.732 / 123.903** | **85.623** | **36.038** |

The many-loop result is an important new **failure baseline**, not a passed budget:

- One full private model copy still occurs; no group-local layout claim.
- Shared-pool `coupled` motion uses 718 queries through the 119 warm frames at both
  1 and 64 finite owners. One shared forecast map contains one input.
- `upward` with 64 loops uses **30,713 queries** through warm motion: 130 cold plus
  257 per warm attempt. Actual continuation ancestry visits are millions per run.
- Its warm retained state includes one 64-input forecast map and **130 projections /
  8,450 slots**. RSS is roughly **398MiB**, versus ~313–316MiB ordinary dimension
  cases and ~164–165MiB paint/pixel controls. RSS is not exact live-heap storage.
- Forecast maps disappear after finite release, leaving 64 active mixed loop tracks.
  Only subsequent cancellation removes the dimension workspace. The expensive
  query/projection work cannot be described as cheap merely because it is bounded.

P7 remains open for private-model size, actual affected-layout work, projection
construction, native proof batching and release/settle disposal. P1 still needs
complete phase/live/peak accounting and additional topology/failure/watch/repeated-
admission benchmarks. No device memory budget or presentation gate is closed.
