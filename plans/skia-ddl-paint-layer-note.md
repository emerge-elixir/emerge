# Parallel paint recording research

Evaluate only if profiling shows cold/dirty paint recording is a material CPU
bottleneck after retained payload reuse. This is optional research, not a release
gate or a reason to redesign the existing ordered cache/composition path.

- Prototype independent immutable `SkPictureRecorder`/`SkPicture` recording for
  dirty layers, with final replay/composition on the render owner thread.
- Consider Ganesh Deferred Display Lists only if the simpler recording experiment
  proves insufficient and the pinned Skia API/deployment constraints support it.
  DDL targets CPU-side preparation, not parallel final drawing; Graphite is separate.
- Never draw into one live `SkCanvas`/`SkSurface` concurrently. Preserve exact
  paint/clip/alpha order, backend ownership and bounded work/storage.
- Compare full-frame CPU/GPU cost and exact pixels under the performance lock;
  remove experiments that do not establish a material gain.

Camera GPU headroom is tracked separately in [RPi5 Camera](active-rpi5-camera-60fps.md).
