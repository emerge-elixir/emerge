# Bundled DRM input qualification

## Remaining acceptance

- Cross-build the complete FP3 NIF/firmware using the app's existing, unmodified
  system artifact. Do not add input packages to a nerves_system or rely on host
  headers/libraries. Inspect the final NIF for accidental libinput, libevdev,
  mtdev or libudev shared-library dependencies.
- Run an actual relocated release in minimal Linux/Nerves without system input
  libraries, a udev daemon, `/run/udev`, installed quirks or a hotplug helper.
  Test startup, capability classification, unplug/replug, late/unreadable nodes,
  device-path reuse and `/dev/input` directory recreation. FP3/eudev and desktop
  tests alone do not establish daemonless acceptance.
- On the phone, exercise fast two-thumb typing and two/four overlapping contacts
  in both release orders, same-key feedback, independent slide-off cancellation,
  holds/repeats, delayed registry/text echoes, keyboard remount/focus replacement,
  and ordinary taps, drag scrolling and inertia.
- Exercise input cancellation on unplug, display off/on and Stop under channel
  backpressure; test external keyboard/mouse discovery and hotplug.

Host initialization/link checks and software contact regressions are not substitutes
for these firmware/minimal-system/device gates. General FP3 renderer qualification
is closed; this is the new input migration's acceptance, not a repeat renderer soak.
User owns device testing/deployment; no assistant device access is authorized.

## References and boundaries

- [Native input build/runtime requirements](../native/emerge_input/README.md)
- [Input subscriptions and payloads](../guides/tutorials/handle_input.md)
- [Event and contact lifetimes](../guides/internals/events.md)
- [Build/artifact guidance](../guides/reference/native-renderer-builds.md)

Keep contact-local captures/timers and mouse/single-primary-touch compatibility.
Do not introduce general multitouch gestures, change scrolling physics or add
implicit touch-to-wake. Preserve unrelated Fairphone app/system changes.
