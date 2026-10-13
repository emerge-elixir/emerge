# Declarative viewport config and DRM display power

Status: **implementation in progress**, authorized by the user. This branch is not release-ready.
Target release: **0.5.0**, intentionally breaking. No backward-compatibility layer for the replaced config/input APIs.
Branch: `feat/declarative-viewport-settings`, originally from `feat/drm-raster-without-gpu` at `b8a03a1`, rebased onto `main` at `6b81166`.

## Implementation progress

Implemented:

- Typed `%Emerge.Config{}` groups/output variants, defaults, named enum types, typed keyword shorthand and canonical normalized types.
- Optional `config(previous)` / `config(previous, state)` callbacks; initial evaluation, replacement semantics, config-only updates and lifecycle replay.
- Desired intent is separate from the last renderer-accepted snapshot. Operational configure errors retain desired config for callbacks/recovery; subsequent rerenders retry complete live snapshots, including reversals after partial failure. Startup snapshots travel with session handshakes so changes made during restart are replayed before upload; delayed ready messages cannot overwrite an adopted session. Native capability/validation failures still reject intent. Queue acceptance is not hardware application. Restart intent updates do not wait behind slow native startup, and ordinary config traffic no longer postpones lifecycle health checks.
- Opt-in atom-list input subscriptions and typed callback payloads; removed public numeric mask helpers and legacy keyword/positional startup APIs.
- Native live-control revisions/completions for the currently supported Linux/native controls: one full-snapshot NIF, bounded nonblocking event admission, current-revision checks before owner effects, session/revision-tagged completion, and `EmergeSkia.config_status/1`. Wayland coalesces controls in a one-slot owner mailbox; headless publishes its existing atomic control. Viewports track pending/confirmed tokens, ignore stale replies and retain desired intent on asynchronous failure. See `guides/internals/config-reconciliation.md`; this is not KMS/presentation completion or startup revision tracking.
- Live window titles on the Wayland event-loop thread and macOS AppKit thread. macOS protocol 17 acknowledges application and rejects stale host/session handles; socket/protocol tests cover errors and host loss.
- All DRM presenters acknowledge resource readiness before first presentation. Explicit successful-upload readiness survives scene coalescing; resize, capture and cursor wakes cannot activate a placeholder. GL/GPU-upload raster also request a layout-scoped replay barrier on session recreation, rejecting queued stale scenes. Vulkan sleeps instead of spinning before upload. A valid empty tree counts as application content.
- Headless binary/PRIME startup and live frame caps now gate all render admissions, not just animation pulses. Bounded latest-scene coalescing preserves earliest pending pipeline latency; an independent control lane rebases deadlines without catch-up frames. Static config changes stay idle, and stop/PRIME retirement remain wakeable. Tests cover cap changes, burst coalescing, terminal-frame retry, native output and config-only recovery. General Linux GPU/PRIME/reconnect qualification was closed upstream; the specific Vulkan acquire synchronization failure remains separately deferred.
- Wayland startup/live caps across CPU raster, raster GPU upload, GL and Vulkan. A coalescing calloop ping wakes updates; deadlines only arm for presentable work. Input, callback/cleanup handling and stop remain independent. Physical refresh estimation ignores capped callback intervals, and animation sampling accounts for the next admission. Private startup FPS is now shared, not headless-specific. See `guides/internals/frame-pacing.md`.
- Migrated affected test fixtures and artifact smoke probe; added normalization, callback, config-only native recovery and upload-readiness tests.
- Backlight foundations in `native/emerge_skia/src/backend/backlight/`: conservative connector association, alias-safe process-wide reservations, pinned directory/attribute descriptors, requested-brightness baselines, quantization, dark-state intent retention and failure-safe leave-dark cleanup. Private-filesystem/fault/concurrency tests pass. These are wired to the retained DRM owner for explicit OpenGL and CPU raster power/brightness.

Still required (not merely unverified):

- macOS host config revisions; safe DRM mode/output reconfiguration.
- Remaining backend controls and hardware qualification; strict config-to-frame proof is no longer part of DRM power scope.
- macOS/DRM live frame caps.
- Remaining guides/demo migration, full input transport contract coverage, release version changes and hardware qualification.

DRM power/brightness now supports explicit OpenGL and CPU raster. Vulkan/gpu-upload
power, live connector/mode changes and DRM/macOS frame caps remain unsupported.
The package version remains 0.4.2 until remaining 0.5 release work is complete.

DRM power uses a simple stop/recreate lifecycle, as requested by the user: off
ignores native UI input and pending presenter work, suppresses illumination,
detaches KMS and destroys GPU resources. The tree actor retains its latest tree.
On creates a new presenter and presents that tree before illuminating. The
viewport can send fresh UI; no GPU state is restored. There are no tree markers,
frame receipts, proof mailboxes, power input epochs or custom commit cookies.

Binding/baseline ownership, independent control admission and initial-off
acquisition are implemented. See `active-native-drm-power.md` for the current
contract and software checks. FP3 has returned to explicit OpenGL. The user
reported CPU raster working; OpenGL device qualification remains their next step.
Vulkan acquisition synchronization repair remains separately deferred in
`active-vulkan-acquire-synchronization.md`.

The remaining design below describes broader release work, not shipped coverage.

## Goals

- Acquiring DRM master / an output lease must not illuminate a startup placeholder.
- Prepare the first application frame, activate KMS with that framebuffer, then enable the managed backlight after presentation completes.
- Add an optional config callback receiving previous config: proposed `config(previous)` or `config(previous, state)`. Both mount and config use `%Emerge.Config{}` with defaults throughout and nested keyword shorthand for unambiguous records. Prefer ordinary struct syntax/updates, not constructor helpers.
- Reconcile window title, frame-rate limit, DRM output/mode, screen power and brightness from application state. No imperative viewport `screen_on/1`, `screen_off/1`, etc.
- Replace the public numeric input mask with a documented, typed atom-list subscription API. Document exactly which raw events reach `handle_input/2`; all closed atom options in the new config model need named public types.
- Preserve callback PID/state, independent DRM outputs, renderer recovery and native ownership guarantees.
- Support battery-powered devices by avoiding unnecessary UI work while the display is off. Emerge only reconciles `display: :on | :off`; the cause of that declaration, including buttons and system power policy, is outside Emerge.

## Baseline findings (before this branch)

| Area | Behavior at branch start / relevant files |
|---|---|
| Public callbacks | `lib/emerge.ex` defines `mount/1` and mutually exclusive `render/0` / `render/1`; no config callback. `rerender/1` coalesces updates. |
| Configuration | `lib/emerge/runtime/viewport/config.ex` merges top-level renderer options with `emerge_skia` (nested wins); `viewport` holds runtime/adapter options. `lib/emerge_skia/options.ex` normalizes native startup options. |
| Viewport lifecycle | `viewport.ex` renders a tree, acquires a renderer, then uploads. `viewport/lifecycle.ex` retains mount configuration for recovery; it will need the latest desired settings instead. |
| Native startup | `native/emerge_skia/src/lib.rs` waits for DRM startup acknowledgement **before starting the tree actor and returning the renderer handle**. Waiting for an uploaded application frame here would deadlock. |
| DRM OpenGL / GPU-upload raster | `backend/drm/gl.rs` draws `RenderState::default()`, activates KMS with it, then acknowledges startup. |
| DRM CPU raster | `backend/drm/raster.rs` starts dirty with default render state; first blocking presentation acknowledges startup. |
| DRM Vulkan | `backend/drm/vulkan.rs` starts with default render state and an initial draw pending; startup acknowledgement follows the first page flip. |
| Ownership | `backend/drm/session.rs` shares a card master and allocates per-output leases. Power control must be output-local, not a card-wide master operation. |
| Updates / transports | Viewport renderer adapters and `lib/emerge_skia/transport.ex` have no general settings-update contract. macOS uses its own host protocol and main-thread window ownership. |
| Existing options | `drm_output` and exact `drm_mode` already exist at startup. `target_fps` currently belongs to `headless`. No backlight control was found in DRM presenters. |

Build on the actual lifecycle implementation; historical proposals are not evidence that every proposed mechanism landed. See the current [DRM output and lifecycle guide](../guides/internals/drm-outputs-and-lifecycle.md).

## Proposed callback contract

The callback is named `config`, not `settings`. See the [current option inventory and proposed schema](viewport-settings-schema.md).

```elixir
alias Emerge.Config

@impl Viewport
def mount(_opts) do
  config = %Config{
    output: %Config.DRM{connector: "DSI-1", backlight: [brightness: 0.7]},
    renderer: [api: :opengl]
  }

  {:ok, %{display: :on, brightness: 0.7}, config}
end

@impl Viewport
def config(previous, state) do
  backlight = %Config.Backlight{previous.output.backlight | brightness: state.brightness}
  output = %Config.DRM{previous.output | display: state.display, backlight: backlight}

  %Config{previous | output: output}
end
```

The callback name `config`, receiving previous config, defaulted structs and `display: :on | :off` are agreed requirements. Proposed arities/order are `config(previous)` or `config(previous, state)`. Prefer nested keyword shorthand in struct literals over a separate constructor/update API. The callback/schema APIs are implemented; native control coverage is listed above. How application state changes is outside this contract.

- Implement neither config callback (mount-only behavior), or exactly one of `config/1` and `config/2`. Both receive previous config; the second arity additionally receives application state. Return `%Emerge.Config{}`, not a keyword list or callback-result tuple.
- Evaluate `config` first in the viewport callback process during the existing coalesced rerender cycle. When the display is on, evaluate render against that same state snapshot; while off, defer routine rendering until the display is requested on. Do not run application callbacks in native/lifecycle workers.
- Each callback receives the **previous validated, normalized desired config** and returns a complete root config, optionally containing nested keyword shorthand. First invocation receives normalized mount config with defaults; later invocations receive the latest valid normalized result, even if native application is pending. Nested records in the argument are always structs. Returning previous unchanged is a no-op; callback/validation failure retains it. Recovery must not reset it to mount defaults.
- Every struct has documented defaults. Root and ambiguous variant choices use explicit structs; fixed-shape nested records accept typed keyword shorthand. Normalize shorthand at the runtime boundary, filling omitted fields from that record's defaults. Prefer explicitly typed struct updates (`%Config{previous | ...}` and the corresponding nested struct types) to preserve untouched values. Use this syntax consistently in public docs and examples, rather than leading with `put_in`, untyped map updates or helper APIs. Build updates one config struct at a time using named intermediate variables (backlight, then output, then root); do not nest them into one deep struct-update expression. Returned fresh records replace rather than merge with previous records; collections replace, not append. No public constructor/update API or legacy flat config is needed.
- Evaluate `config` on the initial render, before renderer creation, as well as rerenders/reloads. Mount alone remains sufficient. This prevents creating an initially-on session when current application state requests off.
- Normalize/validate one canonical struct config and diff by responsibility. Remove old renderer keyword config, duplicate `emerge_skia`/`viewport` namespaces and deprecated config aliases rather than converting them. Application-defined mount arguments and GenServer process-start options are separate from renderer config and may remain keyword lists.
- Settings-only changes must work when the tree diff is empty. Unchanged normalized settings cause no native work.
- Validate a settings revision before applying it. Callback/validation failures retain the last valid settings and report the error. A valid off/dim request must not depend on successful UI rendering; an on request recreates the presenter and presents the latest tree.

### Config structure

Use six typed groups: `output`, `renderer`, `input`, `assets`, `diagnostics`, `runtime`. Output is exactly one of Window, DRM or Headless, so unrelated backend fields cannot coexist. Native ownership remains internal, outside declarative config. See the [schema proposal and old-to-new migration reference](viewport-settings-schema.md).

| Proposed field | Scope / rule |
|---|---|
| `output.title` (Window) | Live on Wayland/macOS; no session restart. |
| `renderer.target_fps` | Live cap, distinct from physical refresh; `nil` restores backend pacing. |
| `output.connector`, `output.mode` (DRM) | Live reconfiguration on the selected card; exact advertised selections never silently fall back. |
| `output.display` (DRM) | `:on \| :off`, default `:on`. Off disables scanout/backlight and unnecessary UI work, not config reconciliation. On requires a valid frame. |
| `output.backlight.brightness` (DRM) | Live normalized `0..1`; zero is not display-off. `nil` resolves to the initial requested brightness captured before darkening and retained through recovery. |
| `output.backlight.device` (DRM) | Startup binding: `:auto \| nil \| path`. Auto requires unambiguous association; nil opts out. Explicit unsupported requests report errors. |
| `input.events`, `runtime.health_check_interval_ms` | Runtime-owned updates. `events` is a list of documented input-category atoms, converted to an internal mask; it filters raw `handle_input/2` delivery, not element interactions. |
| Output type, renderer API, card/GPU node, assets, adapter identity, other unclassified fields | Startup-only initially; changes report unsupported rather than silently restarting or being ignored. |

Struct organization does not make every field mutable. Classify all fields and validate capabilities explicitly. Update the custom-adapter contract to the new API directly; no legacy adapter shim. Unsupported live changes must be rejected honestly.

## Reconciliation and ownership

- Keep initial and latest valid desired configuration in viewport state; synchronize the desired restart configuration with the lifecycle owner. Recovery must not resurrect old title/output/brightness/power values.
- Keep desired, pending and applied settings distinct. Native queue acceptance is not KMS completion. Report revision-tagged success/failure through existing session-aware routing; expose enough status to diagnose requested versus applied power/output.
- Use one internal settings reconciliation entry point through the renderer adapter/transport, rather than one NIF per property. Normal-scheduler NIF work only validates/enqueues bounded work; KMS/sysfs/GPU work stays on owning native threads. Blocking waits, if needed, use the appropriate dirty scheduler.
- On creates a fresh presenter and requests the latest retained tree. Do not add config-tree receipts or no-op diff acknowledgement machinery.
- Backend control remains independently wakeable and coalesces desired updates. Off stops the current presenter; no old presenter state is reused on wake.
- Reuse current lifecycle generations and cleanup gates; do not invent a competing recovery owner. A quarantined session is not reusable merely because the desired display changed.

## DRM startup and power sequence

### Split readiness from visibility

1. Acquire/reserve only the selected output; discover mode, allocate presenter resources and establish backlight control without enabling a placeholder.
2. Publish **ready to accept a tree** once initialization succeeds, without requiring a modeset/page flip. Start the tree actor and return the renderer handle. A renderer can be healthy while dark or waiting for its first tree.
3. Upload/layout the application tree at the selected output dimensions and render a completed scanout frame. Track explicit successful tree initialization: neither a resize-generated empty scene, a cursor wake nor `render_version > 0` alone proves application content exists. An intentionally empty uploaded tree is valid; waiting for every asynchronous asset is not required.
4. If current desired power is on, submit an atomic enable containing mode/routing, primary framebuffer and geometry together. Preserve each presenter's GPU completion/fence requirements.
5. After matching presentation completion (blocking completion or the applicable page-flip/out-fence), enable the managed backlight at the latest requested brightness. Ioctl submission alone is insufficient. Keep cursors hidden until the primary frame is valid.

Keep startup readiness separate from first presentation; do not add a proof state machine.

### On, off and reconfiguration

- **Off:** suppress managed illumination promptly, quiesce/resolve in-flight work, disable the selected CRTC/planes safely. Retain the lease, viewport state and desired brightness. Do not rely on receiving ordinary page flips from an inactive CRTC.
- **While off:** accept application state/settings updates and defer routine tree building/layout/rendering. Display state changes only through config; input does not implicitly turn it on. Avoid continuous drawing/presentation, cursor-only commits and false liveness timeouts. Retire video frames safely; explicit capture, if supported while off, is bounded on-demand work and must not turn on the display.
- **On:** create a fresh presenter and render the latest tree, then illuminate after first presentation. Do not restore GPU resources from before off.
- **Brightness:** changing it while off only updates desired state; it cannot wake the display. Power off preserves requested brightness. Explicit brightness zero remains zero on wake; it is not equivalent to KMS power off.
- **Mode/output:** validate availability and reserve target resources before destructive changes where possible. Quiesce the old output, rebuild size-dependent buffers/layout/input geometry, and gate target activation on a correctly sized frame. A dark transition is acceptable; placeholder flashes are not. Preserve the old working output on preflight failure; report failures after teardown honestly rather than promise unconditional rollback.
- **Recovery/hotplug:** reconcile the latest desired selection/power, reset frame readiness and reject stale completions. Never migrate an explicit output to another viewport's output. Reuse existing safe cleanup/quarantine rules.
- **Stop/error:** best-effort darkening must not bypass GPU/KMS ownership safety or unblank an output with invalid scanout. Define clean-stop backlight policy before coding; do not restore illumination blindly in destructors.

### Backlight limits and device selection

- DRM master/leases do not themselves control or exclusively own sysfs backlights. Bind by explicit device or proven unambiguous connector association; never choose the first `/sys/class/backlight` entry. Reserve a managed backlight identity inside Emerge to avoid sibling viewports controlling the same device. External managers can still conflict.
- Proposed default: `:auto` manages an unambiguously associated device; absence means KMS-only operation. An explicit unavailable/unwritable device is an error. Ambiguous discovery needs explicit configuration, not a guess. Output changes must re-resolve/reserve the associated device and safely retire the old binding; incompatible explicit bindings fail preflight.
- Linux exposes requested `brightness` in `0..max_brightness`, optional hardware readback via `actual_brightness`, and `bl_power` values `0` (on) / `4` (off). Validate finite normalized inputs, quantize deliberately and test boundaries. Do not confuse requested brightness with physical light output.
- Keep managed illumination suppressed across the enable commit, including drivers that unblank panels during modesetting. Verify whether zero brightness, `bl_power`, or both are required on supported devices; do not assume a post-commit userspace write can undo an earlier flash.
- Acquiring master does not guarantee the inherited display is already off. Decide whether takeover deliberately blanks the selected output/backlight or preserves an existing boot image until replacement; never blank every output on the card.
- KMS inactive means scanout/pipeline off, not guaranteed physical monitor mains power. HDMI/DP monitor sleep and panel/backlight timing are hardware-dependent. DDC/CI, external GPIO power rails and brightness fades are outside the initial scope.

## Display off and reduced resource use

Battery savings are a consequence of the display being off, not a second application sleep/power state. Emerge consumes `display: :on | :off` without knowing why it changed. Buttons, GPIO, wake sources and system power policy belong to other parts of the system; this feature adds no handling or configuration for them.

- **Off:** darken the managed display and safely quiesce outstanding GPU/KMS work. Defer routine render callbacks, tree diff/serialization/layout work, animation pulses, cursor/hover refreshes and video drawing. Already-started work must complete or cancel safely; no thread/process suspension.
- **Settings remain responsive:** retain callback PID/state and normal application message handling. Reconciliation must not require an active CRTC, rendered scene or presentation event. A newer off declaration must cancel illumination from a pending enable. No new input policy or special event turns the display on.
- **Updates while off:** evaluate settings when requested and retain latest application state without building intermediate trees. Coalesce deferred native work; do not queue missed frames/animation ticks. Consume/release video frames safely without an unbounded backlog.
- **On:** render current state, refresh layout/event state, prepare a current frame, then activate and illuminate at the retained desired brightness. Render failure leaves the output dark. Initially-off sessions and recovery must also remain dark and quiescent until config requests on.
- **Incidental wakeups:** audit heartbeat/liveness checks, hotplug polling, stats logging, asset/loading-indicator timers and fence/video cleanup. Prefer notifications or display-state-aware deadlines; preserve necessary ownership/failure detection without treating intentional inactivity as a stall. One off display must not pause another viewport.
- **Scope:** display off does not suspend the BEAM, OS or device, or automatically pause application timers, camera/decoder producers or networking. Presenter GPU resources are destroyed while off; measure rather than promise complete device power-off.
- **Acceptance:** test extended display-off periods with an animating/video scene and application state changes, then declare `display: :on` through config. Verify no recurring render/layout/presentation work, bounded frames, responsive reconciliation and first-frame-before-light ordering. Measure CPU use, wakeups, off-to-on latency and battery/current draw against active and static-screen baselines on the target device. No physical button is required for Emerge's acceptance tests. Record unavoidable periodic work rather than claiming zero CPU use.

## Windowed backends and cadence

- Apply title updates on the Wayland event-loop thread and macOS/AppKit host thread, including protocol/transport support. DRM power controls must not affect desktop-wide monitor power.
- Define `target_fps` as an upper bound; `nil` restores backend pacing. Cap animation, video and other redraw sources consistently without fabricating physical refresh changes or spinning on timers. Static scenes remain idle. DRM mode selection controls refresh separately.
- Keep input responsive independently of the render cap. While off, stop presentation-driven pulses; on resume sample animations according to existing time semantics instead of replaying a backlog.

## Delivery slices and acceptance gates

1. **Settle 0.5.0 contract/schema:** approve typed groups/output variants, full-config callback semantics, input payloads and defaults without compatibility constraints; classify mutable/startup-only fields and document errors/capabilities.
2. **First-frame startup:** basic application-upload gating and layout-scoped tree replay plus split native startup handshake; integrate CPU raster, GL/GPU-upload raster and Vulkan. Test that startup returns without a tree and performs no placeholder activation.
3. **Declarative plumbing and typed input:** callbacks, normalization, diffing, adapter/transport updates, revision/completion reporting and lifecycle replay. Add `input.events`, public category/payload types and exact callback documentation; remove the public numeric-mask API. Numeric encoding may remain internal. Establish title/config-only and raw-input filtering tests before hardware effects.
4. **DRM display/backlight and low-power off state:** safe device binding, per-output control, first-frame illumination, off/on races, deferred UI work, responsive settings reconciliation and incidental timer audit across all DRM presenters.
5. **Cadence and output/mode changes:** live pacing, resource-safe reconfiguration and layout/frame barriers; preserve independent outputs and latest settings through recovery.
6. **0.5.0 release/docs and qualification:** bump package/artifact versions during implementation; update tests, examples, viewport tutorial/API, low-level renderer reference and internals. Add a concise 0.4-to-0.5 migration guide and changelog, not runtime compatibility shims. Coordinate standalone demo migration. Report hardware limits explicitly.

Tests required as implementation lands:

- **ExUnit:** absent/1-/2-arity callbacks; previous-config/state argument order; mount config on first call and latest desired config thereafter (including pending application/off/recovery); invalid results retain previous config; conflicting arities/malformed returns; struct defaults, shorthand/explicit-struct equivalence, idempotent normalization, normalized callback arguments, minimal construction, standard nested update preservation, explicit record/collection replacement and no mutation; rejection of removed config forms; initial evaluation; nil/static-field rules; coalescing/no-op config; the new custom-adapter contract; recovery preserves PID/state.
- **Pure Rust/fakes:** basic startup/tree replay, control admission/status and brightness/binding contracts. Keep tests hardware-free; do not rebuild the deleted power proof/fault model.
- **Input contract:** test every atom category, empty/all/default/duplicate/invalid lists, order-insensitive comparison, live replacement and recovery. Validate actual `handle_input/2` tuples against public types/docs on native/macOS; raw filtering must not disable element events or text/scroll behavior. Document backend availability and verify all closed config enums have named types.
- **Native integration:** start without upload, upload later, initially off, capture/cursor/video before first tree, queue pressure, session loss and revision-tagged acknowledgements. Ensure no startup deadlock and no idle/off watchdog failure.
- **Hardware, explicitly opted in:** CPU DRM, GL, raster GPU upload and Vulkan; backlit panel and external monitor; delayed first tree; repeated off/on, brightness zero, rapid toggles, mode/output changes, unplug/reconnect and two independent outputs. Use visual/optical observation as well as commit/write traces; logs alone cannot prove absence of flashes.
- Run `cargo test` and `mix test` after implementation, including DRM-raster-only and full DRM feature coverage. Prefer `./ci-tests.sh` for full local CI. Qualify macOS title/cadence on macOS; mark unavailable hardware gates unverified.

## Decisions to resolve before coding

1. **Callback/shorthand:** confirm `config(previous)` / `config(previous, state)` arities/order and fixed-shape fields accepting typed keyword shorthand. Defaults and normalized previous config are requirements. First callback runs before native startup. Explicitly typed struct updates preserve untouched fields without a separate constructor/update API and are the preferred documentation syntax.
2. **Public structure:** confirm proposed struct/field names and the new typed input payload/default contract. The release is 0.5.0 with no legacy keyword/mask compatibility. Mount and `config` use the same struct schema; output variants separate window, DRM and headless. Display on/off is not application power or suspension.
3. **Power policy:** blank inherited content on takeover versus preserve boot-image handoff; clean-stop leave-off versus explicit restoration policy. Strict no-flash behavior requires controllable, qualified backlight hardware.
4. **Backlight policy:** approve conservative `:auto` discovery with explicit device override and linear normalized brightness (not perceptual brightness).

## Kernel references

- [DRM/KMS state and commit semantics](https://docs.kernel.org/gpu/drm-kms.html): CRTC `active` controls hardware activity; resource reservation and commit completion are separate concerns.
- [Backlight sysfs ABI](https://github.com/torvalds/linux/blob/master/Documentation/ABI/stable/sysfs-class-backlight): brightness range, actual versus requested brightness and backlight power values.
- [Backlight driver model](https://docs.kernel.org/gpu/backlight.html): blanking and display association are driver-dependent.

## Phone-test scope

Explicit OpenGL and CPU raster support DRM power/brightness. FP3 selects OpenGL;
the user already reported CPU raster working. Vulkan/gpu-upload power, connector/
mode changes and DRM caps remain guarded. See `active-native-drm-power.md` and
`active-fairphone-display-controls.md` for the simple lifecycle and test handoff.
Hardware testing is performed by the user; assistant device access is not needed.

## 0.5 documentation migration work

- [x] Audit README, published guides, module docs and runnable documentation examples.
- [x] Replace legacy mount/startup options and numeric input-mask examples with
      typed config records and atom subscriptions; distinguish low-level native
      event payloads from viewport `Emerge.Input` values.
- [x] Update backend support, assets, diagnostics and live/static field guidance
      to current implemented behavior, without claiming pending hardware results.
- [x] Write and register the 0.4-to-0.5 migration guide, mapping all removed options
      and adapter APIs; retain explicitly historical 0.4 examples as history.
- [x] Add documentation regression checks and validate Rust, ExUnit, quality and
      public/internal ExDoc generation. Leave release/version bump and external
      consumer changes outside this documentation task.

### Documentation validation

- New public guides: `guides/migrations/0.5.md`,
  `guides/reference/viewport-configuration.md`, `guides/tutorials/handle_input.md`.
  The schema plan now indexes the implemented API instead of proposing stale fields.
- Hardware-free regressions normalize/adapt all four configuration-reference
  examples, compile the counter/input tutorial modules, parse current guide snippets,
  reject stale startup recommendations and verify docs registration/order.
- `cargo test`: 1,580 unit tests plus 1 + 13 fixtures; `mix test`: 645 checks
  (15 doctests, 630 tests), 13 excluded. Local quality passes (191 Credo files).
- Public and internal ExDoc both build without warnings, each generating 31
  screenshots. Local link checks pass for 67 public and 82 internal-build pages.
  Public guides link internal-only material via canonical repository source URLs.
- Logs: `/tmp/emerge-05-docs-{focused,mix,rust,quality,public,internal}.log`.
  Run Mix compilation/docs sequentially: a first concurrent quality/docs attempt
  raced shared build artifacts; sequential public/internal builds passed.
- No release/version bump, hardware qualification, firmware build, consumer changes
  or deployment. Package version remains 0.4.2 pending release work.

### Documentation commit sequence

Keep the three cross-linked public guides together to avoid broken intermediate
ExDoc links, then separate supporting material and regression coverage:

1. Publish the configuration/input references and 0.5 migration guide, API module
   docs, README/changelog, historical 0.4 notice and ExDoc registration/order checks.
2. Migrate viewport/state/asset tutorials and asset internals; correct the removed
   startup recommendation in the missing-asset error and repair tutorial syntax.
3. Align native build, DRM lifecycle, architecture, input transport, macOS,
   frame-pacing and video interoperability guidance with the implemented API.
4. Add executable documentation regressions and record the completed migration work.

Each staged snapshot passed `cargo test` and `mix test` in an isolated worktree:
1,580 Rust unit tests plus 1 + 13 fixtures throughout; the first three snapshots
have 641 Elixir checks, and the regression-test snapshot has 645. Final quality
passes (191 Credo files); public/internal ExDoc builds are warning-free, generate
31 screenshots each, and pass local link checks for 67/82 pages. Cold dependency
compilation emitted upstream Elixir 1.20 warnings; the warm quality run is clean.

Per-snapshot refs/logs and preservation backups are retained under
`/tmp/emerge-docs-commit-sequence-20261011-182458/`. Unrelated plan/image hashes and
the Fairphone working tree are preserved. Nothing is pushed or version-bumped.

### Config documentation readability follow-up

- [x] Give every public config record a short purpose, placement example, field
      meanings/defaults/units and a clearly separated update/backend boundary.
- [x] Replace the migration's monolithic option table with focused before/after
      examples and small topic-specific rename lists. Separate live settings,
      restart-required settings and unsupported backend features.
- [x] Validate documented examples against normalization, field documentation
      coverage, Rust/ExUnit, quality and public/internal ExDoc/link checks.

The migration targets ordinary 0.4.2-to-0.5.0 application upgrades: dependency
update/precompiled downloads, mount, grouped settings, raw input and optional
live callbacks. Removed native lifecycle/status/protocol/qualification material
and forced source-build/deploy instructions from that user-facing guide. The
configuration reference is similarly focused on application use.

All 18 public config records now explain their fields with placement examples.
Regression checks cover every field and normalize/validate their examples plus
six migration config examples without opening a renderer. Runtime code, schemas,
defaults and types are verified unchanged.

Validation: 647 Elixir checks (15 doctests, 632 tests), 13 excluded; 1,580 Rust
unit tests plus 1 + 13 fixtures; quality passes on 191 files. Public/internal
ExDoc builds are warning-free, each generates 31 screenshots, and local links
pass for 67/82 pages. Logs: `/tmp/emerge-config-readable-*.log`.

Runtime behavior, package versions, Fairphone and unrelated files are unchanged.

### Tutorial consistency follow-up

- [x] Audit the five tutorials for 0.5 examples, obsolete startup/config advice,
      unexplained renderer-level material and consistency between examples.
- [x] Rework Use assets around viewport mount, explicit asset/font/path/cache
      structs and practical application steps; keep reference details in module docs.
- [x] Exercise tutorial callbacks/configs and validate Rust/ExUnit, quality and
      generated documentation/links. Preserve the pending sidebar-order changes.

Use assets now starts with a complete gallery viewport rather than standalone
renderer startup. Short examples cover logical/verified paths, image fitting,
backgrounds, SVGs, font registration, runtime-file policy and optional cache tuning.
Other tutorials use explicit records/simple record updates; the state guide targets
Solve 0.3.1 and its helper example no longer calls an undefined view function.

Regression checks compile the gallery and verified-path helper using existing
fixture files, validate four asset configurations, exercise the counter's live title,
reject renderer startup in tutorials and check the renamed nearby-section anchor.
The state guide's controller graph, lookup, UI and state-derived title were also
exercised against installed Solve 0.3.1 without starting a viewport.

Validation: 652 Elixir checks (15 doctests, 637 tests), 13 excluded; 1,580 Rust
unit tests plus 1 + 13 fixtures; quality passes on 191 files. Public/internal
ExDoc builds are warning-free, each generates 31 screenshots, and local links
pass for 67/82 pages. The rendered nearby link resolves to the renamed section.
Logs: `/tmp/emerge-tutorials-*.log`.

No runtime API changes, external consumer changes or device testing in this task.
