# Frame Latency Implementation Notes

Last updated: 2026-04-28.

Status: completed/reference. The active frame-latency plan was closed after the
Wayland pacing and animation-cadence fixes landed.

## Completed Context

Pipeline stats are split enough to diagnose frame latency:

- `pipeline submit->frame callback`
- `pipeline submit->tree`
- `pipeline tree`
- `pipeline render queue`
- `pipeline submit->swap`
- `pipeline swap->frame callback`

The first broad late-replacement attempt improved patch-to-visible latency, but
it also produced repeated slow `present submit` samples around a full refresh
interval. Narrowing that with fixed millisecond windows was not the right model
because it guessed around compositor timing instead of fixing the blocking
primitive.

## Wayland Model

Wayland clients should pace rendering from `wl_surface.frame` callbacks. For
EGL Wayland surfaces, `eglSwapBuffers` can still block if the EGL swap interval
is left at its default vsync behavior. That conflicts with manual
frame-callback pacing and was the likely source of full-interval
`present submit` stalls during late replacement.

Implemented behavior:

- keep one normal `wl_surface.frame` callback per presentation window
- set the Wayland EGL swap interval to `SwapInterval::DontWait` when supported
- allow late replacement only when disabling swap-interval pacing succeeded
- allow at most one patch-derived replacement while a frame callback is pending
- never late-replace animation-active scenes
- never request a second frame callback for a late replacement commit
- keep animation pulses and display interval estimates tied to normal
  callback-paced draws

If `SwapInterval::DontWait` is unsupported, Wayland falls back to strict
frame-callback pacing.

## Implemented Slice

Completed changes:

- `GlEnv::swap_buffers_nonblocking`
- Wayland EGL setup attempts
  `gl_surface.set_swap_interval(&gl_context, SwapInterval::DontWait)`
- `PresentState::draw_decision` gates late replacement on nonblocking swap
- fixed replacement windows and fixed slow-submit backoff were removed
- replacement is one-shot per pending callback
- animation-active render scenes are excluded from late replacement
- callback timestamps drive animation sample timing; idle callbacks are
  discarded if no draw is ready
- tests cover unsupported blocking-swap fallback and one-shot replacement

## Validation Result

Live `../emerge_demo` validation covered:

- showcase borders hover
- interaction typing
- todo add/filter/animate-exit
- app selector alpha animation

Observed result:

- app-selector enter/exit animation no longer gets stuck from late replacement
- animation sample times are anchored to Wayland frame callbacks rather than
  post-render `swap_buffers` completion
- no public frame-latency option or mode was added

Keep watching future traces for repeated full-frame `present submit` stalls,
`pipeline submit->swap` growth during patch bursts, animation-active scenes
entering late replacement, and backend-specific latency behavior outside
Wayland/EGL.

## Follow-Up Finding

The app-selector close flash was not fully explained by frame scheduling. The
remaining issue happened when a menu closed while its enter fade was active: the
exit ghost used the declared exit `from` keyframe instead of the current
rendered visual attrs. Exit ghosts now retarget their first exit keyframe to the
current visual attrs so interrupted enter/exit transitions stay continuous.

## Other Backends

DRM/KMS:

- do not implement late replacement while a page flip is in flight unless the
  backend explicitly supports replacing an in-flight commit
- coalesce to latest render state while waiting for page-flip completion
- add render, commit, and page-flip timing stats before changing behavior

Raster/offscreen:

- no external presentation callback exists
- keep latest-only coalescing and render timing

macOS/Metal:

- instrument drawable acquisition, command-buffer commit, and presentation
  callback timing before changing policy
- prefer limiting queued drawable depth over replacement behavior until traces
  prove otherwise
