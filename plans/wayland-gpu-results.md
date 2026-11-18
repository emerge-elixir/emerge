# Wayland cards: implementation and evidence

The [standalone OpenGL and Vulkan demo](wayland-demo.md#run-the-standalone-demo) runs the
same ordinary viewport as the [CPU checkpoint](wayland-demo.md). Both GPU
presenters were exercised on hardware. **The opt-in Vulkan synchronization
validation gate was not run; this is not full Vulkan qualification.** Neither
backend changes the default CPU demo, `:auto`, the wire, or the UI DSL. The
launcher observations below predate the move to the standalone OTP application.

## What changed

- [Startup](../native/emerge_skia/src/session/wayland/mod.rs) waits for the
  GPU window's initial configure (and advertised Vulkan feedback) before
  selecting a presenter. Proven absence of supported hardware yields the
  existing `wl_shm` presenter and one ordinary warning. Ambiguous hardware,
  missing features and transport errors do not. There is no runtime switch.
- [Paint](../native/emerge_skia/src/tree/paint.rs) settles draw and hit
  contributions without allocating CPU pixels. The CPU sink still draws
  glyphs normally. [Hardware text](../native/emerge_skia/src/tree/paint/gpu.rs)
  reads outlines of the same bundled font, tessellates geometry, and records
  triangles at the retained glyph positions. The GPU computes coverage on
  a 4x MSAA RGBA8 target. No software mask, glyph atlas or bitmap executor
  is part of that path. A missing outline is an error, not an omitted glyph.
- The [display worker](../native/emerge_skia/src/session/wayland/gpu.rs) seals
  one command recording and its pending routes. Only that worker can settle
  the reserved candidate hit list. An edit during a blocking driver call
  cannot publish newer callbacks with older pixels. Equal drawing can skip
  a submit only when the prior submission is still valid for that target.
  The two retained color surfaces change roles on successful handoff. GPU
  capture reads the submitted one on demand via a bounded worker request;
  it does not redraw, read a recycled buffer or read back every frame.
- [EGL](../native/emerge_skia/src/session/wayland/gl.rs) owns swap/commit.
  [Vulkan](../native/emerge_skia/src/session/wayland/vulkan.rs) uses a single
  graphics/present queue, FIFO, generation-scoped acquisition, a preceding
  `ALL_COMMANDS` wait, and per-image render-finished semaphores and WSI fences
  (`VK_EXT_swapchain_maintenance1`). A busy acquire retains the request;
  `SUBOPTIMAL` presents, `OUT_OF_DATE` does not. Unproven retirement
  quarantines the native graph rather than reporting reusable resources.
- The existing stats schema now reports counted successful submissions,
  submit duration, and (when available) the entered output's current mode.
  Uncollected timing families have no samples; this does not measure scanout.

## What was observed

| Control | Result |
| --- | --- |
| Before | Both explicit requests failed with `Wayland requires the wayland feature and an explicit CPU raster renderer`. |
| Hardware selection | Private Sway headless **compositor output**, GLES compositor on `/dev/dri/renderD128`. GL: AMD Radeon RX 7900 XTX, radeonsi, OpenGL ES 3.2 Mesa 26.2.7. Vulkan: `AMD Radeon RX 7900 XTX (RADV NAVI31)`, `MESA_RADV`, compositor feedback matched render node `226:128`. Both ordinary source-build launchers opened and exited on close. |
| Compositor output and input, each API | `grim` and `zwlr_virtual_pointer_v1` on the actual `wl_seat`: surface `(17,17)` press/release in `handle_input/2`, card-local `(5,5)` in `handle_info/2`, moving card and count, old spot miss/new spot hit. At 240px Coral wrapped and selected. At scale 2 the card still reported `(5,5)`. Four idle captures generated no client surface submission. Close, reopen and owner death cleaned up. No screenshot NIF or `send_input/2` substituted for compositor acceptance. |
| Startup through cold/new text, resize and scale | In each fresh renderer process GDB observed 355 Ganesh vertex and 64 rectangle calls; the matching GL or Vulkan command path recorded 28 indexed draws and eight non-indexed draws. GL set clear color 16 times; Vulkan began 16 render passes (a load-op clear need not issue `vkCmdClearAttachments`). No calls to host pixel wrapping, bitmap rectangle drawing, FreeType glyph-image generation, software path masks, coverage-mask allocation, glyph atlas insertion, or probed host pixel write/staging paths. This is evidence for the selected cards scene and drivers, not all Skia operations. |
| Positive controls | A separate **non-candidate** private build deliberately drew a raster rect, blurred shape and text, uploaded its bitmap, and used ordinary atlas text. Per API: one pixel wrap, one bitmap rect, 13 glyph images, one coverage-mask allocation, eight atlas adds, two host writes and two writes to the corresponding backend. This process was not used for visual acceptance. |
| Fallback | Pixman compositor with llvmpipe for GL, or an isolated lavapipe ICD for Vulkan: requested API retained, selected raster, CPU capabilities, no Vulkan device. The same compositor pixel/input/resize/lifecycle controls passed. One logged warning per session, including after redraw and resize; explicit CPU and hardware starts had none. Two eligible Vulkan devices without compositor feedback returned an ambiguity error. |
| Capture (separate) | Both source builds and both single-API NIFs read the submitted card's RGB `<<42,145,90>>`. This does not substitute for the burst, repeated capture or sync-validation gate. |
| Native and BEAM | Private release builds/tests and strict Clippy including tests: headless 40, shm 42, GL-only 43, Vulkan-only without GL 67, both APIs 67, matched default 43. Cargo/Mix format, forced warnings-as-errors compilation, strict Credo and dev Dialyzer passed. The A/B/C route reducer test is separate from real input. |
| Matched Mix suite | **643/737 passed, 94 failed, 21 excluded**. No new failure names or exclusions vs the CPU checkpoint. The two repaired failures are the `EmergeSkia.DiagnosticsTest` startup collection and startup-only configuration tests. One new passing route-ordering test. |

### Allocation control

Repeated independent processes exposed an intermittent fitting-detach
`[1,0,1]` on both the old CPU checkpoint and the GPU candidate. A private
allocator backtrace identified `Batch::reserve` → hash-table `reserve_rehash`:
removal tombstones could reduce reported capacity at unchanged resident count.
The [scratch index](../native/emerge_skia/src/tree/wire/batch.rs) now keeps
reserved bucket heads and links in the batch entries; reset touches only used
buckets. IDs still compare exactly. No test or threshold was relaxed.
The repaired control passed 120 fresh-process repetitions. All six release
profiles recorded fitting `[0,0,0]`, initial `[72449,9785,72174]`, one-over
text `[0,6,0]` and one-over topology `[0,58,0]`. These are Linux/glibc
allocate/reallocate/free **calls through draw-input preparation**, not bytes,
GPU allocations, pixels, driver latency or throughput.

## Reproduction and limits

Private evidence: `/workspace/emerge-investigation-artifacts/wayland-gpu-cnulVZ`.
`matrix`, `check-gpu`, `controls.exs`, `trace-script`, `launcher`, `launch-maps`,
`logs/{red,hw-*,fallback-*,trace-*,positive-*,read-*,last-suite,repeat-fixed}`.
The independently built launcher NIFs and Cargo artifacts were identified by
actual process mappings, not assumed byte-identical. The `hw-*` / `fallback-*`
directories retain the eight compositor observations from each run.

The [opt-in synchronization gate](../scripts/check-wayland-vulkan.exs) still
requires explicit hardware consent, a validation layer, matched startup and
teardown reports and zero sync warnings/errors. Its existing tests now use
v10 updates and supported pixel heights without dropping the no-readback burst,
second-flush capture, cap or stop assertions. No physical scanout, matched
timing, unrelated renderer support or product-wide correctness claim.
