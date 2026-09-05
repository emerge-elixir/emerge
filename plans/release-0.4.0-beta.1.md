# 0.4.0-beta.1 release preparation

Release from `headless-backend`, preserving ancestry from `v0.3.4`.
Do not create a tag or publish until the source and platform gates are reviewed.

## Source preparation

- [x] Align Mix, Cargo, Cargo.lock, README, and dated changelog to `0.4.0-beta.1`.
- [x] Mark both Linux NIF and macOS-host GitHub uploads as prerelease/not latest.
- [x] Keep exact-tag validation and protected Hex publication; retain source
  validation in CI and the publication workflow, not artifact jobs.
- [x] Run full local CI, warning-free docs, and unpacked-package source checks.
- [x] Verify beta selection uses precompiled downloads (not a dev/source build).
- [x] Commit the prepared source; do not tag or publish automatically.

## Publication gates and procedure

1. Configure the GitHub `hex` environment with required approval and make
   `HEX_API_KEY` available to it. Allow release-tag refs for manual dispatch.
   Automatic `workflow_run` publication runs on the default-branch ref and needs
   that ref allowed too; exact checked-out release-tag validation remains mandatory.
2. Push the prepared `headless-backend` commit and manually dispatch CI on that
   branch. Review Linux/macOS source CI and remaining platform qualification.
   Local Linux validation cannot replace macOS builds or Trellis/RPi hardware tests.
3. Create an annotated exact tag on the reviewed commit:
   `git tag -a v0.4.0-beta.1 -m "Release Emerge 0.4.0-beta.1"`.
4. Push `v0.4.0-beta.1`. Tag CI validates source; the artifact workflow must build
   all 18 Linux NIF archives and both macOS host archives/checksums successfully.
5. Confirm the GitHub release is a prerelease and does not replace the latest
   stable release. Do not move an already published tag to fix a failed release.
6. Approve protected Hex publication after artifact success. `workflow_run`
   automation requires the Hex workflow on GitHub's default branch; if needed,
   dispatch the existing Hex workflow using the tag as both workflow ref and input:
   `gh workflow run private_hex_release.yml --ref v0.4.0-beta.1 -f ref=v0.4.0-beta.1`.
7. The Hex job revalidates the exact tag, verifies macOS assets, generates all NIF
   checksums from release assets, and publishes the package/docs. Do not generate
   fake checksums or publish from this workstation.
8. Confirm Hex keeps `0.3.4` as latest stable. Consumers opt into the beta with
   `{:emerge, "== 0.4.0-beta.1"}`. Qualify the downloaded ARMv7 raster/OpenGL
   artifacts on Trellis before claiming hardware completion.

## Validation results

- Full local CI: 460 Elixir tests (3 hardware tests excluded), 1,007 Rust tests
  plus the benchmark fixture, formatting, warning-denied Clippy, Credo, Dialyzer.
- 30 documentation screenshots generated; docs build with warnings as errors.
- Unpacked `0.4.0-beta.1` Hex package passes locked Cargo checks for all default
  targets and the minimal `embedded-cpu` profile using registry dependencies.
- Actual NIF-module tests select/download the beta ARMv7 archive using Trellis's
  compiler mapping with Rustler absent; checksum rejection and restoration pass.
  These use host NIF payloads, not hardware qualification of ARM code.
- Workflow lint, exact Mix/Cargo/lock/changelog/README agreement, and ancestry
  from `v0.3.4` pass. No tag, release upload, or registry publication was performed.

