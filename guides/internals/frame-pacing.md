# Native frame caps

`Emerge.Config.Renderer.target_fps` is a positive 32-bit render-admission limit.
`nil` restores backend pacing; it does not request continuous rendering.
Headless and Wayland support startup and live updates. DRM and macOS caps are
currently rejected. Physical display refresh remains independent of this limit.

Update the typed config without replacing the viewport or renderer:

```elixir
alias Emerge.Config

def config(%Config{renderer: %Config.Renderer{} = rendering} = previous, state) do
  rendering = %Config.Renderer{rendering | target_fps: state.frame_limit}
  %Config{previous | renderer: rendering}
end
```

The previous config is desired intent. A cap update does not cancel an already
admitted frame. Application timers, producers and tree updates are not throttled
by this setting. Unchanged content is not redrawn merely because its cap changes.

## Scheduling

`backend/cadence.rs` holds the shared cap and owner-thread admission clock.
Intervals round up to nanoseconds; deadlines derive from the last admission,
not from render completion. Lowering, raising or removing a cap recomputes the
next deadline. Missed periods never generate catch-up frames.

Headless binary/PRIME loops keep a latest pending scene and select independently
on control, stop and lease-retirement events. Binary output retains autonomous
failure retries. Native animation pulses cannot be starved by continuous scene
traffic. Uncapped headless animation retains its existing 16 ms pacing policy.

Wayland applies the same admission gate before raster, GPU-upload, GL or Vulkan
rendering, including capture, video, late replacement and acquire-retry redraws.
A coalescing calloop ping wakes live cap changes without adding scene messages or
marking content dirty. Dispatch only arms a cap deadline when the presenter could
otherwise draw. Waiting for configuration, a compositor callback or an acquire
retry never creates an expired-deadline spin. The existing 250 ms shutdown poll
remains; input, callbacks and cleanup are not delayed until the render deadline.

Wayland keeps compositor/output-derived refresh estimates separate from the cap.
Capped callback intervals are not used to learn physical refresh. Animation
sampling predicts at least the next admissible application frame instead of
sampling a stale pre-cap callback deadline. Queue-latency statistics include the
admission wait, and pending scenes preserve their earliest submission timestamp.

## Validation

Pure Rust tests cover deadline rebasing, idle behavior, control coalescing,
calloop wake/disconnect behavior and physical-refresh estimation. ExUnit covers
headless output and lifecycle replay without a compositor.

The opt-in Wayland test briefly opens a window on the selected compositor:

```sh
mix test test/emerge_skia/wayland_frame_rate_test.exs --include wayland_hardware
EMERGE_WAYLAND_TEST_API=gpu_upload mix test test/emerge_skia/wayland_frame_rate_test.exs --include wayland_hardware
EMERGE_WAYLAND_TEST_API=opengl mix test test/emerge_skia/wayland_frame_rate_test.exs --include wayland_hardware
# Requires a build with Wayland Vulkan support:
EMERGE_WAYLAND_TEST_API=vulkan mix test test/emerge_skia/wayland_frame_rate_test.exs --include wayland_hardware
```

It checks burst limits, live uncapping, final pixels, config-only idleness,
refresh statistics and shutdown while capped. Passing on one compositor does
not qualify every compositor/GPU or PRIME synchronization path.

For Vulkan validation, install the Khronos validation layer and enable it with
`EMERGE_VULKAN_VALIDATION=1`. To enable synchronization checks, place
`khronos_validation.validate_sync = true` in `vk_layer_settings.txt` and point
`VK_LAYER_SETTINGS_PATH` at its directory. Run the same Vulkan test with these
settings and inspect native validation output: **an ExUnit pass is insufficient**
if `Vulkan validation final count` reports errors.

The 2026-10-10 run on Hyprland/RADV RX 7900 XTX passed the functional checks but
failed synchronization validation with acquired-image layout-transition hazards
(`SYNC-HAZARD-WRITE-AFTER-READ`; 4 errors, 0 warnings). Vulkan qualification remains
blocked until those hazards are fixed. The reproduction and environment record
is in `plans/active-linux-gpu-qualification.md` in the repository.
