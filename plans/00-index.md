# Cleanup Planning Index

This folder collects cleanup plans for improving readability, ownership boundaries,
and documentation before larger feature work.

These are planning documents, not implementation commitments.

- They are intentionally separated so work can be staged.
- They assume current behavior should stay stable unless a plan explicitly says otherwise.
- They are written as proposals, because some findings still need validation before code moves.

## Suggested Reading Order

1. `plans/01-docs-and-tests-alignment.md`
2. `plans/03-rust-runtime-boundary-split.md`
3. `plans/04-refresh-pipeline-separation.md`

## Status Snapshot

- `plans/01-docs-and-tests-alignment.md` - partially completed
- `plans/03-rust-runtime-boundary-split.md` - not started
- `plans/04-refresh-pipeline-separation.md` - not started

The Elixir cleanup track is complete and is no longer tracked as an active plan.

## Short Summary

### `01-docs-and-tests-alignment.md`

Low-disruption cleanup focused on stale docs, missing docs, and uneven test organization.

Part of this is already complete through the recent user-facing DSL cleanup, but the larger docs/internals pass is still open.

### `03-rust-runtime-boundary-split.md`

Medium-disruption module reorganization on the Rust side.

Focuses on shrinking `native/emerge_skia/src/lib.rs` and making runtime ownership easier to follow.

### `04-refresh-pipeline-separation.md`

High-disruption architectural cleanup.

Focuses on decoupling render work, event rebuild work, and shared refresh semantics.

## Recommended Order

If the goal is to improve readability with the least risk:

1. Align docs and tests with current code.
2. Use the completed Elixir cleanup as the stable base for the remaining docs and Rust cleanup plans.
3. Split Rust runtime boundaries.
4. Revisit deeper refresh-pipeline separation only after the codebase is easier to reason about.

## Shared Ground Rules

- Prefer structural moves before behavioral rewrites.
- Keep one clear source of truth for semantics such as paint order, nearby ordering, and refresh behavior.
- Avoid adding adapters or wrappers that only rename complexity.
- Write or update docs as code moves, not after the fact.
- Preserve useful low-level tests even if the surrounding test layout changes.

## Cross-Cutting Questions To Validate

These questions affect more than one plan and should be answered before implementation begins:

- Which native helper functions are part of the real public surface, and which exist mainly for tests/debugging?
- Should global asset/font/image state remain acceptable for the current stage, or should ownership become renderer-scoped soon?
- How much of the current test suite should continue to target the raw EMRG/native boundary directly?
- Should `guides/internals/feature-roadmap.md` remain a roadmap, or become a current capability/status document?

## Exit Criteria For The Overall Cleanup Phase

Cleanup work should be considered successful when:

- a new contributor can read the docs and get a correct picture of the current architecture
- major ownership boundaries are visible from module structure alone
- the largest files are large because of domain depth, not because several unrelated concerns were left together
- public API modules read like public API modules, not mixed implementation buckets
- test layout makes it obvious where to add new coverage
