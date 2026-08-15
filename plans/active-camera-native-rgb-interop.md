# Active plan: Camera-native packed RGB interop

Status: host implementation complete; pinned-RPi5 qualification pending. NV12 planar remains the production path until every target promotion gate passes.

## Implementation status

- Complete: persistent generic packed-image imports for RGBA/BGRA, truthful allocation-size checks, exact topology identity, bounded idle-only eviction, synchronization, and separate diagnostics.
- Complete: strict libcamera `XRGB8888`/`XR24` production and mock paths with one-object/one-plane validation, explicit linear modifier, Rec.709 RGB/full semantics, analysis-stream coexistence, and lifecycle coverage.
- Complete: Emerge `XR24` admission as Vulkan `B8G8R8A8_UNORM` / Skia `BGRA8888` / opaque alpha, persistent reuse, exact stream validation, and renderer format inventory.
- Complete on host: explicit `LinearBufferToOptimalBgra` fallback for V3DV: persistent packed DMA-BUF texel-buffer imports, bounded persistent optimal BGRA outputs, exact byte-copy/alpha shader, early source release, ordinary Skia images at arbitrary z-order, and fail-closed capability selection. Pinned-RPi5 pixels, validation, safety, and performance remain unqualified.
- Complete: Camera `auto|nv12|xrgb8888` selection, renderer-inventory gating before capture, immutable propagation through drained stream incarnations, diagnostics, and fail-closed forced mode.
- Pending hardware only: Phase 0 topology/allocation attestation, pixel oracles, validation-layer/MMU checks, A/B/A performance, fault/soak qualification, and promotion decision.

## Goal

Test whether PiSP packed RGB can eliminate Emerge's NV12 staging and YUV conversion.
Prefer direct sampled import when the producer modifier is sampleable; otherwise use one
bounded persistent linear-buffer-to-optimal-BGRA compute copy. Neither path may add CPU
copies, EGL/GL interop, per-frame Vulkan memory imports, unsafe transfer reads, or forged
allocation assumptions.

The primary candidate is libcamera `XRGB8888` (`XR24`) imported as Vulkan
`B8G8R8A8_UNORM` and wrapped by Skia as `BGRA8888` with opaque alpha. This is also
the format already used by the pinned DRM/KMS Vulkan presenter, so changing Emerge's
scanout format is not part of the first experiment.

## Why this candidate

The pinned Raspberry Pi libcamera/PiSP code supports `RGB888`, `BGR888`,
`XRGB8888`, and `XBGR8888`; its default viewfinder format is `XRGB8888`. The current
Emerge DRM Vulkan presenter already intersects KMS `XRGB8888`, GBM
`SCANOUT|RENDERING`, and Vulkan `B8G8R8A8_UNORM` modifier support.

Candidate order:

1. `XRGB8888` / `XR24` / Vulkan `B8G8R8A8_UNORM` / Skia `BGRA8888`;
2. `XBGR8888` / `XB24` / Vulkan `R8G8B8A8_UNORM` / Skia `RGBA8888`, only if the
   active V3DV device cannot directly import the first candidate;
3. 24-bit `RGB888`/`BGR888` only if 32-bit bandwidth is the measured blocker and
   V3DV proves a legal direct sampled import. Do not add a 24-to-32-bit conversion
   merely to call the producer output RGB.

Alpha-bearing Camera formats are unnecessary. The `X` byte is ignored by importing
with opaque alpha semantics; it must never be interpreted as straight or premultiplied
alpha.

At 2560x1440, NV12 is 5.53 MB/frame and XRGB8888 is 14.75 MB/frame. At 60 FPS,
Camera output writes rise from roughly 332 MB/s to 885 MB/s. The experiment is useful
only if removing the NV12 conversion/staging and reducing GPU work outweighs that
extra PiSP/memory traffic.

## Non-negotiable contracts

- Keep `buffer_count: 10`, `max_in_flight: 4`, newest-pending/current/bounded-retired
  behavior, target-before-capture startup, and page-flip scanout authority.
- Keep one `SYNC_FD` acquire fence per frame and return ownership to
  `VK_QUEUE_FAMILY_EXTERNAL` from `GENERAL` before releasing the Camera lease.
- Require one object, one plane, explicit linear modifier `0`, exact pitch/offset,
  and the complete DMA-BUF allocation size for packed Camera frames.
- For direct images, pass object size into Vulkan image creation and reject an allocation
  smaller than `vkGetImageMemoryRequirements*`. For staged packed input, validate the exact
  packed span and imported Vulkan buffer requirement instead. Never pad the descriptor or
  weaken requirements.
- Cache imported packed images by stream incarnation, device/inode, object size,
  FourCC, modifier, dimensions, offset, and pitch. Active reuse or topology collision
  fails closed; eviction is idle-only and bounded.
- After warm-up, Camera's ten buffers must not cause per-frame image/memory import.
- Never pool a semaphore after Skia accepts it. Uncertain queue, Ganesh, device, or
  external-ownership state is terminal and quarantined.
- Preserve top-left orientation and opaque-alpha composition.
- Explicit Vulkan and forced packed-RGB modes fail closed. They never fall back to CPU
  upload, EGL/GL, software Vulkan, Camera-side NV12, or a different packed format.
- NV12 planar remains an explicit rollback path, not a runtime fallback after a stream
  has opened.

## Color contract

libcamera adjusts RGB streams to `YcbcrEncoding::None` and full range. A requested
Rec.709 Camera stream should therefore negotiate:

- Rec.709 primaries;
- Rec.709 transfer;
- RGB/no YCbCr matrix;
- full range;
- unspecified chroma location;
- opaque alpha.

Keep limited-range Rec.709 plus explicit chroma siting for NV12. Make libcamera and
Elixir validation format-aware rather than relaxing either contract. Emerge must use
UNORM images and perform no implicit sRGB or YUV transform on packed RGB bytes.

Promotion requires byte-exact RGB oracle results. Channel swaps, limited/full-range
mistakes, transfer changes, or relying on the value of the `X` byte reject the path.

## Phase 0: prove the target facts first

Add a forced, qualification-only `XRGB8888` lane and collect these facts before broad
refactoring:

- libcamera format inventory contains `XRGB8888` at the requested size;
- strict configuration remains exactly 2560x1440, `XR24`, modifier `0`, one plane,
  and ten buffers at 60 FPS;
- primary XRGB plus the existing small NV12 PiSP analysis stream is accepted;
- reported stride, frame size, object size, and plane span are stable for all ten
  buffers;
- the selected V3DV device advertises linear external DMA-BUF import for
  `B8G8R8A8_UNORM` with the exact usage Ganesh needs;
- every real Camera allocation meets the resulting Vulkan memory requirement;
- validation layers and `dmesg` report no V3D/MMU fault.

If Camera allocations are too small for the truthful Vulkan requirement, stop the
direct-import branch. Never forge size metadata.

Target/source investigation rejected non-linear producer buffers as a direct-import
escape hatch on the pinned stack. Libcamera and its Rust binding can accept
application-owned DMA-BUF `FrameBuffer`s, but the RPi pipeline queues them through
`V4L2_MEMORY_DMABUF`, whose buffer/plane API carries fd, offset, and length but no DRM
modifier. The PiSP back-end programs output addresses from the DMA address plus the
negotiated linear bytes-per-line layout; its RGB formats expose no V3D UIF output mode.
V3DV, conversely, advertises `SAMPLED_IMAGE` for `B8G8R8A8_UNORM` only with optimal/UIF
tiling, not linear modifier `0`. Supplying a UIF allocation would therefore make PiSP
write linear pixels into tiled storage and corrupt the image. PiSP wallpaper/SAND and
PiSP raw-compression layouts are not V3D UIF.

The implemented alternative does not use a Vulkan transfer source or TFU read. It imports
the truthful Camera allocation as an `R32_UINT` uniform texel buffer, reads only bounded
packed bytes in compute, and writes through an `R32_UINT` storage view into an
importer-owned mutable optimal `B8G8R8A8_UNORM` image. The ignored X byte is forced to
255. The source returns to `QUEUE_FAMILY_EXTERNAL` as soon as the compute submission
completes; the bounded optimal output remains owned through arbitrary-z-order Skia
composition and renderer retirement. This is staged RGB rather than direct interop and
must be measured against NV12 planar without silent production promotion.

## Phase 1: generic VideoInterop packed-image support

In `video_interop`:

- replace the RGBA-only direct constructor with an explicit packed-image format
  (`R8G8B8A8_UNORM` or `B8G8R8A8_UNORM`);
- require source allocation size for every direct packed import;
- query modifier plane count, sampled/transfer features, external-memory import, and
  dedicated-allocation requirements for the selected format and exact usage;
- preserve external queue-family acquire/release and one-shot sync-file ownership;
- add pure Rust tests for format mapping, size rejection, one-plane layout, channel
  order, and capability failures.

Do not add new per-frame NIF calls. Capture and release remain on the existing native
service/dispatcher threads; BEAM schedulers must not block on fences or camera I/O.

## Phase 2: libcamera producer support

In `membrane_libcamera`:

- extend the primary DMA-BUF format contract from NV12-only to `NV12 | XRGB8888`;
- keep raw byte output and the analysis stream NV12-only initially;
- make even-dimension, chroma-location, plane-count, pitch, color-space, and FourCC
  checks format-specific;
- validate the negotiated RGB color space as Rec.709/Rec.709/RGB/full;
- preserve exact native object size, modifier, offset, pitch, sync fence, lease, and
  shutdown behavior;
- extend the mock backend to emit truthful one-plane XRGB descriptors and add lifecycle,
  delayed-fence, malformed-layout, and finalization tests.

The existing NV12 analysis stream remains independent. A primary XRGB stream must not
change its format or color contract.

## Phase 3: persistent Emerge direct import

In Emerge:

- admit `XR24` as one-plane opaque packed RGB and map it to Vulkan
  `B8G8R8A8_UNORM` plus Skia `BGRA8888`;
- retain `AB24`/RGBA compatibility, but do not pretend `XR24` is `AB24`;
- add a bounded persistent packed-source cache with the same incarnation and topology
  collision rules as NV12;
- use `AlphaType::Opaque`, `SurfaceOrigin::TopLeft`, and no color conversion effect;
- acquire external content from `GENERAL`, wait on the exact Camera `SYNC_FD`, compose,
  release to external, and retire the source lease only after the release fence proves
  completion;
- report packed cache imports/hits/misses, active reuse rejection, object-size failure,
  composition timing, and lease-release timing separately from NV12 conversion stats;
- preserve the last valid displayed frame if a candidate frame fails before uncertain
  ownership; quarantine after uncertainty.

Expose the renderer's exact PRIME input inventory in renderer readiness information,
not only `prime_video: true`. Camera format selection must intersect producer and
renderer facts before capture starts.

## Phase 4: Camera selection and rollback

Add an internal qualification switch such as:

```text
CAMERA_VULKAN_CAPTURE_FORMAT=auto|nv12|xrgb8888
```

- `nv12` keeps today's planar path;
- `xrgb8888` is forced and fail-closed;
- `auto` remains NV12-first until XRGB passes all target gates, then may prefer XRGB
  only when both libcamera and Emerge advertise the exact contract.

The chosen format is immutable for one stream incarnation. Changing it requires the
normal drained stop/reopen path. OpenGL remains an explicit rollback configuration,
not a fallback from failed Vulkan RGB startup.

## Emerge output-format decision

Keep DRM scanout as KMS `XRGB8888` imported as Vulkan `B8G8R8A8_UNORM` for the first
qualification. It is already the proven KMS/GBM/V3DV intersection and directly matches
the preferred Camera format.

Do not switch scanout to:

- ABGR/RGBA solely to match an alternate Camera byte order; Vulkan sampling already
  presents logical RGBA channels without a full-frame conversion;
- RGB565, because it violates current color precision;
- ten-bit packed output, because Camera, Skia blending, KMS support, and the pixel
  contract would all need separate qualification;
- NV12, because UI composition requires an RGB render target.

Only reopen scanout format selection if target modifier/capability evidence proves
XRGB Camera import impossible and another KMS format gives a measured end-to-end win.

## Validation sequence

1. Host tests in dependency order: VideoInterop Rust/ExUnit, MembraneLibcamera mock
   Rust/ExUnit, Emerge DRM-Vulkan Rust/ExUnit, then Camera tests.
2. Cross-build AArch64 NIFs in isolated target directories and restage them last.
3. On the pinned RPi5, run forced XRGB startup and ten-buffer topology/allocation
   attestation before rendering a long sample.
4. Run generated packed-DMA-BUF channel/range/stride fixtures, then Camera/PiSP pixel
   oracles. Use a paired NV12/RGB qualification stream or a locked sensor test pattern
   when available.
5. Run idle NV12 A, forced XRGB B, and closing NV12 A with identical firmware, controls,
   scene, clocks, validation settings, and duration.
6. Repeat the winning candidate on the active Camera Focus scene and through hide/show,
   delayed/error fences, source reuse, stale generations, shutdown, hotplug, and device
   loss.
7. Run a long FD/RSS and cache-cardinality soak, followed by OpenGL rollback.

## Promotion gates

Promote XRGB only if all are true:

- capture and presentation sustain 60 FPS in the active Focus scene;
- authoritative GPU busy time leaves at least 30% headroom (use the existing
  <=10.86 ms active-scene gate, not additive cross-submission latency intervals);
- conversion submissions and staged-output writes are zero;
- no more than one persistent import per Camera buffer occurs after warm-up;
- exact RGB pixels, orientation, range, transfer, and channel order pass;
- no credit drops, active reuse, topology collisions, synchronization errors,
  `EBUSY`, missed vblanks, validation callbacks, V3D/MMU faults, quarantine, or device
  loss occur;
- lease release remains bounded and `buffer_count: 10` / `max_in_flight: 4` never
  starve capture;
- FD/RSS and cache cardinality remain bounded through the soak and shutdown is
  deterministic.

If XRGB misses any correctness gate, retain NV12 planar. If it is correct but not
materially faster, retain NV12 because it uses 62.5% less Camera output bandwidth.
