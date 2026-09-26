# DRM implementation progress

Implementation authorized. This file supersedes earlier proposed public-supervisor
and application-supervision topologies in the design/audit documents.

## Implemented

- Discovery, exact advertised mode IDs and explicit connector selection.
- One master per device identity, independent per-output lease/event FDs, atomic
  connector/encoder/CRTC/plane reservations, explicit safe lease revocation.
- Native owner monitoring before startup; essential-worker exit monitoring; persistent
  session health/completion; idempotent concurrent stop and bounded waits; startup
  deadlines and retained asynchronous rollback. Failed/uncertain output cleanup keeps
  its reservation; Vulkan process-global terminal quarantine is unchanged.
- Public callback PID, names, direct calls/messages, callback execution context, reload
  membership and video entry points remain intact. Private per-viewport supervision
  manages renderer replacement, full reupload, generation relays and endpoint ownership.
- **No Emerge application callback, auto-started global supervisor or global endpoint
  service.** Applications explicitly supervise viewport children with their chosen strategy.
- Callback death ends that public instance. Applications decide callback replacement;
  native renderer replacement alone preserves callback state. Pending cleanup never
  authorizes reuse. Retry attempts are bounded and back off.
- Headless PRIME uses the explicit producer rather than the startup process, and reports
  completion only after lease-owner, native and release-dispatcher drain.
- Relay retirement acknowledgements drain queued native close messages before recovery;
  a close arriving during cleanup still reaches the callback and honors its override.
- Process-global GL dispatch is initialized once with EGL pinned, avoiding global-table
  rewrites while sibling contexts render or restart.
- Added API/regression tests and `guides/internals/drm-outputs-and-lifecycle.md`.

## Final validation

- `EMERGE_SKIA_BUILD=1 ./ci-tests.sh all` passed: format, warnings-as-errors compile,
  strict Credo, Clippy, full-sweep ExUnit (542 tests/doctests, four opt-in hardware tests
  excluded), default Rust tests (1475 unit + 14 integration tests), and Dialyzer.
- `cargo test --release --features drm-all --lib`: 1575 passed, one opt-in hardware test ignored.
- DRM-all Clippy with warnings denied passed.
- Default, DRM-all, OpenGL DRM-only, Vulkan-only DRM, and no-default/no-GPU builds checked.
- The final code was retested on both real outputs with OpenGL and Vulkan; both passed.
  The default configured NIF artifact was restored after these opt-in tests.
- Real AMD integrated GPU `/dev/dri/by-path/pci-0000:6c:00.0-card`: `DP-4` (3840×2160)
  and `DP-5` (1920×1080), both connected. **Discrete GPU card1 was not modeset.**
- Independent lease acquisition/exclusion/reacquisition passed on both outputs.
- Actual two-viewport **OpenGL and Vulkan** tests both passed: initial presentation,
  duplicate rejection, three stop/recovery cycles for A while B keeps presenting,
  callback A `:kill` with its handle retained, and new A with B unaffected.
- Hardware testing caught Mesa retaining A's lease FD in shared GPU state. Explicit
  lease revocation after safe retirement fixes restart without stopping B. Added a
  retained-FD regression to the native opt-in lease test.

## Remaining limitations / validation boundaries

- GPU hangs are not detected by the worker-exit monitor. Pending cleanup retains the
  session/reservation; it never starts a replacement over uncertain owners.
- GPU device-loss, hot-unplug, and presenter-panic fault injection have not been
  validated. Vulkan terminal uncertainty still requires a cold VM restart.
- Startup is deadline-bounded but runs synchronously in the private lifecycle worker;
  callback/native owner death can cancel it independently. No separate public prepare/
  cancel handshake or card-incarnation recovery protocol was added.
- macOS host transport does not implement the new native owner/status/bounded-stop
  contract. Custom renderer adapters retain their old PID and liveness contract.
- No input-device routing policy was added; output selection is not input isolation.
