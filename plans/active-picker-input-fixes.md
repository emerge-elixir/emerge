# Picker input fixes

Targets: mouse-coordinate worktree and `/workspace/emerge_demo`.

- [x] Reproduce decorative front-nearby blockers covering the pad and hue marker
  using actual native listener dispatch, not only synthetic Solve payloads.
- [x] Make decoration non-blocking using existing behind-content/normal-child
  composition. Preserve popup/front-nearby hit isolation semantics.
- [x] Reproduce and fix suppression of real coalesced cursor movement replayed
  after a stale-listener wait; keep stationary revalidation callback-free.
- [x] Add native and demo regressions, including moving across the marker,
  scaled pads and delayed rebuilds. Recheck rendered picker colors.
- [x] Run native `cargo test`, framework and demo `mix test`, formatting,
  warnings-as-errors compilation and Clippy. Note unavailable live displays.

## Results

Both failures were reproduced before fixing them: the real demo tree received
raw button input but emitted no pad mouse-down; stale-lane replay emitted down/up
without the buffered move. Decorations now use behind-content and normal children;
front-nearby popup blocking is unchanged. Real replay uses normal dispatch, while
stationary geometry revalidation still suppresses synthetic moves.

- Framework `./ci-tests.sh all`: passed (703 ExUnit/doctest cases, 12 hardware
  exclusions; 1,627 Rust unit plus 14 integration tests; Clippy and Dialyzer clean).
- Explicit debug `cargo test`: passed the same 1,641 Rust tests.
- Demo formatting, warnings-as-errors compilation and tests: passed (118 tests).
- New native harness hooks drive actual button/motion input through the real
  demo tree, retained controller patches and NIF payloads at local 1x/2x and
  inherited 1x/1.25x. The framework also tests the native float-map bridge.
- Raster pixels and the refreshed preview were checked:
  `/tmp/emerge-demo-mouse-coordinates.png`.
- Live desktop/macOS input remains untested without a display. Restart the demo
  to load the updated native library.
