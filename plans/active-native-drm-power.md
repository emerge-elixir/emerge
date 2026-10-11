# DRM display power — simple stop/recreate lifecycle

## Current contract

- Keep output/backlight ownership and requested settings across off/on.
- Off immediately ignores native UI input, suppresses the backlight and stops the
  presenter. Ignore queued render work. Do not save/restore GPU state.
- Keep the native tree actor and latest tree; the viewport can send a fresh tree.
- On creates a new presenter, renders the latest tree as its first application
  frame, and enables the managed backlight after presentation.
- Initial-off acquires ownership/routing without requiring tree upload.
- Detach KMS and finish OpenGL once at teardown before freeing driver-used buffers.
  Ordinary power cycles destroy resources; only unsafe cleanup retains ownership.
- Keep original requested brightness in the viewport-owned native baseline
  resource, including renderer recovery. Clean stop leaves the managed lamp dark.
- Explicit OpenGL and CPU raster support power/brightness. Vulkan and raster
  GPU-upload power, live connector/mode changes and DRM caps remain guarded.
- FP3 uses explicit OpenGL again. CPU raster was tested successfully by the user.
  OpenGL device qualification is the user's next step; no device access requested.

## Deleted machinery

Config-tree markers/BEAM acknowledgement state, native receipts/revocation,
receipt-bearing scenes, presentation proof mailboxes, power input epochs/kernel
backlog defenses, bespoke DRM commit-cookie UAPI, generic GL retirement/fault
model, and their associated regression suites. Use existing tree replay and
ordinary DRM page-flip APIs. No strict config/frame provenance guarantee.

## Validation

Run `cargo test`, CPU-only and combined presenter profiles, Clippy, Emerge/Fairphone
host tests and local quality checks. Tests do not operate host devices. Record
post-simplification results separately from earlier proof-heavy CPU checks.
Hardware timing, DCS behavior and physical flashes remain user qualification.

Vulkan acquisition synchronization repair remains separately deferred.

Post-simplification checks:
- Default Rust: 1,543 passed; CPU-only: 1,519 passed, 1 ignored; combined
  Wayland/DRM GL/Vulkan: 1,664 passed, 2 ignored. Each profile also passes
  the 1 + 13 fixture tests.
- Emerge: 632 passed (15 doctests, 617 tests), 13 hardware/full-sweep exclusions.
- Fairphone host: 37 passed (2 doctests, 35 tests).
- Combined all-target Clippy `-D warnings`, local quality, formatting, and both
  repositories' diff checks pass. Docs generated with 31 screenshots.
- No OpenGL power device result is claimed; the user runs the FP3 build/test.

## FP3 OpenGL startup hang report

User device logs show startup cleanup timing out followed by DSI-1 busy retries.
GL did an unconditional `receive_events()` before publishing resource readiness.
On a blocking fd that waits for a flip; a frame cannot be submitted until readiness
returns and the viewport uploads its tree. Stop cannot interrupt that read, leaving
cleanup and lease release pending. Replace it with a zero-time poll: discard only
already-readable events. Keep stop/recreate unchanged; do not extend timeouts or
release ownership while the presenter is still using it. A private blocking socket
test covers empty and queued reads without opening a DRM device. Device diagnosis
is consistent with this defect; corrected OpenGL device operation remains unverified.

Startup-read fix validation: focused blocking-socket test passes; default Rust
1,543, CPU-only 1,520 (1 ignored), combined presenters 1,665 (2 ignored), plus
fixture suites. Emerge 632 passed (13 excluded); combined all-target Clippy and
local quality pass. No FP3 hardware run is claimed. Rebuild/reboot is necessary
for the user's already-stuck native presenter; ordinary retries cannot free its
still-held lease.

## Commit sequence

1. Bound upload/patch queue admission so native calls do not wait for tree work.
2. Preserve captured backlight baselines and avoid redundant post-detach writes.
3. Integrate retained DRM ownership, fresh presenters, input suspension, native
   control completion and viewport-owned baseline resources as one coherent unit.
4. Expose supported power/brightness controls and acquire initially-off viewports
   without rendering/uploading; include public and lifecycle regressions.
5. Document the simple lifecycle, supported boundaries and user qualification.
6. Clarify diagnostic collection versus logging, backend limitations and GPU waits;
   cover the collection/logging matrix with headless integration tests.

Keep Fairphone changes separate. Leave the unrelated freedreno investigation and
`prime_output_bug.png` uncommitted. Validate commit snapshots before moving on.

## Post-rebase commit checks

Rebased onto `main` at `6b81166`. Each implementation commit was checked in an
isolated worktree with `cargo test` and `mix test`. At the public-control commit
`fc64fe2`, default Rust has 1,580 passing tests, CPU-only has 1,557 passing and one
ignored, and combined Wayland/DRM GL/Vulkan has 1,702 passing and two ignored.
Each Rust profile also passes the 1 + 13 fixture tests. Emerge has 639 passing
checks (15 doctests, 624 tests), with 13 hardware/full-sweep exclusions.
Combined all-target Clippy with `-D warnings` and Rust formatting pass.

These are software checks only. No firmware build, deployment or corrected
OpenGL device qualification occurred during the commit split.
