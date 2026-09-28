# FP3 long-running animation corruption

Reported: Goat title slide left running for >12h; its SVG then flashes during animations on other slides, degradation persists until renderer restart.

Investigation:
1. Trace title animation, deployed dependency/backend, clocks and scene/cache ownership.
2. Build an accelerated regression for any reproducible failure; distinguish confirmed defects from device-only hypotheses.
3. Make a focused fix if proven and validate with cargo test and mix test.

## Findings

- Goat's `mix.exs` points at `../../emerge-headless`. Both that checkout and this one are at `ce83fcd26974513d5f29db1fb7e45db3e50791c8`; the installed FP3 firmware revision has not been verified.
- `goat/config/fp3.exs`: DRM/OpenGL, Mesa Freedreno/Adreno 506, 1080x2160, software cursor.
- `goat/lib/goat/presentation/view/slides/01_title.ex`: 18-second infinite SVG tint loop, 543 keyframes, 181 gradient stops/frame, in a nearby overlay above the same untinted SVG. There is no slide-change timer on the title.
- Timing uses `Instant` and elapsed `f64` milliseconds; frame/cache counters are `u64`. No 12-hour rollover was found.
- Added accelerated native regression: identical title-loop pixels at matching phases after 12/24/72 hours; removing the title host also removes its nearby SVG and animation entry; subsequent frames on the same renderer/cache match a fresh title-free scene. Passes. This is clock/lifecycle coverage, **not** a GPU allocation soak.

## DRM framebuffer lifetime defect

Before the fix, `native/emerge_skia/src/backend/drm/gl.rs::framebuffer_for_bo` cached `GEM handle (u32) -> DRM framebuffer` until session teardown. A handle is not a permanent buffer identity. Once a GBM BO is destroyed, its GEM handle may be reused; the old framebuffer can continue referencing the old backing allocation. Returning that framebuffer for a new BO presents stale pixels instead of the buffer just rendered. Obsolete framebuffer references also retain backing allocations until renderer restart.

The current FP3 system changelog selects Mesa 26.1.8. Its EGL/GBM implementation explicitly destroys unused BOs **within a live surface**: `get_back_bo` calls `destroy_oldest_unused_bo` after 1000 consecutive qualifying excess-buffer frames. Therefore a session-long mapping is unsafe even without resize/hotplug.

Source inspected: https://raw.githubusercontent.com/chaotic-cx/mesa-mirror/mesa-26.1.8/src/egl/drivers/dri2/platform_drm.c (`get_back_bo`, `destroy_oldest_unused_bo`, `release_buffer`). The 1000-frame condition is not a guaranteed onset time; it depends on buffer pressure/scheduling.

### Local reproduction

An opt-in regression allocates/destroys GBM BOs, queries their DRM framebuffer dimensions, and forces handle recycling by varying buffer sizes. It never sets a mode or touches a plane/scanout.

Against the original implementation on local `/dev/dri/card1`, it fails on **iteration 1**:

```
GEM handle=1, recycled=1
actual framebuffer: (64, 64)
new BO:             (80, 64)
```

This directly reproduces the stale framebuffer lookup, independently of the animation engine. Different dimensions make the identity mismatch obvious in the test; FP3's equal-sized swapchain buffers could silently display old pixels without an invalid-size error. The AMD driver printed an acceleration-permission warning, but GBM allocation and DRM framebuffer ioctls succeeded (this test does not require GPU rendering).

This fits old title pixels flashing only while new frames are being presented and restart clearing the problem. It also explains accumulation: new handles leave retired BO storage pinned by old framebuffer objects; recycled handles can alias those stale objects. The exact FP3 incident still needs device confirmation, not a claim that the phone was reproduced locally.

### Fix

`PrimaryFramebuffer` is now GBM BO userdata and owns an `Arc<Card>`. The BO destruction callback removes the framebuffer; the session-long GEM-handle map is gone. Dropping a front-buffer lock still only releases it to the surface. Existing KMS teardown/quarantine ordering remains unchanged, and the card/output lease survives through framebuffer cleanup.

The hardware regression also checks repeated lookup on one live BO, framebuffer retirement after BO destruction, and 128 allocations with observed handle reuse.

### Validation

Command for the opt-in regression (use an accessible KMS card):

```sh
EMERGE_TEST_DRM_CARD=/dev/dri/card1 cargo test \
  --manifest-path native/emerge_skia/Cargo.toml --release \
  --no-default-features --features drm \
  recycled_gbm_handles_do_not_reuse_stale_framebuffers -- --ignored --nocapture
```

- Before fix: regression fails with stale dimensions above.
- Accelerated title loop/removal regression: passes.
- Post-fix opt-in DRM/GBM regression: passes (128 allocation lifecycles, observed GEM handle reuse).
- `cargo test --release`: 1479 unit + 14 integration tests passed.
- `cargo test --release --no-default-features --features drm`: 1483 unit + 14 integration tests passed; 2 hardware tests ignored by default (the new regression was run explicitly above).
- `cargo clippy --release --no-default-features --features drm --tests -- -D warnings`: passes.
- `mix test`: 542 passed, 10 excluded (after fetching the already-locked `video_interop 0.1.2`; no lockfile change).
- All Cargo commands used `--manifest-path native/emerge_skia/Cargo.toml`. No FP3 soak/presentation test was run.

### FP3 follow-up

Goat currently uses `../emerge-headless`, **not this worktree**. Apply this fix there or point the build at this worktree before rebuilding FP3 firmware; nothing in the sibling checkout/firmware was changed by this investigation.

Repeat the title-slide soak and slide changes on FP3. If it still occurs, capture a renderer screenshot and a photograph/video of the panel before restarting: a correct pre-swap screenshot with corrupt scanout separates presentation from scene/GPU drawing. Record the actual firmware/Emerge/Mesa revisions and kernel DRM/GPU fault logs as well. Disabling only the paint-layer cache does not bypass this framebuffer lifetime bug.
