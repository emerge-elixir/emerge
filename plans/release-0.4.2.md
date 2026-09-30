# Emerge 0.4.2 release preparation

Prepare `release/v0.4.2` from `d4c8163`, retaining the macOS raster commits and
the preceding Linux CI Cargo-cache isolation fixes. Do not tag, push, publish,
rewrite earlier releases or manufacture artifact checksums during preparation.

## Scope

- Synchronize Mix, Cargo and the locked root crate at 0.4.2 without changing
  dependency versions.
- Date the 0.4.2 changelog and cover macOS raster artifacts, packaged-artifact
  validation, warning-free checksum-only compilation and CI cache isolation.
- Update installation/release examples and identify 0.4.2 as the first release
  with Darwin raster archives. Preserve historical release notes and fixtures.
- Validate release-note extraction, locked/offline Cargo metadata, full local
  CI, workflow syntax and whitespace, then make one release-preparation commit.

## Remaining release gates

1. Integrate the release into the default branch and obtain successful trusted
   CI for the exact final commit SHA. A squash/rebase changes that SHA.
2. Only after authorization, create/push `v0.4.2` for that commit. The artifact
   workflow reuses its CI result, builds all release profiles, and gates
   publication on packaged-artifact checks, including both macOS raster probes
   and the AArch64 DRM stock-RPi5-rootfs checks.
3. Publish Hex only after successful CI and artifact workflows for the same SHA;
   generate checksums from the actual published archives, never local fixtures.
4. Upgrade consuming projects such as `bb_nsk` only once matching Hex/GitHub
   artifacts are available. Its headless raster configuration remains valid.

Expected new Darwin raster archives:

- `libemerge_skia-v0.4.2-nif-2.15-aarch64-apple-darwin.so.tar.gz`
- `libemerge_skia-v0.4.2-nif-2.15-x86_64-apple-darwin.so.tar.gz`

Local Linux checks do not qualify macOS binaries, older macOS runtimes, the
physical panel or any Pi GPU/driver/camera behavior. Real release builds and
platform validation remain pending.

## Validation

- `elixir scripts/release-notes.exs v0.4.2` passed and extracted only the dated
  0.4.2 section.
- Locked/offline `cargo metadata --no-deps` passed and reported root crate
  version 0.4.2. Cargo.lock changes only the root crate version.
- `./ci-tests.sh all` passed: 592 Elixir checks (4 hardware exclusions) and
  1,496 Rust tests, plus formatting, warnings-as-errors compilation, Credo,
  Clippy and Dialyzer. The Linux run used the cached pinned `SKIA_SOURCE_DIR`.
- `actionlint` and whitespace checks passed. No release tag or publication was
  created, and no consuming application's dependency or lockfile was changed.
- Validation log: `/tmp/emerge-0.4.2-ci.log` (local, not a release artifact).
