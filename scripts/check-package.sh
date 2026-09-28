#!/usr/bin/env bash
# Validate the actual Hex source archive, not files available only in the checkout.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
package="$work/emerge"
cd "$root"
mix hex.build --unpack --output "$package"

# Reuse downloaded dependency sources, but compile into a fresh build directory.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/native/emerge_skia/target}"
cargo check --manifest-path "$package/native/emerge_skia/Cargo.toml" \
  --locked --all-targets --features bench-diagnostics
cargo check --manifest-path "$package/native/emerge_skia/Cargo.toml" \
  --locked --no-default-features --features embedded-cpu

cd "$package"
export MIX_ENV=dev
export MIX_DEPS_PATH="$root/deps"
export MIX_BUILD_PATH="$work/build"
export EMERGE_SKIA_BUILD=true
export EMERGE_INCLUDE_INTERNAL_DOCS=false
unset EMERGE_SKIA_CHECKSUM_ONLY
mix compile --warnings-as-errors
# Check before regeneration: missing assets can otherwise silently paint fallbacks.
mix docs.screenshots --check
mix docs --warnings-as-errors
elixir scripts/check-doc-links.exs doc
