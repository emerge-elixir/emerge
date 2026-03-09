# Event Registry Dispatch Hardening Plan (Next Cycle)

## Purpose

Define the remaining hardening work needed to move from a working dispatch architecture
to a maintainable, explicit, and drift-resistant implementation.

This plan is future-facing only. Completed migration items are intentionally omitted.

## Scope

This hardening cycle covers:

- dispatch job compilation structure
- strict separation of mutation-only vs output-derivation APIs
- hit-testing source-of-truth consolidation
- removal of legacy helper/fallback paths no longer needed by dispatch
- final test and docs alignment with the current runtime model

External contracts and payload semantics must remain stable unless explicitly changed and tested.

## Architectural Guardrail (Non-Negotiable)

Do **not** reintroduce selector-style dispatch logic.

Selector-style dispatch logic means runtime/event-loop code deciding winners through
feature-specific branching (ad-hoc detect/handle routing, procedural fallback chains, or direct
listener selection outside registry resolution).

Required rule:

- All listener/winner selection must happen through:
  - `DispatchJob` compilation
  - registry winner resolution
  - `DispatchRuleAction` execution into `DispatchOutcome`

Allowed in runtime orchestration:

- input normalization/coalescing
- prediction enrichment
- observer forwarding
- `DispatchOutcome` application
- runtime state advancement
- no-pred accounting/logging

Not allowed:

- feature-specific winner selection in runtime
- procedural fallback routing bypassing registry selection
- direct hit-test-to-listener emission outside dispatch jobs/rules

## Target Model

Runtime has two lanes:

- **Dispatch lane**: compile jobs -> resolve winners -> execute actions -> produce typed `DispatchOutcome`.
- **Observer lane**: forwarding only (no side effects).

Per normalized input event:

1. Normalize/coalesce input.
2. If dispatch-candidate, compile one or more `DispatchJob`s.
3. For each job, resolve candidate rules and choose one deterministic winner.
4. Execute all winning actions into typed outcome records.
5. Run observer forwarding.
6. Apply `DispatchOutcome` side effects and advance runtime state.

Core constraints:

- one winner per job
- winner may execute multiple actions
- no feature-specific fallback routing in dispatch selection
- observer lane is forwarding-only
- existing `TreeMsg` payload contracts remain stable

## Remaining Distance From Ideal

1. `compile_dispatch_jobs_for_event(...)` is branch-heavy and mixes multiple feature families.
2. Some event-derived output APIs still mutate processor internals and include trigger branches
   that dispatch does not rely on.
3. Hit-testing geometry logic exists in more than one place (drift risk).
4. Legacy helper paths remain (`cycle_focus(...)` and related baseline-compare usage).
5. Some tests still validate via helper-parity patterns rather than explicit behavior contracts.

## Workstreams

### 1) Decompose Job Compilation Pipeline

**Goal**
Make job compilation explicit, ordered, and easy to reason about without behavior change.

**Plan**
- Split `compile_dispatch_jobs_for_event(...)` into pass helpers with explicit responsibilities, e.g.:
  - primary trigger job pass
  - hover transition pass
  - style clear pass
  - scrollbar/scroll runtime pass
  - text/focus runtime pass
- Keep one top-level coordinator that preserves canonical job ordering.
- Keep trigger/stat derivation explicit and independent of helper internals.
- Add order-focused tests for representative event families.

**Acceptance**
- Output-equivalent job sets and ordering for existing event sequences.
- No behavior change in winner selection or emitted outcomes.
- Existing sequence regression coverage remains green.

### 2) Enforce Pure Output Derivation vs Mutation-Only State APIs

**Goal**
Ensure dispatch outcome derivation does not depend on mutating helper side effects.

**Plan**
- Introduce/expand pure `derive_*` helpers for event -> request translation used by dispatch actions.
- Restrict mutating helpers to explicit runtime advancement stage.
- Remove dead/unreachable trigger branches from derivation helpers
  (for example, branches that are not reachable through the current dispatch trigger wiring).
- Tighten function naming to make semantics obvious:
  - `derive_*` (pure)
  - `advance_*` / `update_*` (mutating)

**Acceptance**
- Dispatch action execution does not rely on incidental state mutation.
- Runtime state changes happen only in advancement/apply stages.
- No behavior drift in text/focus/scrollbar/style outputs.

### 3) Consolidate Hit Testing Into Single Source of Truth

**Goal**
Prevent geometry/clip/radii drift between registry and processor hit paths.

**Plan**
- Extract shared hit-test geometry primitives into one module used by both:
  - pointer candidate hit verification in registry
  - direct flag-based hit-testing helpers
- Keep data-shape adapters thin; put geometric truth in one place.
- Add focused tests for clip + rounded-corner + z-order semantics against shared primitives.

**Acceptance**
- No duplicated geometric decision logic.
- Pointer hit behavior remains unchanged across representative cases.
- Registry and processor hit paths cannot diverge due to duplicate implementations.

### 4) Remove Legacy Focus/Fallback Helper Surface

**Goal**
Eliminate legacy helper flows that are no longer part of the dispatch model.

**Plan**
- Remove `cycle_focus(...)` from production code path once rule-driven focus cycling is fully covered.
- Remove remaining helper branches that encode legacy key/fallback behavior outside active dispatch routes.
- Keep only stateful sequence tests that add unique regression value; delete helper-parity-only scaffolding.

**Acceptance**
- Focus cycling behavior remains rule-driven and unchanged.
- No hidden fallback logic remains outside registry/job resolution.
- Test suite no longer depends on legacy helper parity for core correctness.

### 5) Final Test Modernization + Guardrail Coverage

**Goal**
Make tests describe architecture behavior directly and guard against regression to selector-style logic.

**Plan**
- Replace remaining helper-vs-helper assertions with explicit expected outcomes.
- Add targeted invariant tests for:
  - deterministic winner resolution
  - one-winner-per-job semantics
  - observer forwarding has no side effects
  - no-pred accounting only when jobs existed and no winner matched
  - dispatch-driven resize behavior
- Keep intentional multi-event sequence regressions where stateful parity matters.

**Acceptance**
- Tests read as behavior/invariant specs for current architecture.
- Guardrail regressions are caught by dedicated tests.
- `cargo test` and `mix test` remain green.

### 6) Documentation Sync (Post-Hardening)

**Goal**
Align internals docs with finalized dispatch/runtime boundaries.

**Plan**
- Update event internals docs to reflect the two-lane model and typed-outcome application path.
- Ensure terminology is consistent (`dispatch lane`, `observer lane`, `DispatchJob`, `DispatchOutcome`).
- Keep payload contract docs stable and explicit.

**Acceptance**
- Docs match runtime architecture and naming.
- No stale references to removed helper or legacy fallback models.

## Execution Order

Recommended order:

1. Workstream 1 (compile decomposition)
2. Workstream 2 (API purity split)
3. Workstream 3 (hit-test consolidation)
4. Workstream 4 (legacy helper removal)
5. Workstream 5 (test hardening)
6. Workstream 6 (docs sync)

## Verification Strategy

Run after each workstream:

- `cargo test` in `native/emerge_skia`
- `mix test` in repo root

Priority checks:

- deterministic winner/action ordering
- pointer hit behavior
- observer forwarding semantics
- resize dispatch path
- focus transition + reveal-scroll behavior
- text cursor/command/edit/preedit correctness
- scrollbar/style runtime behavior
- no-pred accounting semantics

## Definition of Done

- Job compilation is decomposed into explicit passes with preserved ordering behavior.
- Dispatch derivation and runtime mutation boundaries are explicit and enforced.
- Hit-testing logic has a single geometric source of truth.
- Legacy helper/fallback surfaces targeted by this plan are removed.
- Tests express current architecture directly and guardrail invariants are covered.
- Internals docs match finalized runtime/dispatch boundaries.
- No selector-style dispatch logic exists outside registry/job resolution paths.
- `cargo test` and `mix test` pass.
