# DRM libinput migration

## Decision and acceptance requirement

Use libinput for the DRM backend instead of extending the hand-written evdev
multitouch decoder. The default DRM input build must be self-contained: no
libinput, libevdev, mtdev or libudev package, udev daemon, udev rules installation,
or libinput-specific nerves_system rebuild is required from the application.

Download pinned, checksum-verified upstream archives at build time rather than
checking their source trees into Emerge. Bundle libinput, libevdev, mtdev and
libudev-zero into the NIF as position-independent static libraries, plus
libinput's runtime quirks data.
Use a libinput path context and Emerge-owned device discovery/hotplug, not a
udev context that assumes a system device manager. System linking may be an
explicit alternative; it must not be the default or an automatic host-dependent
choice.

This removes additional userspace input-package requirements, not the baseline
need for Linux input drivers, readable /dev/input nodes and /sys, or the renderer's
existing DRM/runtime dependencies.

Emerge still owns hit testing, virtual-key activation, scroll physics and
contact-local key lifetimes. Bundling libinput does not fix the shared key tracker
by itself. Keep the tested scrolling implementation working until its replacement
is validated.

## Implementation status

Implemented in the library's DRM input backend and shared event runtime,
including drag-inertia routing to the scrolling container. The implementation is
committed in buildable steps; see [the commit sequence](active-input-commit-sequence.md)
for references and isolated staged-snapshot validation.

Based on `main` at `544de0f`. The build commit now downloads pinned sources
instead of introducing upstream source trees. DRM-raster tests pass (1,573 unit
tests, one ignored, plus 1 + 13 fixtures); bundled input has nine passing tests.
Strict Clippy and the standalone source-package ExUnit check pass. Full Mix tests
remain blocked by missing dependencies.

- Private `native/emerge_input` crate builds pinned PIC-static C sources with
  Cargo's target toolchain. It uses an owning Rust wrapper/narrow C shim rather
  than input-sys, avoiding its dynamic link declarations entirely.
- Pinned source archives are downloaded into a verified build cache. Explicit
  offline builds use pre-populated archives; the Hex source package retains only
  integration code, checked patches, source pins and library license notices.
- Quirks from the downloaded libinput archive are embedded as immutable data;
  there is no deployed absolute path or extraction/environment-mutation step.
- DRM uses path-context discovery, inotify plus 500 ms reconciliation/retries,
  device identity checks, cancellation, pointer unit conversion and owner-thread
  keyboard repeat. Native device/context references never leave the input owner.
- Per-contact virtual-key captures/deadlines, stale FIFO replay, pressed feedback,
  focus/mount cleanup and single-primary-touch compatibility are integrated.
- Explicit `:touch` observers, schema/masks, documentation and package closure
  checks are included. Mouse observers keep their original payloads.
- Host NIF ELF dependencies have no input libraries; bundled native symbols are
  private. The probe cross-builds against the unmodified ARMv7 RPi3 SDK and an
  AArch64 Nerves toolchain's libc-only sysroot (no input headers/libraries), with
  only libc/libm/libgcc/loader dependencies. Host context/quirks tests also run
  without a udev service or `/run/udev` database.
- Native tests and strict Clippy pass; focused Elixir observer/package/language
  tests pass using existing compiled modules. Normal Mix tests remain blocked by
  missing Hex (Emerge) and nerves_bootstrap (app).

Still required: complete FP3 NIF/firmware cross-build, actual minimal-system input
and discovery/hotplug/release-relocation tests, all presenter build profiles and
physical phone typing/scrolling/display/power checks. No real input/uinput devices
or QEMU are available here; initialization and cross-link checks do not substitute
for those hardware acceptance tests. Explicit system-linking/device-override APIs
are not added in this implementation.

## Initial findings (before implementation)

- The former `drm_input.rs` folded ABS_X/ABS_MT_POSITION_X and ABS_Y/ABS_MT_POSITION_Y
  into one BTN_TOUCH-driven cursor, without multitouch slot identity.
- `RuntimeOverlayState.virtual_key` and its timer are single mouse captures.
  Replacing the backend alone will not fix overlapping virtual-key taps.
- FP3 explicitly enables dynamic eudev device creation, but that is not a valid
  requirement for general Nerves support. Neither the system SDK nor the target
  root filesystem may be assumed to contain libudev.
- This host has libinput 1.32.0 and libudev; a successful host build is therefore
  insufficient evidence that bundling works.
- The Rust `input`/`input-sys` bindings do not provide an automatic bundled build.
  Their native link declarations must be coordinated with the static archives,
  rather than accidentally restoring a dynamic libinput dependency.
- libudev-zero supports static linking, needs only libc at runtime and derives
  ID_INPUT properties from kernel/sysfs capabilities. It does not reproduce a
  complete udev rule/hwdb installation. Its udev-monitor hotplug needs a device
  manager/helper; the default Emerge path must not rely on that monitor.
- Unfinished evdev-specific code was removed from the active build and saved in
  `/tmp/emerge-touch-evdev-draft-ITzYjZ`; it is not the implementation to deploy.

## Implementation boundaries

### 1. Self-contained native build

- Gate the bundle behind DRM support, with bundling selected by default for every
  DRM presenter profile. Do not add these libraries to Wayland, macOS or
  headless-only builds.
- Pin upstream versions and source checksums, retain license notices, and
  integrate the C build/link steps with Emerge's existing Cargo/Rustler and Nerves
  cross-compilation setup. Never pick host headers/libraries when targeting the
  Nerves SDK.
- Compile the complete input dependency closure with PIC and keep native symbols
  private to the NIF; do not interpose another NIF's or the process's libudev.
- Disable unneeded optional features such as GUI tools, libwacom, Lua plugins,
  upstream tests and documentation. A daemon or target-side build tool must not
  be installed or spawned to make this backend work.
- Embed quirks data from the pinned libinput archive into the NIF. Do not hard-code
  a build-machine path or depend on /usr/share/libinput being present in the
  system image. Preserve explicit upstream overrides without Emerge mutating
  process-wide environment variables.
- Build-time automation belongs in Rust build scripts or Mix/Elixir, not new
  Python scripts. Document any upstream host build-tool requirements separately
  from target-runtime requirements.
- Preserve the app's current nerves_system_fp3 dependency/artifact. Do not edit
  any nerves_system or app dependency cache as part of bundling these libraries.

### 2. Daemonless discovery and device classification

- Create a libinput path context with restricted opens on the existing input
  thread and enumerate /dev/input/event* at startup. libudev-zero supplies the
  libudev API used internally by libinput, without a system libudev.so.
- Discover add/remove/change events directly, for example with inotify on
  /dev/input. Treat notifications as a request to reconcile the actual device
  list, not as proof that a device is ready or that its pathname still identifies
  the same device.
- Retain a bounded rescan/retry fallback for permission/device-initialization
  races, watch overflow, missing/recreated /dev/input and missed notifications.
  No rebroadcast helper, udev socket or /run/udev database may be required.
- Validate libudev-zero's computed ID_INPUT/ID_INPUT_* against kernel capability
  fixtures and real hardware. Cover INPUT_PROP_DIRECT, MT-only touchscreens,
  touchpads, relative/absolute mice and keyboards; patch or upstream compatibility
  fixes if needed. Do not pretend every ABS device is a touchscreen.
- Test classification with no externally populated udev properties. For hardware
  that genuinely needs an override, expose explicit device configuration rather
  than requiring the app to install a system-wide udev rule.
- Remove devices and cancel their active contacts before a replacement device at
  the same event path can participate. Retry devices that are not yet ready
  without busy loops or unbounded logs.

### 3. DRM event adapter

- Use an owning Rust wrapper and narrow C shim without dynamic native dependencies.
- Keep input dispatch on its existing native thread and poll libinput's FD
  alongside discovery and the existing Stop/output-control wake FD. Do not put
  device opens, dispatch or blocking channel waits on a BEAM scheduler.
- Preserve ordered delivery, stop responsiveness, cursor visibility, screen-size
  updates, output-off input gating, physical button policy and device cleanup.
  Do not replace backlight wake policy with implicit touch-to-wake.
- Translate normalized pointer motion/buttons/axes to the existing InputEvent
  contract; preserve line/precise scrolling units rather than changing physics.
- Translate touch down/motion/up/cancel/frame with seat-slot identity and a fresh
  contact generation, retaining each contact's own position. Slot reuse and
  device removal must not release or activate a replacement contact.
- libinput does not translate keyboard keycodes into text. Preserve canonical
  key routing and current keyboard translation initially; decide xkbcommon/compose
  support explicitly rather than claiming libinput provides it.

### 4. Independent virtual-key contacts

- Carry contact identity through event buffering/replay; never merge movement
  or release events from different contacts.
- Store virtual-key captures and hold/repeat deadlines per contact/key, not in
  the one mouse tracker. Release commits only that contact's armed tap.
- A second key down, another finger moving or releasing, and unrelated
  controller rerenders must not cancel the first key.
- A contact moving outside its own key can still cancel that contact, matching
  existing slide-off behavior. Keep pressed feedback active while another contact
  still holds the same key.
- Cancel safely on touch cancel, device removal, blur, input gating, key removal,
  key remount or focus replacement. Never deliver a stale key to a replacement
  field or keyboard layer.
- Keep existing mouse behavior and single-primary-touch scrolling/button behavior
  for non-key UI. General multitouch scrolling/pinch/swipe APIs are not part of
  this keyboard fix.
- Keep raw input observer compatibility explicit: any new public touch event or
  subscription needs corresponding schema, protocol, documentation and tests.

## Validation and rollout

1. Build against an unmodified Nerves SDK without libinput/libudev headers or
   libraries. Inspect the NIF's ELF dependencies: no libinput, libevdev, mtdev or
   libudev shared-library requirement may remain.
2. Run in a minimal Linux/Nerves environment without libudev, a udev daemon,
   /run/udev database, system libinput quirks or a hotplug helper. Verify startup,
   device classification, unplug/replug, late device nodes and directory recreation.
   FP3/eudev and desktop systems are additional compatibility tests, not proof of
   this acceptance requirement by themselves.
3. Unit-test translation independently of hardware: contact identity, frames,
   cancellation, slot reuse, pointer units and keyboard routing.
4. Test two/four overlapping virtual-key contacts in both release orders,
   same-key contacts, independent slide-off cancellation and hold/repeat timers.
5. Exercise delayed registry receipts, text echoes, layer changes, remounts,
   output off/on, device unplug and stop under channel backpressure.
6. Run cargo test and Mix tests; compile all affected backend feature profiles
   and cross-build using the app's existing FP3 system artifact. Verify relocation
   of bundled data in an actual release, not only a source checkout.
7. On the phone, verify fast two-thumb typing, drag scrolling and inertia,
   brightness, power-off/on, external keyboard/mouse and device hotplug.
8. Keep bundling, backend migration and virtual-key runtime changes as distinct
   commits so failures can be attributed and rolled back independently. No
   libinput-specific nerves_system commit should be necessary.

## References

- https://docs.rs/input/latest/input/
- https://github.com/illiliti/libudev-zero
- https://wayland.freedesktop.org/libinput/doc/latest/architecture.html
- https://wayland.freedesktop.org/libinput/doc/latest/device-configuration-via-udev.html
- https://wayland.freedesktop.org/libinput/doc/latest/api/group__event__touch.html
- [Independent virtual-key touches](active-independent-virtual-key-touches.md)
