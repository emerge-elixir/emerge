# 0.6 migration guide

- [x] Document the six breaking pointer message shapes, logical float coordinates,
  tuple/external-PID/custom-wrapper routing, unchanged APIs and optional dragging.
- [x] Explain matching native binaries, source-checkout rebuilds and restart;
  distinguish the planned 0.6 release from currently published versions.
- [x] Add release-facing navigation/changelog links and docs configuration tests.
- [x] Validate documented handler/wrapper examples, cargo test, mix test and docs.

Validation (including the event documentation audit):
- `./ci-tests.sh all`: 716 Elixir cases passed, 12 hardware exclusions;
  formatting, warnings-as-errors compilation, Credo, Clippy and Dialyzer passed.
- Rust debug and release: 1,639 unit + 14 integration tests passed.
- `mix docs --warnings-as-errors`: passed; 31 documentation screenshots.
- Checked 387 local links/anchors across 13 affected generated pages: none broken.
- Full-sweep package test includes the new migration guide; release navigation
  orders 0.6 before 0.5. Handler and fixed-argument adapter examples execute in tests.
- Restored the demo's native feature matrix serially with `mix deps.compile emerge --force`.

No package version bump or demo source changes. Demo diff/status are unchanged;
`main` remains untouched. Logs: `/tmp/emerge-event-docs-{ci,rust,build,demo-restore}.log`.
