# D16 stalled input retention and replay work

Base commit: `33ee559`; D12–D16 remain uncommitted.
Functional identity: `source-sha256:51e3bac45337545965610bc0ce1f334f3297fff7f954ae4d77052cb83e8ee852`.

## Before/after work evidence

- `before.log`: enqueueing 2,000 retained commits visits **2,001,000** coalescer
  inputs. After incremental tail coalescing: **2,000**.
- `replay-before.log`: 128 successive native edit receipts trigger **341,376**
  untouched-tail reinsertion visits. After FIFO remainder transfer: **0**.

The test-only counter records coalescer calls/normalization visits. It does not
measure all dispatch/layout/text-copy/allocation work or elapsed performance. The
fresh-input vector path shares the adjacent-coalescing primitive; the stale lane owns
a FIFO. Keys, composition, commits and button edges remain ordering barriers.

## Finite retention charges

`retention.log` records this x86-64 build's requested representation capacities:

| Queue state | Records | Slot bytes | String-capacity bytes |
|---|---:|---:|---:|
| 100,000 adjacent cursor positions | 1 | 160 | 0 |
| 4,096 commits, each reserving 256 string bytes | 4,096 | 163,840 | 1,048,576 |
| Same queue after all but one commit are consumed | 1 | 163,840 | 256 |
| Fully drained | 0 | 0 | 0 |

The FIFO header is another **32 bytes**; each event slot is **40 bytes**. Payload
pointer checks verify transfer without string cloning. Spare slot capacity persists
during partial replay and is synchronously released on full drain/destruction. This
trade-off is not a memory-reduction claim. Capacities exclude allocator metadata and
usable-size rounding, other channels/queues, raw observers, local edits, trees,
retained scenes, global caches and GPU allocations. They are not RSS or a budget.

## Directed qualification

Seven new unit functions cover the two work defects, mixed cursor/scroll/resize/key/
button/UTF-8 composition ordering, wrapped-tail joins, finite capacity charges,
4,096 buffered host commits with 1,024 skipped obsolete responses (both policies),
remount recovery and raw observation once, and real actor 256-commit recovery versus
4,096-commit Stop without acknowledgments. Remounted unfocused inputs do not inherit
old edits; buffered raw events replay against accepted current state.

These are finite burst/ordering schedules, not arbitrary elapsed-time/TTL histories.

## Validation

- `cargo.log`: **1433 Rust units + 14 integration**.
- `clippy.log`: benches/tests/`bench-diagnostics`, warnings denied.
- `mix.log`: source-built release NIF, **520** tests/doctests, 9 excluded.
- `ci.log`: `./ci-tests.sh all`, **1433 + 14** Rust, **526** Elixir tests/doctests,
  3 excluded, **0** Dialyzer errors; quality/formatting pass.
- `source.sha256`: D15 selected inputs plus the stalled-input test module; its digest
  is the identity above. `changes-from-D15.txt` identifies changed selected inputs.
- `binaries.sha256`: tested release NIF and default-feature debug unit executable.

Run manifests from the repository root. Historical console EOFs are preserved.

## Remaining gates

The lossless noncoalescible queue remains **unbounded**. A hard bound needs explicit
upstream backpressure/overflow semantics that preserve control/Stop—not silent input
eviction. No cap policy was invented. Long non-staling replay can still occupy one
dispatch; end-to-end latency, all queues/allocations, arbitrary stall-duration/resource
histories and accepted device memory budgets remain open. No P1–P9 package, locked
performance or device/presentation gate closes. No new NIF/atom/wire version,
production thread, resource lock, global cache or animation clock. No commit or push.
