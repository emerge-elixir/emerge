# Active Plan: `video_interop` Library and Migration

Status: generic ownership hardening, reusable Membrane sink, Emerge integration,
and explicit acquire synchronization implemented; downstream consumer migration
and hardware validation remain pending.

The authoritative application-facing ownership, direct Emerge connection, and
synchronous shutdown design is
`../video_interop/plans/library-owned-video-lifecycle.md`.

## Decision

`emerge-elixir/video_interop` is one project containing:

- one Hex package: `video_interop`;
- one publishable Rust crate: `video-interop`;
- optional Rust crate features for API-specific adapters.

Do not split core, EGL, Vulkan, or Rustler integration into separate crates or
packages. The single crate keeps modules and dependencies isolated with Cargo
features:

```text
default = ["rustler"]
rustler = optional Rustler schema integration
egl     = future optional EGL native-fence adapter
vulkan  = future optional Vulkan sync-file adapter
```

Metal and Direct3D remain later features of the same crate.

## Repository

The project now exists at `/workspace/video_interop`:

```text
video_interop/
├── Cargo.toml                         # test workspace only
├── mix.exs                            # Hex package :video_interop
├── lib/video_interop/                 # Elixir contract
├── rust/video-interop/                # only publishable Rust crate
└── test/native/schema_test/           # test-only NIF fixture
```

The workspace fixture is not a second library. It exists only to compile and
exercise the exact Elixir/Rustler schema.

## Implemented foundation

### Elixir

The framework-neutral package exposes:

```elixir
%VideoInterop.Frame{
  coded_width: width,
  coded_height: height,
  visible_rect: %VideoInterop.Rect{},
  storage: %VideoInterop.DMABuf.Descriptor{},
  acquire_sync: :implicit | %VideoInterop.SyncFile{},
  lease: %VideoInterop.Lease{}
}
```

It also provides:

- `VideoInterop.Format` with storage-specific `%VideoInterop.DMABuf.Format{}`;
- AVDRM object/layer/plane descriptors;
- DRM fourcc and modifier helpers;
- structural frame/format/descriptor validation;
- generic `VideoInterop.Lease` and isolated `VideoInterop.LeaseOwner`;
- `:video_interop_*` issue, retain, release, failure, and drain messages.

The package has no Membrane dependency and loads no NIF.

### Rust

The single `video-interop` crate provides:

- `FrameDescriptor`, `Storage`, `AcquireSync`, and owned counterparts;
- DMA-BUF descriptor validation and close-on-exec fd duplication;
- transactional cleanup when a later object duplication fails;
- `OwnedFd`-based RAII for objects and acquire sync files;
- optional exact Rustler encoding/decoding under the default `rustler` feature;
- explicit `PreparedVideoFrame -> ClaimedVideoFrame` lease transfer;
- automatic claimed-lease retirement through a dedicated native release worker;
- a core-only `default-features = false` build with no Rustler dependency.

Ownership states are:

```text
Elixir frame / caller owns release
        │ validate + duplicate_cloexec
        ▼
PreparedVideoFrame / caller still owns release
        │ native subsystem accepts + claim()
        ▼
ClaimedVideoFrame / native owns release
        │ explicit retire or Drop
        ▼
{:video_interop_release, token, holder}
```

Dropping a prepared frame closes duplicate fds without sending release. Dropping
a claimed frame or claimed lease queues release from a native thread, so
`OwnedEnv::send_and_clear` never runs on a BEAM scheduler thread.

## Scope

The foundation supports process-local Linux DMA-BUF and sync-file acquire
fences. It does not allocate, import, render, present, or move handles between
OS processes.

The contract separates:

- storage from frame geometry;
- acquire synchronization from future release synchronization;
- owned native handles from external producer leases;
- transport-independent frames from Membrane buffers and timestamps.

Future storage variants can extend `storage` without renaming a DMA-BUF-specific
frame field.

## Next feature: `egl`

Add EGL synchronization as an optional feature/module in the same
`video-interop` crate.

V0.1 EGL scope:

- caller-provided EGL API/loader and display;
- caller-owned current context and render thread;
- `EGL_ANDROID_native_fence_sync` capability probing;
- producer create -> caller client flush -> duplicate sync-file -> destroy;
- consumer sync-file import with exact fd ownership transfer;
- server-side wait when available and bounded client-wait fallback;
- explicit timeout, unsupported, and destroy-failure results.

The feature must not create an EGL display/context, GBM device, GL surface,
thread, Skia context, or render loop. Emerge remains responsible for `glFinish`
fallback, renderer poisoning, slot pools, and publication policy.

Every EGL call needs an ownership table covering success and failure. Exported
fds are wrapped immediately in `OwnedFd` and verified close-on-exec. An EGL sync
that fails destruction remains represented for render-thread retry instead of
being lost inside an error.

## Following feature: `vulkan`

Add Vulkan synchronization as another optional module in the same crate after
the EGL path is stable.

Initial Vulkan scope:

- caller-owned `ash` instance/device/queue integration points;
- `VK_KHR_external_semaphore_fd` capability checks;
- binary semaphore `SYNC_FD` import/export;
- one-shot export and temporary-import state;
- exact success/failure fd ownership;
- rejection of timeline-semaphore and `OPAQUE_FD` misuse.

Do not create instances, devices, queues, threads, or rendering runtimes. Defer
DMA-BUF external-memory image import and DRM modifier negotiation.

## Membrane adapter

Use the standalone `/workspace/membrane_video_interop` adapter project and keep
`../colibri/membrane_dmabuf` unchanged as the old-contract recovery point until
migration completes. The new adapter depends on Hex `video_interop` and retains
only Membrane integration:

- `%Membrane.Buffer{}` insertion/extraction for `%VideoInterop.Frame{}`;
- canonical metadata key `:video_interop`;
- direct `%VideoInterop.Format{}` use as the Membrane stream format;
- a reusable ownership-safe consumer sink over `VideoInterop.Consumer`;
- Membrane buffer transport validation and cleanup routing.

The detailed coordinated cutover is tracked in
`/workspace/video_interop/plans/membrane-video-interop-migration.md`.

It must contain no generic descriptor, synchronization, lease implementation,
fd duplication, EGL/Vulkan code, or Rust crate. Native NIF consumers depend
directly on the single `video-interop` crate.

Use an atomic cutover rather than a fake metadata-only compatibility layer. Old
frames use `descriptor`/`synchronization` fields and `:membrane_dmabuf_*` lease
messages; merely accepting `:dmabuf` under the new package would release to the
wrong protocol. All current consumers are unpublished WIP and can migrate
together.

## Consumer migration order

1. Harden `../video_interop` issue/drain/consumer-session ownership.
2. Add the generic consumer sink in `../membrane_video_interop`, validated with
   fake consumers before any producer cutover.
3. `../emerge-headless`
   - Replace the Hex dependency with `video_interop`.
   - Replace `membrane-dmabuf` with `video-interop` in Cargo.
   - Emit/consume `%VideoInterop.Frame{storage:, acquire_sync:}` directly.
   - Keep headless PRIME output independent of Membrane.
4. `../emerge_demo`
   - Use Emerge's direct headless-output-to-`VideoTarget` connection.
   - Delete application-owned descriptor conversion, keepalive handling,
     `PrimeBridge`, and `PrimeRenderer`.
5. Atomically migrate `../colibri/membrane_video_surfaces` and
   `../colibri/membrane_libcamera` to canonical adapter transport and the
   `video-interop` crate.
6. Migrate `../emerge_video_demo` and the Colibri camera application to
   `%Membrane.VideoInterop.Sink{}` without application-owned lifecycle code.

Keep path dependencies only on integration branches. Publish no downstream
artifact until dependencies resolve without sibling paths.

## Validation

Implemented foundation gates:

```bash
mix format --check-formatted
mix test
mix hex.build
mix docs

cargo fmt --all -- --check
cargo test --workspace
cargo test -p video-interop --no-default-features
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p video-interop --no-default-features --all-targets -- -D warnings
cargo package -p video-interop --allow-dirty
```

Feature-specific gates:

- injected EGL function-table failure tests;
- exact create/flush/dup/destroy ordering;
- imported-fd success/failure ownership tests;
- Vulkan binary semaphore state tests;
- hardware EGL -> Vulkan and Vulkan -> EGL sync-file tests;
- linkage proof that the core-only crate does not link EGL/Vulkan/Rustler;
- sustained Emerge fd growth, lease retirement, and explicit-sync validation.

## Publication order

1. crates.io `video-interop` with core + optional Rustler support;
2. Hex `video_interop`;
3. Hex `membrane_video_interop`;
4. migrated downstream packages;
5. updated `video-interop` releases adding optional `egl`, then `vulkan`,
   features when their gates pass.

Do not block the lightweight foundation on unfinished graphics adapters.

## Completion criteria

- One Hex package and one publishable Rust crate own all generic interop code.
- The crate builds with and without Rustler.
- Optional graphics dependencies activate only through crate features.
- Every fd has an explicit borrowed, owned, consumed, or closed transition.
- Emerge, video surfaces, and libcamera share one frame/sync/lease contract.
- `membrane_video_interop` is only a Membrane transport adapter.
- Supported EGL paths publish acquire fences without producer `glFinish()`;
  unsupported paths remain correct through Emerge's conservative fallback.
