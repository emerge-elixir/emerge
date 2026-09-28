# Emerge 0.4.0 release readiness

Updated: 2026-09-28 (UTC).
Baseline: `976e0585f08ded95e8a7e2444883317f9ed13c9e`, `headless-backend`, followed
by the local release-preparation commit sequence described below.

**Do not tag yet.** The known repository defects are fixed locally. Final pushed
CI/artifact qualification, default-branch integration, release scope/approval,
and the final release date remain outstanding. Preparation commits were requested
separately after validation; nothing was pushed, merged, tagged, or published.
Hardware was not exercised.

This record supersedes the September 1 release audits; those are historical,
not acceptance for this candidate. Local fixes are not evidence that the new
workflow has run successfully on GitHub or on macOS.

## Verified baseline (original audit)

- Mix/Cargo: `0.4.0`; Rust floor: 1.91; `v0.3.4` is an ancestor.
- 36 commits since beta.1; published stable/prerelease: 0.3.4/0.4.0-beta.1.
- [PR #73](https://github.com/emerge-elixir/emerge/pull/73) was open;
  protected `main` remained `6fc99f6` (v0.3.4).
- [PR CI](https://github.com/emerge-elixir/emerge/actions/runs/36252627692)
  passed Linux Rust floor/stable and macOS for the original candidate, not these
  subsequent preparation changes.
- Elixir/Rust VideoInterop 0.1.2 are published, registry-backed dependencies.
  Skia remains pinned to `0d2261c63941f4b534522246cc1ace13ca4242d8`.
- The latest successful artifact run found was
  [beta.1](https://github.com/emerge-elixir/emerge/actions/runs/33960489933).
  Earlier musl/RISC-V recipe evidence is in
  `plans/nerves-embedded-build-investigation.md`; it does not qualify this revision.

## Commit organization

1. Enforce the no-Python policy in `AGENTS.md`; replace existing benchmark tools
   with standalone Elixir, formatter coverage and ExUnit regressions.
2. Update native dependencies and the lockfile to resolve advisory findings.
3. Close package assets/benchmarks, document redistribution, and add tested Elixir
   package/release checks.
4. Correct public migration, renderer recovery, and platform-support documentation.
5. Gate release automation on exact-commit validation.
6. Consolidate the user-facing 0.4.0 draft changelog without changing older history.
7. Record readiness evidence, historical-audit supersession, and remaining gates.

## Repository fixes

- [x] Ban repository-owned Python code in `AGENTS.md` and enforce it with ExUnit.
  Replace the new release helpers and all four existing Python benchmark scripts
  with standalone Elixir. Preserve measurement matrices, exclusive-lock checks,
  source/binary snapshots, per-process isolation and summary semantics. Add
  formatter coverage and 15 ExUnit tests; upstream Skia build-tool dependencies
  are unchanged.
- [x] Consolidate user-facing release notes since stable 0.3.4, preserve older
  history, use one sentence for input fixes, and omit the rejected protocol/
  coordinated-upgrade bullet. The date and draft notice are intentionally pending.
- [x] Correct macOS video support in `README.md`, `lib/emerge_skia.ex`, and the
  migration guide: owned single-plane RGBA8888 binary frames, implicit sync,
  premultiplied/straight/opaque alpha; no DMA-BUF/PRIME or retained capture.
- [x] Document list-based `Animation.change/3`, animated min/max rejection,
  shadow/glow visual changes, and native viewport recovery. Explain stable
  callback identity/state, replaceable/unavailable renderer handles, consumed
  frames on unavailable endpoints, stop/status semantics, and platform limits.
- [x] Remove stale host-v13 upgrade prose from the migration guide rather than
  maintaining brittle protocol-version instructions in user-facing migration text.
  Protocol references remain in the implementation/internals; no changelog
  protocol/upgrade warning was reintroduced.
- [x] Replace the broken public `assets-images.html` link with its repository URL.
- [x] Package all sample assets, benchmark sources/fixtures/helpers, the
  `gradient_mask.svg` fixture, and all root screenshot PNGs. Include scripts by
  Elixir/shell source extension, not generated outputs. The package still excludes native build
  output and maintainer-only internal guides.
- [x] Add archive-content/byte-equality regressions, source-photo hash checks,
  and fresh consumer build-path isolation to the package tests.
- [x] Add `scripts/check-package.sh`: unpack the actual Hex archive, compile
  all targets with `bench-diagnostics` and the embedded CPU configuration,
  force a source NIF build, check all screenshots **before** regeneration, and
  build warning-free public docs with local-link validation.
- [x] Verify packaged-photo provenance and align `NOTICE`, `THIRD_PARTY_ASSETS.md`,
  `priv/sample_assets/SOURCES.md`, and the README. Include the Unsplash license
  text/restrictions and existing Tabler MIT license. JPEG bytes are unchanged.
- [x] Update patched Rust dependencies and remove the unmaintained font stack;
  no advisory suppression or maintenance exception was needed.
- [x] Gate artifact and manual/automatic Hex publication on reusable exact-SHA
  Linux Rust floor/stable, Elixir-minimum, and macOS CI. Keep existing floor/stable
  job names. Add dependency audits, package checks, and release-helper tests to CI.
- [x] Stage every build as an Actions artifact; only publish after the entire
  matrix and source validation pass. Create a draft, upload the complete set,
  then expose it. Never overwrite an existing release. Fail missing-artifact
  uploads, reject moved tags/unfinalized release notes, and require the tagged
  commit to be integrated into the default branch.
- [x] Resolve manual/automatic Hex refs once to immutable SHAs; recheck tag
  identity before publishing. Generate checksums from release downloads and
  assert the checksum file is present in the assembled Hex archive.
- [x] Complete final local regression rerun, minimum-Elixir consumer tests,
  Rust-floor all-target checking, and DRM-all warning-denied Clippy.

### Photo evidence

Fresh downloads on 2026-09-28 matched the vendored JPEGs byte-for-byte:

| File | Photographer / original | Download |
|---|---|---|
| `static.jpg` | Andrew Ridley, https://unsplash.com/photos/Kt5hRENuotI | https://picsum.photos/id/1018/640/420 |
| `fallback.jpg` | Christian Joudrey, https://unsplash.com/photos/mWRR1xj95hg | https://picsum.photos/id/1043/640/420 |

Picsum metadata supplies the photographer/original links. Full hashes and
source URLs are recorded in `priv/sample_assets/SOURCES.md`. The
[Unsplash License](https://unsplash.com/license) permits redistribution, including
commercial use, while restricting unmodified photo sales and competing image
services; attribution is optional. No claim of an earlier unlawful use was made.

### Dependency disposition

The original audit found dev/build advisories, a runtime unsoundness warning,
and two maintenance warnings. These are fixed without treating every finding as
an exploitable production vulnerability:

| Dependency | Change / disposition |
|---|---|
| `crossbeam-epoch` | 0.9.18 → 0.9.21; Criterion/Rayon dev path, RUSTSEC-2026-0204 |
| `wayland-scanner` / `quick-xml` | 0.31.9 → 0.31.11 / 0.39.2 → 0.41.0; build/proc-macro path, RUSTSEC-2026-0194/0195 |
| `memmap2` | 0.9.10 → 0.9.11; fixes RUSTSEC-2026-0186 runtime unsoundness warning |
| `resvg` / `usvg` | 0.47 → 0.48.1; uses harfrust/fontdb 0.24 rather than rustybuzz/ttf-parser, removing RUSTSEC-2026-0206/0192 maintenance warnings |

Cargo audit 0.22.2 reports **zero vulnerabilities and zero warnings**, including
with `--deny warnings`. Rust 1.91 source checking passes with this lockfile.
No native renderer API/code adaptation was required. Cross-target artifact builds
still need to be run against the final commit and updated dependencies.

## Local validation

Original audit evidence: `/tmp/emerge-0.4.0-audit/`.
Fix/validation logs: `/tmp/emerge-release-fixes/`; Elixir-only conversion recheck:
`/tmp/emerge-elixir-automation/` (all ephemeral).

| Check | Result |
|---|---|
| `./ci-tests.sh all` | PASS: format, warnings-as-errors compile, strict Credo, Clippy, Dialyzer |
| Full-sweep Elixir tests, final rerun | PASS: 564 tests/doctests, four hardware tests excluded |
| Default `cargo test --release` | PASS: 1,482 unit + 14 integration tests |
| DRM-all `cargo test --release --no-default-features --features drm-all` | PASS: 1,534 unit + 14 integration tests; two hardware tests ignored |
| Rust 1.91 `cargo check --locked` and `--all-targets --features bench-diagnostics` | PASS (host Cargo targets, not the cross-architecture artifact matrix) |
| DRM-all warning-denied Clippy | PASS |
| `cargo fmt -- --check` | PASS |
| Repository screenshot check and warning-free public docs | PASS: 31 screenshots; 44 HTML pages with no broken local file links |
| `bash scripts/check-package.sh` | PASS: feature-enabled benches, embedded CPU sources, forced-source NIF, all 31 byte-identical screenshots, warning-free docs/local links from the unpacked archive |
| Cargo audit 0.22.2 `--deny warnings` / `mix hex.audit` | PASS, no exceptions |
| Actionlint 1.7.12 / shell syntax / Elixir helper tests | PASS; 15 new ExUnit tests included in the full suite |
| Elixir benchmark CLI synthetic smoke | PASS: fake Cargo/probe executables, 90 event-pressure and 18 animation-closeout processes, source archives, separate build diagnostics and summaries. Not a performance measurement |
| Elixir 1.19.0 minimum | PASS: 564 tests/doctests including the unpacked consumer; four hardware tests excluded. Locally tested on OTP 29.0.5; the new CI pair uses OTP 28.0.2 |

The initial local minimum-version test used a global Hex archive compiled under
Elixir 1.20, failing on `Enum.__in__/2`. A separate minimum-version Hex archive is
used for the successful retry; this was tooling compatibility, not a project runtime failure.
Fresh consumer tests also clear inherited build/dependency paths to prevent
accidental reuse of the test runner's compiled application.

## Still required before tagging/publication

- [ ] Confirm actual release date and remove the `CHANGELOG.md` draft notice.
  `scripts/release-notes.exs` deliberately refuses draft/undated/mismatched notes.
- [ ] Push/review the changes and run the expanded source CI on the final commit,
  including macOS and Elixir 1.19.0/OTP 28.0.2. Local Linux results do not replace it.
- [ ] Merge approved changes/workflows into protected `main` before tagging and
  revalidate the final SHA. `workflow_run` uses the default-branch definition,
  not whichever workflow happens to exist in the checked-out source tree.
- [ ] Confirm maintainer access, `HEX_API_KEY`, tag/release permissions and any
  approval/protection requirements. These settings were not inspected or changed.
  The Hex workflow publishes automatically after its checks; configure a protected
  approval environment before tagging if staged maintainer inspection is required.
- [ ] Run an artifact-workflow **branch dispatch** before tagging. It validates
  sources and stages Actions artifacts without publishing GitHub/Hex releases.
- [ ] Inspect all 24 builds for architecture, hard-float/libc ABI, font closure,
  raster GPU-dependency exclusion, Vulkan-only OpenGL exclusion and dynamic loading.
- [ ] Decide and record the hardware/performance scope below, performing required
  smokes or explicitly limiting claims where evidence is absent.

### Expected artifact/consumer gates

Require **22 Linux NIF archives**, NIF ABI **2.15**:

| Target | Profiles |
|---|---|
| x86_64 GNU, AArch64 GNU | 8 each: Wayland/OpenGL default, DRM, DRM/Wayland, raster, Vulkan/OpenGL, Wayland-Vulkan, DRM-Vulkan, headless-Vulkan |
| ARMv7 hard-float GNU, x86_64 musl, RISC-V64 GNU | 2 each: raster default, DRM/headless OpenGL |

Plus both macOS host archives and their SHA-256 sidecars: **26 release assets**
from 24 build jobs. Do not reuse beta.1 artifacts or hashes.

After authorized tagging, generate the 22-entry
`checksum-Elixir.EmergeSkia.Native.exs` from the actual published archives in
checksum-only mode. It remains intentionally untracked locally. Verify it is in
the final Hex archive; test a fresh consumer/cache without Rustler and explicit
forced-source fallback with Rustler. Exercise both macOS host downloads, hashes
and handshakes. A failed draft/upload requires deliberate maintainer recovery,
not replacement of public archives after checksums have been published.

## Qualification to complete or explicitly defer

These are not all mandatory redesigns for 0.4.0. The release owner must approve
an explicit scope/deferral; this pass does not silently waive hardware gates.

- Actual macOS Metal/raster gradients, animation, per-session assets and owned
  RGBA playback. Source CI is not live presentation qualification.
- Four Wayland PRIME routes, sync/fence errors, Vulkan lifecycle/device loss,
  multi-GPU, DRM restoration and FD/RSS/lease soaks:
  `plans/active-linux-gpu-qualification.md`.
- NV12 color/range/chroma oracles and sustained RPi5 Camera 60 FPS:
  `plans/active-rpi5-camera-60fps.md`. Do not claim the target as achieved.
- Trellis, x86_64 musl and MangoPi Nerves package selection/loading; constrained
  image/SVG memory and animation/gradient cadence: the embedded, picture-memory,
  low-resource-animation and gradient plans.
- FP3 long title-slide soak with `68bf225`:
  `plans/fp3-long-running-animation-investigation.md`. The phone incident is
  not confirmed fixed by the local GBM handle-reuse reproduction/fix.
- Existing screenshot-benchmark coverage and cached/direct text/shadow pixel
  differences: `plans/screenshot-cache-benchmark-investigation.md`. Package docs
  screenshot equivalence does **not** certify the entire renderer benchmark suite.
- Maximum decoded image dimensions/pixels remain uncapped. Encoded-file/cache
  limits do not prove hostile-asset/decompression safety.
- Gray8 remains outside stable 0.4 output; Gray4 remains unsupported.

## Final release sequence

1. Finish final local/pushed validation and approve hardware/release scope.
2. Run the complete non-publishing artifact dry-run; inspect and smoke the results.
3. Finalize notes/date, merge approved workflows/source to `main`, and validate the
   clean final commit, versions, ancestry, and exact-SHA source/artifact gates.
4. Only with authorization, tag `v0.4.0`. Wait for all checks/builds before exposing
   the complete staged release. Do not overwrite published archives.
5. Inspect/smoke the checksum-bearing Hex package; allow authorized publication
   and verify public package/docs links and fresh-consumer installation.
6. Announce with the migration link; update demo dependencies separately. Use a
   patch release, not replacement binaries, for subsequent fixes.
