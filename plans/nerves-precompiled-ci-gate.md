# Nerves precompiled artifacts and one CI gate

Branch: `fix/nerves-precompiled-ci-gate`.

- Add a dedicated DRM OpenGL+Vulkan archive; never select the desktop bundle for a DRM-only application.
- Build GNU DRM/OpenGL, DRM/Vulkan and DRM/all with embedded FreeType and fontconfig disabled. Keep published archives, not application-side replacement NIFs.
- Validate the packaged ELF architecture, NIF entry point and dependency allowlist. Load all three AArch64 DRM artifacts against the checksum-pinned stock Nerves RPi5 2.0.1 rootfs before publishing. This checks libc/C++ ABI and transitive dependencies, not GPU/hardware behavior.
- Reuse a successful `ci.yml` run for the exact commit. Release jobs wait for that result, do not invoke CI again, and do not build before the gate passes. Hex publication also requires a successful artifact workflow for that commit. Keep platform/version CI coverage and artifact-specific checks.
- Add selection/download/checksum, ELF policy and workflow-gate regressions. Run `mix test`, `cargo test` and local quality checks. Document remaining release/hardware validation; do not overwrite 0.4.0 assets or publish from this working tree.

## Implemented and validated

- Dedicated `drm_all` selection covers both the backend/API matrix and the legacy DRM+Vulkan settings. GNU DRM variants force embedded Skia. Checksum metadata includes both new archives.
- Artifact matrices wait for exact-SHA CI success; tag pushes and release callers no longer repeat CI. Version extraction runs once, unnecessary Mix dependency installation was removed from native builds, and additional full-sweep tests no longer repeat ordinary tests.
- Added fail-closed workflow/ELF policy tests, RPi5 archive download/cache/checksum regressions and workflow wiring tests. API failures, failed/skipped workflows, PRs, foreign repositories and other SHAs cannot pass the gate.
- `./ci-tests.sh all`: **580 Elixir checks passed**, 4 hardware exclusions; **1,496 Rust tests passed**. Formatting, warnings-as-errors compilation, Credo, Clippy and Dialyzer passed. Updated workflow tests also passed after the final archive-path assertion.
- CI-mode `mix test --only full_sweep`: **6 passed, 578 excluded**, confirming ordinary tests are not repeated. Final formatting, `actionlint` and `git diff --check` passed.
- Built the actual x86_64 `drm-all,embedded-freetype` NIF with the release GN/linker flags; ELF policy and eager `dlopen` passed. Only GBM and standard C/C++ runtime libraries are needed. The checker correctly rejects the existing published 0.4.0 desktop Vulkan archive for xkbcommon/fontconfig.
- Toolchain: Elixir 1.20.2, OTP 29.0.5, Rust 1.91.0. Local validation used `SKIA_SOURCE_DIR` pointing to the already cached, pinned rust-skia source because the GN download endpoint did not resolve. No application-side NIF substitution or dependency changes.

## Still required before shipping

- Run the release workflow to build/check the actual AArch64 archives and execute the stock-rootfs load gate. Local x86_64 results do not qualify AArch64 or hardware.
- Version 0.4.1 and its dated release notes are prepared on `release/v0.4.1`. Pass CI on the final release commit before tagging. No release has been published or tag moved; 0.4.0 assets remain untouched.
- Qualify physical RPi5 display/input behavior separately. The pinned stock 2.0.1 baseline has OpenGL ES, not Vulkan; a Vulkan renderer needs a system supplying the appropriate Vulkan driver stack.
