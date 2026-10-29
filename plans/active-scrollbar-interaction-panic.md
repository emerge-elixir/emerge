# Scrollbar interaction panic

Work in `/workspace/emerge-mouse-local-coordinates`; preserve previous framework
and demo changes.

- [x] Reproduce `scrollbar press needs interaction` with real clipped/zero-area
  scrollbar geometry before fixing it.
- [x] Make all four scrollbar thumb/track press slots handle unavailable pointer
  regions normally; preserve keyboard, retained state and visible scrollbar input.
- [x] Audit related pointer/scrollbar builders for the same assumption and test
  missing, collapsed, clipped and singular interaction states, plus rebuilds.
- [x] Add native/NIF regressions and exercise the real demo at small viewports.
- [x] Run cargo test, mix test, demo tests and full CI; rebuild the native library.

No live desktop display is available. A running demo must restart to load the NIF.

## Reproduction and fix

The exact panic was reproduced before the fix in three native regressions:
fully clipped live scrollbar metrics, a collapsed cross-axis, and focused keyboard
state on an invisible scroller. Scrollbar metrics are retained runtime geometry,
not proof of visibility. The shared press builder now takes a resolved state
reference and returns `Option<Listener>` instead of unwrapping a missing region;
all four axis/thumb/track slots propagate absence normally. Metadata is retained.

## Audit

Related primary-pointer, wheel, hover, nearby-blocker and captured-drag builders
already handle optional regions. Scrollbar hover uses `and_then`; inverse-transform
consumers return no actions when an inverse is unavailable. The two unsafe
geometry expects removed here were shared by all four scrollbar press paths.
Other inspected runtime unwraps are guarded by explicit nonempty/Some conditions,
not by the presence of unrelated layout metrics. No unrelated refactoring added.

## Validation

- Six new Rust regressions cover missing state, full clipping, zero-width/height,
  singular transforms, visible hits, keyboard retention and full/cached/render
  rebuilds with scroll visibility transitions at 1x/1.6x/2x.
- Two NIF regressions verify renderer survival and actual scrollbar input after
  hiding/revealing a nested scroller.
- Demo regression patches every showcase tab at three small/scaled viewport
  configurations (36 native scenes), including cursor-state revalidation.
- Explicit debug cargo test: 1,637 unit plus 14 integration tests (1,651 total).
- Framework ./ci-tests.sh all: 710 ExUnit/doctest cases passed, 12 hardware
  exclusions; formatting/Credo/Clippy/Dialyzer and release Rust tests passed.
- Demo warnings-as-errors compile and mix test: 123 passed.
- Initial parallel validation mixed the demo wide-feature NIF with framework
  default-feature assertions. After diagnosing the two matrix failures, full CI
  was rerun serially and passed. Then the demo dependency was force-rebuilt with
  its own configured features and its full suite passed again. Shared path-dep
  native artifacts must not be copied concurrently by different configurations.
- Logs: /tmp/emerge-scrollbar-before.log, /tmp/emerge-scrollbar-ci.log,
  /tmp/emerge-scrollbar-rust.log, /tmp/emerge-scrollbar-demo-final.log.

Live desktop input remains untested. Restart the demo to load the rebuilt NIF.
All prior work and the demo makeup_emerge dependency edit are preserved.
