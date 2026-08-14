# Paint-layer model simplification

## Problem

Paint-layer topology currently depends on transient damage state and a collection of recursive eligibility checks. `split_paint_layer_content_owned/1` then discovers nested boundaries from an already flattened render-node list and keeps only the prefix before the first child layer. This makes topology vary with the last update, leaves later static content direct, and encourages scene-specific exceptions.

## Target model

Paint layers are stable semantic objects produced directly by tree construction. Damage changes a layer generation; it never creates or removes the layer.

The builder recognizes only these boundary kinds:

- root composition;
- Nearby mount;
- scroll content;
- declared transform/alpha animation;
- slider value overlay;
- direct live/external media.

Each component builder returns a layer tree directly rather than returning a flat node list that is scanned and split later. A layer has:

- static own paint commands;
- semantic child layers with explicit composition order;
- stable identity, bounds, placement, policy, and generation.

Unsupported/custom shapes remain ordinary in-flow content; they do not cause new inferred layers.

## Camera contract

The normal Camera tree deterministically produces:

1. one direct root layer containing background and Video;
2. one top Nearby layer;
3. one bottom Nearby layer containing panel background, border, labels, buttons, and static slider tracks;
4. six slider value layers containing filled track and thumb.

The topology is the same on initial render, clean refresh, slider interaction, and camera-frame redraw. Video never enters a cacheable payload.

## Refactor

1. Add structural tests for semantic layer topology before changing implementation.
2. Introduce a small layer-tree builder result used by root, Nearby, Slider, scroll, and declared animation builders.
3. Move slider slot partitioning into the Slider renderer: track is parent paint; filled track and thumb are the slider value child layer.
4. Make root and Nearby builders establish their boundaries directly.
5. Replace damage-created dynamic boundaries with generation invalidation of existing semantic layers.
6. Remove `MovingBoundaryRequirements`, media-poison propagation, damage-only layer wrapping, media order workaround, and flat-node paint-layer splitting.
7. Keep renderer cache admission generic over the resulting semantic layer tree.
8. Validate direct versus cached pixels for clipping, transforms, alpha, Nearby order, scroll, focus, custom sliders, and live Video.

## Progress

- [x] Replaced flat own-prefix/child-tail storage with ordered scoped content and per-run cache slots.
- [x] Made Root an unconditional `DirectOnly` semantic layer.
- [x] Made mounted Nearby roots unconditional `Cacheable` semantic layers.
- [x] Added generic three-slot Slider partitioning: track remains parent-owned; fill and thumb share one `SliderValue` layer.
- [x] Added local `DirectMedia` layers for Video under cacheable semantic owners while root-owned Video remains direct.
- [x] Added structural ScrollContent and declared Animation layers with scroll-offset normalization outside payload generation.
- [x] Removed dirty-created boundaries, recursive media/focus requirements, the Slider media workaround, and arbitrary `RenderLayerCache` storage.
- [x] Added exact nine-layer Camera-like topology, ownership, dirty/clean, slider-local generation, and camera-only tests.
- [x] Keyed cached payloads by exact own-run content rather than the enclosing layer generation, so one changing label/value run does not invalidate static sibling runs.
- [x] Added GPU replacement probation, one-replacement-per-frame staggering, and latest-only run-family residency so interaction draws transient versions direct instead of creating bursts of short-lived render-target snapshots.
- [x] Validated steady both-on RPi5 cadence at 60.2 FPS with zero missed vblanks.
- [x] Confirmed the real Camera scene has exactly nine semantic layers during active interaction.
- [x] Coalesced Camera's exposed requested-control text/state to 20 Hz while retaining native slider motion and the independent camera-control debounce; this reduced five-second patch batches from 506 to 85 and restored camera completion to 60 FPS.
- [x] Hardware disproved exact active-clip deduplication as the dominant fix: 56–85 clip shapes were skipped on sampled frames without improving active cadence, so the optimization was removed.
- [x] Added reproducible exact Camera active-shutter and active-focus Criterion fixtures, including the focused interaction style observed on hardware, and restored the renderer benchmark suite to current semantic layer names.
- [x] Fixed the renderer Criterion gate to finish asynchronous cache-setup rendering before timed iterations and verified direct RX 7900 XTX access reports `radeonsi, navi31` rather than llvmpipe.
- [x] Rejected scope-isolated run splitting after alternating corrected RX 7900 results were neutral and non-repeatable; restored broad own-run coalescing without another RPi firmware cycle.
- [x] Added bounded sampled-frame paint-run diagnostics with semantic id/role, slot, cache outcome, bounds, primitive summary, and CPU duration.
- [x] Revalidated broad-coalescing on a fresh RPi5 idle/active/recovered sequence: idle was 59.2 FPS with 4 missed vblanks, focused Focus-distance interaction was 50.6 FPS with 47 misses and a 15.5 ms sampled GPU render elapsed, and recovery was 60.0 FPS with no misses and a 10.0 ms GPU render elapsed. Camera completion remained 60 FPS and no GBM/fence starvation occurred.

## Acceptance

- Layer topology never changes solely because dirty flags changed.
- The Camera fixture always reports the exact nine semantic layers.
- Warm camera frames render direct Video and composite retained top/bottom/slider payloads without direct static UI primitives.
- Generic paint order and alpha grouping match uncached rendering pixel-for-pixel.
- The refactor removes more boundary/splitting code than it adds.
- Default/no-default Rust tests and Clippy, Mix tests, formatting, and RPi5 cadence validation pass.
