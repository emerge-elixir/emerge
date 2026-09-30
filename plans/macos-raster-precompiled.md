# macOS raster precompiled NIFs

Preserve `bb_nsk`'s headless BW1/GRAY2 frame previews on macOS. Do not replace
those previews with native windows or remove the host renderer.

- Add x86_64/Apple Silicon Darwin NIF targets with a raster-only default archive
  (no variant suffix, like ARMv7 raster). Keep `macos_host` a separate product.
- Select those archives for `compiled_backends: []`; retain source-build fallback
  for unsupported GPU/window NIF combinations and normal native-window routing.
- Build CPU-only Skia with platform-native font support (CoreText on macOS),
  a macOS 11 deployment floor and no Metal backend. Include both archives in
  generated release checksum metadata.
- Verify packaged Mach-O architecture, dependencies and deployment floor; load
  the real archive without Rustler and smoke-test headless packed-frame output on
  each macOS runner before upload. Do not repeat the standard CI matrix.
- Add selection/cache/integrity and release-policy regression tests; run Elixir
  and Rust tests locally. Linux results do not qualify native macOS artifacts.

Existing changes to `.github/workflows/ci.yml` and
`test/release_workflow_test.exs` belong to the CI cache-isolation work and must
remain untouched. The implementation commits left the version unchanged;
these binaries are included in the subsequent [0.4.2 release preparation](release-0.4.2.md).
No tag or publication has been performed as part of that preparation.

## Commit sequence

At the user's request, keep the work in three dependency-ordered commits:

1. Keep checksum-only NIF compilation warning-free, with its regression test.
2. Add macOS raster target/profile selection and both release builds, keeping
   selection/metadata tests and consumer documentation with the feature.
3. Validate packaged macOS raster NIFs before upload, including Mach-O checks,
   isolated render/cache/integrity probes, release wiring, tests and release docs.

## Validation

- `./ci-tests.sh all`: 592 Elixir checks passed (4 hardware exclusions),
  1,496 Rust tests passed; formatting, warnings-as-errors compilation, Credo,
  Clippy and Dialyzer passed. Used the cached pinned `SKIA_SOURCE_DIR` on Linux.
- Fresh production checksum-only compilation passed with warnings as errors.
  The packaged-raster smoke harness passed against a Linux host fixture in
  that fresh build, without Rustler available in its probe VMs.
- `actionlint` and `git diff --check` passed. Existing CI cache-isolation files
  remained byte-for-byte unchanged by this work; that work was committed
  separately while these changes were in progress.
- Actual Apple Silicon/Intel artifact builds, Mach-O inspection and native
  load/render probes still require the release workflow on macOS runners.
  No macOS binary, old-macOS runtime or physical panel is qualified locally.
- `bb_nsk` remains unchanged. Once a release ships these archives, upgrade its
  pinned Emerge dependency while retaining the existing headless raster config.
