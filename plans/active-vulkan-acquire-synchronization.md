# Vulkan acquired-image synchronization

Status: **planned, deferred**. Separate from the declarative viewport config work;
resume this after that implementation, not as an expansion of its scope.

## Reproduction and evidence

On commit `853d951`, hardware Wayland Vulkan on Hyprland / RADV RX 7900 XTX
passes `test/emerge_skia/wayland_frame_rate_test.exs`, but Khronos synchronization
validation reports `SYNC-HAZARD-WRITE-AFTER-READ` between image acquisition and
Skia's initial image layout transition. Native validation produced **4 errors,
0 warnings**. See [the environment/qualification record](active-linux-gpu-qualification.md).
The originating commit has not been bisected; do not assume pacing introduced it.

Relevant paths:

- `native/emerge_skia/src/backend/wayland/vulkan.rs`: swapchain acquisition.
- `native/emerge_skia/src/backend/vulkan/frame.rs`: acquisition semaphore passed
  to Ganesh `Surface::wait`, with destruction ownership transferred on acceptance.
- Skia's `SkSurface.h` documents only transfer/fragment-stage blocking for that
  API. The observed initial transition uses `TOP_OF_PIPE` as its source stage;
  the present wait scopes do not establish the required execution dependency.

## Implementation steps

1. Reproduce with explicit hardware Vulkan and synchronization validation;
   compare capped and uncapped rendering and, if practical, the pre-pacing
   baseline. Preserve logs and verify the selected GPU/API, not only build flags.
2. Audit initial and reused swapchain image layouts, semaphore wait stage masks,
   queue ownership, capture transitions and Ganesh's tracked image state. Prove
   the execution/memory dependency chain against Vulkan synchronization rules.
3. Evaluate a small native GPU handoff submission: wait for acquisition with
   appropriate scope, perform image preparation, then hand Skia the real image
   state and an explicit GPU dependency. Compare with broadening Skia's actual
   wait scope; its public `Surface::wait` API does not expose that stage mask.
   Choose the smallest correct solution; the handoff design is not yet proven.
4. Keep preparation resources bounded and owner-thread managed. Consume each
   binary semaphore once, track submission completion before destruction/reuse,
   and preserve rejected-wait, submit-failure, device-loss and teardown safety.
   Handle swapchain generations explicitly. Never just relabel an image layout.
5. Audit other users of the shared Vulkan frame engine (DRM/headless/imports),
   without claiming those routes reproduce the observed Wayland issue.

No per-frame device/queue-idle calls, CPU acquisition waits, validation suppression
or unrelated renderer/pacing redesign. Teardown-only waits remain governed by
existing cleanup/quarantine rules. Native wrappers stay thin; pure state/ownership
logic gets Rust tests and runtime behavior gets opt-in integration coverage.

## Acceptance

- Existing frame-cap assertions pass with **zero synchronization errors**;
  native validation counts must gate qualification independently of ExUnit exit.
- Repeat first use/reuse, sustained redraw, capped/uncapped transitions, capture,
  resize/recreation, hide/show and stop with work outstanding.
- Exercise rejection/failure paths with deterministic ownership tests; no double
  waits, premature destruction, unbounded resources or stale-generation reuse.
- Run `cargo test`, `mix test`, Vulkan-enabled Rust tests and applicable Clippy.
- Repeat relevant shared-path hardware checks where available. Mark unavailable
  DRM/PRIME/platform gates unverified; one Wayland smoke is not full qualification.
