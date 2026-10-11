# Bundled DRM input commit sequence

Scope: only the Emerge worktree input implementation. Preserve Fairphone app
changes, the freedreno investigation plan/image and the original Emerge checkout.
No release/version bump, push, firmware deployment or hardware qualification.

Commit order keeps each intermediate snapshot buildable and retains the existing
DRM touchscreen behavior until the runtime can consume contact-aware events:

1. `build(input): bundle the DRM native input dependency closure`
   - Checksum-pinned upstream downloads/cache, license notices, embedded quirks,
     thread-local owning wrapper, DRM feature gating, Cargo locks and complete
     Hex integration-source package closure.
2. `feat(input): expose contact-aware touch observations`
   - Touch phases/IDs, native encoding and independent observer mask, Elixir
     subscriptions/docs, exhaustive test matches and public-contract regressions.
3. `fix(events): isolate virtual-key capture and timers per contact`
   - Contact-local lifetimes, feedback ownership, hold/repeat, stale replay,
     cancellation/focus/mount checks and single-primary-touch compatibility tests.
4. `refactor(drm): normalize device input with bundled libinput`
   - Replace evdev decoding with path-context discovery/reconciliation, owned
     device cleanup, normalized pointer/keyboard/touch events and regressions.
5. `docs(input): document bundled DRM deployment and validation`
   - Build/runtime/event guidance and implementation/validation handoff plans.

Validate the exact staged tree in an isolated worktree before each code commit,
using DRM-raster cargo tests and attempting Mix tests. Test the bundled crate,
source packaging and touch Elixir contracts where available. Normal Mix remains
blocked if Hex is missing; report that instead of claiming full ExUnit validation.

## Original staged validation

These counts record the original staged-snapshot runs, before rewriting the build
commit to use source downloads. Commit IDs are intentionally omitted because the
rewritten history must not introduce the original upstream source trees.

| Stage | DRM-raster unit tests |
|---|---|
| Static build/package closure | 1,559 |
| Public/native touch contract | 1,560 |
| Contact-local runtime | 1,572 |
| DRM libinput migration | 1,573 |

Each code snapshot passes the additional 1 + 13 fixtures (one unit test remains
ignored), all-target DRM-raster Clippy with warnings denied, and the three bundled
input tests. Focused Elixir source-package/language checks pass at the first
snapshot; all four focused checks including touch contracts pass thereafter,
using existing compiled dependencies. Every ordinary `mix test` attempt is
blocked by missing Hex/SCM for Rustler; no full Mix success is claimed.

The original split preserved upstream sources unchanged. The current branch
instead downloads checksum-pinned archives at build time, retaining integration
code and license notices without upstream source trees or quirks in the package;
see [build-time source downloads](active-input-source-downloads.md).
The documentation commit records this sequence and the release/hardware handoff.
Full firmware, physical phone/minimal-system hotplug and the complete release
artifact matrix remain unqualified.

Preservation snapshot and per-commit logs: `/tmp/emerge-input-commits-SjZxJh/`.
