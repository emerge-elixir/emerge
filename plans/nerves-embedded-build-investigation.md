# Nerves x86_64 musl and MangoPi RISC-V64 release support

## Confirmed failure

The published `emerge 0.4.0-beta.1` package and current source recognize
`x86_64-nerves-linux-musl-gcc` as Rust target `x86_64-unknown-linux-musl`, but
`EmergeSkia.BuildConfig.precompiled_targets/0` only includes x86_64 GNU,
aarch64 GNU and ARMv7 GNU hard-float. The release workflow, published checksum
manifest and GitHub release assets also omit musl.

A fresh-process probe using the actual Hex package reproduced the supplied
error without Rustler installed and without explicit force-build environment
variables: target `x86_64-unknown-linux-musl`, backends `[:drm]`, forced source
build `true`, then `Rustler dependency is needed to force the build`.
Rustler is an optional dependency and therefore absent from the consuming app.

Evidence: `/tmp/emerge-beta1-package-dir`, `/tmp/emerge-beta1-nerves-probe.exs`,
`/tmp/emerge-beta1-nerves-probe.log`, `/tmp/emerge-beta1-github-release.json`.
Published Hex release checksum:
`03c57dce012a348c576157bd6918cea8c477a40f617717a2260f4b57854cad1c`.
Investigation source: `96b3c36`.

## Implementation completed

- Added embedded raster/default and DRM/OpenGL artifacts for
  `x86_64-unknown-linux-musl` and `riscv64gc-unknown-linux-gnu`.
- Added MangoPi compiler detection and normalized `riscv64` to Rust's
  `riscv64gc`; existing desktop GNU and ARM profiles are unchanged.
- Added containerized, locked source builds, ELF/libc/dependency validation,
  and eager load checks (native musl and RISC-V under QEMU).
- Fixed musl's dynamic CRT for both the NIF and native bindgen build scripts,
  installed the required libclang development library, and corrected the
  DMA-BUF ioctl request type (`c_int` on musl versus `c_ulong` on glibc).
- Added no-Rustler download/cache/checksum/environment-restoration regressions,
  default Nerves profile selection, and checksum metadata for all four archives.

## Validation

All four artifacts built successfully from source with Rust 1.91.0 in isolated
Alpine 3.22.3 / Ubuntu 24.04 roots using the release-container recipes. The local
runs used bubblewrap rather than Docker; the GitHub workflow itself has not
been executed here. RISC-V artifacts passed eager dynamic loading under QEMU.
The musl raster artifact also loaded against the actual Nerves musl toolchain
15.3.1 runtime libraries. These load checks do not initialize BEAM or a GPU.

Full local CI: 505 Elixir tests/doctests; 1082 Rust unit tests, one fixture test
and 13 benchmark-support tests; Clippy, Credo and Dialyzer passed. Separate
`cargo test` and `mix test` passed (500 Elixir tests/doctests, eight excluded).
Actionlint and shell syntax checks passed. The final combined validation command
hit its tool timeout during the separate Mix run, which was rerun successfully.

Code identity (excludes this investigation report):
`96b3c361e504a468648f794d24fd6bd3aa31b3a0+code-patch-sha256:17ae45b35852f79a0244ca402ee368481779c8c05b56cda41de814caa188e96e`.

Evidence:
- `/tmp/nerves-embedded-targets.patch`, `/tmp/nerves-embedded-targets-source-id`
- `/tmp/emerge-{musl,riscv}-build-final.log`
- `/tmp/emerge-nerves-loader-final.log` (includes actual archive hashes)
- `/tmp/embedded-targets-{ci,cargo,mix}-final.log`
- Local archives: `/var/tmp/emerge-{musl,riscv}-artifacts/`

## Release and device qualification still required

- Publish a subsequent package release with the four new archives and generated
  checksums. Adding assets to beta.1 alone cannot fix its target registration.
- Re-run actual Nerves compatibility scans against that package, including
  `nerves_system_x86_64 1.34.2` and `nerves_system_mangopi_mq_pro 0.17.2`.
- Qualify BEAM/NIF startup and actual rendering on the target systems. GPU
  profiles require suitable target libraries and drivers; host tests and QEMU
  load checks do not establish physical-board graphics support.

No archives were published and no source changes were committed by this task.
