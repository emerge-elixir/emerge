# Emerge 0.4.0 Commit Audit

## Scope

This audit reviews the committed range from `v0.3.4` through `088f644`
(`Add packed grayscale headless output`). Candidate asset-cache, memory-log, and
unconditional-vector changes that were still in the working tree at audit time
are noted separately where they mitigate a finding.

The history is not a normal descendant range:

- `git log v0.3.4..088f644` reports 41 commits.
- Eight commits are patch-equivalent to work already present in `v0.3.4`.
- There are 33 genuinely new commits.
- The tree comparison is approximately 42,800 additions and 12,200 deletions.
- `headless-backend` forked from `v0.3.2`; `v0.3.4` is not an ancestor.

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

It is not ready to tag as 0.4.0. Public dependency publication, package source
builds, Gray4 correctness, ancestry reconciliation, and release gating remain
blockers. Vulkan and multi-renderer lifecycle qualification also remain open.

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

### 2. The Hex package contains an invalid Cargo project

`native/emerge_skia/Cargo.toml` declares five explicit benchmark targets:

- `layout`
- `patch`
- `emrg`
- `renderer`
- `stats`

`mix.exs` packages native `src/` and support files but not
`native/emerge_skia/benches/`.

The exact package check was:

```bash
mix hex.build --unpack
cd emerge-0.4.0/native/emerge_skia
cargo check --locked --no-default-features --features embedded-cpu
```

It failed because all five declared benchmark source files were absent. After
supplying those files manually, Cargo reached the next blocker and failed on
the missing sibling `video-interop` path.

Either include the benchmark source tree in the Hex package or remove the
explicit benchmark declarations from the published manifest. Package
validation must compile the unpacked package; `mix hex.build --unpack` alone is
not sufficient.

### 3. Advertised Gray4 output is malformed for odd multi-row frames

`EmergeSkia` accepts and documents `headless.pixel_format: :gray4`, but the
committed `pack_gray4` implementation packs one flattened pixel stream. It does
not restart packing at each row boundary.

For a 3x2 frame:

- declared stride is 2 bytes;
- required output is 4 bytes;
- committed output is 3 bytes.

The Gray4/Gray8 active plan also states that these formats remain unfinished.
Gray4 and Gray8 should be removed from the accepted public 0.4 contract until
they have exact row, tail, alpha, ownership, and multi-row tests, or the active
plan should be completed before release.

### 4. The branch is not descended from v0.3.4

A merge trial between `088f644` and `v0.3.4` reported conflicts across release
workflows, manifests, renderer code, DRM, macOS, stats, tests, and plans. The
candidate also carries patch-equivalent copies of earlier commits while lacking
the published release ancestry.

Before tagging:

1. Create a clean integration branch from current `origin/main`/`v0.3.4`.
2. Merge or rebase the genuinely new work.
3. Resolve the published 0.3.3/0.3.4 fixes explicitly rather than relying on
   similar later patches.
4. Run regression and package tests on the resulting descendant.
5. Verify `git merge-base --is-ancestor v0.3.4 <release-commit>`.

## High-priority findings

### 5. Release tags can publish without running CI

The normal CI workflow runs on pull requests and pushes to `main`/`master`, not
tag pushes. The tag-triggered artifact workflow builds archives but does not run
Mix tests, Rust tests, Clippy, Dialyzer, docs, full feature checks, or unpacked
package compilation.

The Hex workflow can automatically publish after the artifact workflow
succeeds. This makes a successful build matrix, rather than a successful release
test suite, the publication gate.

Require an exact-tag validation job before publication. It should run:

```bash
./ci-tests.sh all
mix test --include full_sweep
mix docs
mix hex.build --unpack
```

It should also compile the unpacked package and run the supported Cargo feature
matrix. The publishing workflow should depend on that result and verify all NIF
and macOS artifacts.

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

### 7. Committed raster decoding and retention are unbounded

At the audited commit, raster insertion decodes the complete source image and
stores it in an unbounded process-global map. A small compressed file may decode
to a very large raster, and a long-lived process can retain every encountered
asset until global shutdown.

The candidate #71/#72 working-tree integration mitigates this with:

- target-sized decode;
- entry- and byte-bounded LRU retention;
- separate encoded source metadata;
- checked decoded byte accounting;
- opt-in asset-memory diagnostics.

That work should land before release, with additional limits for maximum source
dimensions/decoded pixels and with the multi-renderer lifecycle tests above.
Runtime file-size limits constrain encoded bytes, not decompression expansion.

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

### 9. Screenshot migration is breaking but under-documented

`render_to_pixels/2` and `render_to_png/2` changed from one-shot tree rendering
that returned a binary to retained renderer capture that returns
`{:ok, binary}` or `{:error, reason}`. The changelog mentions on-demand capture
but not the removed tree form or changed return shape.

Prefer retaining the old tree clauses as deprecated wrappers around
`TreeRenderer`. Otherwise add a dedicated 0.3-to-0.4 migration section with
before/after examples and a complete list of public return-shape changes.

### 10. Release notes are stale

The committed 0.4 changelog date predates later commits and omits packed
BW1/Gray2 output and the centered-text fix. The existing release audit also
predated the accepted BW1/Gray2 correction and used the old commit count.

Before release:

- use an `Unreleased` heading until the tag date is known;
- document headless grayscale contracts and limitations;
- document screenshot migration;
- document the VideoInterop cold-restart/upgrade requirement;
- remove duplicated fixes already released in 0.3.4;
- update the setup guide for headless and Vulkan selection.

### 11. Rust version requirements are undeclared

The sibling `video-interop` crate declares Rust 1.91, while Emerge has no
`rust-version` and tells users only to install a Rust toolchain. CI follows
floating `stable`.

Set `rust-version = "1.91"`, document it, test the minimum version, and retain a
second latest-stable job for forward compatibility.

### 12. Minor release hygiene

- `.gitignore` ignores `emerge_skia-*.tar`, but `mix hex.build` creates
  `emerge-*.tar`.
- Native test support is included in the package after the explicit
  `test_support.rs` exclusion was removed.
- `Options.rendering_api_start_error/1` returns `nil` in every branch and has a
  test that only confirms the dead behavior.
- Public headless/video setup guidance is much smaller than the implementation
  surface and currently depends heavily on internal plans.

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
2. memory diagnostics and protocol transport;
3. unconditional vector/SVG support;
4. application-level picture validation.

## Suggested execution order

1. Reconcile branch ancestry on a clean integration branch.
2. Publish VideoInterop dependencies and regenerate locks.
3. make the unpacked Hex package compile from registry-only sources.
4. Remove or complete Gray4/Gray8.
5. Land bounded target-sized raster decoding.
6. Make asset runtime ownership per renderer.
7. Add exact-tag release CI and feature-matrix gates.
8. Complete or explicitly defer Vulkan/video hardware qualification.
9. Add migration documentation, MSRV, and final changelog entries.
10. Tag only from a clean, pushed commit descended from `v0.3.4`.

## Audit validation performed

This audit used Git history/tree comparisons, manifest and workflow review,
public registry checks, and an exact `mix hex.build --unpack` package probe. The
unpacked Cargo source-build probe exposed the missing benchmark files and then
the sibling path dependency. No hardware qualification was performed as part of
this audit.
