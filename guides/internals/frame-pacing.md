# Native frame production and caps

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

Wayland Vulkan consumes each WSI acquire semaphore in a native, wait-only
`ALL_COMMANDS` submission on Ganesh's queue before handing over the image. This
protects Skia's initial layout transition, which `SkSurface::wait`'s narrower
fragment/transfer wait does not cover. Acquisition stays zero-timeout; a pool
bounded by the swapchain image count uses separate WSI and submit fences for
nonblocking retirement. Pool exhaustion uses the existing 1 ms acquire retry.
There is no ordinary-frame CPU wait or second acquisition wait for capture.
Failed teardown retains uncertain native ownership and reports the session as
quarantined; a render fence is not presentation completion.

Wayland keeps compositor/output-derived refresh estimates separate from the cap.
Capped callback intervals are not used to learn physical refresh. Animation
sampling predicts at least the next admissible application frame instead of
sampling a stale pre-cap callback deadline. Queue-latency statistics include the
admission wait, and pending scenes preserve their earliest submission timestamp.

## Bounded look-ahead

DRM (GL, Vulkan and CPU), Wayland and headless share
`backend/production.rs`. Successful normal submission grants preparation of one
successor. Output completion releases presentation capacity; it does **not**
grant a duplicate sample. Presenters retain their own buffer, fence, callback,
lease and draw-admission rules. There is no public pacing switch.

The tree actor receives requests through a separate capacity-one lane, with at
most one request queued or running. It acknowledges scene, inactive and retry
outcomes, including failures and unchanged output. Retry deadlines preserve the
existing layout-failure backoff. Registry publication still precedes scene
publication; a blocked registry cannot block input or Stop. Scenes continue to
use the capacity-one latest-scene handoff.

A `sample_at` prediction is not a presentation timestamp. Enter/exit transitions
carry per-admission, one-shot observation tokens in their scene snapshots. Their
initial poses wait for applicable output progress; an older frame cannot anchor
a later mount, even when the numeric node id is reused. Predictions never permit
buffer reuse.

- **DRM:** submission requests the successor before the pending flip finishes.
  GL and Vulkan retain one prepared frame. CPU scanout uses three dumb buffers
  and nonblocking page-flip events. Only matching completion (or a successful
  blocking detach during teardown) permits reuse of a scanout buffer. Cursor-only
  commits do not grant credits. GL retains its bounded cursor fairness window.
- **Wayland:** normal submissions grant credits; late replacements do not.
  Frame callbacks observe the normal submitted scene, not the latest received
  scene. These callbacks indicate compositor readiness, not physical display or
  buffer release. A late video replacement containing active animation remains
  eligible for a normal opportunity.
- **Headless:** successful delivery grants early tree work. Animation-derived
  scenes still wait for the cadence deadline (16 ms when uncapped), including
  the final frame. PRIME exhaustion retains one latest scene and waits for lease
  retirement rather than sampling again or spinning. Delivery is not evidence
  that a downstream physical display has presented the frame.

macOS still uses its existing AppKit-local tree preparation and animation clock.
Moving it to a worker and native frame opportunities remains a separate, open
stage of `plans/active-frame-production-pipeline.md`. No macOS latency improvement
or hardware qualification is claimed by the Linux implementation.

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

For Vulkan qualification, install the Khronos validation layer and build this
checkout with `compiled_backends: [wayland: [:vulkan]]` (also qualify the combined
Wayland build). With explicit permission to open test windows, run:

```sh
elixir scripts/check-wayland-vulkan.exs --allow-hardware /tmp/wayland-vulkan-validation.log
```

The runner forces source builds and Vulkan selection, enables synchronization
validation programmatically via `EMERGE_VULKAN_SYNC_VALIDATION=1`, and checks the
complete native output after the test process exits. It requires hardware-device
confirmation, matching startup/teardown reports for every Vulkan instance, and
zero errors or warnings. Missing reports and quarantined resources fail the gate.
An ordinary ExUnit pass alone is **not** synchronization qualification.

The tests cover sustained image reuse without screenshots, explicit capture's
second flush, caps/uncapping, static idle and Stop. Interactive resize/occlusion,
suboptimal/out-of-date recreation and additional drivers still need target-device
qualification; changing a Window's startup size is not a live resize API.

`EMERGE_VULKAN_VALIDATION=1` remains the general validation-layer switch. Manual
`vk_layer_settings.txt` setup still works, but the gate requires the programmatic
sync-validation setting rather than trusting an external settings file.

The historical Hyprland/RADV RX 7900 XTX run passed functional checks but reported
4 acquired-image synchronization errors. With the native handoff fix, the targeted
hardware gate passes on that GPU in both Vulkan-only and combined Wayland builds:
3 tests per build, with zero validation errors or warnings through teardown.
This does not qualify other drivers or the remaining interactive lifecycle cases.
See `plans/active-vulkan-acquire-synchronization.md`, separate from the
already-closed general Linux GPU/PRIME/reconnect qualification.
