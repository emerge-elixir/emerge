# ARMv7 release artifacts

## Evidence

- Run `33923915654` at `2dcc55d` failed both ARM32 jobs in `Build the project`.
- Detailed logs require authenticated GitHub access (HTTP 403); annotations
  only expose exit code 1.
- User-provided raster logs for run `33931152579` at `829b0a8` confirm GN
  generation succeeds, then rust-skia cannot execute `ninja` (`ENOENT`). The
  `is_skia_standalone` warning is non-fatal; the missing library at packaging
  is a consequence of the failed build.
- Trellis uses Cortex-A7 and `armv7-nerves-linux-gnueabihf`. `mix.exs` already
  maps that compiler to `armv7-unknown-linux-gnueabihf` for source builds.
- The legacy cross image has an obsolete host toolchain for a Skia source build.

## Changes

- [x] Name jobs `linux_armv7_raster` and `linux_armv7_opengl`; use the ARMv7
  triple consistently in Cargo targets, artifact metadata, tests, and docs.
- [x] Pin the modern cross ARMv7 image by OCI digest and use its actual
  `/usr/arm-linux-gnueabihf` sysroot, linker, CMake, and bindgen configuration.
- [x] Install host `ninja-build` in the shared ARMv7 cross image setup, used
  by both raster and OpenGL source builds.
- [x] Keep embedded Skia free of desktop font dependencies and raster free of
  GPU dependencies; verify the produced ELF in the artifact jobs, including
  ELF32, ARMv7, and VFP register argument attributes.
- [x] Reuse the compiler-prefix mapping already in `mix.exs`. Apply its Rust
  target architecture to `TARGET_ARCH` around the existing preflight and
  `use RustlerPrecompiled`, then restore the original value in `after`.
  No separate CPU table, custom loader, or downstream override is needed.
- [x] Isolated-process tests compile the actual NIF module using Trellis's
  environment and the real Mix target mapping, with Rustler absent. Cover
  raster download/load, OpenGL cache/load, checksum failure with environment
  restoration, and ARMv6 source fallback. Test archives contain the host NIF,
  not ARM code; hardware qualification remains separate.
- [x] Audit validation: full `./ci-tests.sh all` with 460 Elixir tests
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

Trellis now selects ARMv7 automatically from its existing toolchain. The
remaining gates are the ARMv7 artifact build and actual Trellis loading/rendering,
including CPU and libc compatibility. Source builds remain the fallback for
unsupported targets/profiles or explicitly forced builds.
