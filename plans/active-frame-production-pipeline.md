# Frame production follow-up

## macOS worker and native frame opportunities

Move `TreeUpdateEngine`/scene preparation off AppKit to an actor-style worker per
session and adopt the shared bounded production contract. Preserve synchronous
host-protocol reply order through asynchronous completion rather than blocking
AppKit for the worker.

- Keep NSWindow/NSView, IME, clipboard and drawable/layer ownership on their proper
  threads. Port registry/event feedback, buffered-input replay and multi-session
  asset isolation without creating a second tree algorithm.
- Drive opportunities from a native display-link/run-loop source and output
  readiness, not `sleep(10 ms)` plus a submission-duration estimate. Callbacks only
  enqueue bounded notifications; they never mutate tree state or call BEAM directly.
- Preserve actual-progress animation anchors: prediction is not presentation.
  Accepted normal submission allows one successor; completion releases capacity
  without granting a duplicate sample. Keep first/final poses and bounded retries.
- Respect the host deployment floor. `CAMetalDisplayLink` requires macOS 14;
  use a compatible older adapter rather than silently raising the floor or adding
  another public pacing setting.
- Preserve raster/Metal drawing. PNG/NSImage transport replacement is a separate
  measured optimization, not required for this runtime work.
- Run deterministic slow-output/backpressure, duplicate completion, lost/no-output
  reply, final/exit/interrupted animation, registry freshness and Stop tests through
  both actor and host adapters. Run macOS CI and qualify Metal/raster on macOS.

## Target-device pacing acceptance

With fresh explicit device consent, measure the slow watch DRM path first, then
ordinary DRM/Wayland routes and headless binary/PRIME with held leases. This is
pipeline-specific cadence/resource acceptance, not a repeat of closed general
GPU/PRIME/reconnect/FP3 qualification. Targeted Wayland Vulkan synchronization
coverage is separate from [its remaining lifecycle/performance gates](active-vulkan-acquire-synchronization.md).

Record:

- input/patch → tree → scene → draw/flush → accepted submission → output progress;
  kernel flip time versus dispatch delay where available;
- physical refresh, unique-frame rate, sample-time error, output-ready-to-submit
  delay and p50/p95/p99 latency;
- constructed/selected/prepared/submitted/presented frames, overwrites, retries,
  occupied buffers, CPU/GPU time, memory and static-scene idle work;
- power/reconnect/cursor/capture, held-lease backpressure and Stop resource safety.

Require a bounded pipeline with no avoidable post-completion tree/draw gap,
oversampling, stale final pixels, unsafe reuse or idle-power regression. With
adequate render budget, throughput should approach the output service limit,
not service time plus tree/draw time. Quantify any latency trade-off from a ready
frame; never claim input-to-light faster than physical panel transfer. Do not
continually redraw staged buffers as a speculative freshness fix.

Close the pacing portion of [low-resource animation work](active-low-resource-animation-smoothness.md)
with measurements. Do not modeset the host desktop card by default.

## Maintained contracts

- [Frame production/caps](../guides/internals/frame-pacing.md)
- [Tree/animation publication](../guides/internals/layout-refresh-render-flow.md)
- [Runtime architecture](../guides/internals/architecture.md)
- [DRM resource/power lifecycle](../guides/internals/drm-outputs-and-lifecycle.md)

No renderer-cache redesign, general layout rewrite, public pacing switch or
config/frame proof machinery. Synchronous `render_to_pixels/2` remains explicit
on-demand work. DRM/macOS frame caps are a separate
[configuration follow-up](active-declarative-viewport-settings.md).
