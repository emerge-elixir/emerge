# V2 Test Migration Matrix

This matrix tracks event-system test coverage while removing V1 code paths.

## Baseline

- `native/emerge_skia/src/events.rs`: 60 tests
- `native/emerge_skia/src/events/runtime.rs`: 6 tests
- Total event-system baseline: 66 tests

## V1/Parity-Oriented Tests and V2 Coverage Mapping

Status legend:

- `covered` = equivalent V2 behavior is already asserted by an active test
- `todo` = replacement test still needed before deleting source test

| Legacy test | V2 coverage test | Status |
| --- | --- | --- |
| `test_registry_v2_arrow_down_parity_without_focus` | `test_registry_v2_arrow_down_parity_without_focus` | covered |
| `test_registry_v2_tab_focus_change_parity` | `test_registry_v2_tab_focus_change_parity` | covered |
| `test_registry_v2_enter_press_parity_for_focused_pressable` | `test_registry_v2_enter_press_parity_for_focused_pressable` | covered |
| `test_shadow_preview_text_command_request_parity` | `test_v2_preview_text_command_request_parity` | covered |
| `test_shadow_preview_text_edit_request_parity` | `test_v2_preview_text_edit_request_parity` | covered |
| `test_shadow_preview_text_commit_emits_change_event_payload` | `test_v2_preview_text_commit_emits_change_event_payload` | covered |
| `test_shadow_preview_backspace_emits_change_event_payload` | `test_v2_preview_backspace_emits_change_event_payload` | covered |
| `test_shadow_preview_backspace_at_start_emits_no_change_event` | `test_v2_preview_backspace_at_start_emits_no_change_event` | covered |
| `test_shadow_preview_backspace_with_selection_emits_change_event_payload` | `test_v2_preview_backspace_with_selection_emits_change_event_payload` | covered |
| `test_shadow_preview_text_preedit_request_parity` | `test_v2_preview_text_preedit_request_parity` | covered |
| `test_shadow_preview_enter_press_does_not_duplicate_press_event` | `test_v2_preview_enter_press_does_not_duplicate_press_event` | covered |
| `test_shadow_preview_tab_emits_focus_transition_events` | `test_v2_preview_tab_emits_focus_transition_events` | covered |
| `test_shadow_preview_window_focus_lost_emits_blur_event` | `test_v2_preview_window_focus_lost_emits_blur_event` | covered |
| `test_shadow_preview_scrollbar_thumb_drag_parity_sequence` | `test_v2_preview_scrollbar_thumb_drag_parity_sequence` | covered |
| `test_shadow_preview_scrollbar_hover_request_parity` | `test_v2_preview_scrollbar_hover_request_parity` | covered |
| `test_shadow_preview_style_runtime_mouse_over_request_parity` | `test_v2_preview_style_runtime_mouse_over_request_parity` | covered |
| `test_shadow_preview_style_runtime_mouse_down_parity_sequence` | `test_v2_preview_style_runtime_mouse_down_parity_sequence` | covered |
| `test_shadow_preview_pointer_element_events_parity_sequence` | `test_v2_preview_pointer_element_events_parity_sequence` | covered |
| `test_shadow_preview_mouse_button_payload_is_none` | `test_v2_preview_mouse_button_payload_is_none` | covered |
| `test_shadow_preview_key_scroll_requests_are_deduped` | `test_v2_preview_key_scroll_requests_are_deduped` | covered |
| `test_shadow_preview_tab_focus_reveal_scroll_is_deduped` | `test_v2_preview_tab_focus_reveal_scroll_is_deduped` | covered |

## Runtime V2-only Regression Tests

- `v2_only_no_prediction_still_advances_hover_state_for_leave_only_target`
- `reconcile_keeps_focused_session_when_descriptor_is_stale`
- `reconcile_applies_descriptor_content_when_unfocused`
- `enrich_predicted_edit_change_events_uses_session_state_for_text_commit`
- `enrich_predicted_edit_change_events_removes_noop_backspace_change`
- `enrich_predicted_edit_change_events_removes_change_when_not_allowed`

## Deletion Gate

Before deleting a V1/parity test, ensure:

1. It is mapped to a V2 behavior test in this file.
2. Status is `covered`.
3. `cargo test` and `mix test` pass after deletion.
