# Active Plan: Vulkan rendering API implementation

Created: 2026-08-10
Status: Shared Vulkan Ganesh, real multi-GPU Wayland WSI, headless linear ABGR8888 PRIME output, shared ABGR8888/NV12 DMA-BUF input, and a no-WSI DRM/KMS GBM-import presenter are implemented. The pinned RPi5 UI-only presenter smoke now passes real V3DV/KMS rendering (56 atomic commits, 57 exact page flips, no EBUSY/missed-vblank/quarantine/device-loss fault, and bounded 462 ms shutdown). Direct NV12 is wired for Camera with independent per-format admission and first-success runtime allocation attestation, but PiSP import/color/synchronization and performance remain runtime-debug gates rather than preimplementation blockers. All four fresh-process Emerge Demo routes pass a candidate RADV smoke with byte-exact solid pixels and short lifecycle soaks. Full M1 five-minute/synchronization-validation/delayed-fence/resize/restart/fault acceptance and authoritative live Camera performance remain open.
Performance authority: RPi5 Camera on DRM/KMS
First validation authority: Emerge Demo PRIME tab on Wayland, matrix notation `headless-main`

## Implementation progress

- [x] Added optional dynamically loaded `ash`, shared Vulkan/DRM/Wayland feature
  names, and feature-aware explicit Linux Vulkan availability contracts.
- [x] Select the probe instance API as `min(loader, Vulkan 1.2)` with a Vulkan
  1.1 floor, and use `min(instance, physical-device API)` for core promotions.
- [x] Extracted pure capability profiles, promotion rules, exact DRM selection,
  and future Wayland graphics+present selection into `backend/vulkan`.
- [x] Added `wayland-vulkan`/`wayland-all`, the exact unavailable-build error,
  shared Wayland raw handles, and a real `RendererEnv::Vulkan` arm. Explicit
  Vulkan startup acknowledgement waits up to five seconds for the first
  configure and a fully initialized Vulkan renderer, then returns the precise
  loader/device/surface/swapchain error; it never falls back. `:auto` remains
  OpenGL-first with raster fallback.
- [x] Pinned rust-skia/skia-safe and coordinated skia-bindings at immutable
  commit `0d2261c63941f4b534522246cc1ace13ca4242d8` (version 0.99.0) for the
  Vulkan BackendSemaphore, Surface wait, and FlushInfo signal wrappers. Exit
  the git pin with the first adopted upstream release containing that commit.
- [x] Implemented presenter-neutral Vulkan loader/instance, hardware device and
  combined queue selection, Ganesh ownership, audited ash/rust-skia raw handle
  conversion, target wrapping, acquired/completed frame contracts with explicit
  layout and queue-family state, custom flush, semaphore synchronization,
  on-demand RGBA capture, device-loss handling, and ordered teardown under
  `backend/vulkan`.
- [x] Implemented the Wayland-only Vulkan surface/swapchain presenter with FIFO,
  B8G8R8A8_UNORM, COLOR_ATTACHMENT|TRANSFER_SRC|TRANSFER_DST, per-image
  render-finished semaphore reuse, one-shot Skia-owned acquire semaphores,
  zero-timeout nonblocking acquisition, resize/out-of-date recreation, and
  successful-present-gated screenshot publication. `TRANSFER_DST` is required
  by Ganesh's borrowed-image validation and is shared by swapchain creation and
  the Skia image descriptor. Compositor frame-callback pacing is retained and
  Vulkan late replacement is disabled.
- [x] Added shared Vulkan one-plane ABGR8888 DMA-BUF import with exact modifier
  image creation/binding, temporary sync-FD semaphore import, explicit
  external-to-graphics acquire, direct top-left Ganesh sampling, application-owned
  graphics-to-external release, and fence-gated canonical lease retirement.
  Failed candidates preserve the last valid frame; one global eight-import cap
  covers current, capacity-blocked stale, and retired Vulkan resources while
  reserving one slot for failure-safe replacement; saturation stops new imports.
  Uncertain tickets transfer to one bounded process-wide quarantine owner whose
  terminal flag rejects later Vulkan importer runtimes until process restart.
- [x] Added headless Vulkan linear ABGR8888 PRIME production with exact DRM-node
  selection, SAMPLED-compatible exported allocations, application-owned
  graphics/external transfers, one-shot sync-FD export, bounded slots, and reuse
  gated by both canonical release and producer-fence completion. Vulkan streams
  negotiate strict `acquire_sync: :sync_file` and modifier `0`.
- [x] Corrected headless Vulkan descriptors to publish the complete size queried
  from the exported DMA-BUF fd rather than `VkMemoryRequirements::size`; aligned
  allocation tails remain outside the checked packed plane span. Host regression
  coverage is complete; repeat both Vulkan-producer demo routes for hardware proof.
- [x] Enabled Wayland Vulkan PRIME admission. `scripts/prime-matrix.sh` ran the
  OpenGL-OpenGL, Vulkan-OpenGL, OpenGL-Vulkan, and Vulkan-Vulkan candidate smoke
  successfully on RADV `/dev/dri/renderD128`; each route exercised byte-exact
  solid captures, animated replacement, hide/show, reconnect, bounded
  leases/resources, and clean producer/consumer shutdown. Ordinary demo PRIME
  validation remains disabled by default until the complete M1 acceptance list
  passes.
- [ ] The first full 9,000-frame M1 run on 2026-08-10 did not pass and is not
  acceptance evidence. OpenGL-OpenGL sustained the producer at 29.4 FPS, but
  the externally provided Wayland compositor stopped scheduling the unfocused
  surface at the required rate: `source_frames=9000`, `main_frames=866`, minimum
  `8100`. No remaining route was promoted after the control failed. Re-run all
  four routes on a compositor/session that keeps the validation window visible
  and active. The harness now also checks byte-exact submitted/main 640x420
  RGBA, writes fresh PNG/raw artifacts, exercises hide/show and reconnect, and
  starts a second clean renderer lifetime before its final FD bound.
- [x] Preserved OpenGL-first `:auto`; explicit Vulkan has no raster fallback.
- [x] Added the `drm-vulkan`-gated, WSI-free `vulkan_probe` binary. It validates
  the configured KMS card through DRM control resources, separately opens an
  explicit primary/render Vulkan selection node, requires one exact-field
  non-software Vulkan 1.1 match, and reports identity, one deterministic
  timestamp-capable graphics queue, extension, sync-FD, generic output/NV12
  format, and bounded DRM-modifier static inventory.
- [x] Inventory success exits zero but reports `probe.status=incomplete`,
  `probe.inventory_passed=true`, and `probe.phase1_ready=false`; it is not a
  Phase 1 pass and does not claim target output or Camera support.
- [x] Added an explicit opt-in `--functional --allocation-direction gbm-import`
  path. It parses the selected primary plane's `IN_FORMATS`, intersects
  XRGB8888 modifiers with Vulkan BGRA import/render support, allocates only from
  that set, imports the DMA-BUF, uses core `QUEUE_FAMILY_EXTERNAL`, clears and
  releases it, exports one master `SYNC_FD`, duplicates that FD per bounded
  atomic `IN_FENCE_FD` attempt, waits for the exact CRTC page flip, and restores
  the pre-probe connector/CRTC/plane state. Post-submit failure quarantines all
  uncertain owners. Unsupported `vulkan-export` fails without trying GBM.
- [x] Functional mode now wraps the exact DRM-modifier image as a Ganesh BGRA
  target, draws deterministic RGBA quadrants, captures them through the bounded
  screenshot path, requires byte-exact pixels, and emits a SHA-256. The shared
  wrapper now describes DRM-modifier tiling explicitly for imported/exported
  DMA-BUF targets rather than claiming optimal tiling.
- [x] Functional success remains deliberately incomplete and cannot admit the
  production presenter: no sync-FD re-import round trip, repeated fault/resource
  run, Camera NV12 path, or authoritative validation debug-callback message count
  is performed.
- [x] Host `drm-vulkan` and `drm-all` probe checks, tests, builds,
  warnings-denied Clippy, and formatting checks pass. The local runtime attempt
  remains a host-environment diagnostic rather than target capability proof.
- [x] The real Wayland presenter slice passes 988 `wayland-vulkan` Rust unit
  tests plus the fixture test, 973 default Rust unit tests plus the fixture,
  Wayland-all warnings-denied Clippy, formatting/diff checks, 440 Emerge Mix
  tests with 7 exclusions, and 48 demo tests.
- [x] Hardware Wayland Vulkan startup now uses compositor linux-dmabuf feedback
  `main_device` as a trusted DRM `dev_t`, matches it exactly against
  `VkPhysicalDeviceDrmPropertiesEXT`, and rejects unresolved multi-GPU
  ambiguity. It no longer depends on Vulkan loader enumeration order.
- [x] Real Wayland Vulkan pixels are validated on the multi-GPU workstation at
  240.2 FPS over a five-second 1201-frame Borders window. Render averaged
  1.032 ms and present submit 0.014 ms. The comparable OpenGL window sustained
  240.0 FPS with 0.966 ms render and 0.093 ms present; combined render+present
  was 1.046 ms Vulkan versus 1.059 ms OpenGL. This proves startup, Ganesh target
  wrapping, pixels, and compositor pacing, but not Vulkan GPU elapsed timing,
  PRIME input, resize/restart, screenshots, or RPi/V3DV behavior.
- [ ] Run `vulkan_probe --drm-card <VC5-KMS-primary>
  --vulkan-drm-node <V3D-render> --require-v3dv --functional
  --allocation-direction gbm-import --validation` against the pinned Nerves
  V3DV image. RPi5/Nerves must pass the separate V3D render node explicitly; no
  VC5/V3D association may be inferred from enumeration order. The Camera target
  release now cross-builds and stages this AArch64 executable, and the pinned
  system diagnostic can invoke it only through an explicit `--run-functional`.
- [ ] Use that pinned-target run to prove the dynamic KMS/Vulkan/GBM modifier
  intersection and exact output image create/import/bind/page-flip path. Host
  compilation and the implemented probe are not target proof.
- [x] Software-complete: full immutable `VideoInterop.Format` now crosses the
  consumer-open NIF boundary into active-stream state. Vulkan NV12 stream admission
  requires explicit sync-file/modifier/BT.709-output-identical color contracts and
  truthful active-device modifier capability. Producer topology is structurally
  validated before claim; the first exact successful image create/import/bind on the
  selected device establishes a renderer-lifetime allocation proof, and later frames
  must match it byte-for-byte before import. ABGR linear support is independent and
  cannot disable an otherwise capable NV12 importer. The shared importer retains exact
  one-object/two-plane NV12, DRM-modifier external-memory import, Ganesh YCbCr metadata,
  acquire/release, bounded retirement, fault quarantine, and Vulkan-specific counters.
- [ ] Runtime-debug gate: run the live PiSP holder through the implemented shared-object
  import and inspect the first precise V3DV failure or established runtime proof, then
  validate exact colors, temporary acquire wait, direct Ganesh sampling, external
  release, and dispatcher retirement.
- [ ] Prove sync-FD import/export, exact ownership barriers, primary-plane
  `IN_FENCE_FD`, retry-safe FD handling, and readback.
- [ ] Prove Skia Ganesh wrapping/state control and the pinned Nerves AArch64
  cross-build/boot. None of the four complete Phase-1 spike questions is yet
  answered.
- [x] `drm-core` isolates KMS discovery/error handling from the unchanged
  OpenGL owner; `drm-vulkan` compiles and dispatches a no-WSI Vulkan presenter
  without EGL/GL/Wayland dependencies. The opt-in presenter admits exactly
  GBM XRGB8888 `SCANOUT|RENDERING` allocations imported as Vulkan
  B8G8R8A8 from the dynamic KMS/Vulkan modifier intersection. It validates the
  KMS primary and a separate exact Vulkan node/device, requires `IN_FENCE_FD`,
  owns three persistent slots and one engine, duplicates a master sync file for
  every atomic attempt, retains prepared state only for structured `EBUSY`, and
  bounds each prepared frame to the configured initial attempt plus
  `drm_startup_retries`. Exhaustion proves the unaccepted GPU submission idle,
  discards the prepared identity without reuse uncertainty, and returns a precise
  backend-unavailable error instead of leaving startup blocked. It then
  promotes/reuses only on the exact CRTC page flip. Presenter admission is
  independent from optional PRIME video-import initialization. Per-format video
  capability is independent: unsupported linear ABGR does not disable generically
  supported NV12, while no supported direct format leaves PRIME false and target
  creation fail-closed. Direct shared Vulkan Video import, when admitted, keeps
  page-flip-gated capture/stream publication,
  animation, stats, and a
  software cursor are wired. Bounded normal shutdown restores KMS and destroys
  owners in order; uncertain ownership moves one complete session to a bounded
  process-lifetime quarantine and rejects later admission until VM restart.
  This is trial-ready software, not pinned-RPi5 acceptance evidence.
  `wayland-vulkan` transitionally implies compatibility Wayland/OpenGL until
  Vulkan-only cfg extraction lands.
- [ ] Vulkan-only dependency extraction remains open: the current
  `wayland-vulkan` release NIF still links compatibility GBM/OpenGL dependency
  closure even though explicit Vulkan execution does not route through EGL/GL.
- [ ] Wayland resize/restart/screenshot/validation-layer/device-loss acceptance
  remains open even though ordinary hardware pixels and pacing now pass.
- [x] Applied DRM Phase-3 review hardening: Vulkan KMS probing enables atomic
  universal planes and uses a strict typed primary; scanout policy permits one
  rendering slot and one commit in flight; Vulkan 1.1 explicitly requires/enables
  `VK_KHR_image_format_list`. The unconnected teardown sequence is labelled
  design-only while allocation remains rejected.
- [ ] Complete the Emerge Demo PRIME `headless-main` acceptance matrix in the
  required order: OpenGL-OpenGL, Vulkan-OpenGL, OpenGL-Vulkan, and
  Vulkan-Vulkan. The short candidate smoke passes and clean renderer restart is
  now exercised. The first 9,000-frame control attempt failed the main-frame
  telemetry gate (`866/9000`) under an unfocused/throttled external compositor;
  five-minute pacing, live resize, delayed-fence, injected-fault, and complete
  telemetry acceptance therefore remain. The headless producer now uses one bounded
  sync-file waiter wake rather than render-thread fence polling.
- [ ] Repeat the matrix with `VK_LAYER_KHRONOS_validation` synchronization
  validation when that layer is available, and run injected delayed-fence,
  rejected-wait, export-failure, and device-loss/quarantine acceptance.

## Direction

Implement Vulkan as a first-class Linux rendering API with one shared Vulkan
engine and three endpoint owners: headless PRIME producer, Wayland WSI
presenter/consumer, and DRM/KMS presenter/consumer. The exact RPi5 Camera
DRM/KMS workload remains the performance authority, while Linux Wayland Vulkan
and headless PRIME Vulkan are supported explicit APIs:

```elixir
EmergeSkia.start(backend: :headless, rendering_api: :vulkan, headless: [mode: :prime], ...)
EmergeSkia.start(backend: :wayland, rendering_api: :vulkan, ...)
EmergeSkia.start(backend: :drm, rendering_api: :vulkan, ...)
```

Vulkan support is not conditional on outperforming OpenGL. Performance results
inform optimization and the future `rendering_api: :auto` selection policy, not
whether the explicit API is retained. Preserve the existing Linux `:auto`
OpenGL-first and raster-fallback semantics until a separate policy change.
Explicit Vulkan startup must either select a verified hardware Vulkan device or
fail with a precise error; it must never silently select OpenGL, raster, or a
software ICD.

Use Skia Ganesh Vulkan so scene traversal, paint-layer caching, assets, text, and
`RenderFrame` remain shared with OpenGL. Do not introduce Graphite in this
implementation slice.

## Goal

Deliver a correct, ownership-safe shared Vulkan rendering API for Linux
headless PRIME production, Wayland presentation/PRIME consumption, and DRM/KMS,
then measure whether V3DV Vulkan provides enough GPU headroom to sustain the
exact live RPi5 Camera Focus scene at 60 FPS.

Before Camera or DRM acceptance, the Emerge Demo PRIME tab must pass the full
`headless-main` renderer matrix: OpenGL-OpenGL, OpenGL-Vulkan,
Vulkan-OpenGL, and Vulkan-Vulkan.

The final performance target remains:

- active median/mean GPU elapsed <= 10.86 ms;
- p95 <= 11.67 ms;
- p99 < 16.67 ms;
- 59.8-60.2 presented FPS;
- zero post-warmup missed-vblank sequence gaps;
- live Camera input remains 59-60 FPS;
- no fence, lease, FD, memory, page-flip, or device-loss errors.

A lower CPU submit time, deeper queue, or 60 FPS with less GPU headroom is not
success.

## Scope

The accepted implementation includes one shared Linux Vulkan core plus both
presenters:

- shared instance/device selection, Ganesh context, frame orchestration, direct
  NV12 DMA-BUF Video importer, capture, timestamps, and completion tracking;
- headless PRIME output through bounded Vulkan-renderable DMA-BUF slots with
  explicit sync-file export and canonical lease-controlled reuse;
- DRM/KMS output through Vulkan-renderable scanout buffers without WSI;
- Wayland output through `VK_KHR_wayland_surface` and `VK_KHR_swapchain`;
- the same `AcquiredTarget`/`CompletedTarget` render contract for DRM slots and
  Wayland swapchain images;
- explicit acquire synchronization and lease-safe GPU retirement;
- hardware cursor and the existing 270-degree DRM scene transform;
- on-demand screenshots acknowledged by each presenter;
- Nerves V3DV loader/ICD packaging;
- deterministic GL/Vulkan correctness on Wayland plus paired RPi5 performance
  tests, with RPi5 DRM remaining the performance authority.

## Non-goals

- Do not change `:auto` selection policy in this implementation slice.
- Do not add macOS Vulkan or headless Vulkan binary/readback presentation.
  Headless Vulkan scope is intentionally limited to PRIME DMA-BUF production
  required by the four-way demo matrix.
- Do not use Vulkan WSI, `VK_KHR_display`, or a Vulkan swapchain for DRM. KMS
  remains the DRM presentation authority. Wayland does use
  `VK_KHR_wayland_surface` and `VK_KHR_swapchain`.
- Do not use CPU video conversion/upload, an EGL/OpenGL interop bridge, or a
  prerecorded RGBA substitute for the performance comparison.
- Do not change semantic paint-layer topology, slider behavior, Camera IDs, or
  scene recognition.
- Do not combine Vulkan work with unaccepted semantic-backing changes, mutable
  repair, KMS media planes, dithering, or Graphite. Fork from whatever renderer
  state is frozen for the authoritative paired OpenGL baseline.
- Do not attempt runtime Vulkan-to-OpenGL migration with outstanding Camera
  frames. Explicit Vulkan device loss stops and drains the session.

## Existing seams to preserve

The public API and native `RenderingApi::Vulkan` accept explicit Linux Vulkan
requests into feature-aware validation. Unavailable builds fail precisely.
Compiled Wayland Vulkan dispatches the real Vulkan runtime before any EGL
initialization; compiled DRM Vulkan remains deliberately rejected as unwired
because only the WSI-free diagnostic probe exists.

`SceneRenderer` already draws through a backend-neutral Skia `Surface` and
Ganesh `DirectContext`. The new backend should reuse:

- `native/emerge_skia/src/renderer.rs` scene traversal and cache;
- the direct/non-cacheable Video scene primitive;
- existing KMS connector, mode, cursor, atomic commit, and page-flip logic;
- existing `LatestFrameStore` screenshot contract;
- existing renderer/session lifecycle and NIF resource ownership.

The API-specific replacements are presentation only: headless PRIME uses
leased DMA-BUF output slots, DRM uses scanout slots and atomic KMS commits, and
Wayland uses swapchain acquire/present. EGLImage/external-OES import and GL
fence retirement are replaced once by the shared Vulkan Video importer used by
Wayland and DRM.

## Mandatory first validation: Emerge Demo PRIME matrix

The first implementation and hardware gate after Wayland UI bring-up is the
Showcase PRIME tab. The main viewport remains a Wayland window. Its source is a
second headless renderer producing canonical leased DMA-BUF frames. Matrix
notation is always `headless-main`:

| Pair | What it isolates |
| --- | --- |
| OpenGL-OpenGL | frozen existing producer/importer control |
| Vulkan-OpenGL | new headless Vulkan exporter against the existing EGL importer |
| OpenGL-Vulkan | existing EGL exporter against the new shared Vulkan importer |
| Vulkan-Vulkan | complete Vulkan export/import/synchronization/lifetime path |

Run in that order so exporter and importer failures are isolated before the
combined path. This gate uses the existing 640x420 ABGR8888 linear PRIME scene.
It proves generic PRIME storage, explicit synchronization, latest-frame
behavior, and canonical lease ownership. It does not claim Camera acceptance:
NV12 multiplanar import, YCbCr conversion, Camera colorimetry, and RPi KMS remain
separate later gates.

### Required implementation seams

1. Add the generic stream acquire policy
   `VideoInterop.Format.acquire_sync = :implicit | :sync_file | :per_frame`,
   preserve it through adapters, forward it during Emerge consumer open, and
   enforce it before claim. Camera color fields remain later M4 work, but the
   accepted matrix already requires strict `:sync_file` and negotiates the
   producer's proven linear DRM modifier explicitly as `0`, not `:per_buffer`.
2. Add `headless-vulkan` and `headless-all` Cargo features. Allow
   `compiled_vulkan_backends: [:headless]` in addition to Wayland/DRM without
   pretending headless is an independently compiled platform backend. The demo
   source build enables Vulkan for both `:headless` and `:wayland`.
3. Enable only `(HeadlessMode::Prime, RenderingApi::Vulkan)` when
   `headless-vulkan` is compiled. Keep binary-mode Vulkan rejected. `:auto`
   keeps its existing OpenGL-first/raster behavior; headless Vulkan remains
   explicit-only and never falls back.
4. Add an explicit headless PRIME allocation node, for example
   `headless: [prime: [drm_node: "/dev/dri/renderD128", ...]]`. Both headless
   OpenGL and Vulkan producers must report the selected DRM `dev_t`; ambiguous
   automatic multi-GPU selection is an error. The test harness asserts that the
   producer node is import-compatible with the Wayland compositor-selected main
   device.
5. Add `backend/headless/vulkan.rs`. It owns a bounded pool of linear
   ABGR8888 DMA-BUF slots, each with the GBM allocation, Vulkan image/memory,
   borrowed Skia surface, exportable completion semaphore, stable metadata, and
   canonical backend release token. It uses the shared `VulkanEngine`; it does
   not use WSI or KMS.
6. Generalize `VulkanTargetSurface` from the Wayland-only
   B8G8R8A8/BGRA target to an explicit validated target descriptor. Wayland
   keeps B8G8R8A8/BGRA; headless ABGR8888 uses the proven matching Vulkan/Skia
   format. Both declare Ganesh's required transfer-source and transfer-destination
   usages.
7. Add presenter-neutral `backend/vulkan/external_image.rs` and `sync.rs`:
   exact DRM modifier plane layouts, DMA-BUF import/bind/export helpers,
   distinct-object FD handling, memory-type selection, temporary SYNC_FD import,
   one-shot SYNC_FD export, and RAII ownership on every failure edge. These are
   low-level helpers, not a media queue or presenter.
8. Extend the shared frame completion contract with a post-Ganesh command and
   nonblocking GPU completion ticket. Vulkan consumers must release every
   sampled imported image from graphics ownership back to the proven
   external/foreign queue family; the ticket signals only after that release
   barrier. A Video frame cannot retire or return its canonical lease before the
   ticket signals. Ordinary frames still do not CPU-wait.
9. Add the first shared Vulkan Video importer for the matrix's single-plane
   ABGR8888 linear frame. Keep pending/current/retired policy and stream identity
   in `VideoRegistry`; Vulkan objects live only in imported pending/current/
   retired owners and never enter paint-layer payloads. NV12 is a later extension
   of this same importer, not another presenter-specific importer.
10. Make demo source and main APIs independently configurable without source
   edits, using stable values such as
   `EMERGE_DEMO_HEADLESS_RENDERING_API=opengl|vulkan` and
   `EMERGE_DEMO_MAIN_RENDERING_API=opengl|vulkan`. Once all four compiled
   routes exist, enable PRIME admission in an explicit candidate matrix profile
   so the tests can run. Enable ordinary demo Vulkan PRIME availability only
   after the matrix passes. The PRIME tab reports the requested/selected pair
   and importer/exporter diagnostics.

### Headless Vulkan slot lifecycle

```text
Available -> Rendering -> ExportedLease -> Available
                  |             |
                  |             +-- only after consumer GPU retirement and canonical release
                  +-- submission failure -> fence-retire -> Poisoned/terminal
```

A slot owns its allocation for the renderer lifetime. Rendering releases image
ownership to the proven external/foreign queue family, signals an exportable
binary semaphore, exports that SYNC_FD payload exactly once, and sends the frame
through the existing canonical lease path. The producer retains the DMA-BUF and
slot until the release token returns. On release it reacquires the proven queue
ownership before reuse. The consumer release message is not itself a GPU fence;
it is trusted only because the consumer retires the canonical lease after its
GPU completion ticket signals after the consumer's graphics-to-external/
foreign release barrier. The OpenGL consumer must provide the equivalent
GPU-complete retirement proof before lease release. Producer reacquisition is
paired with that release; completion without the ownership barrier is not
sufficient.

Backpressure remains `drop_new` with a fixed maximum in flight. No output slot is
reused after terminal error, uncertain ownership, failed semaphore export, or
before canonical release. Shutdown closes admission, drains leases and release
callbacks, waits/destroys Vulkan state on the backend thread, and joins the
release dispatcher exactly as the OpenGL producer does.

### Matrix acceptance

Each pair runs in a fresh process under the performance lock and records both
requested and selected APIs, physical/DRM device identity, format/modifier,
explicit-sync mode, frame counters, queue depths, leases, FDs, and RSS.

Required checks per pair:

- the PRIME tab continuously displays the animated headless scene for at least
  five minutes with 29-31 produced/imported FPS and a 240 Hz main window;
- `fallbacks == 0`; accepted runs use an exported acquire SYNC_FD and a GPU wait,
  never `glFinish`, `vkQueueWaitIdle`, CPU polling, or an RGBA upload;
- every Vulkan-containing pair runs with synchronization validation and produces
  zero validation messages;
- a deliberately delayed producer acquire fence proves that the main renderer
  never samples the new generation early, and delayed consumer completion proves
  the producer slot and canonical lease are not released/reused before the
  consumer release barrier and completion ticket;
- one pending latest frame, bounded imported-retired frames, at most three
  producer leases, balanced prepare/claim/release counts, and no monotonic FD/RSS
  growth;
- switch away from PRIME and back, hide/show Video, resize the Wayland window,
  disconnect/reconnect the source, and stop/restart both renderers without a
  stale frame, early release, or capture termination;
- on injected preclaim failure the caller remains owner; on every postclaim
  failure the consumer retires exactly once; failed import/acquire preserves the
  last valid current frame;
- on-demand main-window screenshots include the requested presented PRIME
  generation and idle rendering performs no readback;
- a static deterministic fixture produces byte-identical main-window RGBA
  screenshots across all four pairs. Run the animated scene separately for
  pacing/lifetime soak so wall-clock animation phase cannot invalidate the byte
  comparison.

**Gate:** all four pairs must pass before enabling Camera work. A passing
Vulkan-Vulkan pair is not a substitute for either mixed pair: the mixed pairs
prove that the canonical PRIME contract is API-neutral. Failure stops at this
matrix and does not move synchronization or lifetime work into Camera or DRM.

## Critical path from demo matrix to full Camera test

The remaining work is serial at ownership boundaries even when packaging or
pure-test preparation happens in parallel:

| Milestone | Deliverable | Gate |
| --- | --- | --- |
| M0 — Wayland UI core | Multi-GPU exact device selection, real pixels, Ganesh target, 240 Hz pacing | Complete for ordinary pixels; retain resize/screenshot/loss follow-ups |
| M1 — Demo PRIME matrix | Generic stream acquire policy, headless Vulkan ABGR output, shared Vulkan ABGR input, four `headless-main` pairs | All matrix acceptance checks above pass |
| M2 — Freeze target baseline and preflight | Freeze immutable cache-on GL Camera baseline and dependency closure; package pinned V3DV loader/ICD; prove split VC5/V3D nodes, exact extensions, timestamps, KMS `IN_FORMATS`/`IN_FENCE_FD`; probe uses `COLOR_ATTACHMENT|TRANSFER_SRC|TRANSFER_DST` | Baseline identity is immutable; static probe remains `incomplete` but every prerequisite is present |
| M3 — One live output/sync spike | One exact KMS/GBM/Vulkan modifier; create/import/bind; Ganesh clear; post-Ganesh barrier; one-shot sync-FD; `IN_FENCE_FD`; page flip; exact readback | Select one allocator direction and one external/foreign ownership recipe; zero validation errors |
| M4 — Stream color and Camera acquire contract | Complete stream-level primaries/transfer/matrix/range/chroma; libcamera negotiated color plus strict explicit sync-file export; Emerge native stream plumbing | Every field and sync policy is validated before any M5 claim; unspecified/unsupported NV12 or implicit acquire is rejected |
| M5 — Live canonical Camera import spike | Validate actual holder and exact NV12 objects/planes/modifier before prepare→claim; temporary semaphore import, direct YCbCr Ganesh sample, graphics-to-external release, GPU completion, ordered retirement | No copy/fallback, balanced receipts, bounded retirement, exact color pattern |
| M6 — DRM decomposition | `drm-core` KMS runner plus OpenGL/Vulkan owner enum, structured errno, `vulkan_drm_node`, Vulkan-only linkage and a no-WSI DRM Vulkan profile | Frozen OpenGL behavior passes; real `drm-vulkan` runner has no EGL/GL, surface, swapchain, or display dependencies/objects |
| M7 — DRM Vulkan UI | At least three persistent scanout slots, slot/ref state, post-Ganesh release, master sync-file retry duplication, atomic/page-flip retirement, cursor and 270° transform | Production runtime proves configured VC5 primary and exact V3D render-node match, rejects omitted/wrong/ambiguous split nodes, uses KMS only, and sustains exact UI at 60 Hz with zero sequence/validation/FD/ownership errors |
| M8 — Production NV12, capture, and timing | Extend the M1 importer to NV12; wire it unchanged to DRM; async page-flip-gated capture; bounded timestamp queries | Five-minute live Camera correctness/lifecycle soak at 59-60 FPS, no fallback or growth, and zero Video paint-cache admissions/stores |
| M9 — Full Camera acceptance | Immutable dual-API firmware, fault matrix, exact GL/Vulkan readbacks, authoritative cache-on active Focus A/B and recovery | Full correctness, lifetime, performance, and zero Video cache-admission targets pass |

### Cross-repository ownership

- `/workspace/video_interop`: stream schema only. Add Rust/Elixir colorimetry and
  acquire-sync policy parity; keep Vulkan, Skia, queueing, and renderer policy
  out of the library. Colorimetry remains stream-level, never per frame.
- `/workspace/emerge-headless`: sole owner of Vulkan external-memory helpers,
  headless exporter, shared importer, Ganesh waits/completion, Wayland/DRM
  integration, bounded retirement, capture, timing, and renderer diagnostics.
- `/workspace/colibri/membrane_libcamera`: report negotiated libcamera color
  fields; supply explicitly configured chroma siting only when the API cannot;
  export producer completion from each distinct DMA-BUF reservation object to
  sync files and merge them for strict `:sync_file` mode. Never silently fall
  back to implicit sync.
- `/workspace/membrane_video_interop`: no importer or lifecycle redesign. Add
  preservation tests for stream color/acquire fields and existing transferred
  receipt/demand behavior.
- `/workspace/colibri/camera`: select DRM Vulkan, explicit VC5/V3D nodes, strict
  color/acquire policy, and expose diagnostics. Do not add descriptor, Vulkan,
  queue, release, or importer logic.

### Stream contract required before NV12

Add an immutable stream acquire policy to `VideoInterop.Format`:
` :implicit | :sync_file | :per_frame`. Existing compatibility paths may retain
`:per_frame`; the accepted demo Vulkan matrix and Camera Vulkan path require
`:sync_file`. Emerge forwards the complete stream colorimetry and acquire policy
once during consumer open and checks each frame against it before claim.

For libcamera strict sync mode, export a sync file from every distinct DMA-BUF
object after request completion using the target-proven DMA-BUF reservation
ioctl, merge multiple fences, and publish one canonical acquire sync file.
Export/merge failure is terminal for the strict source, not an implicit fallback.
Target hardware proof decides the exact ioctl flags and confirms the resulting
fence blocks early Vulkan sampling.

Initial accepted NV12 mappings are explicit and capability-checked:

- until explicit source and destination color spaces are managed, only
  BT.709/BT.709/BT.709 identical to the output contract is accepted; BT.601-family
  and BT.2020 are rejected rather than falsely claiming color management;
- limited/full map to Vulkan narrow/full range;
- left, center, top-left, and top map to the matching midpoint/cosited offsets;
- unsupported bottom siting and every unspecified value are rejected;
- selected modifier features must support the requested siting and filter.

No resolution-based matrix guess or driver-default conversion is accepted.

## Architecture decisions

### 1. Shared Ganesh Vulkan engine with `ash`

Use optional dynamically loaded `ash` without winit or vulkano. Create one
presenter-neutral engine under `native/emerge_skia/src/backend/vulkan/`:

- `capabilities.rs`: inventory, profiles, promotion rules, and exact device/queue
  selection constraints;
- `instance.rs`/`device.rs`: loader, instance, hardware device, queue, enabled
  capabilities, and long-lived function ownership;
- `ganesh.rs`/`frame.rs`: DirectContext, target wrapping, controlled flush, and
  the shared `AcquiredTarget<T> -> CompletedTarget<T>` orchestration;
- `external_image.rs`/`sync.rs`/`video.rs`: one exact DMA-BUF memory helper,
  one synchronization implementation, and one direct Vulkan Video importer
  shared by headless export where applicable and unchanged by Wayland/DRM input;
- `capture.rs`/`timing.rs`: common staging readback and query-pool timing.

Presenter-specific code stays outside that core:

- `backend/headless/vulkan.rs` owns leased linear PRIME output slots, export
  metadata, and producer release-token reuse; it has no WSI or KMS;
- `backend/drm/vulkan.rs` owns scanout slots, external ownership, sync-FD/KMS
  bridging, atomic commits, and page-flip acknowledgement; it has no WSI;
- `backend/wayland/vulkan.rs` owns `VkSurfaceKHR`, swapchain generations,
  acquire/present, resize, and Wayland presentation acknowledgement.

All three endpoints supply opaque tokens and acquisition/final-state
requirements to the same frame core. None duplicates scene traversal,
Vulkan Video, capture, or timing. Avoid a broad renderer trait until the
concrete headless, Wayland, and DRM lifecycles prove one is needed.

The common frame contract is:

```text
AcquiredTarget<T>:
  presenter token T + borrowed image description
  current layout/queue owner + acquire waits
  requested final layout/queue owner + completion kind

CompletedTarget<T>:
  unchanged presenter token + GPU completion fence
  present semaphore or one-shot master sync-file
  optional capture ticket + optional timestamp ticket
```

Headless maps `T` to a leased PRIME output slot and acknowledges reuse only on
canonical consumer release. DRM maps `T` to a scanout slot and acknowledges
completion on page flip. Wayland maps `T` to a swapchain generation/image and
acknowledges successful presentation while preserving compositor frame-callback
pacing. Resize replaces only swapchain-dependent Wayland resources; the Vulkan
device, DirectContext, scene caches, and shared Video imports remain alive.

### 2. Validate KMS and match Vulkan through separate explicit DRM nodes

Treat the configured `drm_card` as the KMS/modeset primary node. Open it first,
require a primary DRM node, and validate it with DRM control-resource queries.
Add a separate eventual runtime `vulkan_drm_node` option for physical-device
selection. The probe exposes this today as `--vulkan-drm-node PATH`. It may
default to `drm_card` for a unified DRM device, but split devices must configure
the Vulkan node explicitly. RPi5/Nerves uses the VC5 primary node for
`drm_card` and the V3D render node for `vulkan_drm_node`.

Open/stat/validate the Vulkan selection node independently. Match exactly its
primary major/minor when it is a primary node or exactly its render major/minor
when it is a render node through `VkPhysicalDeviceDrmPropertiesEXT`. Require
exactly one Vulkan 1.1-or-newer hardware match. Never infer a VC5/V3D,
primary/render, or KMS/Vulkan association from enumeration order.

Never select by enumeration order, device name, `card0`, PCI preference, or
host-wide “best GPU” scoring. Generic host inventory remains hardware-agnostic;
target runs use `--require-v3dv` and require a provable `MESA_V3DV` driver ID.
Log and expose:

- Vulkan API and driver versions;
- device/driver UUIDs and device name;
- primary/render DRM major/minor;
- graphics queue family and `timestampValidBits`;
- timestamp period and enabled extensions;
- confirmation that the ICD is V3DV on RPi5.

For headless PRIME Vulkan, require an explicit allocation/selection DRM node on
multi-GPU systems and match it exactly through the same physical-device
properties. For Wayland, use compositor linux-dmabuf `main_device` feedback as
already implemented. For DRM/KMS, keep separate `drm_card` and
`vulkan_drm_node`. Enumeration order is never authoritative in any endpoint.

Absence of a provable DRM/Vulkan device match is a Vulkan startup failure.

Require Vulkan 1.1 plus the target-proven capabilities needed by the selected
path, including external memory FD/DMA-BUF, DRM format modifiers,
`VK_KHR_image_format_list` for the Vulkan 1.1 modifier path, external semaphore
sync FD, sampler YCbCr conversion, and the selected graphics/external ownership
mechanism. Treat `VK_EXT_queue_family_foreign` as diagnostic until Phase 1
selects and proves whether foreign or core external ownership is required.
Probe importable/exportable flags, dedicated-allocation requirements,
memory-type bits, exact modifier plane count/layout, and every required image
usage. Timestamp support is required for accepted performance measurement.
Validation/debug and memory-budget extensions remain diagnostic-only.

### 3. Direct KMS scanout, not Vulkan WSI

Preferred path: allocate a bounded pool of GBM scanout BOs and import their
DMA-BUF storage into Vulkan images. This keeps the existing DRM/KMS authority and
atomic page-flip model.

Before choosing a format/modifier, intersect:

1. KMS primary-plane `IN_FORMATS` FourCC/modifier pairs;
2. GBM allocations available with `SCANOUT | RENDERING`;
3. Vulkan DRM-modifier format properties supporting color attachment and
   transfer source usage.

Initial output tuple:

- DRM `XRGB8888`;
- Vulkan `B8G8R8A8_UNORM`;
- Skia `BGRA8888`, opaque alpha, top-left origin;
- exact BO modifier, per-plane stride, and offset.

For each slot:

- keep the GBM BO alive;
- create one persistent KMS framebuffer;
- duplicate its DMA-BUF FD for Vulkan import;
- create a Vulkan external-memory image using the exact DRM modifier plane
  layouts;
- import and bind the duplicated FD with the required memory type and dedicated
  allocation metadata;
- wrap the borrowed image as a Skia Vulkan backend render target.

Treat canonical `:implicit`, DRM modifier invalid, and explicit linear modifier
`0` as distinct. Do not coerce an implicit/invalid modifier to linear. Accepted
output and Camera paths require exact modifier provenance or a separately proven
implicit mechanism.

If GBM-to-Vulkan import is unsupported on V3DV, Phase 1 may probe Vulkan
allocation/export to KMS as the alternate direction. Phase 1 must record one
allocator decision and amend the later resource-ownership design before Phase 3.
Implementation must not proceed with two vague allocator alternatives.

### 4. Scanout ownership and references

Use at least three primary slots. Track the logical KMS state plus independent
references and GPU ownership:

```text
logical: Available -> Rendering -> Prepared -> CommitInFlight -> Current
owner:   Foreign/KMS <-> VulkanGraphics
refs:    vulkan_submission + atomic_commit + kms_current + capture_copy
```

A slot becomes `Available` only after a later page flip replaces it, its Vulkan
completion fence is signaled, and all capture references are retired. A slot is
never reused while any reference is nonzero or ownership/layout is uncertain.

Represent the primary output in common DRM code with a narrow token enum, for
example:

```text
PrimaryBufferToken = GbmFrontBuffer(existing BO) | VulkanSlot(slot_id)
```

Keep connector/mode/input/cursor/page-flip orchestration shared. The Vulkan
owner receives page-flip completion and retires slot IDs; it does not transfer
raw Vulkan objects across threads.

`IN_FENCE_FD` is only an execution wait. It does not perform Vulkan image layout
or queue-family ownership transfer. Phase 1 must establish the exact validated
VulkanGraphics-to-external/foreign and external/foreign-to-VulkanGraphics
barriers used by V3DV, including whether `VK_QUEUE_FAMILY_FOREIGN_EXT` is
required. Both output and imported Camera images use the measured mechanism.

For atomic retries, retain one owned master sync-file and duplicate it for each
ioctl attempt:

| Outcome | FD and slot policy |
| --- | --- |
| semaphore export fails | no commit; retain slot until Vulkan fence completes, then fail/poison |
| atomic commit succeeds | close userspace duplicate and master after ioctl; kernel owns its reference |
| atomic `EBUSY` | close attempt duplicate, retain master and prepared slot, retry with a fresh duplicate |
| other ioctl failure | close duplicate and master; retain slot until Vulkan fence, then poison session |
| shutdown while prepared | close all FDs; retire slot only after Vulkan completion |

Do not export the same `SYNC_FD` semaphore payload more than once. `EINVAL`, lost
DRM master, page-flip timeout, impossible transition, or uncertain ownership
poisons the explicit Vulkan session rather than recycling buffers.

### 5. Ganesh surface and final texture state

Build one Skia Vulkan `DirectContext` from the selected instance, physical
device, device, graphics queue, exact enabled extension list, and an `ash`-backed
`GetProc` callback.

For each scanout slot use:

- `gpu::vk::ImageInfo`;
- `gpu::backend_render_targets::make_vk`;
- `gpu::surfaces::wrap_backend_render_target`.

Keep one DirectContext and persistent per-slot Skia surfaces. Do not recreate the
context or surface every frame.

Extend `RenderFrame` minimally so flushing is backend-controlled. Compile a
spike against the exact skia-safe 0.99 APIs and use
`flush_surface_with_texture_state` to leave the image in a known layout while it
is still owned by the Vulkan graphics queue. Only after that flush/submit may an
application post command copy for capture, timestamp completion, and perform the
final foreign-ownership release.

Keep the `GetProc` provider and its ash instance/device handles alive for the
entire Ganesh context lifetime. Put raw-handle conversion between `ash::vk` and
rust-skia Vulkan aliases in one audited unsafe module; ABI-compatible handles are
not treated as interchangeable Rust types.

A capability spike must prove the exact returned image state and two-stage
ordering. Do not guess Ganesh's internal layout or request foreign ownership from
Skia before a later application command still needs the image.

### 6. Queue and KMS synchronization

Keep all access to the Vulkan queue, Ganesh DirectContext, command pools, and
external images on the DRM render thread.

Use application command submissions around Ganesh on the same queue:

1. after KMS/page-flip release, acquire the selected scanout image from the
   measured external/foreign owner, establish the exact layout, and optionally
   write a start timestamp;
2. connect that dependency to the exact Ganesh submission, render, and submit;
3. have Ganesh finish in a known graphics-owned layout;
4. execute an ordered post-Ganesh command buffer that performs optional readback,
   writes the end timestamp, and performs the final foreign-ownership release;
5. signal an exportable binary semaphore and a Vulkan completion fence;
6. export the semaphore payload once as a master sync-file FD;
7. duplicate the master for primary-plane `IN_FENCE_FD` in each nonblocking
   atomic commit attempt.

The exact stage masks, access masks, layouts, queue-family indices, temporary
semaphore lifetime, and destruction point must pass synchronization validation.
Queue ordering alone is not treated as a memory-visibility proof.

The Phase 1 spike must select one compiled way to connect external acquire waits
to Ganesh: either a rust-skia Vulkan `BackendSemaphore` path consumed by
`DirectContext::wait`, or another synchronization-validation-proven GPU
dependency. If skia-safe 0.99 lacks the required public wrapper, patch/contribute
that wrapper as an explicit dependency slice. A CPU wait or an unverified prior
queue submission is not an accepted substitute.

Normal Vulkan operation requires asynchronous sync-file export and KMS
`IN_FENCE_FD`. A diagnostic CPU fence wait may be used only to bring up pixels;
it must be labelled degraded and cannot produce accepted performance results.

Close the userspace in-fence FD after the atomic ioctl has consumed it. Retain
Vulkan fences/semaphores until their slot lifecycle permits reset/destruction.

### 7. Direct NV12 Vulkan Video

This extends the shared Vulkan importer only after its single-plane ABGR8888
path passes all four Emerge Demo PRIME matrix pairs. Do not create a second
Camera-only importer or move ownership policy into a presenter.

A performance result is invalid until the live Camera takes the direct Vulkan
path.

Refactor the importer boundary into an API-specific enum while preserving the
shared target registry, current/pending/retired state, stream/incarnation checks,
newest-frame behavior, and release dispatcher.

For Camera NV12:

- validate canonical object/plane ownership, offsets, strides, coded/visible
  sizes, and modifier;
- import the actual DMA-BUF objects as
  `VK_FORMAT_G8_B8R8_2PLANE_420_UNORM` when the target layout supports it;
- support shared-object and disjoint-plane binding only when explicitly proven;
- require sampled-image and sampler-YCbCr-conversion capabilities;
- wrap the imported image as a borrowed Skia backend texture carrying exact
  YCbCr conversion metadata;
- retain Video as direct media that never enters paint-layer cache payloads.

The canonical `VideoInterop.Format` has stream colorimetry/range metadata, but it
may be `:unspecified` and Emerge does not currently forward it to the native
stream specification. Add a coordinated VideoInterop/Camera/Emerge contract
slice:

- the producer advertises proven primaries, transfer, matrix, range, and chroma
  location;
- `open_consumer` forwards all five once with target/incarnation/stream identity;
- OpenGL and Vulkan consume the same explicit conversion contract;
- accepted Camera runs reject unspecified conversion instead of guessing.

Do not compare different implicit YUV conversions.

Acquire sync-file handling:

- preserve canonical prepare/duplicate/validate-before-claim and exact transfer
  receipts;
- transfer the claimed FD into a temporary Vulkan semaphore with `SYNC_FD`
  import;
- connect that wait to the exact Ganesh sampling submission through the
  Phase-1-proven mechanism;
- release the sampled image back to the proven external/foreign queue family
  after Ganesh use, and retain the semaphore and claim until the post-release
  completion ticket signals;
- preserve current frame on import/wait failure;
- release the new failed frame through the existing exact ownership path.

Retirement:

- mark the last output submission that sampled each imported frame;
- move replaced/hidden frames into a bounded retired queue;
- poll completion nonblockingly;
- drop in order: Skia Image/backend texture, Vulkan view/conversion/image/memory,
  duplicate FDs, then the canonical claim/lease;
- dispatch producer release only after GPU access is impossible.

When the retired queue reaches its bound, stop importing newer frames and keep
the current image. Never destroy/release old entries early. A sustained inability
to drain becomes a terminal session error after a defined watchdog interval.

No recurring `vkQueueWaitIdle`, CPU acquire wait, early lease release, or hidden
RGBA conversion is allowed in accepted performance runs.

### 8. Screenshots

Preserve bounded, on-demand capture only.

When a capture is requested, the post-Ganesh command buffer copies the complete
primary image to a reusable mapped staging buffer before KMS ownership release.
Publish only after both the copy fence signals and that exact generation
successfully page-flips. An `EBUSY`, failed commit, or never-presented prepared
frame cannot become the latest screenshot. Keep an explicit capture reference on
the slot until publication or failure. Invalidate noncoherent memory, convert
BGRA/XRGB to the public RGBA shape, force opaque alpha, and retain the exact
render generation.

Screenshots include composed Video and software cursor, but not a separate
hardware cursor plane, matching current semantics. No readback occurs when no
capture is pending.

### 9. GPU timing

Add a bounded Vulkan query pool with two timestamps per sampled frame:

- start after acquire synchronization and before scene/video/cursor work;
- end after all render work, optional capture copy, and final release barrier.

Read asynchronously with availability; never wait in the DRM loop. Convert using
`timestampPeriod`, handle `timestampValidBits`, and correlate each sample with:

- stats-window epoch;
- render generation/version;
- renderer-cache enabled state;
- KMS commit and eventual page-flip sequence.

Expose the result through the existing `gpu_render_elapsed` concept so GL and
Vulkan reports have equivalent meaning. Keep API-specific counters for query
pool saturation, unavailable results, wrapping, device loss, fence import/export,
and validation errors.

For serialized diagnostics, compare per-frame GL and Vulkan fences with queue
depth one. Do not compare `glFinish()` against `vkQueueWaitIdle()` unless their
scope is explicitly proven equivalent.

### 10. Shutdown and NIF safety

No new blocking work runs in a NIF call. Startup hands work to the existing
native backend thread; Vulkan and KMS state remains thread-confined. Normal NIF
calls return existing result shapes and precise startup errors.

Shutdown order:

1. close video admission and stop new render requests;
2. drain/cancel pending frames through the canonical release path;
3. stop new Vulkan/KMS submissions;
4. drain the in-flight atomic commit/page flip;
5. disable cursor/primary/CRTC where possible;
6. flush Ganesh and wait for the device on the backend thread only;
7. resolve/fail pending captures and timing samples;
8. drop SceneRenderer caches and Video SkImages while DirectContext is alive;
9. drop scanout Skia surfaces;
10. purge/defer Ganesh resources, then drop DirectContext;
11. destroy imported video and scanout Vulkan resources;
12. release producer leases;
13. remove KMS framebuffers, then drop GBM BOs;
14. destroy Vulkan device/debug messenger/instance/loader;
15. release DRM mode blobs/master last.

For Wayland, perform the analogous ordering: drop scene/Video SkImages and
wrapped output surfaces before DirectContext, then imported images, old/current
swapchains, device, `VkSurfaceKHR`, instance, and loader while the Wayland
connection/surface remain live.

On `VK_ERROR_DEVICE_LOST`, stop submissions, abandon the DirectContext, disable
KMS where applicable, destroy the lost device, and only then release producer
leases because no device can access their memory. Do not describe this as a
normal GPU drain from uncertain ownership. Never block or send to BEAM from a
resource destructor.

## Cargo and build features

Vulkan must remain optional. Preserve the existing GL-only build and ultimately
prove Vulkan-only DRM and Wayland builds do not require EGL/GL at runtime.

Target feature direction:

```text
wayland-core   = SCTK + Wayland + raw-handle dependencies
linux-opengl   = GL/EGL/glutin + skia-safe/gl + GBM import-egl + video EGL
vulkan         = ash + skia-safe/vulkan
headless-vulkan = vulkan + GBM drm-support         # PRIME producer only
headless-all   = linux-opengl + headless-vulkan
wayland        = wayland-core + linux-opengl       # compatibility/default GL
wayland-vulkan = wayland-core + vulkan             # target Vulkan-only build
wayland-all    = wayland + wayland-vulkan          # dual API
drm-core       = DRM + evdev + GBM drm-support
drm            = drm-core + linux-opengl           # compatibility GL build
drm-vulkan     = drm-core + vulkan                  # Vulkan-only build
drm-all        = drm + drm-vulkan                   # comparison firmware
```

Make `drm-core`/`wayland-core` the platform cfgs and gate OpenGL and Vulkan code
separately. Move `skia-safe/gl` out of the unconditional dependency feature list
and keep GBM `import-egl` in `linux-opengl`, so Cargo features can select APIs
independently. Narrow existing `cfg(any(wayland, drm))` blocks that actually mean
EGL/GL. During the current implementation slice, `wayland-vulkan` intentionally
implies compatibility `wayland`/OpenGL; explicit Vulkan still selects the real
Vulkan runtime, but this is not the target Vulkan-only dependency graph and is
recorded as open cfg extraction.

Extend `compiled_vulkan_backends` to accept `:headless` as the always-present
headless runtime capability. It does not add `:headless` to `compiled_backends`;
it adds `headless-vulkan`/`headless-all` to the Rustler feature set. The demo
build requests `[:headless, :wayland]` Vulkan support and remains a forced source
build until immutable Vulkan artifacts exist.

When Vulkan is not compiled, `rendering_api: :vulkan` must fail with
“Vulkan rendering support is not available in this build,” not “not implemented”
and not fallback.

Required build matrix:

| Build | Gate |
| --- | --- |
| default Wayland/GL | unchanged tests and linkage |
| `--no-default-features` | core/raster remains GL/EGL/Vulkan-free |
| headless OpenGL PRIME | frozen producer/export/lifecycle control |
| headless Vulkan PRIME | no EGL/GL linkage; explicit DMA-BUF/sync-FD export |
| headless GL + Vulkan | all four demo matrix pairs compile in one source build |
| Wayland Vulkan transitional | real explicit Vulkan runtime compiles/tests; runtime failure is precise and never falls back |
| Wayland GL + Vulkan | dual feature checks; OpenGL behavior unchanged |
| future Wayland Vulkan only | no EGL/GL dynamic dependencies |
| DRM GL only | existing path unchanged |
| DRM Vulkan only | no EGL/GL dynamic dependencies |
| DRM GL + Vulkan | dual-API comparison firmware |
| Vulkan disabled + explicit `:vulkan` | precise startup error |
| host Vulkan diagnostic | validation/readback tests |
| Nerves AArch64 validation | V3DV + validation layers |
| Nerves AArch64 release | V3DV without validation overhead |

## Nerves/V3DV packaging

Before DRM/Camera implementation, pin the external Nerves-system/Buildroot
commit and update a diagnostic RPi5 image using the actual symbols from that
tree. The workstation demo matrix does not wait for Nerves packaging:

- enable Mesa V3DV and the Vulkan loader;
- package the V3DV ICD JSON and referenced shared object;
- package validation-layer JSON/shared libraries and debug-utils support only in
  diagnostic firmware;
- include `vulkaninfo` or a small probe only in diagnostic firmware;
- probe kernel sync-file support, primary-plane fencing, DRM modifiers,
  device-node permissions, loader search paths, and ELF dependencies;
- verify immutable package/lock identities.

The probe must reject lavapipe and report the hardware/extension/format/modifier
matrix. Do not rely on host `VK_ICD_FILENAMES` paths in production. The
Vulkan-only linkage gate applies to the Emerge NIF/renderer dependency closure;
other Camera firmware components may legitimately use graphics libraries.

## Implementation phases and gates

### Phase 0A: Record the current workstation baseline — complete enough for M1

- Record the complete dirty snapshot/diff digest, rust-skia pin, selected
  workstation devices/ICD/compositor, demo assets, and the successful 240 Hz
  Wayland Vulkan/OpenGL evidence.
- Preserve the existing OpenGL-OpenGL PRIME producer/importer as the M1 control.
- Keep accepted renderer-cache/clip changes and exclude semantic-backing
  experiments.

This record is sufficient to begin M1 and does not make the dirty development
closure immutable release evidence.

### Phase 1A: Emerge Demo headless-main PRIME matrix — mandatory first validation

- Add/preserve/enforce the generic stream acquire-sync policy needed by strict
  matrix routes before any frame claim.
- Implement headless Vulkan ABGR8888 linear PRIME output and the shared Vulkan
  ABGR8888 PRIME importer described in the mandatory first-validation section.
- Preserve the existing headless OpenGL producer and Wayland OpenGL importer as
  controls; do not rewrite them around Vulkan.
- Add independent demo source/main API configuration and enable the PRIME source
  for matrix runs.
- Run OpenGL-OpenGL, Vulkan-OpenGL, OpenGL-Vulkan, then Vulkan-Vulkan in fresh
  processes.
- Pass static byte-equality, animated soak, hide/show, resize,
  disconnect/reconnect, stop/restart, fault ownership, lease/FD/RSS, and
  on-demand screenshot gates.

**Go:** all four pairs pass with explicit GPU synchronization, zero fallback,
bounded resources, and balanced canonical ownership. This is the only gate that
unlocks NV12 Camera implementation and DRM integration.

### Phase 0B: Freeze the authoritative RPi baseline before target work

- Select the common renderer state used by both target APIs and preserve the
  known-good cache-on GL firmware.
- Capture immutable GL UI-only, video-only, combined idle, active Focus, and
  recovered windows with no disjoint/fence/lease errors.
- Freeze cross-repository Emerge, Camera, libcamera, VideoInterop, system,
  kernel, Mesa, firmware, and dependency identities.

Development path overrides remain allowed through M1. Phase 1B and final
hardware acceptance require a reproducible registry-ordered closure with no
unpublished substitutions.

### Phase 1B: Authoritative target capability and ownership spike

- Cross-build the exact skia-safe 0.99 Vulkan calls for Nerves AArch64, including
  the audited ash/rust-skia raw-handle boundary and long-lived `GetProc` owner.
- Boot pinned V3DV diagnostic firmware with validation support.
- Probe DRM-device match, all external-memory flags, dedicated allocation,
  memory types, exact modifier layouts/usages, output allocator directions,
  graphics/external ownership, sync-file import/export, `IN_FENCE_FD`, timestamp
  support, and Skia mutable texture state.
- Prove one KMS-compatible Vulkan image can be cleared, transitioned, fenced,
  page-flipped, and read back with correct channel order.
- Add/verify explicit stream primaries, transfer, matrix, range, chroma-location,
  and strict acquire-sync transport. Reject unspecified/unsupported conversion
  or implicit acquire before ownership transfer.
- Only after that admission succeeds, consume a live canonical Camera holder
  through prepare/duplicate/validate, claim, acquire wait, GPU sampling,
  graphics-to-external release, completion, and dispatcher retirement. Record
  ownership receipts and assert every preclaim/postclaim failure edge. A stale
  descriptor or dumped FD is not a valid fixture.
- Prove the exact Camera NV12 object/plane/modifier can be wrapped and sampled by
  Ganesh without CPU/GL fallback.
- Select one output allocator direction and one acquire-to-Ganesh synchronization
  mechanism. Write a short measured design amendment with exact barriers,
  ownership indices, FDs, and drop order before Phase 2.

**Do not proceed with this unsafe or unproven design** if direct NV12 sampling,
canonical ownership, safe GPU acquire, output allocation, Ganesh target state,
V3DV packaging, or KMS-compatible storage cannot be proven. Record the failed
capability and revise the Vulkan architecture; failure does not remove Vulkan
from the supported-API roadmap.

### Phase 2: GL-preserving Linux decomposition and optional features

Before adding the DRM Vulkan render arm and publishing Vulkan-only variants:

- split shared DRM/KMS lifecycle from API-specific EGL/GL initialization;
- keep raster separate and introduce narrow API-owner enums for DRM and Wayland;
- centralize live Wayland raw handles and make `RendererEnv` own bracketed frame,
  resize, capture, present, and teardown access;
- make `RenderFrame` flushing backend-controlled;
- remove EGL/GL types from shared Vulkan-facing signatures;
- implement `drm-core`/`wayland-core`, OpenGL, and Vulkan cfg boundaries plus
  precise feature-availability errors and a DRM-only compiled-unwired guard;
- ensure the DRM Vulkan device profile enables no surface/swapchain/display
  extension and creates no WSI object;
- keep existing GL/raster startup, pacing, pixels, screenshots, video, timing,
  and shutdown behavior unchanged.

Run the frozen GL controls after the refactor. Do not proceed if either GL path
regresses beyond noise.

**Go:** default, no-default, compatibility GL, Vulkan, and aggregate build gates
pass; GL pixels, lifecycle, and performance remain unchanged.

### Phase 3A: Shared Vulkan engine and Wayland UI presenter — UI path complete

- Add the shared Vulkan instance/device/Ganesh/frame owner with hardware-only
  selection and one timestamp-capable graphics+present queue.
- Patch/pin the missing rust-skia Vulkan `BackendSemaphore` construction and
  `DirectContext::wait` wrappers before submitting Ganesh work.
- Add Wayland `VkSurfaceKHR`, FIFO swapchain generations, acquire/present,
  resize-only retirement waits, and frame-callback pacing.
- Use shared capture/timestamps and publish captures only after successful
  presentation acknowledgement.
- Preserve the device, DirectContext, renderer cache, and Video imports across
  swapchain resize.

**Current gate state:** ordinary hardware pixels and 240 Hz pacing pass.
Deterministic cross-API pixels, resize stress, stop/restart, screenshots,
validation layers, and device-loss remain open and are exercised first through
Phase 1A where applicable.

### Phase 3C: DRM Vulkan UI presenter

- Reuse the same Vulkan engine/frame/capture/timing owner with the exact Phase-1
  allocator and synchronization decision.
- Add modifier-aware scanout slots and full logical/owner/reference state.
- Integrate `CompletedTarget<slot>` with atomic KMS/page-flip ownership.
- Add one-shot master sync-file export with per-attempt atomic duplicates.
- Add hardware cursor and page-flip-gated screenshot acknowledgement.
- Run UI-only RPi validation without Video first.

**Go:** production runtime proves the configured KMS primary and exact Vulkan
render-node match (including negative omitted/wrong/ambiguous split-node tests),
presents exclusively through KMS atomic/page-flip with no WSI, sustains 60 Hz
exact deterministic UI, reports no validation errors, keeps slots/FDs/memory
bounded, publishes fresh latest-presented screenshots, restarts cleanly, and
uses no CPU completion wait in normal mode.

### Phase 4: Extend the validated shared importer to direct NV12 Camera Video

- Start from the Phase-1A ABGR8888 shared Vulkan importer; do not replace it or
  create presenter-specific Camera importers.
- Make video resource ownership API-specific without changing canonical public
  ownership, while keeping registry/reconciliation/current/pending/retired policy
  API-neutral.
- Add one exact NV12 external-memory importer, Phase-1-proven
  acquire-to-Ganesh wait, explicit YCbCr conversion, direct Skia image use, and
  fence-based retirement used unchanged by both presenters.
- Implement retired-queue capacity behavior: stop newer imports and preserve
  current; never release early.
- Preserve hidden-video release, current-frame preservation on failures,
  newest-frame semantics, ownership receipts, abandonment guards, and shutdown
  drainage.

**Go:** Wayland Video lifecycle/correctness gates pass and the RPi Camera path
sustains live 59-60 FPS with balanced prepare/claim/release counters, no
fallback/copy, bounded retirement, zero lifetime errors, and zero Video
paint-cache admissions/stores.

### Phase 5: Correctness and fault validation

Compare GL and Vulkan GPU readbacks for:

- Camera Focus and Shutter phases;
- cold/warm/replacement/cache-disabled paths;
- text, gradients, borders, rounded and relaxed clips, alpha, glow/shadows;
- root transforms at 0/90/180/270 degrees and reflections;
- Video ordering, contain geometry, color conversion, and crop;
- resize, restart, context replacement, screenshots, software/hardware cursor;
- acquire-fence timeout/error, invalid modifier, OOM, atomic failure, device loss,
  and killed producer/consumer shutdown.

Evaluate two references separately:

1. deterministic UI equality after only documented row-order/channel
   normalization;
2. frozen Video equality after both APIs use the same explicit colorimetry,
   range, chroma siting, crop, and filtering contract.

Default acceptance is zero differing RGBA pixels. If Skia backend rasterization
still differs, report exact counts and stop for an explicit product decision; do
not silently introduce a tolerance.

Run validation layers with synchronization validation and zero errors. Validation
is disabled for performance runs.

### Phase 6: Host guardrail

Run serialized GL/Vulkan GPU-complete Criterion cases using identical Camera
fixtures, cache warmup, sample count, and per-frame fence completion. Run each
exact case in a fresh process under the exclusive performance lock.

Host results are a correctness and catastrophic-regression guardrail only; RX
performance cannot predict V3DV. A large host regression must be understood
before target deployment.

### Phase 7: Paired RPi5 A/B

Use one dual-API release firmware so kernel, Mesa, Camera, Skia, assets, and
application code are identical. First prove that enabling Vulkan in the binary
does not regress its OpenGL path by more than 3% or 0.25 ms p95.

For each API measure:

1. UI-only while capture remains active;
2. video-only;
3. combined idle;
4. exact automated active Focus interaction;
5. recovered.

Protocol:

- authoritative renderer cache enabled, zero Video paint-cache admissions/stores,
  and reported observed queue/prepared/in-flight depth;
  the GL driver-managed GBM pool is not claimed to equal Vulkan's nominal slots;
- fixed output/video format and exact modifier where both APIs support it;
- shader/pipeline/cache warmup is proven by stable compilation/cache counters and
  reported separately from cold startup;
- after warmup, five-second discard, then at least three 60-second windows;
- alternate GL/Vulkan/Vulkan/GL, then reverse;
- record clocks, temperature, throttling, Camera FPS, draw/cache counts, video
  age, fences, leases, FDs, and KMS sequence;
- no screenshots, validation, debug logs, forced completion, or CPU wait in
  measured windows.

Classify the performance result:

- **RPi5 performance target met:** active median improves by at least 2.0 ms,
  confidence interval excludes zero, p95 does not regress, and the full
  <=10.86 ms / stable-60 target passes with the soak and lifecycle cycles.
- **Supported but optimization remains:** correctness and lifecycle gates pass,
  but Vulkan does not yet meet the RPi5 performance target. Keep explicit
  `rendering_api: :vulkan`, report the bottleneck, and continue optimization.

Performance alone never removes the Vulkan API.

## Tests

### Pure/unit tests

- rendering API compatibility and feature-availability matrix;
- instance API capping and effective instance/device promotion legality;
- composable DRM/Wayland profiles and combined graphics+present queue selection;
- separate KMS-card and explicit primary/render Vulkan-node matching, including
  wrong-field, duplicate, software, and Vulkan 1.0 rejection;
- output format/modifier intersection and rejection reasons;
- external FD ownership transfer on every failure edge;
- headless Vulkan PRIME slot state, one-shot SYNC_FD export, release-token
  reuse, backpressure, and terminal shutdown;
- generic ABGR8888 DMA-BUF import and exact linear modifier validation;
- scanout slot state transitions and atomic error handling;
- query timestamp conversion/wrap/availability;
- NV12 shared/disjoint plane validation and memory-type selection;
- acquire/retirement state machine and bounded queues;
- BGRA/XRGB screenshot conversion.

### Integration tests

- Vulkan Ganesh context/surface creation under a hardware-gated test;
- deterministic Emerge Demo PRIME matrix for OpenGL-OpenGL,
  Vulkan-OpenGL, OpenGL-Vulkan, and Vulkan-Vulkan;
- deterministic Wayland and DRM GL/Vulkan scene readback;
- Wayland swapchain resize, frame-callback pacing, and presentation acknowledgement;
- paint-layer cache cold/hit/replacement behavior on Vulkan;
- direct Video remains non-cacheable;
- on-demand screenshot freshness and no idle readback;
- explicit startup error when Vulkan is absent;
- stop/restart and device-loss cleanup.

### Required validation commands

Use `scripts/performance-lock.sh shared` for builds/tests and exclusive locking
for GPU/RPi measurements.

At minimum:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features headless-vulkan
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features headless-all
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features wayland
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features wayland-vulkan
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features wayland-all
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features drm
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features drm-vulkan
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features drm-all
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --all-targets --no-default-features --features wayland-vulkan -- -D warnings
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --all-targets --no-default-features --features drm-vulkan -- -D warnings
cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check
VIDEO_INTEROP_PATH=/workspace/video_interop mix test
cd /workspace/emerge_demo && mix test
cd /workspace/emerge_demo && ./scripts/validate-prime-rendering-matrix.sh
./ci-tests.sh
```

Also run the immutable Nerves release build, inspect linked libraries, and boot
hardware validation firmware. Run `cargo test` and `mix test` after each completed
implementation slice, with full CI at integration gates.

## Diagnostics required before performance claims

- immutable source/build/system identity;
- requested and selected headless/main rendering API pair;
- producer allocation DRM node plus consumer Vulkan physical
  device/driver/UUID and DRM match;
- output and Camera formats/modifiers/plane layouts;
- graphics queue and timestamp properties;
- validation state and message count;
- primary slot count/state and KMS in/out fence mode;
- GPU median/p95/p99 and query pool health;
- scene/render/present versions and KMS sequence;
- cache hits/stores/evictions and primary draw counts/pixels;
- Camera delivered/imported/dropped FPS and frame age;
- acquire/retirement fence counters and lease balance;
- FD/RSS/device-memory counts;
- device-loss, atomic error, page-flip timeout, and fallback counters.

A valid demo matrix result must prove direct ABGR8888 DMA-BUF transfer,
explicit acquire synchronization, balanced leases, and `fallbacks == 0`. A valid
Vulkan Camera result must additionally prove direct NV12 import, explicit
primaries/transfer/matrix/range/chroma-location conversion, and
`fallbacks == 0`.

## Error policy

| Failure | Policy |
| --- | --- |
| Vulkan not compiled | precise startup error |
| explicit Vulkan startup failure | return error; no fallback |
| DRM/Vulkan device mismatch | startup error |
| no unique Wayland graphics+present hardware device/queue | startup error |
| headless PRIME producer DRM node is absent/ambiguous/incompatible | startup error; no fallback |
| headless Vulkan sync-FD export or slot ownership failure | poison producer; drain leases before teardown |
| Wayland surface/swapchain unavailable | startup error; no fallback |
| missing required external-memory/sync/modifier support | startup error |
| no output modifier intersection | startup error |
| no direct Camera NV12 capability | Camera performance lane rejected |
| individual video import/acquire failure | release new frame, retain current |
| timestamp unavailable | disable GPU timing and reject performance claim |
| screenshot failure | fail that request; renderer may continue |
| atomic `EBUSY` | close attempt FD, retain master sync-file and prepared frame, retry with a fresh duplicate |
| atomic `EINVAL`, lost master, page-flip timeout | poison/stop session |
| optional cache OOM | purge optional cache and retry once |
| `VK_ERROR_DEVICE_LOST` | fatal session drain/teardown |

## Fallback and maintenance

- Existing OpenGL-first/raster-fallback `:auto` behavior remains unchanged until
  a separate selection-policy decision.
- Users can explicitly choose `rendering_api: :opengl` on unsupported or
  temporarily regressed Vulkan systems.
- Do not attempt live API migration with active Video leases.
- Namespace any persistent Vulkan cache by driver/device UUID and invalidate it
  safely on mismatch.
- Capability or correctness failures block release of the affected Vulkan path
  until fixed; performance shortfalls do not remove the API.

## Next implementation sequence

Do not begin with the full DRM renderer or Camera-specific NV12 code.

1. Add and enforce the generic stream acquire-sync policy required by the
   matrix; leave Camera-specific color metadata for M4.
2. Complete the shared external-image, sync-FD, consumer release-barrier, and
   post-Ganesh completion contracts needed by a generic leased frame.
3. Implement headless Vulkan ABGR8888 PRIME output while preserving the existing
   OpenGL producer.
4. Implement shared Vulkan ABGR8888 PRIME input for the Wayland main renderer
   while preserving the existing OpenGL importer.
5. Run and accept the four Emerge Demo `headless-main` pairs. Stop here on any
   pixel, synchronization, ownership, lease, FD, or shutdown failure.
6. Freeze the immutable target GL baseline. On the RPi5, run the split-node
   V3DV probe and answer these four remaining
   target questions:
   - Can Ganesh wrap and control the final state of a KMS-compatible V3DV image?
   - Can the actual Camera NV12 DMA-BUF/modifier be imported and sampled with the
     explicit negotiated color conversion?
   - Can Vulkan export a render-completion sync FD accepted by primary-plane
     `IN_FENCE_FD`, with exact ownership barriers and retry-safe duplication?
   - Can the complete feature set cross-build and boot in the pinned Nerves image?
7. Record one output allocator direction and exact ownership/barrier amendment,
   then implement DRM UI, extend the already-validated importer to NV12, and run
   the full Camera lifecycle/correctness/performance gates.

A “no” at the demo matrix stops generic PRIME work before Camera complexity. A
“no” to any target question stops the proposed DRM/V3DV architecture before
unsafe resource integration and requires a measured amendment; it does not
remove explicit Vulkan from the roadmap.
