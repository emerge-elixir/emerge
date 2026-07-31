# Active Plan: Linux Headless PRIME Output

Status: implementation complete; hardware PRIME validation pending.

## Goals

Provide retained Linux headless DMA-BUF output with independent backend and
rendering API selection:

```elixir
EmergeSkia.start(
  otp_app: :my_app,
  backend: :headless,
  rendering_api: :auto,
  width: 800,
  height: 480,
  headless: [
    target: self(),
    mode: :prime,
    prime: [max_in_flight: 2, on_backpressure: :drop_new]
  ]
)
```

`backend_renderer` and `:gl` remain deprecated aliases for `rendering_api` and
`:opengl`.

## Selection behavior

| `rendering_api` | mode | behavior |
| --- | --- | --- |
| `:auto` | `:binary` | try Linux offscreen OpenGL, then raster |
| `:opengl` | `:binary` | OpenGL readback; fail if unavailable |
| `:raster` | `:binary` | raster binary output |
| `:auto` | `:prime` | OpenGL/GBM only; fail if unavailable |
| `:opengl` | `:prime` | OpenGL/GBM only; fail if unavailable |
| `:raster` | `:prime` | reject; raster does not export DMA-BUF |
| `:metal` / `:vulkan` | any | reject for current Linux headless support |

Explicit rendering API choices never fall back. PRIME startup probes GBM
allocation, EGLImage import, texture binding, and FBO completeness before
reporting success.

## Canonical DMA-BUF contract

`../colibri/membrane_dmabuf` is the canonical descriptor, validation, and lease
contract. PRIME frames are delivered as `{message_tag, frame}`, where `frame`
is a key/value list with string keys:

```elixir
%{
  "mode" => "prime",
  "sequence" => non_neg_integer(),
  "width" => pos_integer(),
  "height" => pos_integer(),
  "dmabuf" => %Membrane.DMABuf.VideoFrame{
    coded_width: pos_integer(),
    coded_height: pos_integer(),
    visible_rect: %Membrane.DMABuf.Rect{},
    descriptor: %Membrane.DMABuf.Descriptor{
      version: 1,
      objects: [%Membrane.DMABuf.Object{}],
      layers: [%Membrane.DMABuf.Layer{planes: [%Membrane.DMABuf.Plane{}]}]
    },
    synchronization: :implicit,
    lease: %Membrane.DMABuf.Lease{}
  },
  "timestamp_native" => integer()
}
```

DMA-BUF fds are borrowed process-local integers. They remain valid until all
holders of the managed lease have released. Consumers use
`Membrane.DMABuf.Lease.retain/2` before fan-out and release each frame or lease
with `Membrane.DMABuf.release/1`.

An isolated `Membrane.DMABuf.LeaseOwner`:

- registers each native backend token before frame delivery;
- validates and accounts for root and retained holders;
- invokes native retirement exactly once after the last holder releases;
- keeps renderer resources alive while shutdown drains outstanding leases;
- dispatches GL/EGL/GBM cleanup back to the headless render thread;
- reaps signaled consumer-side import fences in the Wayland frame callback so
  cleanup does not require redrawing an otherwise unchanged scene;
- derives active video targets from the current render scene and immediately
  releases frames submitted to targets whose video elements are not rendering.

`EmergeSkia.stop/1` marks a PRIME session stopped immediately and starts lease
draining. The internal session retains the native renderer until the lease owner
reports that every outstanding frame has retired, then stops the native runtime.

## FD and resource ownership

The current export format is a single-plane, linear GBM `ABGR8888` buffer:

- the GBM BO owns the backing storage;
- export requests `RENDERING | LINEAR`, not `SCANOUT`, so consumers never depend
  on implicit driver-specific tiling metadata;
- Skia renders the export target with top-left video orientation;
- Emerge retains the BO, EGLImage, GL texture, FBO, and exported `OwnedFd` while
  the lease is active, then returns the complete slot to a bounded pool for reuse;
- PRIME frames share one persistent Skia direct context instead of rebuilding GPU
  state, shaders, and caches for every exported frame;
- descriptor object size comes from `fstat`, not a pitch/height estimate;
- explicit GBM modifiers, including linear modifier `0`, are preserved;
- startup retries EGL display and GBM device candidates until it finds a
  compatible export pair;
- multi-plane GBM allocations are rejected until EGL import and descriptor
  construction support every plane correctly;
- native cleanup runs with the EGL context current on the render thread.

Explicit sync-file acquire synchronization is implemented for supported EGL
drivers as described in `active-headless-prime-explicit-sync.md`. It keeps
`glFinish()` plus `:implicit` as the unsupported-driver or runtime-failure fallback;
hardware validation remains pending.

## Backpressure

Default `max_in_flight` is `2`. V1 supports only `on_backpressure: :drop_new`.

- An unreleased slot is never destroyed or reused.
- At capacity, new PRIME delivery is skipped without blocking a NIF scheduler.
- Retained animations remember a capacity drop and schedule a new pulse when a
  holder releases a slot.
- Static uploads dropped at capacity are not retried; a later tree update
  produces the next frame.
- PRIME mode reports screenshot capture as unsupported because it intentionally
  avoids GPU readback.

Future policies may include render-thread blocking or pending-frame replacement,
but neither is part of V1.

## Implementation status

### Configuration and architecture

- [x] Canonical `backend: :headless` plus `rendering_api` selection.
- [x] Deprecated `backend_renderer` and `:gl` aliases with warnings.
- [x] Nested `headless.prime.max_in_flight` and `on_backpressure` options.
- [x] Binary auto fallback and PRIME OpenGL-only selection.
- [x] Precise rejection of unsupported combinations.

### Export path

- [x] Offscreen EGL/OpenGL context and GBM device discovery.
- [x] Startup exportability probe.
- [x] Bounded, lease-safe GBM/EGLImage/FBO slot allocation and reuse.
- [x] Persistent Skia direct-context retargeting across export slots.
- [x] Canonical `Membrane.DMABuf.Descriptor` encoding.
- [x] Actual object-size reporting and single-plane enforcement.
- [x] Render-thread retirement of EGL/GL/GBM resources.

### Lifecycle

- [x] Canonical managed leases through `Membrane.DMABuf.LeaseOwner`.
- [x] Fan-out holder accounting and idempotent release.
- [x] Backpressure capacity tied to unreleased native frames.
- [x] Require a live local delivery target and begin draining if it exits.
- [x] Asynchronous shutdown draining without invalidating outstanding fds.
- [x] Retained-animation resumption when capacity returns.

### Tests and validation

- [x] Option normalization and compatibility aliases.
- [x] Rendering API/backend compatibility matrix.
- [x] Rust unit suite and Elixir suite.
- [x] Hardware-gated descriptor, validation, retain/release, backpressure, and
  subsequent-frame flow.
- [ ] Run the hardware-gated test on a suitable EGL/GBM/DMA-BUF device:

```bash
EMERGE_SKIA_HEADLESS_PRIME_TEST=1 \
  mix test test/emerge_skia_test.exs
```

- [x] Wire the `emerge_demo` PRIME tab as an independent generic-map consumer of
  canonical headless DMA-BUF output.
- [x] Run the export/import path on hardware far enough to validate delivery,
  lease flow, and presentation; the first run exposed tiled pixel corruption.
- [x] Re-run after requiring linear export BOs and confirm tiled corruption is gone.
- [ ] Re-run after correcting exported frames to top-left video orientation.
- [x] Implement the explicit-fence slice in
  `active-headless-prime-explicit-sync.md` to remove the producer-side
  `glFinish()` stall while retaining a safe implicit fallback (hardware
  validation pending).
- [ ] Publish `membrane_dmabuf` 0.1.0 to Hex and `membrane-dmabuf` 0.1.0 to
  crates.io, then replace the temporary sibling-path development overrides.
  Emerge package verification remains blocked until those canonical packages
  exist.

A target that exits after receiving a frame still must have arranged
`Membrane.DMABuf.release/1` for every holder before exit. As required by the
canonical contract, process death alone cannot prove that external hardware has
finished with a borrowed DMA-BUF.

## Validation before completion

```bash
cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check
cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings
cargo test --manifest-path native/emerge_skia/Cargo.toml
mix format --check-formatted
mix test
./ci-tests.sh all
git diff --check
```
