# Active plan: DRM display framerate and animation cadence

## Status

Implemented and audited in the current worktree; target hardware validation pending.

Current local stance after the cross-backend enter-animation audit: keep the physical mode interval/stat fixes, but do not rely on DRM event timestamp/future-vblank prediction or broad backend animation-completion workarounds while the core tree animation completion fix is being validated.

## Problem

A DRM display can report a fixed mode such as `1024x600 @ 60Hz`, while renderer stats do not consistently show `display: 60 fps`. The same timing source is also used for DRM animation pulse prediction, so sparse primary page flips can affect animation sampling.

## Previous behavior

- DRM chooses a mode in `native/emerge_skia/src/backend/drm.rs` and derives an initial frame interval from `mode.vrefresh()`:
  - `refresh_hz = mode.vrefresh().max(1) as f64`
  - `frame_interval = 1.0 / refresh_hz`
- `DrmPresentState` wraps `PresentPredictionState` from `native/emerge_skia/src/backend/present.rs`.
- `PresentPredictionState::observe_present/1` updates its estimated frame interval from the time between observed presentation callbacks/page flips.
- DRM only calls `observe_present` when a submitted primary page flip completes.
- The page-flip handler uses `Instant::now()` as `presented_at`; it does not use the DRM event timestamp/duration field.
- Stats display fps is the inverse of `predicted_next_present_at - presented_at`, recorded in `RendererStatsCollector::record_display_interval`.
- DRM animation pulses are sent only after primary page flip completion, with the same `presented_at` and `predicted_next_present_at` pair.

## Root cause

On DRM, page-flip events are presentation acknowledgements for submitted primary commits, not a continuous monitor-vblank stream. If the renderer submits primary frames every other vblank, or misses a vblank during load, the observed interval becomes about `33ms` on a `60Hz` display. The current estimator then treats that sparse commit cadence as the display interval.

Effects:

1. `display_fps` in stats can show achieved primary flip cadence (for example `30fps`) instead of the physical mode refresh (`60Hz`).
2. Animation prediction can double-count missed frames: after a sparse `33ms` observed primary interval, the next predicted present becomes `presented_at + 33ms` rather than `presented_at + 16.67ms`.

## Animation jitter/skip audit

Two additional DRM-specific sources were checked:

- Page-flip receive-time jitter: using `Instant::now()` when the event is handled can shift animation sample times if event delivery is delayed. The current local worktree keeps receive-time sampling and mode-interval prediction because DRM event timestamp/future-vblank prediction appeared less smooth during manual audit.
- Cursor-only atomic commits: a hardware-cursor-only commit can occupy the single in-flight DRM commit slot immediately after an animation pulse and make the follow-up primary miss a vblank. The current local worktree uses the short follow-up primary window rather than indefinite animation-primary blocking; cursor updates are still combined with primary commits when primary work is ready.

## Correction plan

1. Add a DRM-specific mode timing helper. **Done.**
   - Prefer an exact interval from the DRM mode timing fields when possible: `clock`, `htotal`, `vtotal`, plus scan/interlace flags if exposed cleanly.
   - Fall back to `mode.vrefresh().max(1)`.
   - Log both the mode refresh and computed frame interval.

2. Split DRM physical mode interval from observed primary flip cadence. **Done.**
   - Make `DrmPresentState` keep `mode_frame_interval` as the prediction source.
   - Do not feed sparse primary page-flip deltas back into the prediction interval for DRM.
   - Optionally keep observed primary intervals only for diagnostics/future `present_fps`, not for `display_fps` or animation prediction.
   - If observed cadence diagnostics are added, prefer deltas from DRM page-flip event timestamps over `Instant::now()` receive-time deltas.
   - Keep `presented_at` as event receive time locally pending target validation; the event timestamp/future-vblank variant was reverted during the cross-backend enter-animation audit.

3. Record DRM stats display interval from the physical mode interval. **Done.**
   - On each primary page flip, call `record_display_interval(mode_frame_interval)`.
   - Keep `frame_count` / renderer `fps` as achieved rendered-presented frames per stats window.
   - If a distinct achieved-present cadence is needed later, add a separate stat rather than overloading `display_fps`.

4. Keep animation pulses page-flip driven, but predict from mode interval. **Done.**
   - Continue sending `TreeMsg::AnimationPulse` only after primary page flip completion.
   - Use `predicted_next_present_at = presented_at + mode_frame_interval`.
   - This preserves backpressure while avoiding sparse-present interval drift.
   - Defer cursor-only commits only during the short follow-up primary window so cursor updates cannot steal the immediate next primary slot. **Done locally; target validation pending.**

5. Handle same-size refresh changes. **Done.**
   - The current hotplug check breaks only on connector/CRTC/dimensions changes.
   - Include mode timing in the comparison so `1024x600@50` to `1024x600@60` recreates the DRM session and updates the mode interval.

6. Tests. **Done for native unit coverage; hardware validation pending.**
   - Unit-test `DrmPresentState` with presents at `0ms`, then `33ms`, and assert the next prediction is `33ms + 16.67ms`, not `33ms + 33ms`.
   - Unit-test stats recording path or helper behavior so DRM records physical mode interval as display fps.
   - Unit-test mode comparison for same dimensions but different refresh/timing.
   - Unit-test cursor-only deferral during the short follow-up primary window.
   - The previous event timestamp conversion/fallback and delayed future-vblank prediction tests were removed with the local revert of that approach.

## Validation

- `cd native/emerge_skia && cargo test`
- `cd native/emerge_skia && cargo test --no-default-features --features drm`
- `mix test`
- On target hardware: enable renderer stats on DRM and confirm:
  - `DRM mode: 1024x600 @ 60Hz` in native logs
  - renderer stats `display` remains near `60 fps` even when rendered `fps` dips
  - animations do not slow down or overshoot after missed primary flips
