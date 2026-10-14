# Config intent and native completion

Viewport callbacks receive the last validated **desired** config, not a hardware
snapshot. Operational failures retain that intent for retry and renderer recovery.
Callback/schema/native-capability validation failures retain the previous intent.
The lifecycle owner queues restart intent independently of slow native startup;
startup handshakes include the config actually used to create the renderer.
Changes made during startup are therefore reconciled before uploading its tree.

## Accepted is not applied

The viewport records the last renderer-accepted snapshot separately. A native
live submission returns a `{session_id, revision}` token. The native owners send:

```elixir
{:emerge_skia_config, {session_id, revision}, :ok}
{:emerge_skia_config, {session_id, revision}, {:error, reason}}
```

These messages go to the submitting process independently of raw input
subscriptions. The native adapter submits from the viewport process. Matching
session/revision tokens prevent late completions from affecting a replacement
renderer or a newer request. Completion does not trigger a render callback.
Failure retains desired intent; a subsequent rerender retries the full snapshot.
Synchronous custom adapters may still return `:ok` for acceptance, not proof of
hardware presentation.

`EmergeSkia.config_status/1` reports:

| Field | Meaning |
|---|---|
| `session_id` | Same session identity as renderer lifecycle status |
| `desired_revision` | Last accepted native live submission; zero before any submission |
| `pending_revision` | Accepted but awaiting owner application, or `nil` |
| `applied_revision` | Last confirmed, non-superseded live-control revision, or `nil` |
| `failed_revision`, `error` | Latest failed revision and reason, or `nil` |

The native desired revision can lag BEAM desired intent after queue rejection.
Startup config is not counted as a live revision. Status retains only bounded
progress, not a history. Applied is historical: a later failure can have partial
side effects; it does **not** imply rollback to the previous settings.

## Owner operations and bounds

Linux/native reconciliation uses one NIF with the complete supported live
snapshot: title, frame cap, private input mask, and DRM power/brightness. It
validates capabilities before accepting a revision. The DirtyIo wrapper uses a
nonblocking enqueue; a full/disconnected queue returns an error without advancing
the revision. Non-DRM controls pass through the event actor; DRM uses an independent
64-entry owner lane and joins input/output acknowledgements. Stale requests are
discarded before their next side effect.

Wayland has a one-slot latest-config mailbox and coalescing calloop wake. The
window owner publishes the cap and calls the title setter before reporting
completion. No scene upload or redraw is required. This is owner-side application,
not compositor acknowledgement or visible presentation. Closing the mailbox
rejects producers and fails its pending request.

Headless publishes the existing atomic cap on the event owner. Future render
admissions read that control directly; no render is needed for acknowledgement.
This adds no new pacing policy. DRM OpenGL, Vulkan and raster (CPU or GPU-upload
presentation) support display power and brightness through the same independent
owner lane. GPU upload reuses OpenGL/GBM retirement; CPU-only and Vulkan-only DRM
builds cannot present it. Automatic API selection, connector/mode changes and
DRM frame caps remain rejected.

Submission, current-revision checks and short owner-side effects are serialized.
Do not extend these critical sections to GPU/KMS waits. Native owner threads send
completion messages; NIF scheduler threads never use `OwnedEnv.send_and_clear`.
On session shutdown, completion delivery may end with the worker. Status treats
pending work as failed once stopping; renderer lifecycle/cleanup remains the
terminal ownership authority and recovery replays desired intent.

## Display off/on

Off defers the viewport render callback and serialization, not acquisition of the
output/backlight owner. Routing is bound once per renderer generation; recovery
can rebind while off without uploading a tree. The native tree actor retains the
latest tree. The viewport sends a fresh tree/diff when on requires it.

As off starts, native UI input is ignored and the presenter is stopped. Its queued
frames and GPU state are discarded, not saved for restoration. KMS is detached
before buffers are released; OpenGL work is finished once at teardown, not once
per frame. Vulkan uses GPU idle, blocking disable and old-event drain for managed
retirement, then releases imports and destroys the presenter. Drained flips do
not publish capture, timing or illumination hooks; managed teardown never
restores inherited scanout. Failed barriers retain/quarantine ownership. On creates a new presenter, asks the tree actor to render its latest
tree, and enables the managed backlight after the first frame is presented.
Initial-off does not require a tree upload. The app's independent wake-key reader
is not suspended.

There are no config-tree markers, frame receipts, presentation proof mailboxes,
power input epochs, or custom commit-cookie protocol. This is a simple
stop/recreate lifecycle, not strict config-to-frame provenance or a promise to
preserve interactions across a power cycle.

## Backlight ownership

Discovery uses proven connector ancestry; ambiguous associations require an
explicit device. Pinned descriptors and an identity reservation prevent another
Emerge owner from taking the same backlight. Other processes are outside that
reservation contract.

Takeover suppresses both `bl_power` and brightness. Brightness changes while dark
retain intent without illuminating. `nil` uses the initial requested brightness,
not `actual_brightness`; a viewport-owned native resource preserves that baseline
across renderer recovery. This follows the Linux
[backlight sysfs ABI](https://github.com/torvalds/linux/blob/master/Documentation/ABI/stable/sysfs-class-backlight).

The output/backlight owner stays alive across presenter recreation. It receives
config snapshots on an independent bounded lane and joins input/output control
acknowledgements. Admission alone does not mean application. Clean stop leaves a
managed device dark. Failed enable attempts suppression again; driver errors are
reported. Destructors do no backlight I/O and never restore illumination.
Unsafe KMS/GPU cleanup retains/quarantines ownership instead of freeing resources
still used by the driver. Ordinary off/on destroys and recreates GPU resources.

## Remaining boundaries

- macOS keeps its existing acknowledged host title/input operations; native
  revision reporting for that transport is not yet implemented.
- Connector/mode changes and automatic API power remain unsupported. Vulkan and
  GPU-upload raster DRM use the shared power owner, independently of the Wayland
  acquisition fix; panel/optical hardware qualification remains open.
- Panel-driver DCS behavior while unprepared needs device testing. Native sysfs
  errors remain failures; the implementation does not silently ignore them.
- Control admission is not a latency guarantee for GPU/KMS or sysfs operations.
- Hardware qualification, including backlight flash observation, remains required.
  Wayland Vulkan acquisition has a separate implemented fix and targeted Hyprland/
  RADV validation; interactive lifecycle and additional-driver checks remain open.
