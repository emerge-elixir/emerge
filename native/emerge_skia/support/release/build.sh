#!/usr/bin/env bash
# Run inside the corresponding release container. Source is mounted read-only;
# build products and archives go outside the checkout.
set -euo pipefail

target="${1:?Rust target required}"
profile="${2:?raster or opengl required}"
version="${3:?version required}"
nif_version="${4:-2.15}"
target_args=(--target "$target")
output_subdir="$target/release"
case "$target" in
  x86_64-unknown-linux-musl)
    # Native musl must also build host tools with a dynamic CRT: bindgen's
    # build script dlopens libclang. An explicit --target would exclude those
    # host tools from RUSTFLAGS and make dlopen fail with "not supported".
    rustc -vV | grep -Fx "host: $target"
    unset CARGO_BUILD_TARGET
    target_args=()
    output_subdir=release
    export RUSTFLAGS="${RUSTFLAGS:-} -Ctarget-feature=-crt-static"
    ;;
  riscv64gc-unknown-linux-gnu) ;;
  *) echo "Unsupported embedded release target: $target" >&2; exit 1 ;;
esac
case "$profile" in
  raster) features=embedded-cpu; suffix="" ;;
  opengl) features=drm,embedded-cpu; suffix=--opengl ;;
  *) echo "Unsupported embedded release profile: $profile" >&2; exit 1 ;;
esac
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][A-Za-z0-9.+-]+)?$ ]]
[[ "$nif_version" == 2.15 ]]
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/build/target}"
export RUSTFLAGS="${RUSTFLAGS:-} -Lnative=$PWD/support/embedded-linux-link-stubs"
export FORCE_SKIA_BUILD=1
export SKIA_GN_ARGS="${SKIA_GN_ARGS:-} skia_use_fontconfig=false skia_use_system_freetype2=false"
export CLANGCC="${CC:-clang}"
export CLANGCXX="${CXX:-clang++}"
export CARGO_PROFILE_RELEASE_STRIP=symbols
cargo build --locked --release --lib "${target_args[@]}" --no-default-features --features "$features"

artifact="$CARGO_TARGET_DIR/$output_subdir/libemerge_skia.so"
bash support/release/verify.sh "$artifact" "$target" "$profile"
# Check eager dynamic dependency resolution before packaging. This is not a
# BEAM/renderer smoke, and QEMU is not physical MangoPi qualification.
loader="$CARGO_TARGET_DIR/load-nif"
case "$target" in
  x86_64-unknown-linux-musl)
    cc -Wall -Wextra -Werror support/release/load.c -o "$loader" -ldl
    "$loader" "$artifact"
    ;;
  riscv64gc-unknown-linux-gnu)
    riscv64-linux-gnu-gcc -Wall -Wextra -Werror support/release/load.c -o "$loader" -ldl
    qemu-riscv64 -L /usr/riscv64-linux-gnu "$loader" "$artifact"
    ;;
esac
name="libemerge_skia-v$version-nif-$nif_version-$target$suffix.so"
output="${ARTIFACT_DIR:-/artifacts}"
mkdir -p "$output"
cp "$artifact" "$output/$name"
tar -czf "$output/$name.tar.gz" -C "$output" "$name"
rm "$output/$name"
sha256sum "$output/$name.tar.gz"
