# D19 pressure-contract design/model evidence

Proposal: `plans/shared-animation-pressure-contract.md` (repository-root path).
**Not runtime enforcement. No default budgets or API changes approved here.**

## Audit

`audit.json` preserves exact excerpts, line ranges and file hashes. The current
Wayland sender drops any full-channel event before the actor FIFO; DRM's physical
input sender waits with Stop checks. macOS frame allocation and observer/dispatch
ordering show why a cap at the actor tail cannot bound ingress/construction or
provide end-to-end losslessness. These are source findings, not device execution.

## Model

- `model.py`: immutable declared-credit model; abstract units, not native bytes.
- `test_model.py`: nine tests including retained credit across sends/acks, peak
  replacement cost, independent controls, terminal status and no healthy eviction.
- `check.py` / `states.json`: depth-12 reachable-state checks for three small
  configurations: **45,476 states / 318,533 transitions**. Nonempty frontiers remain.
- `tests.log`: model tests. No timing, liveness or production-allocation claims.

Assumptions: charges are known/reserved before allocation; release follows actual
last-owner disposal; the integrator identifies genuine matching receipts. The model
cannot establish those assumptions. Native mounts, callbacks, rendering, SDK/kernel/
BEAM queues, allocator behavior and hardware are not modeled. Global counters in a
native implementation also need checked arithmetic (Python integers do not wrap).

Run from repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover \
  -s plans/artifacts/shared-animation-remaining/pressure-contract -p test_model.py -v
PYTHONDONTWRITEBYTECODE=1 python3 \
  plans/artifacts/shared-animation-remaining/pressure-contract/check.py
```

## Identity and validation

`source.sha256` identifies the proposal/model and audited source inputs; this is a
**design artifact identity**, not a new runtime build identity. Runtime sources
remain D18 (`source-sha256:e98b67da5aa6ddf42f947be3dcef2b2a86fb35336d3e1ed082c77e9dd415032c`).
`cargo.log` records **1450 Rust units + 14 integration**; `mix.log` records **520**
tests/doctests (9 excluded). `ci.log` records full CI: **1450 + 14** Rust, **526**
Elixir tests/doctests (3 excluded), **0** Dialyzer errors. `clippy.log` records denied
warnings for benches/tests/`bench-diagnostics`; CI quality/formatting passes.
These are unchanged-runtime regression runs, not new Rust/Elixir tests.
`identity.json` records artifact/runtime scopes separately.

No P1–P9 closure, new memory cap, locked benchmark or platform qualification.
Existing D12–D18 changes/backups/stash preserved; no commit/push.
