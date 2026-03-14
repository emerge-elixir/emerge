# Plan 03: Rust Runtime Boundary Split

## Goal

Make the Rust runtime easier to understand by shrinking `native/emerge_skia/src/lib.rs` and moving orchestration into explicit runtime-focused modules.

## Status

Status: not started

Dependency update:

- the Elixir cleanup baseline is now complete
- the user-facing DSL contract is stricter and more explicit
- the public boundary going into serialization/NIF paths is less ambiguous than before
- Elixir-side attr validation and tree helper boundaries are more stable than before
- no Rust runtime/module reorganization has been done yet

## Why This Plan Exists

The Rust side has reasonable domain separation at a high level, but the top-level crate entry still owns too much runtime wiring:

- resource definitions
- startup and shutdown
- backend-specific branching
- tree actor spawning
- offscreen rendering entry points
- asset lifecycle hooks
- utility tree NIFs

That makes `lib.rs` harder to reason about than it should be, and it hides the true ownership boundaries between the NIF layer and the runtime implementation.

## Main Objective

Turn `native/emerge_skia/src/lib.rs` into a thin boundary layer.

It should be easy to answer these questions from module structure alone:

- what the NIF surface exports
- what state belongs to a renderer/session
- what the tree actor owns
- how backends start and shut down
- where offscreen rendering lives

## Scope

In scope:

- Rust module reorganization
- startup/shutdown path cleanup
- channel helper extraction
- runtime ownership clarification

Out of scope:

- major behavior changes
- event/render semantic changes
- backend rewrites

## Proposed Structure

One possible layout:

```text
native/emerge_skia/src/
  lib.rs
  actors.rs
  runtime/
    mod.rs
    resource.rs
    tree_actor.rs
    channels.rs
    startup.rs
  nif/
    mod.rs
    renderer.rs
    raster.rs
    tree.rs
    video.rs
```

This exact layout is not mandatory, but the separation should be:

- NIF boundary code
- runtime/session code
- backend implementations

## Proposed Workstreams

### Workstream A: extract shared messaging helpers

Several parts of the runtime repeat similar channel-send/backpressure behavior.

Tasks:

- identify send helpers that are conceptually the same
- create a small shared helper module for non-destructive queue behavior
- keep logging and caller intent explicit so the helper does not become too generic

Benefits:

- less repeated queue logic
- easier to reason about channel behavior consistently

### Workstream B: move tree actor implementation out of `lib.rs`

Tasks:

- move tree actor config types and loop implementation into `runtime/tree_actor.rs`
- keep the actor loop responsible for tree-refresh behavior, not NIF registration details
- keep refresh decision logic close to the actor instead of in the crate root

Benefits:

- one of the most important runtime loops becomes easier to test and read
- `lib.rs` stops being the place where all runtime logic accumulates

### Workstream C: split NIF surfaces by concern

Tasks:

- move renderer/session NIFs together
- move offscreen raster NIFs together
- move tree utility/debug NIFs together
- optionally move video target NIFs together if that reads better than leaving them mixed into the main file

Benefits:

- easier to find a NIF by concern
- better separation between production-facing renderer APIs and lower-level test/debug helpers

### Workstream D: reduce inline backend startup branching

Tasks:

- move backend-specific startup plumbing into backend-owned functions or a small startup layer
- keep `lib.rs` focused on selecting a backend, not assembling all threads inline
- preserve the current one-renderer/multiple-backends direction while making backend launch code easier to follow

Benefits:

- clearer startup story
- fewer unrelated details inside the crate entrypoint

## Suggested Sequence

1. Extract messaging helpers and tree actor module.
2. Move NIFs into concern-based modules.
3. Move backend startup wiring behind clearer functions.
4. Re-evaluate `lib.rs` and trim anything still acting as overflow logic.
5. Run Rust and Elixir test suites.

## Files Most Likely To Change

- `native/emerge_skia/src/lib.rs`
- `native/emerge_skia/src/actors.rs`
- `native/emerge_skia/src/backend/wayland.rs`
- `native/emerge_skia/src/backend/drm.rs`
- `native/emerge_skia/src/assets.rs`
- new `native/emerge_skia/src/runtime/*`
- new `native/emerge_skia/src/nif/*`

## Risks

- most of the risk is mechanical rather than semantic, but startup-order bugs are still possible
- if the split is partial, code may end up duplicated between old and new boundaries
- too much abstraction around backend startup can make boot flow harder to follow rather than easier

## Validation Questions

- Should tree utility/debug NIFs remain in the main native module or move into a more clearly internal namespace?
- Should asset lifecycle remain global for now, even after runtime ownership is reorganized?
- Is there value in introducing a small renderer/session object module on the Elixir side to mirror the Rust runtime split later?

## Done Means

This plan is done when:

- `native/emerge_skia/src/lib.rs` is mostly boundary wiring and exports
- the tree actor has a clear home and can be understood without scrolling through unrelated NIF functions
- backend startup paths are easier to read and compare
- contributors can find runtime/session code without treating `lib.rs` as the answer to everything
