# Direct video frame submission

Status: implementation and local full-suite, package, and documentation
validation complete across VideoInterop, Emerge, Membrane transport, and
`emerge_demo`; publication and hardware validation remain.

This plan replaces renderer-owned `VideoTarget` handles, consumer-session ceremony,
and direct `connect_video_output`/`disconnect_video_output` wiring with two public
operations:

```elixir
video([width(fill()), height(fill()), image_fit(:cover)], :camera)

:ok = Emerge.submit_video_frame(viewport, :camera, frame)
```

The target is a viewport-local atom. A submitted frame is retained only when that
target is present in the renderer's current scene. Otherwise Emerge consumes and
drops it immediately.

This plan also expands `VideoInterop.Frame` from DMA-BUF-only borrowed storage to
a storage-neutral frame that can own a BEAM binary. Binary storage does not use a
lease. DMA-BUF and other borrowed/reusable storage continue to require one.

## Goals

- Make displaying video require only a target atom and a frame submission.
- Let producers submit `%VideoInterop.Frame{}` directly to a running viewport.
- Drop frames cheaply when their target is not currently visible.
- Keep only the latest submitted frame for each visible target.
- Support owned binary pixels and leased DMA-BUFs through the same frame type.
- Emit `%VideoInterop.Frame{}` from both binary and PRIME headless renderers.
- Let ordinary Membrane raw-video pipelines convert to and from VideoInterop
  without adding Membrane dependencies to Emerge or `video_interop`.
- Route headless-Emerge-to-target-Emerge delivery through
  `membrane_video_interop`, not application mailbox forwarding.
- Apply the completed API and pipeline migration to `../emerge_demo`.
- Preserve exact DMA-BUF ownership, synchronization, and GPU-safe retirement.
- Remove public connection, target-registration, and consumer-session APIs from
  Emerge.

## Non-goals

- Emerge does not decode compressed video.
- Emerge does not schedule playback from PTS/DTS or provide A/V synchronization.
- Emerge does not own a video queue. Submission is latest-frame-wins.
- Emerge does not perform arbitrary pixel-format conversion. It accepts a
  documented set of formats; Membrane pipelines insert a converter when needed.
- `video_interop` does not depend on Membrane.
- `membrane_video_interop` does not depend on Emerge; its sink invokes a
  configured submission callback/MFA.
- There is no Emerge-owned automatic connection or connect/disconnect API.
  Emerge-to-Emerge transport is an explicit Membrane pipeline.

## Public API

### UI target

Change `Emerge.UI.video/2` to accept an atom:

```elixir
@spec video(attrs(), atom()) :: Emerge.UI.t()

def video(attrs, target) when is_atom(target)
```

Example:

```elixir
video(
  [width(fill()), height(px(360)), image_fit(:contain)],
  :preview
)
```

Rules:

- The atom is scoped to one viewport renderer.
- Reusing an atom in multiple video elements draws the same frame in each element.
- The atom is encoded into EMRG using `Atom.to_string/1`; native code never creates
  atoms from external data.
- Strings and `%EmergeSkia.VideoTarget{}` are rejected.
- A target atom carries no renderer reference, dimensions, mode, or lifetime.
- A video element has no intrinsic size before or after submission. Applications
  size it with normal layout attributes. The frame dimensions are used only for
  image fitting inside the laid-out rectangle.
- A visible target with no submitted frame draws nothing.

Remove the current implicit `image_size` derived from `VideoTarget.width/height`.
Keep `image_fit`, clipping, transforms, opacity, nearby layers, and repeated target
usage working as they do today.

### Frame submission

Add:

```elixir
@spec Emerge.submit_video_frame(
        GenServer.server(),
        atom(),
        VideoInterop.Frame.t()
      ) :: :ok | {:error, term()}

def submit_video_frame(viewport, target, frame)
```

Semantics:

- `viewport` identifies the destination viewport.
- `target` is the atom passed to `Emerge.UI.video/2`.
- The call consumes the frame on every normal return. The caller must not release
  or resubmit the same leased holder afterward.
- `:ok` means Emerge handled the frame. It does not promise that the frame reached
  a display.
- If the target is absent from the renderer's current scene, Emerge releases leased
  storage, drops owned binary storage, returns `:ok`, and does not wake or redraw
  the renderer.
- If the target is visible, submission replaces any older pending frame and asks
  the renderer to redraw.
- The last accepted frame remains visible until replaced or until the target leaves
  the scene.
- Removing a target retires its pending/current DMA-BUF frame and drops its binary
  frame. Re-adding the target starts empty and requires a new submission.
- Invalid frames, unsupported storage/layouts, unavailable backends, and stopped
  viewports return `{:error, reason}` after Emerge has released any caller-owned
  lease.
- Frame dimensions and formats may change between submissions. The renderer safely
  rebuilds target resources rather than requiring an explicit stream reconnect.

Do not route high-rate submission through the viewport GenServer mailbox. That
would deadlock when called from the viewport's own callback and would serialize UI
callbacks behind video traffic. Register a private, concurrent renderer submission
endpoint when the viewport mounts:

1. Resolve a viewport name to its PID.
2. Look up `{renderer_module, renderer_handle, viewport_generation}` in a private
   Emerge-owned registry/ETS table.
3. Invoke the renderer's `submit_video_frame/3` callback directly from the caller.
4. Unregister the endpoint before renderer shutdown.
5. Treat races with shutdown as caller-owned rejection and release through the
   same receipt-normalization path.

The native renderer resource remains authoritative for open/closed admission, so
an endpoint lookup racing shutdown cannot enqueue into a closed renderer.

The renderer callback returns ownership-tagged receipts, not plain errors:

```elixir
{:ok, :transferred | :released}
{:error, {:caller_owned | :transferred, reason}}
```

`Emerge.submit_video_frame/3` normalizes those receipts using the same rules as
`VideoInterop.consume/2`:

- release on `:caller_owned`;
- never release on `:transferred`;
- return `:ok` for accepted or inactive-dropped frames;
- raise on an invalid ownership receipt rather than guessing.

## VideoInterop frame model

The current 0.1 model is DMA-BUF-specific despite generic outer names. Change it
before publishing 0.1.0.

### Frame shape

Make a frame self-describing so one-shot submission does not require opening a
format session first:

```elixir
%VideoInterop.Frame{
  format: %VideoInterop.Format{},
  visible_rect: %VideoInterop.Rect{},
  storage: %VideoInterop.Binary{} | %VideoInterop.DMABuf.Descriptor{},
  acquire_sync: :implicit | %VideoInterop.SyncFile{},
  lease: nil | %VideoInterop.Lease{}
}
```

Changes:

- Move coded width and height into `Frame.format.width/height`.
- Store the complete format on each frame.
- Default `visible_rect` to the full coded frame in constructors.
- Make `lease` optional instead of enforced.
- Keep timing outside the frame. Membrane PTS/DTS remains on
  `%Membrane.Buffer{}`; Emerge presents on submission.

### Binary storage

Add:

```elixir
%VideoInterop.Binary{
  data: binary(),
  planes: [%VideoInterop.Binary.Plane{}]
}

%VideoInterop.Binary.Plane{
  offset: non_neg_integer(),
  stride: pos_integer()
}

%VideoInterop.Binary.Format{
  pixel_format: atom(),
  bw1_polarity: :one_is_black | :one_is_white | nil
}
```

`VideoInterop.Format.storage` becomes either
`%VideoInterop.Binary.Format{}` or `%VideoInterop.DMABuf.Format{}`.

Provide constructors that remove struct-building ceremony:

```elixir
frame =
  VideoInterop.Frame.binary(data,
    width: 640,
    height: 480,
    pixel_format: :rgba8888,
    stride: 2560
  )
```

Support explicit plane lists for planar formats. A tightly packed single-plane
constructor may derive the stride when it is unambiguous.

Initial binary format contract:

- Required for existing headless output: `:rgba8888`, `:rgb888`, `:gray8`,
  `:gray2`, and `:bw1`.
- BW1 remains row-packed, MSB-first, with explicit polarity and zero tail bits.
- Gray2 remains row-packed, MSB-first, levels `0..3`, with zero tail bits.
- Add formats needed by the first Membrane integration only after defining exact
  byte/plane layouts. Prefer a small explicit table over accepting unchecked atoms.
- Do not expose Gray4 as stable until its odd-width multi-row packing is fixed.

Validate:

- positive dimensions;
- full plane count for the pixel format;
- minimum and maximum stride;
- offset/stride/row spans within `byte_size(data)`;
- chroma plane dimensions for subsampled formats;
- visible rectangle bounds;
- alpha mode and colorimetry compatibility where relevant.

### Ownership

Ownership depends on storage:

- `%VideoInterop.Binary{}` owns its immutable BEAM binary and must have
  `lease: nil` and `acquire_sync: :implicit`.
- `%VideoInterop.DMABuf.Descriptor{}` borrows reusable external storage and must
  have a valid `%VideoInterop.Lease{}`. Sync-file rules remain unchanged.

Update generic operations:

- `VideoInterop.release(binary_frame)` is a no-op.
- `VideoInterop.retain(binary_frame)` returns the same immutable frame.
- DMA-BUF retain/release behavior remains unchanged.
- Validation rejects a DMA-BUF frame without a lease and rejects unnecessary
  leases/fences on binary storage.

A destination renderer must copy or otherwise take native ownership of binary
pixels before `submit_video_frame/3` returns. It must never retain a Rustler term
or pointer tied to the calling NIF environment. After return, ordinary BEAM
reference counting and garbage collection are sufficient; there is no release
message.

### Rust crate

Mirror the storage-neutral model in `video-interop`:

- add owned/borrowed binary storage and validated plane layout types;
- make `FrameDescriptor.storage` an enum;
- make lease preparation/claim conditional on borrowed storage;
- keep DMA-BUF fd duplication and prepare-to-claim behavior unchanged;
- decode binary terms without retaining environment-bound pointers;
- add feature-parity tests with and without Rustler;
- update Hex/Cargo artifact parity checks for the new production sources.

Do not publish the currently prepared DMA-BUF-only 0.1.0 artifacts. Re-run the
0.1 release audit after this contract lands.

## Native Emerge target model

Replace explicit native target creation and stream opening with scene-derived
targets.

### Registry

Refactor `VideoRegistry` so the current render scene is the target registry:

- `active_scene_targets: HashSet<String>` remains authoritative.
- Remove public target resources, renderer epochs exposed through target handles,
  target incarnations, and one-open-stream-per-target state.
- A target entry is created lazily when a valid frame is submitted to an active
  target ID.
- A target entry stores the latest frame's dimensions, format, storage kind, and
  pending/rendered resources.
- Inactive IDs have no queued frame and no retained producer lease.
- A format or dimension change retires the old target resources and installs a
  new generation atomically.
- Snapshot/render synchronization continues to use internal generations so a
  stale GPU import cannot replace a newer frame.

### Admission

Add one NIF/transport operation:

```text
video_frame_submit(renderer, target_id, VideoInterop.Frame)
```

Admission order:

1. Validate the target ID and frame structure.
2. Validate backend support for the frame storage and pixel format.
3. Check renderer admission and current target visibility.
4. If inactive, return a caller-owned inactive receipt without allocating,
   importing, waking, or redrawing.
5. For DMA-BUF, duplicate descriptors/fences and prepare the lease.
6. For binary storage, copy bytes into renderer-owned memory.
7. Under the registry lock, replace the pending frame and claim the DMA-BUF lease
   only when queue ownership is established.
8. Retire the replaced frame through its storage-specific path.
9. Wake the renderer and return the exact ownership receipt.

All fallible DMA-BUF checks stay before lease claim. Once claimed, every error,
replacement, target removal, context loss, and renderer shutdown path must retire
the claim exactly once.

### Rendering

Generalize the renderer's video image lookup over two storage paths:

- binary frames become Skia raster images and work on raster, OpenGL, Vulkan, and
  Metal rendering routes;
- DMA-BUF frames keep the existing Linux OpenGL/Vulkan import and synchronization
  paths.

Both paths expose the frame's actual width/height to `image_fit`. Keep video
primitives direct-only so retained paint-layer caches never freeze live frames.

For macOS, add a protocol command carrying target ID, format, plane layout, and
binary bytes. Binary submissions must work through the external host. DMA-BUF
submission returns an unsupported-storage error. Bump the host protocol version
and update both sides together.

## Headless output

Both headless modes emit the same public frame type:

```elixir
{:emerge_skia_frame, %VideoInterop.Frame{} = frame}
```

### Binary mode

- Replace the string-keyed frame list/map with `%VideoInterop.Frame{}` using
  `%VideoInterop.Binary{}` storage.
- Set `lease: nil`.
- Preserve exact stride, packed-row, polarity, and dithering behavior in the
  binary format metadata.
- Remove transport-only sequence/timestamp fields unless a real caller requires
  them; they are not needed for display submission or EInk duplicate suppression.

### PRIME mode

- Continue wrapping exported DMA-BUFs in `%VideoInterop.Frame{}` with a managed
  lease and abandonment guard.
- Require a live local `headless.target` PID, as binary mode already does.
- Send the same `{:emerge_skia_frame, frame}` tuple.
- Remove the internal consumer destination, connection references, notify tuples,
  reconnect behavior, and disconnected producer mode.
- Keep bounded `max_in_flight`, backpressure, explicit synchronization, release
  retry, and synchronous drained shutdown.

Do not connect two Emerge renderers with an application `handle_info/2` bridge.
Use `membrane_video_interop` as the transport boundary:

```text
headless Emerge target PID
  -> Membrane.VideoInterop.Source
  -> optional Membrane conversion/processing
  -> Membrane.VideoInterop.Sink
  -> Emerge.submit_video_frame(window, :preview, frame)
```

There is still no Emerge connect/disconnect lifecycle. The Membrane pipeline owns
transport startup and shutdown; Emerge only produces and consumes frames.

## APIs and code to remove

Delete rather than deprecate, because 0.4 and VideoInterop 0.1 are unreleased:

- `Emerge.connect_video_output/3`
- `Emerge.disconnect_video_output/1`
- renderer behaviour callbacks for connect/disconnect
- `EmergeSkia.video_target/2`
- `EmergeSkia.video_target_info/1`
- `%EmergeSkia.VideoTarget{}`
- `EmergeSkia.VideoTargetConsumer`
- public `EmergeSkia.VideoConsumerSession`
- public `EmergeSkia.submit_video_frame/2` session API
- deprecated raw `EmergeSkia.submit_prime/2`
- target-new/info/raw-submit and consumer-session open/submit/close NIFs
- direct-connection notifications and tests
- target incarnation and stream identity exposed solely for those handles
- documentation, changelog, plans, demo code, and diagnostics describing direct
  connections or target registration

Keep internal generations and ownership receipts wherever they are still needed
to prevent stale frame installation and double retirement.

## Membrane adapter

Implement `membrane_video_interop` only after the storage-neutral VideoInterop
contract is stable.

### Raw video to VideoInterop

Convert this pair:

```elixir
{%Membrane.RawVideo{}, %Membrane.Buffer{payload: binary}}
```

into a binary `%VideoInterop.Frame{}` without copying the payload:

- map Membrane pixel-format atoms through an explicit table;
- map width, height, framerate, colorimetry, and alpha semantics;
- derive or validate plane offsets/strides from the RawVideo format;
- keep PTS/DTS and arbitrary Membrane metadata on the Membrane buffer, not in the
  VideoInterop frame.

### VideoInterop to raw video

For binary frames, return a `%Membrane.RawVideo{}` stream format and
`%Membrane.Buffer{payload: data}` when the plane layout is representable by
Membrane. Reject unsupported padded/multi-object layouts explicitly rather than
silently repacking.

DMA-BUF frames continue through VideoInterop-aware Membrane metadata/stream
formats and retain lease ownership rules. Conversion helpers must never pretend
a borrowed DMA-BUF is a raw binary payload.

### Headless ingress source

Provide a `Membrane.VideoInterop.Source` with a supervised local ingress endpoint
that can be used directly as `headless.target`. The ingress accepts
`{:emerge_skia_frame, %VideoInterop.Frame{}}`, transfers ownership into the
pipeline, and applies bounded demand/backpressure rules:

- binary frames can be dropped by normal BEAM garbage collection;
- leased frames dropped before pipeline admission are explicitly released;
- source/pipeline shutdown drains or releases every accepted leased frame;
- no application process translates descriptors or lease messages;
- ingress process death causes the supervised headless producer to restart or
  stop according to the application's supervision strategy.

Do not use an unacknowledged generic mailbox push contract for leased frames.
Define an admission acknowledgement/ownership handoff between the ingress and
source element so caller, ingress, and pipeline ownership is never ambiguous.
The headless producer's direct PID delivery and abandonment guard remain the
failure fallback, not the normal release path.

### Emerge submission sink

Provide a `Membrane.VideoInterop.Sink` that consumes complete
`%VideoInterop.Frame{}` values and invokes a configured callback or MFA. It must
not import Emerge. The Emerge demo configures it with the equivalent of:

```elixir
submit: {Emerge, :submit_video_frame, [MyApp.Window, :preview]}
```

The sink appends the frame to those configured arguments and calls:

```elixir
Emerge.submit_video_frame(MyApp.Window, :preview, frame)
```

The sink treats `:ok` as consumed, treats `{:error, reason}` as already consumed
and reports the pipeline error according to its policy, and never releases the
same frame again. Sink shutdown must wait for or safely abandon in-progress
submission without inventing ownership.

Pixel conversion remains a normal Membrane pipeline concern, for example by
placing a converter before the sink when Emerge does not support the source
format.

## Implementation phases

### Phase 1: replace the VideoInterop data contract

Repositories: `video_interop`.

1. Add binary storage/format/plane modules and constructors.
2. Put `Format` on `Frame`; remove duplicate coded dimensions.
3. Make lease validation storage-dependent.
4. Update retain/release/consume behavior for owned binary frames.
5. Mirror the model in the Rust crate and Rustler codecs.
6. Rewrite tests, README, ExDoc, changelog, package lists, and artifact parity.
7. Run the complete Elixir 1.17/current and Rust 1.91/current release matrix.

Exit gate: both packages build/package from one clean tree, binary frames require
no lease, and existing DMA-BUF ownership tests still pass unchanged in meaning.

### Phase 2: simplify the Emerge Elixir API

Repository: `emerge-headless`, branch `headless-backend`.

1. Change `Emerge.UI.video/2` and validation to atom targets.
2. Update EMRG encoding tests for deterministic atom-to-string target IDs.
3. Add the private viewport renderer endpoint registry.
4. Add renderer `submit_video_frame/3` callback and top-level Emerge API.
5. Implement receipt normalization and exact release behavior.
6. Remove connect/disconnect callbacks and public functions immediately.
7. Remove `VideoTarget` construction and consumer-session public APIs.

Exit gate: a fake renderer proves visible-submit, inactive-drop, error ownership,
viewport restart, shutdown race, registered-name lookup, and submission from the
viewport's own process without deadlock.

### Phase 3: native generic submission and binary rendering

Repository: `emerge-headless`.

1. Add generic frame submission transport/NIF calls.
2. Refactor scene-derived target registry and latest-frame replacement.
3. Add owned binary frame validation/copy and Skia raster image creation.
4. Preserve DMA-BUF prepare/claim/import/retirement without public sessions.
5. Handle target removal and format/dimension replacement.
6. Implement binary rendering on raster, Wayland OpenGL, DRM OpenGL, and Vulkan.
7. Add macOS protocol/host binary submission.
8. Delete obsolete native target/session APIs and dead registry state.

Exit gate: all software tests pass and no public or native call path still requires
a target resource or stream connection.

### Phase 4: unify headless production

Repositories: `emerge-headless`, NameBadge downstream.

1. Emit binary VideoInterop frames from raster/GL headless output.
2. Emit leased VideoInterop frames from PRIME output.
3. Remove PRIME connection state and require a delivery PID.
4. Update NameBadge `FrameSink` to read `%VideoInterop.Binary{}`.
5. Preserve packed-frame validation and identical-binary suppression.

Exit gate: BW1/Gray2 outputs remain byte-for-byte identical and PRIME slots remain
bounded, ownership-safe, and synchronously drained.

### Phase 5: Membrane conversion and Emerge transport

Repository: new `membrane_video_interop` project or its eventual public worktree.

1. Add RawVideo/Buffer conversion helpers and explicit format maps.
2. Add no-copy binary conversion tests.
3. Add DMA-BUF ownership tests.
4. Add the supervised headless-frame ingress and
   `Membrane.VideoInterop.Source`.
5. Add the callback/MFA-driven `Membrane.VideoInterop.Sink`.
6. Prove source admission, demand, drop, shutdown, and lease release behavior.
7. Validate an ordinary decoded/raw Membrane pipeline feeding Emerge.
8. Validate headless binary and PRIME Emerge sources feeding a target Emerge
   viewport through the source/sink pipeline.

Exit gate: both ordinary raw video and headless Emerge output can reach an atom
Emerge target through `membrane_video_interop`, without target handles, consumer
sessions, application mailbox bridges, or manual lease messages.

### Phase 6: migrate `../emerge_demo`

Repository: `../emerge_demo`.

Apply the completed APIs only after Phases 1-5 are stable:

1. Replace `%EmergeSkia.VideoTarget{}` creation with an atom such as
   `:headless_prime_preview` in `Emerge.UI.video/2`.
2. Delete `EmergeSkia.video_target/2`, `connect_video_output/3`, connection
   references, connection notifications, reconnect logic, and disconnect logic.
3. Configure the headless viewport's target as the supervised
   `membrane_video_interop` ingress endpoint.
4. Add a Membrane pipeline from `Membrane.VideoInterop.Source` to
   `Membrane.VideoInterop.Sink`.
5. Configure the sink to call
   `Emerge.submit_video_frame(showcase_viewport, :headless_prime_preview, frame)`.
6. Keep optional conversion/diagnostic elements inside the pipeline rather than
   in Emerge callbacks.
7. Remove `PrimeBridge`, `PrimeRenderer`, descriptor conversion, keepalive, or
   lease-routing code if any remains.
8. Update demo supervision so ingress/pipeline startup precedes the headless
   renderer and shutdown drains in the reverse order.
9. Update demo documentation and validation scripts to describe the Membrane
   route.

Exit gate: the demo's headless binary/PRIME showcase reaches its visible Emerge
target only through `membrane_video_interop`, survives target hide/show and
viewport restart, and has bounded frames, leases, file descriptors, and RSS.

### Phase 7: cleanup, documentation, and release

1. Remove all stale direct-connection and `VideoTarget` references across Emerge,
   VideoInterop, `../emerge_demo`, plans, and changelogs.
2. Document only the atom target and submit API in `Emerge`/`Emerge.UI`.
3. Document storage contracts and constructors in VideoInterop.
4. Document the headless-to-Emerge Membrane source/sink pipeline in
   `membrane_video_interop` and `../emerge_demo`, not as an Emerge connection API.
5. Re-run Emerge and demo package/source checks with registry-only dependencies.
6. Publish `video-interop`, then `video_interop`, then the Membrane adapter.
7. Update Emerge and demo locks only after registry artifacts are verified.

## Test matrix

### VideoInterop

- binary constructor defaults and explicit planes;
- packed and padded stride bounds;
- planar span/subsampling bounds;
- binary retain/release no-op behavior;
- DMA-BUF lease still mandatory;
- binary lease/fence rejection;
- Elixir/Rustler schema parity;
- Cargo core-only, all-features, Clippy, Rustdoc, package, and parity gates.

### Emerge Elixir

- atom-only `video/2` target validation;
- no intrinsic size without explicit layout;
- target atom codec round-trip;
- renderer endpoint registration/unregistration;
- stopped/not-ready viewport release behavior;
- exact ownership receipt normalization;
- callback-self submission does not deadlock;
- custom renderer unsupported behavior;
- absence of connect/disconnect and target-handle APIs.

### Native renderer

- inactive target drops without wake or allocation;
- active target latest-frame replacement;
- target removal releases pending and displayed frames;
- same atom drawn in multiple nodes;
- binary data remains valid after the BEAM term is collected;
- dimension/format changes rebuild safely;
- RGB/RGBA/Gray8 upload correctness;
- BW1/Gray2 unpack correctness when used as an Emerge video source;
- DMA-BUF implicit and sync-file paths;
- lease release on replace, hide, rejection, context loss, and shutdown;
- raster, Wayland, DRM, headless, Vulkan, and macOS routes.

### End to end

- ordinary Membrane raw-video frame to visible Emerge target;
- invisible target consumes/drops without producer backpressure;
- headless binary Emerge through `membrane_video_interop` to windowed Emerge;
- headless PRIME Emerge through `membrane_video_interop` to windowed Emerge;
- ingress/source/sink shutdown releases every leased frame;
- target removed/re-added during production;
- viewport restart while the Membrane producer continues;
- migrated `../emerge_demo` uses no direct mailbox bridge or connection API;
- sustained run with bounded RSS, file descriptors, leases, and GPU imports.

## Validation commands

Emerge:

```bash
mix format --check-formatted
mix test
cd native/emerge_skia && cargo test
cd native/emerge_skia && cargo clippy --all-targets --all-features -- -D warnings
./ci-tests.sh all
```

VideoInterop:

```bash
mix format --check-formatted
mix test
mix docs --warnings-as-errors
mix hex.build
cargo test --workspace --all-features
cargo test -p video-interop --no-default-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo package -p video-interop
scripts/check-release-artifact-parity.sh
```

Hardware acceptance remains required for Linux DMA-BUF OpenGL/Vulkan import and
retirement. Binary frame correctness must be covered by deterministic software
pixel tests on every backend available in CI.

## Completion criteria

- Emerge's only application-facing video operations are `video(attrs, atom)` and
  `submit_video_frame(viewport, atom, frame)`.
- No connect/disconnect API or renderer-owned target handle remains.
- Binary VideoInterop frames contain no lease and rely on normal BEAM ownership.
- Borrowed DMA-BUF frames retain exact lease and synchronization guarantees.
- Hidden targets drop frames without allocation, wake, redraw, or leaked holders.
- Both headless output modes emit `%VideoInterop.Frame{}`.
- An ordinary Membrane raw-video pipeline can reach Emerge through explicit
  adapter conversion.
- Headless-to-target Emerge forwarding runs through `membrane_video_interop` and
  uses `Emerge.submit_video_frame/3` only at the sink boundary.
- `../emerge_demo` is migrated to that pipeline after the library implementation
  is complete.
- No application mailbox bridge or Emerge-specific connection lifecycle remains.
