# Active Plan: Backend / Renderer Unification and Headless Output

Created: 2026-05-27
Status: phase 2 complete

## Confirmed decisions

- Public renderer selector option: `backend_renderer`.
- Default renderer selector: `:auto`.
- GL renderer atom: `:gl`.
- Headless platform atom: `backend: :headless`.
- Headless-specific options should be nested under `headless: [...]`.
- Headless binary output defaults to `pixel_format: :rgba8888`.
- `:bw1` output should include a polarity option.
- Headless mode should include a targeted FPS option.
- Headless should support input/synthetic input in a later phase.
- Dithering is a later slice and needs a separate larger design discussion.
- macOS should follow the same `backend` + `backend_renderer` convention.
- Remove `macos_backend`; passing it should raise a migration/deprecation error
  telling callers to use `backend_renderer` instead.
- `backend_renderer: :gl` should fail startup if GL/EGL is unavailable.
- `backend_renderer: :auto` should make the default path work with whatever
  renderer/present path is available on the selected platform.
- `backend_renderer: :auto` fallback should cover startup and should also
  recover from runtime renderer/present failure where safe.
- Wayland/DRM `backend_renderer: :auto` should try `:gl` and fall back to
  `:raster` when the raster presentation path is available.
- True raster fallback without GL should eventually use CPU-present paths:
  Wayland `wl_shm` and DRM dumb-buffer / CPU KMS presentation.
- The first Wayland/DRM raster implementation should cover both useful paths:
  CPU raster + GPU upload for testing when GL exists, and true CPU present for
  GL-unavailable fallback.
- macOS should reject `backend_renderer: :gl`.
- Linux backends should reject `backend_renderer: :metal`.
- `backend_renderer: :metal` is macOS-only for current windowed backends; future
  macOS headless may support Metal too.
- Renderer cache should be disabled by default for raster renderer mode, and
  stats should include the disable reason. Existing `renderer_cache:
  [enabled: true]` is enough to explicitly opt into raster paint-layer caching;
  raster cache payloads use normal CPU memory instead of GPU resources.
- `backend_renderer: :auto` runtime failure recovery should try seamless
  renderer migration where possible. If migration fails, retry a few times, then
  stop the renderer/session and surface an error.
- Stats should expose requested and selected renderer as structured data, e.g.
  `backend_renderer: %{requested: :auto, selected: :gl}`, while logs can display
  `backend_renderer: :auto (:gl)`.
- Stats should expose meaningful renderer-specific data. If a renderer has no
  GPU, stats should not expose a GPU section.
- macOS runtime convergence should happen early, ideally in the first or second
  implementation slice.
- macOS may keep its AppKit-driven host topology. The convergence requirement is
  reuse of shared runtime semantics, shared `TreeMsg` / `RenderMsg` boundaries,
  and shared tests; matching Linux's multi-thread actor topology is not required
  unless profiling or correctness gaps justify it.
- Current one-shot tree-render APIs should become hard errors. Keep the
  `render_to_pixels`/`render_to_png` names, but change their signatures to
  accept a renderer handle and return output from that renderer's latest frame.
- Screenshot calls should return `{:ok, binary} | {:error, term}`.
- Screenshot capture should return the latest already-presented frame, not force
  rendering of pending dirty scene work.
- Screenshot options should include pixel format, scale, region, background,
  timeout, and PNG compression options.
- Add a renderer info query, e.g. `EmergeSkia.renderer_info(renderer)`, so code
  can inspect requested/selected backend renderer outside stats.
- `backend_renderer: :raster` is shorthand for
  `backend_renderer: [raster: [present: :auto]]`.
- Wayland/DRM raster rendering can use the nested renderer option to force the
  raster present path: `backend_renderer: [raster: [present: :auto | :gpu_upload | :cpu]]`.
- `backend_renderer: :auto` can also carry fallback options, e.g.
  `backend_renderer: [auto: [raster: [present: :cpu]]]`.
- macOS should reject invalid renderer config, including Linux-only raster
  present overrides, while still accepting valid `:auto`, `:metal`, and
  `:raster` selections.
- Runtime renderer migration should emit a message such as
  `{:emerge_skia_renderer_changed, renderer, from: :gl, to: :raster, reason: reason}`
  to all relevant targets (log target, starter/session owner, viewport/runtime
  observers where available); viewport code should log it.
- Investigate whether off-main-thread drawing on macOS makes sense at all.
  Prefer drawing off the AppKit main thread and presenting on the main thread
  only if benchmarks/inspection show it helps; simpler code is acceptable if
  off-main-thread drawing is too complex or not beneficial.
- First unify the current backends (`:wayland`, `:drm`, `:macos`) under one
  model. Headless work comes after that foundation is done.
- PRIME/dma-buf output is later work, not part of the first headless slice.

## Goal

Make every runtime backend follow the same selection model:

```text
platform backend  +  renderer backend  +  output mode/pixel format
```

Examples the final API should be able to express:

- Wayland window + GL renderer
- Wayland window + raster renderer
- DRM/KMS output + GL renderer
- DRM/KMS output + raster renderer
- macOS window + Metal renderer
- macOS window + raster renderer
- headless session + raster renderer + binary frame delivery
- headless session + GPU renderer + PRIME/dma-buf frame delivery
- later: any supported platform + Vulkan renderer where available

The main architectural change is to stop treating `backend` as both the display
system and the rendering technology. `backend` should mean the platform/session
backend. A separate renderer selection should choose GL/raster/Metal/Vulkan.

## Non-goals for this slice

- Do not implement Vulkan yet.
- Do not remove the external macOS host requirement; AppKit still needs a host
  process that owns the macOS main thread.
- Do not redesign Elixir reconciliation, retained layout, event dispatch, or
  paint-layer caching as part of this work.
- Do not make `video_target` and headless output the same public API. They are
  related concepts, but `video_target` is currently an input into the render
  tree while headless output is frame delivery from the renderer.

## Current context

Relevant existing pieces:

- Public start options live in `lib/emerge_skia.ex` and
  `lib/emerge_skia/options.ex`.
- Native windowed Linux startup flows through `EmergeSkia.Transport.Native` and
  `Native.start_opts/1`.
- macOS startup flows through `EmergeSkia.Transport.MacosHost` and the external
  `native/emerge_skia/src/bin/macos_host.rs` process.
- macOS already has a renderer-like option, `macos_backend: :auto | :metal |
  :raster`, but it is backend-specific naming.
- `native/emerge_skia/src/backend/raster.rs` is currently an offscreen CPU
  renderer helper, not a full runtime backend for viewport sessions.
- `EmergeSkia.render_to_pixels/2` and `render_to_png/2` are synchronous
  one-shot offscreen tree-render APIs today. They should be deprecated/migrated
  toward renderer-state screenshot APIs so capture uses the current retained
  state/backend/renderer instead of a separate raster-only tree path.
- `video_target` has existing PRIME descriptor support and validation that can
  inform, but should not dictate, headless output shape.
- `plans/platform-runtime-architecture-differences.md` explains current macOS vs
  Linux orchestration differences.

## Terminology

Use these terms consistently in code and docs:

- **Platform backend**: owns the session/window/output device, input source,
  lifecycle, event loop, wake/present timing, and OS-specific handles.
  Candidates: `:wayland`, `:drm`, `:macos`, `:headless`.
- **Renderer backend**: owns how a `RenderScene` is drawn. Simple selections are
  `:auto`, `:gl`, `:raster`, `:metal`, and future `:vulkan`.
  `backend_renderer: :raster` means raster with default `present: :auto`; raster
  can also be configured as
  `backend_renderer: [raster: [present: :auto | :gpu_upload | :cpu]]`.
  `:auto` can be configured with fallback options as
  `backend_renderer: [auto: [raster: [present: :auto | :gpu_upload | :cpu]]]`.
- **Output mode**: how completed frames leave the renderer. Windowed backends
  present to their surface/device. Headless can deliver `:binary` frames or
  `:prime` descriptors to a process.
- **Pixel format**: memory/descriptor format of delivered or presented pixels,
  especially for headless binary output and devices such as e-ink screens.

## Public API direction

Preferred start shape:

```elixir
EmergeSkia.start(
  otp_app: :my_app,
  backend: :headless,
  backend_renderer: :raster,
  width: 800,
  height: 480,
  headless: [
    target: self(),
    mode: :binary,
    pixel_format: :rgb888
  ]
)
```

Windowed examples:

```elixir
EmergeSkia.start(otp_app: :my_app, backend: :wayland, backend_renderer: :gl)
EmergeSkia.start(otp_app: :my_app, backend: :wayland, backend_renderer: :raster)
EmergeSkia.start(otp_app: :my_app, backend: :wayland, backend_renderer: [raster: [present: :cpu]])
EmergeSkia.start(otp_app: :my_app, backend: :drm, backend_renderer: :gl)
EmergeSkia.start(otp_app: :my_app, backend: :drm, backend_renderer: :raster)
EmergeSkia.start(otp_app: :my_app, backend: :drm, backend_renderer: [raster: [present: :gpu_upload]])
EmergeSkia.start(otp_app: :my_app, backend: :macos, backend_renderer: :metal)
EmergeSkia.start(otp_app: :my_app, backend: :macos, backend_renderer: :raster)
```

Compatibility/migration:

- Keep `backend` as the platform backend name.
- Add `backend_renderer` as the cross-platform renderer selector.
- Remove `macos_backend` from the accepted runtime options. If callers pass it,
  raise a clear migration/deprecation error telling them to use
  `backend_renderer`.
- Document `backend_renderer: :auto` defaults per platform:
  - macOS: prefer Metal, fall back to raster.
  - Wayland/DRM: try GL first and fall back to raster when the raster
    presentation path is available, so the default works with whatever renderer
    path is available. True fallback without GL requires CPU-present support
    (`wl_shm` on Wayland, dumb-buffer / CPU KMS on DRM). Auto fallback can be
    configured with `backend_renderer: [auto: [raster: [present: ...]]]`.
  - headless: prefer raster for `mode: :binary`; prefer GPU for `mode: :prime`
    if PRIME export is requested and supported.
- Explicit renderer choices should not silently fall back. For example,
  `backend_renderer: :gl` fails if GL/EGL is unavailable, macOS rejects
  `backend_renderer: :gl`, and Linux rejects `backend_renderer: :metal`.

`backend_renderer` is the chosen public option name. Avoid reusing bare
`renderer` because `renderer` already commonly means the session handle.

## Headless backend semantics

Headless is deferred until the current window/device backends share the same
platform + renderer selection model. This section records the intended target
shape for the later headless phase.

A headless runtime backend is a retained session, not just a synchronous
`render_to_pixels/2` call.

Startup requires:

- `width`
- `height`
- a target pid/process for delivered frames
- output mode
- pixel format, defaulting to `:rgba8888`

Headless-specific options should be nested under `headless: [...]`:

```elixir
headless: [
  target: pid(),
  mode: :binary | :prime,
  pixel_format: :rgba8888 | :rgb888 | :gray8 | :gray4 | :gray2 | :bw1,
  bw1_polarity: :one_is_black | :one_is_white,
  target_fps: pos_integer() | nil,
  frame_message: :emerge_skia_frame
]
```

`target_fps` controls the requested delivery cadence for retained headless
output. Exact pacing/drop behavior should be decided when the headless backend
is implemented.

Frame delivery should be message based:

```elixir
{:emerge_skia_frame, renderer, frame}
```

For binary mode, `frame` should include at least:

```elixir
%{
  mode: :binary,
  sequence: non_neg_integer(),
  width: pos_integer(),
  height: pos_integer(),
  scale: float(),
  pixel_format: atom(),
  stride_bytes: pos_integer(),
  data: binary(),
  timestamp_native: integer()
}
```

For PRIME mode, `frame` should include descriptor data similar to existing
video-target PRIME descriptors, but for produced frames:

```elixir
%{
  mode: :prime,
  sequence: non_neg_integer(),
  width: pos_integer(),
  height: pos_integer(),
  drm_format: non_neg_integer(),
  modifier: non_neg_integer() | nil,
  planes: [map()],
  timestamp_native: integer()
}
```

The exact PRIME fd ownership contract must be specified before implementation:
who owns/ closes fds, whether descriptors are one-shot, and whether explicit
release/ack messages are needed for backpressure.

## Pixel format plan

Initial binary pixel formats should cover common use cases:

- `:rgba8888`
- `:rgb888`
- `:gray8`
- `:gray4`
- `:gray2`
- `:bw1`

For packed formats (`:gray4`, `:gray2`, `:bw1`) define and test:

- bit ordering within a byte
- row stride/padding
- black/white polarity for `:bw1`, exposed as an option such as
  `bw1_polarity: :one_is_black | :one_is_white`
- thresholding behavior
- alpha handling before grayscale conversion

Dithering is intentionally not part of the first headless/pixel-format slice. It
needs a separate design discussion because the right algorithm and configuration
can depend heavily on display technology, update cadence, ghosting behavior, and
power constraints.

E-ink use cases should not require callers to receive RGBA and convert in
Elixir. Conversion should happen natively after Skia rendering/readback and
before frame delivery.

## Capability matrix to design first

Create a small explicit compatibility matrix before changing backend code.
Current-backend rows are the first implementation target; headless rows are
later.

Draft target matrix:

| Platform backend | Renderer backend | Output | Notes |
| --- | --- | --- | --- |
| `:wayland` | `:gl` | window | Current EGL path. |
| `:wayland` | `:raster` | window | Needs raster draw plus both GPU-upload present for testing and `wl_shm` CPU present for true GL-free fallback. |
| `:drm` | `:gl` | KMS | Current/future EGL+GBM path. |
| `:drm` | `:raster` | KMS | Needs raster draw plus both GPU-upload present for testing and dumb-buffer / CPU KMS present for true GL-free fallback. |
| `:macos` | `:metal` | window | Current host path. |
| `:macos` | `:raster` | window | Current host fallback path. |
| `:headless` | `:raster` | binary | First headless path to implement. |
| `:headless` | `:gl` | binary | Optional; may require readback. |
| `:headless` | `:metal` | binary | Future macOS headless path. |
| `:headless` | `:gl` | prime | Requires dma-buf/PRIME export support. |
| any | `:vulkan` | varies | Future only. |

Unsupported combinations should fail during startup with precise errors, not
silently fall back unless `backend_renderer: :auto` was requested.

## Architecture direction

### 1. Normalize config around the split

Introduce shared config concepts in Elixir and Rust:

```text
PlatformBackendKind = wayland | drm | macos | headless
RendererBackendKind = auto | gl | raster | metal | vulkan
HeadlessOutputMode = binary | prime
PixelFormat = rgba8888 | rgb888 | gray8 | gray4 | gray2 | bw1 | ...
```

Keep the Elixir option normalization authoritative for public errors, then pass
normalized strings/atoms to native/host layers.

### 2. Separate platform startup from render-surface creation

Refactor backend startup around two decisions:

1. platform backend creates/owns the session and output target
2. renderer backend creates/owns the drawing surface for that target

Avoid a large trait hierarchy at first. Prefer simple enums, factories, and
shared helpers until at least two real backends use the same abstraction.

### 3. Keep runtime semantics shared

Wayland/DRM/headless should use the same actor-backed runtime semantics where
possible:

```text
input/source events -> event runtime -> tree update -> render scene -> output
```

Headless has no OS input source by default, but upload/patch/animation/asset
changes should still drive the same tree update and render publication path.
Headless should support input/synthetic input later through the same shared
runtime semantics, not through a separate headless-only event model.

macOS can remain externally hosted, but it should consume the same normalized
platform/renderer config and report the selected renderer backend in the same
shape as other platforms.

### 4. Introduce output sinks

Model final frame publication as a backend-specific sink:

- window/compositor present sink
- DRM/KMS page-flip sink
- headless pid binary sink
- headless pid PRIME descriptor sink

This keeps renderer backends focused on drawing and format/export, while
platform backends handle delivery, present timing, and backpressure.

### 5. Screenshot APIs replace one-shot raster tree rendering

The current `render_to_pixels/2` and `render_to_png/2` APIs render a supplied
Elixir tree through a separate synchronous raster path. They do not reflect the
current retained renderer state, selected platform backend, selected renderer,
assets, animations, scroll offsets, or runtime state.

Keep the names, but change their signatures and semantics so they ask the given
renderer actor/session for the latest rendered frame. They should not accept a
new Elixir tree and should not use the old one-shot raster tree-render path.

```elixir
{:ok, pixels} = EmergeSkia.render_to_pixels(renderer, pixel_format: :rgba8888)
{:ok, png} = EmergeSkia.render_to_png(renderer, png: [compression: :default])
```

Options should include pixel format, scale, region, background, timeout, and PNG
compression options where applicable.

Capture returns the latest already-presented frame. It should not force render
pending dirty work just to satisfy a screenshot request.

The old tree-render signatures should become hard errors with migration guidance
rather than warning-backed aliases. These APIs should not be the foundation for
headless runtime work; headless is a retained output backend.

### 6. Treat PRIME as a capability, not a global mode

PRIME output requires a renderer/device path that can export descriptors. It
should be advertised through backend capabilities and rejected early when not
available.

Do not make raster+PRIME appear supported unless there is a real dma-buf CPU
allocation/write path.

## Headless backpressure and NIF safety

Headless frame delivery crosses native threads and BEAM processes, so follow the
Rustler/NIF safety rules explicitly:

- Store the destination as a `LocalPid` in the renderer resource/session state.
- Send frames from native worker/runtime threads using `OwnedEnv`/Rustler send
  patterns; never block a normal NIF scheduler waiting for the receiver.
- Return/send binaries as BEAM binaries, not `Vec<u8>` terms.
- Do not hold renderer resource locks while rendering, converting pixels, or
  sending frame messages.
- Decide a backpressure policy before implementation:
  - drop-oldest when a newer frame supersedes it,
  - block the render thread until capacity is available, or
  - require explicit frame release/ack for PRIME descriptors.
- PRIME fd lifetime must be deterministic and tested.

## Implementation phases

Keep phases inspectable and independently reviewable. Prefer small slices with
clear config/state/stat assertions over large backend rewrites.
Convergence should remove platform-specific orchestration where shared runtime
code can own the behavior; avoid adding permanent wrapper layers that leave the
macOS host with duplicate event/tree/render control flow.

### Phase 1: Public config and migration errors

Status: implemented in this slice.

- [x] Add `backend_renderer` option normalization with default `:auto`.
- [x] Accepted renderer selections for this plan: `:auto`, `:gl`, `:raster`,
  `:metal`, future `:vulkan`, configured raster form
  `backend_renderer: [raster: [present: :auto | :gpu_upload | :cpu]]`, and
  configured auto fallback form
  `backend_renderer: [auto: [raster: [present: :auto | :gpu_upload | :cpu]]]`.
  `:raster` is equivalent to `[raster: [present: :auto]]`.
- [x] Remove `macos_backend` as an accepted option and raise a clear migration
  error if callers still pass it.
- [x] Preserve current default behavior where possible under
  `backend_renderer: :auto`:
  - Wayland/DRM keep the current GL path until raster fallback is implemented.
  - macOS keeps the current host behavior: prefer Metal and fall back to raster.
- [x] Explicit renderer choices fail when unsupported; only `:auto` may
  fallback.
- [x] macOS rejects `backend_renderer: :gl` and Linux-only nested raster present
  config.
- [x] Linux rejects `backend_renderer: :metal`.
- [x] Do not implement headless in this phase; `backend: :headless` startup
  returns a clear `not implemented` error.
- [x] Add tests for valid/invalid combinations and migration-error handling.
- [x] Update docs to describe the platform/render split.

### Phase 2: macOS runtime convergence

- Status: implemented in this slice.
- Converge macOS runtime orchestration early, before or alongside broader
  renderer-selection refactors.
- Keep the external macOS host process for AppKit/main-thread ownership.
- Keep the AppKit-driven host topology on macOS. Each viewport/session owns its
  own event runtime, tree update pump, and render pump, while AppKit text-input
  hooks stay synchronous on the main thread.
- Multiple macOS viewports should each get their own runtime pump. Only
  host-level AppKit/main-thread resources and other unavoidable macOS state
  should be shared.
- Keep the backend/window structure platform-specific, but make event/tree/render
  orchestration converge.
- Preserve macOS text-input/AppKit constraints while sharing stale-lane,
  registry-install, buffered-input replay, present timing, and render-state
  installation semantics.
- Investigate whether drawing off the AppKit main thread makes sense at all.
  Prefer drawing off the AppKit main thread and presenting on the main thread
  only if benchmarks/inspection show it improves frame behavior. Desired signal
  includes lower frame latency, fewer AppKit stalls, lower render time, and not
  making the shared topology overly complex.
- Add shared tests at the `DirectEventRuntime` / `TreeUpdateEngine` boundary and
  macOS host-path tests for registry synchronization behavior.
- [x] Added a shared `HostEventRuntime` / `TreeUpdateEngine` boundary guard for
  the macOS-style direct host path: stale listener input buffers, a tree update
  publishes the registry response, and replay leaves the host runtime fresh with
  no buffered input left.
- [x] Removed the dead macOS host `session_running` request/reply path; the
  Elixir host process already answers `running?/1` from cached session state.
- [x] Cleaned up macOS-host-only clippy findings while inspecting the host path,
  including replacing the tuple-heavy start-session decoder with a named
  `DecodedStartSession`.
- [x] Removed the unused macOS tree-update policy wrapper; host tree messages
  now use the single `process_tree_messages` path with `ReturnErr`.
- [x] Moved the macOS session handoff into a per-session runtime pump that owns
  `HostEventRuntime`, `TreeUpdateEngine`, render state, dirty state, and stats.
- [x] Publish macOS layout output through `RenderMsg::Scene` before installing
  render state, matching the shared tree-to-render message boundary.
- [x] Deleted the duplicate macOS helper layer for layout installation,
  upload/patch wrappers, and direct event/tree/render state ownership from
  `HostSession`.

### Phase 3: Capability matrix for current backends

- Add shared normalized config types for platform backend and renderer backend
  on the Elixir/native/host boundary.
- Add explicit compatibility checks for current backends before startup work.
- Explicit renderer choices should fail when unsupported; only `:auto` may
  fallback.
- Keep current working paths unchanged while adding the matrix:
  - Wayland + GL
  - DRM + GL
  - macOS + Metal/raster through the host
  - synchronous raster offscreen APIs
- Include the future Linux CPU-present raster fallback rows in the matrix even
  before those paths are implemented.

### Phase 4: Renderer-aware stats, diagnostics, and info

- Report requested and selected renderer backend as structured data, e.g.
  `backend_renderer: %{requested: :auto, selected: :gl}`.
- Logs may display the same data compactly as `backend_renderer: :auto (:gl)`.
- Add a public renderer info query, e.g. `EmergeSkia.renderer_info(renderer)`,
  that exposes selected platform backend, requested renderer backend, selected
  renderer backend, and relevant capabilities without requiring stats to be
  enabled. Target shape:

  ```elixir
  %{
    backend: :wayland,
    backend_renderer: %{requested: :auto, selected: :gl},
    capabilities: %{
      gpu: true,
      renderer_cache: true,
      screenshot: true,
      raster_present: [:gpu_upload, :cpu]
    }
  }
  ```
- Keep GPU-only timings/counters out of raster stats. If there is no GPU, there
  should be no GPU stats section.
- For raster mode, default renderer-cache stats should show an explicit disabled
  reason such as `reason: :raster_renderer`.
- If raster cache is explicitly enabled for experiments/tests, paint-layer cache
  semantics should stay the same as GPU modes, but raster payloads should be
  stored in normal CPU memory. Stats should show the actual enabled/disabled
  state per supported family.
- Keep present/pipeline timings platform-specific enough to distinguish GL swap,
  raster upload/present, Metal present, and future headless frame delivery.

### Phase 5: Screenshot API migration

- Keep the `render_to_pixels` and `render_to_png` names, but change their public
  signatures to accept a renderer handle and request the latest frame from that
  renderer actor/session instead of accepting a tree.
- Make old tree-render signatures hard errors with migration guidance.
- Return `{:ok, binary} | {:error, term}`.
- Return the latest already-presented frame; do not force pending dirty work to
  render synchronously.
- Support options for pixel format, scale, region, background, timeout, and PNG
  compression where applicable.
- Ensure screenshot capture uses the selected platform/backend renderer where
  possible instead of a separate raster-only tree-render path.
- Ensure both native and macOS transports implement the same renderer-handle
  screenshot contract.

### Phase 6: Current Linux GL backend split

- Refactor Wayland and DRM startup around two decisions:
  1. platform backend creates/owns window/device, input, wake, present timing,
     and lifecycle
  2. renderer backend creates/owns the drawing surface/render target
- Extract GL/EGL surface setup into helpers that are selected by
  `backend_renderer: :gl` instead of being implicit in the platform backend.
- Keep actor-backed runtime semantics and existing input behavior unchanged.
- Keep the public behavior unchanged for default `:auto` runs.

### Phase 7: Wayland raster renderer support

- Add Wayland + raster presentation support.
- Implement CPU raster + GPU upload for explicit raster testing when GL exists.
- Implement true CPU-present fallback with `wl_shm` so `backend_renderer: :auto`
  can still work when GL is unavailable.
- Explicit `backend_renderer: :raster` or `backend_renderer: [raster: ...]`
  should mean Skia draws through the raster renderer, even if presentation
  uploads the resulting pixels.
- Use the nested `present` option to force raster present path (`:auto`, GPU
  upload, or CPU present) so tests can exercise both paths.
- Renderer cache is disabled by default for raster rendering; existing
  `renderer_cache: [enabled: true]` is enough explicit opt-in for raster
  paint-layer caching with CPU-memory payloads.

### Phase 8: DRM raster renderer support

- Add DRM + raster presentation support.
- Implement CPU raster + GPU upload for explicit raster testing when GL exists.
- Implement true CPU-present fallback with dumb-buffer / CPU KMS presentation so
  `backend_renderer: :auto` can still work when GL is unavailable.
- Explicit `backend_renderer: :raster` or `backend_renderer: [raster: ...]`
  should mean Skia draws through the raster renderer, even if presentation
  uploads the resulting pixels.
- Use the nested `present` option to force raster present path (`:auto`, GPU
  upload, or CPU present) so tests can exercise both paths.
- Renderer cache is disabled by default for raster rendering; existing
  `renderer_cache: [enabled: true]` is enough explicit opt-in for raster
  paint-layer caching with CPU-memory payloads.

### Phase 9: First real headless backend, raster + binary

- Start only after current backends share the common platform + renderer model.
- Add retained headless runtime session using the shared tree update/render path.
- Use nested `headless: [...]` options, defaulting binary pixel format to
  `:rgba8888`.
- Add `bw1_polarity` for 1-bit black/white output.
- Add `target_fps` as the requested delivery cadence.
- Render with the raster renderer into CPU pixels.
- Convert to requested binary pixel format.
- Deliver `{:emerge_skia_frame, renderer, frame}` messages to the configured pid.
- Cover upload, patch, asset refresh, animation frame, target-FPS pacing, stop,
  and receiver-death behavior.

### Phase 10: Headless GPU and PRIME output

- Add headless GL surface/device setup where supported.
- Add binary readback for GPU headless if useful.
- Add macOS headless Metal support if a Metal offscreen path is needed.
- Add headless input/synthetic-input support through the shared event runtime in
  a later phase.
- Add PRIME export output with explicit fd ownership/backpressure rules.
- Reuse descriptor validation concepts from `video_target`, but keep the public
  headless output API separate.

### Phase 11: Future renderer backends

- Add `:vulkan` only after the split is stable.
- Treat Vulkan like another renderer backend selected through the same config and
  compatibility matrix.

## Tests and validation expected for future implementation

Elixir:

- option normalization for `backend` and `backend_renderer`
- migration error when callers use removed `macos_backend`
- current-backend startup errors for unsupported combinations
- Linux rejects `backend_renderer: :metal`
- macOS rejects `backend_renderer: :gl` and Linux-only nested raster present
  config
- renderer cache disabled by default for raster renderer mode, with disable
  reason exposed in stats
- `render_to_pixels` / `render_to_png` new latest-frame signatures, ok-tuple
  returns, supported options, plus hard errors for old tree-render signatures
- `renderer_info/1` or equivalent selected-renderer introspection
- later: nested `headless` options, default `:rgba8888` pixel format,
  `bw1_polarity`, `target_fps`, and frame message shape tests

Rust:

- config decode and compatibility matrix tests
- current Wayland/DRM/macOS-default behavior unchanged for default paths
- macOS rejects GL renderer selection
- Linux rejects Metal renderer selection
- renderer-aware stats snapshots/log labels remain meaningful per renderer
- screenshot capture uses the latest already-presented frame from the current
  retained renderer/session
- renderer info query reports requested and selected renderer backends
- raster paint-layer cache stores CPU-memory payloads when explicitly enabled
- later: pixel format conversion fixture tests, especially packed 1/2/4-bit
  formats, excluding dithering until its separate design slice
- later: headless raster frame delivery and target-FPS pacing tests
- later: PRIME descriptor/fd lifetime tests

Manual/integration:

- Wayland GL default smoke
- DRM GL default smoke
- Wayland raster GPU-upload smoke once implemented
- Wayland `wl_shm` fallback smoke once implemented
- DRM raster GPU-upload smoke once implemented
- DRM dumb-buffer / CPU KMS fallback smoke once implemented
- macOS Metal/raster smoke through `emerge_demo`
- later: headless raster binary smoke with RGB and e-ink-style packed formats
- later: PRIME smoke only on hardware/CI with dma-buf support

Minimum validation for code slices remains:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml
mix test
```

## Open questions

- Should `bw1_polarity` default to `:one_is_black`, `:one_is_white`, or be
  required whenever `pixel_format: :bw1` is selected?
- Should headless frame delivery drop stale frames by default or apply
  backpressure?
- What exact `target_fps` pacing semantics should headless use when rendering is
  slower or faster than the target cadence?
- For macOS, does investigation show off-main-thread drawing makes sense, or is
  simpler main-thread-oriented code better?
