# Animation/headless rebase validation

Source: `source-sha256:8cfca2fac4d83c3573e6496e46aff586cc21f7ee59efd126a37d3e6f52221626`. Base: `39997f0`; rebased branch HEAD: `84906f7`.
Implementation remains uncommitted. Five planning commits replay identically
(`range-diff.txt`). The full pre-rebase work survives in the retained safety stash;
`identity.json` records its identity and the additional patch/index/untracked backup.

- Rust: **1392 units + 14 integration**, debug and CI release, pass.
- Standalone Mix: **520 tests/doctests**, 9 excluded.
- Full CI: **526 tests/doctests**, 3 excluded; Dialyzer **0 errors**.
- Clippy benches/tests/`bench-diagnostics` with denied warnings, fmt and diff checks pass.

Logs: `cargo.log`, `mix.log`, `ci.log`, `clippy.log`; initial compiler collisions in
`compile-before.log`. `restored-work-audit.txt` verifies preservation of all saved
files/symlinks before the final reporting edits. `source.sha256` includes the newly
split Mix helpers and build workflow/support sources as well as native/Elixir/tests.

The added 18-case regression covers paragraph inline decoration under dimension
animation, inherited alignment changes, reparenting, failed publication/retry,
ghost owner remapping before reflow, terminal/cleanup and cached/fresh/old raster
replay. Existing upstream paragraph and animation tests are both retained.

No performance, new release-target or physical presentation claim follows from
these historical results. The original rebase checklist remains in Git at `6b2c4aa`.

## Collisions and review

| Area | Result |
|---|---|
| Changelog | Only textual conflict. Retained both branches' entries and animation's macOS protocol 14, not upstream's historical 13. |
| Upstream inline-animation test | Compile collision: animation API now needs mutable runtime access and returns a fallible transaction. Updated the test to the current API; retained its paint-only versus reflow assertions. |
| Upstream renderer test | Compile collision: scene wrapper omitted animation's `fonts` and `images`. Preserved both snapshots from the source scene rather than replacing them with live bindings. |
| Paragraph alignment | Upstream inherited-text-alignment resolve key remains intact; animation contexts already carry full inherited font/alignment state. Combined suites and directed alignment/transport tests pass. |
| Inline decoration layout/ghosts | New `paragraph_boxes` reset with cold native layout state and remap owners during ghost cloning. Added 18 schedules across Full/Active/Dirty, two scales and three alignments, exercising animation/reparent/retry/terminal cleanup and retained raster replay. No additional functional collision reproduced. |
| Borders/shadows/render hashing | Retained upstream per-corner radii, full interior shadow painting and matching hashes/bounds. Existing upstream raster/cache tests and animation replay tests pass together. This visual change is intentional, not rolled back. |
| Mix/native packaging | Retained split helpers, target mapping, musl flags, external-resource invalidation and recursive native source packaging. Source-built NIF, package/config tests and CI pass; no animation API/tag was overwritten. |


Upstream also changed the default payload budget from 16 to 64 and added inline-box
storage; pre-rebase performance results do not qualify the combined tree. Later
[closeout measurements](../../../shared-animation-closeout/performance/table.md)
provide a new disclosure, not a controlled speedup or physical-platform pass.
