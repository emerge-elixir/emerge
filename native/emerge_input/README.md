# Bundled DRM input

Linux-only, private Emerge dependency. Every DRM presenter enables this crate;
Wayland, macOS and headless-only builds do not.

`build.rs` downloads the pinned upstream archives in `sources.rs`, verifies their
SHA-256 hashes before extraction, and compiles their C sources with Cargo's target
compiler and PIC. Upstream source trees, headers and quirks are not checked into
this repository or included in the Hex source package. Only Emerge's integration,
checked patches, header generator and upstream license notices are retained here.

The build host needs `curl` (HTTPS) and `tar` with gzip, xz and bzip2 support, plus
the existing target C toolchain. No pkg-config, Meson or Python is required.
Rustler's Nerves environment supplies the compiler/sysroot as for the renderer.
The target still needs only libc/libm for this input bundle: downloads and source
extraction happen on the build host, never on a deployed device. Precompiled NIFs
include the input libraries and do not download anything at runtime.

## Download cache and offline builds

The default archive cache is `native/emerge_input/target/input-sources/`, outside
tracked/package files. Set `EMERGE_INPUT_CACHE_DIR` to an absolute directory to
share it across source checkouts, target profiles or CI jobs. Downloads are
published atomically only after verification. Cached archives are hash-checked
again whenever the build script runs; a corrupt archive fails the build rather
than being accepted or silently replaced.

Set `EMERGE_INPUT_OFFLINE=1` or `CARGO_NET_OFFLINE=true` to forbid downloads.
A missing archive then fails with its expected cache filename, source URL and
hash. Populate the cache with the exact four archives listed in `sources.rs`
before an air-gapped build; downloading from an internal mirror is fine provided
the bytes match the pinned hashes. Cargo's `--offline` flag alone does not expose
an offline setting to custom build scripts, so also set one of these variables.
Cargo's Rust dependency cache must already be populated for offline builds.

Verified archives are extracted into Cargo's `OUT_DIR` on each build-script run.
Patches and generated files are kept separately, leaving the upstream originals
unchanged. Cleaning Cargo's target directory removes the default download cache;
use an external `EMERGE_INPUT_CACHE_DIR` to retain it across clean builds.

## Runtime integration

The small C shim owns native event destruction and restricted device opens. The
Rust context is thread-local and owns one context reference plus explicit device
references. Restricted opens are read-only/nonblocking/close-on-exec; Emerge does
not expose device LED/output APIs, so write access is not a new prerequisite.
No Rust callback or panic crosses a C callback boundary. The NIF's
link/export policy keeps the bundled native symbols private.

Emerge uses a libinput **path context**, discovering `/dev/input/event*` itself.
libudev-zero derives classification from kernel/sysfs capabilities; no udev
service, `/run/udev` database, external hotplug helper or system libudev is needed.
Inotify prompts reconciliation; bounded rescans handle late nodes, permission
changes, directory recreation and missed notifications. Linux input drivers,
readable device nodes and `/sys` are still required.

Quirks from the pinned libinput archive are embedded as immutable data in the NIF.
A checked build-time patch feeds these bytes to upstream's FILE parser using
read-only fmemopen. Deployment/release relocation requires neither hard-coded
absolute paths nor extraction or environment changes. The upstream explicit
`LIBINPUT_QUIRKS_DIR` override remains available.

## Pinned sources

| Library | Version | License | Source archive SHA-256 |
|---|---|---|---|
| libinput | 1.29.2 | MIT | `02631a861fc6b9fb9ca6ae51cf863334e0699bf60c91c389ac420f3fa0053bdc` |
| libevdev | 1.13.4 | MIT | `f00ab8d42ad8b905296fab67e13b871f1a424839331516642100f82ad88127cd` |
| mtdev | 1.1.7 | MIT | `a107adad2101fecac54ac7f9f0e0a0dd155d954193da55c2340c97f2ff1d814e` |
| libudev-zero | 1.0.3 | ISC | `0bd89b657d62d019598e6c7ed726ff8fed80e8ba092a83b484d66afb80b77da5` |

Sources (the authoritative pins are in `sources.rs`):

- https://gitlab.freedesktop.org/libinput/libinput/-/archive/1.29.2/libinput-1.29.2.tar.gz
- https://www.freedesktop.org/software/libevdev/libevdev-1.13.4.tar.xz
- https://bitmath.org/code/mtdev/mtdev-1.1.7.tar.bz2
- https://codeload.github.com/illiliti/libudev-zero/tar.gz/refs/tags/1.0.3

Full upstream library license notices are retained under `licenses/`. Downloaded
Linux UAPI headers retain their original notices. `event_names.rs` generates
libevdev's lookup header from its pinned Linux headers; tests check byte-for-byte
parity with the original upstream generator's output.

Build-time patches fail if an upstream replacement no longer matches exactly once:

1. Read embedded quirks through the existing parser.
2. Recognize MT-only axes and respect INPUT_PROP_DIRECT instead of misclassifying
   direct touchscreens advertising BTN_TOOL_FINGER as touchpads.
3. Classify ABS capabilities independently of REL for composite devices.

The classifier fixture helper exercises the actual patched upstream function,
not a duplicate Rust classifier. Optional libwacom/Lua/GUI/tool dependencies are
not compiled. Tablet-specific and gesture-specific event APIs are not exposed by
the adapter; existing keyboard text translation remains Emerge-owned, not libinput.

## Validation

```sh
cd native/emerge_input
cargo test
cargo run --example probe
# After populating both source and Cargo dependency caches:
EMERGE_INPUT_OFFLINE=1 cargo test --offline
```

`probe` initializes a daemonless context and parses the compiled-in quirks without
needing physical hardware. It can be cross-built and copied to an ordinary Nerves
image. Inspect its ELF dependencies (and the NIF's): there must be no libinput,
libevdev, mtdev or libudev shared-library dependency. Test actual device discovery,
hotplug and typing on hardware separately; a context-only smoke test does not
qualify a touchscreen driver.
