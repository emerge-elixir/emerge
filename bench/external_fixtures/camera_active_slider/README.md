# Camera active-shutter-slider fixture

These eight EMRG trees come from the real `Camera.UI` at the rotated RPi5 display
size. Phases change the requested and applied shutter values while preserving the
exact nine semantic paint layers and direct Video node. The renderer benchmark
activates the shutter slider's focus interaction style for phases 1-7.

Regenerate from the sibling Camera repository:

```bash
cd /workspace/colibri/camera
MIX_ENV=test mix run \
  ../../emerge-headless/bench/external_fixtures/camera_active_slider/generate.exs
```

The renderer Criterion case decodes and lays out every phase at 1440x2560, warms
the payload cache, finishes asynchronous setup work, cycles phases 1-7, and calls
`glFinish()` per sample so results include GPU completion rather than command
enqueue alone.
