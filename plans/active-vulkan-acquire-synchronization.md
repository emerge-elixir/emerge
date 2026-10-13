# Wayland Vulkan acquired-image synchronization

Status: **implemented; software CI and targeted Hyprland/RADV hardware gate pass**.
Interactive resize/occlusion, additional devices and performance measurements remain open.

- Worktree: `/workspace/emerge-wayland-vulkan-sync`
- Branch: `plan/wayland-vulkan-sync`, from `main` at `c641a9d`
- Scope: Wayland swapchain acquisition and its Ganesh handoff, not frame pacing,
  a public option, or a general Vulkan/renderer rewrite.

## Evidence and diagnosis

On `853d951`, Wayland Vulkan on Hyprland / RADV RX 7900 XTX passed
`test/emerge_skia/wayland_frame_rate_test.exs`, but synchronization validation
reported **4 errors, 0 warnings**: `SYNC-HAZARD-WRITE-AFTER-READ` between image
acquisition and Skia's initial layout transition. The originating commit was not
bisected; do not attribute this to pacing. See [the qualification record](active-linux-gpu-qualification.md).
The historical logs were unavailable here. Fresh baseline/fixed hardware evidence
is recorded below; attribution to a particular introducing commit remains unbisected.

The relevant code is unchanged in principle at `c641a9d`:

- `backend/wayland/vulkan.rs::acquire_next_image` uses zero-timeout acquisition.
  First-use images are `UNDEFINED`; reused images are `PRESENT_SRC_KHR`.
  Getting an image index does **not** mean presentation has finished reading it.
- `backend/vulkan/frame.rs::render` passes the acquisition semaphore to
  `Surface::wait(..., true)`, transferring destruction ownership to Skia.
- In the pinned rust-skia `0d2261c` / Skia `90af6f5`, `SkSurface.h` guarantees
  transfer/fragment-shader blocking only. `GrVkCommandBuffer.cpp` actually waits
  at `FRAGMENT_SHADER | TRANSFER`; `GrVkImage.cpp` maps an `UNDEFINED` layout's
  source stage to `TOP_OF_PIPE`. The narrower wait does not protect that initial
  transition. Reuse still needs explicit testing.
- Ganesh receives the same `VulkanDevice::queue()` as the presenter. Our
  `VulkanTargetSurface::set_state` updates Rust bookkeeping only, not a real
  barrier or Ganesh's tracked state.

Paths above are under `native/emerge_skia/src/`. Discarding contents using
`UNDEFINED` does not permit a layout transition while WSI is reading the image.

## Implemented fix: a native wait-only submission

Before Ganesh receives an acquired Wayland target, enqueue this dependency on
**the exact same graphics/present queue**:

```text
WSI acquisition signals acquire semaphore
  -> vkQueueSubmit(wait = acquire semaphore, stage = ALL_COMMANDS,
                   command buffers = [], signal semaphores = [],
                   fence = wait-retirement fence)
  -> Ganesh's existing layout transitions and rendering
  -> image's existing render-finished semaphore -> vkQueuePresentKHR
```

The `vkQueueSubmit` specification includes commands in **later same-queue
submissions** in the semaphore wait's second synchronization scope, constrained
by its stage mask. `ALL_COMMANDS` supplies the required broad scope. This relies
on an explicit semaphore dependency, not submission order alone. The submit
fence covers the wait batch and permits nonblocking retirement checks.

The adapter is Wayland-local in `backend/wayland/vulkan/acquire.rs`:

- Preserve actual image layouts and Ganesh's tracked layouts; let Ganesh perform
  its existing transitions. No native layout-preparation command buffer, second
  handoff semaphore, Skia fork, or synchronization2/API-floor change is needed.
- Consume the WSI semaphore **once**, natively. Only after successful submission
  pass `acquire_semaphore: None` into the shared engine; never also pass it to
  `Surface::wait` or transfer its destruction to Skia.
- Keep all queue calls on the presenter owner thread. No new scheduler, BEAM wait,
  public API, or synchronization switch. Validate before claiming this fixed;
  measure the extra submit before considering a narrower wait scope.
- Leave shared `Surface::wait` behavior alone: headless external acquisition and
  imported video have separate contracts. DRM/probe targets currently pass no
  acquisition semaphore to the shared frame engine.

## Ownership and failure contract

Use an acquisition-slot pool bounded by the live swapchain image count.
Acquisition-slot indices are **not** swapchain image indices. Reserve before WSI;
never grow an unbounded retirement list when the GPU stalls.

Each slot owns a semaphore, a **WSI acquisition fence**, and a **native wait-submit
fence**. Supply both semaphore and acquisition fence to WSI: the acquisition fence
provides completion proof if acquisition succeeds but native submission fails.
A device-idle call alone is not proof that an unconsumed WSI signal completed.
Preallocate these objects rather than adding per-frame fence allocation.

```text
available -> acquired/unsubmitted -> native wait submitted -> available
                    \-> terminal retirement/quarantine on failure
```

Retain generation/image identity with outstanding work. Reuse/reset submitted
slots only after **both** fences complete; do not assume they become observable
simultaneously. Poll during acquisition/retirement, not in a new idle-time loop.
If no slot is free, return the existing no-frame result and use Wayland's single
1 ms acquisition-retry source, without spending submission credits.

| Outcome | Required handling |
|---|---|
| Allocation failure before WSI | No acquisition; unwind unused resources only. |
| `NOT_READY` / `TIMEOUT` / acquisition `OUT_OF_DATE` | No acquisition signal; release reservation and retain existing bounded retry/recreation. |
| Successful/suboptimal acquisition | Record ownership before fallible index/state checks; submit exactly one native wait before Ganesh. |
| Bad image/state or native submit failure after acquisition | Do not draw or immediately destroy a possibly pending semaphore. Retain the WSI fence and use proved terminal cleanup/quarantine. Never await a submit fence that was not submitted. |
| Draw/flush/capture failure after native submission | The wait remains pending work; retire by its fences, never resubmit its semaphore or falsely mark presentation. |
| Resize, recreation, Stop, device loss | Retire old-generation slots before reset/destruction; preserve terminal/abandon/quarantine behavior when ownership cannot be proved. |

Audit the current successful-acquire branches that directly destroy semaphores,
and shared `retire_rejected_acquire_semaphore`. Moving WSI ownership into this
adapter should remove Wayland's reliance on those cleanup assumptions without
speculatively rewriting other callers.

Keep render-finished semaphores **per swapchain image**. A wait-retirement fence
is not presentation completion. Review existing recreation/teardown assumptions
where touched; new presentation-lifetime diagnostics remain blockers and cannot
be waived by an acquisition-fence check.

## Implementation and remaining gates

- [x] Native acquire pool and wait-only submission; tests exercise the production
  lifecycle through a narrow Vulkan-call fake, including partial allocation,
  exhaustion, either fence order, failures, stale generations and shutdown.
- [x] Wayland integration preserves layouts, nonblocking acquisition, capture and
  per-image render-finished signals. Failed ownership barriers quarantine the
  native graph and propagate to the existing session lifecycle/Stop status.
  The graph retains SCTK's owning `Window` and the Wayland connection, not just a
  surface proxy whose protocol object the application could otherwise destroy.
- [x] Opt-in no-readback reuse and capture regressions, plus the existing cap/idle
  test. Live window size is not a supported config operation; interactive resize
  remains manual rather than pretending a config-only change exercises it.
- [x] `scripts/check-wayland-vulkan.exs` and ExUnit-tested log checker. It requires
  three actual hardware Vulkan confirmations and complete, unique zero-error /
  zero-warning native reports, including any extra probe instances.
- [x] `EMERGE_VULKAN_SYNC_VALIDATION=1` enables the feature through
  `VK_EXT_validation_features`, failing if unavailable. Native session IDs pair
  startup/final reports. Callback counters remain alive through instance
  destruction (including messenger-creation failure); final counts follow it.
  General validation and shared frame-engine rendering behavior remain unchanged.
- [x] Fixed the pre-existing Vulkan-only irrefutable-pattern warning exposed by
  the required Clippy profile; no Video import behavior changed.
- [x] Complete the software matrix below.
- [x] With explicit device consent, pass the enforcing hardware gate for both
  Vulkan-only and combined Wayland builds on Hyprland/RADV RX 7900 XTX.
- [x] Reproduced the original hazard on `main` (`c641a9d`) with the same tests:
  ExUnit passed, but native synchronization validation reported 14 errors.
- [ ] Interactive resize/occlusion/recreation, additional targets and performance
  measurements; these are not implied by the regression-suite pass.

No second production strategy, scheduler or public synchronization mode was added.

## Commit organization

1. **Vulkan diagnostics:** explicit synchronization validation, paired instance
   reports, and callback storage alive through teardown.
2. **Regression gate:** opt-in hardware cases and the tested Elixir runner that
   rejects missing/nonzero native reports. This exposes the pre-fix hazard.
3. **Wayland fix:** bounded acquire waits, ownership/failure tests, owning-window
   lifetime and session quarantine propagation; preserve shared render contracts.
4. **Documentation:** synchronization contract, baseline/fixed hardware evidence,
   software validation and remaining qualification boundaries.

## Validation after implementation

```sh
EMERGE_SKIA_BUILD=1 ./ci-tests.sh
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features wayland-vulkan
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features wayland-vulkan --all-targets -- -D warnings
cargo test --manifest-path native/emerge_skia/Cargo.toml --features drm-all,wayland-all,headless-all
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --features drm-all,wayland-all,headless-all --all-targets -- -D warnings
```

If shared files change, also check minimal, DRM-Vulkan-only and headless-Vulkan-only
builds. Preserve the CPU-only profile's GPU-free dependencies. Ordinary tests must
not open a display or DRM card.

**Hardware requires explicit consent; the user authorized Wayland testing.**
No DRM/KMS modesetting or unrelated desktop controls were exercised.
Use source-built `compiled_backends: [wayland: [:vulkan]]`, then the combined
Wayland build, with disposable local configuration. Confirm `renderer_info/1`
reports Vulkan and `software: false`; record actual GPU, driver and compositor.

Use the enforcing runner from this worktree:

```sh
elixir scripts/check-wayland-vulkan.exs --allow-hardware /tmp/wayland-vulkan-validation.log
```

It sets source-build, Vulkan-selection and programmatic sync-validation flags;
it never treats ExUnit success alone as qualification. A layer-settings file is
still useful for reproducing the old base, which lacks the new diagnostic flag.

Acceptance:

- First use of every image, sustained reuse/animation **without capture**, then
  capture/known-color checks: zero synchronization/lifetime errors or unexpected
  warnings, with complete native stderr retained through shutdown.
- Capped/uncapped transitions, static idle, occlusion, resize/recreation,
  suboptimal/out-of-date handling and prompt Stop with work outstanding.
- Bounded resource high-water mark, no new normal-frame CPU wait, unchanged pixel
  correctness, and acceptable submit cost/cadence/idle behavior against the same
  device baseline. An extra queue submit is not assumed free.

RADV/Hyprland is the known failing target; another available driver/compositor is
useful corroboration. Do not reopen closed general GPU/PRIME/reconnect/FP3 gates.
Mark unavailable checks unverified and never modeset the host desktop card.

## Boundaries and references

No Graphite migration, pacing/prediction retuning, screenshot redesign, generic
backend loop, validation suppression, or per-frame device/queue-idle workaround.
Existing explicit capture and teardown waits remain separate from steady state.

- [Queue submission wait scopes](https://docs.vulkan.org/spec/latest/chapters/cmdbuffers.html#vkQueueSubmit)
- [Synchronization, fences and layout dependencies](https://docs.vulkan.org/spec/latest/chapters/synchronization.html)
- [WSI acquisition and asynchronous signal operations](https://docs.vulkan.org/spec/latest/chapters/VK_KHR_surface/wsi.html#vkAcquireNextImageKHR)
- [Presentation semaphore reuse](https://docs.vulkan.org/guide/latest/swapchain_semaphore_reuse.html)
- [Pinned SkSurface wait contract](https://github.com/rust-skia/skia/blob/90af6f5c5a3ed8788b805fe6ca3fcdf28c2c2bc9/include/core/SkSurface.h#L636)
- [Pinned Ganesh wait stages](https://github.com/rust-skia/skia/blob/90af6f5c5a3ed8788b805fe6ca3fcdf28c2c2bc9/src/gpu/ganesh/vk/GrVkCommandBuffer.cpp#L647)
- [Pinned image-transition stages](https://github.com/rust-skia/skia/blob/90af6f5c5a3ed8788b805fe6ca3fcdf28c2c2bc9/src/gpu/ganesh/vk/GrVkImage.cpp#L321)
- [Native build profiles](../guides/reference/native-renderer-builds.md)

## Validation results

Software logs: `/tmp/emerge-wayland-sync-final.zx4Ca9` (final owning-Window
lifetime rerun); `/tmp/emerge-wayland-sync-final.bTVx7R` (initial complete pass).

- Full `EMERGE_SKIA_BUILD=1 ./ci-tests.sh`: formatting, warnings-as-errors compile,
  Credo, Clippy, 672 ExUnit/doctest passes (9 hardware exclusions), 1,615 default
  release Rust unit tests, and Dialyzer (zero errors).
- Vulkan-only Wayland: 1,651 Rust unit tests and all-target Clippy passed.
- All-Linux features: 1,754 Rust unit tests, 2 ignored, and all-target Clippy passed.
- Each Rust test profile also passed 1 fixture test and 13 border-cache tests.
- All-target checks: no-default/minimal, DRM Vulkan, headless Vulkan, CPU-only DRM.

Hardware logs: `/tmp/emerge-wayland-vulkan-hardware.s3tZz4`.

- Hyprland protocols; RADV RX 7900 XTX (`software: false`), Mesa
  `26.2.4-arch1.1`, Vulkan loader/tools `1.4.363.0-1`.
- Khronos layer `1.4.363.0-1` was missing from the container. An official Arch
  package was signature-verified and extracted under the log directory;
  `VK_ADD_LAYER_PATH` and `LD_LIBRARY_PATH` selected it for child processes only.
  System packages were not changed.
- `baseline-native.log`: unmodified production code at `c641a9d`, with the same
  test files overlaid and sync validation enabled via the layer-settings file.
  **3 ExUnit tests passed but validation reported 5 / 5 / 4 errors** (capture /
  no-readback reuse / frame caps), zero warnings. All 14 were the acquired-image
  `WRITE_AFTER_READ` layout-transition hazard with `TOP_OF_PIPE` source scope.
- `vulkan-only-native.log` and `combined-native.log`: **3 tests passed per build,
  all 3 native instances per run reported errors=0 / warnings=0** after teardown,
  with programmatic synchronization validation enabled. Both enforcing gates
  exited successfully.
- Coverage: 48 successive no-readback frame updates, repeated known-color capture,
  live caps/uncapping, static idleness, and Stop with work outstanding.
- Both hardware profiles and the full software matrix were rerun successfully
  after strengthening quarantine to retain the owning SCTK Window. The final
  source hashes are in `validated-code-sha256`; earlier hardware logs are in
  `initial/`. Temporary baseline worktrees were removed.
- Temporary configuration was removed and default dev/test NIF builds restored;
  the gate's 5 software tests passed again (`default-final-restore.log`). No
  performance improvement or cross-driver/general GPU qualification is inferred
  from these passes.
