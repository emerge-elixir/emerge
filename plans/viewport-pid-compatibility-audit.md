# Viewport PID compatibility audit

> Implementation resolution: preserve the public callback PID and use private per-viewport
> lifecycle supervision. No application callback or automatically started global runtime.
> See [actual progress and validation limits](drm-implementation-progress.md); proposed
> topology/requirements below are not a claim that every design item is implemented.

Status: repository audit and implementation requirements, not code changes.
Related: [lifecycle/recovery plan](drm-renderer-lifecycle-recovery.md), [DRM output plan](drm-output-selection.md).

## Scope and conclusion

Reviewed tracked library code, Rust recipient/relay paths, relevant tests, guides and public examples. Searched both viewport names and indirect usage (`self()`, stored event PIDs, `send`, `GenServer`, `:sys`, links/monitors, registration and producer/target PIDs). Geometric/layout uses of the word viewport are unrelated. External applications, the separate demo repository and dependency internals are not covered; documented Solve/Membrane integration assumptions are included below.

**A viewport PID is currently a callback-process identity, not just a renderer lookup key.** Replacing it with a stock supervisor PID only works for explicitly adapted lookup APIs; it breaks arbitrary messaging and changes monitoring/inspection semantics. Do not approve an API migration based solely on frame submission working.

## Distinct identities in the new design

1. **Unit identity/root PID:** lifetime of the library-owned supervised unit.
2. **Callback PID:** `mount`, `render`, `handle_info`, input/close callbacks, subscriptions and process-local state.
3. **Lifecycle PID:** owns admission/cleanup/retry decisions and monitors the callback.
4. **Renderer session ID/handle:** changes on native renderer replacement; not a BEAM process identity.
5. **Other recipients/owners:** arbitrary event targets, headless frame sinks, PRIME relay/LeaseOwner, log targets, tasks and video producers. These must not be rewritten as viewport/unit PIDs.

Keep these explicit in module APIs/state. Native resource ownership and diagnostic/input routing are separate. Raw PIDs held by callers do not automatically follow a process restart.

## Repository inventory

| Location / usage | Current dependency | Required handling |
|---|---|---|
| `lib/emerge.ex`: generated `start_link/1`, `child_spec/1`; `runtime/viewport.ex`: `start_link/2` | Returns/links the actual callback GenServer; caller can send messages and observe its exit | Decide root vs callback return contract before implementation. Never return an unlinked child as if it were the supervised root |
| `runtime/viewport.ex`: `@genserver_start_options`, `child_spec/2` | `name`, timeout/debug/spawn/hibernate options apply to the callback; child ID derives from module/name; type is worker | Specify name ownership and where each option applies. Test atom/`:via`/`:global` registration and duplicate names. Do not silently register a user's message-target name on the supervisor |
| `Emerge.renderer/1`, `Runtime.Viewport.renderer/1` | `GenServer.call(pid, {:emerge_viewport, :renderer})` goes directly to callback state | Adapt explicitly if a unit PID is accepted. Define not-ready/restarting behavior; a previously returned native handle is session-specific, not magically updated |
| `Emerge.submit_video_frame/3`, `runtime/video_endpoints.ex` | Direct `:persistent_term` lookup keyed by callback PID; bypasses all GenServer mailboxes | Lifecycle-owned, generation-scoped registration under the chosen public identity. Preserve direct submission and frame ownership. If callback aliases are supported, remove old aliases on callback death |
| `runtime/viewport.ex`: initial upload / `terminate_viewport/2` | Registers/unregisters the video endpoint with `self()` | Move responsibility to lifecycle/owner-aware registry. Current `:persistent_term` entries do not disappear on `:kill`; whole-unit death also requires automatic or externally monitored removal, not just `terminate/2` |
| `lib/emerge/ui/event.ex` | Every bare pointer/change/focus/key handler captures `{self(), message}`; virtual-key hold events contain explicit PIDs | Run UI construction in the callback process, not lifecycle/startup tasks. Rebuild trees after callback replacement. Preserve explicit arbitrary/remote destination PIDs |
| `engine/event_registry.ex`, `engine/diff_state.ex`, viewport `route_element_event/4` | Stores `{pid, message}` by element ID; dispatch uses raw `send`; viewport applies `wrap_payload/3` | Keep payload wrapping in callback context. Do not remap all stored PIDs or redirect external targets. Discard old callback's event registry/diff state |
| Viewport callbacks, `rerender/1`, `delegate_handle_info/2` | User messages arrive directly; rerender uses `GenServer.cast(self(), ...)`; mount/render/callbacks share process state | Preserve the callback execution context and mailbox semantics. Lifecycle recovery asks the callback to render; it must not invoke user render code itself |
| `guides/tutorials/state_management.md`: `Solve.Lookup` | Render-time lookups subscribe/cache in the calling process; update messages drive its `handle_info` | Renderer-only restart preserves callback/subscriptions. Callback restart rebuilds subscriptions in the replacement. No moving render/lookup work to short-lived tasks |
| Viewport `set_input_target` / `set_log_target` in initial upload; `renderer/skia.ex` and transport adapters | Both currently receive callback `self()` | Keep input/close callbacks routed to the current callback (or a generation-tagging relay), independently of lifecycle owner. Route terminal health to lifecycle, not only callback |
| Rust `lib.rs`: `InputTargetRelay`, `set_input_target`, heartbeat; `events/runtime.rs`; `native_log.rs` | Stored `LocalPid` destinations; heartbeat and close share the input target; startup logs default to NIF caller `env.pid()` | Do not point all recipients at the native owner. Explicitly preserve startup diagnostics when the NIF caller becomes a task. Guard input/close/heartbeat delivery against stale sessions |
| `runtime/viewport.ex`: heartbeat handling / watchdog timer | Timer sends to `self()`; logs/input/close currently refresh heartbeat; messages lack a renderer generation | Move health polling/timers to lifecycle, cancel/tag timers. Logs or old-session messages cannot prove the new renderer is healthy |
| `runtime/viewport/reload_group.ex`, code reloader | `:pg` stores callback PIDs; broadcasts send source-reload messages directly | Keep callback membership or use an intentional reload endpoint, never accidentally the supervisor. Rejoin once on callback restart, remove dead members, avoid duplicate rerenders after native-only recovery |
| Viewport `handle_info({:EXIT, ...})`, `terminate/2`; existing tests | Traps exits, ignores normal linked exits, stops on abnormal exits; public PID is monitored/linked/stopped by callers | Specify unit vs callback exit semantics; audit new internal links so task/helper exits do not accidentally become application callback messages or whole-unit shutdown |
| `lib/emerge_skia/headless_prime_session.ex` | `start/1` captures `producer = self()`; session monitors producer and destination, forwards logs; relay/LeaseOwner control safe drain | Pass an explicit long-lived producer/lifecycle PID when using startup tasks. Task completion must not trigger producer `DOWN`. Preserve the PRIME drain chain; do not force native stop ahead of outstanding frame leases |
| Headless options/output (`options.ex`, Rust `backend/headless`, PRIME relay) | `headless.target` is a real live local frame recipient; native PRIME target is the relay PID | Leave sink and relay identities intact. A supervisor PID may pass a liveness check but is not a valid implicit frame consumer. If a callback chooses itself as sink, it remains callback-generation-specific |
| `lib/emerge_skia/macos/{host,session}.ex` | Per-session input/log PIDs; buffered resize/focus/close/events/logs; GenServer replies target startup caller | Preserve recipient roles and session association across the new lifecycle/task boundary. Late buffered close/input from an old renderer cannot reach a replacement as if current |
| Public/docs/tests using `GenServer.stop`, `Process.monitor/link/exit`, `:sys.get_state/replace_state`, `Supervisor.which_children` | Operate on the returned callback PID, including callback-state shape and exit reason assertions | Classify each as public compatibility or test-internal usage. Adapt internal tests deliberately; do not silently redefine observable PID behavior just to make tests pass |
| `guides/internals/video-interop-architecture.md`: Membrane sink callback | Producer/pipeline retains the supplied viewport PID in an MFA/closure | Keep the chosen unit lookup identity stable across renderer replacement. If API identity changes, give an explicit migration; producers do not discover callback replacements automatically |

## Newly identified blockers / fixes

1. **`self()` is semantic.** UI events, timers and Solve subscriptions attach to the callback process. Only native start/stop waits belong in lifecycle tasks; callback execution cannot move there.
2. **PRIME startup from a task is unsafe without adaptation.** Its current producer monitor would see that task exit and begin draining a healthy session. Supply stable producer identity explicitly, including early log routing. Distinguish the library lifecycle owner from the relay/LeaseOwner that must preserve borrowed-frame safety during shutdown.
3. **Endpoint cleanup cannot rely on `terminate/2`.** Existing persistent-term mappings can retain a dead viewport's renderer after `:kill`. Use owner-bound registry lifetime or a monitor outside the killed unit to remove them; retain direct frame lookup. Evaluate an OTP Registry/ETS-backed owner-bound mapping rather than automatically copying the present storage design. Never add a per-frame GenServer call.
4. **Tag more than terminal notifications.** An old renderer's queued input, close or heartbeat can affect a new session using the same callback PID, especially when node IDs are reused. Add opt-in session-tagged internal delivery (or a per-session relay) while preserving low-level public message compatibility. Stale close must not close the replacement.
5. **Registration/removal must be generation-safe.** Old cleanup cannot erase a replacement endpoint. Callback aliases must not survive callback death. Registry state must disappear on owner or whole-unit death, and frame submit racing with removal must still consume/release exactly once.
6. **Root PID compatibility remains a design gate.** The frame API can use a supervisor PID as a key; raw `send(root, message)` cannot reach the callback through an ordinary Supervisor. Specify public endpoint, naming, linking/monitoring, stopping and lookup APIs together before selecting the topology's public facade.

## Frame path to preserve

```text
Producer -> Emerge.submit_video_frame(public_viewport, target, frame)
         -> direct endpoint lookup -> current native renderer
```

Lifecycle owns registration, but never receives high-rate frames. Publish only after successful initial upload/readiness. While absent/recovering, consume/release the frame and return `:viewport_not_ready`; do not queue it or resubmit it to a replacement. A racing submission is either accepted by the old session and retired there, or rejected and released under the existing ownership contract.

## Required compatibility tests

- Public PID/name/message contract: start, direct `send`, renderer lookup, video submission, stop, caller links, monitors and exit reasons. Include named registration and two instances of the same viewport module.
- Bare event payloads plus explicit other-process targets for pointer/change/focus/key/virtual-key events; `wrap_payload/3` and self-rerender still run in the callback process.
- Callback-local subscription/timer fixture (and Solve integration where available): native renderer restart leaves subscriptions intact; callback restart creates fresh ones and discards the old registry.
- Hot reload hits the live callback exactly once after repeated renderer/callback restarts; no supervisor receives UI/reload messages unintentionally.
- Inject old-session input, resize, close, log and heartbeat after replacement. They cannot dispatch against a new event registry, report it healthy or stop it.
- Kill callback, lifecycle owner and whole unit separately while retaining public/native handles elsewhere. Check registry entries, callback aliases and reservations; old cleanup cannot remove the replacement mapping.
- Race borrowed-frame submission with endpoint removal/replacement; verify exactly one release and no replay into the new session. Keep the submission path mailbox-free.
- Start PRIME through a managed task, let the task exit, and verify output remains live; actual producer/sink death still begins ownership-safe draining. Test early log delivery and outstanding leases during unit death.
- macOS buffered events and headless sink targets keep correct recipients. Existing fake renderer adapters continue to exercise intended callback identity, not accidental startup-task identity.
- Capture the registered PID in test cleanup closures; ExUnit `on_exit` runs in another process, so calling `self()` there is not an unregister of the original endpoint.

Primary suites to extend: `test/emerge/viewport_test.exs`, `test/emerge/code_reloader_test.exs`, `test/emerge/ui_test.exs`, `test/emerge_skia/video_interop_session_test.exs`, `test/emerge_skia/macos/host_test.exs`, and native lifecycle/event tests.

Update `lib/emerge.ex` / viewport/event module docs, `guides/tutorials/{set_up_viewport,state_management}.md`, video-interop internals, and any migration/example affected by the final endpoint decision. No application supervision strategy is required by this design.
