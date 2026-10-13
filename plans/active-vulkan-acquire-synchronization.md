# Wayland Vulkan synchronization follow-up

## Remaining qualification

- Exercise interactive resize, occlusion, swapchain recreation and
  suboptimal/out-of-date handling, including Stop with work outstanding. Window
  startup size is not a supported live resize operation.
- Corroborate on additional drivers/compositors. Do not reopen closed general
  GPU/PRIME, Vulkan reconnect or FP3 acceptance.
- Measure bounded resource high-water marks, normal-frame CPU waits, extra-submit
  cost, cadence and idle behavior against the same device baseline. The additional
  queue submit is not assumed free; do not infer performance gains from regressions.

Use source-built Vulkan-only and combined Wayland profiles, confirm Vulkan and
`software: false`, and retain complete native output through teardown:

```sh
elixir scripts/check-wayland-vulkan.exs --allow-hardware /tmp/wayland-vulkan-validation.log
```

Require zero synchronization/lifetime errors or warnings and matching native
startup/teardown reports for every instance. Missing reports or quarantined
resources fail even when ExUnit passes. Extend manual lifecycle coverage rather
than treating the automated capture/reuse/cap tests as interactive acceptance.

Hardware requires explicit consent for the new run. Do not modeset a DRM card or
exercise unrelated desktop power controls. Preserve nonblocking acquisition,
bounded fence retirement and fail-safe quarantine; no validation suppression,
per-frame queue/device idle, synchronization mode or pacing rewrite.

The shipped synchronization contract, targeted Hyprland/RADV evidence and gate
procedure are maintained in [frame pacing](../guides/internals/frame-pacing.md)
and [native build guidance](../guides/reference/native-renderer-builds.md#wayland-vulkan-synchronization-in-050).
