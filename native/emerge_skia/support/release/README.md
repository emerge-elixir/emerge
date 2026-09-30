# Embedded release builds

## GNU DRM and RPi5

The main release matrix builds GNU x86_64/AArch64 `drm`, `drm_vulkan` and
`drm_all` archives with embedded FreeType and source-built Skia with fontconfig
disabled. `drm_all` contains DRM OpenGL+Vulkan, without the desktop Wayland
presenter. `scripts/check-embedded-artifact.exs` checks the packaged library's
architecture, NIF entry point, unresolved font/desktop symbols and SONAME allowlist.

Before publication, all three AArch64 archives are loaded with `load.c` in the
unmodified stock Nerves RPi5 **2.0.1** userspace, downloaded with a pinned SHA256.
Native ARM runners use `chroot`, so neither the builder's libraries nor a host
sysroot can conceal missing runtime libraries or GLIBC/GLIBCXX versions. No
extra libraries are installed into this rootfs. This is eager dynamic loading,
not a BEAM or hardware/GPU test; stock 2.0.1 supplies OpenGL ES, not Vulkan.

CI is reused by exact source SHA before any release builds start. These
artifact-specific checks remain separate from the ordinary test suite.

## musl and RISC-V containers

The release workflow builds two profiles for each target:

| Container | Rust target | Profiles |
|---|---|---|
| `Dockerfile.musl` | `x86_64-unknown-linux-musl` | `raster`, `opengl` |
| `Dockerfile.riscv64` | `riscv64gc-unknown-linux-gnu` | `raster`, `opengl` |

Both use Rust 1.91, the locked dependencies, and Skia built from source with
embedded FreeType and no system fontconfig. `opengl` enables DRM and headless
OpenGL; it is not a Vulkan or Wayland artifact. Raster has no GPU dependencies.

For example, from the repository root:

```sh
docker build -t emerge-musl -f native/emerge_skia/support/release/Dockerfile.musl \
  native/emerge_skia/support/release
mkdir -p /tmp/emerge-artifacts
docker run --rm \
  --mount "type=bind,source=$PWD,target=/source,readonly" \
  --mount "type=bind,source=/tmp/emerge-artifacts,target=/artifacts" \
  --env EMERGE_SOURCE_REVISION="$(git rev-parse HEAD)" \
  emerge-musl x86_64-unknown-linux-musl raster 0.4.1 2.15
```

Use an immutable patch identity instead of just HEAD for a dirty checkout.
`CARGO_TARGET_DIR` defaults to `/build/target`; the source mount stays read-only.
The musl build is native inside Alpine and omits Cargo's `--target` so the
`-crt-static` override also reaches host build scripts: bindgen must dlopen
libclang. The RISC-V container cross-compiles against GNU libraries with the
LP64D ABI, using Clang's `riscv64` spelling and Rust's `riscv64gc` spelling.

Before packaging, `verify.sh` checks ELF architecture, libc ABI, `nif_init`, and
absence of unwanted font/GPU dependencies. `load.c` checks eager dynamic
loading natively on musl and under QEMU on RISC-V. This does **not** initialize
the BEAM, render frames, or qualify a board's GPU/driver stack.

Archives follow RustlerPrecompiled's naming contract: raster has no variant
suffix; OpenGL has `--opengl`. The Hex release workflow downloads all published
archives and generates their checksums. Do not commit local build hashes as
release checksums or relabel GNU artifacts as musl.
