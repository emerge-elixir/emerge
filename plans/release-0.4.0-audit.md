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
warnings-as-errors compilation, strict Credo, Clippy, 438 Elixir tests including
the full sweep with three hardware tests excluded, 1,005 Rust tests plus the
benchmark fixture, and Dialyzer. Warning-free docs and unpacked-package
all-target and embedded-CPU Cargo checks also pass with the coordinated sibling
VideoInterop source. Registry-only validation remains blocked by publication.

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

It is not ready to tag as 0.4.0. Public dependency publication, registry-only
lock/package validation, exact release dates, and a clean pushed release commit
remain blockers. Gray4 removal, ancestry reconciliation, package source closure,
and release gating are complete. Vulkan hardware and multi-renderer lifecycle
qualification remain open.

## Release blockers

### 1. Public registry dependencies are unavailable

The candidate depends on packages that are not published:

- `mix.exs` declares `video_interop ~> 0.1.0` unless
  `VIDEO_INTEROP_PATH` supplies a sibling checkout.
- `native/emerge_skia/Cargo.toml` declares `video-interop = 0.1.0` and
  patches crates.io to `../../../video_interop/rust/video-interop`.
- `mix.lock` has no `video_interop` entry.
- The Cargo lock records the path package without a registry source/checksum.

Registry checks at audit time returned no `video_interop` Hex package and no
`video-interop` crate. A clean `mix deps.get` failed for that reason.

Before release:

1. Publish the Rust crate.
2. Publish the Elixir package.
3. Remove the Cargo path patch.
4. Regenerate both locks from registry sources.
5. Build in a clean checkout without sibling repositories or path variables.

### 2. Hex package Cargo sources corrected; registry validation remains blocked

`native/emerge_skia/Cargo.toml` declares five explicit benchmark targets. The
first audited package omitted `native/emerge_skia/benches/`, so Cargo rejected
the unpacked manifest before compilation.

`mix.exs` now packages the benchmark sources, native support files, the ordered
public tutorials, migration notes, and user-facing references. Maintainer-only
internal guides remain outside the Hex package. Documentation is validated from
the unpacked archive. Registry-only native compilation remains blocked until the
published `video-interop` crate replaces the coordinated sibling path patch.

The final package gate remains:

```bash
mix hex.build --unpack
cd emerge-0.4.0/native/emerge_skia
cargo check --locked --no-default-features --features embedded-cpu
```

Run it without sibling paths after VideoInterop publication.

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

### 6. Asset runtime ownership is incompatible with multiple native renderers

The public viewport guide says multiple windows may run concurrently, but the
native asset subsystem is process-global:

- starting a renderer stops the existing asset worker;
- starting a renderer clears shared source status;
- `configure_assets_nif` accepts a renderer resource but ignores it;
- stopping any renderer stops global assets and clears global caches;
- worker thread handles are discarded rather than joined.

A second Wayland or headless renderer can therefore replace the first
renderer's asset configuration and rerender destination. Stopping either
renderer can invalidate the other. A stale worker can also outlive one renderer
lifetime and mutate state shared with the next.

Move source status, configuration, worker ownership, and tree notifications into
a per-`RendererResource` asset runtime. A decoded payload cache may remain
process-wide if it is separately bounded, generation-safe, and independent of
renderer lifecycle.

Add tests that:

- run two native headless renderers with different asset roots;
- load delayed assets in both;
- stop one renderer while the other remains active;
- restart a renderer while prior asset work is queued;
- verify no stale status, cache invalidation, or rerender routing crosses the
  renderer boundary.

### 7. Decoded raster retention is bounded; decode expansion still needs a hard cap

The #71/#72 integration landed target-sized decode, entry- and byte-bounded LRU
retention, separate encoded source metadata, checked decoded-byte accounting,
and periodic asset-memory diagnostics.

A remaining limit is maximum source dimensions/decoded pixels. Runtime file-size
limits constrain encoded bytes, not decompression expansion. Keep this residual
work with the multi-renderer lifecycle tests above.

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

Either complete these gates before calling the paths stable, or clearly mark the
Vulkan/video functionality experimental in 0.4 and exclude it from compatibility
claims until qualification is complete.

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
- Registry-only package validation still depends on VideoInterop publication.

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

1. Publish VideoInterop dependencies and regenerate locks.
2. Repeat the unpacked Hex package checks from registry-only sources.
3. Resolve or explicitly defer per-renderer asset runtime ownership.
4. Complete or explicitly defer Vulkan/video hardware qualification.
5. Set final changelog dates and tag only from a clean, pushed commit descended
   from `v0.3.4`.

## Audit validation performed

This audit used Git history/tree comparisons, manifest and workflow review,
public registry checks, full local CI, warning-free docs, and unpacked Hex
package Cargo probes for all targets and embedded CPU. The final probes still
use the coordinated sibling VideoInterop source because the packages are not
published. No new hardware qualification was performed as part of this audit.
