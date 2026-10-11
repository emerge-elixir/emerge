# Fairphone display controls

- Direct fixed FP3 config in `UI.Display`: `/dev/dri/card0`, `DSI-1`, explicit
  OpenGL, `/sys/class/backlight/1a94000.dsi.0`, Inter Emoji.
- Solve `config/1` stays subscribed while off. Settings owns the direct brightness
  slider; Solve controller owns a linked, non-grabbing KEY_POWER reader.
- No separate power GenServer, app-side sysfs writes, config abstraction, fallback,
  or extra knobs. Local source dependency; initial config validated by host tests.
- Native off destroys presenter resources and ignores UI input. On creates a
  fresh presenter and renders the latest retained tree. No GPU restoration or
  config/frame proof machinery.
- CPU raster worked in the user's device test. OpenGL is restored for the user's
  next build/test, following the consumer README. Software checks are not hardware
  qualification. Vulkan power remains guarded.
- User owns device testing/deployment. No assistant device access. Existing target
  build setup requires the user's configured SSH public keys; do not generate keys
  or change authorization.

Validation results: `active-native-drm-power.md`.
