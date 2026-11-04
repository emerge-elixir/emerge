# Demo migration and mouse-coordinate showcase

Target: `/workspace/emerge_demo` (existing `emerge040` branch).
Preserve the user's existing `makeup_emerge` path edit in `mix.exs`.

- [x] Point Emerge at `../emerge-mouse-local-coordinates` and align dependencies.
- [x] Migrate viewport, headless producers, examples, tests and validation scripts
  to `Emerge.Config`; retain the existing renderer/API selection.
- [x] Adapt fixed-payload Solve routes to positional mouse payloads without
  dropping their original argument or introducing four-element Solve messages.
- [x] Add a Mouse coordinates page: HSV color picker, shared 1x/2x pads, logical
  coordinate readout, enter/leave/down/up/move feedback and click-to-save swatches.
  No manual scale division; leaving a pad ends its drag and retains raw coordinates.
- [x] Add controller, routing, page, scaling/layout and raster-render regressions;
  update runnable snippets and README.
- [x] Run demo formatting, warnings-as-errors compilation and full `mix test`;
  rerun native `cargo test`. Note unavailable display/hardware checks.

## Validation

- Demo: `mix format --check-formatted`, `mix compile --warnings-as-errors`,
  and `mix test --warnings-as-errors` passed (117 tests).
- Native: `cargo test` passed (1,626 unit tests plus 14 integration tests).
- Rendered and inspected a raster preview; regression tests check picker pixels
  against HSV colors and logical pad dimensions at local/inherited scales.
- Preview: `/tmp/emerge-demo-mouse-coordinates.png`.
- No desktop display is available here; live Wayland/macOS input and the PRIME
  hardware validation script remain unexecuted. Existing local makeup dependency
  override was preserved.
