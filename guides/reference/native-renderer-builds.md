# Build the native renderer

Emerge normally downloads a precompiled Linux NIF or the matching macOS host.
The release matrix covers x86_64 GNU/musl, AArch64 GNU, ARMv7 hard-float,
and RISC-V64 GNU. A
source build is required only for unsupported targets or custom backend
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

Each value is `:all` or an exact GPU API list. Wayland, DRM, and headless
support `:opengl` and `:vulkan`; macOS supports `:metal`. Atom entries retain
the compatibility behavior: `[:wayland, :drm]` selects OpenGL for both.

The release artifact profiles are:

| Target | Default artifact | Additional variants |
|---|---|---|
| `x86_64-unknown-linux-gnu` | Wayland/OpenGL | DRM/OpenGL, combined Wayland/DRM, minimal raster, Vulkan-only Wayland/DRM/headless, combined Vulkan/OpenGL |
| `aarch64-unknown-linux-gnu` | Wayland/OpenGL | DRM/OpenGL, combined Wayland/DRM, minimal raster, Vulkan-only Wayland/DRM/headless, combined Vulkan/OpenGL |
| `armv7-unknown-linux-gnueabihf` | Minimal raster | DRM/headless OpenGL |
| `x86_64-unknown-linux-musl` | Minimal raster | DRM/headless OpenGL |
| `riscv64gc-unknown-linux-gnu` | Minimal raster | DRM/headless OpenGL |

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
from source. See `EmergeSkia.start/1` for valid backend and rendering API
combinations.

The GNU `drm`, `drm_vulkan` and `drm_all` archives embed FreeType and disable
Skia fontconfig. They do not require xkbcommon, Wayland or desktop font libraries;
applications supply their registered font assets. Release builds inspect the
**packaged** ELF and load all three AArch64 archives inside the checksum-pinned
stock Nerves RPi5 2.0.1 rootfs. This checks transitive library dependencies and
GLIBC/GLIBCXX symbol compatibility, not just matching library filenames.

These fixes require a release after **0.4.0**. Its existing archives are not
replaced: 0.4.0's combined DRM API selection downloads the desktop Vulkan bundle,
and its standalone GNU DRM builds can still require fontconfig.

Stock RPi5 2.0.1 provides Mesa V3D/OpenGL ES, not a Vulkan driver stack. Use
`compiled_backends: [drm: [:opengl]]` and `rendering_api: :opengl` on that system.
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
stock-rootfs load, tag/version and packaging checks remain mandatory before
publishing. Hex publication reuses successful CI and artifact workflow results
for the same commit, including manual publication, rather than invoking CI a
third time. Previously published release hashes are never overwritten.

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
