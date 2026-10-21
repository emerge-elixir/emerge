# Build the native renderer

For Nerves, Raspberry Pi, custom images and SPI panels, start with
[configure Nerves graphics](https://hexdocs.pm/emerge/Emerge.html#module-viewport-configuration): it has a capability decision tree,
stock-system defaults and complete build/runtime recipes.

> This guide describes the 0.5.1 feature model. The new canonical
> archives are not supplied by 0.5.0. A precompiled build requires the
> matching release's archives and generated checksum manifest; an unreleased
> checkout uses an explicit source build. Existing release assets are immutable.

## Baseline and optional features

**Headless and CPU raster are always compiled.** They are not optional native
features. DRM and Wayland add presenters; OpenGL and Vulkan add shared rendering
APIs. The macOS window backend remains the separate AppKit host with Metal.
Headless Metal is deferred.

Keep one compile-time configuration list:

```elixir
# config/config.exs
config :emerge, compiled_backends: [headless: [:opengl]]
```

Each entry selects a backend and API list, or `:all` for its supported APIs.
Atom shorthand retains compatibility: `[:drm]` means `[drm: [:opengl]]`,
`[:wayland]` means `[wayland: [:opengl]]`, and `[:macos]` means `[macos: [:metal]]`.
The legacy `[:headless]` shorthand also selects OpenGL; for the minimal baseline
use `[]` or `[headless: [:raster]]` explicitly.
Explicit `:raster` is accepted for existing raster-capable Linux outputs and
adds no optional renderer feature.

Resolution takes a deduplicated **union** in stable order:

| Configuration | Optional features | Archive variant |
| --- | --- | --- |
| `[]` or `[headless: [:raster]]` | none | baseline, no suffix |
| `[headless: [:opengl]]` | `opengl` | `opengl` |
| `[drm: [:raster]]` | `drm` | `drm` |
| `[drm: [:opengl]]` | `drm, opengl` | `drm_opengl` |
| `[headless: [:opengl], drm: [:opengl]]` | `drm, opengl` | `drm_opengl` |
| `[wayland: [:opengl]]` | `wayland, opengl` | `wayland_opengl` |
| `[headless: [:vulkan]]` | `vulkan` | `vulkan` |
| `[drm: [:vulkan]]` | `drm, vulkan` | `drm_vulkan` |
| `[drm: :all]` | `drm, opengl, vulkan` | `drm_opengl_vulkan` |

API entries enable shared implementation, not a per-backend exclusion mask:

```elixir
config :emerge,
  compiled_backends: [wayland: [:vulkan], drm: [:opengl]]
```

This builds DRM, Wayland, OpenGL and Vulkan. Both selected Linux presenters can
use their supported compiled APIs; headless can also use supported offscreen
paths. Ordering entries or repeating an API does not create a different artifact.
Legacy `compiled_opengl_backends`/`compiled_vulkan_backends` inputs continue to
normalize through this model, but cannot be combined with matrix syntax.

Supported output modes remain distinct: headless Vulkan is PRIME-only; raster
cannot export PRIME. Including an API does not add unsupported output/format
combinations. See [viewport configuration](https://hexdocs.pm/emerge/Emerge.html#module-viewport-configuration).

## Defaults versus explicit configuration

| Build environment, setting omitted | Default |
| --- | --- |
| Normal Linux desktop | `[wayland: [:opengl]]` |
| Recognized stock Nerves system/version | Capability profile in the [system table](https://hexdocs.pm/emerge/Emerge.html#module-nerves-and-device-defaults) |
| Unknown/custom Nerves system | `[]` (headless/raster baseline) |
| macOS | `[macos: [:metal]]` using the external host |

Explicit `compiled_backends` always wins. In particular, `[]` means **no optional
features**, not "use platform defaults". A `MIX_TARGET` nickname or CPU architecture
alone is not a known board/image profile. Read [Nerves defaults and overrides](https://hexdocs.pm/emerge/Emerge.html#module-viewport-configuration).

Build-time inclusion is separate from runtime selection:

```elixir
# Build-time, config/config.exs:
config :emerge, compiled_backends: [drm: [:opengl]]
```

```elixir
# Runtime, returned from mount/1 or passed to EmergeSkia.start/1:
%Emerge.Config{
  output: %Emerge.Config.DRM{card: "/dev/dri/card0"},
  renderer: [api: :opengl],
  assets: [otp_app: :my_app]
}
```

Runtime configuration cannot enable a missing native feature. Prefer explicit
APIs in device recipes; `:auto` is output-specific and does not mean "always pick
Vulkan if compiled". A headless build still needs `%Emerge.Config.Headless{target:
...}` and an application that consumes frames; compilation does not choose a
panel or create a recipient process.

## Inspect resolution before compiling

```sh
mix emerge.build_info
mix emerge.build_info --systems
mix emerge.build_info --profiles
```

The task reads application configuration and reports system assumptions,
configuration/default provenance, canonical features and the selected variant.
It does not compile/load the NIF or probe display devices. Before the dependency
has compiled enough to expose its Mix task, run:

```sh
elixir deps/emerge/scripts/build-info.exs \
  --backend headless:opengl --target aarch64-unknown-linux-gnu
```

The standalone script has no dependency on Mix startup, Rustler or compiled BEAM
files. Without explicit `--backend` options it reports environment defaults, not
an application config it has not loaded. In Emerge's checkout use
`elixir scripts/build-info.exs`. `--systems` and `--profiles` print catalog tables.

## Exact artifact variants

A single catalog, `mix/native_profiles.exs`, drives runtime build resolution,
checksum/variant metadata, system defaults and generated release matrices. Each
NIF archive has exactly the requested optional backend/API features; no compatible
superset fallback silently adds DRM, Wayland or another GPU API.

The baseline archive has no suffix. Others use the canonical feature names:

```text
libemerge_skia-v<VERSION>-nif-2.15-<TARGET>.so.tar.gz
libemerge_skia-v<VERSION>-nif-2.15-<TARGET>--opengl.so.tar.gz
libemerge_skia-v<VERSION>-nif-2.15-<TARGET>--drm_opengl.so.tar.gz
```

An `opengl` archive does **not** contain a DRM/KMS or Wayland presenter. It still
needs the target's EGL/GBM/GPU libraries and device access. A `drm` archive contains
CPU KMS presentation and bundled input but no GL/EGL, GBM or Vulkan dependencies.
Platform modifiers, including embedded FreeType, are recorded separately from
the optional feature identity. GPU-free/headless profiles avoid desktop libraries.

The following table is generated by `scripts/build-info.exs --profiles`. It lists
planned release coverage, not a claim that a development version is published.
Coverage is intentionally curated per target for common configurations; other
valid configurations use source builds rather than larger prebuilt feature sets:

<!-- profiles:start -->
| Rust target | Transport | Optional features | Variant |
| --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `nif` | `[]` | `baseline` |
| `x86_64-unknown-linux-gnu` | `nif` | `["opengl"]` | `opengl` |
| `x86_64-unknown-linux-gnu` | `nif` | `["vulkan"]` | `vulkan` |
| `x86_64-unknown-linux-gnu` | `nif` | `["opengl", "vulkan"]` | `opengl_vulkan` |
| `x86_64-unknown-linux-gnu` | `nif` | `["drm"]` | `drm` |
| `x86_64-unknown-linux-gnu` | `nif` | `["drm", "opengl"]` | `drm_opengl` |
| `x86_64-unknown-linux-gnu` | `nif` | `["drm", "vulkan"]` | `drm_vulkan` |
| `x86_64-unknown-linux-gnu` | `nif` | `["drm", "opengl", "vulkan"]` | `drm_opengl_vulkan` |
| `x86_64-unknown-linux-gnu` | `nif` | `["wayland"]` | `wayland` |
| `x86_64-unknown-linux-gnu` | `nif` | `["wayland", "opengl"]` | `wayland_opengl` |
| `x86_64-unknown-linux-gnu` | `nif` | `["wayland", "vulkan"]` | `wayland_vulkan` |
| `x86_64-unknown-linux-gnu` | `nif` | `["wayland", "opengl", "vulkan"]` | `wayland_opengl_vulkan` |
| `x86_64-unknown-linux-gnu` | `nif` | `["drm", "wayland"]` | `drm_wayland` |
| `x86_64-unknown-linux-gnu` | `nif` | `["drm", "wayland", "opengl"]` | `drm_wayland_opengl` |
| `x86_64-unknown-linux-gnu` | `nif` | `["drm", "wayland", "vulkan"]` | `drm_wayland_vulkan` |
| `x86_64-unknown-linux-gnu` | `nif` | `["drm", "wayland", "opengl", "vulkan"]` | `drm_wayland_opengl_vulkan` |
| `aarch64-unknown-linux-gnu` | `nif` | `[]` | `baseline` |
| `aarch64-unknown-linux-gnu` | `nif` | `["opengl"]` | `opengl` |
| `aarch64-unknown-linux-gnu` | `nif` | `["vulkan"]` | `vulkan` |
| `aarch64-unknown-linux-gnu` | `nif` | `["opengl", "vulkan"]` | `opengl_vulkan` |
| `aarch64-unknown-linux-gnu` | `nif` | `["drm"]` | `drm` |
| `aarch64-unknown-linux-gnu` | `nif` | `["drm", "opengl"]` | `drm_opengl` |
| `aarch64-unknown-linux-gnu` | `nif` | `["drm", "vulkan"]` | `drm_vulkan` |
| `aarch64-unknown-linux-gnu` | `nif` | `["drm", "opengl", "vulkan"]` | `drm_opengl_vulkan` |
| `aarch64-unknown-linux-gnu` | `nif` | `["wayland"]` | `wayland` |
| `aarch64-unknown-linux-gnu` | `nif` | `["wayland", "opengl"]` | `wayland_opengl` |
| `aarch64-unknown-linux-gnu` | `nif` | `["wayland", "vulkan"]` | `wayland_vulkan` |
| `aarch64-unknown-linux-gnu` | `nif` | `["wayland", "opengl", "vulkan"]` | `wayland_opengl_vulkan` |
| `aarch64-unknown-linux-gnu` | `nif` | `["drm", "wayland"]` | `drm_wayland` |
| `aarch64-unknown-linux-gnu` | `nif` | `["drm", "wayland", "opengl"]` | `drm_wayland_opengl` |
| `aarch64-unknown-linux-gnu` | `nif` | `["drm", "wayland", "vulkan"]` | `drm_wayland_vulkan` |
| `aarch64-unknown-linux-gnu` | `nif` | `["drm", "wayland", "opengl", "vulkan"]` | `drm_wayland_opengl_vulkan` |
| `armv7-unknown-linux-gnueabihf` | `nif` | `[]` | `baseline` |
| `armv7-unknown-linux-gnueabihf` | `nif` | `["opengl"]` | `opengl` |
| `armv7-unknown-linux-gnueabihf` | `nif` | `["drm"]` | `drm` |
| `armv7-unknown-linux-gnueabihf` | `nif` | `["drm", "opengl"]` | `drm_opengl` |
| `x86_64-unknown-linux-musl` | `nif` | `[]` | `baseline` |
| `x86_64-unknown-linux-musl` | `nif` | `["opengl"]` | `opengl` |
| `x86_64-unknown-linux-musl` | `nif` | `["drm"]` | `drm` |
| `x86_64-unknown-linux-musl` | `nif` | `["drm", "opengl"]` | `drm_opengl` |
| `riscv64gc-unknown-linux-gnu` | `nif` | `[]` | `baseline` |
| `riscv64gc-unknown-linux-gnu` | `nif` | `["opengl"]` | `opengl` |
| `riscv64gc-unknown-linux-gnu` | `nif` | `["drm"]` | `drm` |
| `riscv64gc-unknown-linux-gnu` | `nif` | `["drm", "opengl"]` | `drm_opengl` |
| `aarch64-apple-darwin` | `nif` | `[]` | `baseline` |
| `aarch64-apple-darwin` | `macos_host` | `["macos"]` | `macos_host` |
| `x86_64-apple-darwin` | `nif` | `[]` | `baseline` |
| `x86_64-apple-darwin` | `macos_host` | `["macos"]` | `macos_host` |
<!-- profiles:end -->

ARMv6 requires a source build. Nerves compiler names map to the Rust target/ABI:
ARMv7 uses hard-float; x86_64 Nerves uses musl rather than GNU; RISC-V uses
`riscv64gc`. An ARM board's marketing name does not identify its firmware ABI.

macOS window applications download `macos_host-v<VERSION>-<TARGET>.tar.gz` instead
of using an in-process AppKit NIF. Darwin baseline archives provide headless
raster on Apple Silicon/Intel; they are Mach-O files despite the `.so` extension.
No headless Metal archive is included in this work.

## Missing artifacts, source builds and toolchain

Unsupported feature sets/targets and missing checksums are reported **before**
an automatic native fallback. The message says that a potentially long Skia build
will occur and points to build-info. Missing optional Rustler is different from a
missing archive. A corrupt download/checksum failure must never silently switch
to a source build; repair the artifact/cache rather than bypassing integrity.

To request source compilation explicitly, consuming applications need:

```elixir
{:rustler, "~> 0.38.0", runtime: false}
```

```sh
EMERGE_SKIA_BUILD=true mix deps.compile emerge --force
# Nerves:
EMERGE_SKIA_BUILD=true MIX_TARGET=your_target mix deps.compile emerge --force
MIX_TARGET=your_target mix firmware
```

Source builds need Elixir 1.19+, Rust 1.91+, a C/C++ toolchain and target libraries
for the selected features. OpenGL builds need EGL/GBM; Wayland additionally needs
Wayland/xkbcommon; Vulkan needs its appropriate loader/driver at runtime. CPU KMS
requires atomic modesetting, compatible planes/dumb buffers and leasing. Libraries
and a successfully loaded NIF do not qualify physical GPU/presentation support.

All DRM profiles statically bundle libinput, libevdev, mtdev and libudev-zero,
plus hardware quirks. They do not require a udev daemon or deployment of external
input packages, but the target needs kernel input drivers, `/dev/input` and sysfs.
Source builds use pinned, hash-verified input archives: set
`EMERGE_INPUT_CACHE_DIR` to share them and `EMERGE_INPUT_OFFLINE=1` to require a
populated cache. The build host needs curl/tar and compression tools. See
[`native/emerge_input/README.md`](https://github.com/emerge-elixir/emerge/blob/main/native/emerge_input/README.md).

### Nerves cross-toolchain details

Emerge resolves the Rust target from Nerves' `CC`/target environment, configures
the SDK sysroot and embeds FreeType instead of requiring desktop font libraries.
Do not install a host-built NIF into firmware. Retain the same target environment
when recompiling after a feature or ABI change.

rust-skia runs Python on the build host. The default is `/usr/bin/python3`; if
needed, set `EMERGE_SKIA_HOST_PYTHON` to an absolute regular host executable:

```sh
EMERGE_SKIA_HOST_PYTHON=/opt/homebrew/bin/python3 MIX_TARGET=your_target mix firmware
```

Wrappers remove target `PYTHONHOME`, `PYTHONPATH` and `LD_LIBRARY_PATH` before
starting it. SDK values including `CC`, `CXX`, `CFLAGS`, `BINDGEN_EXTRA_CLANG_ARGS`,
`RUSTFLAGS` and `SKIA_GN_ARGS` remain build inputs; override only when diagnosing
a toolchain failure. Confirm `NERVES_SDK_SYSROOT` and `NERVES_TOOLCHAIN` belong
to the active firmware system, not another target or the host.

### Direct Cargo development

Cargo defaults remain Wayland/OpenGL. For explicit profiles disable defaults:

```sh
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features opengl
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features drm
```

Cargo cannot conditionally enable a dependency from the conjunction of two
features. The resolver adds `drm-vulkan-deps` or `wayland-vulkan-deps` for those
pairs; the native compatibility aliases `drm-vulkan`, `wayland-vulkan`,
`drm-all` and `wayland-all` include that glue. Those aliases are not archive names.
Use build-info to see the full Cargo feature list for a matrix. `drm` and
`wayland` alone no longer imply OpenGL.

## Wayland Vulkan validation

Only with permission to open a real window on a running compositor, select
`compiled_backends: [wayland: [:vulkan]]` and run:

```sh
elixir scripts/check-wayland-vulkan.exs --allow-hardware /tmp/wayland-vulkan-validation.log
```

Repeat with `[wayland: [:opengl, :vulkan]]` for the combined build. The runner
requires complete startup/teardown and synchronization-validation reports; missing
reports, warnings and quarantined resources fail. It is not an ordinary CI probe.
See the [frame-pacing validation contract](https://github.com/emerge-elixir/emerge/blob/main/guides/internals/frame-pacing.md#validation)
for scope and hardware qualification limits.

## Validate and publish

```sh
./ci-tests.sh
bash scripts/check-package.sh
elixir scripts/native-release-matrix.exs
```

Release jobs consume the catalog-generated matrix, check feature/dependency
isolation, inspect packaged ELF/Mach-O files and preserve stock-rootfs eager-load
checks and macOS raster smoke coverage. Source/package tests include standalone
build-info and checksum-only consumers without Rustler. Physical display/GPU
checks require explicit opt-in; do not probe CI hosts as part of resolution.

Native artifacts are published only after successful CI on the exact integrated
commit. Hex publication generates the matching checksum manifest from those
archives. Existing release hashes are never replaced. Changing version numbers,
passing unit tests or generating the matrix alone does not publish artifacts or
establish device support.

For macOS local host development use
`EMERGE_SKIA_MACOS_HOST_BUILD_LOCAL=true iex -S mix`. Mixed host/NIF development
can retain the explicit `load_macos_nif` opt-in; the default host-only path stays
unchanged. See [viewport configuration](https://hexdocs.pm/emerge/Emerge.html#module-viewport-configuration) for runtime
contracts, and [Nerves targets](https://hexdocs.pm/emerge/Emerge.html#module-viewport-configuration) for board/image selection.
