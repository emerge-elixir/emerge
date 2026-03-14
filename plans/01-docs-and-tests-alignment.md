# Plan 01: Docs And Tests Alignment

## Goal

Make the documentation describe the code as it exists now, and make the test layout reflect current boundaries instead of earlier architecture.

## Status

Status: partially completed

Completed already as part of the finished Elixir cleanup baseline:

- README constructor semantics were updated
- `Emerge.UI` docs now describe `text/1` as a standalone content element
- core Elixir tests were migrated away from shorthand container calls
- attrs-first constructor ordering is now reflected in user-facing examples
- weighted fill is now documented canonically as `{:fill, n}`

Remaining work still includes the larger internals-docs pass, ExDoc extras cleanup, roadmap cleanup, and broader test organization work.

## Why This Plan Exists

The codebase has already moved through major refactors, but the written documentation still mixes current behavior, old behavior, and future intent.

That causes two problems:

- contributors can build the wrong mental model before reading the code
- cleanup discussions are harder because docs and implementation are talking past each other

This is the least risky cleanup plan because it mostly improves clarity without changing runtime behavior.

## Current Pain Points

### User-facing docs

- `README.md` is in better shape after the DSL cleanup, but it still needs a broader review as the primary entry point.
- The README layout overview can still be expanded for the current API surface.
- Public docs should now consistently describe explicit `2`-arity container constructors, attrs-first image/video/input constructors, and `text/1` as standalone content.

### Internal guides

- `guides/internals/events.md` still refers to removed interaction concepts.
- `guides/internals/assets-images.md` still uses older version framing.
- `guides/internals/architecture.md` still uses some older naming and ownership descriptions.
- `guides/internals/feature-roadmap.md` mixes roadmap content with stale implementation status.
- `guides/internals/nearby-semantics.md` exists, but is not currently published through ExDoc extras.

### Tests

- Core Elixir tests were already updated for the explicit container API.
- Some test areas now follow the intended public contract more clearly, but overall suite organization is still uneven.
- Top-level tests such as `test/tree_test.exs` and `test/emrg_roundtrip_test.exs` are useful, but they still sit outside a clear grouping model.
- Some low-level tests hand-build EMRG binaries, which is useful coverage, but should be a deliberate choice rather than leftover structure.

## Scope

In scope:

- docs cleanup
- ExDoc extras cleanup
- test organization and helper extraction
- naming cleanup inside docs/tests

Out of scope:

- runtime behavior changes
- public API redesign
- deep Rust architecture changes

## Proposed Workstreams

### Workstream A: README stabilization

Update `README.md` so it is safe as the primary onboarding document.

Tasks:

- fix the quick-start example so it is valid Elixir
- expand the high-level layout overview to reflect current public primitives
- add short, accurate explanations for:
  - offscreen rendering
  - current asset behavior
  - input targeting and masks if those remain public-facing
  - video support if it is intended to remain discoverable

Success criteria:

- the example reads cleanly and is mechanically correct
- the README does not imply older API constraints that no longer exist

### Workstream B: Internal architecture docs refresh

Update internal guides so they describe the current runtime model.

Tasks:

- rewrite stale ownership language in `guides/internals/architecture.md`
- replace removed interaction references in `guides/internals/events.md`
- update asset guide language to current EMRG/runtime behavior in `guides/internals/assets-images.md`
- decide whether `guides/internals/feature-roadmap.md` is:
  - a true roadmap, or
  - a current implementation status page

Success criteria:

- no guide points to removed files or modules
- guide terminology matches current code structure
- architecture diagrams and module lists are still true after reading the code

### Workstream C: Publish missing useful docs

Tasks:

- add `guides/internals/nearby-semantics.md` to `mix.exs` extras
- consider adding a short guide for:
  - offscreen `asset_mode`
  - video/DRM Prime integration
  - user-facing input model

Success criteria:

- important current concepts are accessible from generated docs, not only from source files

### Workstream D: Test layout normalization

Tasks:

- decide which tests are truly public API tests and which are native-boundary tests
- extract repeated EMRG-binary helper code if it remains useful in multiple places
- move or regroup top-level tests into a more intentional layout if that improves discoverability
- move large inline test modules in Rust to sibling test files where it helps readability

Possible target shape:

- Elixir public/API tests under domain folders
- native boundary/wire-format tests clearly labeled as such
- Rust internal tests following the newer `tree/layout/tests` and `tree/render/tests` pattern when practical

Success criteria:

- a contributor can tell where to add a new test without scanning the entire repo
- low-level tests remain clearly low-level instead of looking like ordinary API tests

## Suggested Sequence

1. Fix README and ExDoc extras.
2. Refresh internal architecture/events/assets guides.
3. Decide how to treat `feature-roadmap.md`.
4. Normalize test layout and helpers.
5. Run docs generation and test suite to ensure nothing drifted during the cleanup.

## Risks

- overediting docs can accidentally overpromise intended future changes as current behavior
- over-normalizing tests can hide useful low-level coverage behind too many helpers
- if terminology is changed in docs but not code comments, a smaller internal drift can remain

## Validation Questions

- Should low-level EMRG tests continue to explicitly target compatibility paths, including older decode behavior?
- Should README mention all public capabilities, or only the most stable ones?
- Is the intended audience for internal guides current contributors only, or also advanced users of the library?

## Done Means

This plan is done when:

- docs no longer describe removed modules or stale behavior
- important guides are published in ExDoc
- test layout is easier to navigate than it is today
- the repo's written architecture is good enough that a deep code read is optional, not mandatory, for orientation
