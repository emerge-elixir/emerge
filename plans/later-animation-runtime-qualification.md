# Later work: animation performance, runtime and platform qualification

**Status: deferred; not a dependency of shared animation implementation closeout.**
The user prioritized [minimal animation closeout](shared-animation-closeout.md).
Animation implementation is now closed; see
[final evidence](artifacts/shared-animation-closeout/README.md). This plan owns the
remainder of the old P1–P9 backlog. Do not start these packages
as an automatic continuation of closeout; choose a focused package separately.
Existing completed fixes/evidence remain preserved. Deferred does not mean solved,
unimportant, safe to ship on every target, or approved for implementation.

## Scope transfer from the old plan

| Old package | Minimal closeout retains | Later work owns |
|---|---|---|
| P1: baseline/accounting | Existing contract evidence and small current benchmark snapshot | Complete phase attribution, exact live/peak allocation and disposal accounting; expanded workload baselines |
| P2: dependency/continuation | Audit implemented clocks/coupling; fix demonstrated supported defects | Additional owner/layout/curve/rate permutations, randomized feedback histories and proof-work optimization |
| P3: provenance | Audit existing immutable inputs and combined-cause regressions | Expanded media/scroll/font concurrency schedules, broader pairwise/soak coverage and provenance cost analysis |
| P4: transport | Audit implemented roles/units/remount/first-source behavior | Randomized topology/churn, broader role/order combinations and capture/replay representation optimization |
| P5: lifecycle | Audit handoffs/holds/terminal/ghost cleanup and release | Long-running lifecycle/resource churn and remaining baseline/phase combinations |
| P6: runtime/input/damage | Existing animated geometry, actor/direct publication and terminal-output regressions | General input throughput, pressure, virtual-key/inertia/IME histories, sustained failure and whole-runtime progress work |
| P7: model/performance | Preserve fast paths; disclose current costs | Full-model copying/storage, COW experiments, scans, ancestry/query batching, release/cancellation cost and target budgets |
| P8: platforms | Available basic build/compatibility checks; explicit exclusions | Physical Wayland/DRM/headless GPU/macOS/constrained-device execution, cadence, faults, synchronization and artifacts |
| P9: audit/docs | Changed animation API/caller/Rustler seams, docs and final CI | Broader unchanged-runtime audit and external platform/release-artifact qualification |

Any old unchecked item not required by the closeout's ten contracts belongs here,
not to an implicit extra completion gate. If later work demonstrates a supported
animation correctness bug, report/fix it as such; do not hide it under qualification.
Use risk-based/factor coverage, not an exhaustive cross-product without a stopping rule.

## L1. Async input editing and consistency — deferred

Reference: [async-input draft](active-async-input-editing.md), D20 measurement evidence
in `artifacts/event-pressure-probe/`, and the [preserved D23 prototype](artifacts/shared-animation-closeout/deferred-d23.patch).
D23 is no longer in the active runtime; the patch includes its source and tests.

- [ ] Revisit the contract before code: latest-installed-snapshot dispatch versus
  waiting for effects of earlier input; geometry/focus/text reconciliation; application
  resets versus echoes; native request completion versus successful publication.
  Neither removing all waits nor the draft's geometry-watermark policy is settled.
- [ ] Decide whether to reuse the D23 native-only prototype. Preserve its failure:
  `authoritative_reset_rejects_old_scope_but_geometry_only_patch_preserves_it` returned
  empty text instead of `abcXY`. Check fixture omission/SetAttrs semantics and actual
  authority behavior; do not merely change the assertion.
- [ ] Specify callback-controlled values, mount/binding lifetimes and host/IME range
  behavior before enabling batching. No callback dropping or value/TTL-as-causal-proof.
- [ ] Implement only the agreed protocol, then validate raw/host/synthetic/clipboard
  order, failures and remounts. Retain D22's existing ordered-host safety unless an
  explicitly tested replacement contract supersedes it.
- [ ] Re-run the unchanged isolated 90-process baseline, then realistic long-text,
  BEAM/reconciliation and sustained input cases. Measure local completion separately
  from native publication/display; no speedup claim from moving a backlog.

## L2. Input pressure and whole-runtime progress — approval required

Reference: [D19 proposal](shared-animation-pressure-contract.md).

- [ ] Decide admission/backpressure/overflow behavior and numerical defaults with
  the user. Terminal renderer failure and proposed limits remain **unapproved**.
- [ ] Address Wayland full-channel semantic-input drops and unbounded runtime FIFOs
  without silently evicting keys, button edges, commits or composition.
- [ ] Account for retained and transient allocation across producers, queues,
  packets, host/IME/clipboard paths and consumption. Native receipts are not BEAM
  consumption or allocator refunds; queue-slot counts are not whole-memory bounds.
- [ ] Qualify continuous arrivals, virtual-key/inertia/capture histories, controls,
  disconnection, callback stalls and shutdown. No claimed preemption of blocked
  drivers/assets/callbacks/joins; no alternate scheduler by default.

D16–D18 improvements already landed in the dirty implementation; this package does
not require rewriting them before it has a measured problem and an agreed policy.

## L3. Animation cost and retention optimization — measurement driven

References: `artifacts/shared-animation-remaining/performance/`,
[low-resource smoothness](active-low-resource-animation-smoothness.md), and the
closeout's current snapshot.

- [ ] Profile private-model construction, queries/ancestry, live layout, registry/
  scene construction, commit and synchronous workspace/ghost disposal separately.
- [ ] Measure actual retained/peak allocations and repeated start/settle/cancel/
  retry/topology churn; distinguish live growth from allocator-retained RSS.
- [ ] Select the smallest measured optimization. COW declarations, leaner query
  nodes, sparse dirty traversal and batching are alternatives, not mandatory stages.
- [ ] Run the full 5k/20k × 1/64-owner matrix and expanded watcher/failure/topology
  workloads with source/build identities, exclusive lock and separate repeated
  processes. Preserve paint/pixel controls and total synchronous cost.
- [ ] Establish explicit geometry-animation memory/time budgets on the intended
  target. The old 20k desktop ≤12ms release gate needs independent justification;
  existing constrained-device refresh/patch budgets retain their original scope.

Historical baseline 03 predates D8 and the rebase: ~76ms warm/~87ms release/~377MiB
RSS for 20k/64 upward; full private-model memory is also expensive in simpler cases.
Those were failures, not current timings or an accepted target-device envelope.
The [closeout snapshot](artifacts/shared-animation-closeout/performance/table.md)
now records 20k/64 length release 15.888ms median, upward warm p50/p95
71.172/86.106ms and release 76.988ms, upward warm RSS 378.7MiB. It is disclosure,
not a controlled before/after speedup or completed optimization.
Do not conceal them when documenting implementation completion.

## L4. Broader robustness and qualification — bounded campaigns

- [ ] Choose explicit finite campaigns for remaining P2–P5 factors: combined input
  and asset races, randomized topology/root/Nearby transport, multi-owner/axis/phase
  composition, ghosts, cancellation/arrival and long-running retention.
- [ ] Extend event/runtime histories only for concrete risk areas: stationary hover,
  captures, IME, synthetic repeats/inertia, delayed installation and failed rendering.
- [ ] Record seeds, schedules, oracle and stopping criteria. Reuse native layout,
  existing actors and headless harness; no second reference layout engine.
- [ ] Promote concrete failures to regressions; green campaigns qualify those tested
  histories, not arbitrary histories or exact whole-runtime memory/liveness.

## L5. Physical platforms, GPU recovery and release artifacts — separate owners

Coordinate rather than duplicate:
[Linux GPU qualification](active-linux-gpu-qualification.md),
[low-resource smoothness](active-low-resource-animation-smoothness.md),
[asset runtime](active-per-renderer-asset-runtime.md), and
[platform differences](platform-runtime-architecture-differences.md).

- [ ] Execute actual Wayland input/resize/scale, DRM scanout/page flips, supported
  headless OpenGL/Vulkan output, macOS AppKit/Metal and constrained-device cadence.
  Test terminal/cleanup frames, stationary hits, repeated animation and idle.
- [ ] Qualify GPU context reconstruction, PRIME/packed-conversion/delivery failures,
  resource synchronization, blocked driver/host behavior and shutdown where supported.
- [ ] Build and verify matching EMRG9/macOS14 host/release artifacts; reject old
  hosts. A Linux protocol test is not external binary or macOS execution evidence.
- [ ] Record each route as executed, simulated, compile-only or unavailable. Keep
  physical cadence/display acknowledgments distinct from native scene/registry output.

Target-specific release claims remain gated by that target's evidence. Missing
hardware does not keep the shared animation implementation section open forever.
