# Emerge 0.4.0 Commit Audit

## Scope

This audit originally reviewed the committed range from `v0.3.4` through
`088f644` (`Add packed grayscale headless output`). The candidate subsequently
landed bounded asset caching, renderer-stat memory reporting, unconditional
vector support, and retained-raster source restoration through `d88d897`.

The original history was not a normal descendant range:

- `git log v0.3.4..088f644` reports 41 commits.
- Eight commits are patch-equivalent to work already present in `v0.3.4`.
- There are 33 genuinely new commits.
- The tree comparison is approximately 42,800 additions and 12,200 deletions.
- `headless-backend` forked from `v0.3.2`; `v0.3.4` was not an ancestor.

### Ancestry reconciliation update

On 2026-09-01, `release/0.4.0-integration` was created directly from `v0.3.4`
and merged `headless-backend` through `d88d897`. Conflict resolution retained
the reviewed headless candidate where both branches had evolved the same
subsystems. The resulting tree is identical to `d88d897` except that it keeps
the corrected `/emerge-*.tar` package ignore from the published release line.

The published fixes were checked explicitly in the merged tree: local macOS
host selection, Metal frame retry and text surface properties, the DRM GLES2
baseline and optional-capability gates, macOS video dead-code configuration,
and column fill relayout are all present. The stale completed GLES2 plan was not
resurrected by the merge.

The public `main` and `v0.3.4` refs were both verified at `6fc99f6`. On the
current reconciled working tree, `./ci-tests.sh all` passes formatting,
warnings-as-errors compilation, strict Credo, Clippy, 447 Elixir tests including
the full sweep with three hardware tests excluded, 1,007 Rust unit tests plus
the benchmark fixture, and Dialyzer. Warning-free docs and unpacked-package
all-target and embedded-CPU Cargo checks also pass with published VideoInterop
dependencies.

## Executive summary

The range adds substantial and valuable functionality:

- renderer/backend selection and capability reporting;
- Wayland and DRM raster presentation;
- headless raster, OpenGL, and Vulkan output;
- canonical VideoInterop producer/consumer ownership;
- Vulkan composition for XRGB8888 and NV12;
- improved renderer diagnostics and screenshot capture;
- packed BW1 and Gray2 rendering with deterministic protected-region
  dithering;
- touch scrolling, centered-text, and Nerves cross-compilation fixes.

It is not ready to tag as 0.4.0. A clean pushed release commit and exact-tag CI
remain blockers. Public VideoInterop publication, registry-only lock and package
validation, Gray4 removal, ancestry reconciliation, package source closure,
release gating, and per-renderer asset ownership are complete. Remaining Vulkan
hardware qualification is tracked separately from the supported API contract.

## Release blockers

### 1. Public registry dependencies verified

The candidate now resolves published VideoInterop 0.1.0 directly:

- `mix.exs` declares `video_interop ~> 0.1.0` without a path override.
- `native/emerge_skia/Cargo.toml` declares `video-interop = 0.1.0` without a
  crates.io patch.
- `mix.lock` records the Hex package checksum.
- The Cargo lock records the crates.io source and checksum
  `74c9b748ac35e4feb2d5a88043fd05dd277d0ac5ccf0883901550c8eea60ce49`.

Registry-only source validation passes 1,007 Rust tests, 447 Elixir tests, and a
forced Emerge source build. Cargo metadata resolves the crate from the Cargo
registry rather than a sibling checkout.

### 2. Hex package Cargo sources and registry validation complete

`native/emerge_skia/Cargo.toml` declares five explicit benchmark targets. The
first audited package omitted `native/emerge_skia/benches/`, so Cargo rejected
the unpacked manifest before compilation.

`mix.exs` now packages the benchmark sources, native support files, the ordered
public tutorials, migration notes, and user-facing references. Maintainer-only
internal guides remain outside the Hex package. Documentation is validated from
the unpacked archive. The unpacked package resolves VideoInterop from Hex and
crates.io and completes its forced native source build without sibling paths.

The final package gate is:

```bash
mix hex.build --unpack
cd emerge-0.4.0/native/emerge_skia
cargo check --locked --no-default-features --features embedded-cpu
```

Run it without sibling paths before the final Emerge tag.

### 3. Malformed Gray4 output removed from the accepted contract

The audit found that the former `pack_gray4` implementation flattened the pixel
stream instead of restarting packing at each row. A 3x2 frame therefore declared
a 2-byte stride and 4-byte output while producing only 3 bytes.

Gray4 has now been removed from option normalization and native conversion, so
it fails during configuration instead of reaching an invalid frame or the
storage-neutral output encoder. Gray8 remains accepted for ongoing work but is
explicitly excluded from the stable 0.4 output contract. Its active plan still
requires exact alpha, ownership, and multi-row qualification before stability.

### 4. Branch ancestry reconciliation completed

The reconciled `headless-backend` history joins the published `v0.3.4` line and
the reviewed headless candidate through `d88d897`. The temporary release
worktree and redundant integration branch were removed. Patch-equivalent copies
remain in the merged history, but the release line has truthful ancestry and the
published 0.3.3/0.3.4 fixes listed above were verified in the merged tree.

Required checks before tagging:

1. Keep subsequent 0.4 work on `headless-backend` until the release commit is
   selected.
2. Repeat the passing full regression and unpacked-package source checks using
   registry dependencies after publication.
3. Verify `git merge-base --is-ancestor v0.3.4 <release-commit>` after every
   history rewrite and immediately before tagging.

## High-priority findings

### 5. Exact-tag validation now gates artifacts and publication

The artifact workflow now begins with a Rust-1.91 release validation job. It
checks exact tag, Mix/Cargo version, dated changelog, and `v0.3.4` ancestry; runs
the quality, test, full-sweep, Dialyzer, and warning-free documentation gates;
and compiles all targets plus the embedded-CPU profile from the unpacked Hex
package. NIF and macOS artifact jobs depend on that validation.

The Hex workflow repeats the exact-tag metadata, full suite, docs, and unpacked
package checks even on manual dispatch, then verifies required macOS assets
before publication. This removes the former path where a successful artifact
matrix or manual dispatch could publish unvalidated source.

The release NIF matrix also publishes minimal raster and Vulkan variants for
x86_64 and AArch64 Linux, plus minimal raster and DRM/headless OpenGL variants
for 32-bit ARM hard-float Linux. The 32-bit ARM raster artifact is the dependency-minimal
NameBadge profile.

### 6. Per-renderer asset runtime ownership completed

Every `RendererResource` now owns an `AssetRuntime`. Its worker, source policy,
source status, encoded records, registered fonts, text metrics, decoded raster
LRU, rendered vector variants, generations, and diagnostics belong only to that
renderer. Worker and backend threads enter the owning context before layout,
asset, render, and statistics work. Shutdown stops and joins only the owning
worker.

The external macOS host applies the same model per session. Asset worker
notifications carry no global destination and rerender only the owning session.
Offscreen rendering creates a temporary runtime, configures and loads its fonts,
renders, then drops it without touching live renderers.

Validation covers two concurrent headless renderers resolving the same logical
path from different roots, stopping one while the other reloads changed source
content, renderer-local font registration, and temporary offscreen font
isolation. Exact source-build CI on macOS remains the release gate for the
external-host route because Linux cross-checking lacks a macOS SDK.

### 7. Decoded raster retention is bounded; decode expansion still needs a hard cap

The #71/#72 integration landed target-sized decode, entry- and byte-bounded LRU
retention, separate encoded source metadata, checked decoded-byte accounting,
and periodic asset-memory diagnostics.

A remaining limit is maximum source dimensions/decoded pixels. Runtime file-size
limits constrain encoded bytes, not decompression expansion. This is independent
of the completed renderer-lifecycle isolation work.

### 8. Vulkan and video qualification is incomplete

The active Linux GPU and RPi5 plans still contain unaccepted routes and fault
matrices, including:

- four Wayland PRIME producer/consumer combinations;
- explicit and forced-implicit OpenGL synchronization;
- Wayland Vulkan resize, screenshot, device-loss, and multi-GPU behavior;
- DRM/Vulkan KMS restore and restart behavior;
- NV12 color/range/chroma pixel oracles;
- delayed/error fence and device-loss injection;
- long FD/RSS/cache/lease soaks.

These remaining platform checks do not change the supported Vulkan/video API
status. Complete or explicitly defer them before release, and keep hardware-
specific compatibility claims scoped to the matrices actually run.

## API and documentation findings

### 9. Screenshot migration documented

`render_to_pixels/2` and `render_to_png/2` changed from one-shot tree rendering
that returned a binary to retained renderer capture that returns
`{:ok, binary}` or `{:error, reason}`. The changelog, migration guide, and API
docs now include the removed form, new return shape, backend support, and
before/after examples.

### 10. Release documentation updated

The 0.4 entries now use one unreleased heading and include packed BW1/Gray2,
centered-text, SVG, screenshot migration, renderer selection, video API,
lifecycle, and Nerves build changes without duplicating 0.3.3/0.3.4 fixes.

The `Emerge` and viewport module documentation now cover viewport usage and
configuration, including headless mode. `EmergeSkia` documents exact renderer,
capture, diagnostics, and video contracts. Migration and native-build material
is grouped separately from the four ordered tutorials. Maintainer internals are
not included in the Hex package; all declared native benchmark sources are.

### 11. Rust version floor declared

Emerge now declares Rust 1.91 in the native Cargo manifest, documents it for
source builds, and runs Linux CI with Rust 1.91 and current stable.

### 12. Minor release hygiene

- `.gitignore` now ignores the actual `emerge-*.tar` package artifact.
- Native test support is included in the package after the explicit
  `test_support.rs` exclusion was removed.
- `Options.rendering_api_start_error/1` returns `nil` in every branch and has a
  test that only confirms the dead behavior.
- Registry-only package validation now uses the published VideoInterop Hex and Cargo artifacts.

## Commit-structure review

Several commits are too broad for reliable review and bisection:

- `f08d80b`: 92 files and about 27,000 additions;
- `c085596`: 50 files and about 8,000 additions;
- `088f644`: 46 files, combining packed output with broad cleanup.

Future backend work should be split by stable contract boundary:

1. schemas and ownership types;
2. backend implementation;
3. public Elixir API;
4. tests and fault injection;
5. documentation and hardware qualification.

The asset work present during this audit should likewise remain reviewable as
separate concerns where practical:

1. bounded target-sized raster decoding;
2. asset-memory renderer stats;
3. unconditional vector/SVG support;
4. application-level picture validation.

## Remaining execution order

1. Push the candidate and validate the per-session macOS asset route in exact source-build CI.
2. Verify the final changelog date and tag only from that clean pushed commit descended from `v0.3.4`.
3. Continue Vulkan/video hardware qualification under its dedicated platform plan.

## Audit validation performed

This audit used Git history/tree comparisons, manifest and workflow review,
public registry checks, full local CI, warning-free docs, unpacked Hex package
Cargo probes for all targets and embedded CPU, concurrent renderer asset-root
isolation, worker-shutdown isolation, renderer-local fonts, and temporary
offscreen font contexts. Registry-only source and unpacked-package native builds
use published VideoInterop artifacts. No new hardware qualification or native
macOS execution was performed as part of this audit.
