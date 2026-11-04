# Event documentation audit

- [x] Audit public modules and tutorials against native dispatch and viewport routing.
- [x] Complete Event payload/coordinate/drag docs and public type coverage.
- [x] Update low-level notifications, viewport/raw-input guidance, and tutorial examples.
- [x] Explain decorative-overlay hit testing and Solve fixed-argument payload adapters.
- [x] Compile/run documented examples; run full CI, cargo test and mix docs serially.
- [x] Restore the demo's native feature matrix without changing demo sources.

Completed:
- Event reference now inventories runtime payloads, external-PID/custom wrapping,
  logical float coordinates, release targeting, stationary hover transitions,
  capture phases, cancellation, discarded sources and incompatible handlers.
- Mouse and drag helper docs include complete handlers; native notifications
  describe pointer/drag/scroll payloads and raw touch coordinates separately.
- Added `drag_attr()` to the public Event union and documented drag phase/position types.
- Input tutorial has a runnable pad and every raw subscription; viewport/input,
  nearby composition and Solve tutorials link the matching interaction guidance.
- Added executable docs regressions for mouse positions, all four drag phases,
  cancellation state, public type coverage and code-block parsing.

Validation:
- `./ci-tests.sh all`: 716 Elixir cases passed, 12 hardware exclusions; quality
  checks, native release tests and Dialyzer passed.
- `cargo test`: 1,639 unit + 14 integration tests passed in debug; release also passed.
- `mix docs --warnings-as-errors`: passed with 31 documentation screenshots.
- 387 generated local links/anchors across 13 affected pages checked; none broken.
- Demo native restoration passed; its tracked diff and untracked-path status are unchanged.

0.6 stays unreleased; package versions and main are unchanged. No demo source edits.
Logs: `/tmp/emerge-event-docs-{ci,rust,build,demo-restore}.log`.

## Commit sequence

- [x] Migration guide, release navigation, packaging and executable migration examples.
- [x] Pointer/drag API reference, runnable input tutorial and documentation regressions.
- [x] Solve routing, overlay hit testing and completed audit/validation notes.

Intermediate commits were checked in an isolated worktree: formatting and 25
migration/configuration tests after the first commit, then formatting and 29
migration/event documentation tests after the second and final commits. The
previous full-CI source snapshot is preserved, apart from these progress notes.
The demo native artifact is restored after serial builds; demo sources and main
remain untouched.
