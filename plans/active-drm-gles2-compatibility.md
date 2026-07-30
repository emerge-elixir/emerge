# Active plan: DRM OpenGL ES 2 compatibility

## Status

Implemented on `drm-gles2-support`, based on commit `0661757` before the v0.3.3 preparation commit. Automated validation is complete, and base DRM startup/rendering is confirmed on GLES2-only Macaw hardware. Extended PRIME degradation checks and the newer-device smoke test remain open.

## Goal

Keep OpenGL ES 2 as the minimum and explicitly requested API for the Linux DRM backend. The base renderer must start on a GLES2-only EGL/GBM device without a compatibility option. Optional capabilities must be enabled only after the context is current and must degrade or become unavailable without stopping ordinary UI rendering.

## Compatibility policy

- DRM requests an `EGL_OPENGL_ES2_BIT` config and an OpenGL ES 2 context on every device, including devices that also support ES3.
- Do not add a public `gles_version` or compatibility option.
- Do not try ES3 first in this slice. Always using the ES2 path prevents newer hardware and CI from hiding an ES2 regression.
- Core UI rendering may depend on EGL/GBM, a GLES2 context, and a Skia Ganesh GLES2 context only.
- Optional profiling and PRIME video initialize after the base renderer. Their absence is a contained capability degradation, not a DRM startup failure.
- Future code must not raise the EGL context requirement merely to enable one optional feature. Gate that feature by the actual GL/EGL version, extension advertisement, and loaded entry points.

## Original regression

Before this implementation, `native/emerge_skia/src/backend/drm.rs::init_egl` filtered for `EGL_OPENGL_ES3_BIT_KHR` and created context version 3. GLES2-only devices therefore failed before rendering. This changed in `8aa5022` while v0.3.2 requested `EGL_OPENGL_ES2_BIT` and context version 2.

The direct GL audit found these calls beyond GLES2 core:

- `glGetStringi`/`GL_NUM_EXTENSIONS` in DRM timer-query detection
- core VAO calls in the video blitter
- core GL sync calls in imported-frame retirement

The video shader, VBO/FBO/readback operations, and ordinary DRM drawing calls are GLES2-compatible. PRIME video additionally depends on EGL DMA-BUF import and `GL_OES_EGL_image_external`, which are optional extensions rather than an ES3 requirement.

## Capability behavior

| Capability | Availability rule | Behavior when unavailable |
| --- | --- | --- |
| Base DRM UI | ES2 EGL config/context and successful Skia Ganesh GLES2 initialization | Return a normal DRM startup error, never panic, because this is the minimum contract |
| GPU timer profiling | Exact `GL_EXT_disjoint_timer_query` advertisement plus every required EXT entry point | Leave `GpuQueueTimer` disabled; stats and UI continue |
| Core VAOs | GLES major version 3+ plus all used core VAO entry points | Use the existing GLES2 VBO/per-draw attribute path |
| Core GL sync objects | GLES major version 3+ plus `FenceSync`, `ClientWaitSync`, and `DeleteSync` | Use the existing `glFinish` imported-resource retirement path |
| PRIME DMA-BUF import | EGL DMA-BUF import support, EGL image create/destroy support, `GL_OES_EGL_image_external`, and `glEGLImageTargetTexture2DOES` | Report PRIME video unavailable through the existing video-target capability check; base UI continues |
| DMA-BUF modifiers | `EGL_EXT_image_dma_buf_import_modifiers` | Permit frames without explicit modifiers; reject modifier-dependent frames without affecting the renderer |
| Direct external texture wrapping | Ganesh accepts the imported external texture | Keep the existing RGBA blit fallback; reject only the video frame if both paths fail |

## Implementation plan

### 1. Restore the fixed GLES2 EGL baseline

File: `native/emerge_skia/src/backend/drm.rs`

- Remove `EGL_OPENGL_ES3_BIT_KHR` from DRM setup.
- In `init_egl`, preserve the current RGB8/XRGB8888 visual-selection logic but filter with `egl::OPENGL_ES2_BIT`.
- Create the context with `egl::CONTEXT_CLIENT_VERSION = 2`.
- Rename ES3-specific comments and failure messages to ES2.
- Extract the config and context attribute arrays into small pure helpers or constants so unit tests can assert the requirement directly.
- Harden `egl_get_platform_display`: gate optional platform-display entry points with exact EGL client extensions where available, try supported EXT/core platform paths in order, and fall back to legacy `eglGetDisplay` when an optional path returns `EGL_NO_DISPLAY`. Fail only after every GBM display path fails.
- Keep swap policy, KMS setup, and Wayland context negotiation unchanged.

### 2. Make GL capability discovery and Ganesh setup safe on GLES2

Files:

- `native/emerge_skia/src/backend/drm.rs`
- `native/emerge_skia/src/backend/skia_gpu.rs`

- After `eglMakeCurrent` and `gl::load_with`, read `GL_VERSION` and the space-delimited `GL_EXTENSIONS` string with `glGetString`.
- Parse extension names as exact whitespace-delimited tokens; do not use substring matching.
- Replace `gl_has_extension`, which currently calls ES3-only `GL_NUM_EXTENSIONS`/`glGetStringi` unconditionally.
- Build a small internal capability snapshot for logging and optional-path decisions. Missing or malformed version/extension data means the optional capability is unsupported; it must not abort base startup.
- Add a fallible `GlFrameSurface` construction path for DRM. Propagate Ganesh default-framebuffer wrapping failure through `create_frame_surface` and the existing DRM startup `Result` instead of reaching `expect` in `create_gl_surface`.
- Keep Wayland behavior unchanged unless sharing the fallible constructor simplifies the API without changing policy.
- Log the actual GLES version and the selected optional paths once at startup.

### 3. Gate profiling without affecting rendering

File: `native/emerge_skia/src/backend/drm.rs`

- Make `GpuQueueTimerApi::load` consume the GLES2-safe extension result.
- Continue requiring every `GL_EXT_disjoint_timer_query` symbol and nonzero timer counter width.
- Preserve `GpuQueueTimer::new`'s warning-and-disabled state when the extension or any symbol is absent.
- Ensure enabling `stats` or `renderer_stats_log` cannot invoke `glGetStringi`, produce `GL_INVALID_ENUM`, or terminate a GLES2 renderer.

### 4. Gate video, VAO, and sync paths

Files:

- `native/emerge_skia/src/video.rs`
- `native/emerge_skia/src/backend/drm.rs`

Changes:

- Pass a small internal capability description into `VideoImportContext::new_current_direct` rather than relying only on loaded function pointers.
- Enable unsuffixed core VAO calls only for a GLES3+ context with all required entry points. On the fixed DRM ES2 context, retain the current VBO and per-draw attribute setup.
- Enable core GL sync calls only for a GLES3+ context with all three required entry points. Otherwise retain the current `glFinish` fallback.
- Before creating a PRIME import context, require exact EGL DMA-BUF import and GL external-image extension advertisements in addition to the existing EGL/GL symbol checks.
- Track DMA-BUF modifier support separately. Reject a frame carrying an explicit modifier if the modifier extension is absent; do not silently drop modifier metadata.
- Preserve the current direct-external-to-RGBA-blit fallback when Ganesh cannot wrap an external texture.
- Keep all PRIME initialization/frame errors contained to video and emit one actionable capability message instead of failing DRM startup.

### 5. Make unavailable PRIME video observable and release every rejected frame

Files:

- `native/emerge_skia/src/backend/drm.rs`
- `native/emerge_skia/src/lib.rs`
- `native/emerge_skia/src/video.rs`

Changes:

- Replace the immutable DRM `prime_video_supported: bool` assumption with availability state held under the `VideoRegistry` mutex. Initialize it as unavailable, set it available only after `VideoImportContext` succeeds, and clear it before DRM session teardown or context recreation.
- Check availability under that same registry lock during both `video_target_new` and `video_target_submit_prime`; existing targets must stop accepting frames while a DRM session lacks import support and may resume after a successful recreation.
- Keep the existing NIF/public API shape. Return an explanatory unsupported-capability error when required DMA-BUF/external-image support is absent.
- When `RendererVideoState::sync_pending` has no import context, drain pending registry frames to the release worker rather than leaving them retained.
- If render-target construction fails before pending frames are snapshotted, explicitly consume/defer those frames before returning the error. Use a cleanup guard or reorder the snapshot so every error path has one owner.
- Reject modifier-dependent frames when modifier import is unavailable and route them through the same native-thread release path.
- Preserve the existing Wayland no-import drain behavior and apply the same ownership invariant to DRM.
- Add no new renderer option. Ordinary renderers and non-video trees remain unaffected.

Ownership invariant: after submission, every frame is held by exactly one registry/import/retirement owner or has been queued for release, regardless of capability changes or setup errors.

### 6. Document the minimum and degradation rules

Files:

- `README.md`
- `guides/tutorials/set_up_viewport.md`
- `lib/emerge_skia.ex`

Document that DRM requires GLES2, needs no version option, and treats timer profiling and PRIME video as optional capabilities. Note that startup logs show the actual GL version and disabled optional paths.

## Tests

Add headless unit coverage in the existing Rust test modules:

- EGL config attributes contain `OPENGL_ES2_BIT` and not the ES3 bit.
- EGL context attributes request client version 2.
- Platform-display selection falls through optional EXT/core failures to the legacy GBM display path.
- Vendor-style `OpenGL ES 2.0 ...` and `OpenGL ES 3.x ...` version strings parse conservatively.
- Extension matching is exact and rejects substring lookalikes.
- An ES2 capability fixture disables core VAO and sync paths even if misleading unsuffixed pointers appear loaded.
- Timer profiling is disabled when the extension or any required EXT symbol is absent.
- PRIME support is false when DMA-BUF import, external-image advertisement, or the image-target entry point is absent.
- Modifier-bearing frames are rejected when modifier import is unsupported; modifier-free frames remain eligible.
- PRIME target creation is rejected while the registry availability state is false.
- Pending frame leases reach the release queue when the import context is absent, target construction fails, modifier import is rejected, or session capability changes from available to unavailable.
- A deterministic submission-versus-disable race test proves that either submission is rejected or the accepted frame is drained, with no pending lease left behind.

No unit test should require DRM master, GBM hardware, or a live EGL context.

## Automated validation

After implementation, run:

```bash
cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features drm
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features drm -- -D warnings
cargo build --release --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features drm
cargo test --manifest-path native/emerge_skia/Cargo.toml
mix test
./ci-tests.sh all
```

The DRM-only build is mandatory because the normal default build exercises Wayland.

## Hardware acceptance

On a genuine GLES2-only DRM/GBM target:

1. Capture `eglinfo` or vendor diagnostics proving no ES3 config/context is available.
2. Deploy the DRM-only build and start an ordinary viewport with `backend: :drm` and no compatibility option.
3. Confirm logs report an OpenGL ES 2.x context and successful first atomic commit/page flips.
4. Exercise rectangles, text, images, clips, gradients, input, hardware/software cursor behavior, animation, repeated rerenders, and clean stop/restart.
5. Enable `renderer_stats_log`; missing timer-query support may warn once but must not stop rendering.
6. Confirm VAO/sync core paths are disabled and no missing-symbol panic or `GL_INVALID_ENUM` occurs.
7. If PRIME extensions are absent, confirm PRIME target creation and submission return contained unsupported-capability errors while surrounding UI continues.
8. If PRIME extensions are present on GLES2, submit and replace frames. Exercise the RGBA fallback on a target known to reject direct Ganesh external wrapping, or use an internal validation-only policy injection; do not add a public option.
9. Verify descriptor release with owner release notifications/video lease counters and stable process FD counts after replacement, rejection, capability loss, and teardown.

Also smoke-test on a newer GLES3-capable DRM device. It should still create the requested ES2 context, proving that normal development hardware exercises the compatibility baseline.

## Non-goals

- No ES3-first negotiation or public GLES version selection.
- No Wayland EGL policy change.
- No replacement renderer if the installed Skia build cannot create a GLES2 Ganesh context; the failure must still propagate without a panic.
- No unrelated swap pacing, KMS, renderer cache, or DRM input changes.

## Residual risks

- Base Skia Ganesh compatibility is confirmed on one GLES2-only DRM/GBM target, but EGL/GBM behavior still varies across vendors.
- PRIME DMA-BUF modifier behavior varies by driver and format and needs target testing.
- The fallible surface path can report that Ganesh rejected a framebuffer, but diagnosing a vendor-specific format mismatch may require a follow-up with hardware evidence.

## References

- Khronos OpenGL ES registry: <https://registry.khronos.org/OpenGL/index_es.php>
- Khronos EGL registry: <https://registry.khronos.org/EGL/>
