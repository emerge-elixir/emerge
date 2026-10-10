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
snapshot: title, frame cap, and private input mask. It validates capabilities
before accepting a revision. The DirtyIo wrapper uses a nonblocking enqueue;
a full/disconnected event queue returns an error without advancing the revision.
The event actor applies input subscriptions, then dispatches backend controls.
Stale requests are discarded before their next side effect.

Wayland has a one-slot latest-config mailbox and coalescing calloop wake. The
window owner publishes the cap and calls the title setter before reporting
completion. No scene upload or redraw is required. This is owner-side application,
not compositor acknowledgement or visible presentation. Closing the mailbox
rejects producers and fails its pending request.

Headless publishes the existing atomic cap on the event owner. Future render
admissions read that control directly; no render is needed for acknowledgement.
This adds no new pacing policy. DRM currently supports only input changes through
this path. Its power/backlight/output controls remain fail-closed.

Submission, current-revision checks and short owner-side effects are serialized.
Do not extend these critical sections to GPU/KMS waits. Native owner threads send
completion messages; NIF scheduler threads never use `OwnedEnv.send_and_clear`.
On session shutdown, completion delivery may end with the worker. Status treats
pending work as failed once stopping; renderer lifecycle/cleanup remains the
terminal ownership authority and recovery replays desired intent.

## Remaining boundaries

- macOS keeps its existing acknowledged host title/input operations; native
  revision reporting for that transport is not yet implemented.
- Power/backlight and output/mode changes need matching config/tree/layout
  revisions, KMS completion, stale-enable protection and ownership-safe cleanup.
  Live-control acknowledgements above are not those guarantees.
- Power controls must stay independently wakeable even when event/tree work is
  blocked. This slice uses the existing bounded event queue for input ordering;
  its acceptance/completion is not a latency guarantee for future power controls.
- Hardware qualification, including backlight flash observation, remains required.
  The separately planned Vulkan acquisition synchronization repair is deferred.
