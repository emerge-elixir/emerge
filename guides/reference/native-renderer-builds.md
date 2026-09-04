# Build the native renderer

Emerge normally downloads a precompiled Linux NIF or the matching macOS host.
Precompiled Linux artifacts cover x86_64, AArch64, and 32-bit ARM hard-float. A
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

Backend features come from compile-time application config:

```elixir
# config/config.exs
config :emerge,
  compiled_backends: [:wayland, :drm],
  compiled_vulkan_backends: []
```

The release artifact profiles are:

| Target | Default artifact | Additional variants |
|---|---|---|
| `x86_64-unknown-linux-gnu` | Wayland/OpenGL | DRM/OpenGL, combined Wayland/DRM, minimal raster, Vulkan |
| `aarch64-unknown-linux-gnu` | Wayland/OpenGL | DRM/OpenGL, combined Wayland/DRM, minimal raster, Vulkan |
| `arm-unknown-linux-gnueabihf` | Minimal raster | DRM/headless OpenGL |

A renderer-only embedded application selects the minimal raster artifact with:

```elixir
config :emerge,
  compiled_backends: [],
  compiled_vulkan_backends: []
```

This is the NameBadge profile. It contains CPU raster rendering, registered
fonts, image decoding, and SVG rendering without desktop or video dependencies.
On 32-bit ARM, `compiled_backends: [:drm]` selects the OpenGL artifact, which also
supports headless OpenGL rendering.

On 64-bit Linux, any valid non-empty `compiled_vulkan_backends` selection uses
the Vulkan artifact. The artifact includes the Wayland, DRM, and headless
Vulkan routes so it can satisfy every supported 64-bit Linux Vulkan profile.
See `EmergeSkia.start/1` for valid backend and rendering API combinations.

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
