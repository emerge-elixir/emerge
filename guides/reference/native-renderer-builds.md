# Build the native renderer

Emerge normally downloads a precompiled NIF or the matching macOS window host.
The release matrix covers x86_64 GNU/musl, AArch64 GNU, ARMv7 hard-float,
RISC-V64 GNU, and macOS raster on Apple Silicon/Intel (0.4.2+).
A source build is required only for unsupported targets or custom backend
combinations.

## Toolchain floor

Source builds require:

- Elixir 1.19 or newer
- Rust 1.91 or newer
- a C/C++ toolchain and the native development libraries for the selected
  backend

The crate declares Rust 1.91 as its minimum and CI tests that version as well as
current stable Rust.

## Force a source build

A consuming application that builds from source must explicitly depend on
Rustler (Emerge's Rustler dependency is optional):

```elixir
{:rustler, "~> 0.38.0", runtime: false}
```

Set the build-only variable before fetching or compiling dependencies:

```sh
EMERGE_SKIA_BUILD=true mix deps.compile emerge --force
```

`EMERGE_SKIA_BUILD` affects artifact selection during compilation. It is not
application runtime configuration.

Linux desktop source builds need EGL, GBM, DRM, fontconfig, FreeType, Wayland,
and xkbcommon development packages when those features are selected. For
example, on Ubuntu:

```sh
sudo apt-get install \
  libegl1-mesa-dev libgbm-dev libdrm-dev \
  libfontconfig1-dev libfreetype6-dev \
  libwayland-dev libxkbcommon-dev
```

Vulkan headers/loaders must also be available for Vulkan builds.

## Build selected backends

Backend features come from one compile-time backend/API matrix:

```elixir
# config/config.exs
config :emerge,
  compiled_backends: [
    wayland: [:opengl],
    drm: [:opengl, :vulkan],
    headless: [:vulkan]
  ]
```

Each value is `:all` or an exact API list. Wayland, DRM, and headless
support `:opengl` and `:vulkan`; DRM additionally accepts `:raster` for CPU-only
scanout, and macOS supports `:metal`. Atom entries retain the compatibility
behavior: `[:wayland, :drm]` selects OpenGL for both.

### DRM without a GPU driver

For devices such as Tegra 3 where KMS works but the OpenGL driver cannot render
Skia shaders, select CPU-only DRM at dependency compilation time:

```elixir
# config/config.exs
config :emerge, compiled_backends: [drm: [:raster]]
```

Return a typed config from your viewport's `mount/1` callback:

```elixir
output = %Emerge.Config.DRM{card: "/dev/dri/card0"}
config = %Emerge.Config{output: output, renderer: [api: :raster]}
{:ok, config}
```

The output record selects DRM; `renderer.api` selects how Skia draws. The viewport
infers `assets.otp_app`, while standalone `EmergeSkia.start/1` requires it explicitly.
See [viewport configuration](viewport-configuration.md).

The Cargo profile is `--no-default-features --features drm-raster`. It uses the
DRM ioctl and bundled libinput stack, with no GBM, EGL, OpenGL, Vulkan or Wayland
dependency. All DRM builds embed FreeType; Nerves builds disable fontconfig
and use registered font assets. No custom headless frame consumer or
per-frame BEAM binary is needed.

Starting with **0.5.0**, this selects the dedicated `drm_raster` archive on every
supported Linux target, including ARMv7. The release must include that archive and
its generated checksum manifest; 0.4 releases do not contain it. Recompile Emerge after changing
configuration (`mix deps.compile emerge --force`). For an unreleased checkout or
a target outside the matrix, use the source-build instructions above.

`renderer: [api: :auto]` selects raster in a DRM build without OpenGL. Explicit
`api: :opengl` and `renderer: [api: :raster, raster_present: :gpu_upload]` require
an OpenGL-enabled build. In all DRM builds, `raster_present: :auto` means CPU;
GPU upload must be requested explicitly.

The KMS driver must expose dumb buffers, an XRGB8888 primary plane, atomic
modesetting and DRM leasing. This does not add a legacy-KMS or fbdev backend.
CPU presentation uses three dumb buffers and nonblocking atomic page-flip events,
with at most one prepared successor. This uses one additional pitched framebuffer
compared with double buffering. A software cursor is used even when
`input.cursor.hardware` is `true`. Screenshots remain RGBA; CPU binary video is
supported, but DMA-BUF/PRIME video import is not. On disconnect or a
removed mode, the backend detaches and exits so the viewport can recover through
its normal lifecycle. Output selection uses `EmergeSkia.drm_outputs/1` as with
the GPU presenters.

### Bundled, daemonless DRM input

Every DRM profile statically includes libinput, libevdev, mtdev and libudev-zero.
Applications do not need to add these packages or a udev daemon to their
`nerves_system`. Source builds download pinned, SHA-256-verified upstream archives
and use the existing cross C compiler. The build host needs `curl` and `tar` with
gzip/xz/bzip2 support, but not pkg-config, Meson or Python. Set
`EMERGE_INPUT_CACHE_DIR` to share the archive cache and `EMERGE_INPUT_OFFLINE=1`
to require pre-populated archives without network access; see the input crate's
README for cache filenames and offline setup. Hardware quirks from the downloaded
libinput archive are embedded into the NIF, so deployment needs neither network
access, `/usr/share/libinput` nor writable extraction paths. Precompiled NIFs
already contain this bundle.

The backend owns a libinput path context and discovers devices itself, with
inotify plus bounded rescan/retry. Linux input drivers, readable `/dev/input`
nodes and `/sys` are required; bundling cannot replace missing kernel drivers.
Physical keyboard translation remains the existing Emerge mapping, with owner-
thread key repeat (400 ms initial delay, 40 ms interval); libinput does not provide
text/IME translation. Device removal cancels contact lifetimes.

DRM touchscreen contacts retain independent identities. Virtual keys activate on
that contact's release, with independent slide-off cancellation, pressed feedback
and hold/repeat timers. Other UI retains single-primary-touch pointer behavior.
Raw touchscreen observations use the explicit `:touch` subscription in
`Emerge.Config.Input.events`, rather than mouse-button emulation; see `Emerge.Input`.
Mouse subscriptions are unchanged. General multitouch gesture listeners are not
introduced here.

For source versions, licenses and a hardware-independent cross-build probe, see
`native/emerge_input/README.md`. DRM precompiled archives from **0.5.0** contain this
implementation. Earlier releases do not; use a matching source checkout if their
artifacts lack the bundled input stack.

No extra precompiled artifact type is needed: the existing target/profile NIF
archive contains the static input code and immutable quirks. The release workflow
builds DRM profiles from the tagged sources, and the Hex workflow generates their
RustlerPrecompiled checksums. RustlerPrecompiled installs the selected NIF into
Emerge's `priv/native`; the Nerves application release includes it in firmware.
The Hex source package retains the downloader, integration code and upstream
license notices, not the downloaded upstream source trees. Source builds fetch
and verify those archives before embedding the quirks. Existing published
archives are unchanged; never replace older release binaries or hashes.

For an unreleased checkout, explicitly select source compilation:

```sh
EMERGE_SKIA_BUILD=true MIX_TARGET=nerves_system_fp3 mix deps.compile emerge --force
```

### Release artifacts

The release artifact profiles are:

| Target | Default artifact | Additional variants |
|---|---|---|
| `x86_64-unknown-linux-gnu` | Wayland/OpenGL | CPU-only DRM, DRM/OpenGL, combined Wayland/DRM, minimal raster, Vulkan-only Wayland/DRM/headless, combined Vulkan/OpenGL |
| `aarch64-unknown-linux-gnu` | Wayland/OpenGL | CPU-only DRM, DRM/OpenGL, combined Wayland/DRM, minimal raster, Vulkan-only Wayland/DRM/headless, combined Vulkan/OpenGL |
| `armv7-unknown-linux-gnueabihf` | Minimal raster | CPU-only DRM, DRM/headless OpenGL |
| `x86_64-unknown-linux-musl` | Minimal raster | CPU-only DRM, DRM/headless OpenGL |
| `riscv64gc-unknown-linux-gnu` | Minimal raster | CPU-only DRM, DRM/headless OpenGL |
| `aarch64-apple-darwin` | Minimal raster (0.4.2+) | Separate `macos_host` window executable |
| `x86_64-apple-darwin` | Minimal raster (0.4.2+) | Separate `macos_host` window executable |

The ARMv7 artifact uses the hard-float ABI of the
`armv7-nerves-linux-gnueabihf` toolchain used by Cortex-A7 systems such as
Trellis. ARMv6 targets require a source build.

Trellis selects the ARMv7 artifact automatically. Emerge uses the Rust target
already resolved from the Nerves compiler in `CC`, so
`armv7-nerves-linux-gnueabihf-gcc` selects `armv7-unknown-linux-gnueabihf`
even though Nerves exposes `TARGET_ARCH=arm`. No environment override is needed.

Nerves x86_64 selects the **musl**, not GNU/glibc, artifact. MangoPi MQ Pro
uses `riscv64gc-unknown-linux-gnu`: the `riscv64-nerves-linux-gnu` compiler and
`TARGET_ARCH=riscv64` are normalized to Rust's `riscv64gc` target. Both default
to the DRM/OpenGL variant in Nerves; Wayland and Vulkan profiles on these two
targets still require source builds. The artifacts use embedded FreeType and
no desktop fontconfig. GPU profiles still require suitable target EGL/GBM/DRM
libraries and drivers; compiling a package does not qualify GPU support on a board.

These new targets require a release containing the matching binaries and
checksum manifest. `0.4.0-beta.1` does **not** contain either target; adding
Rustler alone does not supply a missing precompiled artifact.

A renderer-only embedded application selects the minimal raster artifact with:

```elixir
config :emerge,
  compiled_backends: []
```

This is the NameBadge profile. It contains CPU raster rendering, registered
fonts, image decoding, and SVG rendering without desktop or video dependencies.
On ARMv7, x86_64 musl and RISC-V64, `compiled_backends: [drm: [:opengl]]` selects the OpenGL artifact,
which also supports headless OpenGL rendering.

Use an exact API list to exclude the other GPU API. For example, an RPi5 DRM
build can omit all OpenGL code with:

```elixir
config :emerge,
  compiled_backends: [drm: [:vulkan]]
```

On 64-bit Linux this selects the `drm_vulkan` artifact. Equivalent
`wayland_vulkan` and `headless_vulkan` artifacts are available. `[drm: :all]`
or `[drm: [:opengl, :vulkan]]` includes both APIs and selects the dedicated
`drm_all` artifact, not the desktop `vulkan` bundle. The legacy combination
`compiled_backends: [:drm], compiled_vulkan_backends: [:drm]` selects the same
DRM-only archive. Custom combinations not covered by the release matrix build
from source. See [viewport configuration](viewport-configuration.md) and `EmergeSkia.start/1`
for runtime output/API combinations. Compile-time `compiled_backends` is separate
from `%Emerge.Config{}`; changing a runtime record does not add native features.

Every DRM Cargo profile inherits embedded FreeType from `drm-core`, including
source builds and combined Wayland/DRM profiles. No separate Cargo feature or
Nerves system FreeType package is needed. Nerves source builds also disable
fontconfig; desktop builds with Wayland may still use system fontconfig.

The GNU `drm_raster`, `drm`, `drm_vulkan` and `drm_all` archives embed FreeType
and disable Skia fontconfig. They do not require xkbcommon, Wayland or desktop font libraries;
applications supply their registered font assets. Release builds inspect the
**packaged** ELF and load all four AArch64 archives inside the checksum-pinned
stock Nerves RPi5 2.0.1 rootfs. This checks transitive library dependencies and
GLIBC/GLIBCXX symbol compatibility, not just matching library filenames.

The GPU DRM archive fixes require **0.4.1 or later**; `drm_raster` additionally
requires **0.5.0 or later**. The existing 0.4.0 archives are not
replaced: 0.4.0's combined DRM API selection downloads the desktop Vulkan bundle,
and its standalone GNU DRM builds can still require fontconfig.

Stock RPi5 2.0.1 provides Mesa V3D/OpenGL ES, not a Vulkan driver stack. Use
`compiled_backends: [drm: [:opengl]]` and `renderer: [api: :opengl]` on that system.
A Vulkan renderer additionally requires a system with the Vulkan loader and
appropriate GPU driver/extensions. Successfully loading the NIF does not qualify
Vulkan support or physical display/input operation.

## Nerves cross-builds

Emerge derives the Rust target and Clang sysroot settings from the Nerves build
environment. It also disables desktop fontconfig, embeds FreeType where needed,
and packages the link stubs used by the embedded profile.

The rust-skia build runs Python on the build host, not from the target sysroot.
It defaults to `/usr/bin/python3`. If that is not the correct host interpreter,
set an absolute path:

```sh
EMERGE_SKIA_HOST_PYTHON=/opt/homebrew/bin/python3 mix firmware
```

The path must name a regular host file. Emerge creates isolated `python` and
`python3` wrappers that remove target `PYTHONHOME`, `PYTHONPATH`, and
`LD_LIBRARY_PATH` before invoking it.

Useful checks when a Nerves source build fails:

1. Confirm `NERVES_SDK_SYSROOT`, `NERVES_TOOLCHAIN`, `CC`, and `CXX` come from
   the active Nerves system.
2. Confirm `EMERGE_SKIA_HOST_PYTHON` is absolute and executable on the host.
3. Remove stale `native/emerge_skia/target` output after changing target triples
   or backend features.
4. Re-run with the same Nerves environment used by `mix firmware`; do not copy
   host-built Skia artifacts into the target build.

`BINDGEN_EXTRA_CLANG_ARGS`, `CFLAGS`, `CXXFLAGS`, `RUSTFLAGS`, and
`SKIA_GN_ARGS` are build inputs. Emerge preserves caller values and appends the
required Nerves flags. Override them only when diagnosing a toolchain problem.

## Headless raster on macOS

Starting with **0.4.2**, the release matrix includes precompiled raster NIFs for
Apple Silicon and Intel (macOS 11 or later). Use a published release containing
both the archives and their generated checksum manifest. Version 0.4.1 and
earlier do not include them; existing release assets are not replaced.

Select raster at dependency compilation time:

```elixir
config :emerge, compiled_backends: []
```

At runtime, retain the existing headless frame sink:

```elixir
EmergeSkia.start(%Emerge.Config{
  output: %Emerge.Config.Headless{
    target: self(),
    size: {400, 300},
    frames: %Emerge.Config.BinaryFrames{pixel_format: :bw1, dither: true}
  },
  renderer: [api: :raster],
  assets: [otp_app: :my_app]
})
```

RGBA, RGB, grayscale, BW1 and Gray2 output use the existing CPU pipeline,
including registered fonts and packed-frame dithering. This path creates no
window and needs neither Rustler nor local Rust/Skia compilation when the
published artifact is available. No macOS NIF opt-in environment variable is
needed for `compiled_backends: []`.

Darwin raster archives have no variant suffix:
`libemerge_skia-v<VERSION>-nif-2.15-<ARCH>-apple-darwin.so.tar.gz`.
Here `<ARCH>` is `aarch64` or `x86_64`; the `.so` payload is a Mach-O library.
The same raster configuration on a Trellis target selects ARMv7 Linux, even
when building firmware on a Mac. Do not select an architecture from the build
host's OS instead of the target environment.

Default macOS window applications still use the separate `macos_host` executable
and Metal. Headless Metal is not supported. GPU/window NIF combinations are not
silently mapped to the raster archive.

Release jobs inspect each packaged Mach-O's architecture, deployment version,
NIF entry point and system-only dependencies. Isolated BEAM probes without
Rustler load the archive, render packed frames and registered text, reuse the
cache, and reject a corrupted archive. These checks run on both macOS runners;
Linux fixture tests alone do not qualify the macOS binaries.

## Develop the macOS host locally

Normal macOS use downloads a versioned `macos_host` artifact. To rebuild and
select the host from a source checkout:

```sh
EMERGE_SKIA_MACOS_HOST_BUILD_LOCAL=true iex -S mix
```

This is a development switch, not an application setting.

## Validate a source build

From the Emerge source tree:

```sh
mix compile --force --warnings-as-errors
mix test

cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings
```

Release validation also compiles the unpacked Hex package, including the
feature-gated benchmarks, and checks its generated documentation screenshots:

```sh
bash scripts/check-package.sh
```

This detects omitted benchmark fixtures, support files, guides, and image inputs.
It checks screenshots before regeneration so missing-asset fallback rendering
cannot silently replace the checked examples. The package includes benchmark
sources/fixtures and sample assets with their redistribution notices.

## Release validation without repeated CI

Run `CI` on the exact release commit (normally by merging it to `main`). Once
that workflow succeeds, push the version tag or dispatch `Build Release Artifacts`
on that ref. Tag pushes do not start CI again. The artifact workflow waits up to
one hour for an existing successful `ci.yml` run for that SHA; it never dispatches
another test run. A PR merge SHA, another commit, a fork, or a failed/skipped run
cannot authorize a release. For a commit without a main-branch run, dispatch CI
on that exact ref first.

Both native build matrices depend on this gate. Artifact-specific feature, ELF,
Mach-O, packed-frame, stock-rootfs load, tag/version and packaging checks remain
mandatory before publishing. Hex publication reuses successful CI and artifact
workflow results for the same commit, including manual publication, rather than
invoking CI a third time. Previously published release hashes are never overwritten.

### Prepare and publish 0.5.0

1. Validate the release checkout before integration:

   ```sh
   EMERGE_SKIA_BUILD=1 ./ci-tests.sh
   cargo test --locked --manifest-path native/emerge_input/Cargo.toml
   bash scripts/check-package.sh
   elixir scripts/release-notes.exs v0.5.0
   ```

   Mix, the renderer Cargo manifest and its lockfile must all say `0.5.0`.
   The changelog must contain one dated, finalized `0.5.0` section. Review its
   known limitations; local software checks are not hardware qualification.
2. Merge the release preparation into the default branch and wait for successful
   CI on that exact commit, including macOS. If the release date changes, update
   the changelog before the final CI run.
3. Create and push `v0.5.0` only on that validated, integrated commit. The tag
   starts `Build Release Artifacts`, which builds the complete NIF and macOS-host
   set and verifies packaged dependencies, macOS raster behavior and stock RPi5
   userspace loading before publishing the GitHub release.
4. Let `Hex Release` reuse the exact CI/artifact results, download all published
   NIFs and generate their checksum manifest before publishing the package/docs.
   The source package must contain that generated manifest. Never use local build
   hashes, copied 0.4 checksums or replacement assets to bypass these gates.

Before tag/publish, only source preparation is complete. No artifact/checksum or
board-qualification claim follows from the version bump alone.

### Wayland Vulkan synchronization in 0.5.0

The Wayland Vulkan acquisition synchronization fix is included in 0.5.0. Recorded
Hyprland/RADV RX 7900 XTX runs pass in Vulkan-only and combined Wayland builds,
with zero native validation errors or warnings through teardown. Coverage includes
sustained rendering, screenshots, live frame-rate limits, idle behavior and stop.
Interactive resize/occlusion/recreation, other drivers and performance measurements
remain separate checks; this is not general Vulkan hardware qualification.

To repeat the targeted checks, install the Khronos validation layer and select
`compiled_backends: [wayland: [:vulkan]]`. Only with permission to open test windows
on a running compositor, run:

```sh
elixir scripts/check-wayland-vulkan.exs --allow-hardware /tmp/wayland-vulkan-validation.log
```

Repeat with `compiled_backends: [wayland: [:opengl, :vulkan]]` for the combined
build. The runner forces source compilation and Vulkan selection, enables sync
validation through `EMERGE_VULKAN_SYNC_VALIDATION=1`, and checks hardware selection
and complete native startup/teardown reports. Missing reports, validation errors,
warnings or quarantined resources fail even when ExUnit passes. See the
[frame-pacing validation guide](https://github.com/emerge-elixir/emerge/blob/main/guides/internals/frame-pacing.md#validation)
for the detailed scope and recorded evidence. This Wayland fix does not add DRM
Vulkan power controls or change headless/video synchronization contracts.

## Maintaining build configuration

`mix.exs` keeps project metadata, dependencies and commands. Build-only helpers
in `mix/native.exs`, `mix/package.exs` and `mix/docs.exs` own SDK preparation,
package contents and documentation settings. They load before dependencies and
are included in the Hex package.

`mix/targets.exs` contains the shared Nerves compiler mapping. The runtime
BuildConfig embeds this data at compilation; runtime code does not load Mix
helpers. Native/BuildConfig track the relevant helper/data files as external
resources so subsequent Mix runs recompile affected modules when those inputs
change. Preserve these bootstrap and invalidation rules when editing helpers.

`test/mix_project_test.exs` covers cold project loading and SDK behavior.
`mix test test/mix_package_test.exs --include full_sweep` additionally checks an
unpacked, offline consumer without Rustler and incremental recompilation.
