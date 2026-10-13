# 0.5.0 release preparation

Branch: `release/0.5.0`, based on `main` at `c641a9d`.

## Prepared

- [x] Mix, renderer Cargo manifest and renderer lockfile are 0.5.0; dependencies are unchanged.
- [x] Dated 0.5.0 release notes include changes since v0.4.2 and explicit backend/hardware limitations.
- [x] Installation, migration, native build guidance and container examples match the release.
- [x] Regression coverage checks manifests, lockfile, changelog, documentation refs and versioned NIF/macOS-host URLs.
- [x] Full local CI (quality, ExUnit, Rust tests and Dialyzer), bundled-input tests, CPU-only DRM and all-Linux-backend Rust tests pass.
- [x] Unpacked Hex source compilation, screenshot checks, public docs and local links pass; internal docs and links also pass.

## Publication gates still open

- [ ] Review the documented limitations, especially the known Wayland Vulkan acquired-image synchronization failure; release preparation does not fix or qualify it.
- [ ] Confirm the changelog date immediately before integration/tagging.
- [ ] Integrate the release commit into the default branch and require CI on that exact SHA, including macOS.
- [ ] Build/verify the complete 0.5.0 artifact matrix, including cross/container jobs, macOS raster probes and stock RPi5 userspace loading. Docker/macOS/board validation was not available locally.
- [ ] Tag/push only the validated integrated commit; let the artifact and Hex workflows generate real checksums and publish.

No tag, push, publication or fabricated checksum was performed. The maintained
[release procedure](../guides/reference/native-renderer-builds.md#prepare-and-publish-050)
owns the commands and immutable-artifact rules.
