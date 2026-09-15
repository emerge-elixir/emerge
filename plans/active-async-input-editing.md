# Async local editing and geometry-dependent input

**Status: deferred; not an animation completion dependency.**
[Minimal animation closeout](shared-animation-closeout.md) is complete;
[later work L1](later-animation-runtime-qualification.md#l1-async-input-editing-and-consistency--deferred)
owns this draft. D22 ordered-host admission is implemented and validated. D23 added
an unfinished native-only async prototype, with one failing reset/geometry-patch
test. It is now removed from the closeout runtime and preserved in
[the deferred forward patch](artifacts/shared-animation-closeout/deferred-d23.patch),
with [fresh failure evidence](artifacts/shared-animation-closeout/deferred-d23-before.log).
No async semantics were enabled by animation closeout.

The remaining text records the earlier proposed direction. Its geometry-wait and
controlled-value contracts require review before implementation resumes; latest-
snapshot dispatch is not an approved replacement either. D19 overflow/renderer-
failure policy remains separate and **unapproved**. No work here takes priority
over animation closeout.

## Intended behavior

- Typing and eligible text edits immediately update the focused mount's local state.
- Tree/layout/render updates follow asynchronously, without a round trip per edit.
- Input that needs newer geometry or a focus/binding decision waits in order.
- Reception continues during that wait, with safe adjacent-only coalescing.
- Callbacks and semantic input are not discarded to achieve faster tree publication.

Evidence: `artifacts/event-pressure-probe/README.md`. On this host, the 20k-node test
kept ingress nearly empty at 1k edits/sec while 736–812 edits waited for tree replies.
This plan targets that dependency, not the event thread's ability to receive input.

## Non-negotiable ordering rules

1. **One ordered semantic input stream.** Local edits may pass a pending *publication*
   of earlier local edits, but may not bypass an earlier unresolved pointer/focus/
   binding-changing action. Example: `type A → click another input → type B` must
   not append B to the old input while the click waits for geometry.
2. Coalesce adjacent pointer positions/latest resize and compatible scroll deltas.
   Never cross press/release, keys, commits, composition, focus/capture changes or
   incompatible target/coordinate contexts. Preserve raw-observer versus replay
   semantics; do not emit callbacks twice or replay obsolete hover crossings.
3. Geometry waits cover a **fixed required edit watermark** and relevant native
   context, not “whatever is newest when the reply arrives.” Later arrivals cannot
   move the requirement forever and starve a click. Failed/foreign/older responses
   never certify readiness. Cached no-op responses remain valid when appropriate.
4. Local text ownership and packets belong to renderer + mount + editing/binding
   lifetime, never reusable numeric IDs. Keep D14 packet admission/revalidation and
   grouped focus effects. Do not strip proof to batch delivery.
5. Local state can lead published geometry. Native caret/hit/IME rectangles must
   come from a coherent published snapshot, not new cursor offsets combined with
   old layout. Native registry readiness still is not compositor/display acknowledgment.

## Implementation slices

### 1. Reproduce the serialization and specify authority

**Files:** `events/runtime.rs`, `actors.rs`, `events.rs`,
`events/runtime/tests/delayed/`, `runtime/tree_update.rs`.

- [ ] Add a held-tree regression: many eligible edits advance local text/cursor and
  ordered callbacks before any tree response, while a later geometry action waits.
- [x] Trace both blockers: `SetTextInputContent` requesting listener freshness **and**
  `PendingDispatchEffects::collect` marking most callbacks as rebuild-dependent.
  Removing only the message's stale flag is not a solution.
- [ ] Define edit sequence/watermark, mount/binding lifetime and response metadata.
  Separate successful publication acknowledgment from transport/attempt completion.
  A failed attempt may free the delivery slot but must not settle a geometry fence.
- [ ] Specify controlled-value conflicts before enabling the fast path:
  - proven echoes of earlier edits cannot overwrite newer local edits;
  - equal text is not proof of an echo, and TTL expiry is not acknowledgment;
  - remount/removal invalidates the old editing lifetime;
  - same-mount authoritative reset, transformed application values, handler/focus
    changes and later local edits need explicit ordering/reset rules and tests;
  - do not guess a merge or replay edits into a new owner.
  Preserve public callback arguments; propagate internal causal metadata through
  reconciliation/codec/host transport if needed. Untagged writes cannot be assumed
  to be harmless echoes. Review this decision table before broad enablement.

**Exit:** failing examples and a concrete authority/conflict table, not a boolean
change that merely stops waiting while allowing stale snapshots to reset text.

### 2. Separate local editing from geometry freshness

- [ ] Replace blanket stale-lane gating with dependency-aware dispatch using the
  same runtime, local text state and ordered input queue—not a second event engine.
- [ ] Initial eligible operations: insertion/commit, logical deletion and selection/
  composition changes whose target and semantics are known from local state.
  Validate Unicode/grapheme and IME ranges. Visual-line navigation, pointer selection,
  Tab/focus traversal and geometry-based scrolling stay fenced when needed.
- [ ] Keep edits progressing despite their own text-size/layout invalidations;
  mark geometry outdated without freezing unrelated local text computation.
- [ ] Treat clipboard, host command/edit APIs, virtual-key/synthetic input and ordinary
  raw input consistently. Eligibility must describe the **whole operation**, including
  binding/focus-changing effects—not just the name of the incoming key.
- [ ] Preserve every ordered callback. Do not infer from a callback name that it
  cannot cause application changes; handle later model updates under slice 1 rules.

### 3. Publish accumulated state without a growing snapshot history

- [ ] For the active contiguous editing cohort, retain local current state, one
  in-flight tree update and one latest dirty state/version. Do not retain a full
  text snapshot per keystroke or build packets merely to discard them immediately.
- [ ] Flush at bounded dispatch/work boundaries and required semantic fences;
  continuous input must not postpone flushing until an idle channel. No arbitrary
  per-character timer or new animation clock.
- [ ] The tree receives versioned state reflecting all covered edits. Coalescing
  native publications is not coalescing away semantic callbacks. Do not combine
  across mount, binding, focus, model/control or other noncommuting boundaries.
- [ ] While the first update is in flight, newer local edits remain usable. Its
  acknowledgment advances only its watermark; it cannot restore its older text,
  cursor, selection or preedit. Then flush the newest pending eligible state.
- [ ] Failure/disconnection handling cannot strand the single in-flight slot.
  Keep native publication transactional and preserve successful patch prefixes/
  first-write animation sources. Use explicit attempt status, not fake success,
  time-based acknowledgment or permanent retries that conceal missing semantics.
- [ ] Superseded internal storage is freed synchronously. Count copies, nested
  payload capacity, callbacks and disposal; no whole-memory-bound claim.

### 4. Continue receiving and coalescing during dependency waits

- [ ] Dispatch the ready prefix; stop at the first unresolved semantic dependency.
  Keep consuming ingress and coalescing only the compatible tail while it waits.
- [ ] Before pointer/focus work, submit the required edit state and fence that
  watermark. Revalidate captures/hover and use the accepted native geometry on replay.
- [ ] Preserve `move, move, press, move, move, release` as
  `latest move, press, latest move, release`, not one final position.
  A queued click/Tab deciding the next target fences subsequent typing.
- [ ] Preserve D17 work quanta and D18 selectable output/input/control/timer handling.
  Stop remains independent of data credit/registry acknowledgment. Do not add another
  unbounded queue to duplicate retained inputs or a frozen subtree/geometry engine.

### 5. Integrate shared native, host and public runtime paths

- [ ] Implement the same authority/dispatch protocol for event actors and
  `HostEventRuntime`; keep bounded host drains and feedback calls.
- [ ] Update registry payload/coalescer logic so newer published binding/remount
  information is handled correctly without treating an old edit acknowledgment as
  current geometry authority. Preserve mount-focus requests and renderer isolation.
- [ ] Replace obsolete value-history/TTL reliance for versioned local edits; retain
  only genuinely necessary compatibility behavior with explicit tests.
- [ ] Audit `events.rs`, `registry_builder.rs`, `runtime/tree_update.rs`, tree actor,
  Elixir event routing/reconciliation, test harness and macOS host/codec call sites.
  Coordinate protocol/schema versions and matching artifacts if metadata crosses
  those boundaries. No public animation feature flag or altered callback arity.

### 6. Regression and failure matrix

- [ ] Tree held while typing A/B/C: local state/callbacks advance; publication catches
  up without three serial waits. Old responses do not roll state back.
- [ ] Text changes width/wrapping; pointer selection waits for the covering geometry.
- [ ] Typing → queued click/Tab → typing, with same target, changed focus, removal,
  remount and authoritative content reset; no targeting the wrong input.
- [ ] Backspace/delete, Unicode/graphemes, selection replacement, IME lifecycle,
  clipboard, synthetic repeat and visual navigation through native and host APIs.
- [ ] Controlled echoes/normalization, A→B→A values, concurrent model edits, old/
  foreign responses, both decode policies, failed preparation/retry and successful
  patch prefixes. Test failure while newer local edits exist.
- [ ] Animation/viewport/scroll/scale/font changes, delayed rendering and same-mount
  movement; compare registry/hit/IME state and final raster output, not just text.
- [ ] Full channels, continuous arrivals, independent Stop/disconnection, bounded
  batch work, raw/callback order and synchronous storage disposal.
- [ ] Public ExUnit traces as well as direct engine, real actors and shared host
  tests. No physical macOS/display claim from native Linux helper tests.

### 7. Re-run the isolated measurement and validate

- [ ] Repeat the **same** 90-process event-pressure matrix with exclusive lock,
  immutable source/build identity and raw results; retain the original baseline.
- [ ] Show the 20k/1k-edits case no longer leaves hundreds of *locally unprocessed*
  edits waiting on per-character tree replies. Report local completion separately
  from publication/geometry catch-up; moving backlog into packets is not success.
- [ ] Add longer text, actual BEAM callbacks/reconciliation, sustained traffic and
  geometry/focus interleaving. Measure published updates/round trips, copies/live
  storage, tail latency and disposal. Do not assert a speedup before measurement.
- [ ] Run `cargo test`, source-built `mix test`, `./ci-tests.sh all`, Rust fmt and
  denied-warning Clippy including benches/tests/`bench-diagnostics`.
- [ ] Update events guide and remaining coverage. No P1–P9 closure from this slice
  alone; overflow policy, ingress loss, device/physical presentation and global
  memory/performance qualification remain separate work.

## D22 implementation checkpoint — ordered host admission prerequisite

- [x] Put raw input, host commands/edits and source-scoped replacement ranges in one
  FIFO; prevent host mutation/callbacks from overtaking queued raw/focus work.
- [x] Preserve adjacent raw coalescing, raw-once delivery, 64-item replay, native
  receipts, host feedback budgets, source-mount evidence and synchronous disposal.
- [x] Scope deferred ranges to a mount plus an opaque local text/focus generation;
  reject remount, focus ABA and intervening edits/accepted replacements. A later
  ordinary key still resolves against the then-current focus.
- [x] Eleven directed regressions plus full validation: Rust 1461 + 14, standalone
  Mix 520, full CI 526, strict Clippy/fmt and Dialyzer 0.
- [ ] Implement local edit authority, causal controlled-value metadata and batched
  publication; remove per-edit waits and re-run the pressure matrix.

Evidence: `artifacts/shared-animation-remaining/validation/ordered-host/`.
A controlled before-behavior run disabling only the three new host admission guards
fails 10 of the final 11 tests. It is not an old-HEAD build. Two D18 transport tests
now explicitly inject already-resolved effects; they no longer rely on the unsafe
host admission bypass to create queue pressure. New tests cover real host admission.

### Authority decisions still needed for the main fast path

| Input/update | Required behavior |
|---|---|
| Own older native publication | Advance only its covered watermark; keep newer local text/selection/preedit |
| Geometry action after local edits | Freeze a covering watermark; later typing cannot bypass the action |
| Proven old application echo | Do not restore old text; acknowledge causal origin without value-history guessing |
| Untagged same-mount write | Cannot assume echo; define authoritative-write ordering and compatibility explicitly |
| Application transformation/reset | Explicit replacement boundary; do not invent an edit merge or silently relabel it as echo |
| Remount/removal | Invalidate old ownership; no old packet/range mutates the replacement |
| Failed publication attempt | Not a geometry acknowledgment; separately complete delivery bookkeeping to allow recovery |

`reconcile_text_input_states` currently recognizes application echoes by matching
pending values with a TTL. That cannot distinguish a delayed echo from an intentional
reset to the same value. This slice **does not remove that barrier or claim to solve
that ambiguity**. Internal callback/reconciliation provenance and native publication
versions remain next, without changing public callback arguments by accident.
No new overflow policy, wire version, NIF, atom, production thread or animation clock.
