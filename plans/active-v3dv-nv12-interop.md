# Active V3DV NV12 interop implementation

Status: host implementation complete; RPi5 correctness and performance qualification remain hardware-gated.

## Goal

Make the pinned V3DV Camera path safe and fast without CPU upload, EGL/GL interop, unsafe TFU reads, silent fallback, weakened allocation checks, or changed Rec.709/left-sited pixels.

## Implementation

- [x] Add nonblocking per-lane Vulkan timestamps for conversion and post-Ganesh completion.
- [x] Cache imported NV12 source buffers by stream incarnation, `(st_dev, st_ino)`, and exact topology; reject active reuse and topology collisions.
- [x] Pool bounded output images, descriptors, command pools/buffers, imported semaphores, fences, and timestamp queries.
- [x] Fence the staged source release separately and retire its canonical lease as soon as the exact external-ownership return completes.
- [x] Optimize the RGBA fallback as one invocation per 2x2 luma block.
- [x] Prefer TFU-free optimal `R8_UNORM`/`R8G8_UNORM` staging when the active device proves sampled, linear-filter, and storage support.
- [x] Compose staged planes through an Emerge-owned raw-child Skia RuntimeEffect with explicit range, BT.709 matrix, and left/center/top siting coordinates.
- [x] Keep the RGBA staged path as the truthful capability fallback and direct image paths unchanged.
- [x] Add host cache-identity, timestamp-wrap, capability, SPIR-V, chroma-coordinate, and RuntimeEffect compilation tests.
- [x] Extend diagnostics/statistics, add opt-in authoritative debug-utils validation counting, and update architecture documentation.
- [ ] Complete byte-exact target pixel-oracle fixtures and rollback/performance qualification on the pinned RPi5.

## First pinned-RPi5 runtime evidence

The first working planar run sustained 300 submitted and released capture frames at
60.0/s with zero no-credit drops, active-reuse rejections, synchronization failures,
quarantines, device loss, `EBUSY`, or missed vblanks. At idle the renderer presented
299 frames in 5.001 seconds (59.8 FPS), imported 286 video frames, and released leases
in 17.227 ms average. This proves the historical source-buffer requirement failure and
four-credit/144-ms lease bottleneck are absent in the running path.

During slider motion capture and lease release remained 60.0/s with zero no-credit
drops, but presentation fell to 48.6 FPS and newest-pending replacement rose to 90.
CPU render averaged 4.042 ms while present submission averaged 12.933 ms; the remaining
active-scene limit is renderer/GPU/presentation pacing rather than producer credit
collapse. The timestamp currently called `composition` spans the interval between the
conversion completion query and the later release submission, so overlapping queue
work means it is a latency interval, not an additive GPU-utilization/headroom measure.

## Forced-RGBA pinned-RPi5 evidence

A forced persistent-RGBA idle run is already decisively slower than the first planar idle
run. Renderer presentation fell from 59.8 to 43.4 FPS (-27.4%), and video presentation
fell from 57.2 to 39.2 FPS (-31.5%). Conversion increased from 6.364 to 13.972 ms
(+119.5%), composition latency from 25.569 to 32.955 ms (+28.9%), total video GPU latency
from 31.933 to 46.927 ms (+47.0%), and average lease release from 17.227 to 27.320 ms
(+58.6%). The RGBA run submitted capture at 60.0/s but replaced 104 newest-pending frames.

This is not source-cache, output-pool, CPU-render, capture-credit, synchronization, or KMS
failure: the run had ten stable source entries, three persistent output slots, no evictions,
busy rejections, active-reuse rejections, topology collisions, no-credit drops, `EBUSY`,
missed vblanks, quarantine, or device loss; CPU render averaged 2.270 ms. The measured
cost is intrinsic to materializing and consuming a 14.75 MB RGBA output rather than the
5.53 MB planar output. Keep planar preferred on the pinned RPi5. Do not spend target time
on an active-scene RGBA run; return to planar for the closing A run. This sample had Vulkan
validation disabled, so it does not satisfy the authoritative validation gate.

## Safety gates

- A cached source cannot be claimed twice.
- Source lease retirement requires the staged acquire fence to prove conversion and external ownership release complete.
- `Surface::wait(..., true)` mismatch, submission uncertainty, device loss, or fence uncertainty quarantines every associated Vulkan object.
- Ganesh-owned ready semaphores are never pooled.
- Output slots return to the pool only after post-Ganesh release completion.
- Cache eviction occurs only for idle entries and is bounded.
- Ordinary frames never CPU-wait or call queue/device idle.
- `TRANSFER_SRC` is not used against unpadded Camera allocations.

## Validation

- [ ] `video_interop`: format, Clippy warnings denied, all Rust tests, all ExUnit tests, SPIR-V validation.
- [ ] `emerge-headless`: format, Clippy warnings denied, `cargo test`, `mix test`, and CI checks where practical.
- [ ] Restage AArch64 artifacts only after host validation.
- [ ] RPi5: exact pixels, zero validation/MMU faults, stable FD/RSS, 300+ captures, 60 FPS and required headroom.
