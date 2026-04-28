# Active Frame Latency Plan

Last updated: 2026-04-28.

Status: active. This plan tracks the frame-latency work after live Wayland
traces showed that the fixed late-replacement window was the wrong solution.

## Completed Context

Pipeline stats are already split enough to diagnose the problem:

- `pipeline submit->frame callback`
- `pipeline submit->tree`
- `pipeline tree`
- `pipeline render queue`
- `pipeline submit->swap`
- `pipeline swap->frame callback`

The first broad late-replacement attempt improved end-to-end latency, but it
also produced repeated slow `present submit` samples around a full refresh
interval. Narrowing that with a fixed 1-4 ms window and a fixed 4 ms backoff was
not the correct model: it guessed around the compositor instead of fixing the
blocking primitive.

## Investigation Result

Wayland clients should use frame callbacks for pacing. A client requests a
callback when it commits a frame, then treats the callback as permission to draw
the next normally paced frame.

For EGL Wayland surfaces, `eglSwapBuffers` can still block if the EGL swap
interval is left at the default vsync behavior. That conflicts with manual
frame-callback pacing and explains the slow `present submit` logs: Emerge was
trying to do a late replacement, but the EGL swap itself could wait for the
display interval.

Sources checked:

- Wayland Book
  (`https://wayland-book.com/surfaces-in-depth/frame-callbacks.html`): frame
  callbacks are the client pacing mechanism and should be requested when
  committing a surface.
- glutin `SwapInterval`
  (`https://docs.rs/glutin/latest/glutin/surface/enum.SwapInterval.html`): EGL
  defaults are platform dependent, Wayland clients should not use `Wait` for
  pacing, and `DontWait` makes `swap_buffers` nonblocking.
- Firefox Wayland bug 1548499
  (`https://bugzilla.mozilla.org/show_bug.cgi?id=1548499`): Firefox manages
  frame callbacks manually and sets the Wayland EGL swap interval to zero.
- GTK Wayland EGL commit
  (`https://mail.gnome.org/archives/commits-list/2017-January/msg01642.html`):
  GTK disables EGL swap interval because its frame clock already paces rendering
  to output vsync.

## Correct Model

Do not use a fixed millisecond deadline for late replacement.

The Wayland backend should:

- keep one normal `wl_surface.frame` callback per presentation window
- disable EGL swap interval with `SwapInterval::DontWait`
- allow late replacement only when disabling swap interval succeeded
- allow at most one patch-derived replacement while a frame callback is pending
- never late-replace animation-active scenes, including patch scenes that start
  or carry enter/exit animations
- never request a second frame callback for a late replacement commit
- keep animation pulses and display interval estimates tied to normal
  callback-paced draws

If `SwapInterval::DontWait` fails, late replacement is disabled and the backend
falls back to strict frame-callback pacing. That keeps correctness and avoids
the present-submit stalls shown in live traces.

## Current Implementation Slice

Status: implemented in the working tree; awaiting live Wayland validation.

- Add `GlEnv::swap_buffers_nonblocking`.
- On Wayland EGL setup, call
  `gl_surface.set_swap_interval(&gl_context, SwapInterval::DontWait)`.
- Gate `PresentState::draw_decision` late replacement on
  `swap_buffers_nonblocking`.
- Remove the fixed 1-4 ms replacement window.
- Remove fixed 4 ms slow-submit backoff.
- Keep one-shot replacement per pending callback.
- Exclude animation-active render scenes from late replacement after app
  selector traces showed that replacement commits could make fade/translate
  animation cadence visibly jittery.
- Keep unit tests for unsupported blocking-swap fallback and one-shot
  replacement behavior.

## Validation Requirements

After implementation, collect the same `../emerge_demo` traces that exposed the
bug:

- showcase borders hover
- interaction typing
- todo add/filter/animate-exit
- app selector alpha animation

Acceptance:

- `present submit` should no longer show repeated full-frame stalls from late
  replacement.
- `pipeline submit->frame callback` should keep the improvement seen when late
  replacement works.
- `pipeline submit->swap` should stay low for patch bursts.
- render, draw, flush, GPU flush, submit, and cache stats must not regress.
- no public frame-latency option or mode should be added.

If the backend still reports slow `present submit` after swap interval is
disabled, investigate compositor/driver behavior before adding policy. The next
step would be better instrumentation, not another fixed timing guess.

## Follow-Up Finding

App selector close flashes were not fully explained by frame scheduling. The
remaining repro happens when the menu closes while its enter fade is still
active: the exit ghost used the declared exit `from` keyframe (`alpha: 1.0`)
instead of the menu's current effective alpha. The correct model is for exit
ghosts to retarget their first exit keyframe to the current rendered visual
attrs, so interrupted enter/exit/interaction transitions stay continuous.

The remaining smooth-then-jittery behavior points back to animation cadence.
Wayland animation timing should be driven by frame callbacks, not by
post-render `swap_buffers` completion. Rendering and cache preparation can vary
slightly per frame; if that variance feeds the next animation sample time, short
100 ms fades visibly wobble even when the compositor cadence is regular. The
Wayland present state now treats callback timestamps as one-shot timing for the
next normal draw and discards them if no draw is ready, so idle callbacks cannot
pollute later animation-starting patches.

## Other Backends

DRM/KMS:

- Do not implement late replacement while a page flip is in flight unless the
  backend has explicit support for replacing an in-flight commit.
- Coalesce to latest render state while waiting for page-flip completion.
- Add matching stats around render, commit, and page-flip event timing before
  changing behavior.

Raster/offscreen:

- No external presentation callback exists, so no late replacement path is
  needed.
- Keep latest-only coalescing and render timing.

macOS/Metal:

- Instrument drawable acquisition, command buffer commit, and presentation
  callback timing before changing policy.
- Prefer limiting queued drawable depth over adding replacement behavior until
  local traces prove otherwise.
