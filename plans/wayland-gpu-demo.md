# Retained cards on Wayland: OpenGL, then Vulkan

**Implemented.** See the [standalone run instructions](wayland-demo.md#run-the-standalone-demo)
and [results and remaining gate](wayland-gpu-results.md). Run the same cards app
with three explicit renderers. **When `:opengl` or `:vulkan` is selected,
only the hardware GPU may draw.** The CPU may compute layout, hits, font metrics, glyph positions
and vector geometry, and encode commands. It must not produce any color or
coverage samples for the frame, even one glyph, mask, tile or intermediate
layer. This applies from startup and cache warm-up through presentation.
`:raster` with `:gpu_upload` is a different mode, never a fallback. Start
with EGL, then add Vulkan image ownership.

## User contract

Keep the 480×240 window, scene, callbacks and no-argument CPU demo unchanged.
Pass the example-only choice into `mount/1`; no new library API, live API
switching, or change to `:auto` or build defaults. Commands:

```sh
cd ../wayland_cards
mix deps.get
WAYLAND_CARDS_API=opengl iex -S mix
WAYLAND_CARDS_API=vulkan iex -S mix
```

| Wayland source build (`compiled_backends`) | Runtime `renderer` | Cargo features, defaults disabled |
| --- | --- | --- |
| `[wayland: [:raster]]` | `[api: :raster, raster_present: :cpu]` | `wayland` |
| `[wayland: [:opengl]]` | `[api: :opengl]` | `wayland,opengl` |
| `[wayland: [:vulkan]]` | `[api: :vulkan]` | `wayland,vulkan,wayland-vulkan-deps` |

Do not pass `raster_present: :cpu` for a GPU API. Both explicit requests failed
at [native selection](../native/emerge_skia/src/session/mod.rs) before this port.
Select the renderer once during bounded startup, before the first frame.
Dispatch the initial bufferless configure without waiting for an app upload.
Report a GPU API as selected only after verifying hardware acceleration on the
actual window.

If the requested API was built but has no supported hardware (including a
software-only driver), release any probe resources and select the existing
Wayland CPU raster / `wl_shm` presenter for the **whole session**. Emit **one
`:warning` per fallback session** through the ordinary renderer log, even with
`diagnostics.logs: []`. Name the requested API, the reason and the selected
renderer, e.g. `Wayland: Vulkan requested; no eligible hardware device; using CPU raster (wl_shm)`.
`renderer_info/1` keeps `requested: :vulkan` (or `:opengl`), reports
`selected: :raster`, CPU capabilities and `vulkan_device: nil`.

Missing build features, invalid config, a missing compositor or a failed probe
that cannot establish device availability remain errors. Once a GPU is selected,
an unsupported draw or device loss is not a reason to mix in CPU painting or
silently switch presenters. Never omit the draw or install a partial frame.

## 1. Share drawing, not pixels

- In [tree/paint.rs](../native/emerge_skia/src/tree/paint.rs), split preparation
  of `Drawing` and hits from the allocation of CPU pixels in `Raster::prepare`.
  Share the ordered command walk, not the CPU raster executor. Only the CPU
  sink may call `rasterize` or wrap host pixel memory. Both GPU sinks clear,
  fill, clip, antialias and blend on device targets; upload geometry and
  uniforms, never CPU-painted pixels. No second scene or layout traversal.
- **Text is the critical case.** A GPU canvas alone is insufficient: Ganesh's
  `GrAtlasManager::addGlyphToAtlas` can upload a host `SkGlyph` image. Keep
  `draw_glyphs_at` on the CPU route. For GPU text, read the bundled font's
  vector outlines and replay them at the retained positions as geometry;
  rasterize coverage on the GPU. Use only a path with no software mask
  fallback (e.g. triangles with hardware multisampling). No CPU bitmap or
  distance-field atlas, including on a cache miss. Audit strike provisioning
  as well as replay: metric queries must not rasterize glyphs as a side effect.
- Keep one EMRG v10 tree and dispatcher. Apply accepted edits, invalidate once
  against the final tree, recompute. The [Wayland loop](../native/emerge_skia/src/session/wayland/mod.rs)
  still owns configure/scale, seat, frame caps, close and the stop wake. Only
  CPU uses shm frame slots. For GPU mode request a compositor-owned cursor,
  not the client's themed shm bitmap fallback. GPU contexts and their destruction
  belong to the display thread, not a Rustler resource or scheduler thread.
- Separate **needs new drawing** from **must submit to this surface**. Remember
  the last successful submission on this surface generation, at this buffer
  extent and scale. Equal drawing plus a still-valid submission permits a
  listener-only install without a swap. A recreated target at the same size
  does not. If a swap is required, fully clear and draw its back buffer or
  acquired image; never assume it contains the previous frame.
- Keep at most one prepared submission. Seal its candidate hits, mapping and
  pending callback routes together before releasing the session lock for a
  potentially blocking driver call. Reuse reserved candidate storage and the
  relay's persistent route map; no borrowed tree pointers across unlock, no
  callback terms in Rust, no public frame/token. Later edits belong to the next
  cycle. Success installs exactly the submitted candidate, leaving newer work
  pending; failure or deferral installs nothing. Neither GPU flush, queue
  submission, frame callback nor initial bufferless commit is presentation.

**Stop:** run the existing headless and shm pixel, input, skip and allocation
controls before adding either GPU driver. Pair native submit tests with a
relay-ordering regression: A is visible, B is prepared, C changes the callback
during B's submit. B failing leaves A's route; B succeeding installs B's,
never C's. C installs on its own turn.

## 2. OpenGL: first GPU window

Adapt the EGL setup in [egl.rs](../native/emerge_skia/src/backend/wayland/egl.rs)
and framebuffer wrapping in [skia_gpu.rs](../native/emerge_skia/src/backend/skia_gpu.rs),
not `SceneRenderer` or `present_rgba_pixels`. Use the live Wayland display and
surface, an alpha-capable EGL window config, current GL/GLES context and Skia
Ganesh. Preserve one-time GL symbol loading. Wrap the actual framebuffer with
its format, sample/stencil counts and bottom-left origin; drawing and input
remain top-left. Resize the EGL drawable and rewrap in configured device pixels.

Flush Ganesh, request the pacing callback for the impending surface commit,
and swap with EGL. EGL owns the buffer attach/commit; do not issue a second
shm or empty commit. Publish only after successful swap and transport handoff.
Handle swap failure or context loss without installing the candidate or waiting
forever on its callback. Do not `glFinish` per frame or assume swap interval
zero guarantees a nonblocking driver. Drop Skia resources while the context is
current, before EGL and the Wayland window. **Stop:** pass the compositor checks
below for GL before porting Vulkan. Verify the current EGL context is hardware
accelerated and record its device and `GL_VENDOR`/`GL_RENDERER`/version. A
software-only driver such as llvmpipe selects the warned CPU raster fallback;
it does not qualify OpenGL.

## 3. Vulkan: a separately qualified presenter

- Adapt [Wayland WSI](../native/emerge_skia/src/backend/wayland/vulkan.rs),
  [acquire pool and tests](../native/emerge_skia/src/backend/wayland/vulkan/acquire.rs)
  and the [Ganesh target wrapper](../native/emerge_skia/src/backend/vulkan/ganesh.rs).
  Do not import the old renderer, video engine or tree actors. Keep the
  hardware-only device policy: require a combined graphics/present queue for
  this `VkSurfaceKHR`; match compositor DRM feedback when available and reject
  ambiguous selection. No eligible hardware device selects the warned CPU
  fallback, not software Vulkan. When Vulkan wins, report that physical device,
  not a fresh probe.
  Check WSI/Ganesh requirements; video import and timestamps are not drawing
  prerequisites. Do not advertise PRIME or other unused capabilities.
- Choose supported FIFO swapchain format/color space, alpha, usage and extent
  (start with BGRA8 UNORM / SRGB nonlinear). Wrap each image with its actual
  descriptor; key ownership by swapchain generation and image index. Use the
  submitted extent for input mapping, or reject an unsupported mapping.
- Acquire without blocking the event loop. `NOT_READY`/`TIMEOUT` keeps the
  request pending; arm at most one cancellable retry if no wake is due. No idle
  tick. Preserve the same-queue `ALL_COMMANDS` acquisition wait **before**
  Ganesh's first layout transition; `SkSurface::wait` alone is insufficient.
  Render, flush to `PRESENT_SRC_KHR`, signal that image's render-finished
  semaphore and have `vkQueuePresentKHR` wait on it. Do not consume acquisition
  twice or recycle a present semaphore just because rendering finished.
- A successful present (including `SUBOPTIMAL`) installs; schedule recreation
  for suboptimal. `OUT_OF_DATE` does not install: retire the generation safely
  and retry the newest accepted tree. Never strand an acquired image on an
  error, leave a phantom frame callback outstanding, or busy-loop on resize.
  Retire acquire slots, queued work **and presentation ownership** before
  destroying borrowed surfaces, semaphores or the swapchain. No device-idle
  per frame. On device loss or unproven retirement, fail/quarantine via the
  session lifecycle; do not unconditionally report successful cleanup.

## Acceptance and release boundaries

**GPU-only gate, per API:** in a fresh process, instrument the CPU raster,
software mask/glyph generation and host-pixel upload paths, including Skia
internals. Check the probes with positive controls. From startup through a cold
text draw, new glyphs, clip, resize and scale change, require zero such calls
for the GPU renderer. Pair that with a device command trace showing clear,
fill and glyph geometry actually drawn on the selected hardware. An unsupported
draw on that device must fail without CPU painting or partial install. A
screenshot or an API name alone proves none of this.

**Fallback gate, per requested API:** on a working compositor with no eligible
hardware, open the same demo, move the card and resize. Assert CPU pixels are
presented via `wl_shm`, `renderer_info/1` distinguishes requested from selected,
and exactly one fallback warning reaches the viewport logger (or the standalone
start caller). Redraw and resize must not repeat it. An explicit CPU start or a
successful hardware start emits no fallback warning. A passing fallback run
never counts as a GPU acceptance run.

1. **Visible demo, per API.** One ordinary viewport on a real/nested compositor.
   Observe compositor pixels, raw `handle_input/2` and routed `handle_info/2`:
   a real seat click at local `(5,5)` moves the card and increments once; old
   spot misses, new spot hits. At 240px the wrapped Coral chip selects; repeat
   at integer scale 2. Check glyphs, colors,
   clip and orientation (do not require CPU/GPU antialiasing bytes to match).
   Check idle produces no swaps, close and owner death stop, fresh window opens.
   No injected resolved events or screenshot NIF as the visibility oracle.
2. **Failure controls.** At the native presenter boundary exercise failed first
   swap/present, blocked acquire, pending resize, same-size recreation, stale
   generation, listener-only reuse, press continuity and stop with outstanding
   work. Verify last submitted hits/routes/geometry, bounded wakeups and safe
   retirement. Never treat submit success as proof of physical scanout.
3. **Diagnostics are separate from visible output.** Capture now reads the
   retained *submitted* GPU color target on demand, without redrawing or reading
   a recycled back buffer. CPU capture is preserved. Do not add stats/PRIME claims
   based on a Cargo feature. Full Vulkan hardware qualification additionally
   requires the [existing opt-in sync gate](../scripts/check-wayland-vulkan.exs):
   retain burst-without-readback, capture/second-flush and cap tests. Port their
   repeated initial uploads to v10 patches and heights to supported pixel sizes;
   implement the actual capture/stats they assert, not weaker assertions.
   Require hardware consent, matched sync-validation startup/teardown and zero
   warnings/errors. No suitable device means **not hardware-qualified**.
4. **Regression matrix.** Private, identified source builds under the shared
   performance lock: headless-only, shm-only, GL-only Wayland, Vulkan-only
   Wayland without OpenGL, and both APIs together. Verify the mapped NIF and
   actual selected API. Run release Rust tests/build/strict Clippy, Mix quality,
   retained tests and the matched full suite; compare exact failure names and
   profile-specific skips against the [CPU checkpoint](wayland-demo.md)
   (96 named failures, 21 exclusions on the default profile). Preserve the
   fitting allocation control independently of GPU/driver allocations. No new
   exclusions, headless GPU/DRM port, timing or hardware claim by inference.
