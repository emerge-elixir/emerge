# Animation onto headless-backend — completed

Worktree: `/workspace/emerge-animation`, branch `plan/shared-animation-core`.
Rebased five planning commits from `592c2fe` onto local `headless-backend` `39997f0`;
new HEAD `84906f7`. Range-diff confirms all five patches are unchanged. All
uncommitted animation implementation and artifacts were restored. They subsequently
landed in the [commit sequence](animation-commit-sequence.md). No push; the headless
worktree was not modified.

- [x] Back up, rebase and restore the complete worktree; resolve textual conflicts.
- [x] Inspect overlapping paragraph/layout/render/cache and build/config changes.
- [x] Add directed regression coverage; run Rust, Mix, Clippy and full CI.
- [x] Record remaining integration/qualification risks.

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

## Remaining risks

- Upstream changed default payload creation budget **16 → 64** and added per-node
  inline-box storage. Existing animation benchmark/memory results are historical,
  not qualification of this combined tree; new locked measurements remain needed.
- New musl/RISC-V targets and real macOS/device presentation were not exercised.
  Release artifacts/checksums must be generated from matching animation code
  (EMRG 9/tag 84, macOS protocol 14), not older headless-only binaries.
- Directed green tests do not close the broader P1–P9 matrix, delayed registry-install
  races or platform gates. No unresolved textual/compile conflict remains; this is
  not a claim that every possible semantic interaction has been qualified.

## Validation and recovery

**1392 Rust units + 14 integration**, standalone Mix **520** (9 excluded), full CI
**526** (3 excluded), Dialyzer **0**; denied-warning Clippy, fmt and diff checks pass.
Evidence: `plans/artifacts/shared-animation-remaining/validation/headless-rebase/`.
Source: `source-sha256:8cfca2fac4d83c3573e6496e46aff586cc21f7ee59efd126a37d3e6f52221626`.

Safety stash retained: `4697539d5e77935d177be9963f1790f2f8b77dd5`.
Additional backup: `/tmp/emerge-animation-rebase-20260914-231941` (original index, binary patch, untracked archive and refs).
Original committed history is also protected by the pre-rebase backup branch.
These are recovery artifacts, not instructions to overwrite current work.
