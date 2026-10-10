# Text input reconciliation: confirmed diagnosis

Status: historical pre-fix diagnosis. The repair is now implemented; see
[text-input-reconciliation.md](text-input-reconciliation.md) for validation.
Source line references below describe the original code.

## Root cause

Emerge confuses acknowledgment of a native tree write with acknowledgment of the
application's change callback. Both use `pending_text_patches`.

1. `content_change_actions` records an expected app echo when an edit emits a
   change callback (`native/emerge_skia/src/events/registry_builder.rs:5612`).
2. A native registry rebuild matching the current content consumes that entry
   (`native/emerge_skia/src/events/runtime.rs:2879`).
   `consume_pending_text_patch_match` also removes every earlier entry through
   the match (`runtime.rs:2783`). The app need not have processed its callback yet.
3. A delayed app patch can no longer match the pending queue. If it differs from
   current content, the focused reconciliation path accepts it as a replacement
   (`runtime.rs:2949–2978`).
4. Accepting the shorter value calls `TextInputState::set_content`
   (`runtime.rs:2928`, `native/emerge_skia/src/events.rs:263`). That clamps the
   caret to the shorter length. Accepting a later, longer echo does not advance
   the caret again.

This is a content-authority bug, not a font/caret drawing issue. The next emitted
change callback actually contains the reordered text.

## Deterministic reproduction

The probe uses the real `HostEventRuntime`, `TreeUpdateEngine`, native content and
runtime commands, change callbacks, and binary `SetAttrs` patches. App echoes use
exactly the emitted strings, in FIFO order, like the supplied Notes controller.
Native writes are allowed to settle before app echoes are delivered.

| Step | Native text and caret | Pending app values |
| --- | --- | --- |
| Type `a`, before native acknowledgment | `a|` | `["a"]` |
| Native acknowledgment | `a|` | `[]` |
| Type `b`, before native acknowledgment | `ab|` | `["ab"]` |
| Native acknowledgment | `ab|` | `[]` |
| App echoes `a` | `a|` | `[]` |
| App echoes `ab` | `a|b` | `[]` |
| Type `c` | `ac|b` | emitted callback is `"acb"` |

Confirmed for both `TextInput` and `Multiline`. A control delivering each echo
before the next keystroke yields `abc|`. No sleeps, out-of-order controller
messages, or keyboard-specific actions are required.

The probe extends pending expiry to 60 seconds before each native acknowledgment
so expiration cannot explain this result. Entries are nevertheless consumed
immediately. Increasing the existing 40–400 ms TTL will not fix this cause.

## Actual application path checked

Inspected the local Fairphone app and its installed dependencies:

- App pins Emerge 0.4.2 and Solve 0.3.1 in `mix.lock`.
- The relevant Emerge runtime, text-state, and viewport files match this worktree
  byte-for-byte. Solve's installed controller and lookup files also match the
  inspected `/workspace/solve` sources.
- `Solve.Lookup.event/2` returns the controller PID and `{:solve_event, :text}`.
  Emerge appends the changed string and sends the message from the viewport.
- Solve invokes the shown `text/2` handler, increments its own state revision,
  and sends a versioned state update to subscribers. Lookup rejects older Solve
  revisions, but those revisions do not identify native text edits in Emerge.
- `FairphoneOs.UI.Display.handle_solve_updated/2` requests a viewport rerender;
  Notes renders `notes.text` back into `Input.text/2`.
- `Keyboard.field_attrs/1` only attaches focus/blur and F1–F3 layer handlers. It
  does not alter text or cursor positions.

Thus correct controller message ordering does not prevent the race: an app value
can be new relative to its previous Solve value but old relative to native edits.
The diagnostic simulates this round trip; it is not an on-device trace of the
running Fairphone app.

## History and missing coverage

`git blame` and the patch for `76ce013` (`Fix DRM todo insert animation stalls`)
show that it introduced consumption on matching native base content. Previously,
focused echo matching consumed the queue when matching incoming `patch_content`.
That commit also added write deduplication to prevent reconciliation feedback
loops. This identifies the origin of the premature-consumption branch, not a
full historical runtime bisect or a recommendation to revert the whole commit.

Existing matching-patch tests seed the pending queue and test echoes while it is
still populated. They do not cover the separate native acknowledgment followed
by delayed FIFO app echoes and a subsequent edit.

## Probe and results

The temporary pre-fix probe was promoted to permanent tests in
`native/emerge_skia/src/events/runtime/tests/delayed/text_echo.rs`. Run:

```sh
cargo test --manifest-path native/emerge_skia/Cargo.toml --lib text_echo
```

Historical results before the fix:


- Observed-bug probe and immediate-echo control: **2 passed**, 1 ignored.
  Passing here confirms the broken behavior, not a fix.
- Explicit correct-behavior assertion: **failed**, actual `("acb", 2)` versus
  expected `("abc", 3)`.

## Fix boundary

Native tree receipts must not acknowledge app echoes. The implementation removes
snapshot-driven resends instead of adding a second write ledger, processes ID’d
app proposals once, and uses a five-second callback-echo window. It does not force
the caret to the end, debounce the controller, or revert the animation-stall fix.
Timeout and intentional-reset limitations are documented in the implemented design.
