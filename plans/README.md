# Plans

Last updated: 2026-08-29.

This directory contains only open implementation plans and durable design/research
notes. Completed implementation logs belong in Git history, the changelog, tests,
or `guides/internals/`, not under an `active-` filename.

## Active implementation plans

### `active-headless-grayscale-output.md`

Gray4 and Gray8 expansion on the accepted direct-Gray8 raster foundation. BW1
and Gray2 implementation and Trellis qualification are complete.

### `active-linux-gpu-qualification.md`

Remaining hardware and publication gates for the implemented Linux rendering and
PRIME stack: four-route Wayland PRIME, OpenGL explicit sync, Vulkan lifecycle,
DRM/Vulkan target proof, and Vulkan-only dependency cleanup.

This is the sole plan for general backend/Vulkan/headless PRIME qualification.

### `active-low-resource-animation-smoothness.md`

Remaining constrained-device work after animation correctness, transform-only
payload reuse, partial attr preparation, and combined refresh traversal landed.
It focuses on target re-baselining, registry geometry updates, safe patch
coalescing, and final cadence validation.

### `active-rpi5-camera-60fps.md`

Authoritative pinned-RPi5 Camera plan. It combines production NV12 validation,
the XRGB candidate decision, active-scene GPU work reduction, exact ownership,
and the 60 FPS / 30% headroom gates.

## Durable references

### `headless-low-memory-grayscale-investigation.md`

Pinned-Skia and local-probe evidence for direct opaque Gray8 rendering, memory
estimates, alpha behavior, dithering direction, and remaining asset/cache risks.

### `layout-caching-roadmap.md`

Long-term retained-layout/cache roadmap. It is not a single active implementation
slice.

### `layout-caching-engine-insights.md`

Cross-engine research from Taffy, Yoga, Flutter, Slint, Iced, and Servo.

### `platform-runtime-architecture-differences.md`

Reference comparing Linux actor-backed runtime orchestration with macOS host
session orchestration.

### `skia-ddl-paint-layer-note.md`

Reference on possible future Skia DDL/picture recording for paint-layer work.

## Consolidation performed

The following implemented plans were removed rather than kept as perpetual
hardware-validation logs:

- backend/renderer unification
- combined render/registry traversal and tree-walk cleanup
- transient enter-animation completion
- DRM physical-framerate cadence correction
- DRM GLES2 compatibility
- semantic paint-layer simplification
- executed cross-repository commit sequencing

The following overlapping plans were consolidated into
`active-linux-gpu-qualification.md`:

- Linux headless PRIME output
- headless PRIME explicit synchronization
- truthful Vulkan PRIME allocation sizing
- broad Vulkan rendering API implementation/qualification
- generic VideoInterop migration and shutdown rollout notes owned primarily by
  the external VideoInterop repositories

The following overlapping Camera plans were consolidated into
`active-rpi5-camera-60fps.md`:

- V3DV NV12 interop qualification
- Camera-native packed RGB interop
- Camera semantic-layer/performance work

Implemented details remain discoverable through Git history, tests,
`CHANGELOG.md`, and `guides/internals/`.

## Maintenance rules

- Use `active-*.md` only for work with concrete unfinished implementation or
  acceptance gates.
- Remove an active plan when implementation is complete; target smoke testing
  alone is not enough reason to retain a large design log indefinitely.
- Consolidate plans that share one implementation path, target, or acceptance
  matrix.
- Keep completed measurements only when they define the baseline for remaining
  work.
- Prefer links to authoritative cross-repository plans over copying their full
  lifecycle design here.
- Keep plans concise and update status when code lands.
