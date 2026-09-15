# Later work: animation performance, runtime and platform qualification

**Status: deferred; not a dependency of shared animation implementation closeout.**
Animation implementation is complete; [final evidence](artifacts/shared-animation-closeout/README.md)
is the completion record, not another plan. This is the **sole deferred backlog**
for async editing, pressure policy, animation optimization and broader qualification.
Select a focused package before resuming work; deferred does not mean approved,
solved, or safe to ship on every target.

The separate [active low-resource plan](active-low-resource-animation-smoothness.md)
owns existing constrained-device transform/patch budgets and cadence work. Do not
create duplicate target gates here. General GPU and asset qualification retain their
existing owners in L5.

Completed implementation, rebase and commit checklists were removed from `plans/`;
Git history through `6b2c4aa` preserves their full text and the earlier async/pressure
drafts. Raw evidence, source archives, manifests and the D23 patch stay in place.
This consolidation changes no runtime contract, approval or measured result.
All old P1–P9 follow-ups are covered by L1–L5; none is an implicit implementation
completion gate. Promote demonstrated supported correctness failures to regressions;
use bounded risk-based campaigns, not exhaustive permutations without a stopping rule.

## L1. Async input editing and consistency — deferred

Evidence: [isolated input measurements](artifacts/event-pressure-probe/README.md)
and the [preserved D23 prototype](artifacts/shared-animation-closeout/deferred-d23.patch).
D23 is no longer in the active runtime; the patch includes its source and tests.

- [ ] Revisit the contract before code: latest-installed-snapshot dispatch versus
  waiting for effects of earlier input; geometry/focus/text reconciliation; application
  resets versus echoes; native request completion versus successful publication.
  Neither removing all waits nor the draft's geometry-watermark policy is settled.
- [ ] Decide whether to reuse the D23 native-only prototype. Preserve its failure:
  `authoritative_reset_rejects_old_scope_but_geometry_only_patch_preserves_it` returned
  empty text instead of `abcXY`. Check fixture omission/SetAttrs semantics and actual
  authority behavior; do not merely change the assertion.
- [ ] Specify callback-controlled values, mount/binding lifetimes and host/IME range
  behavior before enabling batching. No callback dropping or value/TTL-as-causal-proof.
- [ ] Implement only the agreed protocol, then validate raw/host/synthetic/clipboard
  order, failures and remounts. Retain D22's existing ordered-host safety unless an
  explicitly tested replacement contract supersedes it.
- [ ] Re-run the unchanged isolated 90-process baseline, then realistic long-text,
  BEAM/reconciliation and sustained input cases. Measure local completion separately
  from native publication/display; no speedup claim from moving a backlog.

### Contract decisions and candidate implementation

D22 already orders raw input, host commands/edits and replacement ranges; keep its
mount/generation checks, 64-item replay and bounded host feedback. Content edits
still wait for registry publication. D23 is a failing native-only experiment, not
an approved fast path. The previous draft's direction was:

- Advance eligible local text/cursor/selection/preedit immediately, then publish
  accumulated state asynchronously in the same runtime and ordered input queue.
- Retain current state, one in-flight publication and one latest dirty state, not
  a text snapshot per keystroke. Flush at bounded work boundaries even under
  continuous arrivals. Native publication coalescing must preserve every callback.
- Never let typing bypass an earlier unresolved click/Tab/focus/binding action.
  A fixed covering edit watermark was proposed to prevent later input starving a
  geometry wait; this policy still needs review. Visual navigation, selection hits
  and IME rectangles require coherent accepted geometry, not new offsets on old layout.
- Keep raw, host, clipboard and synthetic input consistent. An operation's complete
  effects determine eligibility; its key/callback name is not a safety proof.

Review an explicit authority table before implementing:

| Update | Required distinction |
|---|---|
| Older own native publication | Acknowledge covered work without restoring older local text/cursor/preedit |
| Proven application echo | Distinguish causal origin from equality of text or TTL history |
| Untagged write / transformation / reset | Specify authoritative replacement and ordering; do not guess an echo or merge |
| Remount, removal, focus/binding change | Invalidate old ownership; no old packet/range targets a replacement |
| Failed attempt | Free delivery bookkeeping if appropriate, but never certify geometry readiness |

The existing value-history/TTL echo heuristic cannot distinguish an intentional
reset to an earlier value from a delayed echo. Public callback arguments must remain
compatible; review internal reconciliation/codec/host metadata and matching protocol
artifacts if a solution needs them. Native receipts do not acknowledge BEAM callback
completion, renderer installation or display.

Validate held-tree bursts, typing→click/Tab→typing, Unicode/graphemes/IME, echo/reset
ABA, failure with newer edits, both patch-decode policies, remounts, asset/viewport/
scroll changes and real actor/shared-host/public paths. Keep successful patch
prefixes and transactional publication. Count construction, copies and synchronous
disposal; no new thread, second geometry engine or overflow policy by implication.
After an agreed implementation, run Rust/Mix/full CI, fmt and strict Clippy including
benches/tests/`bench-diagnostics`, plus the controlled measurements above.

## L2. Input pressure and whole-runtime progress — approval required

Evidence: [D19 source audit and bounded model](artifacts/shared-animation-remaining/pressure-contract/README.md).

- [ ] Decide admission/backpressure/overflow behavior and numerical defaults with
  the user. Terminal renderer failure and proposed limits remain **unapproved**.
- [ ] Address Wayland full-channel semantic-input drops and unbounded runtime FIFOs
  without silently evicting keys, button edges, commits or composition.
- [ ] Account for retained and transient allocation across producers, queues,
  packets, host/IME/clipboard paths and consumption. Native receipts are not BEAM
  consumption or allocator refunds; queue-slot counts are not whole-memory bounds.
- [ ] Qualify continuous arrivals, virtual-key/inertia/capture histories, controls,
  disconnection, callback stalls and shutdown. No claimed preemption of blocked
  drivers/assets/callbacks/joins; no alternate scheduler by default.

D16–D18 improvements are committed; do not rewrite them without a measured problem
and agreed policy. D22 closed the old host admission bypass, but did not bound memory.

### Unapproved D19 proposal retained for review

D19 proposed an explicit, sticky **terminal failure of the affected renderer** when
noncoalescible input cannot fit finite configured record, owned-capacity and input-size
limits. That behavior and all defaults remain unapproved; zero/unset must not silently
be reinterpreted as a safe bound. The alternative is proven producer backpressure
with bounded upstream storage and independent control/ack paths—not blocking a
Wayland callback or adding another unlimited queue.

If that proposal is selected, its essential requirements are:

- Reserve credits before acceptance, copying, wire/clipboard decoding and effect
  construction. Include expansion, nested payloads, spare capacity and old/new
  coexistence. Tiny edits can copy large text; input-size limits alone are insufficient.
- Credits travel with storage until actual last-owner disposal or an accounted
  persistent-state handoff. Sending, dequeueing or receiving an acknowledgment does
  not free live capacity. Allocation arithmetic must be checked.
- Cover producers, raw/host/clipboard/synthetic/replay paths, scratch packets, outbox,
  channels and receiving batches. Audit macOS peer-declared lengths before allocation.
  The initial scope is managed native interaction transport/construction, **not**
  persistent models, SDK/kernel queues, BEAM mailboxes, GPU, allocator overhead or RSS.
- Preserve semantic edges, callback order, mount proofs and causal receipts. Once an
  operation has emitted callbacks, rejecting its packet cannot retract them.
- A fault would latch a fixed-size reason, close admission, request independent Stop,
  and expose both notification and queryable terminal status. Neither notification
  delivery nor data credit may be required for stopping. Do not return ordinary
  unhandled-input `false` and trigger Cocoa fallback for a terminal failure.
- Do not publish a newly failed partial packet. Already admitted work may finish
  until quiescence; terminal failure is not rollback or preemption of blocked native
  calls. Restart would create a new renderer/mount lifetime.
- Agree public error/status and host protocol changes before enabling anything.
  Then test boundary/oversize allocation, mid-dispatch failure, callback/Stop races,
  remounts, delayed peers and disposal through native, host and public paths.

The nine Python model tests and depth-12 exploration (45,476 states / 318,533
transitions) check a declared-credit model, not native allocation or liveness. No
numerical budget, native concurrency proof or whole-runtime bound follows.

## L3. Animation cost and retention optimization — measurement driven

References: `artifacts/shared-animation-remaining/performance/` and the closeout's
current snapshot. Existing transform/patch target work stays exclusively in
[low-resource smoothness](active-low-resource-animation-smoothness.md); this section
owns broader geometry-animation cost/retention work, not a duplicate checklist.

- [ ] Profile private-model construction, queries/ancestry, live layout, registry/
  scene construction, commit and synchronous workspace/ghost disposal separately.
- [ ] Measure actual retained/peak allocations and repeated start/settle/cancel/
  retry/topology churn; distinguish live growth from allocator-retained RSS.
- [ ] Select the smallest measured optimization. COW declarations, leaner query
  nodes, sparse dirty traversal and batching are alternatives, not mandatory stages.
- [ ] Run the full 5k/20k × 1/64-owner matrix and expanded watcher/failure/topology
  workloads with source/build identities, exclusive lock and separate repeated
  processes. Preserve paint/pixel controls and total synchronous cost.
- [ ] Establish explicit geometry-animation memory/time budgets on the intended
  target. The old 20k desktop ≤12ms release gate needs independent justification;
  existing constrained-device refresh/patch budgets retain their original scope.

Historical baseline 03 predates D8 and the rebase: ~76ms warm/~87ms release/~377MiB
RSS for 20k/64 upward; full private-model memory is also expensive in simpler cases.
Those were failures, not current timings or an accepted target-device envelope.
The [closeout snapshot](artifacts/shared-animation-closeout/performance/table.md)
now records 20k/64 length release 15.888ms median, upward warm p50/p95
71.172/86.106ms and release 76.988ms, upward warm RSS 378.7MiB. It is disclosure,
not a controlled before/after speedup or completed optimization.
Do not conceal them when documenting implementation completion.

## L4. Broader robustness and qualification — bounded campaigns

- [ ] Choose explicit finite campaigns for remaining P2–P5 factors: combined input
  and asset races, randomized topology/root/Nearby transport, multi-owner/axis/phase
  composition, ghosts, cancellation/arrival and long-running retention.
- [ ] Extend event/runtime histories only for concrete risk areas: stationary hover,
  captures, IME, synthetic repeats/inertia, delayed installation and failed rendering.
- [ ] Record seeds, schedules, oracle and stopping criteria. Reuse native layout,
  existing actors and headless harness; no second reference layout engine.
- [ ] Promote concrete failures to regressions; green campaigns qualify those tested
  histories, not arbitrary histories or exact whole-runtime memory/liveness.

## L5. Physical platforms, GPU recovery and release artifacts — separate owners

Coordinate rather than duplicate:
[Linux GPU qualification](active-linux-gpu-qualification.md),
[low-resource smoothness](active-low-resource-animation-smoothness.md),
[asset runtime](active-per-renderer-asset-runtime.md), and
[platform differences](platform-runtime-architecture-differences.md).

- [ ] Execute actual Wayland input/resize/scale, DRM scanout/page flips, supported
  headless OpenGL/Vulkan output, macOS AppKit/Metal and constrained-device cadence.
  Test terminal/cleanup frames, stationary hits, repeated animation and idle.
- [ ] Qualify GPU context reconstruction, PRIME/packed-conversion/delivery failures,
  resource synchronization, blocked driver/host behavior and shutdown where supported.
- [ ] Build and verify matching EMRG9/macOS14 host/release artifacts; reject old
  hosts. A Linux protocol test is not external binary or macOS execution evidence.
- [ ] Record each route as executed, simulated, compile-only or unavailable. Keep
  physical cadence/display acknowledgments distinct from native scene/registry output.

Target-specific release claims remain gated by that target's evidence. Missing
hardware does not keep the shared animation implementation section open forever.
