# Mouse input commit sequence

Scope: `/workspace/emerge-mouse-local-coordinates`, branch
`feat/mouse-local-coordinates`. Do not commit or modify the demo repository.

- [x] Preserve an exact snapshot of current files before staging.
- [x] Commit logical mouse coordinates, transport, docs and native-input coverage.
- [x] Commit real buffered cursor replay and its regression.
- [x] Commit explicit captured-drag API, codecs, docs and regressions.
- [x] Commit invisible/collapsed scrollbar listener safety and regressions.
- [x] Commit authoritative runtime hover ownership and stale-snapshot regressions.
- [x] Commit implementation/validation notes, verify the final tree matches the
  preserved snapshot, and report commit IDs.

Validate intermediate Rust compilation and formatting without rewriting the
working files. Full tip validation was already recorded in the hover-fix plan.

## Result

Five code commits were assembled without rewriting the working files:

- `373c837` logical mouse coordinates and transport/routing coverage.
- `c2689ed` real buffered cursor replay and native/NIF regression.
- `bcf775a` captured drag API and protocol-19 lifecycle payloads.
- `74cc44d` invisible/collapsed scrollbar interaction safety.
- `86f9a7e` authoritative hover ownership after leave.

Every intermediate code snapshot passed formatting and its full Rust event test
subset (332, 333, 336, 342 and 344 tests respectively). Coordinate and drag
macOS codec subsets passed; relevant ExUnit/NIF/viewport suites passed at each
stage (53, 2, 58, 2 and 60 cases). The final code exactly matches the previously
validated working snapshot; full-tip CI results remain recorded in the hover plan.

Implementation notes are committed separately. Demo tracked/untracked content
was preserved and no demo commits or main-branch merges were made.
