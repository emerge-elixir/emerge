# Input source mirrors and Actions cache

Status: implemented and locally validated; committed on
`fix/input-source-mirrors-cache`. No push, tag or publication.

## Commit sequence

1. `82ebe7a` — `fix(input): retry checksum-verified source mirrors`:
   ordered download fallback and network-free Rust regressions.
2. `8903373` — `ci(input): share verified source archives across native builds`:
   archive verification helper, shared Actions cache, cross/container mounts and
   CI/ExUnit coverage.
3. `docs(input): document mirror fallback and shared source caching`:
   download order, cache/offline behavior and local validation evidence.

The commit split preserves the previously validated implementation contents.

## Implementation

- Ordered HTTPS download URLs, mirrors first and upstream last; transport or
  checksum failures remove partial files and try the next URL. Stop at the first
  verified archive, then publish atomically. Existing corrupt-cache and offline
  behavior remains fail-closed. Archive versions and SHA-256 pins are unchanged.
- Independently downloaded and verified all configured alternative mirrors:
  - libinput: OSUOSL BLFS, FreeDif BLFS.
  - libevdev: Void Linux, OSUOSL BLFS, FreeDif BLFS.
  - mtdev: Void Linux, OSUOSL BLFS.
  - libudev-zero: Buildroot, Gateworks.
- Linux CI and release jobs restore the same `sources.rs`-keyed Actions cache;
  only original `.tar.*` archives are saved, not extracted/patched trees or target
  binaries. Main-branch stable CI tests the input crate and seeds the cache for
  tag builds. No broad restore prefix or target/Rust-specific archive cache.
- Separate restore/save steps persist complete verified downloads even when
  subsequent compilation fails. A small ExUnit-tested Elixir helper reads the
  authoritative Rust pins and rejects missing, partial or corrupt caches before
  saving. GitHub branch scope, retention and eviction still apply.
- Cross mounts/passes the external cache and offline settings. Release Docker
  builds mount it writable independently of the read-only source checkout.
- Existing exact-SHA CI/tag/publication gates are unchanged. No hardware tests,
  package/permission changes or release operations were needed.

## Validation

- Fresh empty-cache `cargo test --locked --manifest-path native/emerge_input/Cargo.toml`:
  all four sources downloaded through the configured primary alternatives and
  matched their pins; 3 library tests and 13 source/generator tests passed.
- Offline cache reuse test and strict all-target input Clippy passed on stable
  and minimum Rust 1.91.0. Regressions cover transport/checksum fallthrough, a
  third URL, stop after first verified response, cleanup after exhaustion, and
  no networking for verified/offline/corrupt cached files.
- `EMERGE_SKIA_BUILD=1 ./ci-tests.sh`: formatting, Credo, native Clippy, 683 Mix
  passes (including full sweep), 1616 native unit tests plus integration suites,
  and Dialyzer (zero errors) passed.
- CPU DRM release Cargo suite: 1599 unit tests passed, one hardware test ignored;
  benchmark integration suites also passed.
- Cache helper/workflow ExUnit suite: 11 passed, including final workflow edits.
- Actionlint 1.7.12 passed for both modified workflows; public ExDoc passed with
  warnings as errors. `git diff --check` passed.
- Logs: `/tmp/emerge-input-mirrors-{cargo-test-final,offline-final,clippy-final,msrv-test,msrv-clippy,drm-raster-test,ci,targeted-mix-final,docs}.log`.

Remote cache restore/save and cross/Docker artifact builds have not been run in
this session (no container engine is installed). They remain remote validation,
not evidence inferred from local tests or workflow linting.
