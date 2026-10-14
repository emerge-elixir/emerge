# 0.5.0 publication gates

Branch: `release/0.5.0`, rebased onto `main` at `5401a29`.
The Vulkan/raster GPU-upload DRM power sequence is integrated through `0028ca3`.

- [ ] Review the remaining backend/hardware limitations. Wayland Vulkan
  interactive resize/occlusion/recreation, other drivers and performance remain
  open; targeted regression passes are not general qualification. DRM Vulkan and
  raster GPU-upload power have integrated-GPU rendering/KMS evidence, but physical
  brightness/DCS/optical, video and destructive-fault qualification remain open.
- [ ] Confirm the changelog date immediately before integration/tagging.
- [ ] Integrate the release commits into the default branch and require CI on
  that exact SHA, including macOS.
- [ ] Build/verify the complete 0.5.0 artifact matrix, including cross/container
  jobs, macOS raster probes and stock RPi5 userspace loading.
- [ ] Tag/push only the validated integrated commit; let the artifact and Hex
  workflows generate real checksums and publish.

Local integration validation passed: `EMERGE_SKIA_BUILD=1 ./ci-tests.sh`, public
and internal `mix docs --warnings-as-errors`, and generated `v0.5.0` release notes.
This does not replace remote CI or the platform/artifact acceptance gates above.

The maintained [release procedure](../guides/reference/native-renderer-builds.md#prepare-and-publish-050)
owns the commands and immutable-artifact rules. No tag, push or publication has
been performed. Docker/macOS/board validation was unavailable locally.
