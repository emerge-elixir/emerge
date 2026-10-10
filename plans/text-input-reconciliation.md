# Text input reconciliation

Status: implemented. Event-runtime authority, mount-scoped app proposal IDs, and a
fixed five-second expected-echo window replace native-snapshot reconciliation.

See the [confirmed diagnosis](text-input-reconciliation-diagnosis.md),
[implemented design](text-input-reconciliation-fix.md), and
[permanent regressions](../native/emerge_skia/src/events/runtime/tests/delayed/text_echo.rs).

## Goal and reported setup

Preserve content, caret, selection, and IME composition across delayed controller
echoes for physical/virtual keyboards and single-line/multiline inputs.

The supplied Fairphone Notes controller stores each full emitted string unchanged
in Solve state, then binds `notes.text` back to `Input.text/2`. The keyboard helper
only adds focus/blur and layer handlers. No application changes are required.
Solve's own revisions order controller updates, but do not acknowledge native edits.

## Confirmed failure

1. Type `a|`; acknowledge it natively while delaying the app echo.
2. Type `ab|`; acknowledge it natively again.
3. Deliver app echoes `a`, then `ab` in FIFO order.
4. The old reconciliation code prematurely consumed the expected echoes on native
   acknowledgments, accepted `a` as a replacement, and clamped the cursor to 1.
5. Typing `c` then emitted `acb`, not `abc`.

This reproduced with real native runtime/tree processing and binary app patches,
even with diagnostic expiry extended to 60 seconds. It needed neither animation
nor out-of-order controller messages. The offending branch originated in
`76ce013` alongside the DRM animation-stall repair; that backend repair is retained.

## Implemented

- [x] Keep latest app declaration separate from native mirrored text.
- [x] Increment proposal IDs at native `SetAttrs` ingestion only for changed app
  content; process each ID once within its mount.
- [x] Stop native snapshots from replacing/resending live text or consuming echoes.
- [x] Retain one callback-only echo ledger with a fixed five-second monotonic TTL.
- [x] Preserve echoes across blur/refocus; discard state/history on removal/remount.
- [x] Apply genuine replacements once, without callbacks or erasing older echoes.
- [x] Preserve existing native barriers, host range rules, and slider TTL behavior.
- [x] Document the controlled-value contract and timeout/reset limitations.

No BEAM wire-format change or Solve metadata plumbing was needed: proposal IDs
are native ingestion metadata. Tests use actual EMRG patches, the shared host
runtime, and the threaded event actor. Existing Elixir routing/serialization tests
remain part of full-suite validation.

## Validation

- Original regression failed before the fix; the corrected invariant now passes.
- Permanent tests cover delayed/FIFO/coalesced and repeated echoes, both native/app
  arrival orders, animation/duplicate snapshots, style-only patches, real virtual
  keys, Unicode/selection/composition, blur/refocus, remounts, replacements,
  no-handler inputs, and deterministic expiry boundaries without sleeps.
- Stale native snapshots emit no corrective text writes; accepted replacements
  publish once. The threaded test inserts event boundaries so both FIFO echoes
  are processed rather than coalesced away.
- `cargo test --release`: 1,503 unit tests and 14 integration tests pass.
- `./ci-tests.sh quality` and `cargo clippy --all-targets -- -D warnings`: pass.
- `mix test --include full_sweep --warnings-as-errors`: 594/595 pass, 4 excluded.
  The remaining test, `test/emerge_skia_test.exs:605`, rejects BGRA8888 through
  `VideoInterop.validate/1`: the unchanged lock pins `video_interop` 0.1.2, which
  lacks BGRA8888 support. This is unrelated to text reconciliation; the lock and
  dependency are intentionally unchanged. `./ci-tests.sh test` stops there, so the
  complete Cargo suite was also run separately.
- No on-device Fairphone validation or Dialyzer claim.

## Contract limits

A first-seen app proposal beyond five seconds may be treated as a replacement.
Processed IDs remain ignored after expiry. An intentional reset equal to an
outstanding unexpired echo is treated as that echo. Supporting arbitrary latency,
out-of-order asynchronous results, or explicit reset causality requires a separate
end-to-end edit/acknowledgment protocol and is outside this repair.

## Follow-up: Todo caret after the first character

Reported during demo testing on all inputs, through physical and virtual keys:
typing the first character leaves the visible caret at the start.

Reproduced: the event runtime held cursor 1 while the tree held cursor 0.
`Element::normalize_extracted_state` clamped native runtime positions against
`layout.effective.content`, which still contained the previous empty frame during
deferred frame preparation. This affects normal uploaded trees, not just active
animations. The removed corrective-write behavior had masked the tree-side error.

Fixed normalization to use the current native mirrored content in
`spec.declared.content`. Frame projection remains deferred; no corrective resends
were restored and the event runtime still owns editing state.

The original fixture started with `RebuildRegistry`, bypassing production frame
authority, and primarily asserted event-owned state. It now initializes through
full frame preparation and checks the native mirror too. With these checks, 13
regressions failed before the normalization fix and pass afterward.

Added a real binary-upload/pointer-focus regression asserting the first edit's
rendered caret area and IME state, without a corrective round trip, for both input
kinds. It also checks insertion in the middle rather than forcing the caret to the
end. The virtual-key test now starts with a virtual keystroke. A normalization unit
test covers Unicode positions, selection/preedit retention, and actual shortening.
Full Rust tests and Mix tests were rerun; the same unrelated BGRA8888 dependency
failure remains in Mix.

## Commit organization

1. Fix native cursor normalization against current text, with its focused unit
   regression. This prerequisite prevents a broken rendered caret when corrective
   resends are removed.
2. Introduce app-only reconciliation and proposal IDs together with integration
   regressions, controlled-input documentation, and release notes.
3. Record the diagnosis, design decisions, and validation history in these plans.
4. Commit the demo's local dependency switch separately in `../emerge_demo`.
