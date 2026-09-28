# Content-sized El child-box regression

Scope: Emerge layout only; the demo's focus-glow styling and unrelated DRM work are unchanged.

## Completed

- Added a failing native regression: an empty 28px child with 12px parent padding and 1px borders produced a 28px wrapper instead of 54px.
- `resolve_el_children` now measures resolved child border boxes, not descendant content extents. Removed the redundant scroll-axis plumbing; active scrolling axes already used child border boxes.
- Native coverage includes default/explicit content sizing, row/column placement, both axes, square/rectangular fixed children, empty/checked/overflowing text, warm caches and scales 1/1.5/2.
- Public UI/NIF integration coverage verifies incremental empty/checked/overflowing text patches, stable child/sibling positions, warm caches, and pixel equality against explicitly sized buttons at all three scales.
- Reran the original todo reproduction and inspected `/tmp/todo-original.png`: the circle is whole; both toggle states reserve 81×81 physical pixels at scale 1.5 (54×54 logical pixels).

## Validation

`EMERGE_SKIA_BUILD=1 ./ci-tests.sh all` passed:

- Formatting, warnings-as-errors compilation, strict Credo and Clippy.
- ExUnit: 544 passed (15 doctests, 529 tests); four opt-in hardware tests excluded.
- Rust: 1476 unit and 14 integration tests passed.
- Dialyzer: zero errors.
- `git diff --check` passed.

Full log: `/tmp/emerge-child-box-ci.log`. The demo must restart to load the rebuilt native library.
