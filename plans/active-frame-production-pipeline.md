# Default frame production: one frame ahead, driven by output readiness

Status: Linux implementation complete; macOS integration and hardware
qualification remain open.

## Commit sequence

The implementation was rebased onto `main` at `ac57e5f`. Shared protocol
compatibility comes first, with each backend's implementation and regressions together:

1. Runtime: bounded requests, per-admission presentation anchors, tree sampling,
   registry backpressure and scene-metadata compatibility.
2. GPU DRM: GL/Vulkan successor requests after accepted commits, slot metadata,
   bounded drains, duplicate-preparation protection and commit retry deadlines.
3. CPU DRM: asynchronous triple buffering, matching flips, retries and safe detach.
4. Wayland: normal-submission credits, callback identity and late-video coverage.
5. Headless: early scene preparation, cadence/lease admission, retry recovery and
   autonomous native animation coverage through the Elixir API.
6. Documentation: the default production contract, validation and remaining gates.

Each code commit's staged snapshot passed `cargo check` with
`drm-all,wayland-all,headless-all` and `--all-targets`. Full local CI and the
Rust feature matrix passed on the final rebased implementation.

## Implementation tracking

- [x] Shared bounded frame-request transport, prediction and first-frame anchors.
- [x] DRM GL/default look-ahead and scheduling regressions.
- [x] DRM Vulkan and asynchronous CPU scanout.
- [x] Wayland/headless integration without weakening output backpressure.
- [ ] macOS tree worker, native frame opportunities and platform validation.
- [x] Linux CI, feature-matrix tests and documentation.
- [ ] Target-device measurements and hardware qualification.

### Implemented contract

`backend/production.rs` contains the small shared credit/acknowledgement transport;
existing cadence/prediction helpers and concrete backend resource owners remain.
The tree actor acknowledges every request and preserves registry-before-scene
publication. Scene snapshots carry per-admission `OnceLock` observation tokens,
so predictions cannot anchor new transitions and reused node ids cannot inherit
an older frame's observation. Transport references in retained scenes are weak:
a queued request cannot retain its own sender or keep a stopped worker alive.

GL/Vulkan request their successor after accepted normal commits, not on flips.
Prepared pixels retain their own animation metadata. GL's retry deadline now
gates commits and its bounded cursor fairness window remains. Vulkan excludes an
already in-flight generation from preparation. CPU DRM uses three dumb buffers,
nonblocking commits, matching flip events, timeout handling and blocking detach.
Headless retains early scenes behind cadence/lease admission rather than issuing
another sample on retirement. Wayland retains normal callback identity and gives
animated late video replacements a later normal opportunity.

macOS still uses its existing AppKit-local runtime. Its worker/native-opportunity
refactor is deliberately left as the separately reviewable stage 5, rather than
changing unqualified native callback/lifetime behavior in this Linux increment.
No device access, deployment or latency improvement is implied by software checks.

### Software validation

Results for the implementation rebased onto `ac57e5f`:

- `EMERGE_SKIA_BUILD=1 ./ci-tests.sh`: passed (format, warnings-as-errors compile,
  Credo, Clippy, 664 ExUnit/doctests, 1,606 Rust release unit tests, Dialyzer).
- `cargo test --features 'drm-all,wayland-all,headless-all'`: passed, including
  1,732 unit tests; two device-only tests remain ignored.
- All-feature `cargo clippy --all-targets -- -D warnings`: passed.
- CPU-only DRM `cargo test` and all-target Clippy with
  `--no-default-features --features drm-raster`: passed (1,584 unit tests;
  one device-only test ignored).
- CPU-only `cargo build`, normal dependency-tree inspection and ELF `NEEDED`
  inspection confirmed no GBM, EGL, GL, Vulkan or PRIME interop linkage.
- Vulkan-only DRM and minimal headless `cargo check`: passed.
- Regression coverage includes slow output, no duplicate completion credit,
  registry backpressure/Stop, first/final enter poses, remount identity, request
  teardown ownership, bounded CPU scanout, Vulkan duplicate preparation, EBUSY
  deadlines, late Wayland video and autonomous headless animation/idle behavior.

The slow watch, real KMS power/reconnect/cursor/capture, PRIME held-lease delivery,
Wayland presentation and macOS still require explicit target-device qualification.
The existing Vulkan acquire synchronization issue remains out of scope.

- Worktree: `/workspace/emerge-drm-frame-pipeline`
- Branch: `plan/drm-frame-pipeline`, starting at `b66eef5` (tree-identical to
  main's `544de0f`), rebased onto `ac57e5f`.
- Reviewed: [PR #89](https://github.com/emerge-elixir/emerge/pull/89),
  head `30bf6e5`, based on `6b81166` rather than this branch's typed-config work.
- Scope: shared production/pacing policy, DRM first, then Wayland, headless and
  macOS. No `drm_animation_pacing` setting or new public pacing mode.

## Recommendation

**Keep one future frame in preparation while the output owns the current work.
When the output becomes ready, submit the prepared frame without waiting for a
new tree round trip.** Use one small shared policy, not one universal backend loop.

The presenter owns readiness, draw admission and buffer lifetimes. The tree worker
owns mutations, layout, animation sampling and scene/registry publication. Neither
waits synchronously for the other. Reuse the existing latest-scene handoff.

An accepted normal submission allows preparation of its successor, once. Its
completion frees presentation capacity; it does not issue a second animation
sample for that same successor. Actual output progress therefore bounds animation
production, with at most one look-ahead frame, not an independent animation timer
running ahead of a slow display.

This is a common default across backends. How much pixel work can overlap depends
on whether the backend has a writable buffer/drawable. A ready scene is useful even
when early drawing is not possible; a ready scene is **not** already-rendered pixels.

## Baseline source findings

Paths below are under `native/emerge_skia/src/`; line references describe `b66eef5`.

| Area | Finding |
|---|---|
| `lib.rs:2543`, `runtime/tree_actor.rs` | Separate tree/event/backend threads already exist on Linux. Production render queue capacity is one. Tree batches are bounded at 64, and registry publication precedes its associated scene. |
| `backend/drm/gl.rs:728–815,1298–1317,3746–3854` | GL already has `current`, `in_flight`, and one `prepared` primary. It can draw while a flip is pending if GBM has a free buffer. Staged pixels deliberately are not continually replaced by camera/scene traffic. |
| `backend/drm/gl.rs:3570–3591,3970–4034` | Animation pulses originate at primary flip completion, not submission. Thus animation-only work usually leaves that look-ahead capacity empty until the expensive panel wait ends. |
| `backend/drm/gl.rs:800–815` | Prediction is simply dispatch time plus mode interval. Kernel timestamps and commit-to-flip timings exist for diagnostics, but do not drive prediction. |
| `backend/drm/raster.rs:152–207,473–684` | CPU DRM has exactly two mapped buffers and a blocking atomic present. Drawing and the next animation pulse wait for its return. Moving only the GL pulse does not fix this path. |
| `backend/drm/vulkan.rs:223–390,1515–1809` | Three persistent scanout slots and explicit fence ownership already permit overlap. Pulses still originate at flip completion. Its draw predicate compares desired only with completed generation, unlike GL's additional in-flight-generation check. Test duplicate preparation before changing it. |
| `backend/wayland/runtime.rs:1519–1557,1701–1727`; `wayland/present.rs` | Wayland already requests the next animation sample after a successful normal submission, then gates normal drawing on the compositor callback. It also has bounded late patch/video replacement policies. Do not mechanically copy DRM flip semantics over this. |
| `backend/headless/{mod,binary}.rs` | Headless schedules a later timer, then asks the tree for a sample. Caps, binary-render retries and PRIME lease retirement already have separate control paths. PRIME exhaustion pauses animation. |
| `bin/macos_host.rs:719–895,1548–1578,2063–2097,2404–2605` | macOS calls the shared tree engine synchronously on the AppKit host loop. It polls every 10 ms and derives animation timing from submission returns, not display feedback. It does not currently have Linux's tree/presenter overlap. |
| `runtime/tree_update.rs:474–497,635–643`; `tree/animation.rs:737` | `AnimationPulse.presented_at` also anchors pending enter/exit clocks. Sampling ahead and reporting presentation are currently coupled. |

Related scheduling defects/risks to cover in the refactor:

- GL's `retry_commit_at` affects polling but does not gate the atomic-commit branch;
  the EBUSY `continue` can attempt again without waiting for the deadline.
- GL and Vulkan drain scene traffic with unbounded `while try_recv` loops. Use
  bounded drains so continuous traffic cannot starve commits, flips or Stop.
- GL captures `emit_animation_pulse` from the latest `render_state.animate` when
  committing, not necessarily the scene whose pixels were prepared earlier.
- GL/Wayland drop pulses when the tree channel is full; Vulkan/headless can block
  sending one. Neither is an adequate shared progress contract.
- After a state transition, some loops can sleep despite more work becoming
  immediately possible. Re-evaluate runnable work before polling.

These are source findings, not device measurements. PR #89 reports approximately
85 ms → 32 ms latency on a slow watch panel; its metric and hardware trace still
need clarification. More overlap cannot remove the panel's physical service time.

## Why not merge the PR as the solution

The useful change is starting successor work before the current flip completes.
Keep that idea, not its public flag or GL-only alternative loop.

The PR also:

- Supplies `now + estimated_commit_to_flip` as `presented_at`, even though that
  presentation has not occurred. This changes enter/exit anchoring semantics.
- Samples at that estimate plus nominal mode interval. The successor may itself
  require another slow transfer; this is not necessarily its visibility time.
- Learns commit-to-dispatch duration, which can include event-loop delay, and
  supplies neither native pacing tests nor other-backend coverage.

Do not change the meaning of existing timing fields to obtain overlap.

## Shared contract — small policy, concrete backend ownership

Extend/refactor `backend/cadence.rs` and `backend/present.rs` rather than add a
scheduler thread, backend trait hierarchy or second rendering framework.

The shared policy needs only:

- latest dirty work and its source generation;
- at most one outstanding animation-sample request and its reply;
- one successor slot, progressing through scene preparation / pixel preparation /
  ready, until submitted or safely invalidated;
- next eligible admission time, existing cap and retry deadlines;
- separately named output-progress observations and predicted sample times.

The backend keeps actual framebuffers, fences, swapchain images, drawables,
current/in-flight ownership and capability-specific callbacks. Do not duplicate
Vulkan's resource lifecycle in a generic scheduler.

### Invariants

1. One KMS commit in flight, one successor being prepared or ready. Extra producer
   updates replace only the undrawn latest scene, never grow a frame FIFO.
2. A successfully submitted frame grants at most one animation successor.
   Completion, cursor-only commits, captures, video cleanup, scene arrival and
   retries do not independently mint additional animation credits.
3. A slow/stalled output stops new animation-only sampling after the bounded
   look-ahead. A 120 Hz timer must not sample a 15 Hz panel 120 times per second.
4. Inputs, patches, assets, live controls and Stop remain responsive. Apply semantic
   messages in order; do not discard patches or throttle listener-registry replies
   to display cadence. Batch work and coalesce only its replaceable output.
5. Preserve the prepared frame's generation, scene identity, animation state and
   timing through submit/retry/complete. Do not consult newer state for its metadata.
6. Unchanged static scenes are idle. An active but visually unchanged animation,
   its final frame and exit cleanup must not lose their next progress opportunity.
7. Failed/Busy/unsubmitted work produces no successful-submission or presentation
   acknowledgement. Retry the retained work, not another animation step.
8. A prediction grants neither buffer reuse nor proof of presentation. Release
   resources only through the backend's existing completion/detach/fence rules.
9. Off, disconnect, resize/recreation and Stop invalidate future work using the
   existing session/replay boundaries. No stale callback may restart an old loop.

This bounds pixel work, not arbitrary application CPU work. Do not claim that the
current latest-scene queue already prevents all superseded layout construction.
Only expand tree-side deferral if measurements justify it; preserve registry and
animation publication transactions from `runtime/tree_update.rs`.

### Timing and first-frame correctness

Separate three facts in the internal interface:

- `submitted`: the backend accepted a particular frame;
- `output_progress`: real flip/readiness/delivery feedback, with its source;
- `sample_at`: prediction for when the successor will be useful.

Carry scene identity with observations; feedback for an older scene must not
anchor an enter/exit mounted only in a newer scene. Reuse existing scene versions,
mount identity and presenter replay boundaries, not a new public receipt protocol.

**Conservative bootstrap:** preserve the initial enter/exit pose and wait for the
first applicable backend progress observation before advancing newly pending
transient clocks. Then use the ordinary one-ahead cycle. Do not speculate a future
anchor and later rewind committed layout/registry/ghost state. One warm-up cycle
without full overlap is preferable to first-frame flashes or early completion.
Tests must cover a new transition inserted while an older frame is in flight.

Keep prediction separate from physical-refresh statistics:

- DRM: use monotonic kernel flip timestamps when valid, retain dispatch time
  separately, and model observed busy service time separately from mode refresh.
  A timer at a predicted flip must never act as the actual readiness event.
- Predict the successor's delivery, not just completion of the already-submitted
  frame. For a serial 65 ms transfer, two successive accepted frames need two
  transfers. Do not blindly add a nominal 16 ms after the first transfer.
- Learn only from eligible busy intervals; idle gaps, caps, cursor-only activity,
  reconnects and delayed event dispatch must not redefine physical refresh.
- Do not reuse the existing estimator's 4–100 ms plausibility window for all
  output service times; legitimate slow panels can take longer than 100 ms.
- Respect existing `target_fps` render-admission semantics. Early tree preparation
  is allowed; early pixel drawing must still obey the cap. No catch-up bursts.
- Late work uses a monotonic current/revised prediction. Do not accumulate a FIFO
  of missed sample times or repeatedly resample solely to chase an estimate.

Frame readiness is not universally presentation: Wayland frame callbacks are
throttling hints, headless emission is delivery, and Metal command submission is
not visibility. Use precise presentation feedback where available; otherwise
label the observation honestly. Do not require every backend to emulate KMS.
Scroll/inertia timing must follow output progress, not the speculative successor
request. Keep native scene/registry publication distinct from physical display;
this does not promise every published intermediate state reaches scanout.

### Reliable tree handoff

Use a bounded coalescing frame-work lane alongside ordinary tree messages, with
one outstanding request, rather than a blocking presenter send or a droppable
one-shot pulse. Its lifetime follows the current presenter/session.

Return an acknowledgement for **every** consumed request: scene produced, no
visual change, inactive, or retryable failure. Scene publication can carry the
normal reply; no-output paths still need a small reply. A coalesced replacement
must retain the covered request identity. Do not make progress depend on an
unrelated future patch. Preserve first-frame anchor observations until consumed.

The actor services control/frame work fairly alongside its existing bounded
message batches and keeps registry-before-scene ordering. The macOS worker uses
this same adapter once moved off the UI thread. Tests must saturate every lane,
not just exercise an empty queue.

### Presenter loop order

```text
handle ready completion/release/control events
consume a bounded batch of latest scene/input/video demand
submit an already-ready frame if the output and cap permit it
  on successful normal submission: request its successor once
prepare at most one successor if demand and writable storage permit it
repeat while an immediate transition made progress
otherwise wait on backend readiness, tree reply, control, stop or a real deadline
```

Never sleep waiting for a wake that the loop already consumed. Never retry EBUSY
before its deadline. Do not block an output-ready commit on fresh tree work if
there is already a valid prepared frame.

## Backend integration

### DRM GL and raster GPU upload — first proving implementation

- Retain `PrimaryFrames` and GBM framebuffer lifetime management. Issue the
  successor request at successful primary submission, with explicit bootstrap
  handling for the blocking initial modeset/first application frame.
- Carry the prepared scene's animation metadata in `PreparedPrimaryFrame`.
- Commit the existing prepared primary first at flip completion; then prepare
  its successor while that commit is pending. Respect `has_free_buffers()`.
- Preserve implicit synchronization, imported-video cleanup and diagnostic-only
  GPU finish. No per-frame `glFinish`, new GPU thread or offscreen copy. Check that
  successor drawing does not extend the in-flight buffer's effective fence wait
  or delay event dispatch enough to erase the gain; API nonblocking is not proof
  that driver/GPU work is nonblocking.
- Gate EBUSY correctly and bound queue draining. Replace the magic 4 ms
  `FOLLOW_UP_PRIMARY_WINDOW` with actual ready/pending primary state, while
  proving that a slow tree cannot starve hardware-cursor updates.
- Remove `DrmPresentState` if it is just duplicating the shared timing helper.

### DRM Vulkan

- Use the same scheduling decisions over the existing three-slot/fence lifecycle.
- Exclude an already in-flight generation from duplicate preparation. Preserve
  newer video/capture/cursor generations as real independent demand.
- Grant successor work only after successful atomic acceptance, never each EBUSY
  attempt. Keep master fence duplication, missing-flip recovery and quarantine.
- Poll/process ready flips before expensive preparation. Keep real completion
  hooks tied to their submitted slot, not mutable latest scene state.

### CPU DRM

- Change steady-state commits to nonblocking page-flip-event commits; include the
  DRM fd in the wait set and reuse buffers only after the corresponding flip.
- Use three dumb buffers by default for current + in-flight + one ready successor.
  This is one additional pitched framebuffer, not an unbounded queue or extra
  shadow/copy buffer. Account for this memory cost on constrained devices.
- Retain blocking setup/detach barriers where required. Preserve CPU-only builds,
  pitch/capture handling, software cursor and first-frame/backlight sequencing.
- Shared policy must also behave safely when no writable storage is available:
  keep a ready scene and wait, never draw into current/in-flight storage. Do not
  add a public double/triple-buffer or pacing switch.

### Wayland: GL, Vulkan, raster and GPU upload

- Preserve compositor-owned admission. Integrate its existing after-submit tree
  look-ahead with the common helper; do not add a second animation pump.
- Separate scene preparation, optional early pixel preparation and surface commit.
  CPU buffers can be prepared before attach; Vulkan can use a safely acquired
  image; GL preparation must not force an early EGL swap/surface commit.
- Prepare pixels early only when existing acquisition rules permit it without
  blocking event dispatch. Otherwise keep the latest scene ready for the callback.
  Do not add an offscreen blit merely to claim universal pixel overlap.
- Frame callback, buffer release and presentation feedback have different jobs.
  Hidden/occluded surfaces stop issuing work after the single look-ahead.
- Audit bounded late patch/video replacement against the same accounting. Retain
  it only where it represents safe compositor replacement, not an extra animation
  tick. Remove redundant state only with parity tests, not by weakening behavior.
- The known Vulkan acquired-image synchronization issue remains a separate blocker
  for hardware qualification; faster scheduling is not evidence that it is fixed.

### Headless binary and PRIME

- Request the next scene early for the already-known next eligible delivery time,
  rather than waiting until that deadline to begin layout.
- Still hold animation-only drawing until the cadence deadline. Uncapped headless
  currently uses 16 ms animation pacing: removing its late pulse timer must not
  create an unlimited self-triggering render loop.
- Keep static/update-driven output immediate when uncapped, existing live cap
  rebasing, binary output retries and no-animation idleness.
- PRIME pool capacity and lease retirement are hard gates. Exhaustion must not
  create extra samples, output copies, storage or busy retries. Retirement wakes
  the existing pending work; it does not grant another animation credit by itself.
- Headless delivery cannot predict an arbitrary downstream physical panel without
  consumer feedback. Do not advertise display synchronization that its API lacks.

### macOS Metal and raster

- Reuse the same policy, but acknowledge that sharing a helper alone does not
  establish concurrency. Move `TreeUpdateEngine`/scene preparation to the existing
  actor-style worker per session, preserving synchronous host-protocol reply order
  through asynchronous completion rather than blocking AppKit for the worker.
- Keep NSWindow/NSView, IME, clipboard and drawable/layer ownership on their proper
  thread. Port the registry/event feedback contract, including host feedback and
  multi-session/asset isolation, instead of creating a second tree algorithm.
- Drive frame opportunities from a display-link/run-loop source and output resource
  readiness, not `sleep(10 ms)` plus a submission-duration estimator. Use drawable
  presentation feedback for observations where available; callbacks only enqueue
  bounded notifications, never mutate tree state or call BEAM directly.
- Select APIs compatible with the supported host deployment floor. In particular,
  `CAMetalDisplayLink` requires macOS 14; do not silently raise the floor to use it.
  An older supported display-link adapter is preferable to another user setting.
- Preserve raster/Metal drawing initially. Replacing raster's PNG/NSImage transport
  is a separate measured optimization, not required to adopt the scheduling policy.

## Implementation sequence

Each step ships its default behavior, removes its replaced scheduling path and
has tests. No permanent old/new mode switch.

1. **Baseline and failing tests.** Reproduce animation-only serialization with a
   fake slow output; record target tree/draw/commit/flip traces using existing
   counters. Establish first-frame, final-frame, backpressure and retry contracts.
2. **Common contract + DRM GL.** Separate sample prediction from progress/anchors,
   implement bounded reliable frame requests, wire one-ahead scheduling into GL,
   and fix the local generation/retry/loop-order issues above. Validate on the
   watch before expanding abstractions around assumptions.
3. **Other DRM routes.** Apply the same helper to Vulkan; make CPU DRM async with
   bounded storage. Run power, reconnect, capture, cursor and video regressions.
4. **Wayland + headless.** Converge their already-related pacing onto the helper,
   retaining platform readiness, caps, lease/failure semantics. Delete replaced
   pulse send/timer helpers and duplicated bookkeeping.
5. **macOS integration.** Move tree work off AppKit and supply native opportunities
   to the same contract. This is a real runtime change, not a one-line port; keep
   it a separately reviewable increment with macOS hardware validation.
6. **Finish consolidation.** Update frame-pacing, architecture and CPU DRM docs;
   close the pacing portion of the low-resource animation plan with measurements.
   Review net scheduling complexity, not just the existence of a shared module.

Do not fold in renderer-cache redesign, general layout optimization, public frame
caps on currently unsupported backends, a new BEAM scheduler, strict config/frame
proof machinery or the unrelated Vulkan synchronization repair. Synchronous
`render_to_pixels/2` remains an explicit draw, not a background frame loop.

This supersedes the low-resource plan's suggestion to postpone all pacing work
until CPU optimization finishes: the serial wait is independently visible in the
source. Measure CPU cost, but do not require a layout rewrite before removing it.

## Validation and acceptance

### Deterministic tests first

Use supplied timestamps and fake readiness/storage, not sleeps or a real KMS card:

- Slow flip: tree + draw takes 20 ms, output service 65 ms. After bootstrap the
  successor is prepared during that wait; no additional 20 ms tree/draw gap is
  inserted after every completion. Exactly one look-ahead, not a growing queue.
- Repeat with 8/16/33/65/150 ms service times, jitter, late events, slow drawing,
  unavailable buffers, caps and long idle gaps. No prediction-based fake flips.
- Burst patches, input and video select latest undrawn content, preserve prepared
  work, do not starve presentation/Stop, and leave the final state visible.
- Duplicate wakes/completions, cursor-only commits, unchanged scenes and retries
  do not mint extra samples. A lost/no-output tree reply cannot stall animation.
- First enter, new enter during an older flip, exit terminal/prune, interruption,
  finite/looping length animation and transformed hit testing retain their rules.
- Busy/unsubmitted/failed draw, lost flip, resize, off/on, reconnect, output lease
  exhaustion and stop with work outstanding preserve buffer/fence/session safety.
- Run the same policy suite for actor and host adapters; retain existing tree
  backpressure, cadence, headless recovery, Wayland and scanout lifecycle tests.

Run `./ci-tests.sh` (includes `mix test` and `cargo test`) plus Rust feature profiles
`--no-default-features --features drm-raster`, `--features drm`, and combined
`--no-default-features --features wayland-all,drm-all,headless-all`; run macOS CI on
macOS. Preserve GPU-free dependency/linkage checks for CPU DRM and warning-free
feature builds. Software results are recorded above; they do not substitute for
hardware measurements or macOS validation.

### Hardware qualification

With explicit device consent: slow watch DRM first, an ordinary DRM display,
Wayland GL/raster/Vulkan, headless binary/PRIME with held leases, macOS Metal/raster.
Do not acquire or modeset the machine's desktop card by default.

Measure separately:

- input/patch → tree → queued scene → draw/flush → accepted submission → output
  progress; kernel flip time versus dispatch delay where available;
- physical mode refresh, achieved unique-frame rate, sample-time error, time from
  output-ready to next submission, and p50/p95/p99 latency;
- constructed/selected/prepared/submitted/presented frames, overwrites, retries,
  occupied buffers, CPU/GPU time and memory, including static-scene idle work.

Success is a bounded pipeline that removes avoidable serial waiting without
worsening latency, oversampling, stale final pixels, resource safety or idle power.
Under sustained demand with adequate render budget, throughput approaches the
output service limit rather than service time plus tree/draw time. This is not a
promise that input-to-light can be shorter than a physical panel transfer.

If a trace shows longer present latency from the one ready frame, quantify the
trade-off before calling it optimal. Do not continually redraw staged buffers as
a speculative freshness fix; use the latest scene at the last safe selection point.

## References and boundaries

- [PR #89 and its reported device result](https://github.com/emerge-elixir/emerge/pull/89)
- [Kernel KMS atomic commit and page-flip rules](https://docs.kernel.org/gpu/drm-kms.html):
  nonblocking commits, EBUSY and event-driven safe old-buffer reuse.
- [Wayland protocol](https://wayland.freedesktop.org/docs/html/apa.html):
  `wl_surface.frame` is a throttling hint; `wl_buffer.release` permits reuse.
- [Wayland presentation feedback](https://wayland.app/protocols/presentation-time)
- [Apple CAMetalDisplayLink](https://developer.apple.com/documentation/quartzcore/cametaldisplaylink)
  and [drawable presentation handlers](https://developer.apple.com/documentation/metal/mtldrawable/addpresentedhandler(_:)).
- [Current frame caps](../guides/internals/frame-pacing.md)
- [Tree/animation publication contract](../guides/internals/layout-refresh-render-flow.md)
- [DRM resource and power lifecycle](../guides/internals/drm-outputs-and-lifecycle.md)
- [Platform orchestration differences](platform-runtime-architecture-differences.md)
- [Low-resource animation work](active-low-resource-animation-smoothness.md)
- [Known Vulkan synchronization blocker](active-vulkan-acquire-synchronization.md)

Software test results are recorded above. Hardware findings remain based on
source/PR/protocol review only; no target benchmarks, firmware deployment or
hardware qualification is claimed.
