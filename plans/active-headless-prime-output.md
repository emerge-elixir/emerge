# Active Plan: Linux Headless PRIME Output

Status: planned, not implemented.

## Goals

Add Linux headless PRIME/dma-buf output for retained headless sessions:

```elixir
EmergeSkia.start(
  otp_app: :my_app,
  backend: :headless,
  backend_renderer: :auto,
  width: 800,
  height: 480,
  headless: [target: self(), mode: :prime]
)
```

Also change headless `backend_renderer: :auto` selection:

- `headless: [mode: :binary]`: try `:gl` first, fall back to `:raster` if GL
  startup fails.
- `headless: [mode: :prime]`: try `:gl`; fail if GL or PRIME export is not
  available. Do not fall back to raster because raster has no real dma-buf export
  path.

## Current state

- Headless raster binary output works.
- Explicit Linux headless GL binary output works through offscreen EGL/GL pbuffer
  rendering and synchronous RGBA readback.
- `headless: [mode: :prime]` is rejected in option normalization/startup.
- `backend_renderer: :auto` for headless currently resolves to raster.
- Existing `video_target` PRIME code supports importing external PRIME
  descriptors into GL. Headless PRIME needs the opposite direction: export
  renderer-produced frames.

## Reference: `../membrane_video_linux/`

Use this repo as the PRIME interoperability target.

Relevant files inspected:

- `../membrane_video_linux/lib/membrane/prime.ex`
- `../membrane_video_linux/lib/membrane/h265/decoder.ex`
- `../membrane_video_linux/native/h265_prime_decoder/src/lib.rs`
- `../membrane_video_linux/lib/membrane/display/sink.ex`
- `../membrane_video_linux/native/drm_prime_sink/src/lib.rs`

Findings to mirror:

- Descriptor structs are `%Membrane.PrimeDesc{}`, `%Membrane.PrimeObject{}`, and
  `%Membrane.PrimePlane{}`.
- `%Membrane.PrimeDesc{}` fields are:
  - `width`
  - `height`
  - `format`
  - `objects`
  - `planes`
  - `keepalive`
  - `owner_pid`
  - `trace_token`
- Object fields are `fd` and `modifier`.
- Plane fields are `obj_idx`, `pitch`, and `offset`.
- Native `Fd` encoders duplicate fds before returning them to BEAM.
- Native `Fd` decoders take ownership of the integer fd passed back into a NIF.
- The H265 PRIME decoder keeps the underlying decoded frame alive via a
  `keepalive` resource. When a downstream sink is done, it sends
  `{:keepalive, keepalive}` to `owner_pid`; the decoder worker then calls its
  native `keepalive_release/1` NIF.
- The DRM PRIME sink imports fds with `prime_fd_to_buffer`, creates a KMS FB with
  `add_planar_framebuffer`, and holds GEM handles until the frame is no longer
  pending/in-flight/stale.
- The DRM PRIME sink queue uses latest-pending replacement: replacing `pending`
  releases the old pending descriptor through the keepalive protocol.
- The DRM PRIME sink collapses per-object modifiers to one common modifier for
  `drm-rs` `PlanarBuffer`; mixed modifiers are rejected.

Implications for Emerge:

- Prefer Membrane-compatible descriptor structs over a bespoke PRIME map.
- Prefer Membrane's keepalive release protocol over an Emerge-specific
  `release_headless_frame(renderer, id)` API for V1.
- Emerge's keepalive resource must release GL/EGL resources on the headless
  render thread, not directly from the BEAM/NIF thread.
- If the receiver is not `membrane_video_linux`, it must either follow the same
  `{:keepalive, keepalive}` protocol or drop/GC the keepalive resource; explicit
  release helper APIs can be added later for non-Membrane consumers.

## Public API shape

Keep existing headless startup options and add nested PRIME controls:

```elixir
headless: [
  target: pid(),
  mode: :binary | :prime,
  pixel_format: :rgba8888, # ignored for :prime
  target_fps: pos_integer() | nil,
  frame_message: :emerge_skia_frame,
  prime: [
    max_in_flight: 2,
    on_backpressure: :drop_new,
    owner: :auto
  ]
]
```

V1 only supports:

- Linux
- `mode: :prime`
- selected renderer `:gl`
- EGL dma-buf export via `EGL_MESA_image_dma_buf_export`
- Membrane-compatible `%Membrane.PrimeDesc{}` output
- keepalive-based release

`headless.prime.owner`:

- `:auto` should create/use an Emerge-owned release process that consumes
  `{:keepalive, keepalive}` messages and calls the native keepalive release NIF.
- A pid may be supplied for advanced integrations that want to own keepalive
  release themselves.

## Frame message shape

Continue current headless delivery style: `{message_tag, frame}` where `frame` is
a key/value list with string keys.

PRIME frame keys:

```elixir
%{
  "mode" => "prime",
  "sequence" => non_neg_integer(),
  "width" => pos_integer(),
  "height" => pos_integer(),
  "prime_desc" => %Membrane.PrimeDesc{
    width: pos_integer(),
    height: pos_integer(),
    format: non_neg_integer(),
    objects: [%Membrane.PrimeObject{fd: non_neg_integer(), modifier: non_neg_integer() | nil}],
    planes: [%Membrane.PrimePlane{obj_idx: non_neg_integer(), pitch: pos_integer(), offset: non_neg_integer()}],
    keepalive: term(),
    owner_pid: pid(),
    trace_token: nil
  },
  "timestamp_native" => integer()
}
```

The descriptor is directly usable as `Membrane.Buffer` metadata under
`:drm_prime` for `membrane_video_linux` sinks.

FD ownership:

- FDs in `%Membrane.PrimeObject{}` are duplicates owned by the receiver once
  passed into another NIF.
- Native Emerge keeps its own exported fds/resources alive through the keepalive
  resource.
- Releasing keepalive closes native-owned fds/EGLImage/GL slot on the render
  thread.

## Backpressure

Default `max_in_flight: 2`.

V1 policy: `on_backpressure: :drop_new`.

- If no export slot is free, render loop should skip PRIME delivery for that
  frame, increment/drop-log a counter, and keep the renderer alive.
- Do not block normal NIF schedulers waiting for release.
- Do not destroy/reuse unreleased PRIME backing storage.
- The Membrane DRM sink already has latest-pending replacement behavior; Emerge
  should still avoid unbounded producer-side in-flight resources.

Future policy options may include `:block_render_thread` or `:drop_old`, but do
not implement them in V1.

## Rendering/export design

The current headless GL pbuffer/default framebuffer is good for binary readback,
but not a reliable dma-buf export target. Add an exportable GL render target for
PRIME mode:

1. Create offscreen EGL/GL context as today.
2. Require EGL export support:
   - `eglCreateImageKHR` or `eglCreateImage`
   - `eglDestroyImageKHR` or `eglDestroyImage`
   - `eglExportDMABUFImageQueryMESA`
   - `eglExportDMABUFImageMESA`
3. Allocate a pool of GL textures and FBOs sized to the viewport.
4. For each render:
   - acquire a free slot
   - bind/check its FBO
   - wrap FBO with `GlFrameSurface`
   - render via `SceneRenderer`
   - flush/submit Skia/GL
   - create EGLImage from `EGL_GL_TEXTURE_2D_KHR`
   - query fourcc/plane count/modifier
   - export fd(s), stride(s), offset(s)
   - build `%Membrane.PrimeDesc{}` using duplicated fds and an Emerge keepalive
     resource
   - send the headless frame message
   - retain slot/EGLImage/native-owned fds until keepalive release
5. On keepalive release:
   - enqueue release to the headless render thread
   - make the EGL context current
   - destroy EGLImage
   - close native-owned exported fds
   - return slot to pool
6. On renderer stop:
   - release all unreleased frames and slots after making the EGL context current

Assume implicit dma-buf synchronization for V1. Add explicit fences only in a
later measured slice.

## Capability and selection matrix

Headless support after this work:

| backend_renderer | mode | behavior |
| --- | --- | --- |
| `:auto` | `:binary` | try GL binary, fallback raster binary |
| `:raster` | `:binary` | raster binary |
| `:gl` | `:binary` | GL binary; fail if GL unavailable |
| `:auto` | `:prime` | try GL PRIME; fail if unavailable |
| `:gl` | `:prime` | GL PRIME; fail if unavailable |
| `:raster` | `:prime` | reject: raster PRIME unsupported |
| `:metal` / `:vulkan` | any | reject as today |

`renderer_info/1` should report output capabilities, preferably extending the
current capability map with a headless section:

```elixir
capabilities: %{
  gpu: true,
  screenshot: true,
  prime_video: false,
  headless: %{
    modes: [:binary, :prime],
    prime: true,
    max_in_flight: 2
  }
}
```

If changing the public shape is too broad, keep `renderer_info/1` unchanged for
V1 and expose PRIME support through startup success/failure. Prefer the explicit
capability map if practical.

## Implementation phases

### Phase 1: Headless option and selection model

- Allow `headless: [mode: :prime]` through Elixir normalization.
- Add normalized `headless.prime.max_in_flight`,
  `headless.prime.on_backpressure`, and `headless.prime.owner`.
- Replace stringly mode checks in native headless startup with an enum.
- Change headless `:auto` selection:
  - binary: GL first, fallback raster
  - prime: GL only
- Keep explicit `:gl` no-fallback behavior.
- Add tests for normalized options and selection errors.

### Phase 2: Keepalive release infrastructure

- Add an Emerge headless PRIME keepalive resource.
- Add a native keepalive release NIF analogous to
  `membrane_video_linux` decoder `keepalive_release/1`.
- Add an Emerge-owned release process for `headless.prime.owner: :auto` that
  receives `{:keepalive, keepalive}` and calls the release NIF.
- The keepalive release NIF must enqueue cleanup onto the render thread; it must
  not destroy GL/EGL resources directly on the NIF scheduler.
- Add tests for release idempotence and renderer-stop cleanup.

### Phase 3: Exportable GL target pool

- Add GL texture/FBO slot pool under `backend/headless/`.
- Split current pbuffer binary renderer from exportable-texture PRIME renderer.
- Ensure all GL resource creation/destruction happens on the headless render
  thread with the context current.
- Add Rust unit tests for pool bookkeeping where possible without real EGL.

### Phase 4: EGL dma-buf export and Membrane descriptor encoding

- Load and validate `EGL_MESA_image_dma_buf_export` function pointers.
- Create EGLImage from rendered GL texture.
- Query fourcc/planes/modifiers.
- Export fd/stride/offset metadata.
- Build `%Membrane.PrimeDesc{}` / `%Membrane.PrimeObject{}` /
  `%Membrane.PrimePlane{}` with duplicated fds and a keepalive resource.
- Match membrane field names exactly: `format`, `objects`, `planes`, `obj_idx`,
  `pitch`, `offset`, `owner_pid`, `trace_token`.
- Prefer a single object/plane if EGL reports one; support multiple planes if
  EGL reports them.
- Avoid mixed modifiers where possible; if EGL returns mixed modifiers, return a
  clear unsupported error because `membrane_video_linux` DRM sink rejects them.

### Phase 5: Render loop integration and backpressure

- Deliver PRIME frames from `RenderMsg::Scene` when mode is `:prime`.
- Publish latest-frame screenshots only for binary/readback frames unless a cheap
  readback is explicitly requested; do not force readback for PRIME V1.
- Enforce `max_in_flight` and `:drop_new`.
- Record stats/log counters for dropped PRIME frames and in-flight count.

### Phase 6: Tests and docs

- Unit tests:
  - option normalization
  - auto GL fallback for binary
  - no raster fallback for PRIME
  - keepalive release behavior
  - descriptor payload shape from mocked export metadata
  - Membrane struct field compatibility
- Hardware-gated integration tests, e.g. `EMERGE_SKIA_HEADLESS_PRIME_TEST=1`:
  - start headless auto PRIME
  - receive `%Membrane.PrimeDesc{}` descriptor
  - assert fd(s), fourcc, plane metadata
  - feed descriptor into `membrane_video_linux` DRM sink NIF/test helper if
    available, or at least import it in a small EGL/DRM smoke test
  - release keepalive
  - verify renderer can deliver subsequent frames
- Docs:
  - `lib/emerge_skia.ex` headless docs
  - `plans/active-backend-renderer-unification.md` Phase 10 status

## Open questions to settle before implementation

- Should Emerge define minimal `Membrane.Prime*` modules for users that do not
  depend on `membrane_video_linux`, or should it only emit compatible struct
  terms from Rust?
- Should `headless.prime.owner: :auto` be implemented in `EmergeSkia.start/1` or
  lower in the native transport layer?
- Should V1 include optional readback for screenshot APIs in PRIME mode, or
  should screenshots return `{:error, :not_available}` until a binary frame is
  rendered?
- Should `trace_token` stay `nil` in V1, or should Emerge add its own compatible
  instrumentation token later?

## Validation before completion

- `cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check`
- `mix format --check-formatted`
- `cargo test --manifest-path native/emerge_skia/Cargo.toml --lib`
- `mix test`
- `EMERGE_SKIA_HEADLESS_PRIME_TEST=1 mix test ...` on hardware/CI with EGL
  dma-buf export support
- `git diff --check`
