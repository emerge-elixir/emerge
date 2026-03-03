# Event Registry V2 Refactor Plan

## Status
Draft for approval.

Approved. Phase A in progress.

## Progress Update

### 2026-02-27 - Phase A Chunk 1 completed (Foundation + Parallel Build)

#### Completed
- Added V2 registry foundation module:
  - `native/emerge_skia/src/events/registry_v2.rs`
- Added core V2 scaffolding types:
  - `TriggerId`, `DispatchRuleId`, `DispatchRule`, `DispatchRulePredicate`, `DispatchRuleAction`
  - `DispatchJob`, `RuleScope`, `PriorityKey`
  - `TriggerBucket`, `PointerIndexPhase1`, `EventRegistryV2`
- Implemented adapter build path:
  - `EventRegistryV2::from_event_nodes(&[EventNode])`
- Implemented Phase A indexes in V2:
  - dense node metadata + `id_to_idx`
  - `focus_order` + `focus_pos`
  - `first_visible_scrollable_by_dir`
  - pointer candidate prefilter lists (`pointer_index.candidates_by_trigger`)
- Wired V2 into existing processor as parallel data (no dispatch switch yet):
  - `EventProcessor` now stores and rebuilds `registry_v2` alongside existing registry
- Added Phase A parity/scaffold tests in `native/emerge_skia/src/events.rs`:
  - `test_registry_v2_focus_order_matches_current_registry_order`
  - `test_registry_v2_first_visible_scrollable_matches_existing_logic`
  - `test_registry_v2_pointer_candidates_keep_topmost_first_order`
  - `test_rebuild_registry_populates_v2_indexes`

#### Validation
- `cargo test` passed (275 tests)
- `mix test` passed (158 tests + 4 doctests)

#### Not Changed (Intentional)
- No behavior switch to V2 dispatcher yet
- No removal of legacy `detect_*` / `handle_*` event routing paths
- No TreeMsg contract changes

#### Follow-up (Completed in Chunk 2)
- Populate `dispatch_rules` / `buckets` with initial real rule entries
- Add parity harness comparing current routing decisions vs V2 query results for selected triggers
- Add lightweight V2 debug/introspection helper for rule/index inspection

### 2026-02-27 - Phase A Chunk 2 completed (Keyboard/Focus Rules + Parity Harness)

#### Completed
- Populated initial V2 dispatch rule buckets for keyboard/focus-only triggers in
  `native/emerge_skia/src/events/registry_v2.rs`:
  - `KeyLeftPress`, `KeyRightPress`, `KeyUpPress`, `KeyDownPress`
  - `KeyTabPress` (forward and reverse)
  - `KeyEnterPress` (focused pressable)
- Added rule population pass to V2 build:
  - `populate_keyboard_focus_rules(event_nodes)`
- Added focused-target directional rules from `EventNode.key_scroll_targets`.
- Added ordered fallback directional rules from
  `first_visible_scrollable_by_dir`.
- Added tab focus-change rules:
  - targeted rules per focused node
  - no-focus ordered rules
  - multi-action tab rules include focus reveal scroll requests when present
- Added V2 preview resolver APIs (test/debug path, not runtime dispatch path):
  - `resolve_winner_for_job(&DispatchJob)`
  - `resolve_actions_for_job(&DispatchJob)`
- Added V2 debug/introspection helper:
  - `debug_bucket_sizes(trigger)`
- Added parity tests in `native/emerge_skia/src/events.rs`:
  - no-focus arrow parity
  - focused directional matcher parity
  - focused fallback parity
  - tab forward/reverse parity
  - enter press parity (+ modifier block parity)
  - bucket population scaffold checks

#### Validation
- `cargo test` passed (281 tests)
- `mix test` passed (158 tests + 4 doctests)

#### Not Changed (Intentional)
- No behavior switch to V2 dispatcher yet
- Runtime event actor still uses legacy dispatch selection path
- No TreeMsg contract changes

#### Remaining in Phase A
- Expand parity harness coverage for additional trigger families
  (pointer/hover/scrollbar/text command/preedit)
- Add richer V2 debug dump utilities for rule content and ordering
- Document exact priority ordering keys used during rule insertion

### 2026-02-27 - Phase A Chunk 3 completed (Shadow A/B Harness Integration)

#### Completed Scope
- Keep V1 and V2 in parallel.
- V1 remains authoritative for side effects.
- V2 runs in shadow mode and predicts outcomes for comparison.
- Initial compare coverage: keyboard/focus slice only (`Tab`, `Enter`, arrows).

#### Completed Runtime Control
- Add dispatch engine mode:
  - `V1Only`
  - `ShadowAB`
  - `V2Only` (reserved for later cutover)
- Add compare mask categories (enabled initially):
  - `FOCUS_CHANGE`
  - `ELEMENT_EVENTS`
  - `SCROLL_REQUESTS`

#### Completed Canonical Outcome Schema
```rust
struct DispatchOutcome {
    focus_change: Option<Option<NodeKey>>,
    element_events: Vec<ElementEventOut>,
    scroll_requests: Vec<ScrollRequestOut>,

    // reserved for later parity families:
    text_cursor: Vec<String>,
    text_commands: Vec<String>,
    text_edits: Vec<String>,
    hover: Vec<String>,
    style_runtime: Vec<String>,
    scrollbar: Vec<String>,
}
```

```rust
struct ElementEventOut {
    target: NodeKey,
    kind: ElementEventKind,
    payload: Option<String>,
}

struct ScrollRequestOut {
    target: NodeKey,
    dx: Milli,
    dy: Milli,
}
```

#### Completed Shadow State and Comparator
- Add `ShadowABState` with:
  - config (`mode`, `compare_mask`, `max_samples`, `verbose_mismatch`)
  - counters (`total`, per-trigger compared/matched/mismatched)
  - bounded mismatch samples (`VecDeque<MismatchSample>`)
- Compare V1 observed outcome vs V2 predicted outcome per processed event.
- Normalize float deltas (`dx`, `dy`) to milli-units before comparison.

#### Implemented Runtime Call-Site Replacement Map
- Add tracked wrappers (send + capture) and use them in `events/runtime.rs`:
  - `send_element_event_tracked(...)`
  - `send_element_event_with_string_payload_tracked(...)`
  - `send_scroll_request_tracked(...)`
- Update helper signatures to thread shadow frame capture:
  - `emit_change_event(..., frame: &mut Option<ShadowEventFrame>)`
  - `apply_focus_change(..., frame: &mut Option<ShadowEventFrame>)`
- Replace direct send calls for these paths only:
  - click / press
  - mouse_down / mouse_up
  - mouse_enter / mouse_leave / mouse_move
  - focus / blur / change
  - all `TreeMsg::ScrollRequest` sends
- Keep all other tree message sends unchanged in this chunk.

#### Implemented Integration Sequence (Per Coalesced Event)
1. Begin frame: `begin_shadow_frame(...)`
2. Attach V2 prediction for keyboard/focus triggers: `attach_v2_prediction(...)`
3. Run existing V1 logic with tracked wrappers
4. End frame compare + stats update: `end_shadow_frame(...)`

#### Implemented `process_input_events` Integration Reference
```diff
diff --git a/native/emerge_skia/src/events/runtime.rs b/native/emerge_skia/src/events/runtime.rs
@@
 fn emit_change_event(
     target: &Option<LocalPid>,
     element_id: &ElementId,
-    value: &str,
+    value: &str,
+    frame: &mut Option<ShadowEventFrame>,
 ) {
     if let Some(pid) = target.as_ref() {
-        send_element_event_with_string_payload(*pid, element_id, change_atom(), value);
+        send_element_event_with_string_payload_tracked(
+            Some(*pid),
+            element_id,
+            change_atom(),
+            value,
+            ElementEventKind::Change,
+            frame,
+        );
     }
 }

 fn apply_focus_change(
     next_focus: Option<ElementId>,
     focused: &mut Option<ElementId>,
@@
     sessions: &mut HashMap<ElementId, TextInputSession>,
     tree_tx: &Sender<TreeMsg>,
     log_render: bool,
+    frame: &mut Option<ShadowEventFrame>,
 ) {
@@
     if previous_focus == next_focus {
         return;
     }
+    capture_v1_focus_change(frame, next_focus.as_ref());
@@
-        if let Some(pid) = target.as_ref() {
-            send_element_event(*pid, &prev_id, blur_atom());
-        }
+        send_element_event_tracked(
+            target.as_ref().copied(),
+            &prev_id,
+            blur_atom(),
+            ElementEventKind::Blur,
+            None,
+            frame,
+        );
@@
-        if let Some(pid) = target.as_ref() {
-            send_element_event(*pid, &next_id, focus_atom());
-        }
+        send_element_event_tracked(
+            target.as_ref().copied(),
+            &next_id,
+            focus_atom(),
+            ElementEventKind::Focus,
+            None,
+            frame,
+        );
 }

 fn process_input_events(
     events: &mut Vec<InputEvent>,
     processor: &mut EventProcessor,
@@
     sessions: &mut HashMap<ElementId, TextInputSession>,
     focused: &mut Option<ElementId>,
     clipboard: &mut ClipboardManager,
+    shadow_state: &mut ShadowABState,
 ) {
@@
     for event in coalesced {
+        let trigger = processor.v2_trigger_for_input_event(&event);
+        let mut frame = begin_shadow_frame(shadow_state, &event, trigger);
+
+        if let Some(predicted) =
+            processor.preview_v2_keyboard_focus_outcome(&event, focused.as_ref())
+        {
+            attach_v2_prediction(&mut frame, predicted);
+        }
@@
         if let Some(pid) = target.as_ref() {
@@
             if let Some(clicked_id) = processor.detect_click(&event) {
-                send_element_event(pid, &clicked_id, super::click_atom());
+                send_element_event_tracked(
+                    Some(pid),
+                    &clicked_id,
+                    super::click_atom(),
+                    ElementEventKind::Click,
+                    None,
+                    &mut frame,
+                );
             }

             if let Some(pressed_id) = processor.detect_press(&event) {
-                send_element_event(pid, &pressed_id, press_atom());
+                send_element_event_tracked(
+                    Some(pid),
+                    &pressed_id,
+                    press_atom(),
+                    ElementEventKind::Press,
+                    None,
+                    &mut frame,
+                );
             }

             if let Some((mouse_id, mouse_event)) = processor.detect_mouse_button_event(&event) {
-                send_element_event(pid, &mouse_id, mouse_event);
+                send_element_event_tracked(
+                    Some(pid),
+                    &mouse_id,
+                    mouse_event,
+                    map_atom_to_kind(mouse_event),
+                    None,
+                    &mut frame,
+                );
             }

             for (hover_id, hover_event) in processor.handle_hover_event(&event) {
-                send_element_event(pid, &hover_id, hover_event);
+                send_element_event_tracked(
+                    Some(pid),
+                    &hover_id,
+                    hover_event,
+                    map_atom_to_kind(hover_event),
+                    None,
+                    &mut frame,
+                );
             }
         }

         if let Some(next_focus) = processor.text_input_focus_request(&event) {
-            apply_focus_change(next_focus, focused, target, sessions, tree_tx, log_render);
+            apply_focus_change(
+                next_focus,
+                focused,
+                target,
+                sessions,
+                tree_tx,
+                log_render,
+                &mut frame,
+            );

             if let Some(focused_id) = focused.as_ref() {
                 for (id, dx, dy) in processor.focus_reveal_scroll_requests(focused_id) {
-                    send_tree(
-                        tree_tx,
-                        TreeMsg::ScrollRequest { element_id: id, dx, dy },
-                        log_render,
-                    );
+                    send_scroll_request_tracked(tree_tx, log_render, id, dx, dy, &mut frame);
                 }
             }
         }
@@
-                        emit_change_event(target, &element_id, &next_content);
+                        emit_change_event(target, &element_id, &next_content, &mut frame);
@@
-                            emit_change_event(target, &element_id, &next_content);
+                            emit_change_event(target, &element_id, &next_content, &mut frame);
@@
-                            emit_change_event(target, &element_id, &next_content);
+                            emit_change_event(target, &element_id, &next_content, &mut frame);
@@
-                        emit_change_event(target, &element_id, &next_content);
+                        emit_change_event(target, &element_id, &next_content, &mut frame);
@@
-                        emit_change_event(target, &element_id, &next_content);
+                        emit_change_event(target, &element_id, &next_content, &mut frame);
@@
-                        emit_change_event(target, &element_id, &next_content);
+                        emit_change_event(target, &element_id, &next_content, &mut frame);

         for (id, dx, dy) in processor.scroll_requests(&event) {
-            send_tree(
-                tree_tx,
-                TreeMsg::ScrollRequest { element_id: id, dx, dy },
-                log_render,
-            );
+            send_scroll_request_tracked(tree_tx, log_render, id, dx, dy, &mut frame);
         }

+        end_shadow_frame(shadow_state, frame);
     }
 }
```

```diff
diff --git a/native/emerge_skia/src/events/runtime.rs b/native/emerge_skia/src/events/runtime.rs
@@
 fn spawn_event_actor(...) {
     ...
+    let mut shadow_state = ShadowABState::new(ShadowABConfig {
+        mode: DispatchEngineMode::ShadowAB,
+        compare_mask: CMP_FOCUS_CHANGE | CMP_ELEMENT_EVENTS | CMP_SCROLL_REQUESTS,
+        max_samples: 200,
+        verbose_mismatch: false,
+    });
@@
     process_input_events(..., &mut clipboard);
+    process_input_events(..., &mut clipboard, &mut shadow_state);
@@
-    apply_focus_change(next_focus, ..., log_render);
+    let mut frame = None;
+    apply_focus_change(next_focus, ..., log_render, &mut frame);
 }
```

#### Validation
- `cargo test` passed (281 tests)
- `mix test` passed (158 tests + 4 doctests)

#### Remaining after Chunk 3
- Expand shadow parity coverage to additional trigger families
  (pointer/hover/scrollbar/text command/preedit)
- Add richer mismatch reporting and debug dump tooling
- Add CI-level A/B validation gate before any `V2Only` cutover

### 2026-02-27 - Phase A Chunk 4 completed (Request-Level Text/IME Parity)

#### Completed
- Promoted shadow parity model from placeholder strings to typed request-level
  outputs in `native/emerge_skia/src/events/shadow_ab.rs`:
  - `TextCommandReqOut`
  - `TextEditReqOut`
  - `TextPreeditReqOut`
- Added request-level capture helpers for V1 observed behavior:
  - `capture_v1_text_command_request(...)`
  - `capture_v1_text_edit_request(...)`
  - `capture_v1_text_preedit_request(...)`
- Extended outcome comparator with request-level categories:
  - `CMP_TEXT_COMMANDS`
  - `CMP_TEXT_EDITS`
  - `CMP_TEXT_PREEDIT`
- Threaded V1 request capture in runtime at request emission points in
  `native/emerge_skia/src/events/runtime.rs`:
  - `text_input_command_request`
  - `text_input_edit_request`
  - `text_input_preedit_request`
- Expanded event trigger mapping for shadow prediction in
  `native/emerge_skia/src/events.rs`:
  - `home`, `end`, `backspace`, `delete`
  - `TextCommit`, `TextPreedit`, `TextPreeditClear`
- Added/kept V2 text action scaffolding in
  `native/emerge_skia/src/events/registry_v2.rs`:
  - `DispatchRuleAction::TextCommand`
  - `DispatchRuleAction::TextEdit`
  - `DispatchRuleAction::TextPreedit`
- Added request-level shadow parity tests in `native/emerge_skia/src/events.rs`:
  - `test_shadow_preview_text_command_request_parity`
  - `test_shadow_preview_text_edit_request_parity`
  - `test_shadow_preview_text_preedit_request_parity`

#### Validation
- `cargo test` passed (284 tests)
- `mix test` passed (158 tests + 4 doctests)

#### Not Changed (Intentional)
- V1 remains authoritative; V2 stays in shadow prediction mode.
- No runtime cutover to `V2Only`.

#### Remaining after Chunk 4
- Expand shadow parity coverage for pointer/hover/scrollbar families.
- Improve mismatch reporting with richer per-trigger debug dump tooling.
- Add CI A/B validation gate before enabling `V2Only` mode.

#### Acceptance Criteria (met for this chunk)
- No behavior change in runtime dispatch.
- Shadow statistics/mismatch samples populated for keyboard/focus/text triggers.
- Full test suites pass:
  - `cargo test`
  - `mix test`

### 2026-02-27 - Phase A Chunk 5 completed (Pointer/Scrollbar/Style Sequence Parity + Better Mismatch Reasons)

#### Completed
- Extended shadow preview parity coverage with sequence-level tests in
  `native/emerge_skia/src/events.rs`:
  - `test_shadow_preview_scrollbar_thumb_drag_parity_sequence`
  - `test_shadow_preview_style_runtime_mouse_down_parity_sequence`
  - `test_shadow_preview_pointer_element_events_parity_sequence`
- Added a reusable V1 outcome harness in tests:
  - `v1_shadow_outcome_for_event(...)`
  - mirrors runtime ordering so preview parity is validated against post-event state transitions.
- Kept existing single-event parity tests for text/IME, scrollbar hover, and style mouse-over.
- Improved shadow mismatch diagnostics in `native/emerge_skia/src/events/shadow_ab.rs`:
  - compare reasons now include lengths and first differing index/value for vector categories.
  - focus-change mismatch reason now includes concrete `v1`/`v2` values.
- Removed atom-equality mapping from preview/runtime parity capture paths:
  - `detect_mouse_button_event(...)` now returns `(ElementId, ElementEventKind)`.
  - `handle_hover_event(...)` now returns `(ElementId, ElementEventKind)` entries.
  - runtime converts `ElementEventKind` to atom only at send sites.

#### Validation
- `cargo test` passed (289 tests)
- `mix test` passed (158 tests + 4 doctests)

#### Not Changed (Intentional)
- V1 remains authoritative for side effects.
- No runtime cutover to `V2Only`.
- No TreeMsg contract changes.

#### Remaining after Chunk 5
- Add per-trigger shadow mismatch summary export/debug command path for runtime introspection.
- Add CI-level A/B parity gate before any `V2Only` enablement.

### 2026-02-27 - Phase A Chunk 6 completed (Shutdown Shadow Dump Path)

#### Completed
- Added a lightweight shadow stats dump formatter in
  `native/emerge_skia/src/events/shadow_ab.rs`:
  - `format_shadow_stats_dump(&ShadowABState, sample_limit)`
  - includes totals, mismatch rate, non-zero per-trigger counters, and recent mismatch samples.
- Added runtime shutdown dump path in
  `native/emerge_skia/src/events/runtime.rs`:
  - opt-in env flag: `EMERGE_SKIA_SHADOW_DUMP`
  - sample limit env: `EMERGE_SKIA_SHADOW_DUMP_SAMPLES` (default `5`)
  - dump emitted on `EventMsg::Stop`
  - dump also emitted when the event channel closes unexpectedly.
- Added Wayland close-path stop forwarding in
  `native/emerge_skia/src/backend/wayland.rs` so event actor receives
  `EventMsg::Stop` when the window closes, which triggers the shutdown dump
  even if Elixir code does not explicitly call `EmergeSkia.stop/1`.
- Added unit tests for dump formatting in
  `native/emerge_skia/src/events/shadow_ab.rs`:
  - summary/trigger/sample coverage
  - sample limit behavior (shows most recent only)

#### Validation
- `cargo test` passed
- `mix test` passed

#### Not Changed (Intentional)
- No dispatch behavior changes.
- No TreeMsg or NIF contract changes.
- V1 remains authoritative; V2 stays shadow compare.

### 2026-02-27 - Phase A Chunk 7 completed (Shadow Mismatch Reduction for Focus/Enter)

#### Completed
- Reduced high-volume shadow mismatches seen in shutdown dump (`KeyTabPress`, `KeyEnterPress`, focus-loss events) by improving preview parity in
  `native/emerge_skia/src/events.rs`:
  - avoid duplicate enter `press` prediction when V2 already emits enter press action.
  - emit synthetic `blur`/`focus` element events when predicted focus changes.
  - include blur prediction for window focus-loss path (`InputEvent::Focused { focused: false }`).
- Added focused parity tests in `native/emerge_skia/src/events.rs`:
  - `test_shadow_preview_enter_press_does_not_duplicate_press_event`
  - `test_shadow_preview_tab_emits_focus_transition_events`
  - `test_shadow_preview_window_focus_lost_emits_blur_event`

#### Validation
- `cargo test` passed
- `mix test` passed

#### Remaining after Chunk 7
- Remaining large mismatch family in sample output is `TextCommit` element-event parity (change-event prediction), which still needs dedicated preview parity work.

### 2026-02-27 - Phase A Chunk 8 completed (Text Change-Event Preview Parity)

#### Completed
- Implemented preview-side text mutation simulation in
  `native/emerge_skia/src/events.rs` for edit requests:
  - `Insert`
  - `Backspace`
  - `Delete`
- Added preview helpers that mirror runtime mutation semantics for char-indexed edits:
  - `preview_text_char_len`
  - `preview_char_to_byte_index`
  - `preview_selected_range`
  - `preview_next_content_for_edit`
- Extended `preview_v2_keyboard_focus_outcome(...)` so text edits that mutate content
  now emit predicted `ElementEventKind::Change` with the new content payload,
  matching V1 capture behavior.
- Added targeted tests in `native/emerge_skia/src/events.rs`:
  - `test_shadow_preview_text_commit_emits_change_event_payload`
  - `test_shadow_preview_backspace_emits_change_event_payload`
  - `test_shadow_preview_backspace_at_start_emits_no_change_event`
  - `test_shadow_preview_backspace_with_selection_emits_change_event_payload`

#### Validation
- `cargo test` passed
- `mix test` passed

#### Remaining after Chunk 8
- Command-driven text mutations (`Cut`/`Paste`/`PastePrimary`) still need
  dedicated change-event payload parity in preview (clipboard-dependent path).

### 2026-02-27 - Phase A Chunk 9 completed (Residual Mismatch Cleanup)

#### Completed
- Fixed mouse button payload parity in `native/emerge_skia/src/events.rs`:
  - preview now emits `MouseDown`/`MouseUp` with `payload: None` (matching V1 capture).
  - test harness `v1_shadow_outcome_for_event(...)` updated to same payload policy.
- Added command-path change-event prediction enrichment in
  `native/emerge_skia/src/events/runtime.rs` using live runtime state:
  - `enrich_predicted_command_change_events(...)`
  - predicts `ElementEventKind::Change` payloads for:
    - `Cut`
    - `Paste`
    - `PastePrimary`
  - uses cloned `TextInputSession` + existing mutation helpers
    (`cut_selection`, `replace_selection_or_insert`, `sanitize_single_line_text`)
    and current clipboard contents.
- Added regression test in `native/emerge_skia/src/events.rs`:
  - `test_shadow_preview_mouse_button_payload_is_none`

#### Validation
- `cargo test` passed
- `mix test` passed

#### Remaining after Chunk 9
- Re-run shutdown shadow dump against demo usage to confirm residual mismatch count
  converges toward zero for this scenario.

### 2026-02-27 - Phase A Chunk 10 completed (Keyboard Scroll Prediction De-dup)

#### Completed
- Added preview-side scroll request de-dup in
  `native/emerge_skia/src/events.rs` via
  `push_unique_scroll_request(...)`.
- Routed all preview scroll insertion paths through the de-dup helper:
  - V2 `DispatchRuleAction::ScrollRequest` actions
  - focus-reveal fallback scrolls
  - legacy `scroll_requests(...)` fallback
- This prevents duplicate predicted scroll entries when both V2 and
  legacy fallback contribute the same scroll request during shadow preview.
- Added targeted regression tests in `native/emerge_skia/src/events.rs`:
  - `test_shadow_preview_key_scroll_requests_are_deduped`
  - `test_shadow_preview_tab_focus_reveal_scroll_is_deduped`

#### Validation
- `cargo test` passed
- `mix test` passed

#### Remaining after Chunk 10
- Re-run shutdown shadow dump to verify keyboard scroll mismatch families
  (`KeyUpPress`, `KeyDownPress`, `KeyRightPress`, residual `KeyTabPress`) are
  reduced/eliminated under real demo interaction sequences.

### 2026-02-27 - Phase A Chunk 11 completed (TextCommit Mismatch Diagnostics)

#### Completed
- Added diagnosis-focused TextCommit instrumentation in
  `native/emerge_skia/src/events/runtime.rs`:
  - captures per-commit trace record (event sequence, commit text/mods, focused element,
    descriptor/session before-state, session after-state, predicted/emitted change payloads).
  - compares shadow mismatch samples against trace and classifies mismatch as:
    - `descriptor_stale`
    - `session_or_ordering`
- Added bounded runtime ring buffer for commit diagnostics:
  - env `EMERGE_SKIA_SHADOW_TEXT_DIAG_RING` (default `64`)
- Added shutdown diagnostics dump for commit ring:
  - printed alongside shadow stats when diagnostics are enabled.
- Added processor API to snapshot descriptor state for diagnostics:
  - `EventProcessor::text_input_descriptor_snapshot(...)` in
    `native/emerge_skia/src/events.rs`.
- Added commit-source logging in Wayland backend (`keyboard` vs `ime`) behind
  diagnostics env flag:
  - `native/emerge_skia/src/backend/wayland.rs`

#### Runtime Knobs
- `EMERGE_SKIA_SHADOW_TEXT_DIAG=1`
  - enables TextCommit mismatch diagnostics and source logging.
- `EMERGE_SKIA_SHADOW_TEXT_DIAG_RING=<n>`
  - sets commit trace ring capacity.
- `EMERGE_SKIA_SHADOW_DUMP=1`
  - also enables TextCommit mismatch diagnostics in runtime (without backend source logs).

#### Validation
- `cargo test` passed
- `mix test` passed

#### Remaining after Chunk 11
- Run demo with diagnostics enabled to classify remaining TextCommit mismatches.
- If most are `descriptor_stale`, consider migrating TextCommit change prediction
  to session-based runtime enrichment.
- If classification trends `session_or_ordering`, fix V1 mutation/caret ordering
  behavior first.

### 2026-02-27 - Phase A Chunk 12 completed (Focused Text Runtime Wins During Reconcile)

#### Completed
- Implemented focused-session authority in
  `native/emerge_skia/src/events/runtime.rs` `reconcile_text_input_sessions(...)`:
  - when descriptor content differs for the focused text input, runtime no longer
    overwrites session content from descriptor.
  - instead it pushes a corrective `TreeMsg::SetTextInputContent` from session
    back to tree to converge descriptor state.
- Preserved focused editing runtime state in stale-descriptor case:
  - no selection/preedit clearing on focused stale mismatch.
  - synchronized session descriptor runtime fields (`content/content_len/cursor/selection_anchor`)
    from active session so caret calculations remain consistent.
- Kept existing unfocused behavior:
  - descriptor content continues to sync into session for unfocused inputs.
- Added runtime regression tests in
  `native/emerge_skia/src/events/runtime.rs`:
  - `reconcile_keeps_focused_session_when_descriptor_is_stale`
  - `reconcile_applies_descriptor_content_when_unfocused`

#### Validation
- `cargo test` passed
- `mix test` passed

#### Remaining after Chunk 12
- Re-run demo with fast typing to verify caret insertion glitch is gone.
- If shadow mismatch remains mostly `descriptor_stale`, optionally add
  session-based TextCommit prediction override to eliminate residual parity noise.

### 2026-02-27 - Phase A Chunk 13 completed (Session-Based Edit Prediction Override)

#### Completed
- Added runtime-side edit prediction override in
  `native/emerge_skia/src/events/runtime.rs`:
  - `enrich_predicted_edit_change_events(...)`
  - simulates `Insert`/`Backspace`/`Delete` on cloned live `TextInputSession`.
- Added predicted change event normalization helpers:
  - `upsert_predicted_change_event(...)` (replace existing `Change` for element)
  - `remove_predicted_change_event(...)` (remove stale/no-op predicted change)
- Wired edit override into preview assembly before `attach_v2_prediction(...)`:
  - command enrichment remains in place
  - edit enrichment now runs next
  - diagnostics `predicted_change` capture now reads final enriched prediction.
- Added runtime regression tests in
  `native/emerge_skia/src/events/runtime.rs`:
  - `enrich_predicted_edit_change_events_uses_session_state_for_text_commit`
  - `enrich_predicted_edit_change_events_removes_noop_backspace_change`
  - `enrich_predicted_edit_change_events_removes_change_when_not_allowed`

#### Validation
- `cargo test` passed
- `mix test` passed

#### Remaining after Chunk 13
- Re-run demo shadow dump to verify `TextCommit` residual `descriptor_stale`
  mismatches converge toward zero with session-based edit override.

### 2026-02-27 - Phase A Chunk 14 completed (Shared V2 Keyboard/Focus Job Runner)

#### Completed
- Refactored keyboard/focus V2 preview assembly in
  `native/emerge_skia/src/events.rs` into reusable job/action helpers:
  - `apply_v2_keyboard_focus_job_for_event(...)`
  - `apply_v2_keyboard_focus_actions(...)`
  - action-level helpers for focus change, scroll request, and event emission.
- Replaced ad-hoc inline V2 action loop in
  `preview_v2_keyboard_focus_outcome(...)` with shared V2 runner path.
- Preserved existing behavior contracts:
  - V1 remains authoritative for runtime side effects.
  - preview keeps fallback paths and parity-oriented guards (including enter press suppression).
  - no tree message contract changes.

#### Validation
- `cargo test` passed
- `mix test` passed

#### Remaining after Chunk 14
- Re-run demo shadow dump to confirm parity remains at/near zero with refactored
  V2 runner path.
- Consider next step: env-gated selective `V2Only` execution for keyboard/focus
  (with immediate fallback to V1 on mismatch) or CI parity gate.

### 2026-02-27 - Phase A Chunk 15 completed (Strict V2 Mode Switch, No Fallback)

#### Completed
- Added startup dispatch mode option plumbing (`v1 | shadow | v2`):
  - `lib/emerge_skia.ex` (`dispatch_mode` option, default `:shadow`)
  - `lib/emerge_skia/native.ex` NIF map spec
  - `native/emerge_skia/src/lib.rs` (`StartOptsNif`, `StartConfig`, event actor spawn args)
- Added strict runtime V2 mode in `native/emerge_skia/src/events/runtime.rs`:
  - `DispatchEngineMode::V2Only` executes predicted V2 outcome as authoritative effects.
  - no V1 side-effect fallback path is executed in V2 mode.
  - missing prediction for dispatch-candidate event is handled as no-op + counter/log.
- Added V2-only no-prediction accounting and shutdown dump:
  - `v2-only no-prediction ...`
  - optional verbose per-event logging via `EMERGE_SKIA_V2_NO_PRED_VERBOSE=1`
- Added dispatch mode parsing with env override in runtime:
  - config-provided mode from startup opts
  - optional `EMERGE_SKIA_DISPATCH_MODE` override for manual testing.
- Added demo CLI switch for manual mode testing:
  - `demo.exs --dispatch-mode <v1|shadow|v2>`
- Fixed V2-only no-prediction internal state advancement:
  - even when a dispatch candidate has no predicted outcome, internal hover/press/drag
    state now advances so subsequent events (for example leave-only hover handlers)
    can still produce correct outcomes.

#### Validation
- `cargo test` passed
- `mix test` passed

#### Remaining after Chunk 15
- Run manual demo in `dispatch_mode=v2` and capture no-prediction counters.
- Start deleting V1 dispatch path pieces once V2 no-prediction coverage is acceptable.

### 2026-02-27 - Phase A Chunk 16 completed (V2-only API + Runtime Path)

#### Completed
- Removed public dispatch mode selection from Elixir API:
  - `EmergeSkia.start/1` no longer accepts `dispatch_mode`
  - passing `dispatch_mode` now raises an explicit `ArgumentError`
  - `demo.exs` now runs with fixed `dispatch_mode=v2` messaging
- Removed startup dispatch mode wiring from NIF config:
  - `StartOptsNif` / `StartConfig` no longer carry `dispatch_mode`
  - event actor startup no longer accepts dispatch mode input
- Event actor now runs V2 mode by default and keeps strict no-fallback semantics.
- Removed legacy V1 execution branch from `process_input_events(...)`; runtime now
  applies V2 outcomes (or no-op with accounting when no prediction is available).
- Added/updated V2-only regression coverage for no-prediction hover progression:
  - leave-only hover targets now still emit `mouse_leave` correctly after internal
    state advancement in V2-only no-prediction path.
- Added test migration tracking document:
  - `guides/internals/v2-test-migration-matrix.md`

#### Validation
- `cargo test` passed
- `mix test` passed

#### Remaining after Chunk 16
- Remove remaining shadow comparison plumbing/types that are no longer needed
  for V2-only execution (`shadow_ab` compare/capture infrastructure).
- Continue test renaming cleanup from parity-oriented names to V2 behavior names,
  keeping migration matrix coverage guarantees.

### 2026-03-03 - Phase A Chunk 17 completed (Remove Shadow Compare Infrastructure)

#### Completed
- Removed shadow A/B compare/capture infrastructure from event module internals:
  - deleted `native/emerge_skia/src/events/shadow_ab.rs`
  - added `native/emerge_skia/src/events/dispatch_outcome.rs` with only active
    V2 dispatch output model/types (`DispatchOutcome`, request/event structs,
    `node_key`, `milli`)
- Updated all runtime and processor references from `shadow_ab::*` to
  `dispatch_outcome::*` without behavior changes.
- Renamed parity-oriented preview tests to V2 behavior naming in
  `native/emerge_skia/src/events.rs`:
  - `test_shadow_preview_*` -> `test_v2_preview_*`
  - helper `v1_shadow_outcome_for_event(...)` ->
    `v1_reference_outcome_for_event(...)`
- Updated text commit diagnostics env naming in Wayland backend:
  - primary: `EMERGE_SKIA_TEXT_COMMIT_DIAG`
  - backward-compatible alias: `EMERGE_SKIA_SHADOW_TEXT_DIAG`
- Updated coverage tracking baseline in
  `guides/internals/v2-test-migration-matrix.md`:
  - event-system baseline now `66` tests (shadow compare-only tests removed).

#### Validation
- `cargo test` passed
- `mix test` passed

#### Remaining after Chunk 17
- Continue optional naming polish in docs/examples that still mention legacy
  shadow terminology where no longer useful.

## Purpose
Refactor event handling so dispatch behavior is fully registry-driven for **all events** (current and future), instead of split between registry lookups and procedural fallback logic.

This includes pointer, keyboard, text/IME, focus, scroll, style-state runtime events, and future event families.

---

## Goals

1. Make registry the single dispatch source of truth.
2. Remove feature-specific routing/fallback branches from dispatch core.
3. Preserve existing external behavior/contracts unless explicitly changed.
4. Keep dispatch efficient (typically `O(1)` to small `O(k)` scans per trigger).
5. Keep implementation complexity reasonable and incremental.

---

## Non-Goals

- No immediate multiline text feature work (but architecture must be multiline-ready).
- No unnecessary tree message contract changes for scrolling.
- No breaking changes to Elixir-facing event payload contracts in this refactor.

---

## Locked Architectural Decisions

1. **One winning listener per dispatch job**.
2. Winning listener may execute **multiple actions**.
3. No non-consuming listeners inside dispatcher.
4. Observer behavior (debug/logging/telemetry/raw forwarding) runs in a separate stage:
   - after normalization/coalescing,
   - before listener dispatch.
5. Scroll operations continue to use existing message contract:
   - `TreeMsg::ScrollRequest { element_id, dx, dy }`.
6. Registry build happens in one tree walk (plus optional indexing finalize pass).
7. Collision handling must come from listener presence/order, not explicit fallback code paths.

---

## Current System Findings (Investigation Summary)

Current code already has strong foundations:
- Event registry built from tree/layout refresh.
- Event actor runtime loop with normalization/coalescing.
- Robust text input runtime/session behavior.
- Existing tree actor + message channels are appropriate.

But dispatch selection is currently mixed:
- registry-assisted hit/capability checks **plus**
- specialized procedural handlers (`detect_*`, `handle_*`, fallback branches).

This leads to:
- policy logic spread across many methods,
- collision handling encoded in control flow,
- harder extension for future events.

---

## Target Dispatch Model

## Event Pipeline

1. Raw input arrives from backend.
2. Normalize/coalesce.
3. Observer stage runs (read-only side effects).
4. Convert each normalized event into one or more `DispatchJob`s.
5. For each job:
   - resolve candidates from registry,
   - choose first matching `DispatchRule` by deterministic order,
   - execute all actions from that winning rule.

## Dispatch Jobs

A single normalized event can produce multiple jobs.

Example (`CursorPos`) for hover transition:
- `HoverLeave(prev_target)` job
- `HoverEnter(next_target)` job
- optional `HoverMove(next_target)` job

Each job still has one winner.

---

## Registry Data Model

## Core Naming

- `DispatchRule` = compiled event rule.
- `DispatchRuleId` = index into `Vec<DispatchRule>`.
- `TriggerId` = normalized trigger key.
- `DispatchJob` = dispatch unit.
- `DispatchRuleAction` = effect list item for winning rule.

## Draft Types (Phase 1)

```rust
type NodeIdx = u32;
type DispatchRuleId = u32;

#[repr(u16)]
enum TriggerId {
    // Pointer
    CursorButtonLeftPress,
    CursorButtonLeftRelease,
    CursorButtonMiddlePress,
    CursorMove,
    CursorEnter,
    CursorLeave,
    CursorScrollXNeg,
    CursorScrollXPos,
    CursorScrollYNeg,
    CursorScrollYPos,

    // Keyboard
    KeyLeftPress,
    KeyRightPress,
    KeyUpPress,
    KeyDownPress,
    KeyTabPress,
    KeyEnterPress,
    KeyHomePress,
    KeyEndPress,
    KeyBackspacePress,
    KeyDeletePress,

    // Text/IME
    TextCommit,
    TextPreedit,
    TextPreeditClear,

    // Window/state
    WindowFocusLost,
    WindowResized,
}
const TRIGGER_COUNT: usize = /* compile-time count */;
```

```rust
struct EventRegistryV2 {
    // Dense node metadata
    nodes: Vec<NodeMeta>,
    id_to_idx: rustc_hash::FxHashMap<ElementId, NodeIdx>,

    // Rule arena
    dispatch_rules: Vec<DispatchRule>,

    // Trigger-indexed buckets
    buckets: Box<[TriggerBucket; TRIGGER_COUNT]>,

    // Pointer hit prefilter index (phase 1)
    pointer_index: PointerIndexPhase1,

    // Focus navigation indexes
    focus_order: Vec<NodeIdx>,
    focus_pos: Vec<u32>, // u32::MAX => not in focus_order

    // Root-first visible fallback slots by direction
    first_visible_scrollable_by_dir: [Option<NodeIdx>; 4], // left,right,up,down
}
```

```rust
struct TriggerBucket {
    // O(1)-average lookup for targeted jobs
    targeted: rustc_hash::FxHashMap<NodeIdx, smallvec::SmallVec<[DispatchRuleId; 2]>>,

    // Ordered fallback for untargeted jobs
    ordered: smallvec::SmallVec<[DispatchRuleId; 8]>,
}
```

```rust
struct DispatchRule {
    scope: RuleScope,
    predicate: DispatchRulePredicate,
    actions: smallvec::SmallVec<[DispatchRuleAction; 4]>,
    priority: PriorityKey, // for deterministic ordering/debug validation
}
```

```rust
enum DispatchJob {
    Targeted { trigger: TriggerId, target: NodeIdx, ctx: DispatchCtx },
    Pointed { trigger: TriggerId, x: f32, y: f32, ctx: DispatchCtx },
    Untargeted { trigger: TriggerId, ctx: DispatchCtx },
}
```

```rust
struct PointerIndexPhase1 {
    // For each pointer-related trigger, reverse-z candidate list
    candidates_by_trigger: Box<[Vec<NodeIdx>; TRIGGER_COUNT]>,
}
```

## Dispatch Rule Scope

```rust
enum RuleScope {
    Target(NodeIdx),        // exact node target
    Focused(NodeIdx),       // exact focused node
    PointerHit,             // uses pointed hit resolution
    NoFocus,                // only when no focused node
    Any,                    // global/default
}
```

## Predicate Sketch

```rust
struct DispatchRulePredicate {
    required_mods: ModMask,
    forbidden_mods: ModMask,
    runtime_flags_all: RuntimeFlags,   // bitmask
    runtime_flags_none: RuntimeFlags,  // bitmask
    // optional dynamic checks by precomputed capability fields in node metadata
}
```

## Action Sketch

```rust
enum DispatchRuleAction {
    EmitElementEvent { element: NodeIdx, atom: EventAtom, payload: EventPayload },
    SendTreeMsg(TreeMsgAction),               // compiles to existing TreeMsg
    FocusChange { next: Option<NodeIdx> },    // includes focused_active updates
    TextEdit { element: NodeIdx, op: TextOp },
    TextCommand { element: NodeIdx, op: TextCommandOp },
    TextCursorSet { element: NodeIdx, x: f32, extend_selection: bool },
}
```

---

## Phase 2 Performance Upgrade Structures

Phase 2 adds stronger asymptotics for large scenes/high pointer traffic.

## 1) Spatial Pointer Grid Index

```rust
struct PointerGridIndex {
    cell_size: f32,
    grids_by_trigger: Box<
        [rustc_hash::FxHashMap<CellId, smallvec::SmallVec<[NodeIdx; 8]>>; POINTER_TRIGGER_COUNT]
    >,
}
```

Expected lookup near `O(1 + m)` where `m` is candidates in touched cells.

## 2) Focused-Key Direct Rule Table

```rust
struct FocusedKeyRuleTable {
    // [node][focused-key-trigger] -> direct winning rule id if static
    table: Vec<Box<[Option<DispatchRuleId>; FOCUSED_KEY_TRIGGER_COUNT]>>,
}
```

Focused key dispatch becomes strict `O(1)` in common path.

## 3) Tab Navigation Tables

```rust
struct FocusNavTable {
    next: Vec<Option<NodeIdx>>,
    prev: Vec<Option<NodeIdx>>,
}
```

Tab/Shift-Tab next target in `O(1)`.

## 4) Optional Predicate Bitmask Precompile

Precompute predicate masks and small capability words to reduce branching and cache misses at runtime.

---

## Registry Build Rules (General)

Single tree walk should register all relevant rules:

- Pointer listeners (`on_click`, `on_mouse_*`, press, drag origin).
- Keyboard listeners (arrow/tab/enter/home/end/etc.).
- Text listeners (edit/command/preedit) only when predicates/capabilities match.
- Focus listeners (`on_focus`, `on_blur`, focused style runtime toggles).
- Scroll listeners (wheel, drag, scrollbar, keyboard-driven scroll).
- Style runtime listeners (`mouse_over`, `mouse_down`, etc.).
- Defaults/fallbacks as explicit low-priority rules (not procedural fallbacks).

Important:
- Collision behavior is solved by deterministic ordering.
- If a higher-priority rule is absent/non-matching, next rule naturally wins.

---

## Dispatch Algorithm (One Winner)

For each `DispatchJob`:

1. Resolve candidate rule IDs:
   - `Targeted`: `bucket.targeted[target]` then optional `bucket.ordered`.
   - `Pointed`: resolve pointer hit target via pointer index, then targeted lookup.
   - `Untargeted`: `bucket.ordered`.
2. Evaluate predicates in sorted order.
3. First matching rule is winner.
4. Execute winner action list in declared order.
5. Stop job.

No feature-specific fallback branches.

---

## Collision Handling Examples (General)

## 1) Pointer move between non-siblings
- Event creates jobs: `HoverLeave(old)`, `HoverEnter(new)`.
- Each job resolves one winner against exact target node.
- No sibling assumptions needed.

## 2) Text input right-arrow at boundary vs scroll
- Focused text-edit rule for right-arrow only matches if movement-capable predicate is true.
- At boundary, that rule does not match.
- Next eligible rule (e.g. scroll rule) wins naturally.

## 3) Tab to off-viewport focusable
- Winning tab rule actions:
  - set focus target,
  - emit one or more reveal `ScrollRequest` actions.
- Still one winner, multiple actions.

## 4) Enter on focused button
- Focused button enter rule wins.
- Actions emit `:press` and any required runtime updates.

---

## Complexity Targets

## Phase 1

- Trigger bucket selection: `O(1)`.
- Targeted job dispatch: `O(1)` average + tiny predicate checks.
- Untargeted dispatch: `O(k)` per trigger bucket (small expected).
- Pointed dispatch: `O(k)` over prefiltered pointer candidates.
- Focus next/prev: `O(1)`.

## Phase 2

- Trigger bucket selection: `O(1)`.
- Focused key dispatch: `O(1)` via focused-key table.
- Pointed dispatch: expected `O(1 + m)` with spatial index.
- Untargeted remains small `O(k)` where required.

---

## Migration Plan

## Phase A - Foundation + Parallel Build
- Add `EventRegistryV2` types and build path in parallel with current registry.
- Keep old dispatcher active.
- Add parity harness to compare routing outcomes.

## Phase B - Dispatcher Skeleton
- Implement generic `DispatchJob` engine and one-winner execution.
- Migrate a narrow trigger set first (e.g. keyboard + focus).

## Phase C - Family Migration
- Migrate pointer/hover/press/click.
- Migrate text/IME.
- Migrate scroll + scrollbar.
- Keep existing message contracts.

## Phase D - Remove Procedural Selection
- Remove specialized dispatch-routing methods and explicit fallback branches.
- Retain pure helpers for geometry/state mutation.

## Phase E - Hardening + Docs
- Add listener ordering and trigger taxonomy docs.
- Add debug dump/introspection of built rule buckets.
- Final parity + regression pass.

---

## Testing Strategy

1. Parity tests (old path vs V2) during migration.
2. Deterministic precedence tests.
3. Collision tests across multiple event families.
4. Pointer transition tests across non-siblings.
5. Focus/no-focus default behavior tests.
6. Property/regression tests for event payload contracts.
7. Performance sanity tests (dense node scene, high pointer frequency).
8. Full suite: `cargo test`, `mix test`.

---

## Risks & Mitigation

- **Dual-path complexity during migration**
  - Mitigate with strict phase gates and parity tests.
- **Ordering regressions**
  - Mitigate with explicit `PriorityKey` and deterministic tests.
- **Overfitting current event set**
  - Mitigate with generic trigger/scope/predicate/action model.

---

## Success Criteria

- Dispatch core is generic and registry-driven.
- One winner per dispatch job with multi-action execution.
- No feature-specific fallback routing in dispatch core.
- Collision behavior is explainable by rule ordering and predicates.
- Existing user-facing behavior preserved (or intentionally improved) with tests.

---

## Approval Checklist

- [ ] Naming (`DispatchRule`, `DispatchJob`, `TriggerId`) accepted
- [ ] One-winner model accepted
- [ ] Observer stage placement accepted
- [ ] Phase 1/Phase 2 data structure plan accepted
- [ ] Migration phases accepted
