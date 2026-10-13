# Multiline word-wrap hardware acceptance

Status: native fix and regressions validated; FP3 hardware acceptance pending.
Branch/worktree: `fix/multiline-input-wrap`, `/workspace/emerge-multiline-input-wrap`.
Base: `main` at `8dc9d6b`.

## Implemented fix

`native/emerge_skia/src/tree/text_layout.rs::layout_text_lines` now restarts `idx`
at `break_idx` after a space wrap, remeasuring the word prefix moved to the new
line. Previously that prefix remained in text/cursor positions but disappeared
from wrap accounting, underestimating line count and auto-height.

With unit advances and width 4, `"a abc de"` now produces `"a "`, `"abc "`, `"de"`
instead of the overflowing `"a "`, `"abc de"`.

Wrapping policy, whitespace/newline/character-index conventions and overlong-word
handling are unchanged. Native auto-height edits already request Resolve; no
invalidation, controller, clipping or application changes were needed.

Regressions cover incremental edits, spacing, Unicode, padded fill-width layout,
sibling movement, alignment, selection/caret/hit/navigation geometry, stale/latest
controller echoes, deletion/bulk insertion and direct versus cached raster frames.
All seven detect the original defect. Source-built `mix test`, Rust debug/release
and DRM/raster suites, CI quality/test checks and strict all-target Clippy passed.

## Remaining hardware acceptance

The Fairphone path dependency now points into this worktree for the next firmware
build. Leave unrelated local app, display and system changes untouched. In Notes
(fill width, 32px padding, 44px font, content-driven height):

1. Type short-word sentences quickly; each word must fit individually.
2. Confirm wrapping happens on the first edit that needs it, with immediate height
   growth and matching caret/selection boundaries.
3. Insert repeated spaces and Enter; paste and backspace across wraps.
4. Repeat with keyboard show/hide and display off/on.
5. Confirm no text/caret escapes the rounded field or leaves stale cached pixels.

Close this plan after hardware acceptance; retain the regression tests and
changelog rather than an active completed implementation or validation log.
