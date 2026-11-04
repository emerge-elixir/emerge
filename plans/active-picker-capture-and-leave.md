# Captured picker, fractional scale, separate leave showcase

Targets: mouse-coordinate worktree and `/workspace/emerge_demo`. Preserve existing work.

- [x] Add opt-in `Event.on_drag/1`: native left-button capture, logical unclamped
  x/y plus start/move/end/cancel phase. Rebind geometry after patches; cancel on
  window leave/focus loss and discard removed/remounted sources. Keep ordinary
  mouse/click/press, scroll/swipe and raw observer semantics unchanged.
- [x] Carry drag maps over NIF/macOS transport; document the public API.
- [x] Migrate the shared picker to captured drag events, with 1x/1.6x/2x versions.
  Clamp HSV values, not incoming coordinates; release outside ends the drag.
- [x] Add a separate Mouse leave showcase with unmodified enter/move/leave
  behavior and visible out-of-bounds leave coordinates.
- [x] Test actual native dispatch, release outside, cancellation, patched marker
  geometry and fractional/inherited scale, plus controller/layout/transport.
- [x] Run cargo test and mix test in the framework, demo tests, and CI checks.

No live desktop is available; note the native-library restart requirement.

## Results

- Framework `./ci-tests.sh all`: passed; 708 ExUnit/doctest cases, 12 hardware
  exclusions, formatting/Credo/Clippy/Dialyzer clean.
- Explicit debug `cargo test` and CI release tests: 1,631 unit plus 14 integration
  tests (1,645 total), all passed.
- Demo formatting, warnings-as-errors compile and `mix test`: 122 passed.
- `mix docs`: passed, including 31 documentation screenshots.
- Actual native input covers 1x/1.6x/2x and inherited 1x/1.25x, moving markers,
  movement and release outside, blur/window-leave cancellation, ordered buffered
  replay, removed handlers/remounts and scale changes during capture.
- Separate uncaptured leave regressions verify border/padding origin and raw
  negative coordinates. Raster previews were inspected:
  `/tmp/emerge-demo-captured-picker.png`, `/tmp/emerge-demo-mouse-leave.png`.
- macOS protocol is now 19; phase plus two f64s round-trip in both codec suites.
- Live desktop/macOS pointer checks remain unexecuted. Restart the demo with
  `EMERGE_SKIA_BUILD=true`; macOS also needs a matching local host build.

This supersedes the original picker stop-on-leave design. Ordinary mouse helpers
still do not capture; capture is explicitly opted into with `Event.on_drag/1`.
No commits or merges were made.
