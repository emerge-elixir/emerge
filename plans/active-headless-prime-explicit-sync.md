# Active Plan: Headless PRIME Explicit Synchronization

Status: implemented in the canonical direct `VideoInterop` path; hardware validation pending.

## Goal

Replace the per-frame headless PRIME `glFinish()` with an exported acquire
sync-file fence. The producer should publish the DMA-BUF as soon as its GPU work
and fence have been queued; consumers must wait on that fence before sampling.

Keep the existing conservative path as a compatibility fallback:

- supported driver: `%VideoInterop.SyncFile{}` plus no producer-side GPU wait;
- unsupported or failed explicit-sync path: `glFinish()` plus `:implicit`;
- never publish a frame whose completion cannot be proven by one of those paths.

This is acquire synchronization only. Existing consumer-side retirement fences
and managed leases still decide when an export slot can be reused.

Implement this after the foundation and optional EGL-feature milestones in
`active-video-interop-library.md`. Emerge should enable the single
`video-interop` crate's `egl` feature rather than maintaining a second EGL fence
abstraction.

## Architecture

### Producer sequence

For each headless PRIME frame:

1. Retarget the persistent Skia context to a lease-safe GBM slot.
2. Draw and submit the scene through Skia.
3. Create `EGL_SYNC_NATIVE_FENCE_ANDROID` with no imported fd.
4. Call `glFlush()` after fence insertion so the driver creates and submits the
   native fence after all preceding rendering commands.
5. Duplicate the fence with `eglDupNativeFenceFDANDROID`, immediately wrap the
   returned caller-owned fd in `OwnedFd`, and set `FD_CLOEXEC`. Failure to set
   close-on-exec closes the fd and enters the conservative fallback.
6. Destroy the temporary EGL sync object. A destroy failure is terminal for the
   renderer instance: close the duplicated fd, publish nothing, and tear down
   the context rather than leaking one sync per frame.
7. Store the `OwnedFd` in the in-flight slot, publish its borrowed integer as
   `%VideoInterop.SyncFile{acquire_fence_fd: fd}`, and retain it until final
   lease release.

The available-slot invariant is that it contains no live per-frame fence. Final
lease release closes the fence before returning the slot to `available`.
Shutdown closes fences together with their in-flight slots.

### Capability and fallback policy

At PRIME startup, query and probe:

- `EGL_ANDROID_native_fence_sync` and its `EGL_KHR_fence_sync` dependency;
- `eglCreateSyncKHR`, `eglDestroySyncKHR`, and
  `eglDupNativeFenceFDANDROID`;
- actual create, flush, duplicate, close, and destroy behavior on the selected
  EGL display/context.

Explicit sync is preferred but not a new required public option. Unsupported
hardware selects the existing `glFinish()`/`:implicit` mode and logs that choice
once.

If create, flush, duplicate, or close-on-exec setup fails at runtime:

- destroy any created EGL sync and close any caller-owned duplicated fd;
- run `glFinish()` and check GL/EGL errors before publishing that frame as
  `:implicit`;
- permanently downgrade that renderer instance to the fallback and log once;
- poison the renderer and publish nothing if sync destruction or the fallback
  cannot prove safe completion.

## Canonical output transport

Use the framework-neutral `video_interop` contract from
`active-video-interop-library.md`.

- Add an internal synchronization value to `HeadlessPrimeExport`.
- Encode the single `video-interop` crate's Rustler acquire-sync DTO as either
  an owned sync-file fence or implicit synchronization in the native relay
  message.
- `EmergeSkia.HeadlessPrimeSession` copies that value into
  `%VideoInterop.Frame{acquire_sync: ...}` instead of hard-coding
  `:implicit`, then removes the internal synchronization relay key alongside
  `"backend_token"` and `"descriptor"`.
- The fence fd is borrowed just like the DMA-BUF object fds. Every asynchronous
  native consumer must duplicate it before releasing its lease holder.
- Fan-out holders share the same fence and slot lifetime through the existing
  `LeaseOwner`; no release-fence transport is added to the v0.1 contract.

## Generic Emerge PRIME input

Emerge input remains producer-independent and must not depend on Membrane
structs.

Extend the generic descriptor map with optional `acquire_fence_fd`:

```elixir
%{
  width: width,
  height: height,
  format: fourcc,
  objects: objects,
  planes: planes,
  acquire_fence_fd: fd_or_nil,
  keepalive: keepalive,
  owner_pid: owner_pid
}
```

Preserve the decoder order as legacy `%Membrane.PrimeDesc{}`, new generic map,
then old generic map. The new map must be attempted before the old map so an
extra fence key cannot be silently ignored. Do not add an optional field directly
to the existing `NifMap`, because Rustler requires every map field to be present.

During `submit_prime`:

- duplicate a supplied borrowed fence with `F_DUPFD_CLOEXEC` along with the
  DMA-BUF fds;
- store the owned duplicate in `PrimeFrame`;
- close it automatically when an inactive, replaced, invalid, or failed frame is
  dropped;
- perform no fence wait in the NIF or on a BEAM scheduler.

Before the GL consumer samples the frame, use an explicit ownership/state
machine:

1. Discover native-fence create/destroy/client-wait support separately from
   server-wait support. The KHR server path requires `EGL_KHR_wait_sync`,
   `GL_OES_EGL_sync`, and `eglWaitSyncKHR`; EGL 1.5 core may provide the
   corresponding core path. Server-wait flags are always zero.
2. If EGL cannot import native fences at all, wait on the owned sync-file with a
   finite one-second `poll()` on the backend render thread. Continue only when
   signaled; timeout/error drops the new frame and preserves the current frame.
3. Otherwise transfer the fd with `IntoRawFd` before calling
   `eglCreateSyncKHR`. EGL owns the fd after the call, including the failure
   case; Rust must never reclaim or close it. Creation failure drops the frame
   and is never treated as implicit.
4. Prefer `eglWaitSyncKHR`/core `eglWaitSync` to queue a server dependency
   without blocking the render thread. Classify false/error separately.
5. If server wait is unavailable or fails, call `eglClientWaitSyncKHR` with
   flags zero and a concrete one-second timeout. Only
   `EGL_CONDITION_SATISFIED_KHR` permits import; timeout, `EGL_FALSE`, or EGL
   error drops the frame.
6. Destroy the EGL sync after a server dependency is queued or a client wait is
   satisfied. If destruction fails, retain the handle in a render-thread cleanup
   queue, request cleanup callbacks, and retry; EGL display teardown is the final
   owner. Never double-close its imported fd.

For the one-time NV12 CPU luma diagnostic, keep the EGL sync live and perform a
bounded client wait on that same sync before mapping. If a server wait is already
queued but the diagnostic client wait times out, skip the diagnostic and still
render safely through the queued GPU dependency. Destroy/retire the sync only
after this decision. A queued GPU wait alone does not synchronize CPU reads.

Existing post-sampling `glFenceSync` retirement remains unchanged: acquire fences
protect producer writes before sampling; retirement fences protect the producer
from slot reuse while the consumer GPU still samples.

## Demo validation

The demo uses `Emerge.connect_video_output/3`; it has no application-owned
`PrimeBridge` or descriptor conversion. The headless relay places either
`:implicit` or `%VideoInterop.SyncFile{}` directly in the canonical frame, and
the Emerge consumer session performs synchronous prepare/claim before the
render thread imports and waits on the owned duplicate.

## Diagnostics

Add log-only measurements and counters unless a public stats field is required.
Represent producer synchronization timings as an enum/optional samples so an
unused path does not record a zero-duration sample:

- `headless PRIME fence export`, recorded only after successful fence export;
- `headless PRIME GPU finish fallback`, recorded only when `glFinish()` runs;
- acquire fences received;
- consumer server waits queued;
- consumer client-wait fallbacks, timeouts, and errors.

Log the selected producer sync mode once. Keep stats schema 19 for log-only
metrics; bump it only if the public `stats/2` payload changes.

On supported hardware, a normal run should show one fence export per published
frame and zero GPU-finish fallback samples. Fence-export time replaces the old
~2.1 ms producer stall, although the GPU rendering work itself still occurs and
may be waited on by the consumer GPU.

## Implementation order

1. Add producer capability probing, synchronization mode, fence export, slot
   ownership, and conservative runtime downgrade in
   `native/emerge_skia/src/backend/headless/offscreen_gl.rs`.
2. Transport canonical synchronization through
   `native/emerge_skia/src/backend/headless/mod.rs` and
   `lib/emerge_skia/headless_prime_session.ex`.
3. Add backward-compatible generic acquire-fence decoding, ownership, and EGL
   waiting in `native/emerge_skia/src/video.rs`.
4. Exercise the direct demo connection with explicit and implicit frames.
5. Add diagnostics, documentation, and hardware-gated coverage.
6. Run independent lifecycle/FD-ownership review before hardware validation.

Steps 1–6 are implemented and independently reviewed. Hardware validation remains.

## Tests

### Unit and integration

- capability selection and startup-probe fallback;
- create/flush/duplicate/destroy error paths cannot publish an unsafe frame;
- in-flight slot owns its fence and available slots do not;
- final lease release and shutdown close fence fds exactly once;
- canonical output validates with both `SyncFile` and `:implicit`;
- old generic maps still decode; new maps duplicate the acquire fence;
- inactive/replaced/import-failed frames close native-owned duplicates and
  release keepalive; decode/validation errors close temporary duplicates but
  leave keepalive release to the caller because ownership has not transferred;
- same-id target recreation cannot carry a fence into a new incarnation;
- consumer server-wait success, bounded poll/client-wait fallback, timeout,
  false/error, sync-create failure, and deferred destroy cleanup;
- fd-transition tests prove transfer on every `eglCreateSyncKHR` call, no Rust
  close after transfer even on creation failure, and exactly-once closure for
  every pre-transfer or producer-owned fd;
- direct connection transport for explicit and implicit synchronization.

### Hardware acceptance

Run the hardware-gated headless PRIME test and the demo PRIME tab on the target
GPU. Require:

- no corruption, stale partial frame, orientation regression, or visual tearing;
- 150 exports/imports/releases over five seconds at 30 FPS;
- explicit `SyncFile` output when support is reported;
- zero producer `GPU finish fallback` samples on supported hardware;
- headless render time reduced by approximately the former 2.1 ms stall;
- bounded process fd count over a sustained release/reuse run;
- fence fd remains valid through retained fan-out leases and closes after final
  release;
- unsupported/forced-fallback mode remains correct with `:implicit`, using a
  private hardware-test capability override such as
  `EMERGE_SKIA_HEADLESS_PRIME_FORCE_IMPLICIT_SYNC=1`;
- tab hiding still drops/releases incoming frames without importing or waiting.

## Validation

```bash
cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features drm
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --all-targets -- -D warnings
mix format --check-formatted
mix test
(cd ../emerge_demo && mix format --check-formatted && mix test)
./ci-tests.sh all
git diff --check
```

Hardware:

```bash
mix test test/emerge_skia_test.exs --include headless_prime_hardware
```

## Main risks

- Incorrect EGL fd ownership can double-close, leak, or accidentally close a
  reused descriptor number.
- `glFlush()` must happen after native-fence insertion; it is not a substitute
  for `glFinish()` without exporting the resulting fence.
- Publishing before the slot owns the duplicated fence can expose an invalid fd.
- Waiting on the consumer CPU merely moves the stall; the intended fast path is
  a GPU server wait.
- A fence timeout must drop the new frame, never sample it or replace the current
  displayed frame.
- Cross-device sync-file behavior and driver extension claims require real
  hardware validation.
