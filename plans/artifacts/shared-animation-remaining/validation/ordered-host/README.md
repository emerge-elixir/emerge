# Ordered host admission — async-input prerequisite

Status: implemented prerequisite, **not completion of async local editing**.
Source identity: `source-sha256:846f7e2970825252a91e70d2abc95e3a952829efcc98ea27cf2718975e25e0a1`.

## Changes

- One semantic FIFO for raw input and host commands/edits/replacement ranges.
- Host entry points retain work while stale instead of bypassing pending focus/
  geometry actions; accepted queued work returns true, not a Cocoa fallback request.
- Raw coalescing stays adjacent-only. Host operations are boundaries; replay shares
  the 64-item quantum, transfers untouched tails, and does not re-forward raw input.
- Ranges require a genuine source mount and opaque local text/focus generation.
  Remount, focus ABA, intervening content/preedit change or accepted replacement
  invalidates their old address. Later keys still resolve current focus.
- No new NIF/atom/wire, production thread, resource lock, global cache or animation
  clock. No overflow cap/terminal renderer policy enabled.

## Evidence

`before.log` is an early six-test controlled reproduction. `before-guards.log` is
against the final 11-test source: **10 fail** when only the three new admission guards
are disabled. `before-guards.diff` describes that diagnostic variant, not a restore
instruction or an old-HEAD build. The fixed `directed.log` passes all **11** tests.

Coverage includes raw/host ordering and receipt retention, clipboard/selection,
queued click target changes, mount/range isolation, focus ABA, initial registry,
failed native batch/recovery, tail payload movement, 4096-command/64-item yields,
context invalidation and synchronous range-token destruction. The external-reset
unit injects authoritative reconciliation metadata; no public codec trace is claimed.

Two D18 transport tests now explicitly supply already-resolved effects. They still
check channel/outbox ordering, receipt lifetime and both host feedback budgets, but
no longer pretend unsafe host admission is a valid way to prepare that pressure.
Six direct-session fixtures explicitly mark their manually seeded state ready.

`charge.log` updates requested-slot accounting to `PendingInput`: **40 bytes/slot**
in this build, same as the former raw type. Strings/range-token allocations and
allocator metadata are separate. This is not an allocator/RSS/memory-cap proof.
The pressure probe now samples the real type; old raw measurements are unmodified.

## Validation

- `cargo.log`: **1461 units + 14 integration**, passing.
- `mix.log`: **520** tests/doctests, 9 excluded.
- `ci.log`: full CI, **526** Elixir tests/doctests, 3 excluded; Rust 1461 + 14;
  Dialyzer 0 errors.
- `clippy.log`: benches/tests/bench-diagnostics, denied warnings, passing.
- Rust fmt checked. `source.sha256`, `source.tar.gz`, `identity.json` and
  `binaries.sha256` identify the tested sources/builds. Binaries remain at normal
  build paths and may be superseded by later work; no benchmark binary claim.

## Remaining

Local edits still serialize behind native publication. Value/TTL echo matching
cannot certify causal controlled-value responses. Versioned editing/batched
publication and the locked performance repeat remain unimplemented. No native
speedup, whole-memory bound, actual macOS execution or P1–P9 closure is claimed.
No commit/push; preexisting work and backups preserved.
