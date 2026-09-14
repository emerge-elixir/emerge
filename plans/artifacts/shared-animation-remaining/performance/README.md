# Locked publication and disposal baseline 01

Historical first-slice baseline. See [coupled baseline 02](README-02.md) for the
latest source and additional scenarios. The current runner has eight cases;
this archive retains the six-case runner that produced baseline 01.

**Not a performance gate pass.** The 20k-node release guardrail remains unmet;
full private-model memory remains large. These are desktop publication probes,
not raster/GPU/compositor/BEAM latency or constrained-device live-heap results.

## Identity and reproduction

- Immutable source: `source-sha256:21f2aae1d34d4538e68594e17bd64c853a0a779a2974c5ae2c1a70f17f23bb9f`.
- `baseline-01/source.tar.gz` contains the selected Rust/Elixir/config/test-asset/build
  source and runner; `source.sha256` records every file. The identity is SHA256 of
  that manifest. HEAD is recorded for reference, not treated as dirty-source identity.
- `identity.json`, `command.json`, `build.jsonl`, `build.log`, `binary.sha256` record
  host/toolchain/build inputs and the actual executed binary. Defaults plus
  `bench-diagnostics`, optimized build.
- AMD Ryzen 9 7950X, Linux x86_64, rustc 1.93.1. No affinity pin or frequency
  isolation imposed; process affinity and per-trial system load are recorded.
- One exclusive performance lock covers snapshot, build and all measurements.
  No tests/builds were run concurrently. Source hashes are checked again afterward.

From the repository root, with no concurrent build/test workload:

```bash
./scripts/performance-lock.sh --source-revision snapshot-created-under-lock exclusive \
  python3 plans/artifacts/shared-animation-remaining/performance/run.py \
  plans/artifacts/shared-animation-remaining/performance/new-baseline
```

The runner creates the real immutable identity **inside the lock before building**,
exports it as `EMERGE_SOURCE_REVISION`, and refuses an existing output directory.
Its source archive is an evidence bundle, not an instruction to restore/overwrite
an active worktree.

## Method

Three independent processes per 5k/20k × 1/64-owner × six-scenario combination:
**72 trials**. Each process records cold publication, 119 ordered warm 120Hz samples,
a finite release at 1s, RSS/high-water RSS and native diagnostics. `warm_ns` retains
raw sample order; scenarios rotate order between trials. No compilation occurs
between measured processes.

Scenarios:

- `paint`: alpha only; no dimension workspace.
- `pixel`: pixel width only; no dimension workspace.
- `length`: finite 40→Fill owners in one pool; workspace disposed at release.
- `moving`: the pool's pixel-sized parent loops; dimension workspace disposed when
  finite owners release, while ordinary pixel motion remains active.
- `mixed`: parent 600→Fill/1s loop, resetting at finite release.
- `independent`: parent 600→Fill/2s loop plus a separate 300→Fill/3s loop, outside
  the large static branch. Finite children must not restart because of that panel.

Mixed loops legitimately keep the dimension workspace alive after the finite
release. Their **later cancellation/settle publication is separately timed** and
must release the workspace and become inactive. Comparing their cheap finite
release against a fully disposed `length` case would be misleading.

All publication calls include normal synchronous workspace disposal. The numeric
`drop_time` measures disposal itself. No deferred retirement or timer-boundary
optimization was introduced. As in the previous probe, rasterization, GPU/BEAM
work and diagnostic formatting/RSS collection are outside the publication timer;
warm returned-output destruction after sampling is not included. Full phase and
renderer-shutdown attribution remain P1/P7 work.

## Results

See [all 24 combinations](table.md); units are milliseconds. Warm p50/p95 columns
are medians of the three per-process quantiles, **not** pooled end-to-end latency.
Release/cancel/drop columns are three-observation medians; release ranges expose
variation. Three releases are insufficient to qualify tail behavior.

Selected 20k/64-owner medians:

| Case | Warm p50 / p95 | Finite release | Cancel/settle | Runtime disposal |
|---|---:|---:|---:|---:|
| paint | 1.980 / 2.488 | 1.840 | — | 0 |
| pixel | 2.390 / 2.969 | 2.010 | — | 0 |
| length | 2.756 / 3.539 | 19.431 | — | 15.997 |
| moving | 3.395 / 4.080 | 18.584 | — | 14.819 |
| mixed | 4.418 / 5.273 | 5.112 | 33.378 | 15.059 |
| independent | 5.814 / 7.890 | 5.852 | 32.126 | 13.512 |

- 20k/64 cold publications: ~57ms paint/pixel versus ~171–179ms dimension cases.
- Largest observed 20k/64 warm sample: **8.942ms**, independent scenario. This is
  not a refresh/device frame-time qualification.
- Dimension workspace still copies all 20k nodes once. Warm native-query totals
  for 64 owners: length **2**, moving **240**, mixed **480**, independent **958**.
  Independent work is cohort-batched, but substantially more expensive than a
  static target. These counters count real queries, not hypothetical touched owners.
- Last/cumulative native statistics now survive ordinary disposal. Inspect
  `retired_queries` in release/settle lines rather than treating workspace absence
  as zero work. `last.source_slots` is a historical gauge, not live slots.
- Continuation ancestry counts measure actual parent-link lookups and can vary
  slightly with hash iteration order. They are not total dependency/layout visits.
- RSS is still roughly ~164–165MiB controls versus ~313–316MiB dimension cases;
  allocator-retained pages are not live-record counts. No target-device absolute
  live-memory budget has been accepted.

The baseline attributes a large release cost to actual synchronous runtime/model
disposal. It also shows substantial **other** cancellation/settle work. It does not
attribute that residual to a particular phase, prove a regression/improvement
against unlocked historical single trials, or close any device/platform gate.

Remaining benchmark cases: content watchers, failure/recovery, topology/role edits,
repeated native admissions/settles, complete phase/heap/peak accounting and larger
release-tail studies. The new repeated-settle unit test is correctness evidence,
not a replacement for those performance trials.
