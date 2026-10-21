# Native build profiles: remaining rollout

Branch: `refactor/build-capabilities`.
Related: [issue #101](https://github.com/emerge-elixir/emerge/issues/101).

The implementation and software acceptance are now covered by source, tests and
maintained guides. Keep only unfinished release/platform work here.

Prebuilt coverage is intentionally curated per target, not exhaustive for every
valid source configuration. Keep the four common ARMv7/musl/RISC-V profiles;
verify configuration-to-job/archive identity without expanding those targets to
all 16 Linux feature unions. Unlisted combinations retain exact-set source fallback.

Builder consolidation is implemented: six architecture/platform builders retain
all 46 NIF and two host archives. ExUnit covers shared Cargo output, compatible
Skia batches, ABI/environment isolation, container cache reuse, archive snapshots,
NIF ABI validation and failure handling. Remote execution remains a release gate.

Uploads are per archive, not per worker: 48 independent GitHub Actions artifacts,
with host checksum sidecars paired with their archives. Upload planning, failure
handling and runtime-credential inheritance have local regression coverage.

## Remaining gates

- Finalize the next release version/date from `0.5.1` after review. Do not
  rename/reuse old 0.5.0 archives or hashes for new canonical feature profiles.
- Run all six grouped builders on remote CI, including time/disk use for a full
  GNU batch, both macOS architectures, ARMv7 cross builds, musl/RISC-V containers,
  packaged ELF/Mach-O checks and the stock RPi5 userspace load gate. Local Linux
  tests do not qualify those binaries or measure remote build-cache savings.
  Verify all 48 independent uploads, host checksum sidecars and filename-based
  RPi5 downloads through the official uploader bridge.
- Publish genuinely minimal OpenGL-only AArch64/ARMv7 artifacts with the other
  canonical profiles; generate the matching real checksum manifest before Hex
  publication. Neither local source builds nor catalog entries publish archives.
- With explicit device access, validate complete firmware/rendering for the known
  stock-system defaults and custom headless/SPI OpenGL use case. Image config and
  successful NIF loading are not physical GPU/presentation qualification.
- Extend known system/version profiles only with documented image/driver evidence;
  retain explicit overrides and conservative unknown-system defaults.

Headless Metal remains deferred, outside this implementation's acceptance scope.

## Maintained references

- [Feature resolution, variants and release validation](../guides/reference/native-renderer-builds.md)
- [Public viewport configuration](../lib/emerge.ex) (moduledoc: build support, Nerves defaults and runtime recipes)
- `mix/native_profiles.exs`, `EmergeSkia.BuildProfiles`, and the generated
  `scripts/native-release-matrix.exs` output are the source of profile coverage.
