# Build the native renderer

Emerge normally downloads a precompiled Linux NIF or the matching macOS host.
Precompiled Linux artifacts cover x86_64, AArch64, and ARMv7 hard-float. A
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

The ARMv7 artifact uses the hard-float ABI of the
`armv7-nerves-linux-gnueabihf` toolchain used by Cortex-A7 systems such as
Trellis. ARMv6 targets require a source build.

Trellis selects the ARMv7 artifact automatically. Emerge uses the Rust target
already resolved from the Nerves compiler in `CC`, so
`armv7-nerves-linux-gnueabihf-gcc` selects `armv7-unknown-linux-gnueabihf`
even though Nerves exposes `TARGET_ARCH=arm`. No environment override is needed.

A renderer-only embedded application selects the minimal raster artifact with:

```elixir
config :emerge,
  compiled_backends: []
```

This is the NameBadge profile. It contains CPU raster rendering, registered
fonts, image decoding, and SVG rendering without desktop or video dependencies.
On ARMv7, `compiled_backends: [drm: [:opengl]]` selects the OpenGL artifact,
which also supports headless OpenGL rendering.

Use an exact API list to exclude the other GPU API. For example, an RPi5 DRM
build can omit all OpenGL code with:

```elixir
config :emerge,
  compiled_backends: [drm: [:vulkan]]
```

On 64-bit Linux this selects the `drm_vulkan` artifact. Equivalent
`wayland_vulkan` and `headless_vulkan` artifacts are available. `[drm: :all]`
or `[drm: [:opengl, :vulkan]]` includes both APIs and selects the comprehensive
`vulkan` artifact; custom combinations not covered by the release matrix build
from source. See `EmergeSkia.start/1` for valid backend and rendering API
combinations.

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

Release validation must also compile the unpacked Hex package so missing native
sources, benchmarks, support files, or guides are detected before publication.
