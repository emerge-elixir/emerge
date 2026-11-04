# DRM boot-splash handoff

Status: implemented and locally validated. A user has confirmed the handoff works
on hardware; the device, renderer and boot conditions were not recorded.
Proposal baseline: `/workspace/emerge`, commit `93f3f9f`, version 0.5.1.
Implementation branch: `feat/drm-boot-splash-handoff`, based on `abf3159`.
Worktree: `/workspace/emerge-drm-boot-splash-handoff`.

## Implementation checklist

- [x] Retain initial-on scanout/backlight through resource preparation and the existing application-frame gate.
- [x] Inspect inherited routing/timings/planes; share tested first transactions across GL, CPU raster and Vulkan.
- [x] Test compatible primary/cursor replacement without modeset; block the first real commit and retain presenter completion proofs.
- [x] Defer modeset suppression until the replacement frame is ready, with a separate DCS acknowledgement from retirement.
- [x] Adopt readback-supported lit backlights without power cycling; defer desired brightness until presentation.
- [x] Reserve inherited cursor planes even for software cursors; reject routing that would disturb an unowned sibling.
- [x] Deterministic native tests and strict all-target Clippy for DRM/OpenGL, CPU DRM, DRM/Vulkan and combined DRM APIs.
- [x] Source-built Mix feature-matrix runs and full local CI quality/test/Dialyzer gates.
- [ ] Record hardware confirmation details and complete the relevant cold/warm boot,
  operation-trace and optical checks for each target.

No public configuration, NIF, asset or native-artifact profile changes are needed.
The opt-in `test/emerge_skia/drm_boot_splash_hardware_test.exs` requires an already-active
inherited splash and an explicitly selected exact mode. It is excluded from ordinary
CI and has not been run. The implementation also handles off/stop before any upload:
CPU raster explicitly detaches inherited scanout, and all presenters retire inherited
cursors even when no application cursor framebuffer was created.

## Local validation results

- `EMERGE_SKIA_BUILD=true ./ci-tests.sh all`: passed, including formatting,
  warnings-as-errors compilation, strict Credo/Clippy, 700 ExUnit tests/doctests
  with full-sweep coverage, native tests and Dialyzer (zero errors).
- `cargo test --release --no-default-features` with `drm,opengl`, `drm`,
  `drm,vulkan,drm-vulkan-deps` and `drm,opengl,vulkan,drm-vulkan-deps`: passed
  (respectively 1659, 1623, 1683 and 1718 native unit tests, plus integration tests).
  Strict all-target release Clippy passed for each profile.
- Source-built `mix test --warnings-as-errors`: passed for DRM/OpenGL (694),
  CPU DRM (694, one skip) and DRM/Vulkan (695), with hardware
  tests excluded. Profiles used temporary external configs and isolated Mix build
  paths; no project configuration was added. The default profile was restored
  and its 694-test ordinary suite passed again.
- `cargo fmt --check`, `mix format --check-formatted` and `git diff --check`: passed.
- The local validation above did not acquire DRM master or test a panel/backlight.
  A user subsequently confirmed the handoff works on hardware, but did not specify
  the device, renderer, boot conditions or measurements. No native artifacts were
  published as part of this work; normal release delivery remains outstanding.

## Goal

Keep Loon's existing kernel-fbdev splash visible while Emerge prepares EGL,
Skia, layout and the first application frame, then replace it directly with that
frame. Do not redraw/copy the logo or change fb_blaster, U-Boot, or Linux.

## Findings at the proposal baseline

These findings describe the code before implementation, not the current behavior.

- `native/emerge_skia/src/backend/drm/power.rs:295-320` acquires the output,
  binds an optional backlight, **darkens it and unconditionally detaches inherited
  scanout before starting the presenter**. `detach()` clears plane FB/CRTC IDs,
  disconnects the connector, sets CRTC `ACTIVE=false` and clears `MODE_ID`.
  This guarantees a blank interval even when backlight management is disabled.
- `backend/drm/gl.rs:3257-3401` already acknowledges resource readiness before
  waiting for an explicit application upload, renders that frame, then commits
  it. `backend/drm/first_frame.rs` rejects startup/stale scenes using the existing
  request barrier. Do not add another first-frame protocol.
- The initial GL commit always sets connector routing, `MODE_ID` and `ACTIVE`
  with `ALLOW_MODESET`. After eliminating initial detach, avoiding an unnecessary
  modeset is the next requirement for a genuinely seamless transition.
- `backend/backlight/mod.rs:320` currently requires a previously darkened device
  before presentation-gated illumination. Skipping only the startup darken/detach
  calls would leave this state machine inconsistent.
- The inspected `power.rs`, `gl.rs`, `first_frame.rs` and `session.rs` are
  byte-identical to the firmware's `deps/emerge` source. This does not identify
  or measure the device's loaded native binary.
- There is no current configuration-only solution: `Backlight{device: nil}`
  avoids brightness writes but not the unconditional KMS detach.

## Proposed behavior — no new API

Preservation is the only normal startup behavior for DRM outputs. There is no
startup setting, opt-in flag, or blank-start alternative. For initial
`display: :on`, leave existing scanout and backlight state untouched until the
first application frame can replace them. An inactive output has nothing to
preserve and is enabled with that first frame, not a placeholder.

Apply this invariant across DRM presenters, with Loon/OpenGL as the first
hardware qualification target. Existing `display: :off`, runtime off/on, stop
and failure cleanup remain explicit reasons to suppress or detach an output;
they are not competing startup policies.

A genuinely different mode may require a hardware blank during the final
transition. That is distinct from unnecessarily blanking the screen throughout
renderer initialization, and must not become an early-blank fallback.

## Proposed implementation

1. **Inspect inherited output state.** After leasing the selected output, read
   its existing active CRTC, connector routing, exact mode timings and
   primary-plane state without writing them. Determine whether the first frame
   can use a no-modeset transaction. Do not ignore explicit connector/mode
   requests, disturb another output, or detach an active output merely because
   its mode differs from the requested one.
2. **Prepare without blanking.** For initial-on startup, retain existing scanout
   and current backlight state while preparing the replacement, including when
   the eventual transition needs a modeset. Reserve/read the managed backlight
   and defer requested brightness changes. Initialize the renderer offscreen
   through the existing startup/resource-ready handshake. Do not postpone the
   NIF start response until presentation: the caller needs its renderer handle
   before it can upload the first tree.
3. **Reuse the application-frame gate.** Render the matching application frame
   into Emerge-owned GBM buffers, without installing a placeholder. Intentional
   empty application frames remain valid; do not guess readiness from pixels,
   timers, asset counts or a nonzero render version.
4. **Replace scanout without resetting the panel.** For matching active routing
   and mode, build a primary-plane update without rewriting connector routing,
   `MODE_ID` or `ACTIVE`. Validate with `TEST_ONLY` without `ALLOW_MODESET`, then
   perform a blocking first commit and reuse the existing presentation signal.
   Retire inherited cursor state in that same transaction if necessary. If a
   modeset is genuinely required, perform its necessary suppression/transition
   only once the replacement application frame is ready, preserving DCS ordering.
   Report that transition; no optical seamlessness promise applies to it. Keep
   each presenter's existing completion proof (blocking commit or page-flip event).
5. **Adopt the lit backlight after presentation.** Add an explicit transition
   from inherited/untouched to application-visible that does not power-cycle or
   zero brightness. Admit lit-state adoption only when managed backlight readback
   supports it; an already-dark backlight uses ordinary illumination instead.
   Apply desired brightness only after first presentation. Keep dark-to-visible
   illumination for already-dark outputs and explicit off/on.
6. **Preserve lifecycle safety.** Input remains gated until application
   presentation. Stop, startup failure, off-before-upload, teardown, DCS ordering,
   buffer retirement and quarantine retain their current safety behavior. Never
   destroy an inherited framebuffer Emerge did not create. Preservation is not
   restoration on shutdown and is not a GPU resource-recovery mechanism.

Main changes would be `backend/drm/power.rs`, the presenters' initial commit
paths (including `backend/drm/gl.rs`), a small compatible-state helper, and the
backlight phase transition. Reuse each presenter's application-frame gate and
presentation proof. No public configuration/schema change, new NIF, scheduler
strategy, daemon, logo asset, framebuffer copy, system library or Linux/U-Boot
change is needed.

## Validation checklist

- Deterministic tests across DRM presenters: active/inactive outputs; matching
  and changed modes; delayed/no first upload; stale and intentional empty frames;
  no disable/zero-brightness writes during initial-on preparation; lit-state
  adoption without power cycling; required modesets delayed until a replacement
  frame is ready; initial-off, cancellation, errors, off/on and sibling isolation.
- Run `cargo test` and `mix test`, including the DRM/OpenGL feature combination
  (`--no-default-features --features drm,opengl`), CPU DRM raster and Vulkan DRM,
  plus normal quality gates. Update startup expectations/documentation to the
  unconditional preservation behavior; keep explicit-off and cleanup guarantees.
- User-authorized Loon cold/warm boot: delay the first upload deliberately and
  verify the logo remains visible until the UI replaces it. Observe KMS/backlight
  operations and the panel optically; application captures cannot measure a
  CRTC-disabled interval. Check no panel power-cycle during a compatible handoff.
- Optical behavior depends on the display hardware and driver. Record the device,
  renderer and boot conditions for each hardware result; source inspection and
  application captures alone cannot establish an optical no-flash guarantee.

Deliver through Emerge's normal native-artifact release/build flow. Loon receives
the improved startup behavior by updating Emerge, with no viewport configuration
change. Do not patch the application's generated `deps/` checkout.
