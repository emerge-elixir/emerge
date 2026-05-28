# Active Plan: Headless GL Renderer

Status: planned, not implemented.

## Goal

Add explicit Linux headless GL support for retained headless sessions:

```elixir
EmergeSkia.start(
  otp_app: :my_app,
  backend: :headless,
  backend_renderer: :gl,
  width: 800,
  height: 480,
  headless: [target: self(), mode: :binary, pixel_format: :rgba8888]
)
```

The first GL slice should render with Skia GPU to an offscreen EGL surface and
read back pixels for the existing binary frame delivery path.

## Current state

- `backend: :headless` works with `backend_renderer: :auto | :raster`.
- `:auto` selects raster for headless binary output.
- `backend_renderer: :gl` is rejected with
  `backend_renderer :gl is not implemented yet for backend :headless`.
- `headless: [mode: :binary]` is the only implemented output mode.
- `headless: [mode: :prime]` remains explicitly not implemented.

## API decisions

- Keep the public API shape unchanged for the first GL slice.
- `backend_renderer: :gl` is explicit and must fail if offscreen EGL/GL cannot
  start. Do not silently fall back to raster.
- Keep `backend_renderer: :auto` selecting raster for `mode: :binary`; GPU
  readback is not a better default.
- Keep the existing `headless` options and frame message shape.
- `renderer_info/1` for headless GL should report:
  - `backend: :headless`
  - `backend_renderer: %{requested: :gl, selected: :gl}`
  - `capabilities.gpu: true`
  - `capabilities.renderer_cache: true` when enabled
  - `capabilities.screenshot: true`
  - `capabilities.prime_video: false` in this slice
- Do not overload `drm_card` for headless GL device selection. If explicit GPU
  selection is needed later, add a nested option such as
  `headless: [gl: [device: ...]]` in a separate slice.

## Non-goals

- PRIME/dma-buf export.
- Headless `mode: :prime`.
- macOS Metal headless.
- Vulkan.
- Synthetic headless input.
- PBO/asynchronous readback optimization. The first slice may use synchronous
  `read_pixels`; optimize only after measuring.

## Implementation phases

### Phase 1: Refactor headless runtime code out of `lib.rs`

- Move headless startup/render-loop helpers from `native/emerge_skia/src/lib.rs`
  into `native/emerge_skia/src/backend/headless/`.
- Keep the exported NIF boundary in `lib.rs` thin.
- Preserve current raster behavior and tests before adding GL.
- Keep frame conversion and `OwnedEnv` frame sending in the headless module, or
  move conversion into a small shared service if tests benefit.

Validation:

- `cargo test --manifest-path native/emerge_skia/Cargo.toml --lib`
- `mix test test/emerge_skia_test.exs test/emerge_skia/options_test.exs`

### Phase 2: Split headless render loop from renderer implementation

Introduce a small internal abstraction, for example:

```rust
trait HeadlessFrameRenderer {
    fn selected_renderer(&self) -> RendererBackendKind;
    fn render(&mut self, state: &RenderState) -> Result<HeadlessRgbaFrame, String>;
}
```

Implement:

- `RasterHeadlessRenderer` using existing `RasterBackend::render_with_timings`.
- `HeadlessRgbaFrame` carrying `{width, height, rgba, timings}`.

The render loop should stay responsible for:

- consuming `RenderMsg::Scene | RenderMsg::Stop`
- stats collection
- `LatestFrameStore::publish_rgba`
- pixel-format conversion
- BEAM frame delivery via `OwnedEnv`
- animation pulse scheduling

### Phase 3: Add offscreen EGL/GL renderer

Add a Linux-only module, for example:

```text
native/emerge_skia/src/backend/headless/gl.rs
```

Initial preferred path:

- Load EGL using the same `libEGL.so.1`/`eglGetProcAddress` pattern as DRM.
- Create a headless EGL display using the most portable available path:
  1. EGL device platform if available.
  2. Surfaceless platform if available.
  3. Default display/pbuffer fallback if supported.
- Bind OpenGL ES.
- Choose an RGBA8 pbuffer-capable config.
- Create a pbuffer surface sized to the headless viewport.
- Make the context current on the headless render thread.
- Load GL symbols.
- Create Skia GL interface/direct context.
- Wrap the current framebuffer with `GlFrameSurface`.
- Render with `SceneRenderer::with_cache_config`.
- Capture RGBA with `GlFrameSurface::capture_rgba_pixels()`.

If direct EGL setup becomes too platform-specific, evaluate glutin pbuffer
creation as a fallback, but avoid coupling headless GL to the Wayland backend.

### Phase 4: Wire selection and capability matrix

- Update `ensure_headless_backend_renderer_supported/1` to allow `:gl` only when
  the native build includes the Linux offscreen GL code path.
- Keep `:metal` and `:vulkan` rejected.
- In `start_headless_renderer_with_config`, choose:
  - raster for `:auto | :raster`
  - GL for `:gl`
- Store `selected_renderer: :gl` in `RendererRuntimeInfo` for explicit GL.
- Ensure renderer-cache status follows the selected renderer.

Explicit unsupported cases should fail early and clearly:

- GL code not compiled: `backend_renderer :gl is not available for backend :headless in this build`
- EGL init failed: return the EGL startup error; no raster fallback.

### Phase 5: Tests

Rust unit tests:

- Capability matrix allows headless `:gl` when compiled.
- Capability matrix rejects headless `:metal` and `:vulkan`.
- Headless renderer selection reports GL for explicit GL.
- Pixel conversion remains renderer-independent.

Elixir tests:

- `EmergeSkia.start(... backend: :headless, backend_renderer: :gl, ...)` either:
  - starts and delivers a binary frame when EGL is available in CI, or
  - returns a tagged unavailable error when EGL is not available.
- `renderer_info/1` reports selected `:gl` on successful startup.
- `render_to_pixels/2` and `render_to_png/2` work after a GL headless frame has
  been rendered, using the latest-frame path.
- Explicit GL does not silently fall back to raster.

Use environment-gated tests for real EGL hardware if CI lacks a headless GL
provider, for example `EMERGE_SKIA_HEADLESS_GL_TEST=1`.

### Phase 6: Documentation

Update:

- `lib/emerge_skia.ex` start options docs.
- `guides/tutorials/set_up_viewport.md` if headless usage is documented there.
- `plans/active-backend-renderer-unification.md` Phase 10 status/link.

Document:

- Headless GL is explicit only.
- Binary output still copies/readbacks pixels.
- `:auto` remains raster for binary mode.
- PRIME/dma-buf is still future work.

## NIF safety notes

- Keep EGL context ownership on the render thread; never make it current from a
  normal NIF call.
- Send BEAM messages from native threads with `OwnedEnv`, as current headless
  raster does.
- Do not encode `ResourceArc<RendererResource>` into frame messages from the
  render thread in this slice.
- Do not block normal NIF schedulers waiting for frames. Startup may return
  errors synchronously; frame delivery stays async.
- Convert GL/EGL failures into `Result<_, String>`; avoid `expect`/panic in
  startup and render paths.

## Validation before completion

- `cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check`
- `mix format --check-formatted`
- `cargo test --manifest-path native/emerge_skia/Cargo.toml --lib`
- `mix test`
- `git diff --check`

If hardware-gated GL tests are added, run them on at least one machine/container
with working EGL headless support before marking this plan complete.
