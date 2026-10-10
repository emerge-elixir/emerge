# Controlled scroll positions

Status: implemented; regression and full-suite validation recorded below.
API direction: bare float fractions or child keys; no pixel/ratio wrappers.
Branch: `feat/controlled-scroll`, based on `c5046c2` (text-input reconciliation).

## Goal

Expose scroll-position attributes that can be bound to ordinary Solve controller
state. The event runtime owns each mounted container's live position. The tree
provides layout geometry and mirrors position for rendering; it is not a second
position authority.

Reuse the text-input contract: versioned app proposals, callback-only expected
echoes, a five-second monotonic echo window, and existing native ordering barriers.
Do not introduce a native-write acknowledgment queue or require Solve revisions
in application handlers.

## Public API

```elixir
# In a view using Emerge.UI and Solve.Lookup helpers:
feed = solve(App, :feed)

column(
  [
    height(px(300)),
    scrollbar_y(),
    scroll_y(feed.y),
    Event.on_scroll(event(feed, :scrolled))
  ],
  items
)

# In its Solve controller (register :scrolled as an event):
def scrolled(%{y: y}, state), do: %{state | y: y}
```

The two input forms are:

```elixir
scroll_y(0.5)             # halfway through the available vertical range
scroll_y(:selected_item)  # align the keyed child's top with the viewport top
scroll_x(:selected_item)  # analogous left-edge alignment
```

- Add `scroll_x(position)` and `scroll_y(position)` to `Emerge.UI.Scroll`, already
  imported by `use Emerge.UI`. A float is a normalized fraction in
  `[0.0, 1.0]`: zero is left/top, one is right/bottom, and 0.5 is halfway through
  the available scroll range, not halfway through total content height/width.
  For each axis: `rendered_offset = fraction * max(content_extent - viewport_extent, 0)`.
  For example, 1,000px of content in a 200px viewport maps 0.5 to a 400px offset.
  Pixel offsets/deltas are internal input/rendering details, not controller values.
- A key argument targets an element carrying `key(value)` in the container's
  scroll content. Resolve its geometry after layout and align the leading edge
  on the requested axis, clamped to the available range. See key semantics below.
- Do not add `px`, `pct`, or `ratio` position forms. Existing `px/1` remains a
  layout-sizing helper, not a scroll-position variant.
- Keep `scrollbar_x/0` and `scrollbar_y/0` as the scroll-enablement controls.
  Position attributes alone do not enable overflow scrolling.
- Add `Event.on_scroll/1`, using existing PID/message routing. Payload:
  `%{x: float, y: float}` in the same normalized `[0.0, 1.0]` units. A callback
  payload can be passed straight back into the position attributes.
- A controller can store the payload unchanged or retain only its controlled axis.
  Updating the stored value programmatically proposes a new position.
- Unspecified axes remain event-owned and uncontrolled; updating one axis cannot
  restore an old declaration for the other axis. Removing a position attribute
  releases app control without jumping to zero. Reintroducing it is a new proposal.
- Float arguments must be finite and within `[0.0, 1.0]`. Other arguments are
  semantic key references, not implicitly converted pixel offsets or fractions.
  Use `0.0`/`1.0` for endpoints; reserve float arguments for fractions, including
  rejecting out-of-range floats rather than treating them as keys. Validate key
  arguments against existing non-nil, non-float key values.
  Runtime motion clamps progress to `[0, 1]`; negative input deltas remain valid.
- Native scrolling works without a callback. App proposals work without one too.
- Accepted app proposals do not emit another callback, matching text-input
  replacement semantics. Each new fraction or key-target proposal is applied
  once; a repeated declaration cannot undo subsequent native movement. Emitted
  fractions must round-trip without rounding producing false replacements.

## Key-target semantics

- `scroll_y(:child)` requests top alignment, not merely making the child visible.
  A child already visible below the viewport's top must still move to the top
  if the scroll range allows it. `scroll_x(:child)` requests left alignment.
- Resolve the child's leading edge and the visible scrollport's leading edge in
  the same unscrolled coordinate space, accounting for ancestor offsets and the
  existing border/padding/transform rules. Clamp the resulting target offset to
  `[0, max_scroll]`, then convert to the event-owned fraction.
- A target near the end may stop short of exact alignment because there is not
  enough content after it. Never invent extra scroll range or overscroll.
- Keys must refer to the addressed container's scroll-content descendants. Do
  not scroll unrelated containers or implicitly treat an outside/overlay node
  as scroll content. Intervening same-axis scrollports are excluded; this is
  not a recursive reveal-all API. Alignment uses layout boxes, not paint transforms.
- Apply once per new proposal after the complete patch batch and a
  valid layout, including a target inserted in that same batch. Do not replay the
  jump on animation frames, style patches, or native acknowledgments. It is not
  a permanent pin to the child as subsequent layout changes occur.
- Native motion after the jump emits fractions through `on_scroll`, never a key.
  Solve can store those fractions and bind them straight back to the same attr.
  A key jump itself is an app replacement and does not emit a feedback callback.
- Resolve semantic keys to numeric node IDs in Elixir using the *new* reconciled
  tree, not the old-key index. Keep native references mount-safe. Do not send
  arbitrary Elixir terms through the binary protocol or add atom creation in Rust.
- Resolve references as part of existing ID assignment/reconciliation work, with
  a shared key lookup map and post-order resolution after descendant IDs exist. Do not scan the whole tree
  independently for every scrolling container or every target.
- Preserve the distinction between an app declaration and its resolved target
  metadata. A completed jump must not silently become a fresh app proposal merely
  because the target moved or was remounted under the same semantic key. A stale
  reference must never address a replacement mount.
- Missing/removed targets leave position unchanged and produce a diagnostic
  with event debug logging enabled. Consume them once; do not defer them until
  a target appears in a later render.

## Pre-implementation findings (historical audit)

### Elixir

- `lib/emerge/ui/scroll.ex` only exposes `scrollbar_x/0` and `scrollbar_y/0`.
- `lib/emerge/engine/attr_codec.ex` already reserves f64 tags 38/39 for
  `scroll_x`/`scroll_y`, but their existing native meaning is a pixel offset.
  Audit compatibility before choosing fresh normalized-position tags or an
  explicit wire-format migration; do not silently reinterpret old payloads.
- `lib/emerge/engine/tree/attrs.ex` strips both as runtime attributes and excludes
  them from attribute hashing. Adding public helper functions alone would not
  serialize or reliably diff them.
- `lib/emerge/ui/event.ex` and `lib/emerge/engine/event_registry.ex` have no
  `on_scroll` event. Add it through the existing incremental event-registration
  and viewport payload route, not a separate Solve integration.
- `Emerge.UI.key/1` requires globally unique keys. `VNode` retains the semantic
  key, but EMRG serialization transmits numeric IDs, not keys. Reconciliation's
  existing `old_key_index` describes the previous tree and cannot resolve a
  target newly inserted in the current update; collect new key-to-ID mappings
  during the existing reconciliation traversal.

### Native ownership and input paths

- `ElementTree::apply_scroll_axis` in `tree/element.rs` applies deltas to
  `NodeLayout.scroll_x/y`, clamps them, and marks render/registry refresh dirty.
- `runtime/tree_update.rs` sums `ScrollRequest` and scrollbar-thumb deltas per
  mount before applying them to the tree.
- `events/registry_builder.rs` produces tree scroll requests for wheel/trackpad,
  drag, keyboard, and focus reveal. Scrollbar drag uses separate thumb messages.
- `events/runtime.rs` owns gesture/inertia state but gets positions back through
  `ScrollbarNode`. It refreshes drag anchors and inertia edge checks from those
  snapshots. All of these paths need migration, not just wheel handling.
- `Element::normalize_extracted_state` directly copies effective scroll attrs into
  layout offsets. That must not remain a path for replaying app declarations.
- `tree/layout.rs` scales declared scroll attrs, clamps offsets when bounds shrink,
  anchors an end-position when bounds grow, and clears disabled axes. These are
  semantic position decisions currently made outside the event runtime.
- Directional listeners depend on current offset/range so nested scrolling can
  propagate when a child reaches an edge. Scroll position also affects retained
  render transforms, clipping, hit testing, hover, and focus reveal.
- Native event payloads currently support strings/floats. The map payload needs a
  fixed-shape position variant in the Rust event layer and BEAM encoding. Also
  update `src/bin/macos_host.rs` and the Elixir macOS notification protocol; that
  transport currently serializes event payloads as strings.

## Ownership and reconciliation

### Mount-scoped event state

Introduce scroll session state for live scrollable containers, including ones
with zero overflow and no visible thumb. Do not use the presence of a painted
scrollbar as the session lifetime. Track mounts through existing lifecycle data.

Session state includes current normalized fractions, enabled axes, current
bounds/scale, proposal watermarks, and expected echoes. Geometry, scrollbar
presentation, and input routing still come from accepted native rebuilds.
Resize retains live fractions, including across temporary zero range.

All position-changing actions go through one event-side transition:

1. Resolve input against current geometry and live positions.
2. Convert pixel movement to progress using the current scrollable range, clamp
   to `[0, 1]`, and update event-owned state. Zero-range axes cannot move and must
   never divide by zero.
3. Publish a mount-scoped absolute normalized position update when it changed;
   the tree projects that fraction into its current scroll range.
4. For runtime-originated changes, emit one final position callback per changed
   container per dispatch, if registered. Record only the payload being emitted
   in the echo ledger immediately before delivery.

Wheel components, drag, inertia, keyboard, scrollbar track/thumb, and focus reveal
must share this transition. Retain listener barriers and bounded replay; this is
not a separate asynchronous-input performance rewrite.

### App proposals

- Retain the last app declaration independently of the native position mirror.
  It is a tagged fraction or semantic target, not just a numeric offset.
- Assign increasing IDs at `SetAttrs` ingestion only when declarations change.
- Prefer per-axis proposals/watermarks and echo matching, so a partial-axis binding
  cannot rewind another axis. Axis removal also needs an explicit declaration
  transition so old cached proposals cannot regain control.
- Match echoes in the original emitted fraction units, before any projection
  through current layout bounds. An echoed fraction remains an echo even if the
  content extent, viewport size, or scale has changed since it was emitted.
- A matching echo preserves newer native movement, inertia, and gesture state.
  Retire matching prefixes conservatively under FIFO/coalesced app publication.
- A genuine app fraction replaces the affected axis once, with no callback.
  A key proposal resolves its target geometry and becomes a native position
  change once; it is not matched against fraction echoes just because the
  resulting position happens to equal one. Retain outstanding old echoes; they
  can still arrive afterward.
- Use the same five-second age limit and processed-ID protection as text input.
  Native rebuilds neither consume echoes nor renew their expiration.
- Compare/canonicalize numeric values deliberately. Callback payloads, stored echo
  values, and decoded declarations must round-trip exactly at non-integer scales;
  avoid a broad epsilon that hides intentional small position changes.

A genuine external jump must not immediately be undone by old inertia or a stale
thumb anchor. Cancel motion/captured scrolling for the affected
container/axis, retaining safe release handling so it cannot become a click.
Do not cancel another container's motion or cancel anything for an echo/no-op.

## Layout changes: fraction preservation

**Retain the live fraction when geometry changes.**

- 0.0 stays at the start, 1.0 stays at the end, and 0.5 remains halfway through the
  new available range. Preserve the live fraction, not the last app declaration;
  user scrolling can already have moved beyond that declaration.
- The tree projects the event-authored fraction against fresh geometry in the
  first resized frame. It does not overwrite that fraction or send it back as
  a position acknowledgment. No corrective write is needed for range changes.
- Geometry changes alone do not emit `on_scroll` when normalized progress is
  unchanged, even though the rendered pixel offset changes.
- At zero range, the rendered offset is zero and wheel/drag motion is a no-op.
  Retain the live fraction across temporary loss of overflow so an
  explicit 1.0 still means the end if content later grows. A new unspecified
  position starts at 0.0. Disabling an axis is a separate lifecycle decision
  (reset its live fraction to zero).
- Bounds/scale still need causal freshness for converting input deltas, thumb
  motion, and focus reveal. Keep existing native barriers; do not accept stale
  scrollbar offsets as live progress or round-trip progress through f32 pixels.

This preserves end anchoring but changes today's intermediate-position behavior:
when content grows, retaining 0.5 can move the visible content. The alternative is
retaining the existing pixel offset (with end anchoring), recalculating the fraction
and notifying the controller when the range changes. That alternative keeps
public attributes normalized too, but needs additional geometry-induced position
transitions in the event runtime. That alternative is not implemented.

Do not add smooth programmatic scrolling or multiple anchoring modes in this
first pass.

## Implementation sequence

1. Add failing full-pipeline tests for app positions, callbacks, and delayed echoes.
   Initialize through real upload/frame preparation, not the registry-only test
   shortcut that missed the first-key caret bug.
2. Add public helpers, declaration validation/serialization/hash participation, and
   `on_scroll` routing. Resolve the old pixel-tag compatibility explicitly and
   allocate/document fraction/target-ID position variants and event-presence
   encoding as needed. Add new-tree key resolution without serializing key terms.
   Keep boundary tests for the fixed-shape position payload on both in-process
   NIF and macOS host transports.
3. Add per-mount scroll session metadata and app proposals, including containers
   without overflow. Use normalized public units and define numeric
   canonicalization, geometry freshness, resize policy, and the separation between
   mirrored fractions and rendered pixel offsets explicitly.
4. Resolve key targets after valid layout, with range-clamped leading-edge
   alignment and mount-safe identity. Move every native scrolling source to the
   shared event transition and absolute tree writes. Remove parallel tree-owned delta paths from runtime use; preserve
   mount-safe batching and native receipts.
5. Migrate bounds normalization/end anchoring and gesture/inertia reconciliation.
   Remove repeated effective-attribute application as a position-setting path.
6. Document the controlled/uncontrolled contract and show ordinary Solve binding.
   Do not modify or commit `../emerge_demo` without a separate request.

Keep Elixir changes incremental: maps for lookup, lists for order, iodata for
encoding, no repeated list indexing or full-tree event-registry rebuilds. No new
NIF entrypoint/scheduler or Solve protocol should be necessary; this extends
existing tree serialization and native event delivery.

## Regression coverage and acceptance

- Single-axis and two-axis declarations; initial fractions; subsequent jumps;
  partial bindings, attribute removal/reintroduction, and no-handler scrolling.
  Verify 0.0/0.5/1.0 against the scrollable range, validation outside `[0, 1]`,
  zero-range behavior, and exact normalized callback/declaration round trips.
- Key targets on initial upload and same-batch insertion; direct/nested children;
  already-visible targets still aligning to the top/left; end-of-range clamping;
  padding/transforms; missing/outside targets; removal/remount; target motion;
  switching key-to-fraction and fraction-to-key; no repeated snap-back after user
  scrolling. Initial key proposals must not be mistaken for already-applied seeds.
- Actual native callback payloads through event registry/viewport/Solve-style
  routing. Coalesce both axes of a dispatch into a coherent final position.
- Native acknowledgments before/after app echoes, multiple native moves before
  FIFO echoes, coalesced renders, repeated numeric values, and expired history.
- Replay of processed proposals, style-only `SetAttrs`, cached animation snapshots,
  mount removal and same-ID remount, and non-echoing/transformed handlers.
- Wheel/trackpad, drag, inertia, arrow keys, thumb/track, and focus-on-mount reveal.
  App jumps during motion must not resume stale momentum or thumb anchors.
- Nested containers, edge propagation, diagonal input split, and clipping/hit
  targets after scrolling. No stale directional listener may swallow propagation.
- Bounds shrink/growth under the selected resize policy, end anchoring,
  zero-range collapse/re-expansion, disabled/re-enabled axes, content changes,
  layout animation, transforms, and non-integer/global/nested scales.
- Retained model, rendered offset/clip/thumb, and event state agree after settling;
  the first native input/app proposal updates the visible position correctly.
- Duplicate snapshots emit zero corrective writes/callbacks. Real app proposals
  publish once. Under fraction preservation, geometry-only projection changes
  neither republish progress nor emit scroll callbacks. No feedback loop.
- Inject timestamps for timeout boundaries; do not use sleeps for correctness.

Run `cargo test`, `mix test`, and `./ci-tests.sh quality` after implementation.
The inherited baseline has an unrelated BGRA8888 failure with the locked
`video_interop` 0.1.2; report it separately rather than silently changing dependencies.

## Limits

API direction: `scroll_x/1` and `scroll_y/1` accept a float fraction or child key;
`Event.on_scroll/1` reports `%{x, y}` fractions in `[0.0, 1.0]`. No pixel/ratio
wrapper variants. Key targets request leading-edge alignment within valid range.
Resize retains live fractions; missing targets are one-shot no-ops and targets
cannot cross independent same-axis scrollports or nearby mounts.

As with text input, a reset equal to an outstanding unexpired echo is ambiguous,
and first-seen responses beyond five seconds may be accepted as app replacements.
Arbitrarily delayed or out-of-order asynchronous controller results require an
end-to-end causal protocol outside this initial scope.

## Implementation record

- Public helpers and validation: `lib/emerge/ui/scroll.ex`, `event.ex`, and
  `internal/validation.ex`. Fraction attrs participate in normal hashing/diffing;
  key declarations use `Emerge.Engine.ScrollTarget` with a stable declaration token
  and current numeric target ID. Reconciliation collects new descendant keys in
  its existing traversal, without a separate whole-tree scan per target.
- Wire tags `85`/`86` encode tagged fraction/target declarations; `87` enables the
  callback. Legacy pixel tags `38`/`39` are not reinterpreted. macOS event kind `19`
  decodes to the same fraction map as the in-process Rustler `NifMap` payload.
- `native/emerge_skia/src/events/scroll.rs` owns sessions, per-axis watermarks and
  callback-only echoes. The driver's `runtime/scroll.rs` translates all relative
  runtime scroll requests into absolute, mount-qualified `SetScrollPosition`
  messages and coalesces callbacks at the dispatch boundary. Existing barriers
  remain intact. Registry-only channel payloads are boxed to keep input envelopes
  small after adding scroll metadata; surrounding test edits are mechanical.
- Tree offsets project event-authored fractions after layout. Retained registry
  metadata includes scroll sessions even without overflow/interactive geometry.
  Live event offsets refresh thumb/inertia anchors rather than trusting receipts.
- New regressions live in `events/scroll/tests.rs`,
  `events/runtime/tests/delayed/scroll.rs`, and
  `test/emerge/controlled_scroll_test.exs`. They cover full binary upload/frame
  preparation, native mirrors, callback coalescing, both echo/receipt orders,
  replacement/remount, one-shot keys, key resolution/token stability, missing and
  nested targets, expiry, no-handler operation, thumb motion, affected-axis
  momentum cancellation, resize/zero range, disabling/re-enabling, codec validation
  and macOS payload validation. Existing viewport routing coverage now forwards
  a structured scroll payload too.

Validation:

- `cargo test`: **1,523 unit + 14 integration = 1,537 passed**.
- `./ci-tests.sh quality`: passed (format, warning-free compilation, strict Credo,
  Clippy). `cargo clippy --all-targets -- -D warnings`: passed.
- `mix docs`: passed; public API and wire/event documentation generated cleanly.
- `mix test --include full_sweep`: **601/602 passed**, four hardware exclusions.
  The sole failure remains `test/emerge_skia_test.exs:635`:
  locked `video_interop` 0.1.2 rejects `:bgra8888`. No dependency change was made.
- Logs: `/tmp/scroll-cargo-final.log`, `/tmp/scroll-mix-final.log`,
  `/tmp/scroll-quality-final.log`, `/tmp/scroll-clippy-final.log`.
- The `../emerge_demo` scrolling showcase now binds a continuous slider and a
  scrollable list to one Solve controller, with a live percentage display.
  `mix test --warnings-as-errors` in the demo: **105 passed**.
- No Dialyzer, actual macOS host execution, or on-device validation claim.
  The demo's pre-existing local dependency edit was left untouched and uncommitted.

## Commit sequence

1. Box registry messages and migrate their consumers independently of scrolling.
2. Add event-owned native scrolling, wire payloads, and native regressions.
3. Expose the public Elixir API, key resolution, and controller-routing tests.
4. Document the contract, protocol, design decisions, and validation.
5. Commit the shared-controller showcase example and tests in `../emerge_demo`,
   excluding its environment-specific dependency-path override.
