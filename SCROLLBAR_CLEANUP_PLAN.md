# Scrollbar Cleanup Plan

This tracks the refactor work to simplify scrollbar behavior and reduce duplication.

## Phase 1 (current)

- [x] 1. Add shared runtime scrollbar attr preservation helper in `native/emerge_skia/src/tree/attrs.rs`
- [x] 2. Replace duplicated runtime preservation logic in `native/emerge_skia/src/tree/layout.rs` and `native/emerge_skia/src/tree/patch.rs`
- [x] 3. Add unit tests for the shared preservation helper
- [x] 4. Remove duplication in `native/emerge_skia/src/tree/element.rs` with private axis helpers
- [x] 5. Add/adjust `native/emerge_skia/src/tree/element.rs` tests for clamp parity and tri-state/no-op behavior

## Next cleanup candidates

- [x] 6. Extract scrollbar-specific interaction internals from `native/emerge_skia/src/events.rs` into `native/emerge_skia/src/events/scrollbar.rs`
- [x] 7. Merge duplicated scrollbar hit-test paths into one typed hit API
- [x] 8. Replace mixed boolean scrollbar interaction flags in `EventProcessor` with a single explicit interaction state enum
- [x] 9. Move pointer-to-scroll mapping helpers next to scrollbar hit logic and reuse across track-click and thumb-drag
- [x] 10. Keep current behavior stable: snap-to-cursor track click, drag continuation, click suppression, axis-specific hover requests
- [x] 11. Deduplicate `render_element` and `render_tree_recursive` in `native/emerge_skia/src/tree/render.rs`
- [x] 12. Keep scrollbar thumb rendering in one path and preserve nearby-element ordering
- [x] 13. Expand render parity tests after dedupe
- [x] 14. Add direct metric tests in `native/emerge_skia/src/tree/scrollbar.rs` (default/hover thickness, min thumb length, clamps)
- [x] 15. Update docs (`EVENTS.md`, `SCROLLING.md`, `PLAN.md`) after refactors
- [x] 16. Run validation pass (`cargo +stable fmt`, `cargo +stable test`, `mix test` attempted; `mix` is unavailable in this environment)
