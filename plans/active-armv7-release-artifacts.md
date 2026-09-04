# ARMv7 release artifacts

## Evidence

- Run `33923915654` at `2dcc55d` failed both ARM32 jobs in `Build the project`.
- Detailed logs require authenticated GitHub access (HTTP 403); annotations
  only expose exit code 1. The exact compiler failure is not yet confirmed.
- Trellis uses Cortex-A7 and `armv7-nerves-linux-gnueabihf`. `mix.exs` already
  maps that compiler to `armv7-unknown-linux-gnueabihf` for source builds.
- The legacy cross image has an obsolete host toolchain for a Skia source build.

## Changes

- [x] Name jobs `linux_armv7_raster` and `linux_armv7_opengl`; use the ARMv7
  triple consistently in Cargo targets, artifact metadata, tests, and docs.
- [x] Pin the modern cross ARMv7 image by OCI digest and use its actual
  `/usr/arm-linux-gnueabihf` sysroot, linker, CMake, and bindgen configuration.
- [x] Keep embedded Skia free of desktop font dependencies and raster free of
  GPU dependencies; verify the produced ELF in the artifact jobs, including
  ELF32, ARMv7, and VFP register argument attributes.
- [x] Keep generic ARM source fallback aligned with RustlerPrecompiled:
  it ignores `TARGET_CPU` and `CC`. Do not normalize only Emerge's preflight
  while leaving the actual downloader resolving `arm`, or mutate the global
  environment while dependencies compile concurrently.
- [x] Audit validation: full `./ci-tests.sh all` with 456 Elixir tests
  (3 excluded), 1,007 Rust tests plus the benchmark fixture, formatting,
  warning-denied Clippy, Credo, and Dialyzer. Actionlint, 18-artifact matrix
  consistency, and warning-free docs also pass.
- [x] Exercise the workflow ELF check against compiled probe libraries:
  ARMv7 hard-float passes; ARMv7 soft-float, ARMv6, and AArch64 fail.
  Force the C locale for stable `readelf` output. Scope environment-selection
  documentation to cross builds rather than native ARMv7 hosts.
- [ ] Rerun both ARMv7 artifact jobs and inspect the produced dependency closure.
- [ ] Qualify the resulting binaries on Trellis (CPU and glibc/libstdc++ ABI).

## Follow-up

Automatic precompiled selection on Trellis remains unresolved: its environment
exposes `TARGET_ARCH=arm`, not `armv7`. Until RustlerPrecompiled supports an
explicit per-library target or CPU-aware resolution, the build environment must
expose `TARGET_ARCH=armv7` to select these artifacts; otherwise Emerge safely
builds from source. Do not claim that renaming the artifact alone enables
precompiled downloads on existing Trellis projects.
