# Repeated pressed feedback on main

Reproduce the reported `Input.button` + `on_press` + `Interactive.mouse_down`
behavior on local main (`b80f524`), without changing the existing feature worktree.

- Drive repeated presses/releases on the same focused button, including primary
  touch and rotation. Reduce away application patches/Solve if native input alone
  reproduces the problem.
- Assert visible pressed/released output and delivered activations, not listener
  cache structure or runtime flags.
- Trace pressed-state invalidation and retained listener reuse; make the smallest
  fix consistent with the observed failure.
- Run focused regressions, full `cargo test` and `mix test` via local CI, and
  quality checks. No hardware confirmation is available in this workspace.

## Result

- Reproduced before the fix on the second press with both mouse and primary
  touch, on a 90-degree rotated button. Solve and application patches are not
  required to trigger it.
- A focus rebuild can cache listeners while the button is pressed. A later
  paint-only release reuses those listeners, whose activation action had been
  omitted because the button was already pressed when they were built.
- Always include the idempotent pressed-style activation action when the style
  exists. This avoids extra registry invalidation and preserves shared feedback
  ownership for overlapping virtual-key contacts.
- One behavioral regression checks rendered pressed/released colors and exactly
  one activation per cycle, for three repeated mouse/touch cycles.
- `EMERGE_SKIA_BUILD=true ./ci-tests.sh test`: 1,654 Rust tests and 720 ExUnit
  tests/doctests passed; 15 hardware exclusions.
- `EMERGE_SKIA_BUILD=true ./ci-tests.sh quality`: passed.
- `cargo test --release --features drm --lib`: 1,727 passed, 2 ignored.

## Commit sequence

Branch: `fix/repeated-mouse-down-feedback`, based on main at `b80f524`.

1. `fix(events): restore pressed feedback on repeated interactions`
   Keep the idempotent activation fix and pixel/event regression together so
   the code commit passes tests independently.
2. `docs(events): record the repeated pressed-feedback fix`
   Add the changelog entry and retain the diagnosis and validation record.

Main and other worktrees remain unchanged. Nothing is pushed.
