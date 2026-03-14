# Plan 04: Refresh Pipeline Separation

## Goal

Clarify the refresh pipeline by separating visual rendering, event rebuild work, and shared retained-state semantics.

## Status

Status: not started

Dependency update:

- the Elixir cleanup baseline is now complete
- the public Elixir DSL contract is now clearer and less ambiguous
- API-shape churn and helper-boundary churn have been reduced before deeper render/event cleanup
- no refresh-pipeline separation work has been started yet

## Why This Plan Exists

This is the deepest cleanup plan.

The current code already has better structure than before, but some key responsibilities are still entangled:

- render traversal still produces event rebuild output
- event rebuild logic is concentrated in a very large registry builder
- some shared logic is duplicated across tree, render, and event code

This plan is not about adding layers.

It is about making existing responsibilities easier to follow and ensuring each important traversal has a clear purpose.

## Important Constraint

This plan should happen only after the codebase is easier to navigate from docs and module structure.

It touches core behavior and therefore has the highest regression risk.

## Scope

In scope:

- refresh output boundaries
- render/event traversal ownership
- registry-builder modularization
- duplicated helper extraction in core Rust modules

Out of scope:

- changing public semantics without an explicit design decision
- introducing compatibility wrappers just to preserve old internal shapes

## Guiding Principles

- paint order and event precedence should still derive from the same retained model
- one source of truth is still the goal, but one source of truth does not require one oversized traversal module
- render code should read like render code
- event rebuild code should read like event rebuild code
- shared semantics should be factored deliberately, not smeared across files

## Proposed Workstreams

### Workstream A: extract duplicated low-level helpers

Before changing refresh boundaries, remove clear duplication that already muddies the code.

Targets:

- binary cursor logic used by deserialize and patch decode
- shared font-weight parsing used by layout and event-side text metadata
- text-length helpers used by text input, render text, and runtime logic
- scroll limit/clamp helpers used in tree mutation, layout, and event rebuild logic

Benefits:

- less noise while evaluating the larger architecture
- lower drift risk in core semantics

### Workstream B: modularize registry builder by concern

`events/registry_builder.rs` is currently too large to read as one unit.

Potential split themes:

- registry storage and views
- pointer/input matchers
- scroll listener assembly
- focus traversal and reveal behavior
- text input listener assembly
- runtime overlay registry composition

The exact split should follow real concern boundaries, not arbitrary line counts.

Benefits:

- easier to reason about event behavior by domain
- smaller review surface for future event changes

### Workstream C: define a clearer refresh output boundary

Introduce a first-class refresh output type representing what the tree actor publishes after layout/refresh.

Possible contents:

- draw commands
- event rebuild payload
- animation state
- IME metadata

This does not require a single merged consumer path, but it should make refresh results explicit rather than implicit.

Benefits:

- clearer tree actor output contract
- cleaner handoff to event actor and render backend code

### Workstream D: separate render traversal from event rebuild traversal

This is the central architectural step.

Possible direction:

- keep shared retained ordering and scene semantics
- allow rendering to build only render output
- allow event rebuild code to walk the retained tree using the same ordering rules without being embedded in render scope construction

This does not necessarily mean two completely independent implementations.

It means each traversal should have a focused job.

Benefits:

- less coupling between paint concerns and input concerns
- easier future changes to either rendering or event logic

### Workstream E: reevaluate runtime state placement only after the above

Only after the refresh boundary is clearer, reassess whether some mutable runtime state still belongs in `Attrs` or should move elsewhere.

This should be treated as optional follow-up, not part of the first pass.

## Suggested Sequence

1. Extract duplicated helpers.
2. Split registry builder into real submodules.
3. Introduce an explicit refresh/frame output type.
4. Separate render traversal from event rebuild traversal.
5. Reassess optional deeper cleanup such as runtime state placement or renderer submodule splits.

## Files Most Likely To Change

- `native/emerge_skia/src/tree/render.rs`
- `native/emerge_skia/src/tree/layout.rs`
- `native/emerge_skia/src/events/registry_builder.rs`
- `native/emerge_skia/src/events/runtime.rs`
- `native/emerge_skia/src/renderer.rs`
- `native/emerge_skia/src/tree/deserialize.rs`
- `native/emerge_skia/src/tree/patch.rs`

## Risks

- precedence or hit-testing behavior can change subtly if shared ordering rules are not preserved exactly
- hover/focus/text-input rebuild semantics are easy to regress if traversal assumptions move
- a badly designed shared abstraction can make the code less readable than it is now

## Validation Questions

- Is the current tight coupling between render traversal and rebuild accumulation still delivering enough value to justify its complexity?
- Which semantics must be snapshot-tested before this work starts?
- Should the first step introduce shared retained traversal utilities, or should each traversal remain explicit and parallel as long as ordering rules are documented once?

## Done Means

This plan is done when:

- render code no longer feels responsible for event system assembly
- registry builder is readable by domain instead of as one giant file
- the tree actor publishes an explicit refresh result
- shared retained semantics are still singular and obvious
- future work on rendering or events can happen with less fear of collateral breakage
