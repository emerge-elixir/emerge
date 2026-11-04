# Mouse leave continues tracking

- [x] Reproduce with actual native moves and controller-driven render patches,
  including crossing multiple probes, gaps, scales and delayed updates.
- [x] Identify stale hover/geometry/dispatch ownership rather than filtering
  legitimate event positions in the demo.
- [x] Fix the root cause and add a failing-before-fix regression.
- [x] Run native/framework/demo suites serializing different NIF build matrices.

Preserve all existing work; no live desktop display is available.

## Root cause and fix

A cached or queued registry can still contain a retained mouse_over_active flag
from when probe A was hovered. After the runtime delivered leave and cleared its
stack, reconciliation treated the empty stack as uninitialized and seeded A again.
Each subsequent publication could emit another leave, updating both Now and Leave.
Hovering B temporarily replaced A; after B left, the empty stack seeded A again.

The runtime now records whether a hover transition has been observed. Retained
flags may bootstrap only an uninitialized runtime; thereafter an empty stack is
authoritative. Existing geometry refresh, initial-state bootstrap and input-reset
behavior remain intact. No per-move full registry rebuild or demo-side filtering
was introduced.

## Reproduction and validation

- Native regression failed before the fix: the first cached publication after
  leaving A emitted a fourth event instead of leaving the count at three.
- Two native regressions cover repeated cached publications, sibling handoff,
  leaving the second box, window leave/re-entry and a snapshot racing the queued
  tree hover-clear.
- Demo NIF regression patches the real controller/view after every callback,
  crosses all 1x/1.6x/2x probes at inherited 1x/1.25x, checks exact enter/move/leave
  counts and verifies both readouts remain frozen outside all boxes.
- Framework ./ci-tests.sh all: 710 ExUnit/doctest cases passed, 12 hardware
  exclusions; formatting/Credo/Clippy/Dialyzer and release Rust tests passed.
- Explicit debug cargo test: 1,639 unit plus 14 integration tests (1,653 total).
- Demo formatting, warnings-as-errors compile and full mix test: 122 passed.
- NIF configurations were built serially; the demo dependency was force-rebuilt
  with its wider configured feature matrix after framework CI.
- Logs: /tmp/emerge-hover-before.log, /tmp/emerge-hover-ci.log,
  /tmp/emerge-hover-rust.log and /tmp/emerge-hover-demo.log.

Live desktop input is unavailable. Restart to load native changes; macOS also
needs a rebuilt local host (EMERGE_SKIA_MACOS_HOST_BUILD_LOCAL=true).
All pre-existing work is preserved; no commits/merges were made.
