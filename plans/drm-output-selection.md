# DRM output selection and independent viewports

> Implementation resolution: preserve the public callback PID and use private per-viewport
> lifecycle supervision. No application callback or automatically started global runtime.
> See [actual progress and validation limits](drm-implementation-progress.md); proposed
> topology/requirements below are not a claim that every design item is implemented.

Detailed implementation plan: [Renderer lifecycle and recovery](drm-renderer-lifecycle-recovery.md).
Compatibility prerequisite: [Viewport PID usage audit](viewport-pid-compatibility-audit.md).
Application supervision policy is out of scope; cleanup and renderer recovery are owned by each viewport's internal supervisor/lifecycle process.

- Add `EmergeSkia.drm_outputs(drm_card: "/dev/dri/card0")`: connector names, connection state, exact mode IDs, dimensions, refresh and preferred flags. Discovery performs no modeset or explicit DRM-master acquisition.
- Add `drm_output: "HDMI-A-1"`, `drm_mode: mode` (or `mode.id`) to renderer/viewport options. Explicit mode requires explicit output and matches full timings, never nearest resolution. Omitted selectors retain automatic selection.
- Share one master per device identity inside the NIF; give each renderer an independent DRM lease containing a connector, free compatible CRTC, primary plane and optional cursor plane. Never share page-flip event queues or drop another renderer's master. Reserve resources during startup; release only after ownership-safe cleanup, retaining uncertain resources instead of reusing them. Require DRM leasing rather than silently use unsafe independent masters.
- Apply selection to GL/raster and Vulkan startup and hotplug checks. Preserve the standalone Vulkan probe path.
- Test mode identity/selection, resource exclusion/lifetime logic, Elixir validation, NIF discovery failures, viewport forwarding; document two-output usage and shared input limitation.
- Run Rust tests (including DRM feature variants), ExUnit, formatting and targeted lint. Real multi-monitor, hotplug and stop/restart verification requires DRM hardware.

## Failure and recovery contract — proposed, not implemented

### Ownership

- The card owner, not an individual renderer, holds DRM master. Each live session holds a reference to that owner and its own leased fd. Stopping A must never issue DROP_MASTER on the shared card or modeset B's resources.
- Keep master while any live or safely retained/quarantined session needs it. After the last clean session finishes, close the card; a later start reacquires master (and can fail if another process acquired it).
- Track each output reservation through `starting -> running -> stopping -> released`, or `quarantined` if GPU/KMS ownership cannot be proven safe. `running? == false` alone does not mean an output is reusable.
- Admission, reservation and release are serialized per card, with generation/session IDs to prevent stale cleanup from releasing a replacement session. Slow GPU teardown and thread joins must not hold the card registry lock or block another session's rendering loop.
- A lease isolates KMS resources and its event queue, not the GPU, driver or BEAM address space. Revoking/closing a lease is not proof that GPU work has finished, nor a guarantee that a monitor immediately blanks.

### Failure matrix

| Event | Master / cleanup | Restart and other viewport |
|---|---|---|
| Normal stop of A | Drain work, disable/restore only A, release A's lease and reservation; retain shared master for B | A can restart after cleanup; B continues |
| A's callback GenServer exits or is killed | A's internal lifecycle owner monitors it and requests native cleanup even if `terminate/2` is skipped or another process retains the resource term | Gate internal callback restart/renderer attachment on cleanup; B continues. No dependency on the application's supervisor |
| A's lifecycle owner or entire viewport unit is killed | Native Rustler monitoring of the lifecycle-owner PID requests cleanup without depending on Elixir `terminate/2` | Old reservations remain blocked until safe cleanup. If the entire unit exits, whether it is restarted is the application's choice |
| A has a recoverable renderer error | Notify its lifecycle owner, stop native actors and perform ownership-safe cleanup | Recreate only A's renderer inside its viewport unit, preserving the live callback state; B continues |
| A's native thread panics and unwinds | A thread-exit guard must invalidate health and arrange safe cleanup; panic catching does not itself prove GPU safety | Restart A only if cleanup is proven safe; otherwise quarantine. Never silently leave a healthy heartbeat after thread death |
| GPU hang, device loss, unresolved page flip/fence, teardown failure | Do not free or reassign resources still possibly used by GPU/KMS; retain and report quarantine | No guaranteed in-process restart. Current Vulkan policy is process-wide and can affect B's Vulkan operations/admission; preserve that safety policy until narrower isolation is proven |
| Native segfault, abort, memory corruption | Native code shares BEAM's process; a fatal fault can terminate the whole VM | Both viewports are lost. Independent native helper processes would be a separate architecture project; even those do not isolate a shared GPU reset |
| BEAM process exits, including SIGKILL | Kernel closes its fds and tears down its DRM ownership, assuming no external fd holders | A new VM can reopen/reacquire subject to driver health and competing masters; previous display contents need not disappear immediately |
| A's monitor disconnects | Suspend/stop only A's output; never migrate an explicitly selected output to B | Proposed reconnect behavior: retry the same connector and exact mode with backoff after safe cleanup; unavailable/stale mode remains an explicit error. B continues unless there is a device-wide fault |
| Duplicate start for A / start races with A's shutdown | Reject as busy/stopping/quarantined; never steal resources from a live session | Existing A/B continue. A replacement waits or retries; it cannot race cleanup |
| Startup fails midway | Roll back that session's partial allocation; preserve other sessions and master | Retry is allowed after safe rollback. Uncertain submitted GPU/KMS work follows quarantine rules |
| Card disappears, DRM master is lost, shared owner fails, or GPU resets | Treat as card/device-level failure, not output-local | Every session on that card may be affected. No promise that B survives; do not automatically reset the shared device to recover A |
| DRM leasing unavailable / too few independent CRTCs or primary planes | Return an explicit startup error without modifying live outputs | A second independent viewport is unsupported on that configuration; no unsafe shared-fd fallback |

### Existing gaps that must be addressed

- `RendererResource` has resource-drop cleanup but no owner-process `down` callback. Add a viewport-owned supervisor and lifecycle process that monitors the callback GenServer; natively monitor the lifecycle PID as a fallback if it or the whole unit dies. Execute blocking cleanup off BEAM scheduler callbacks, keep ownership distinct from input/log recipients, and specify the public PID/API compatibility contract before implementation.
- Health currently uses an atomic flag and a separate heartbeat thread; a presenter panic need not clear that flag. Monitor all essential renderer actors, not just the presenter. Thread liveness also does not prove GPU progress; use progress deadlines only while work is pending, not while an idle display is static.
- `handle_check_renderer/1` currently stops the callback process when rendering stops. Replace this with library-owned renderer recovery inside the viewport unit; distinguish intentional close from failure without relying on an external supervisor to perform cleanup or restart.
- Current joins can wait indefinitely. Define bounded externally visible shutdown/restart behavior without pretending a hung Rust/GPU thread can be safely killed. On timeout, report stopping/quarantined and prevent resource reuse; cleanup of A must not stall B.
- The DRM Vulkan presenter already retains uncertain sessions until VM restart and sets a Vulkan-global terminal flag. Do not promise local Vulkan fault recovery merely by adding leases. Reworking this into per-device/per-session quarantine needs a separate ownership proof and bounded-retention design.
- Audit GL/EGL/GBM and Vulkan teardown for shared device/display/cache lifetimes as well as KMS ownership. Leases alone do not make every native resource independent.
- Application supervision configuration is not a library concern. Each viewport owns its internal lifecycle/recovery supervision, with bounded retries/waiting-for-output behavior rather than a crash loop. The library does not promise to keep a viewport alive if the application deliberately stops its unit.

### Acceptance tests

- Run A and B; stop/restart A repeatedly while B continues page flips with unchanged connector/CRTC/plane ownership.
- Separately kill A's callback, lifecycle owner and whole unit with `:kill`, including a retained renderer reference elsewhere. Verify internal monitoring, native fallback and cleanup without an application supervisor; verify replacement attachment only after safe release.
- Inject failures at each startup stage and panics in essential actors; verify reservation rollback, health reporting, and absence of orphan sessions.
- Race concurrent starts/stops on one output; verify exactly one owner and protection against stale-generation cleanup.
- Inject pending-flip/fence timeout and teardown failure; verify unsafe resources are not reused and the actual quarantine scope is reported honestly.
- Disconnect/reconnect A, remove its selected mode, and exercise device-wide loss separately; verify output-local versus card-level failure classification.
- Kill/restart the VM on real DRM hardware; verify master can be reacquired without relying on userspace destructors.

No implementation changes are authorized yet; these recovery requirements are part of plan review.
