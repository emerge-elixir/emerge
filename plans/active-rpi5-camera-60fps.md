# Active Plan: RPi5 Camera 60 FPS Qualification

Status: host implementation and initial target proof complete; production NV12 qualification and active-scene GPU headroom remain open

## Goal

On the pinned RPi5 Camera Focus scene require:

- 59.8-60.2 presented FPS;
- median GPU render elapsed <=10.86 ms;
- p95 <=11.67 ms and p99 <16.67 ms;
- exact pixels, interaction state, DMA-BUF ownership, and shutdown;
- no sustained missed-vblank, fence, lease, FD, RSS, or V3D/MMU fault.

This plan consolidates the former V3DV NV12, packed-XRGB experiment, semantic
paint-layer, and Camera stable-60 plans. General Wayland/headless/DRM Vulkan
qualification lives in `active-linux-gpu-qualification.md`.

## Current baseline

| Window | FPS | GPU elapsed | Missed vblanks |
| --- | ---: | ---: | ---: |
| cache-on idle | 59.2 | 8.87 ms | 4 |
| Focus active | 50.6 | 15.52 ms | 47 |
| recovered | 60.0 | 10.01 ms | 0 |
| cache-off active | 25.8 | 33.22 ms | 170 |

The active path must remove at least 4.66 ms. Renderer caching stays enabled.

## Implemented foundation

- Stable nine-layer Camera topology with direct Video and semantic Nearby/slider
  layers.
- Persistent V3DV NV12 source imports, bounded output pools, exact synchronization,
  early source release, and deterministic retirement/quarantine.
- Preferred optimal NV12/separate-plane staging with compute-planar and RGBA
  rollback paths.
- First planar target run sustained 60 capture FPS with no credit collapse;
  active presentation remained about 48.6 FPS.
- Forced RGBA was decisively worse and is not a candidate.
- Host-complete XRGB8888 producer/import support and bounded
  linear-buffer-to-optimal-BGRA fallback.
- Camera `auto|nv12|xrgb8888` selection and exact renderer-format admission.
- Corrective slider registry state, no-op scene suppression, 20 Hz exposed state,
  stable paint generations, and transform-only payload reuse.
- Combined render/registry traversal and paint-layer model cleanup.
- Current timing, scene-selection, cache, video, fence, and lease diagnostics.

## Locked constraints

- Keep direct Video, nine semantic layers, focus styling, exact clip/paint order,
  native immediate thumb motion, 20 Hz publication, and 60 ms camera application.
- Keep `buffer_count: 10`, `max_in_flight: 4`, newest-pending behavior, one
  sync-file acquire fence, external ownership return, and page-flip authority.
- No CPU upload, EGL/GL interop, per-frame Vulkan imports, forged allocation
  sizes, scene-specific renderer IDs, lower resolution, or hidden visual changes.
- RPi5 live DRM measurements are authoritative; desktop Criterion is a guardrail.

## Remaining work, in order

### 1. Freeze an authoritative production NV12 baseline

Capture matching 60-second windows from one binary:

- UI-only;
- video-only;
- combined idle;
- combined Focus-active;
- recovered.

Use three alternating active windows after warmup with fixed firmware, clocks,
cooling, controls, query cadence, and gestures. Reject samples with thermal
throttle, disjoint query, GBM starvation, ownership imbalance, or concurrent
benchmark/build activity.

Instrumentation overhead must remain <=0.25 ms GPU median and <=0.2 FPS.

### 2. Close NV12 correctness and safety gates

For the selected `auto` plan and forced planar rollback:

- pass byte-exact Rec.709/limited-range/chroma-siting pixel oracles;
- prove the selected V3DV transfer or separate-plane path and exact allocation
  attestation;
- run validation/debug callbacks and inspect `dmesg` for V3D/MMU faults;
- exercise delayed/error fences, active-reuse rejection, stale incarnation,
  hide/show, stream replacement, restart, and device loss;
- sustain at least 300 captures, then a 30-minute FD/RSS/cache/lease soak;
- confirm Camera capture/release remains 59-60 FPS during UI interaction.

NV12 planar remains production until every gate passes.

### 3. Decide the XRGB8888 experiment

Run one forced target qualification only after the NV12 baseline is fixed:

- attest exact `XR24`, modifier 0, one-plane topology, pitch/span/allocation, and
  ten-buffer stability;
- verify Rec.709 RGB/full range, channel order, orientation, and opaque alpha;
- prove validation/MMU cleanliness and bounded persistent reuse;
- run NV12 A / XRGB B / NV12 A under identical conditions.

Promote XRGB only if it is exact, reaches the final FPS/headroom gates, and is a
material end-to-end win despite 14.75 MB frames versus 5.53 MB NV12 frames. If it
requires the staged BGRA copy and is neutral/slower, remove the candidate from
`auto` and keep forced mode only if it remains useful for diagnostics; otherwise
remove it entirely.

### 4. Reduce active UI GPU work

The current per-run cache is the baseline. Preserve completed correctness/deletion
work and prototype one isolated `semantic_backing` strategy only if the frozen
baseline still misses 10.86 ms:

- back only outer eligible cacheable subtrees; root and Video stay direct;
- target one top Nearby and one bottom Nearby backing;
- reject unsafe transforms, clips, alpha, direct media, unresolved content, or
  context mismatch as a whole candidate;
- keep exact ordered content manifests and conservative old/new damage union;
- clear damage and replay complete ordered content under the damage clip;
- own bounded GPU surface ring slots with explicit repair/snapshot/draw/flush
  lifecycle and no steady allocation/COW after warmup;
- keep `per_run` as instant rollback until target acceptance.

Required instrumentation:

- backing candidates/rejections, full/partial repairs, damage pixels, replayed
  scopes, primary draws/pixels, allocations, slots, and resident bytes;
- after warmup target <=2 backing draws, <=4 total primary UI payload draws,
  <=0.80 million sampled UI pixels, and damage p95 <=25% of backing area.

Keep the experiment only if paired active median improves >=2.0 ms with a 95%
confidence interval excluding zero, p95 does not regress, idle/recovered regress
<=3%, and exact GPU-readback/lifecycle tests pass.

### 5. Measure only proven residual costs

If a measured deficit remains, test independently:

- sparse border source strips for large transparent cached borders (keep only at
  >=0.75 ms);
- proven opaque root/video prefix folding (keep each change only at >=0.50 ms).

Do not stack neutral micro-optimizations or reopen KMS media-plane promotion; the
current landscape NV12 plus 270-degree scene transform is not a supported plane
rotation path.

## Measurement discipline

- Serialize all builds/tests/benchmarks with `scripts/performance-lock.sh` and
  hold the exclusive lock across deploy/measure windows.
- Record immutable source/snapshot identity, target software versions, renderer,
  modifier, strategy, temperature/throttle state, timing percentiles, KMS gaps,
  draw/pixel counts, and ownership counters.
- Alternate A/B/B/A and reverse on repeat. Never run competing GPU measurements.
- Remove losing branches and flags after the decision.

## Acceptance

- Three accepted active windows meet median/p95/p99 and 59.8-60.2 FPS gates.
- Idle/recovered GPU elapsed regresses <=3%.
- Pixel oracles, screenshots, slider state, nine-layer topology, and direct Video
  remain exact.
- Capture/release stays 59-60 FPS with no credit drop, topology collision,
  `EBUSY`, missed-vblank sequence gap, fence/lease imbalance, quarantine,
  validation callback, V3D/MMU fault, or resource growth.
- A 30-minute target soak and clean repeated shutdown/restart pass.
- One production format path and one documented rollback remain; rejected
  experiments are deleted.

## Validation

```bash
cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features drm
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --all-targets -- -D warnings
mix format --check-formatted
mix test
./ci-tests.sh all
git diff --check
```
