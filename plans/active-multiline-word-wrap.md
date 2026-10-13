# Multiline word-wrap hardware acceptance

Branch/worktree: `fix/multiline-input-wrap`, `/workspace/emerge-multiline-input-wrap`.
The Fairphone path dependency points into this worktree for the next firmware
build. Leave unrelated app, display and system changes untouched.

## Remaining FP3 Notes acceptance

In Notes (fill width, 32px padding, 44px font, content-driven height):

1. Type short-word sentences quickly; each word must fit individually.
2. Confirm wrapping on the first edit that needs it, immediate height growth and
   matching caret/selection boundaries.
3. Insert repeated spaces and Enter; paste and backspace across wraps.
4. Repeat with keyboard show/hide and display off/on.
5. Confirm no text/caret escapes the rounded field or leaves stale cached pixels.

Device testing/deployment is user-owned. Close this plan after acceptance;
software regressions and the changelog retain the fix, not an implementation log.
