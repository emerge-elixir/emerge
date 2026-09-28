# DRM renderer lifecycle and recovery

> Implementation resolution: preserve the public callback PID and use private per-viewport
> lifecycle supervision. No application callback or automatically started global runtime.
> See [actual progress and validation limits](drm-implementation-progress.md); proposed
> topology/requirements below are not a claim that every design item is implemented.

Status: proposed implementation plan; review before coding.
Parent: [DRM output selection](drm-output-selection.md).
Required compatibility gate: [Viewport PID audit](viewport-pid-compatibility-audit.md), covering public messaging, events, reloads, process-local subscriptions, video endpoints and backend recipient/owner roles.

## Goal and limits

A viewport-local failure must stop/release only that viewport, permit restart after safe cleanup, and leave another viewport presenting. Keep the shared DRM master while another session needs it.

This is not isolation from native memory corruption, process abort, GPU reset or driver failure. Unresolved GPU/KMS ownership remains a non-restartable fault. Do not weaken existing Vulkan safety rules to make a restart appear successful.

The application's supervision tree and restart strategy are out of scope. Cleanup and renderer recovery belong to a library-owned viewport unit and must work when it is started directly, under any application supervisor, or never restarted by its caller.

## Viewport-owned supervision

Proposed internal topology (one independent unit per viewport):

```text
Viewport.Supervisor                  # library-owned, :rest_for_one
├── RendererLifecycle               # owns native sessions; monitors callback process
├── Task.Supervisor                 # bounded startup/stop wait tasks, not GPU owners
└── Viewport.Server                 # mount/render callbacks and application state
```

- Start lifecycle owner first, callback server last. Reverse shutdown order stops callbacks before the owner. The lifecycle process observes callback `DOWN` and requests cleanup even when the callback's `terminate/2` never ran.
- The lifecycle owner stays alive through callback crashes. A restarted callback waits for the previous generation to release its session before attaching a replacement; it never races cleanup.
- Renderer-only failure is handled inside the unit: lifecycle owner cleans up, applies retry policy and starts a new renderer, then asks the live callback server for a full tree upload. Do not kill the callback server or depend on an application supervisor to replace the renderer.
- If the lifecycle owner itself dies, the internal supervisor tears down dependent children before restarting them. For direct DRM sessions, native Rustler monitoring of the lifecycle PID independently cancels them. PRIME's adapter must preserve its relay/LeaseOwner draining chain rather than forcing native teardown before outstanding leases retire. New owner/worker generations must still wait for old native cleanup or observe quarantine.
- The owner stays responsive: potentially blocking native start/stop waits run as bounded managed tasks, with task completion tagged by owner/callback/session generation. Native session ownership is the lifecycle PID, never a short-lived task PID. Publish a cancellation handle before blocking startup begins so callback death can also cancel an in-progress start.
- Intentional callback close stops the whole unit, not an automatically resurrected window. Use OTP significant-child/auto-shutdown support where the supported OTP floor permits it, or an explicit unit-close protocol otherwise. Test this separately from abnormal callback restart.
- If the whole unit is killed, native owner monitoring still requests cleanup; neither the lifecycle owner's `terminate/2` nor a surviving Elixir supervisor is required for native safety.
- All retry limits and child restart strategy here are internal to this viewport unit. If the unit itself exits, whether the application restarts it is solely the application's decision.

### Public PID compatibility — resolve before implementation

Today `start_link/1` returns the callback GenServer PID, `Emerge.renderer/1` calls it directly, video endpoints are keyed by it, and applications can send it arbitrary messages. A stock OTP supervisor PID cannot silently replace that endpoint.

- Keep distinct concepts for the supervised unit and the callback/message endpoint. Audit `start_link`, `child_spec`, named registration, callback `self()`, direct `send`, renderer lookup and video submission.
- Choose an explicit compatible lifecycle entry point or a documented endpoint API before changing the returned PID. Do not implement a fake Supervisor that forwards arbitrary callback messages, or return an unlinked child PID while claiming it represents the supervised root.
- The internal cleanup/ownership design does not depend on the API choice. Returning the supervisor root is acceptable only with an explicit endpoint/migration contract; preserving current direct-PID behavior requires a compatible public endpoint arrangement.

## Proposed contracts

- One authoritative lifecycle record per session, with a fixed-width session ID. Separate operational state (`starting`, `running`, `stopping`, `stopped`, `quarantined`) from outcome (`normal` or structured failure) and cleanup completion.
- Failure records carry `reason`, `scope` (`session`, `card`, `process`), and recovery (`after_cleanup`, `retry_output`, `vm_restart`, `configuration_required`). No error-string parsing for policy.
- `running?/1` is true only for a healthy running session. It is not proof of output availability after returning false.
- Proposed `EmergeSkia.renderer_status/1`: `{:ok, %{session_id: ..., state: ..., failure: ..., cleanup_complete: ...}} | {:error, reason}`. Notifications include session ID; status is authoritative if a notification is missed.
- Proposed `EmergeSkia.stop(renderer, timeout: 5_000)`; `stop/1` uses the same finite default. Return `:ok` only after successful cleanup, otherwise a structured timeout/failure. Timeout never authorizes resource reuse. Update transport/renderer specs, not just the public wrapper.
- Proposed low-level `owner: pid | nil`, default `nil` for compatibility with existing resource-lifetime behavior. Direct DRM viewport sessions use the library-owned `RendererLifecycle` PID, which monitors their callback process. Adapters with additional safety owners, notably PRIME's relay/LeaseOwner, must map lifecycle death to safe draining rather than bypassing it. Input/log targets do not transfer ownership. Reject invalid/dead/remote owners. No live ownership transfer in this change.
- Initial native startup wait has a finite total deadline covering all retries/fallbacks; proposed default 15 seconds. Cancellation/late success cannot publish a running session after timeout or owner death. This bounds the caller's wait, not a kernel/driver call.
- Apply the core native lifecycle consistently where `RendererResource` is shared. Preserve headless PRIME lease accounting and macOS host semantics; unsupported new transport capabilities must be explicit, not silently ignored. Custom viewport renderers get an optional status capability, with legacy behavior retained if absent.

## 1. Centralize lifecycle and completion ownership

**Files:** `native/emerge_skia/src/lib.rs`; new `native/emerge_skia/src/runtime/lifecycle.rs` and tests.

- Extract the existing running/stop flags, shutdown ownership and terminal result into one session control object. Keep backend loop flags as derived implementation details, not independent health authorities.
- Model transitions with pure functions and test first-failure retention, monotonic cleanup, quarantine escalation, and duplicate stop/failure notifications. A normal stop request cannot overwrite a failure.
- Give one per-session coordinator responsibility for startup completion, cancellation and cleanup completion. Workers and resource callbacks hold plain Rust control references, not self-retaining cycles of `ResourceArc<RendererResource>`.
- Completion is persistent, not a one-shot consumed by the first waiter. Concurrent/repeated `stop` callers observe the same result. Taking thread handles must not make later stops incorrectly report success.
- Split nonblocking stop request from bounded wait. Normal-scheduler NIFs use atomics/nonblocking snapshots; any bounded waits run on `DirtyIo`. GPU calls stay on their owning threads.

**Done when:** state-machine tests cover stop/failure races and two waiters cannot observe false cleanup success.

## 2. Monitor ownership before startup and close rollback gaps

**Files:** `lib/emerge_skia/{options,native}.ex`, `lib/emerge/runtime/viewport/renderer/skia.ex`, `native/emerge_skia/src/lib.rs`.

- Construct/register the monitored resource and session controller before spawning actors or acquiring display/GPU resources. Implement `down` inside `#[rustler::resource_impl] impl Resource for RendererResource`.
- Register the selected owner through Rustler's resource monitor API. Monitor-registration failure cancels startup. Owner death between registration and successful startup is handled by the same cancellation path. Direct DRM viewports use the lifecycle manager as native owner, not the callback server or startup task.
- Adapt existing caller-dependent transports before moving start into tasks: PRIME currently captures/monitors `producer = self()`, and native startup logs default to `env.pid()`. Pass long-lived producer/log identities explicitly. Keep PRIME's native safety owner and frame relay/draining protocol separate from the application lifecycle identity.
- Add an internal prepare/start handshake that publishes the resource/cancellation handle to the lifecycle manager before blocking initialization. Its callback-process monitor can then cancel startup without waiting for a result or killing a NIF task. Keep the public synchronous `start/1` wrapper over this protocol.
- `down` and resource destruction only latch cancellation and wake the controller; no joins, DRM ioctls, video draining, blocking mutex acquisition or panic-prone cleanup on BEAM callback threads.
- A startup guard owns every partial initialization step. Replace scattered error returns/manual rollback with the same cleanup path, including errors after backend startup but before resource return, asset initialization failure, thread-spawn failure and owner loss.
- Replace unbounded startup channel receives with the total startup deadline. Recheck cancellation immediately before committing success.
- OpenGL-to-raster fallback may begin only after the previous attempt safely released its resources. It must not bypass busy, ownership loss, timeout or quarantine errors, and must retain the exact output/mode selection.

**Done when:** killing the viewport during startup and while running initiates cleanup even if another process retains the renderer term; all startup failpoints leave either a released session or an explicit retained/quarantined one.

## 3. Replace heartbeat-only health with supervised workers

**Files:** `native/emerge_skia/src/lib.rs`, `runtime/tree_actor.rs`, `events/runtime.rs`, `drm_input.rs`, `backend/drm/{gl,vulkan}.rs` and supporting asset/video workers.

- Add a common worker-exit guard for every essential thread. Unexpected return, channel disconnection or unwinding reports failure, clears healthy state and requests sibling cancellation. Returns during an acknowledged stop are normal.
- Audit which auxiliary worker failures are terminal versus degraded functionality; do not treat absence of an input device as a dead renderer. Include asset/release workers where their loss prevents completion.
- The existing heartbeat becomes a report of authoritative session health. It cannot keep a session healthy after a required worker exits. Record worker identity and failure once.
- Add pending-work progress deadlines for accepted GPU submissions/page flips and queued CPU work. No progress alarm on an idle/static viewport; expensive asset/layout work needs its own appropriate deadline, not a frame-rate assumption.
- Put panic handling at actual worker boundaries. For presenters, retain GPU/KMS ownership outside the unwind-prone operation, catch before uncertain resources are destroyed, then attempt only a proven-safe shutdown or quarantine. A generic outer `catch_unwind` after destructors have run is not a safety solution.
- Never resume using a panicked GPU session. Audit partial-initialization and teardown destructors; panic-free ownership guards must retain uncertain resources on the correct thread. Segfault, abort and double-panic remain process failures.

**Done when:** injected failures in tree, event, input and presenter workers invalidate health promptly; a heartbeat cannot mask them; CPU-only recoverable failures permit safe teardown/restart.

## 4. Make shutdown bounded to callers without unsafe thread killing

**Files:** shutdown helpers and `CleanupDispatcher` in `native/emerge_skia/src/lib.rs`, actor loops, `lib/emerge_skia/transport/native.ex`.

- Replace shutdown delivery dependent on full tree/event data queues with an independent cancellation signal. Blocking actor sends/selects must observe cancellation. Signal all components before waiting on any one.
- Move potentially blocking asset stop, video drain, presenter teardown and joins behind the per-session controller/worker ownership. The status/deadline controller itself never blocks in those operations.
- Use one absolute shutdown deadline across stages, not a new full timeout per worker. Only join threads already known to have exited; retain unfinished handles and their resource owners if the deadline expires.
- A timeout returns an error promptly while native cleanup may remain alive. Mark unresolved GPU/KMS resources quarantined; do not detach them and then advertise the output as free. Quarantine is sticky for the initial implementation, even if late work later completes.
- Reserve bounded cleanup/retention capacity before starting a session. One stuck session gets one retained record, not new threads on every retry. Reject further allocations when the applicable budget is exhausted; never evict uncertain resources to make room.
- Keep A's cleanup independent of B's queues and locks. No process/card registry lock while waiting for GPU work, actor completion or destructors.
- Audit existing `CleanupDispatcher` abort paths. Expected cleanup errors become recorded failures; a catchable panic may be contained only if safe retention is already guaranteed. Do not remove aborts for unrecoverable ownership/invariant corruption merely to keep the VM alive.
- `stop/1` and `stop/2` preserve failure results across repeated calls. Resource destruction remains a best-effort request, not a hidden unbounded wait.

**Done when:** stalled workers, full actor queues and stalled cleanup return within the caller's deadline; B continues rendering; repeated stop/start attempts neither report false success nor grow retained resources unboundedly.

## 5. Integrate shared card ownership and restart-safe leases

**Files:** `native/emerge_skia/src/backend/drm/{core,mod,gl,vulkan}.rs`; new card/session ownership module. Depends on phases 1–4 and the parent's discovery/selection work.

- Key the shared owner by validated DRM device identity, not merely the path string. Track card incarnation so device removal/replacement cannot reuse stale reservations.
- Serialize short admission transitions and assign a generation to every reservation. Track connector, encoder routing, CRTC, primary plane and optional cursor plane. Select free compatible resources without disturbing another session.
- Reserve before slow allocation, perform slow setup outside registry locks, then commit only if generation/cancellation/card state still matches. A per-card initialization gate prevents two racing master opens.
- Create a separate DRM lease/fd per session; never duplicate a shared event-reading fd as a substitute. Hold the master independently of presenter lifetimes. Remove per-presenter DROP_MASTER calls on the shared card.
- Release only after acknowledged safe output teardown and resource cleanup; quarantine keeps the reservation and required card ownership. Stale cleanup cannot release a replacement's reservation.
- GL and Vulkan cleanup touches only that session's connector/CRTC/planes. Audit modeset snapshots, framebuffer/blob lifetime, cursor teardown and every fallible allocation. Correct GL's current behavior of logging teardown failure and continuing to destroy scanout resources.
- Audit EGL display initialization/termination, GBM owners, Vulkan device resources, video import ownership and global caches. Refcount genuinely shared lifetimes; no global EGL termination or device reset to recover A while B is live.
- Card failure invalidates all its sessions explicitly. Do not recover a card-level fault by silently migrating outputs or taking resources from B.

**Done when:** two real outputs can present independently; repeated A stop/start and partial startup failures leave B's master, routing, GPU context and page flips intact. Duplicate starts fail deterministically as busy/stopping/quarantined.

## 6. Separate local faults from genuinely unsafe GPU faults

**Files:** DRM presenters, lifecycle module, `native/emerge_skia/src/video.rs` and Vulkan support modules.

- Replace the blanket `terminal error -> quarantine` classification with an explicit cleanup decision: was work submitted, is completion known, has scanout been safely detached, and are imported-video leases still owned?
- A cleanly quiesced output disconnect, actor failure or pre-submission error is session-local and restartable. Unresolved submission/fence/page flip, device loss or failed teardown remains unsafe. Never infer safety just from an error category.
- Preserve process-wide Vulkan terminal admission for uncertain resources in the initial implementation. Report that scope to callers and existing sessions; do not claim B remains fully usable or restartable after such a fault. Clean local errors must not unnecessarily set this global flag.
- Audit all existing runtime/import admission checks against the reported scope. Affected sessions receive a terminal diagnostic and stop accepting unsafe new work; any shutdown must retain the same ownership guarantees.
- Keep retention bounded across multiple sessions, including the current DRM Vulkan single-retained-session assumption. Admission/retention accounting must cover concurrent failures without an unbounded `mem::forget` fallback; exhaustion rejects new sessions before allocations.
- Narrowing true uncertain-ownership quarantine to one session/device is a separate gated change: enumerate every shared GPU/import owner, prove sibling independence and test device-loss/retirement races first. If that proof cannot be made, keep VM restart as the documented recovery.

**Done when:** clean local errors restart without poisoning B; unsafe errors retain resources and consistently report `vm_restart` with honest scope. No policy weakening solely to satisfy a restart test.

## 7. Introduce viewport-owned supervision and recovery

**Files:** `lib/emerge/runtime/viewport{,/state,/config,/renderer,/renderer/skia,/reload_group}.ex`, `runtime/video_endpoints.ex`; new viewport supervisor/lifecycle-owner modules; transport adapters including `headless_prime_session.ex` and macOS session routing; `lib/emerge{,_skia}.ex`; viewport, event, endpoint and reload tests. Use the PID audit for the complete call-site inventory.

- First settle the public endpoint compatibility contract above and satisfy the PID audit. Make the library own the internal topology, monitor registration and shutdown order without assuming any application supervision configuration.
- Preserve callback-local `self()` semantics for UI event helpers, rerender casts, timers, Solve subscriptions, payload wrapping and arbitrary application messages. Do not evaluate user render/mount callbacks inside lifecycle or startup tasks. Keep explicit external event destinations and headless frame-sink PIDs unchanged.
- Move native renderer acquisition, handle retention, status monitoring, cleanup and retry timers from callback-server state into `RendererLifecycle`. The callback server keeps mount/render state and only the current generation's rendering attachment; all tree/video operations must reject or ignore stale attachments appropriately.
- Establish callback monitoring before startup, and lifecycle-owner monitoring natively before allocation. Use a handshake and fixed generation tokens, not races between `terminate/2`, task result delivery and a new callback registering itself.
- Add optional renderer status support and session-tagged lifecycle events. Native terminal events reach the lifecycle owner directly and bypass the heartbeat fast path; status polling covers missed notifications. Input/log recipients can remain the callback server. Internal input/resize/close/heartbeat delivery also needs session tagging or a session relay so an old renderer cannot act on a replacement's callback/event registry; preserve low-level public message compatibility.
- Keep video submissions as direct registry lookups, not supervisor/lifecycle mailbox traffic. Use owner-bound endpoint registration or monitoring outside the unit so whole-unit `:kill` cannot retain stale `:persistent_term` entries. Apply generation-safe registration/removal, including callback aliases if supported; preserve exactly-once frame ownership on lookup/shutdown races.
- Recover a safely stopped native renderer in place inside the unit. Preserve application callback state, reset the native diff state, re-register generation-scoped video endpoints and upload a fresh full tree. Do not exit the viewport merely to make an external supervisor run recovery.
- On callback crash, its owner observes `DOWN`, cancels outstanding startup/render work and starts cleanup. The internally restarted callback cannot attach a new renderer until cleanup completes; stale task results are cleaned up, never published to the replacement.
- On lifecycle-owner death, native monitoring is the fallback. Restarting the internal owner and callback must not adopt old handles or treat loss of an Elixir reference as completed native cleanup.
- Preserve structured startup errors; currently `start_and_upload_renderer/3` turns failures into log strings and can leave a viewport with neither renderer nor retries.
- Lifecycle states include `waiting_for_output`, `waiting_for_cleanup` and `unrecoverable`. Missing/disconnected output uses cancellable capped backoff (proposed 250 ms to 5 s). Always retain the exact explicit output/mode.
- Conflict with another live viewport, invalid configuration and quarantine are reported distinctly and do not trigger blind restart loops. Persistent recoverable failures get a bounded internal retry policy. In unrecoverable state keep diagnostics/control responsive while refusing unsafe native work.
- Normal close shuts down the entire unit intentionally. Full unit termination remains safe if callback/lifecycle `terminate/2` is bypassed: native owner monitoring and resource cleanup complete or quarantine independently. Report cleanup errors; do not wait indefinitely in BEAM shutdown callbacks.
- Keep transport compatibility explicit for custom renderers, macOS host sessions and headless PRIME. The library's internal supervisor is not a substitute for native thread/GPU lifetime control.

**Done when:** without relying on an application supervisor, renderer failure recovers inside A, callback `:kill` cleans up and gates internal restart, owner `:kill` triggers native fallback, and whole-unit `:kill` leaves no reusable-but-unsafe resources. B is untouched; normal close leaves no orphan internal unit.

## 8. Validation and delivery gates

Resolve the viewport unit/endpoint contract and PID audit first. Deliver phases 1–4 as native lifecycle groundwork, then 5–6 with output leases, then 7 with internal supervision and recovery. Run the PID audit's compatibility tests alongside the gates below. Do not advertise independent crash recovery until the combined gates pass.

| Layer | Required coverage |
|---|---|
| Pure Rust | State transitions, first-fault retention, concurrent stop waiters, cancellation during startup, admission/rollback, stale-generation release, bounded quarantine accounting |
| Native integration | Owner monitor and `:kill` with retained resource references; failpoints at startup stages; essential-worker panics; full-channel cancellation; fake blocked teardown and finite caller waits; exact terminal error shapes |
| ExUnit | Direct-started viewport units without an application supervisor; renderer-only in-place recovery; callback/owner/unit `:kill`; internal restart gating; normal close of the whole unit; PID/name/message/video API compatibility; session filtering; retry cancellation; custom renderers |
| DRM hardware | A/B render counters; kill/stop/restart A repeatedly; GL/raster and Vulkan configurations including mixed presenters; same/different cards; mode removal and hotplug; duplicate start; shutdown with a pending flip; master reacquisition after VM SIGKILL |
| Fault safety | Simulated unresolved fences/page flips and teardown failure preserve ownership; process-global Vulkan quarantine accurately affects admission; retained memory/fds/threads stay within the declared budget |

- Use test-only injection points, never production NIFs that deliberately panic. Test fatal native/process behavior in a subprocess so it cannot take down the test runner.
- Assert B's presentation continues, not merely that its `running?` flag remains true. Check fds, native threads and reservations after repeated A failures.
- Run `cargo test`, DRM-enabled Rust tests/lint for GL and Vulkan-only builds, and `mix test`; use `./ci-tests.sh` for full local coverage. Validate non-DRM/no-GPU feature builds since lifecycle code is shared.
- Record which hardware fault cases were actually exercised. If no multi-output DRM device is available, hardware gates remain unverified rather than assumed passed.
- Document public defaults, endpoint identity, library-owned lifecycle behavior, and the difference between stopped, reusable and quarantined. Do not prescribe application supervision/restart policy. A native helper-process architecture is deferred unless isolation from segfaults becomes a requirement.

No production code changes are part of this planning task.
