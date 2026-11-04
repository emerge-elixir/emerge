# Swipe metadata

Add payloads to all four existing swipe handlers:
`%{start: %{x: float(), y: float()}, end: %{x: float(), y: float()}, duration_ms: float()}`.

- Preserve recognition, axis locking, scroll precedence and cancellation behavior.
- Record local start position and monotonic timestamp at the original press;
  resolve the local release position from the source's current geometry.
- Preserve edge timestamps across buffering and primary-touch mouse emulation.
- Encode the same payload through Rustler and macOS (versioned wire contract).
- Update public event/native docs, changelog and 0.6 migration guidance.
- Keep tests behavioral: a timed delayed gesture (mouse/touch), real NIF delivery,
  and fixed wire vectors; adapt existing swipe expectations without new
  listener/tracker representation assertions.
- Run cargo test and mix test via local CI, plus quality checks.

Preserve the existing uncommitted migration-guide warning removal.

## Completed

- Metadata delivered through the NIF, normal viewport wrapping and macOS protocol 20.
- Behavioral checks cover all four directions, local geometry changes during a
  buffered mouse/touch gesture, original edge timing, and fixed wire vectors.
- Existing DRM input tests accept timestamped touch edges.
- `EMERGE_SKIA_BUILD=true ./ci-tests.sh test`: 1,665 Rust tests; 734 ExUnit
  tests/doctests passed, with 15 hardware exclusions.
- `EMERGE_SKIA_BUILD=true ./ci-tests.sh quality`: passed.
- `cargo test --release --features drm --lib`: 1,738 passed, 2 ignored.
- Final `EMERGE_SKIA_BUILD=true mix test`: 728 passed, 21 exclusions (including
  the six full-sweep tests already covered by local CI).
- `EMERGE_SKIA_BUILD=true mix docs` and `git diff --check`: passed.

The migration warning removal is preserved.

## Commit sequence

1. `fix(input): preserve touch timestamps through buffered dispatch`
   Keep ingress timestamps through actor backpressure, pending input and
   primary-touch pointer emulation; adapt existing DRM input tests.
2. `feat(events): include local endpoints and duration in swipe events`
   Keep recognition, NIF maps, macOS protocol 20, public API types/docs and
   behavioral/wire tests together so the payload change is atomic.
3. `docs(events): explain swipe metadata and the 0.6 migration`
   Update guides, changelog and this implementation record; retain removal of
   the redundant drag-attribute warning.

Retain the existing six pointer-event commits unchanged. Nothing is pushed.
