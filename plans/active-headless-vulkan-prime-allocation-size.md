# Active Plan: Truthful headless Vulkan PRIME allocation sizes

Status: implemented; direct Vulkan/OpenGL producer hardware proof passed, Wayland consumer matrix pending.

## Audit result

The plan is sound: the strict importer is correct and the producer must publish the fd-backed
allocation. Implementation uses `RawFd` rather than `BorrowedFd` for the public probe so invalid-fd
callers retain a typed syscall error without constructing an invalid borrowed-fd value. No
application-side demo workaround or importer relaxation was admitted.

## Diagnosis

The failure is a producer metadata bug exposed by the hardened `video_interop` importer, not an
NV12/import-validation bug:

- the demo source is `640x420` ABGR8888, so its visible packed span and current Vulkan memory
  requirement are `640 * 420 * 4 = 1_075_200` bytes;
- `ExportedDmaBufImage` publishes that Vulkan requirement as the descriptor object size;
- the exported DMA-BUF fd reports an actual page-rounded allocation of `1_077_248` bytes
  (`263 * 4096`), including a real 2,048-byte tail;
- `video_interop` commit `bda8969` correctly requires the descriptor's complete allocation size to
  equal the fd-backed size, so both Vulkan-producer demo routes now fail before import.

The fix must publish the complete fd-backed allocation while continuing to constrain image access
to the one visible plane. Do not weaken exact importer validation, truncate the fd, pretend the tail
is visible image data, or require the fd-backed size to equal `VkMemoryRequirements::size`.

## Invariants

- `VideoInterop.DMABuf.Object.size` is the complete size reported by the DMA-BUF fd.
- Vulkan's requested/bound allocation size and the exported fd-backed allocation size are distinct
  facts; the latter may be larger because of exporter alignment.
- The fd-backed size must be at least the Vulkan image memory requirement.
- Plane `offset`, `pitch`, dimensions, and checked row-span arithmetic remain the authority for bytes
  that the image may address. Any allocation tail remains unreferenced padding.
- Size probing happens once when a persistent export slot is created, never per rendered frame.
- Probe failure, zero size, contradictory probes, or an fd-backed size smaller than the Vulkan
  requirement fails slot creation before publication.
- Existing explicit-sync, lease, backpressure, retirement, and slot-reuse behavior is unchanged.

## Implementation plan

### 1. Make fd-backed allocation probing reusable and canonical

In `../video_interop/rust/video-interop`:

1. Extract the Linux DMA-BUF size query currently embedded in
   `src/vulkan/identity.rs` into the core producer/consumer helper
   `video_interop::dmabuf_allocation_size(RawFd)`.
2. Preserve the hardened semantics from `bda8969`:
   - query `SEEK_END`;
   - preserve/restore the shared file position when `SEEK_CUR` is supported;
   - cross-check a nonzero `fstat.st_size`;
   - reject zero, unavailable, negative/overflowed, or disagreeing results with a typed error.
3. Keep `verified_dmabuf_identity` responsible for comparing a caller's declared size with the
   observed size, but implement it through the shared helper so producer and consumer cannot drift.
4. Add core tests for exact memfd size, position restoration, zero size, and non-seekable failure;
   retain Vulkan tests for exact, under-declared, and over-declared descriptors.
5. Document that the helper returns the complete fd-backed allocation, not a visible plane span or
   Vulkan image requirement.

This is an additive Rust API. It does not change the Elixir frame schema or relax any importer check.

### 2. Publish the actual size from Emerge's persistent export slots

In `native/emerge_skia/src/backend/vulkan/external_image.rs`:

1. After `vkGetMemoryFdKHR` succeeds and the returned fd is wrapped in `OwnedFd`, call the canonical
   allocation-size helper.
2. Compare the observed fd-backed size against `requirements.size`; accept equality or a larger
   aligned allocation and reject only a smaller allocation.
3. Store the observed value explicitly as `fd_allocation_size` rather than storing
   `requirements.size` under the ambiguous `allocation_size` name.
4. Preserve current failure cleanup: a probe/validation error drops the exported fd and destroys the
   image before freeing memory through the existing construction error path.
5. Expose only the fd-backed value to `backend/headless/vulkan.rs` when constructing
   `PrimeObjectMeta`. Keep Vulkan import/bind calls based on Vulkan's own requirements.

Also replace Emerge's divergent local size helpers in
`backend/headless/offscreen_gl.rs` and `backend/drm/functional_probe.rs` with the canonical query
where feature boundaries allow. This keeps OpenGL output, Vulkan output, and DRM probe/import
metadata under one definition and removes the current fstat-only versus seek/fallback split.

Add pure tests for the export-size relation: equal and larger fd-backed allocations are valid;
zero/smaller values fail. In `video.rs`, add a canonical packed-frame case proving a descriptor with
an allocation tail validates while one shorter than the checked plane span still fails.

### 3. Add a Vulkan producer regression test

Extend the hardware-gated headless PRIME coverage in `test/emerge_skia_test.exs` with an explicit
`rendering_api: :vulkan` case at `640x420`:

- receive one canonical frame directly from the headless producer;
- assert one linear ABGR8888 object/plane and sync-file acquisition;
- assert the published object size equals the size observed through its live fd and is at least the
  checked plane span;
- validate, retain/release, produce a subsequent frame, and stop cleanly;
- keep fd and lease counts bounded.

The existing `:auto` test remains the OpenGL control and must continue selecting OpenGL.

### 4. Validate through `../emerge_demo`

No application-side descriptor rewrite belongs in `emerge_demo`; it should remain the end-to-end
producer/consumer acceptance harness. Run the two affected routes first:

```bash
cd ../emerge_demo
EMERGE_DEMO_PRIME_DRM_NODE=/dev/dri/renderD128 \
  ./scripts/prime-matrix.sh vulkan opengl
EMERGE_DEMO_PRIME_DRM_NODE=/dev/dri/renderD128 \
  ./scripts/prime-matrix.sh vulkan vulkan
```

Require both to accept frames without allocation-size drops, preserve byte-exact submitted/main
pixels, pass hide/show and reconnect, restart both renderers, and remain within existing FD/RSS
bounds. Then run the full four-route matrix so both OpenGL producer controls remain unchanged.

Repeat the Vulkan-Vulkan route with `EMERGE_VULKAN_VALIDATION=1`. It must produce no Vulkan
validation errors when the descriptor publishes the larger fd-backed allocation while the imported
image memory is allocated/bound according to the consumer's Vulkan requirements.

Only change the demo harness if needed to make a failed descriptor-size assertion immediate and
route-specific; do not add a workaround in `PrimeSource` or the application relay.

### 5. Documentation and integration order

1. Update `../video_interop/rust/video-interop/README.md` to distinguish complete fd-backed
   allocation size from logical packed/NV12 spans.
2. Update `guides/internals/video-interop-architecture.md` and
   `plans/active-vulkan-rendering-api.md` with the headless Vulkan publication rule and validation
   evidence.
3. Add changelog entries only when implementation lands.
4. Commit the reusable `video_interop` probe first, then the Emerge producer fix/tests/docs, then any
   optional demo-harness assertion. Never leave Emerge pointing at a `video_interop` revision that
   lacks the helper.

## Validation

```bash
# video_interop
cd ../video_interop
cargo fmt --all -- --check
cargo test --workspace --no-default-features
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
mix format --check-formatted
mix test

# Emerge
cd ../emerge-headless
cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo test --manifest-path native/emerge_skia/Cargo.toml \
  --no-default-features --features headless-vulkan
cargo clippy --manifest-path native/emerge_skia/Cargo.toml \
  --all-targets --features headless-all -- -D warnings
mix format --check-formatted
mix test
./ci-tests.sh all
git diff --check

# Demo host tests and hardware matrix
cd ../emerge_demo
mix format --check-formatted
mix test
EMERGE_DEMO_PRIME_DRM_NODE=/dev/dri/renderD128 ./scripts/prime-matrix.sh
```

## Implementation evidence

On RADV `/dev/dri/renderD128`, the direct 640x420 headless producer probes passed for both APIs:

```text
headless Vulkan: declared=1077248 fd=1077248 visible=1075200
headless OpenGL: declared=1077248 fd=1077248 visible=1075200
```

This proves the reported Vulkan producer mismatch is corrected and the OpenGL control still
publishes the same complete allocation. Default and headless-Vulkan Rust suites, the complete
Emerge quality/test/dialyzer CI split, dual Wayland/headless Vulkan all-target Clippy,
`video_interop` all-feature Rust/Clippy/ExUnit, and Emerge Demo ExUnit pass. The four-route demo
matrix remains pending because this environment has no running Wayland compositor.

## Completion criteria

- The Vulkan producer publishes `1_077_248` for the reported failing fd (or whatever exact size that
  live driver reports), not the `1_075_200` visible/Vulkan requirement.
- The strict importer reports zero declared-versus-fd allocation mismatches.
- Both Vulkan-producer demo routes pass byte-exact rendering and lifecycle checks.
- OpenGL producer routes, explicit synchronization, lease retirement, and bounded resource behavior
  do not regress.
- No code treats the fd allocation tail as visible pixels or weakens exact descriptor validation.
