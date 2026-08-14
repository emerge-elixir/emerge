# Cross-repository commit sequence

Status: executed on 2026-08-14. Source commits were created in dependency order and all
indexes are clear. Raw investigation artifacts and the separate RP1 D-PHY working hunks
remain deliberately uncommitted.

## Objective

Turn the current guarded VideoInterop, Vulkan, V3DV NV12, Camera lifecycle, and RPi5
system work into reviewable commits without losing unrelated dirty work or claiming
unfinished target qualification.

## Repository baseline

| Order | Repository | Branch | Baseline |
| ---: | --- | --- | --- |
| 1 | `/workspace/video_interop` | `master` | `fc53b27` |
| 2 | `/workspace/membrane_video_interop` | `master` | `d2db478` |
| 3 | `/workspace/colibri/membrane_libcamera` | `main` | `f81cd9d` |
| 4 | `/workspace/emerge-headless` | `headless-backend` | `e6bb0c8` |
| 5 | `/workspace/emerge_demo` | `prime-validation` | `22c5a3e` |
| 6 | `/workspace/colibri/nerves_system_rpi5` | `imx585-cef168-libcamera-dmabuf` | `e2a29da` |
| 7 | `/workspace/colibri/camera` | `main` | `746b62a` |

The order is the source dependency order, not a publication claim. The Nerves system is
technically independent of the core libraries but must precede the final Camera target
assembly.

## Execution record

Intertwined final-state implementations were kept atomic where an artificial split would
have created untested intermediate trees.

| Repository | Commits |
| --- | --- |
| `video_interop` | `3c1e123` Add guarded VideoInterop lifecycle and Vulkan DMA-BUF import |
| `membrane_video_interop` | `1854499` Make the Membrane video sink guarded and terminal-draining |
| `membrane_libcamera` | `1664631` Fix Nerves libcamera C++ sysroot discovery; `74518b6` Harden libcamera DMA-BUF synchronization and finalization |
| `emerge-headless` | `f08d80b` Add Vulkan rendering and deterministic video composition; `991a06c` Document Vulkan video architecture and target qualification |
| `emerge_demo` | `4ef72d8` Add the four-route explicit Vulkan PRIME matrix; `476c13d` Pin OpenGL PRIME source in demo tests |
| `nerves_system_rpi5` | `1562400` Make custom RPi5 system builds local and reproducible; `e3a2b29` Package V3DV diagnostics and persistent crash evidence; `8525aec` Ignore malformed libcamera array controls; `62e5d8b` and `2d2e9ec` record committed qualification identities |
| `camera` | `3ace47b` Adopt fail-closed Vulkan Camera lifecycle; `4cf56c0` Document Camera Vulkan adoption and qualification gates |

Validated after committing:

- VideoInterop: default/Vulkan Rust matrices and 96 ExUnit tests;
- MembraneVideoInterop: 47 ExUnit tests;
- MembraneLibcamera: 17 mock Rust tests and 74 ExUnit tests with 3 excluded;
- Emerge: 1,055 DRM-Vulkan Rust tests, benchmark fixture, and 448 ExUnit tests with
  7 excluded;
- Emerge Demo: 49 ExUnit tests;
- Camera: 142 ExUnit tests using explicitly staged host mock NIFs, followed by restoring
  AArch64 target artifacts;
- Nerves system: diagnostic parser tests, shell syntax checks, and checked-in
  configuration validation.

## Hard rules

1. Never use `git add -A` or `git add .` in these worktrees. Stage exact path lists and
   use `git add -p` only for named shared files.
2. Before staging, save a binary patch, untracked-source manifest, branch, and HEAD for
   every repository outside the repository itself. This is recovery evidence, not a commit.
3. Do not stage `.pi/`, `.pi-subagents/`, raw investigation logs, Cargo `target/`, `_build/`,
   generated NIFs, firmware, local Nerves artifacts, crash dumps, or temporary manifests.
4. Do stage intentional test-fixture source and lock files, checked-in SPIR-V, benchmark
   `.emrg` fixtures, Buildroot package definitions, and target diagnostic scripts.
5. A commit boundary must compile from its own tree. If a proposed patch split cannot do
   that without temporary compatibility code, collapse adjacent commits instead of
   manufacturing an unreviewed intermediate design.
6. Test a proposed commit in a clean linked worktree at that commit/tree, not in a dirty
   worktree where later unstaged changes can hide missing dependencies.
7. Keep open hardware gates truthful in plans and changelogs. Do not claim 60 FPS active
   acceptance, authoritative validation, pixel-oracle acceptance, or soak completion.
8. Commit messages must not contain `Co-Authored-By` lines.

## Explicit exclusions

Leave these uncommitted or move them to an external evidence archive:

- all `.pi/` and `.pi-subagents/` trees;
- `/workspace/colibri/membrane_libcamera/{current-open-session.txt,head-open-session.txt}`;
- `/workspace/colibri/camera/{firmware-stock-dphy.log,forced-target-app.log,forced-target-deps.log}`;
- `/workspace/colibri/nerves_system_rpi5/{artifact-891-stock.log,artifact.log,cache-broken-symlinks.log,clean-build.log}`;
- all files under ignored `target/`, `_build/`, `priv/native`, `priv/diagnostics`, and Nerves
  artifact/image directories;
- the separate RP1 D-PHY experiment and diagnosis. Its `post-build.sh` baseline guard and
  README discussion must not be folded into Vulkan packaging commits. Preserve those hunks
  for a separately approved commit or patch.

## Commit queue

### 1. `video_interop`

#### VI-1 — `Extend canonical video format synchronization metadata`

Include:

- Elixir and Rust format/colorimetry schema;
- `acquire_sync` policy validation;
- Rustler encode/decode and schema round-trip fixtures;
- format-focused tests and the required fixture-build plumbing.

Shared files requiring hunk staging: `rust/video-interop/src/beam.rs`,
`rust/video-interop/src/lib.rs`, `test/native/schema_test/src/lib.rs`, `mix.exs`, and docs.

Gate: default Rust tests, schema tests, ExUnit, format, and Clippy.

#### VI-2 — `Make canonical lease abandonment and release dispatch deterministic`

Include:

- `VideoInterop.AbandonmentGuard`;
- guarded child leases and authority checks;
- bounded release tombstones and abandonment handling in `LeaseOwner`;
- Rust `ReleaseDispatcher`, close/join semantics, health/fatal paths, and claimed-frame
  retirement;
- schema-consumer fixture and lifecycle/abandonment tests;
- corresponding README and changelog sections.

This is one atomic ownership protocol commit. Do not split Elixir guard publication from
native dispatcher authority.

Gate: all default Rust and ExUnit tests, including process-death, delayed-dispatch,
startup-failure, panic, and shutdown cases.

#### VI-3 — `Add generic Vulkan DMA-BUF import and synchronization`

Include:

- Vulkan Cargo feature/dependencies and lockfile changes;
- `rust/video-interop/src/vulkan/`;
- checked-in shader source and Vulkan 1.1 SPIR-V;
- generic external-memory import, exact sizing/topology, sync lanes, source cache, output
  pools, timing, planar/RGBA strategies, and fail-closed staging preference;
- Rust crate documentation and Vulkan README/changelog sections.

Gate: Vulkan and default Rust test matrices, warnings-denied Clippy, `spirv-val` for both
binaries, shader hash verification, and `git diff --check`.

### 2. `membrane_video_interop`

#### MVI-1 — `Make the Membrane video sink guarded and terminal-draining`

Keep this adapter migration atomic. Include:

- guarded canonical buffer insertion/discard;
- reusable `Membrane.VideoInterop.Sink` lifecycle;
- observer and frame-accepted callbacks;
- composite versus standalone completion policy;
- deterministic terminal draining, close-at-most-once behavior, and unknown-ownership
  escalation;
- native guard-authority test fixture, lockfile, support fakes, tests, README, and changelog.

Do not commit `test/native/*/target/` or staged fixture binaries.

Gate: format, all ExUnit tests, repeated test run for lifecycle races, and package build.

### 3. `membrane_libcamera`

#### MLC-1 — `Fix Nerves libcamera C++ sysroot discovery`

Include only the cross-toolchain root/linker discovery changes in `mix.exs`. This is an
independent build fix and should remain separately revertible.

Gate: the AArch64 Nerves compile path reaches the real libcamera backend.

#### MLC-2 — `Harden libcamera DMA-BUF synchronization and finalization`

Keep the producer protocol atomic unless a clean-tree dry run proves the color/sync and
finalization halves independently buildable. Include:

- exact requested/negotiated color space and chroma contract;
- complete allocation size publication;
- acquire `SYNC_FD` export/merge and terminal failure handling;
- native release dispatcher admission and custodian;
- guarded canonical frames;
- asynchronous close/finalization outcomes, bounded finalization, quarantine, and
  process-lifetime service pin;
- source-side composite drainage and exact terminal correlation;
- real/mock backend parity, diagnostics, shutdown tests, hardware-test updates, and docs.

Exclude the two raw open-session text files.

Gate: Rust format/Clippy/tests for the mock/default closure, all host ExUnit tests that do
not require host libcamera, and the authoritative AArch64 real-backend compile. Record the
known host `pkg-config` limitation rather than weakening the real build.

### 4. `emerge-headless`

`renderer.rs`, `stats.rs`, `video.rs`, `render_scene.rs`, `native lib.rs`, Cargo files, and
several tests contain overlapping work. First reproduce the proposed splits in clean
linked worktrees. If E-1 through E-3 do not each build, combine them into one atomic
`Add explicit Vulkan rendering and guarded video interop` commit. Never commit a broken
intermediate tree merely to reduce diff size.

#### E-1 — `Unify native renderer backends and preserve DRM GLES2`

Include:

- DRM monolith decomposition into `drm/{core,gl,mod}`;
- presenter-neutral backend ownership and environment plumbing;
- GLES2 baseline/capability degradation behavior;
- shared Wayland handles/environment extraction;
- headless, macOS, cursor, wake, and renderer lifecycle adjustments required by the
  decomposition;
- focused backend/options tests and the GLES2 plan/docs.

Do not include Vulkan presenters or NV12 implementation in this commit unless required for
a buildable boundary.

#### E-2 — `Add explicit Vulkan presentation across Wayland, headless, and DRM`

Include:

- Vulkan feature profiles and rust-skia pin;
- explicit API configuration and fail-closed admission;
- shared Vulkan loader/instance/device/Ganesh/frame/synchronization modules;
- Wayland WSI, headless PRIME, and no-WSI DRM/KMS presenters;
- exact DRM identity selection, scanout slots, fences, page flips, restoration,
  quarantine, and UI-only/video-optional admission;
- Vulkan functional probe, release workflow/profile updates, tests, and rendering plan.

#### E-3 — `Integrate guarded retirement and persistent V3DV NV12 staging`

Include:

- guarded `VideoConsumerSession` shutdown and exact target/stream retirement;
- Vulkan video import integration in `video.rs` and renderer plumbing;
- persistent source/cache/output/sync resources;
- planar R8/RG8 path, exact BT.709 range/siting RuntimeEffect, truthful RGBA fallback,
  transfer declarations only on local outputs, timing and validation counters;
- statistics/tests, architecture guide, shutdown-hardening plan, V3DV evidence plan, and
  this cross-repository commit-sequence plan;
- both checked staging-policy modes while retaining production `auto` preference for
  planar.

Gate E-1 through E-3: default and DRM-Vulkan Cargo tests, warnings-denied Clippy, default
and Vulkan compile profiles, ExUnit, NIF export/linkage check, and `git diff --check`.

#### E-4 — `Simplify semantic paint layers and add Camera performance fixtures`

Include:

- ordered semantic paint-layer content and deterministic Camera topology;
- paint cache ownership/invalidation simplification;
- related tree refresh/layout cleanup only where required by the model;
- renderer/cache statistics and benchmark support;
- Camera slider and Focus fixtures, generators, performance lock script, plans, and docs.

Keep Video direct and outside paint-cache payloads. Do not mix unfinished experimental
persistent semantic-backing work from other worktrees.

Gate: default Rust and ExUnit suites, benchmark-fixture test, deterministic fixture
regeneration/hash comparison, and cache pixel/topology tests.

### 5. `emerge_demo`

#### DEMO-1 — `Add the four-route explicit Vulkan PRIME matrix`

Include the runtime API/node configuration, PRIME source/view updates, matrix scripts,
tests, and README. Keep this on `prime-validation`; do not merge it to `main` until its
local Emerge/VideoInterop dependencies have publishable identities.

Gate: `mix test` and four fresh-process matrix routes where GPU hardware is available.

### 6. `nerves_system_rpi5`

Before committing, bump the custom system `VERSION`/changelog if required by the local
artifact checksum policy, and replace README wording that says the implementation diff is
uncommitted with final commit identities.

#### SYS-1 — `Make custom RPi5 system builds local and reproducible`

Include:

- unpublished local artifact-site policy;
- deterministic host-libarchive configuration;
- local portable-artifact installation script and package file inclusion;
- only the associated README text.

#### SYS-2 — `Package V3DV diagnostics and Vulkan validation`

Include:

- Broadcom Vulkan driver/loader/tools/libdrm test configuration;
- pinned SPIR-V, Vulkan Utility Libraries, and Validation Layers packages;
- target diagnostic, parser tests, config checker, license metadata, and Python ignores;
- Vulkan/GL rollback closure checks in `post-build.sh`;
- diagnostic BusyBox features and V3DV documentation.

Exclude D-PHY-specific `post-build.sh` and README hunks.

Gate: parser tests, shell syntax checks, host config check, full Buildroot/Nerves system
build, target-tree check, and portable artifact installation.

#### SYS-3 — `Persist RPi5 VM and kernel crash evidence`

Include:

- bcm2712-correct ramoops cells;
- persistent erlinit shutdown report and bounded crash dump;
- BusyBox `sync` support needed by Camera crash journaling;
- focused documentation.

Gate: DT compile/system build. Hardware pstore remains explicitly unqualified.

#### SYS-4 — `Ignore malformed libcamera array controls`

Include only `0021-pipeline-rpi-ignore-malformed-array-controls.patch` plus any precise
series/documentation reference required to apply it. Keep this separately revertible from
Vulkan packaging.

The RP1 D-PHY baseline guard remains outside this sequence until separately approved.

### 7. `camera`

Camera files are highly cross-cutting. Dry-run the splits in a clean worktree. If C-2
through C-4 cannot independently compile and test, combine them into one atomic
`Adopt fail-closed Vulkan Camera lifecycle` commit. Keep build tooling and performance
instrumentation separate where possible.

#### C-1 — `Isolate Camera target NIF and Vulkan probe builds`

Include target artifact staging, isolated Cargo target directories, ELF/linkage checks,
Nerves linker sysroot fix, fixture-build plumbing, and related ignores/tests. Include the
deferred-cleanup plan, but keep generated binaries and firmware excluded.

#### C-2 — `Make Camera stream replacement and shutdown composite`

Include:

- reusable Membrane preview adapter and observer-only `Camera.VideoSink`;
- source/preview/analysis barrier correlation;
- guarded detection sink and buffer disposal;
- lifecycle guardian, cold-restart latch, pipeline adapter, and bounded exact process-down
  handling;
- fail-closed unknown ownership and no unsafe same-VM reopen;
- shutdown/lifecycle tests and native guard fixture source.

#### C-3 — `Require target-qualified Vulkan startup and presentation`

Include:

- persistent explicit OpenGL/Vulkan boot selection;
- pinned RPi5 defaults for `/dev/dri/card1`, `/dev/dri/renderD128`, and left chroma siting,
  with deliberate build-time overrides;
- immutable Camera qualification config;
- target-before-capture startup, bounded viewport startup, exact renderer epoch/target/
  stream/page-flip readiness, and previous-frame preservation;
- split KMS/Vulkan nodes and no silent fallback;
- startup/config/readiness tests and target documentation.

#### C-4 — `Persist Camera lifecycle diagnostics and cold-restart reasons`

Include crash journal, bounded RingLogger persistence, diagnostic snapshots, exact restart
reason synchronization, application/platform startup integration, and tests. Do not claim
pstore acceptance until hardware proves it.

#### C-5 — `Add Camera Vulkan qualification and interaction diagnostics`

Include:

- probe component/node truthfulness and explicit chroma inputs;
- renderer/capture diagnostics;
- 20 Hz control publication pacing and related UI state;
- planar/RGBA image-build switch and current hardware evidence;
- active Vulkan adoption plan, README, and focused tests.

Record RGBA as rejected for pinned-RPi5 performance, keep `auto` preferring planar, and
leave the closing planar A run plus validation/pixel/soak/headroom gates open.

Gate C-1 through C-5: format, host mock/unit tests, target config evaluation with Camera
node variables unset, AArch64 release assembly, NIF hash/linkage equality, and final target
artifact restaging after host builds. Firmware generation remains separate if `fwup` is
unavailable.

## Execution procedure

For every proposed commit:

1. Create the recovery bundle and record the current status.
2. Stage only the listed paths/hunks.
3. Inspect `git diff --cached --stat`, `git diff --cached --check`, and the complete staged
   diff. Confirm excluded artifacts are absent.
4. Materialize/test the staged tree in a clean linked worktree. For downstream projects,
   point local dependencies at clean worktrees containing the exact earlier commit hashes.
5. Commit only after the staged tree passes its gate.
6. Record the new hash in this plan and in downstream provenance notes.
7. Recheck the original worktree status so no unrelated hunk disappeared.

After all repository-local commits, run the complete integration matrix from clean
worktrees in dependency order, then assemble the AArch64 Camera release and restage target
NIFs last.

## Publication order

Publication is a later operation and remains blocked by final package review and target
gates. When authorized, publish in this order:

1. `video_interop` 0.1.0;
2. `membrane_video_interop` 0.1.0 against the published core;
3. Emerge 0.4.0 and MembraneLibcamera against exact published core/adapter versions;
4. update Demo/Camera dependency declarations and locks;
5. build the versioned custom Nerves system and Camera firmware from clean committed trees.

Never publish or deploy a partial guarded ownership protocol.

## Completion criteria

- every intended source change belongs to a named commit;
- every commit is buildable and reviewable from its own tree;
- all seven repositories have zero staged files and only explicitly preserved exclusions
  left dirty;
- downstream provenance records exact dependency commit hashes;
- final host/default/Vulkan/AArch64 tests pass from clean committed worktrees;
- target-unqualified items remain clearly marked rather than inferred from host success.
