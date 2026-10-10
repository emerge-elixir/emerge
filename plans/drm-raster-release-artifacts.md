# CPU-only DRM release artifacts

## Implemented

- `drm_raster` release entries and exact archive selection for GNU x86_64/AArch64,
  ARMv7 hard-float, x86_64 musl and RISC-V64 GNU; existing defaults preserved.
- Embedded fonts, packaged ELF dependency checks, and AArch64 stock RPi5 load gate.
- Shared `drm-core` now includes embedded FreeType for every DRM build, including
  source builds and mixed presenters. CI verifies feature inheritance.
- Updated checksum metadata tests and docs. No published checksums fabricated.

## Local validation

- ExUnit: 592 passed, 11 excluded.
- Default Rust release tests: 1,502 passed.
- CPU DRM release tests: 1,476 passed, one hardware test ignored.
- `./ci-tests.sh quality`, CPU DRM Clippy, actionlint and shell syntax checks passed.
- GPU-free Cargo dependency checks passed for all five targets; FreeType feature
  inheritance verified for DRM core/raster/OpenGL/Vulkan/all and mixed presenters.

Cross-target archive builds and stock-rootfs loading remain release-CI validation;
no release assets have been published. Original unrelated untracked files untouched.
