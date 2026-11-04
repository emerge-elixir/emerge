# DRM touch wake

Status: implemented and locally validated. Hardware acceptance remains open.
Base: `main` at `e73e249`.
Branch: `feat/drm-touch-wake`.
Worktree: `/workspace/emerge-drm-touch-wake`.

## Goal

Allow an application to choose a single tap, double tap or long press to wake a
DRM display turned off through config. Preserve ordinary off-state input isolation,
consume the wake gesture, and reuse the existing presenter restart and first-frame
backlight sequencing. No application touch subscription should be required.

This wakes a display, not a suspended operating system. The input device and its
kernel driver must continue delivering touch events while the display is off.

## Proposed public API

Add one live field to `Emerge.Config.DRM`:

```elixir
%Emerge.Config{
  output: %Emerge.Config.DRM{
    card: "/dev/dri/card0",
    display: :off,
    wake_on_touch: :double_tap
  },
  renderer: [api: :opengl],
  assets: [otp_app: :my_app]
}
```

- `wake_on_touch: nil` is the default and disables touch wake entirely. Omitting
  the field is equivalent to `nil`: single taps, double taps and long presses
  must not wake the display.
- `:single_tap`: wake on release of one valid tap.
- `:double_tap`: wake on release of the second valid nearby tap.
- `:long_press`: wake once a stationary contact reaches the hold duration, without
  requiring its release.
- Exactly one gesture is selected. Reject booleans, lists and unknown atoms.
- Support explicit DRM OpenGL, Vulkan and raster (CPU or GPU-upload) wherever the
  existing power controls are supported. Keep `renderer.api: :auto` guarded.
- Apply changes while on or off. Changing/disabling the policy cancels an in-flight
  recognition; it must not turn the display on by itself.
- Wake requests only change `output.display` to `:on`. Preserve brightness,
  subscriptions and every other setting. Brightness zero still means zero; a wake
  does not override it or promise that the panel becomes visibly bright.

A wake is a new desired display-on config transition, not a temporary native
brightness override. It must update the viewport's desired config and recovery
snapshot as well as native intent. Otherwise the viewport still skips rendering
while off, and a rerender/recovery could immediately undo the wake.

### Applications that derive display state from their own state

Use the existing `c:Emerge.handle_input/2` callback with the event
`{:display_wake, gesture}`; do not add a wake-specific callback. The default input
handler ignores the event. Most apps need only the wake config field. Apps whose
`config/2` derives `display` from state can subscribe with
`input: [events: [:display_wake]]` and update that state in their input handler:

```elixir
def handle_input({:display_wake, _gesture}, state) do
  {:noreply, %{state | sleeping?: false}}
end

def handle_input(_event, state), do: {:noreply, state}
```

The runtime first records the accepted on intent, dispatches the wake input event,
then uses the usual config/render reconciliation. The next `config/2` receives the
updated previous config. A config callback that explicitly returns off remains
authoritative; do not add a hidden override that wins against later application
config. Document this
case and test it. Wake input requires `:display_wake` in `input.events`; an empty
list or `:touch` alone must not invoke the handler. The subscription can change
live. Recognition and internal intent/recovery synchronization do not depend on
this observer subscription. Filter delivery in the viewport; do not add a native
raw-input mask bit for this control notification. Test subscribed, unsubscribed,
and live replacement behavior. It is not proof of first-frame presentation.

For standalone `EmergeSkia` users, wake is automatic through the same native power
owner. Deliver a control notification to the existing input target, independently
of raw input subscriptions, so callers can update their own stored config:

```elixir
{:emerge_skia_display_wake, {session_id, revision}, :double_tap}
```

This proposed message identifies an accepted native transition. The ordinary
`emerge_skia_config` completion for that revision still reports application or
failure. Route these notifications through the viewport's existing generation-aware
renderer relay. Do not add a raw touch subscription requirement or reinterpret a
pointer click as touch.

## Findings in the current code

- `native/emerge_skia/src/drm_input.rs`: the libinput context, device discovery and
  input thread survive display-off. `poll()` and `rescan_devices()` discard events
  whenever `BackendWake::input_enabled()` is false, apart from device removal.
  `Contacts.active` is repeatedly cleared. Recognition cannot use those cleared
  contacts, and changing only the forwarding path would still miss rescan events.
- `backend/drm/power.rs`: a persistent lease/backlight owner stops and joins the
  presenter on off. `input_gate` is also false during startup, waking and teardown;
  false does **not** mean a completed off state suitable for wake recognition.
  On recreates a presenter and retains the application-frame/presentation gate.
- `events/runtime.rs`: a second gate drops input, clears buffered interactions and
  disables ordinary interaction timers. Wake recognition must not reopen it or
  depend on those timers, hit testing or the retained event registry.
- `runtime/config.rs`: `ConfigControl` serializes revision admission, supersession
  and completion. DRM config uses an independent bounded owner lane and joins
  input/output acknowledgement. Keep this authority for touch-originated on intent.
- `lib/emerge/runtime/viewport.ex`: desired-off skips the render callback and tree
  upload. Desired config, renderer-accepted config, pending/applied revisions and
  lifecycle recovery are separate. A native-only flag flip is insufficient.
- `native/emerge_input/src/lib.rs::Event` and its C shim currently provide device,
  slot and coordinates but no event timestamp. Reading a batch with `Instant::now()`
  would misclassify queued taps/holds. Add source timestamps rather than guessing.
- Every DRM input context currently discovers `/dev/input/event*`; there is no
  physical touchscreen-to-connector assignment. This feature must not invent one.

## Recognition contract

Implement a small pure state machine on the DRM input thread with explicit event
and clock inputs. Recognition is separate from normal touch-to-UI processing.
Suggested initial constants, to be verified on hardware before implementation is
considered complete:

| Limit | Initial value |
|---|---|
| Maximum tap duration | 250 ms |
| Tap/hold movement slop | 12 output-coordinate pixels |
| First release to second down | 300 ms maximum |
| Distance between taps | 24 output-coordinate pixels maximum |
| Long-press duration | 600 ms |

These are documented internal defaults, not additional public knobs in the first
version. Use monotonic libinput timestamps, preserve the C/Rust event layout match,
and add timestamp/shim tests. Map source time to the input poll deadline explicitly;
recognize already-completed queued gestures in source order, not dispatch timing.
Drain queued cancel/move/up events before firing an expired hold deadline.

- Single tap requires down then up within duration/slop limits. A down alone does
  not wake. A double tap requires two valid taps from the same device with no
  overlapping contacts. An expired first tap does not wake a double-tap policy.
- Long press fires once at its deadline while the same contact is still held and
  within slop. It must work without a subsequent move/up event or a busy poll loop.
- Track maximum displacement, not just the final position; dragging out and back
  is not a tap. Reject malformed/non-finite positions and out-of-order timestamps.
- A second concurrent contact cancels the candidate. Require all contacts released
  before rearming; do not turn a two-finger sequence into a later single tap.
- Cancel, device removal/replacement, screen-size change, policy revision, new
  off session, stop or owner failure resets recognition. Reused slots/device
  addresses must not inherit the earlier candidate.
- Contacts already down when off begins cannot wake it. Drain them until all are
  released/cancelled, then require a fresh down in the armed off state.
- Consume the complete wake touch sequence, including the second tap and the
  release/motion after a recognized long press. Also drain contacts begun during
  the wake transition. They must not become clicks, virtual keys, text selection,
  drag/scroll, raw touch callbacks or a partially replayed contact after presentation.
- Mouse buttons, pointer motion, keyboard, wheel and touchpad-generated pointer
  events do not wake the display. Their normal off-state suppression is unchanged.

## Ownership and ordering

### 1. Arm only a completed, healthy off state

Publish a small per-renderer wake-control snapshot from the power owner to its
input thread: enabled policy, session identity and the applicable off revision.
Arm after suppression, presenter retirement/join and KMS detach have completed,
including initial-off startup. Never infer arming solely from `input_gate == false`.

Reset/disarm on on intent, config/policy changes, stop, quarantine and renderer
replacement. Discard touch events that predate the armed interval, even if read
later from libinput. Preserve an all-contacts-released boundary across these state
changes. Any private recognition generation supplements config tokens only for
local cancellation; it is not a new frame/presentation protocol.

### 2. Submit one bounded, revision-checked wake request

Recognition sends at most one request per armed interval to the existing power
owner, containing the observed session/off revision and gesture. Use a bounded,
coalescing control slot/wake signal, not an unbounded input/message queue. Keep
input dispatch responsive under event-actor/tree backpressure. Cover both `poll()`
and `rescan_devices()` event draining with the same off-input handling.

The power owner checks that this is still the current healthy off state and policy.
Stop, a newer config, a different renderer or a disabled policy wins over a stale
request. Retiring an old presenter must finish before a new one can start.

### 3. Reuse native config admission, not a second power policy

Extend `ConfigControl` only as needed to retain the latest accepted live snapshot
and admit an internal display-on mutation against an expected current revision.
Keep revision check, snapshot merge, bounded enqueue and revision publication in
the same short admission critical section. Queue failure must not consume a
revision or change desired intent. Never hold that lock across sysfs, GPU/KMS waits
or presenter joins.

The accepted wake receives an ordinary new revision; merge only `display_on: true`
into the latest live config. A concurrent brightness/input change cannot be reverted
by a stale full snapshot. Preserve existing joined acknowledgements and error,
supersession and retry behavior. Do not directly change the input gate, restore old
GPU resources, illuminate a panel, or fake an applied revision from the recognizer.

Send the accepted-wake notification before its completion, on the same ordered
control delivery path. Target the current session's input/control recipient rather
than a captured stale PID. The event actor must be able to relay this one control
message while ordinary UI input remains gated; raw subscription masks do not filter
it. Retain a bounded pending notification if that actor's queue is temporarily full.
No new public NIF, scheduler strategy, native thread or external input daemon is
needed; any new native-to-BEAM sends happen only on existing owner threads.

### 4. Reconcile the viewport and render current state

Handle accepted wakes in `Emerge.Runtime.Viewport`, before generic user messages.
Check the renderer generation/session and revision against current accepted intent;
a delayed wake must not overwrite a newer explicit off, policy change or replacement
session. Initial-off uses the existing startup/session identity, not a made-up live
revision. Duplicate notification must be idempotent.

Update the normalized desired config, private native options, lifecycle recovery
snapshot and renderer-accepted/pending state together. Dispatch
`{:display_wake, gesture}` through the existing input handler only when subscribed,
then schedule one normal reconciliation/render so an initially-off viewport can supply its first
tree and a sleeping viewport uploads current app
state. Do not route wake through element events or require a user rerender first.
Test completion arriving near notification/reconciliation and normal callback errors.
Reuse existing retained-tree and subsequent viewport-upload semantics; this feature
must not introduce a stronger config-to-frame provenance protocol.

The presenter still requests a matching application frame. Only successful
presentation enables managed illumination and normal UI input; DrmInput additionally
swallows the held wake contacts until their lifetime ends. On failure retain desired
intent and use existing recovery/quarantine rules, never automatically clear them.

## Scope and limitations

- This is DRM-only. No Wayland/macOS/headless wake support, OS suspend/resume,
  idle timeout, touchscreen driver power management or global power-button policy.
- Use the renderer's existing touch sources. A source shared by several opted-in
  DRM viewports can wake each of them; a viewport with the policy disabled stays
  off. Document this limitation rather than claiming connector-local touch routing.
  Device-to-output assignment is a separate feature if required.
- A touchscreen that stops producing events when its panel is unprepared cannot
  be awakened by this software recognizer. Keep another wake source available and
  qualify each target's driver behavior. Do not change permissions or deployment.
- Keep input-mask/category changes separate. Wake is a control path, not permission
  to forward raw touches while the screen is off.

## Implementation sequence

1. [x] Add `wake_on_touch` schema/types, normalization and live-change validation;
   wire startup and complete native live snapshots. Define the wake input/control
   notification contract and update the adapter and standalone API docs.
2. [x] Add libinput source timestamps and a pure tested recognizer. Integrate it
   with the input thread's poll deadlines and all dispatch/rescan paths, including
   contact draining across off/on and device changes.
3. [x] Add owner-published off arming and bounded wake admission through the existing
   revision/config lane. Test races and notification/completion ordering without KMS.
4. [x] Reconcile accepted wakes into viewport desired/recovery state, dispatch the
   wake through `handle_input/2`, upload current UI and preserve input gating.
5. [x] Add cross-layer regressions, public examples and an opt-in interactive
   hardware test. Revise docs that say touch cannot wake a blanked display.
6. [ ] Run authorized hardware acceptance; the interactive test is excluded from
   ordinary CI and has not been run.

### Implementation notes

Accepted-wake notifications use the existing `InputTargetRelay` directly, rather
than passing through the gated/bounded event actor. The power owner sends the
notification before it can consume its internally queued config request; that
request's completion uses the same recipient relay. This avoids a second event
queue and leaves ordinary input filtering unchanged. Standalone callers register
an input target to enable recognition and receive notifications. Initial-off
standalone callers must still provide a first application tree.

Wake requests skip changing the raw input mask, preserving startup/direct
subscriptions as well as accepted live settings. Live mask validation now includes
the already-public touch category (`0x100`); unknown bits remain rejected.

## Validation

Deterministic tests before hardware:

- Pure recognizer: all three gestures; exact threshold boundaries; excessive
  duration/motion; moved-out-and-back; taps too far apart; double-tap timeout;
  multitouch; cancellation/removal/reuse; invalid coordinates/timestamps; delayed
  batches; hold deadlines without further events; reset/rearm/all-up behavior.
- Input integration: no wake when on, starting, retiring, quarantined or disabled;
  no ordinary off-state input; no tap-through after long press or a delayed first
  frame; stable recognition through rescan/device churn; mouse/keyboard do not wake.
- Power/config: initial-off and runtime-off, wake while an old child retires,
  simultaneous off/brightness/policy submissions, queue rejection, stale/duplicate
  requests, exactly-once notification, joined completion and startup/recovery.
- Config defaults: an omitted field normalizes to `nil`; explicit `nil` disables
  all three gestures, emits no wake notification and cancels pending recognition.
- Viewport/fake adapter: config-only wake without callbacks when unsubscribed;
  explicit `:display_wake` subscription, `:touch` alone, live replacement; initial-off first
  tree; state-driven config with/without handling wake input; input/validation failure;
  new off winning over a late wake; obsolete renderer messages; current app state
  on wake; ordinary config completion; recovery retains the new desired on intent.
- Standalone API: automatic wake plus notification; same power validation and
  revision behavior, no implicit public callback requirement.
- Disabled sibling does not change output/brightness. Shared enabled touch sources
  follow the documented routing limitation, with independent renderer tokens.

Run `cargo test` (including `native/emerge_input`) and `mix test`; prefer
`EMERGE_SKIA_BUILD=true ./ci-tests.sh all` for the full local gates. Also run native
tests/strict Clippy and source-built Mix tests for CPU DRM, DRM/OpenGL, DRM/Vulkan
and combined DRM APIs. Update any wrappers for new NifMap keys: omitted optional
settings must retain the default rather than cause `badarg`. Run `mix docs` and
check release-facing links; internal plans/guides are not release documentation.

Only with user authorization, test an explicit panel/touchscreen for each gesture:
initial-off, ordinary off/on, hold through first presentation, repeated cycles,
policy changes, hotplug, no-upload/slow-upload, brightness zero, idle power and
sibling behavior. Observe the screen and underlying UI actions to verify no
wake-through activation and no illumination before an application frame. Record
hardware, driver, API and event delivery while off. Do not use physical input,
change device permissions or deploy firmware during ordinary tests.

## Local validation results

- `EMERGE_SKIA_BUILD=true ./ci-tests.sh all`: passed, including formatting,
  warnings-as-errors compilation, strict Credo/Clippy, 720 ExUnit tests/doctests
  with full-sweep coverage, 1639 default native unit tests plus integration tests,
  and Dialyzer with zero errors.
- Native release tests and strict all-target Clippy passed for DRM/OpenGL (1671
  unit tests), CPU DRM (1635), DRM/Vulkan (1695), and combined DRM APIs (1730),
  plus the integration suites. Hardware-only tests remained ignored.
- `cargo test --manifest-path native/emerge_input/Cargo.toml`: 4 unit tests and
  13 source/generator tests passed; strict all-target input Clippy passed too.
- Source-built Mix suites passed for DRM/OpenGL, CPU DRM (one
  skip) and DRM/Vulkan. Temporary external configs and isolated Mix build paths
  were used; no project config or renderer feature defaults changed. The default
  profile was restored and passed again.
- A separate hardware-free NIF contract test decoded every wake gesture and nil
  through native capability validation, without admitting a revision on a
  non-DRM renderer.
- `mix docs` passed. Formatting and diff checks passed.
- The interactive `drm_touch_wake_hardware_test.exs` was added but not run. No DRM
  master acquisition, physical touch injection, panel/backlight operation, firmware
  deployment or native-artifact publication was performed for this task.

## Completion criteria

A config-only opt-in wakes a supported DRM output using the chosen touch gesture,
updates both native and viewport intent, renders current application state, preserves
first-frame/backlight safety, and never forwards the wake contact to the UI.
Disabled behavior remains unchanged. Software test coverage is not physical-device
qualification; record target acceptance separately before claiming it.
