#!/usr/bin/env bash
set -euo pipefail
artifact="${1:?shared library required}"
target="${2:?Rust target required}"
profile="${3:?raster or opengl required}"
export LC_ALL=C
header="$(readelf --file-header "$artifact")"
deps="$(readelf --dynamic "$artifact")"
# GNU readelf handles both architectures; host nm need not support RISC-V.
symbols="$(readelf --dyn-syms --wide "$artifact")"
grep -Eq 'Class: +ELF64' <<<"$header"
grep -Eq 'Type: +DYN' <<<"$header"
case "$target" in
  x86_64-unknown-linux-musl)
    grep -q 'Machine:.*X86-64' <<<"$header"
    if grep -Eq 'GLIBC_[0-9]|NEEDED.*(libc\.so\.6|ld-linux)' <<<"$deps $symbols"; then
      echo 'musl artifact contains a GNU libc dependency' >&2; exit 1
    fi
    grep -Eq 'NEEDED.*libc\.(musl-x86_64\.so\.1|so)' <<<"$deps"
    ;;
  riscv64gc-unknown-linux-gnu)
    grep -q 'Machine:.*RISC-V' <<<"$header"
    grep -q 'Flags:.*double-float ABI' <<<"$header"
    grep -Eq 'NEEDED.*libc\.so\.6' <<<"$deps"
    ;;
  *) exit 1 ;;
esac
if grep -Eq 'NEEDED.*(fontconfig|freetype)' <<<"$deps" ||
   grep -Eq 'UND +(Fc|FT_)' <<<"$symbols"; then
  echo 'Embedded artifact requires system fontconfig/FreeType' >&2; exit 1
fi
case "$profile" in
  raster)
    if grep -Eq 'NEEDED.*(EGL|GLES|GL|gbm|drm|wayland)' <<<"$deps"; then
      echo 'Raster artifact requires a GPU/desktop library' >&2; exit 1
    fi ;;
  opengl) ;;
  *) exit 1 ;;
esac
# Verify that this is actually a loadable NIF, not just an arbitrary ELF file.
grep -Eq 'GLOBAL +DEFAULT +[0-9]+ +nif_init$' <<<"$symbols"
printf '%s\n%s\n' "$header" "$deps"
