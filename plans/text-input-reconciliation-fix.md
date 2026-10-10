# Implemented fix: event-owned text, app-only reconciliation

Status: implemented with mount-scoped proposal IDs and a fixed five-second echo
window. Targets the confirmed FIFO, unchanged-value controller race without
changing the Notes handler or binding. Reset ambiguity remains documented.
See [implementation and validation](text-input-reconciliation.md).

## Core change

The event runtime owns live text, caret, selection, and composition for a mounted
input session. The native tree is its rendering/layout mirror, not a second
content authority. Reconcile only new Elixir-authored content proposals.

Use **one expected-app-echo ledger**, populated only by actual `on_change`
callbacks. Native receipts and animation rebuilds cannot consume it. Use a fixed,
generous app-round-trip timeout rather than a frame-derived TTL.

Do not add a pending-native-text-write queue. Instead, remove content write-back
triggered merely by a mismatch between runtime state and a native snapshot.
Publish content once when the runtime edits it or accepts a genuine app
replacement. A stale native snapshot is not a request to publish it again.
This removes the cause of the feedback loop rather than deduplicating its writes.

Existing listener barriers/outbox delivery still handle native ordering and
freshness. They are not a second text acknowledgment ledger and do not
acknowledge Elixir echoes. Geometry/style, focus/mount lifecycle, and those
receipts must still be processed; only native copies of editing state are ignored
as content authority. A newly mounted input is initialized from its app-declared
value rather than inheriting an old runtime session.

## Normal sequence

```text
Action                         Live state   Pending app echoes
---------------------------------------------------------------
Type a                         a|           [a]
Native tree acknowledges a     a|           [a]
Type b                         ab|          [a, ab]
Native tree acknowledges ab     ab|          [a, ab]
App echoes a                   ab|          [ab]
App echoes ab                  ab|          []
Type c                         abc|         [abc]
```

## Reconciliation rules

1. On a local edit, publish its native content/runtime update once. If it emits a
   callback, record the emitted full value in an ordered, mount-scoped echo ledger.
2. On a native rebuild without a new app content proposal, update geometry/style
   and handle existing ordering receipts. Do not overwrite live editing state,
   consume expected echoes, or issue content corrections for snapshot mismatches.
3. On a **new app content patch**, classify it before applying content:
   - Matching expected echo: consume the match and the earlier prefix it supersedes;
     preserve current content, caret, selection, and preedit. Keep all later echoes.
   - Equal to current live content: no text replacement or cursor/preedit reset.
   - Neither: accept through the application-replacement path and publish the
     accepted state to the tree once. This is not a user edit and emits no new
     `on_change`. Do not automatically erase outstanding echo history; those
     callbacks may still return afterward.
4. When a render coalesces several ordered controller updates, matching the later
   value can acknowledge the earlier prefix. This assumes FIFO app publication;
   arbitrary out-of-order asynchronous results need explicit edit identities.
   For repeated equal values, match conservatively from the oldest entry rather
   than guessing the newest revision from string equality.
5. A recognized echo requires no native content write: the event runtime already
   published the edit. The tree must carry app text as a proposal rather than
   overwriting the mirrored content before classification. No compare-and-resend
   loop is needed.

## Elixir proposal identity (agreed)

Represent each proposal as a value plus a monotonically increasing ID scoped to
its input mount: logically `(mount_id, proposal_seq, content)`.

- Assign a new sequence at the Elixir patch ingestion boundary when app-declared
  content changes. Native edits, animation pulses, and registry refreshes do not
  create proposal IDs. This is internal tree/event metadata; it requires no ID
  plumbing through the supplied Solve controller.
- Keep the latest app proposal separate from the native text mirror. Native
  `SetTextInputContent` must neither overwrite nor clear that proposal: its
  classification may still be pending in the event runtime.
- Carry the same proposal ID through all cached/animation registry snapshots.
- The event runtime remembers `last_processed_proposal_id` for the same mount.
  Only a greater sequence enters echo/replacement classification. Mark it
  processed whether it is an echo, an equal-value no-op, or a replacement.
- Leave the latest proposal in the tree; there is no need for an extra native
  acknowledgment command just to clear it. Duplicate snapshots are harmless
  because of the event runtime's watermark. Later app proposals replace the slot;
  existing FIFO/coalescing rules apply when intermediate snapshots are skipped.
- Distinguish changed application content from a full `SetAttrs` that only changes
  styles and repeats the last app-declared value. Compare against that declaration,
  not the native-mutated mirror. A style-only patch must not invent a new proposal.
- Reset the sequence/watermark with the mount, never transfer it by reused node ID.

Example: proposal 12 (`"a"`) is recognized as an echo once. Five animation
snapshots containing proposal 12 do nothing to editing state. Proposal 13
(`"ab"`) is then classified once. No content resend or native-write ledger is
needed.

This identifies app proposals, not the originating native edit. It solves
snapshot replay; value-only echo/reset ambiguity remains as documented below.

## Lifetime and cleanup

- Scope echo history to a mounted input; clear it on removal, remount, or renderer
  teardown. Do not transfer it by numeric node ID to a new mount.
- Do not discard expected echoes solely on blur. A delayed response can cross a
  blur/refocus. Apply the same app-proposal classification to an existing unfocused
  session: the current direct unfocused content assignment and reset-from-rebuild
  paths would otherwise bypass event-runtime authority. Focus loss can clear
  selection/composition as a lifecycle action, not adopt stale snapshot content.
- No echo ledger for inputs that emit no change callbacks.
- Reclaim matching/coalesced echo prefixes immediately and unmatched entries on
  expiry. No indefinite history is required for non-echoing handlers.

## Echo timeout policy

A timeout is a deliberate latency contract for text input, not an acknowledgment
that the controller processed an event. It is acceptable to stop recognizing
unmatched echoes outside the normal round-trip window.

- Fixed window: **5 seconds**, measured with a monotonic clock from
  callback emission/recording. Keep it independent of frame rate and native
  acknowledgment. The duration is a tuning choice, not derived from frame timing.
- Native receipts, animation snapshots, and duplicate proposal IDs do not shorten
  or extend the window. Do not keep stale entries alive by refreshing them on
  every rebuild.
- Prune expired entries before adding or matching echoes. Cleanup must not itself
  issue content writes or generate registry feedback.
- Missing echoes, transformations, and render coalescing that produces no content
  patch therefore cannot accumulate history indefinitely during continued use.
  The window bounds retained history in time, not to a fixed byte count independent
  of input size/rate.
- A previously processed proposal remains ignored by its ID even after echo
  history expires. A *first-seen* proposal arriving after expiry can no longer be
  recognized as that old echo; process it under ordinary app-replacement rules.
  Supporting arbitrarily delayed responses is outside this repair's contract.
- No end-to-end Solve acknowledgment protocol or input backpressure is required
  for this scoped fix.

## Remaining value-only ambiguity

An intentional replacement equal to an outstanding, unexpired echo is
indistinguishable from that echo. Under this proposal it is treated as an echo;
an explicit reset operation or origin/edit metadata would be needed to
disambiguate that case. Proposal IDs prevent snapshot replay, not this ambiguity.

## Implementation scope

- `events/registry_builder.rs`: continue publishing edits and record expected
  echoes only for emitted callbacks; no native-write ledger.
- `events/runtime.rs`: make content reconciliation app-proposal-only; remove
  snapshot-driven corrective writes and their deduplication bookkeeping. Replace
  frame-based pending-patch expiry with the fixed app-echo window. Retain native
  barriers, echo history, and mount-safe lifecycle handling.
- `tree/element.rs`, `tree/patch.rs`, and registry state: retain the latest ID'd
  app proposal independently of native content; distinguish app declarations
  from runtime content. Use a mount-scoped event-runtime processing watermark.
- No change to the supplied Notes controller or public binding for the diagnosed
  FIFO unchanged-value case. Broader acknowledgment/reset APIs are a separate
  decision, not an implicit requirement imposed on each controller.

## Regression coverage

The saved failing diagnostic is now a permanent regression in
`events/runtime/tests/delayed/text_echo.rs`. Coverage includes native
ack before/after app echo, multiple native acks before echoes, delays beyond the
old TTL, coalesced echoes, repeated values, duplicate animation snapshots,
style-only `SetAttrs`, cursor-in-middle/selection/IME, blur/refocus, same-ID remount,
no-handler inputs, and genuine replacements followed by older echoes. Assert that
native-only stale snapshots emit zero corrective text writes and that each
accepted app replacement publishes once. Use controlled timestamps to test echoes
within the window, expiry boundaries, first-seen late proposals, processed-proposal
replay after expiry, and cleanup for non-echoing handlers; no sleep-based tests.

Full Cargo and Mix suites were run, including the original animation and
feedback-loop tests. See the validation summary for the unrelated BGRA8888 Mix
failure. The DRM animation presentation fix was not reverted.
