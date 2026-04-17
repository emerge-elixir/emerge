#!/usr/bin/env bash

set -euo pipefail

mode="${1:-all}"

run_quality() {
  mix format --check-formatted
  mix credo --strict
  cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings
}

run_tests() {
  if [[ "${CI:-}" == "true" || "${CI:-}" == "1" ]]; then
    mix test
  else
    mix test --include full_sweep
  fi

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
