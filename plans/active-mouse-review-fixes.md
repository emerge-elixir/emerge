# Mouse review fixes

Work in the feature-branch worktree; preserve existing edits to CHANGELOG.md,
guides/migrations/0.6.md and test/emerge/documentation_config_test.exs.

- [x] Preserve normal focus-loss and keyboard cleanup when cancelling capture.
- [x] Drop capture when its source no longer has usable interaction geometry.
- [x] Fold both behavioral regressions into existing tests.
- [x] Replace private coordinate/slot tests with native input-to-event coverage;
  remove redundant helper/codec and documentation-implementation assertions.
- [x] Keep delayed replay/hover, viewport wrapping, validation and wire contracts;
  assert delivered events instead of private pointer tracker state.

Validation:
- `EMERGE_SKIA_BUILD=true ./ci-tests.sh test`: passed, including full_sweep;
  1,663 Rust tests and 732 ExUnit tests/doctests, 15 hardware exclusions.
- `EMERGE_SKIA_BUILD=true ./ci-tests.sh quality`: passed formatting, strict
  compilation, Credo and Clippy.
- `git diff --check`: passed.

Net reduction: 849 lines across test files (plus eight test module declarations).
Existing changelog, migration and documentation-config test edits are untouched.
