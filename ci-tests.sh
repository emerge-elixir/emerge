#!/usr/bin/env bash

set -euo pipefail

mode="${1:-all}"

run_rust_feature_checks() {
  cargo check --manifest-path native/emerge_skia/Cargo.toml --no-default-features
  cargo check --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features wayland
  cargo check --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features drm
  cargo check --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features wayland,drm
}

run_rust_feature_clippy() {
  cargo clippy --manifest-path native/emerge_skia/Cargo.toml --no-default-features -- -D warnings
  cargo clippy --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features wayland -- -D warnings
  cargo clippy --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features drm -- -D warnings
  cargo clippy --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features wayland,drm -- -D warnings
}

run_quality() {
  mix format --check-formatted
  mix credo --strict
  run_rust_feature_checks
  run_rust_feature_clippy
}

run_tests() {
  mix test
  cargo test --release --manifest-path native/emerge_skia/Cargo.toml
}

run_dialyzer() {
  local output_file
  output_file="$(mktemp)"

  if mix dialyzer >"${output_file}" 2>&1; then
    cat "${output_file}"
    rm -f "${output_file}"
    return 0
  fi

  cat "${output_file}"

  if grep -q "File not found:" "${output_file}"; then
    echo "Detected a stale Dialyzer PLT; rebuilding local PLTs and retrying..." >&2
    rm -f _build/dev/dialyxir_*.plt _build/dev/dialyxir_*.plt.hash
    rm -f "${output_file}"
    mix dialyzer
    return 0
  fi

  rm -f "${output_file}"
  return 1
}

case "$mode" in
  quality)
    run_quality
    ;;
  test)
    run_tests
    ;;
  dialyzer)
    run_dialyzer
    ;;
  all)
    run_quality
    run_tests
    run_dialyzer
    ;;
  *)
    echo "usage: ./ci-tests.sh [quality|test|dialyzer|all]" >&2
    exit 1
    ;;
esac
