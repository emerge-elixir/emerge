# Per-renderer asset runtime

## Status

Implemented. Full release validation remains part of the 0.4.0 candidate gate.

## Problem

Native asset source state, worker ownership, decoded raster/vector caches, registered fonts, and related generations are process-global. Starting, configuring, or stopping one renderer can affect another renderer.

## Contract

- Every native renderer owns one independent asset context and worker.
- Configuration, source status, encoded records, decoded caches, vector variants, registered fonts, and diagnostics are scoped to that context.
- Renderer shutdown stops and joins only its own asset worker.
- macOS host sessions own separate contexts even though they share one host process/thread.
- Offscreen renders use a temporary context and cannot alter live renderers.

## Validation

- [x] Run two headless renderers with distinct roots and independent async asset loading.
- [x] Stop one renderer and reload changed content in the other.
- [x] Verify renderer-local registered fonts and offscreen font isolation.
- [x] Bind renderer statistics snapshots to the owning asset context.
- [x] Run the complete `cargo test`, `mix test`, Clippy, docs, and unpacked-package release gates.
- [ ] Validate the external macOS host in exact source-build CI on a macOS runner.
